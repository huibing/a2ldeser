//! Example: Extract calibration values from A2L + HEX files.
//!
//! Demonstrates the full pipeline: load A2L metadata, load HEX binary,
//! and extract typed values (scalar, curve, map, array, string).
//!
//! Usage:
//!   cargo run --example extract_characteristic -- <A2L_FILE> <HEX_FILE>

use std::path::Path;

use a2ldeser::extractor::{Extractor, PhysicalValue};
use a2ldeser::hex_reader::HexMemory;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <A2L_FILE> <HEX_FILE>", args[0]);
        std::process::exit(1);
    }

    // Step 1: Load the A2L file (metadata)
    let (a2l, _warnings) =
        a2lfile::load(&std::ffi::OsString::from(&args[1]), None, false).expect("Failed to load A2L");
    let module = &a2l.project.module[0];

    // Step 2: Load the Intel HEX file (binary flash data)
    let hex = HexMemory::from_file(Path::new(&args[2])).expect("Failed to load HEX");

    // Step 3: Create the extractor (combines resolver + hex reader + conversion)
    let ext = Extractor::new(module, &hex);

    // --- Extract a scalar VALUE ---
    // A VALUE is a single calibration parameter (e.g., an enable flag, a threshold).
    match ext.extract_value("g_xcp_enable_status") {
        Ok(val) => {
            println!("Scalar VALUE: {}", val.name);
            println!("  Raw:      {:?}", val.raw);
            println!("  Physical: {}", fmt_phys(&val.physical));
            println!("  Unit:     {}", val.unit);
        }
        Err(e) => eprintln!("  Error: {e}"),
    }
    println!();

    // --- Extract a CURVE (1D lookup table) ---
    // A CURVE maps one axis (e.g., RPM) to output values (e.g., torque).
    // Pick the first curve from the A2L file as an example.
    let first_curve = module
        .characteristic
        .iter()
        .find(|c| c.characteristic_type == a2lfile::CharacteristicType::Curve);

    if let Some(c) = first_curve {
        let name = a2lfile::A2lObjectName::get_name(c);
        match ext.extract_curve(name) {
            Ok(curve) => {
                println!("CURVE: {} ({} points)", curve.name, curve.x_axis.len());
                println!("  X axis ({}): {:?}", curve.x_unit, &curve.x_axis[..curve.x_axis.len().min(5)]);
                let display: Vec<String> = curve.values.iter().take(5).map(fmt_phys).collect();
                println!("  Values ({}): [{}]", curve.unit, display.join(", "));
                if curve.x_axis.len() > 5 {
                    println!("  ... ({} more points)", curve.x_axis.len() - 5);
                }
            }
            Err(e) => eprintln!("  Error extracting curve '{}': {e}", name),
        }
        println!();
    }

    // --- Extract a MAP (2D lookup table) ---
    // A MAP maps two axes (e.g., RPM × load) to output values.
    let first_map = module
        .characteristic
        .iter()
        .find(|c| c.characteristic_type == a2lfile::CharacteristicType::Map);

    if let Some(m) = first_map {
        let name = a2lfile::A2lObjectName::get_name(m);
        match ext.extract_map(name) {
            Ok(map) => {
                println!("MAP: {} ({}x{}, unit: {})", map.name, map.x_axis.len(), map.y_axis.len(), map.unit);
                println!("  X axis ({}): {:?}", map.x_unit, &map.x_axis[..map.x_axis.len().min(5)]);
                println!("  Y axis ({}): {:?}", map.y_unit, &map.y_axis[..map.y_axis.len().min(5)]);
                // Print first row
                if let Some(row) = map.values.first() {
                    let vals: Vec<String> = row.iter().take(5).map(fmt_phys).collect();
                    println!("  Row 0: [{}]", vals.join(", "));
                }
            }
            Err(e) => eprintln!("  Error extracting map '{}': {e}", name),
        }
        println!();
    }

    // --- Extract a VAL_BLK (1D array) ---
    let first_vb = module
        .characteristic
        .iter()
        .find(|c| c.characteristic_type == a2lfile::CharacteristicType::ValBlk);

    if let Some(vb) = first_vb {
        let name = a2lfile::A2lObjectName::get_name(vb);
        match ext.extract_val_blk(name) {
            Ok(blk) => {
                let vals: Vec<String> = blk.values.iter().take(8).map(fmt_phys).collect();
                println!("VAL_BLK: {} ({} elements, unit: {})", blk.name, blk.values.len(), blk.unit);
                println!("  [{}]", vals.join(", "));
            }
            Err(e) => eprintln!("  Error extracting val_blk '{}': {e}", name),
        }
        println!();
    }

    // --- Extract an ASCII string ---
    let first_ascii = module
        .characteristic
        .iter()
        .find(|c| c.characteristic_type == a2lfile::CharacteristicType::Ascii);

    if let Some(a) = first_ascii {
        let name = a2lfile::A2lObjectName::get_name(a);
        match ext.extract_ascii(name) {
            Ok(ascii) => println!("ASCII: {} = \"{}\"", ascii.name, ascii.text),
            Err(e) => eprintln!("  Error extracting ascii '{}': {e}", name),
        }
    }
}

fn fmt_phys(v: &PhysicalValue) -> String {
    match v {
        PhysicalValue::Numeric(n) => format!("{n}"),
        PhysicalValue::Verbal(s) => format!("\"{s}\""),
    }
}
