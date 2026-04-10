# a2ldeser

A Rust library and CLI tool for deserializing [ASAP2 (A2L)](https://www.asam.net/standards/detail/mcd-2-mc/) ECU calibration files and extracting typed values from Intel HEX flash images.

## Features

- **Full A2L metadata resolution** — resolves cross-references across Characteristics, Measurements, RecordLayouts, AXIS_PTS, COMPU_METHODs, and COMPU_VTABs
- **Intel HEX reader** — loads I32HEX flash images into a queryable memory model
- **COMPU_METHOD conversion** — supports Identical, Linear, RatFunc, TabVerb (with COMPU_VTAB/COMPU_VTAB_RANGE), and TabIntp
- **All characteristic types** — VALUE (scalar), CURVE (1D lookup), MAP (2D lookup), VAL_BLK (array), ASCII (string)
- **Measurement support** — resolves measurement metadata with RAM address guard
- **Raw byte decoding** — decode hex bytes from XCP/CCP traces or memory dumps
- **CLI tool** — extract values from HEX files or decode raw bytes, with `--format json` support

## Installation

```sh
cargo install --path .
```

Or add as a dependency:

```toml
[dependencies]
a2ldeser = { path = "path/to/a2ldeser" }
```

## CLI Usage

All subcommands support `--format json` for structured JSON output (default: `text`).

### Extract characteristic from HEX file

```sh
a2ldeser extract <A2L_FILE> <HEX_FILE> <NAME>

# Scalar value
a2ldeser extract my.a2l my.hex g_xcp_enable_status
# → g_xcp_enable_status: 1  (raw: U32(1))

# JSON output
a2ldeser --format json extract my.a2l my.hex g_xcp_enable_status
# → { "Value": { "name": "g_xcp_enable_status", ... } }

# 1D curve
a2ldeser extract my.a2l my.hex MyCurve
# → MyCurve (CURVE, 10 points, unit: rpm)

# 2D map
a2ldeser extract my.a2l my.hex MyMap
# → MyMap (MAP, 8x8, unit: Nm)

# ASCII string
a2ldeser extract my.a2l my.hex CalPartNumber
# → CalPartNumber (ASCII): "P0425521 AT"

# List all characteristics
a2ldeser extract my.a2l my.hex list
```

### Decode raw bytes

```sh
a2ldeser decode <A2L_FILE> <NAME> <RAW_BYTES>

# Decode a measurement
a2ldeser decode my.a2l engine_speed "0x1027"
# → engine_speed (MEASUREMENT, Uword): raw=U16(10000) → 10000 rpm

# Accepts: "0xffe6", "ffe6", "ff e6", "FF E6"
```

### List all objects

```sh
a2ldeser list <A2L_FILE>
```

### Export all values to file

```sh
# Export as JSON (array of all extracted objects)
a2ldeser export <A2L_FILE> <HEX_FILE> -o output.json

# Export as CSV (flat: name, type, x, y, value, unit)
a2ldeser export <A2L_FILE> <HEX_FILE> -o output.csv
```

### Batch extraction with summary

```sh
a2ldeser summary <A2L_FILE> <HEX_FILE>
# → Extraction complete: 10751 succeeded, 0 failed out of 10751 total
# → Successes by type:
# →   VALUE: 9374
# →   CURVE: 355
# →   MAP: 344
# →   VAL_BLK: 673
# →   ASCII: 5
```

## Library API

### Extract values from HEX files

```rust
use a2ldeser::extractor::Extractor;
use a2ldeser::hex_reader::HexMemory;

// Load A2L and HEX files
let (a2l, _) = a2lfile::load(&a2l_path, None, false).unwrap();
let module = &a2l.project.module[0];
let hex = HexMemory::from_file("flash.hex").unwrap();

// Create extractor and extract values
let ext = Extractor::new(module, &hex);

let val = ext.extract_value("my_param").unwrap();
println!("{}: {:?}", val.name, val.physical);

let curve = ext.extract_curve("my_curve").unwrap();
println!("X: {:?}, Y: {:?}", curve.x_axis, curve.values);

let map = ext.extract_map("my_map").unwrap();
let ascii = ext.extract_ascii("my_string").unwrap();
let blk = ext.extract_val_blk("my_array").unwrap();

// Auto-detect type
let obj = ext.extract_any("some_name").unwrap();

// Batch extract all with error recovery
let report = ext.extract_all();
report.print_summary();
```

### Decode raw bytes

```rust
use a2ldeser::resolver::Resolver;
use a2ldeser::types::A2lValue;
use a2ldeser::compu_method;

let resolver = Resolver::new(module);

// Resolve measurement metadata
let meas = resolver.resolve_measurement("engine_speed").unwrap();
let raw = A2lValue::from_bytes(&meas.datatype, &[0x10, 0x27]).unwrap();
let physical = compu_method::convert_raw_to_physical(&raw, &meas.conversion, module).unwrap();
```

### Resolve A2L metadata (without reading binary data)

```rust
use a2ldeser::resolver::Resolver;

let resolver = Resolver::new(module);

// Resolve characteristic metadata
let resolved = resolver.resolve_characteristic("my_curve").unwrap();

// Resolve measurement metadata
let meas = resolver.resolve_measurement("engine_speed").unwrap();
println!("Address: 0x{:08X}, RAM: {}", meas.address, meas.is_ram());
```

## Module Structure

| Module | Purpose |
|--------|---------|
| `types` | `A2lValue` enum — typed deserialization from raw bytes |
| `compu_method` | COMPU_METHOD conversion (all 6 types) |
| `resolver` | Cross-reference resolution for Characteristics and Measurements |
| `hex_reader` | Intel HEX (I32HEX) reader with address-based byte access |
| `extractor` | End-to-end pipeline combining all modules |

## Examples

```sh
# Extract characteristics from A2L + HEX files
cargo run --example extract_characteristic -- my.a2l my.hex

# Decode raw bytes using A2L metadata
cargo run --example decode_bytes -- my.a2l engine_speed "0x1027"

# Explore A2L file structure
cargo run --example list_objects -- my.a2l
```

## Dependencies

- [`a2lfile`](https://crates.io/crates/a2lfile) v3.3.2 — A2L file parser
- [`ihex`](https://crates.io/crates/ihex) v3.0.0 — Intel HEX parser
- [`clap`](https://crates.io/crates/clap) v4 — CLI argument parsing
- [`serde`](https://crates.io/crates/serde) v1 — Serialization framework
- [`serde_json`](https://crates.io/crates/serde_json) v1 — JSON output

## License

MIT
