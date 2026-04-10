use std::path::{Path, PathBuf};
use std::process;

use a2lfile::A2lObjectName;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

use a2ldeser::compu_method;
use a2ldeser::extractor::{ExtractedObject, Extractor, PhysicalValue};
use a2ldeser::hex_reader::HexMemory;
use a2ldeser::resolver::{ResolvedCharacteristic, Resolver};
use a2ldeser::types::A2lValue;

/// A2L file deserializer — extract calibration values from ECU flash data.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Text, global = true)]
    format: Format,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
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
    /// Extract all characteristics and report success/failure summary
    Summary {
        /// Path to the A2L file
        a2l: PathBuf,
        /// Path to the Intel HEX file
        hex: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    let fmt = cli.format;

    match cli.command {
        Command::Extract { a2l, hex, name } => cmd_extract(&a2l, &hex, &name, fmt),
        Command::Decode { a2l, name, raw } => cmd_decode(&a2l, &name, &raw, fmt),
        Command::List { a2l } => cmd_list(&a2l, fmt),
        Command::Summary { a2l, hex } => cmd_summary(&a2l, &hex, fmt),
    }
}

fn load_a2l(path: &Path) -> a2lfile::A2lFile {
    let (a2l, _) = a2lfile::load(path.as_os_str(), None, false)
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
    if !hex.len().is_multiple_of(2) {
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

// ========================================================================
// extract subcommand
// ========================================================================

fn cmd_extract(a2l_path: &Path, hex_path: &Path, name: &str, fmt: Format) {
    let a2l = load_a2l(a2l_path);
    let module = &a2l.project.module[0];

    if name == "list" {
        let items: Vec<_> = module.characteristic.iter()
            .map(|ch| (ch.get_name().to_string(), format!("{:?}", ch.characteristic_type)))
            .collect();
        match fmt {
            Format::Text => {
                for (n, t) in &items {
                    println!("{n}\t{t}");
                }
            }
            Format::Json => {
                let arr: Vec<_> = items.iter()
                    .map(|(n, t)| json!({"name": n, "type": t}))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr).unwrap());
            }
        }
        return;
    }

    let hex = HexMemory::from_file(hex_path).unwrap_or_else(|e| {
        eprintln!("Error loading HEX file: {e}");
        process::exit(1);
    });

    let ext = Extractor::new(module, &hex);

    match ext.extract_any(name) {
        Ok(obj) => output_extracted(&obj, fmt),
        Err(e) => {
            eprintln!("Error extracting '{name}': {e}");
            process::exit(1);
        }
    }
}

// ========================================================================
// decode subcommand
// ========================================================================

fn cmd_decode(a2l_path: &Path, name: &str, raw_hex: &str, fmt: Format) {
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
        match fmt {
            Format::Text => {
                println!("{} (MEASUREMENT, {:?}): raw={:?} → {} {}",
                    meas.name, meas.datatype, raw, fmt_phys(&physical), meas.unit);
            }
            Format::Json => {
                println!("{}", serde_json::to_string_pretty(&json!({
                    "name": meas.name,
                    "kind": "MEASUREMENT",
                    "datatype": format!("{:?}", meas.datatype),
                    "raw": raw,
                    "physical": physical,
                    "unit": meas.unit,
                })).unwrap());
            }
        }
        return;
    }

    // Try as characteristic
    if let Ok(resolved) = resolver.resolve_characteristic(name) {
        let (datatype, conversion, unit) = match &resolved {
            ResolvedCharacteristic::Value(v) => {
                let dt = v.layout.fnc_values_datatype.as_ref().unwrap_or_else(|| {
                    eprintln!("Error: characteristic '{name}' has no fnc_values data type");
                    process::exit(1);
                });
                (*dt, v.conversion.clone(), v.unit.clone())
            }
            ResolvedCharacteristic::ValBlk(vb) => {
                let dt = vb.layout.fnc_values_datatype.as_ref().unwrap_or_else(|| {
                    eprintln!("Error: characteristic '{name}' has no fnc_values data type");
                    process::exit(1);
                });
                (*dt, vb.conversion.clone(), vb.unit.clone())
            }
            ResolvedCharacteristic::Curve(c) => {
                let dt = c.layout.fnc_values_datatype.as_ref().unwrap_or_else(|| {
                    eprintln!("Error: characteristic '{name}' has no fnc_values data type");
                    process::exit(1);
                });
                (*dt, c.conversion.clone(), c.unit.clone())
            }
            ResolvedCharacteristic::Map(m) => {
                let dt = m.layout.fnc_values_datatype.as_ref().unwrap_or_else(|| {
                    eprintln!("Error: characteristic '{name}' has no fnc_values data type");
                    process::exit(1);
                });
                (*dt, m.conversion.clone(), m.unit.clone())
            }
            ResolvedCharacteristic::Ascii(_) => {
                let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
                let text = String::from_utf8_lossy(&bytes[..end]).to_string();
                match fmt {
                    Format::Text => println!("{name} (ASCII): \"{text}\""),
                    Format::Json => {
                        println!("{}", serde_json::to_string_pretty(&json!({
                            "name": name,
                            "kind": "ASCII",
                            "text": text,
                        })).unwrap());
                    }
                }
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
        match fmt {
            Format::Text => {
                println!("{name} (CHARACTERISTIC, {:?}): raw={:?} → {} {}",
                    datatype, raw, fmt_phys(&physical), unit);
            }
            Format::Json => {
                println!("{}", serde_json::to_string_pretty(&json!({
                    "name": name,
                    "kind": "CHARACTERISTIC",
                    "datatype": format!("{:?}", datatype),
                    "raw": raw,
                    "physical": physical,
                    "unit": unit,
                })).unwrap());
            }
        }
        return;
    }

    eprintln!("Error: '{name}' not found as measurement or characteristic");
    process::exit(1);
}

// ========================================================================
// list subcommand
// ========================================================================

fn cmd_list(a2l_path: &Path, fmt: Format) {
    let a2l = load_a2l(a2l_path);
    let module = &a2l.project.module[0];

    match fmt {
        Format::Text => {
            println!("=== Characteristics ({}) ===", module.characteristic.len());
            for ch in module.characteristic.iter() {
                println!("  {}\t{:?}", ch.get_name(), ch.characteristic_type);
            }
            println!("\n=== Measurements ({}) ===", module.measurement.len());
            for meas in module.measurement.iter() {
                println!("  {}\t{:?}", meas.get_name(), meas.datatype);
            }
        }
        Format::Json => {
            let chars: Vec<_> = module.characteristic.iter()
                .map(|ch| json!({
                    "name": ch.get_name(),
                    "type": format!("{:?}", ch.characteristic_type),
                }))
                .collect();
            let meas: Vec<_> = module.measurement.iter()
                .map(|m| json!({
                    "name": m.get_name(),
                    "datatype": format!("{:?}", m.datatype),
                }))
                .collect();
            println!("{}", serde_json::to_string_pretty(&json!({
                "characteristics": chars,
                "measurements": meas,
            })).unwrap());
        }
    }
}

// ========================================================================
// summary subcommand
// ========================================================================

fn cmd_summary(a2l_path: &Path, hex_path: &Path, fmt: Format) {
    let a2l = load_a2l(a2l_path);
    let module = &a2l.project.module[0];

    let hex = HexMemory::from_file(hex_path).unwrap_or_else(|e| {
        eprintln!("Error loading HEX file: {e}");
        process::exit(1);
    });

    let ext = Extractor::new(module, &hex);
    let report = ext.extract_all();

    match fmt {
        Format::Text => report.print_summary(),
        Format::Json => {
            let success_counts = report.success_counts();
            let failure_counts = report.failure_counts();
            let failures: Vec<_> = report.failures.iter()
                .map(|f| json!({
                    "name": f.name,
                    "type": f.type_label,
                    "error": f.error.to_string(),
                }))
                .collect();
            println!("{}", serde_json::to_string_pretty(&json!({
                "total": report.total(),
                "succeeded": report.successes.len(),
                "failed": report.failures.len(),
                "success_counts": success_counts,
                "failure_counts": failure_counts,
                "failures": failures,
            })).unwrap());
        }
    }
}

// ========================================================================
// helpers
// ========================================================================

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

fn output_extracted(obj: &ExtractedObject, fmt: Format) {
    match fmt {
        Format::Json => {
            println!("{}", serde_json::to_string_pretty(obj).unwrap());
        }
        Format::Text => print_extracted_text(obj),
    }
}

fn print_extracted_text(obj: &ExtractedObject) {
    match obj {
        ExtractedObject::Value(val) => {
            println!("{}: {} {} (raw: {:?})", val.name, fmt_phys(&val.physical), val.unit, val.raw);
        }
        ExtractedObject::Curve(curve) => {
            println!("{} (CURVE, {} points, unit: {})", curve.name, curve.x_axis.len(), curve.unit);
            println!("  X ({}): {:?}", curve.x_unit, curve.x_axis);
            println!("  Y: {:?}", curve.values.iter().map(fmt_phys).collect::<Vec<_>>());
        }
        ExtractedObject::Map(map) => {
            println!("{} (MAP, {}x{}, unit: {})", map.name, map.x_axis.len(), map.y_axis.len(), map.unit);
            println!("  X ({}): {:?}", map.x_unit, map.x_axis);
            println!("  Y ({}): {:?}", map.y_unit, map.y_axis);
            for (i, row) in map.values.iter().enumerate() {
                let vals: Vec<String> = row.iter().map(fmt_phys).collect();
                println!("  [y={:.4}] {}", map.y_axis[i], vals.join(", "));
            }
        }
        ExtractedObject::ValBlk(vb) => {
            let vals: Vec<String> = vb.values.iter().map(fmt_phys).collect();
            println!("{} (VAL_BLK, {} elements, unit: {})", vb.name, vb.values.len(), vb.unit);
            println!("  [{}]", vals.join(", "));
        }
        ExtractedObject::Ascii(ascii) => {
            println!("{} (ASCII): \"{}\"", ascii.name, ascii.text);
        }
    }
}
