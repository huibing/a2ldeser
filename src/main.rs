use std::path::PathBuf;
use std::process;

use a2lfile::A2lObjectName;
use clap::{Parser, Subcommand};

use a2ldeser::compu_method;
use a2ldeser::extractor::{Extractor, PhysicalValue};
use a2ldeser::hex_reader::HexMemory;
use a2ldeser::resolver::{ResolvedCharacteristic, Resolver};
use a2ldeser::types::A2lValue;

/// A2L file deserializer — extract calibration values from ECU flash data.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extract a characteristic from a HEX file
    Extract {
        /// Path to the A2L file
        a2l: PathBuf,
        /// Path to the Intel HEX file
        hex: PathBuf,
        /// Characteristic name (or "list" to list all)
        name: String,
    },
    /// Decode raw hex bytes using A2L metadata (measurement or characteristic)
    Decode {
        /// Path to the A2L file
        a2l: PathBuf,
        /// Object name (measurement or characteristic)
        name: String,
        /// Raw bytes in hex, e.g. "0xffe6" or "ffe6" or "ff e6"
        raw: String,
    },
    /// List all objects in the A2L file
    List {
        /// Path to the A2L file
        a2l: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Extract { a2l, hex, name } => cmd_extract(&a2l, &hex, &name),
        Command::Decode { a2l, name, raw } => cmd_decode(&a2l, &name, &raw),
        Command::List { a2l } => cmd_list(&a2l),
    }
}

fn load_a2l(path: &PathBuf) -> a2lfile::A2lFile {
    let (a2l, _) = a2lfile::load(&path.as_os_str().to_os_string(), None, false)
        .unwrap_or_else(|e| {
            eprintln!("Error loading A2L file: {e}");
            process::exit(1);
        });
    a2l
}

/// Parse a hex string like "0xffe6", "ffe6", "ff e6", or "FF E6" into bytes.
fn parse_hex_bytes(s: &str) -> Vec<u8> {
    let s = s.trim().strip_prefix("0x").or_else(|| s.trim().strip_prefix("0X")).unwrap_or(s.trim());
    let hex: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if hex.len() % 2 != 0 {
        eprintln!("Error: hex string must have even number of digits, got '{hex}'");
        process::exit(1);
    }
    hex.as_bytes()
        .chunks(2)
        .map(|chunk| {
            let s = std::str::from_utf8(chunk).unwrap();
            u8::from_str_radix(s, 16).unwrap_or_else(|_| {
                eprintln!("Error: invalid hex byte '{s}'");
                process::exit(1);
            })
        })
        .collect()
}

fn cmd_extract(a2l_path: &PathBuf, hex_path: &PathBuf, name: &str) {
    let a2l = load_a2l(a2l_path);
    let module = &a2l.project.module[0];

    if name == "list" {
        for ch in module.characteristic.iter() {
            println!("{}\t{:?}", ch.get_name(), ch.characteristic_type);
        }
        return;
    }

    let hex = HexMemory::from_file(hex_path).unwrap_or_else(|e| {
        eprintln!("Error loading HEX file: {e}");
        process::exit(1);
    });

    let ext = Extractor::new(module, &hex);

    if let Ok(val) = ext.extract_value(name) {
        println!("{}: {} {} (raw: {:?})", val.name, fmt_phys(&val.physical), val.unit, val.raw);
        return;
    }
    if let Ok(curve) = ext.extract_curve(name) {
        println!("{} (CURVE, {} points, unit: {})", curve.name, curve.x_axis.len(), curve.unit);
        println!("  X ({}): {:?}", curve.x_unit, curve.x_axis);
        println!("  Y: {:?}", curve.values.iter().map(fmt_phys).collect::<Vec<_>>());
        return;
    }
    if let Ok(map) = ext.extract_map(name) {
        println!("{} (MAP, {}x{}, unit: {})", map.name, map.x_axis.len(), map.y_axis.len(), map.unit);
        println!("  X ({}): {:?}", map.x_unit, map.x_axis);
        println!("  Y ({}): {:?}", map.y_unit, map.y_axis);
        for (i, row) in map.values.iter().enumerate() {
            let vals: Vec<String> = row.iter().map(fmt_phys).collect();
            println!("  [y={:.4}] {}", map.y_axis[i], vals.join(", "));
        }
        return;
    }
    if let Ok(vb) = ext.extract_val_blk(name) {
        let vals: Vec<String> = vb.values.iter().map(fmt_phys).collect();
        println!("{} (VAL_BLK, {} elements, unit: {})", vb.name, vb.values.len(), vb.unit);
        println!("  [{}]", vals.join(", "));
        return;
    }
    if let Ok(ascii) = ext.extract_ascii(name) {
        println!("{} (ASCII): \"{}\"", ascii.name, ascii.text);
        return;
    }

    match ext.extract_value(name) {
        Err(e) => {
            eprintln!("Error extracting '{name}': {e}");
            process::exit(1);
        }
        _ => unreachable!(),
    }
}

fn cmd_decode(a2l_path: &PathBuf, name: &str, raw_hex: &str) {
    let a2l = load_a2l(a2l_path);
    let module = &a2l.project.module[0];
    let resolver = Resolver::new(module);
    let bytes = parse_hex_bytes(raw_hex);

    // Try as measurement first
    if let Ok(meas) = resolver.resolve_measurement(name) {
        let raw = A2lValue::from_bytes(&meas.datatype, &bytes).unwrap_or_else(|| {
            eprintln!(
                "Error: need {} bytes for {:?}, got {}",
                A2lValue::datatype_size(&meas.datatype),
                meas.datatype,
                bytes.len()
            );
            process::exit(1);
        });

        let physical = decode_value(&raw, &meas.conversion, module);
        println!("{} (MEASUREMENT, {:?}): raw={:?} → {} {}",
            meas.name, meas.datatype, raw, fmt_phys(&physical), meas.unit);
        return;
    }

    // Try as characteristic
    if let Ok(resolved) = resolver.resolve_characteristic(name) {
        // Get data type from the resolved characteristic
        let (datatype, conversion, unit) = match &resolved {
            ResolvedCharacteristic::Value(v) => {
                let dt = v.layout.fnc_values_datatype.as_ref().unwrap_or_else(|| {
                    eprintln!("Error: characteristic '{name}' has no fnc_values data type");
                    process::exit(1);
                });
                (dt.clone(), v.conversion.clone(), v.unit.clone())
            }
            ResolvedCharacteristic::ValBlk(vb) => {
                let dt = vb.layout.fnc_values_datatype.as_ref().unwrap_or_else(|| {
                    eprintln!("Error: characteristic '{name}' has no fnc_values data type");
                    process::exit(1);
                });
                (dt.clone(), vb.conversion.clone(), vb.unit.clone())
            }
            ResolvedCharacteristic::Curve(c) => {
                let dt = c.layout.fnc_values_datatype.as_ref().unwrap_or_else(|| {
                    eprintln!("Error: characteristic '{name}' has no fnc_values data type");
                    process::exit(1);
                });
                (dt.clone(), c.conversion.clone(), c.unit.clone())
            }
            ResolvedCharacteristic::Map(m) => {
                let dt = m.layout.fnc_values_datatype.as_ref().unwrap_or_else(|| {
                    eprintln!("Error: characteristic '{name}' has no fnc_values data type");
                    process::exit(1);
                });
                (dt.clone(), m.conversion.clone(), m.unit.clone())
            }
            ResolvedCharacteristic::Ascii(_) => {
                // For ASCII, just display the bytes as a string
                let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
                let text = String::from_utf8_lossy(&bytes[..end]);
                println!("{name} (ASCII): \"{text}\"");
                return;
            }
        };

        let raw = A2lValue::from_bytes(&datatype, &bytes).unwrap_or_else(|| {
            eprintln!(
                "Error: need {} bytes for {:?}, got {}",
                A2lValue::datatype_size(&datatype),
                datatype,
                bytes.len()
            );
            process::exit(1);
        });

        let physical = decode_value(&raw, &conversion, module);
        println!("{name} (CHARACTERISTIC, {:?}): raw={:?} → {} {}",
            datatype, raw, fmt_phys(&physical), unit);
        return;
    }

    eprintln!("Error: '{name}' not found as measurement or characteristic");
    process::exit(1);
}

fn cmd_list(a2l_path: &PathBuf) {
    let a2l = load_a2l(a2l_path);
    let module = &a2l.project.module[0];

    println!("=== Characteristics ({}) ===", module.characteristic.len());
    for ch in module.characteristic.iter() {
        println!("  {}\t{:?}", ch.get_name(), ch.characteristic_type);
    }

    println!("\n=== Measurements ({}) ===", module.measurement.len());
    for meas in module.measurement.iter() {
        println!("  {}\t{:?}", meas.get_name(), meas.datatype);
    }
}

fn decode_value(raw: &A2lValue, conversion: &str, module: &a2lfile::Module) -> PhysicalValue {
    // Try verbal first
    if let Ok(Some(label)) = compu_method::convert_raw_to_string(raw, conversion, module) {
        return PhysicalValue::Verbal(label);
    }
    // Numeric
    match compu_method::convert_raw_to_physical(raw, conversion, module) {
        Ok(v) => PhysicalValue::Numeric(v),
        Err(e) => {
            eprintln!("Warning: conversion failed: {e}");
            PhysicalValue::Numeric(raw.as_f64().unwrap_or(f64::NAN))
        }
    }
}

fn fmt_phys(v: &PhysicalValue) -> String {
    match v {
        PhysicalValue::Numeric(n) => format!("{n}"),
        PhysicalValue::Verbal(s) => format!("\"{s}\""),
    }
}
