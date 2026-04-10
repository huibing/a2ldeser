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
| `src/resolver.rs` | Cross-reference resolver: Characteristic → axes → layout → units |
| `src/lib.rs` | Library root re-exporting all modules |
| `tests/integration.rs` | Integration tests against the real sample A2L file |

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

**Axis resolution chain:**
- `FixAxisPar` → computed values: `offset + shift * i` for `i in 0..count`
- `FixAxisParList` → explicit breakpoint list from the A2L file
- `ComAxis` → resolves `axis_pts_ref` → `AXIS_PTS` object (address, deposit_record, max_points)
- `StdAxis` → axis breakpoints embedded in the characteristic's binary record

**Key API:**
- `Resolver::compute_fix_axis_par_values(offset, shift, count)` → `Vec<f64>`
- `Resolver::list_characteristics(CharacteristicType)` → filtered list
- `ResolvedLayout.fnc_values_datatype` → the data type for reading binary values

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
