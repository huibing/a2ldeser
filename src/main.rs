use std::path::PathBuf;
use std::process;

use a2lfile::A2lObjectName;
use clap::Parser;

use a2ldeser::extractor::{Extractor, PhysicalValue};
use a2ldeser::hex_reader::HexMemory;

/// A2L file deserializer — extract calibration values from ECU flash data.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Path to the A2L file
    a2l: PathBuf,

    /// Path to the Intel HEX file
    hex: PathBuf,

    /// Name of the characteristic to extract (or "list" to list all)
    name: String,
}

fn main() {
    let cli = Cli::parse();

    let (a2l, _) = a2lfile::load(&cli.a2l.as_os_str().to_os_string(), None, false)
        .unwrap_or_else(|e| {
            eprintln!("Error loading A2L file: {e}");
            process::exit(1);
        });
    let module = &a2l.project.module[0];

    if cli.name == "list" {
        for ch in module.characteristic.iter() {
            println!("{}\t{:?}", ch.get_name(), ch.characteristic_type);
        }
        return;
    }

    let hex = HexMemory::from_file(&cli.hex).unwrap_or_else(|e| {
        eprintln!("Error loading HEX file: {e}");
        process::exit(1);
    });

    let ext = Extractor::new(module, &hex);

    // Try each extraction method in turn
    if let Ok(val) = ext.extract_value(&cli.name) {
        println!("{}: {} {} (raw: {:?})", val.name, fmt_phys(&val.physical), val.unit, val.raw);
        return;
    }

    if let Ok(curve) = ext.extract_curve(&cli.name) {
        println!("{} (CURVE, {} points, unit: {})", curve.name, curve.x_axis.len(), curve.unit);
        println!("  X ({}): {:?}", curve.x_unit, curve.x_axis);
        println!("  Y: {:?}", curve.values.iter().map(fmt_phys).collect::<Vec<_>>());
        return;
    }

    if let Ok(map) = ext.extract_map(&cli.name) {
        println!("{} (MAP, {}x{}, unit: {})", map.name, map.x_axis.len(), map.y_axis.len(), map.unit);
        println!("  X ({}): {:?}", map.x_unit, map.x_axis);
        println!("  Y ({}): {:?}", map.y_unit, map.y_axis);
        for (i, row) in map.values.iter().enumerate() {
            let vals: Vec<String> = row.iter().map(fmt_phys).collect();
            println!("  [y={:.4}] {}", map.y_axis[i], vals.join(", "));
        }
        return;
    }

    if let Ok(vb) = ext.extract_val_blk(&cli.name) {
        let vals: Vec<String> = vb.values.iter().map(fmt_phys).collect();
        println!("{} (VAL_BLK, {} elements, unit: {})", vb.name, vb.values.len(), vb.unit);
        println!("  [{}]", vals.join(", "));
        return;
    }

    if let Ok(ascii) = ext.extract_ascii(&cli.name) {
        println!("{} (ASCII): \"{}\"", ascii.name, ascii.text);
        return;
    }

    // If nothing worked, show the last error
    match ext.extract_value(&cli.name) {
        Err(e) => {
            eprintln!("Error extracting '{}': {e}", cli.name);
            process::exit(1);
        }
        _ => unreachable!(),
    }
}

fn fmt_phys(v: &PhysicalValue) -> String {
    match v {
        PhysicalValue::Numeric(n) => format!("{n}"),
        PhysicalValue::Verbal(s) => format!("\"{s}\""),
    }
}
