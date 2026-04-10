//! Example: Explore A2L file structure.
//!
//! Lists all characteristics and measurements in an A2L file,
//! grouped by type with summary statistics.
//!
//! Usage:
//!   cargo run --example list_objects -- <A2L_FILE>

use a2lfile::{A2lObjectName, CharacteristicType};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <A2L_FILE>", args[0]);
        std::process::exit(1);
    }

    let (a2l, _) =
        a2lfile::load(&std::ffi::OsString::from(&args[1]), None, false).expect("Failed to load A2L");
    let module = &a2l.project.module[0];

    // --- Characteristics summary ---
    let chars: Vec<_> = module.characteristic.iter().collect();
    let values = chars.iter().filter(|c| c.characteristic_type == CharacteristicType::Value).count();
    let curves = chars.iter().filter(|c| c.characteristic_type == CharacteristicType::Curve).count();
    let maps = chars.iter().filter(|c| c.characteristic_type == CharacteristicType::Map).count();
    let val_blks = chars.iter().filter(|c| c.characteristic_type == CharacteristicType::ValBlk).count();
    let asciis = chars.iter().filter(|c| c.characteristic_type == CharacteristicType::Ascii).count();

    println!("=== Characteristics ({} total) ===", chars.len());
    println!("  VALUE:   {values}");
    println!("  CURVE:   {curves}");
    println!("  MAP:     {maps}");
    println!("  VAL_BLK: {val_blks}");
    println!("  ASCII:   {asciis}");
    println!();

    // --- Measurements summary ---
    println!("=== Measurements ({} total) ===", module.measurement.len());
    println!();

    // --- COMPU_METHODs ---
    println!("=== COMPU_METHODs ({}) ===", module.compu_method.len());
    for cm in module.compu_method.iter() {
        println!("  {} ({:?})", cm.get_name(), cm.conversion_type);
    }
    println!();

    // --- Record Layouts ---
    println!("=== Record Layouts ({}) ===", module.record_layout.len());
    for rl in module.record_layout.iter() {
        let has_fnc = rl.fnc_values.is_some();
        let has_x = rl.axis_pts_x.is_some();
        let has_y = rl.axis_pts_y.is_some();
        println!(
            "  {} (fnc_values: {}, axis_x: {}, axis_y: {})",
            rl.get_name(), has_fnc, has_x, has_y
        );
    }
    println!();

    // --- Axis Points ---
    println!("=== Axis Points ({}) ===", module.axis_pts.len());
    for ap in module.axis_pts.iter() {
        println!(
            "  {} @ 0x{:08X} (max_axis_points: {})",
            ap.get_name(),
            ap.address,
            ap.max_axis_points
        );
    }
    println!();

    // --- Print first 10 characteristics of each type as samples ---
    println!("=== Sample Characteristics ===");
    for ctype in &[
        CharacteristicType::Value,
        CharacteristicType::Curve,
        CharacteristicType::Map,
        CharacteristicType::ValBlk,
        CharacteristicType::Ascii,
    ] {
        let samples: Vec<_> = chars
            .iter()
            .filter(|c| &c.characteristic_type == ctype)
            .take(5)
            .collect();

        if !samples.is_empty() {
            println!("  {:?}:", ctype);
            for c in samples {
                println!(
                    "    {} @ 0x{:08X} (deposit: {}, conversion: {})",
                    c.get_name(),
                    c.address,
                    &c.deposit,
                    &c.conversion
                );
            }
        }
    }
}
