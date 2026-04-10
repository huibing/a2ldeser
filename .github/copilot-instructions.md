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

let mut logmsgs = Vec::<A2LError>::new();
let a2l = a2lfile::load(&input_path, None, &mut logmsgs, false)
    .expect("could not load a2l file");

// Access data through the typed AST:
//   a2l.project.module[0].measurement
//   a2l.project.module[0].characteristic
//   a2l.project.module[0].compu_method
//   a2l.project.module[0].record_layout
//   a2l.project.module[0].axis_pts
```

Key `a2lfile` features to leverage:
- `a2lfile::load()` — parse an A2L file into a typed struct hierarchy
- `.check()` — consistency validation
- `.sort_new_items()` — sort after modifications
- `.write()` — serialize back to A2L format
- IF_DATA blocks are accessible for protocol-specific data (e.g., XCP config)

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
