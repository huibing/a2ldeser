# Copilot Instructions — a2ldeser

## Project Overview

A Rust tool that uses the [`a2lfile`](https://github.com/DanielT/a2lfile) crate (v3.3.2) to parse A2L (ASAP2) files and extract/deserialize ECU measurement and calibration data. The reference spec is `refs/ASAM_AE_MCD-2_MC_BS_V1-7-1.pdf` (ASAP2 v1.70).

**Do not implement A2L parsing from scratch** — use the `a2lfile` crate which provides full ASAP2 v1.7.1 support.

## Build & Test

```sh
cargo build              # build
cargo test               # run all tests
cargo test <test_name>   # run a single test by name
cargo clippy             # lint
cargo fmt --check        # check formatting
```

## Key Dependency: `a2lfile` Crate

The `a2lfile` crate handles all A2L parsing. Core usage pattern:

```rust
use a2lfile::*;

let (a2l, logmsgs) = a2lfile::load(&input_path, None, false)
    .expect("could not load a2l file");

// Access data through the typed AST:
//   a2l.project.module[0].measurement
//   a2l.project.module[0].characteristic
//   a2l.project.module[0].compu_method
//   a2l.project.module[0].record_layout
//   a2l.project.module[0].axis_pts
```

**API notes:**
- `load()` returns `(A2lFile, Vec<A2lError>)` — a tuple, not a single value
- `name` fields on all named objects are `pub(crate)` — use `get_name()` from the `A2lObjectName` trait
- Most other fields (e.g., `datatype`, `conversion`, `axis_descr`, `coeffs`) are `pub`
- Structs with private `__block_info` fields can't be constructed with literal syntax — use `::new()` constructors
- `load_from_string()` is available for testing with inline A2L content
- `AxisDescrAttribute` variants are `ComAxis`, `FixAxis`, `StdAxis`, `CurveAxis`, `ResAxis` — these indicate axis *type*, not dimension (X/Y is determined by position in the `axis_descr` vector)
- `AxisPts.deposit_record` is a `String` (record layout name); `AxisPts.deposit` is an `Option<Deposit>` with only a `mode` field
- `FixAxisPar` fields: `offset: f64`, `shift: f64`, `number_apo: u16`
- `FixAxisParList` field: `axis_pts_value_list: Vec<f64>`
- Lookup RecordLayouts (e.g., `Lookup1D_FLOAT32_IEEE`) typically only define `fnc_values`; axis geometry comes from `axis_descr`, not from the layout's `axis_pts_x/y` fields

## Key Dependency: `ihex` Crate

The `ihex` crate (v3.0.0) handles low-level Intel HEX record parsing. We wrap it
in `HexMemory` (`src/hex_reader.rs`) which builds a contiguous memory image.

```rust
use a2ldeser::hex_reader::HexMemory;

let hex = HexMemory::from_file(Path::new("my.hex"))?;
let val = hex.read_u32_le(0x80040100)?;
```

Do not use `ihex` directly — always go through `HexMemory`.

## A2L Format Quick Reference

A2L files use `/begin KEYWORD` … `/end KEYWORD` block syntax. The hierarchy is:
`PROJECT` → `MODULE` → data objects (`MEASUREMENT`, `CHARACTERISTIC`, `COMPU_METHOD`, `RECORD_LAYOUT`, `AXIS_PTS`, etc.)

The `a2lfile` crate maps this directly to Rust structs — refer to its docs rather than parsing raw text.

## Reference Files

Files in `refs/` are large reference artifacts — do not modify or parse at build time:

| File | Purpose |
|------|---------|
| `ASAM_AE_MCD-2_MC_BS_V1-7-1.pdf` | ASAP2 v1.70 specification |
| `zc-blanc_rear_c-target-xcp.a2l` | Sample A2L file (XCP transport, ~13 MB) |
| `zc-blanc_rear_c_tc389-inca.hex` | Sample Intel HEX file (for ECU flash) |

## Architecture Guidelines

- Use Rust 2024 edition (set in `Cargo.toml`).
- All A2L parsing goes through `a2lfile::load()` — do not write custom parsers.
- Always collect and surface `A2LError` log messages from parsing; do not silently discard them.
- When extracting data, iterate the typed structs (e.g., `module.measurement`, `module.characteristic`) rather than string-matching on raw file content.

## Module Structure

| Module | Purpose |
|--------|---------|
| `src/types.rs` | `A2lValue` enum — all A2L data types with `from_bytes()`, `as_f64()` |
| `src/compu_method.rs` | COMPU_METHOD conversions (IDENTICAL, LINEAR, RAT_FUNC, TAB_*, TAB_VERB) |
| `src/resolver.rs` | Cross-reference resolver: Characteristic + Measurement → axes → layout → units |
| `src/hex_reader.rs` | Intel HEX file reader: `HexMemory` memory image with address-based access |
| `src/extractor.rs` | End-to-end pipeline: Resolver + HexMemory + A2lValue + CompuMethod → physical values |
| `src/lib.rs` | Library root re-exporting all modules |
| `tests/integration.rs` | Integration tests against the real sample A2L and HEX files |

## Resolver Pattern

The `Resolver` struct walks the A2L reference graph to produce fully-resolved metadata:

```rust
use a2ldeser::resolver::*;

let (a2l, _) = a2lfile::load(&path, None, false)?;
let module = &a2l.project.module[0];
let resolver = Resolver::new(module);

// Resolve a single characteristic
match resolver.resolve_characteristic("my_curve")? {
    ResolvedCharacteristic::Curve(c) => {
        println!("axis: {:?}", c.x_axis.source);
        println!("unit: {}", c.unit);
    }
    ResolvedCharacteristic::Map(m) => {
        println!("x: {:?}, y: {:?}", m.x_axis.source, m.y_axis.source);
    }
    ResolvedCharacteristic::Value(v) => {
        println!("conversion: {}", v.conversion);
    }
}

// Bulk resolve
let curves = resolver.resolve_all_curves();  // Vec<Result<ResolvedCurve, _>>
let maps = resolver.resolve_all_maps();      // Vec<Result<ResolvedMap, _>>
```

### Measurement Resolution (RAM Variables)

Measurements are **RAM variables** — they live in ECU memory at runtime and are
**NOT present in flash HEX files**. The resolver resolves their metadata
(address, data type, conversion, unit) but prevents HEX reads with a clear error.

```rust
// Resolve measurement metadata
let meas = resolver.resolve_measurement("engine_speed")?;
println!("{}: {} @ {:?}", meas.name, meas.unit, meas.ecu_address);
assert!(meas.is_ram());  // always true

// Attempting to read from HEX always fails
let result = resolver.read_measurement_from_hex("engine_speed", &hex);
// Err(MeasurementIsRam { name: "engine_speed", address: Some(0xD0001000) })
// "measurement 'engine_speed' is a RAM variable ... not in flash HEX files"

// Bulk resolve
let all_meas = resolver.resolve_all_measurements();
```

**Measurement vs Characteristic:**
| | Characteristic | Measurement |
|---|---|---|
| Memory | Flash (calibration) | RAM (runtime) |
| In HEX file | ✅ Yes | ❌ No |
| Writable | Via flash tool | ECU writes at runtime |
| Access | HEX read or XCP | XCP/CCP only |
| Resolver type | `ResolvedCharacteristic` | `ResolvedMeasurement` |

**Axis resolution chain:**
- `FixAxisPar` → computed values: `offset + shift * i` for `i in 0..count`
- `FixAxisParList` → explicit breakpoint list from the A2L file
- `ComAxis` → resolves `axis_pts_ref` → `AXIS_PTS` object (address, deposit_record, max_points)
- `StdAxis` → axis breakpoints embedded in the characteristic's binary record

**Key API:**
- `Resolver::compute_fix_axis_par_values(offset, shift, count)` → `Vec<f64>`
- `Resolver::list_characteristics(CharacteristicType)` → filtered list
- `ResolvedLayout.fnc_values_datatype` → the data type for reading binary values

### HexMemory — Intel HEX Reader (`src/hex_reader.rs`)

`HexMemory` loads an Intel HEX file into a flat, address-indexed memory image
using `BTreeMap<u32, Vec<u8>>` segments. Contiguous records are automatically
merged into single segments.

```rust
use a2ldeser::hex_reader::HexMemory;
use std::path::Path;

// Load from file
let hex = HexMemory::from_file(Path::new("refs/my_flash.hex"))?;

// Address queries
hex.contains(0x80040000, 4);      // true if 4 bytes are readable at addr
hex.min_address();                 // Some(0x80040000)
hex.max_address();                 // Some(0x801FFFFF)

// Typed reads (little-endian)
let raw_u8  = hex.read_u8(addr)?;
let raw_u16 = hex.read_u16_le(addr)?;
let raw_u32 = hex.read_u32_le(addr)?;
let raw_f32 = hex.read_f32_le(addr)?;
let bytes   = hex.read_bytes(addr, len)?;

// Segment introspection
hex.segment_count();              // number of contiguous regions
hex.total_bytes();                // total data bytes across all segments
for (base, data) in hex.segments() { ... }
```

**Supported HEX formats:**
- I32HEX (Extended Linear Address, record type 04) — common for 32-bit automotive ECUs
- I16HEX (Extended Segment Address, record type 02)
- Standard Data records (type 00), EOF (type 01)

**Integration with other modules:**
```rust
// Read a characteristic's raw value from the HEX file
let ch = module.characteristic.iter().find(|c| c.get_name() == "MyParam").unwrap();
let bytes = hex.read_bytes(ch.address, 4)?;
let raw_val = A2lValue::from_bytes(&bytes, DataType::Float32Ieee)?;
let phys_val = convert_raw_to_physical(raw_val.as_f64()?, &a2l_file, &compu_method_name)?;
```

### Extractor — End-to-End Pipeline (`src/extractor.rs`)

The `Extractor` combines all modules into a single pipeline that reads fully-
converted physical values from ECU flash data:

```
A2L metadata → Resolver → address + data type + layout
                ↓
HEX binary  → HexMemory → raw bytes at address
                ↓
              A2lValue  → typed raw value
                ↓
            CompuMethod → PhysicalValue (Numeric or Verbal)
```

```rust
use a2ldeser::extractor::*;

let ext = Extractor::new(module, &hex);

// Scalar value
let val = ext.extract_value("my_param")?;
println!("{}: {:?} {} (raw: {:?})", val.name, val.physical, val.unit, val.raw);

// 1D curve
let curve = ext.extract_curve("my_curve")?;
for (x, y) in curve.x_axis.iter().zip(curve.values.iter()) {
    println!("  x={x} → y={y:?}");
}

// 2D map
let map = ext.extract_map("my_map")?;
println!("{}x{} map", map.x_axis.len(), map.y_axis.len());

// Measurement — always fails (RAM, not in flash)
let err = ext.extract_measurement("engine_speed").unwrap_err();
// ExtractError::Resolve(MeasurementIsRam { ... })
```

**`PhysicalValue` enum:**
- `PhysicalValue::Numeric(f64)` — from IDENTICAL, LINEAR, RAT_FUNC conversions
- `PhysicalValue::Verbal(String)` — from TAB_VERB / COMPU_VTAB lookups
- Use `.as_f64()` or `.as_str()` for type-safe access

## Critical Design Areas

### 1. Comprehensive A2L Type Enum

Define a unified Rust enum that covers **all** possible A2L value types: unsigned/signed integers (8/16/32/64-bit), floats (32/64-bit), strings, and arrays. This type enum is central to deserialization — every MEASUREMENT, CHARACTERISTIC, and AXIS_PTS must map to a concrete variant. Do not use loosely-typed representations (e.g., `f64` for everything); preserve the original type fidelity from the A2L `RECORD_LAYOUT` and `deposit`/`fnc_values` definitions.

### 2. CURVE and MAP Deserialization

`CURVE` and `MAP` characteristics require special handling beyond simple scalar reads:
- **CURVE**: 1D lookup table — requires deserializing both the axis values (from `AXIS_DESCR` → `AXIS_PTS` or embedded `FIX_AXIS`) and the function values, respecting the `RECORD_LAYOUT` structure.
- **MAP**: 2D lookup table — two axes (X and Y) plus a 2D value array. Row-major vs column-major ordering depends on the `RECORD_LAYOUT` deposit direction (`ROW_DIR` / `COLUMN_DIR`).
- Both must handle `NO_AXIS_PTS_X/Y` (axis length stored in the data), rescaling via `AXIS_RESCALE`, and shared axis references (`AXIS_PTS_REF`).
- Always validate axis dimensions against the actual data size.

### 3. Full COMPU_METHOD Support

`COMPU_METHOD` is the conversion/computation engine in ASAP2 — it translates raw ECU values to physical values. All conversion types must be supported:
- **RAT_FUNC**: Rational function with 6 coefficients (a–f): `f(x) = (a*x² + b*x + c) / (d*x² + e*x + f)`
- **TAB_INTP** / **TAB_NOINTP**: Lookup table with/without interpolation (references `COMPU_TAB`)
- **TAB_VERB**: Verbal/enum conversion (references `COMPU_VTAB` or `COMPU_VTAB_RANGE`)
- **LINEAR**: Simple linear `f(x) = a*x + b` (coefficients in `COEFFS_LINEAR`)
- **IDENTICAL**: No conversion (raw = physical)
- **FORMULA**: Free-form formula string (handle or flag as unsupported if not implementing an expression evaluator)

Link each MEASUREMENT/CHARACTERISTIC to its COMPU_METHOD by name, and apply the conversion when deserializing values to physical units. Handle the `COMPU_METHOD "NO_COMPU_METHOD"` sentinel (identity conversion).
