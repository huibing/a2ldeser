//! Example: Decode raw bytes using A2L metadata.
//!
//! Given a measurement or characteristic name and raw hex bytes (e.g., from
//! an XCP/CCP trace or manual memory dump), decode the value using the
//! A2L type and COMPU_METHOD definitions.
//!
//! Usage:
//!   cargo run --example decode_bytes -- <A2L_FILE> <NAME> <HEX_BYTES>
//!
//! Example:
//!   cargo run --example decode_bytes -- my.a2l engine_speed "0x1027"

use a2ldeser::compu_method;
use a2ldeser::resolver::Resolver;
use a2ldeser::types::A2lValue;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: {} <A2L_FILE> <NAME> <HEX_BYTES>", args[0]);
        eprintln!("  HEX_BYTES: \"0xffe6\", \"ffe6\", or \"ff e6\"");
        std::process::exit(1);
    }

    // Load A2L
    let (a2l, _) =
        a2lfile::load(&std::ffi::OsString::from(&args[1]), None, false).expect("Failed to load A2L");
    let module = &a2l.project.module[0];
    let resolver = Resolver::new(module);

    let name = &args[2];
    let bytes = parse_hex_bytes(&args[3]);

    // Try as measurement first (most common for raw-byte decoding)
    if let Ok(meas) = resolver.resolve_measurement(name) {
        let expected_size = A2lValue::datatype_size(&meas.datatype);
        if bytes.len() != expected_size {
            eprintln!(
                "Error: {:?} expects {} bytes, got {}",
                meas.datatype, expected_size, bytes.len()
            );
            std::process::exit(1);
        }

        let raw = A2lValue::from_bytes(&meas.datatype, &bytes).expect("Failed to parse bytes");
        println!("Object:     {} (MEASUREMENT)", meas.name);
        println!("DataType:   {:?}", meas.datatype);
        println!("Raw value:  {:?}", raw);

        // Apply COMPU_METHOD conversion
        if meas.conversion != "NO_COMPU_METHOD" {
            // Try verbal (TAB_VERB) first
            if let Ok(Some(label)) = compu_method::convert_raw_to_string(&raw, &meas.conversion, module) {
                println!("Physical:   \"{}\"", label);
            } else if let Ok(phys) = compu_method::convert_raw_to_physical(&raw, &meas.conversion, module) {
                println!("Physical:   {} {}", phys, meas.unit);
            } else {
                println!("Physical:   (conversion failed)");
            }
        } else {
            println!("Physical:   {} {} (no conversion)", raw.as_f64().unwrap_or(f64::NAN), meas.unit);
        }
        return;
    }

    // Try as characteristic
    if let Ok(resolved) = resolver.resolve_characteristic(name) {
        use a2ldeser::resolver::ResolvedCharacteristic;
        let (datatype, conversion, unit) = match &resolved {
            ResolvedCharacteristic::Value(v) => (
                v.layout.fnc_values_datatype.as_ref().expect("no fnc_values").clone(),
                v.conversion.clone(),
                v.unit.clone(),
            ),
            ResolvedCharacteristic::ValBlk(vb) => (
                vb.layout.fnc_values_datatype.as_ref().expect("no fnc_values").clone(),
                vb.conversion.clone(),
                vb.unit.clone(),
            ),
            ResolvedCharacteristic::Ascii(_) => {
                let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
                let text = String::from_utf8_lossy(&bytes[..end]);
                println!("Object:  {} (ASCII)", name);
                println!("Value:   \"{}\"", text);
                return;
            }
            other => {
                eprintln!("Decode for {:?} not supported — use extract instead", std::mem::discriminant(other));
                std::process::exit(1);
            }
        };

        let raw = A2lValue::from_bytes(&datatype, &bytes).expect("Failed to parse bytes");
        println!("Object:     {} (CHARACTERISTIC)", name);
        println!("DataType:   {:?}", datatype);
        println!("Raw value:  {:?}", raw);

        if conversion != "NO_COMPU_METHOD" {
            if let Ok(Some(label)) = compu_method::convert_raw_to_string(&raw, &conversion, module) {
                println!("Physical:   \"{}\"", label);
            } else if let Ok(phys) = compu_method::convert_raw_to_physical(&raw, &conversion, module) {
                println!("Physical:   {} {}", phys, unit);
            }
        } else {
            println!("Physical:   {} {} (no conversion)", raw.as_f64().unwrap_or(f64::NAN), unit);
        }
        return;
    }

    eprintln!("Error: '{}' not found as measurement or characteristic", name);
    std::process::exit(1);
}

/// Parse hex string ("0xffe6", "ffe6", "ff e6") into bytes.
fn parse_hex_bytes(s: &str) -> Vec<u8> {
    let s = s.trim().strip_prefix("0x").or_else(|| s.trim().strip_prefix("0X")).unwrap_or(s.trim());
    let hex: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(hex.len() % 2 == 0, "hex string must have even number of digits");
    hex.as_bytes()
        .chunks(2)
        .map(|chunk| {
            let s = std::str::from_utf8(chunk).unwrap();
            u8::from_str_radix(s, 16).expect("invalid hex byte")
        })
        .collect()
}
