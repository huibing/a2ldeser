# FIX_AXIS Support for a2ldeser

## Problem

MAP/CURVE characteristics using `FIX_AXIS` axis type are parsed incorrectly because:

1. **FIX_AXIS_PAR formula is wrong**: Current `compute_fix_axis_par_values` computes `offset + shift * i` (0-indexed, linear multiplication). The correct formula is `offset + (i-1) * 2^shift` (1-indexed, power-of-two multiplier).

2. **FIX_AXIS_PAR_DIST is not handled**: The resolver skips `fix_axis_par_dist` entirely, falling through to `StdAxis` which returns placeholder index values.

## Changes

### Data Model (`src/resolver.rs`)

- Fix `AxisSource::FixAxisPar` — no structural change, only formula fix
- Add `AxisSource::FixAxisParDist { offset: f64, distance: f64, count: u16 }` variant

### Resolver (`src/resolver.rs`)

- **`resolve_axis_source()`**: Add check for `ad.fix_axis_par_dist` after `fix_axis_par_list` check, before `axis_pts_ref` check
- **`compute_fix_axis_par_values()`**: Fix formula to `offset + (i as f64 - 1.0) * 2.0_f64.powf(shift)` for `i in 1..=count`
- **`compute_fix_axis_par_dist_values()`**: New function returning `offset + (i as f64 - 1.0) * distance` for `i in 1..=count`

### Extractor (`src/extractor.rs`)

- **`axis_point_count()`**: Add `AxisSource::FixAxisParDist { count, .. } => Ok(*count as usize)`
- **`read_axis_values()`**: Add branch for `AxisSource::FixAxisParDist` calling `compute_fix_axis_par_dist_values()`

## Formulas

| Variant | Formula | Parameters |
|---------|---------|------------|
| FIX_AXIS_PAR | `xi = offset + (i-1) * 2^shift` , 1 ≤ i ≤ num | offset, shift, num |
| FIX_AXIS_PAR_DIST | `xi = offset + (i-1) * distance` , 1 ≤ i ≤ num | offset, distance, num |
| FIX_AXIS_PAR_LIST | Explicit list | axis_pts_value_list |

## Files Modified

| File | Scope |
|------|-------|
| `src/resolver.rs` | `AxisSource` enum, `resolve_axis_source()`, `compute_fix_axis_par_values()`, new `compute_fix_axis_par_dist_values()` |
| `src/extractor.rs` | `axis_point_count()`, `read_axis_values()` |
| `tests/integration.rs` | Update fix_axis_par expected values, add fix_axis_par_dist test if sample available |
