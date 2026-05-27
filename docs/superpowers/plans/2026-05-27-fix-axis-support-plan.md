# FIX_AXIS Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix FIX_AXIS_PAR formula and add FIX_AXIS_PAR_DIST support to correctly resolve MAP/CURVE axis values.

**Architecture:** Three changes: fix the FIX_AXIS_PAR formula in resolver, add FIX_AXIS_PAR_DIST variant to the AxisSource enum + resolver + extractor, and update tests to match correct values.

**Tech Stack:** Rust, a2lfile crate (ASAP2 parser), ihex crate

---

### Task 1: Fix FIX_AXIS_PAR formula and add FIX_AXIS_PAR_DIST to resolver

**Files:**
- Modify: `src/resolver.rs` (AxisSource enum, resolve_axis_source, compute_fix_axis_par_values, new compute_fix_axis_par_dist_values)

- [ ] **Step 1: Add FixAxisParDist variant to AxisSource enum**

In `src/resolver.rs`, add to the `AxisSource` enum (around line 96):
```rust
/// FIX_AXIS_PAR_DIST: computed from offset + (i-1) * distance.
FixAxisParDist {
    offset: f64,
    distance: f64,
    count: u16,
},
```

- [ ] **Step 2: Fix compute_fix_axis_par_values formula**

Current (wrong): `(0..count).map(|i| offset + shift * i as f64)`
Correct: `(1..=count).map(|i| offset + (i as f64 - 1.0) * 2.0_f64.powf(shift))`

```rust
pub fn compute_fix_axis_par_values(offset: f64, shift: f64, count: u16) -> Vec<f64> {
    (1..=count)
        .map(|i| offset + (i as f64 - 1.0) * 2.0_f64.powf(shift))
        .collect()
}
```

- [ ] **Step 3: Add compute_fix_axis_par_dist_values function**

```rust
pub fn compute_fix_axis_par_dist_values(offset: f64, distance: f64, count: u16) -> Vec<f64> {
    (1..=count)
        .map(|i| offset + (i as f64 - 1.0) * distance)
        .collect()
}
```

- [ ] **Step 4: Add fix_axis_par_dist check in resolve_axis_source**

After the `fix_axis_par_list` block (line 415), before `axis_pts_ref` check (line 418), add:
```rust
// Check for FIX_AXIS_PAR_DIST
if let Some(ref fapd) = ad.fix_axis_par_dist {
    return Ok(AxisSource::FixAxisParDist {
        offset: fapd.offset,
        distance: fapd.distance,
        count: fapd.number_apo,
    });
}
```

- [ ] **Step 5: Build and check for compilation errors**

Run: `cargo build`
Expected: Clean build with no errors

### Task 2: Update extractor for FIX_AXIS_PAR_DIST

**Files:**
- Modify: `src/extractor.rs` (axis_point_count, read_axis_values)

- [ ] **Step 1: Add FixAxisParDist branch to axis_point_count**

In `src/extractor.rs` around line 518, add:
```rust
AxisSource::FixAxisParDist { count, .. } => Ok(*count as usize),
```

- [ ] **Step 2: Add FixAxisParDist branch to read_axis_values**

In `src/extractor.rs` around line 536, after FixAxisPar branch:
```rust
AxisSource::FixAxisParDist {
    offset, distance, count: n,
} => Ok(Resolver::compute_fix_axis_par_dist_values(*offset, *distance, *n)),
```

- [ ] **Step 3: Build and check for compilation errors**

Run: `cargo build`
Expected: Clean build

### Task 3: Update tests

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Update resolved_curve_fix_axis_par_values monotonic assertion**

The old formula with shift=0 gives all-equal values (offset + 0*i = 0). The new formula with shift=0 gives offset + (i-1)*1 = i-1 (monotonic increasing). The test already handles this because it checks `w[1] >= w[0]` which holds for both, but the assertion comment and logic should be validated.

The test at line 974 already has a monotonic check with `*shift < 0.0` allowance. With shift=0 and the corrected formula, values become `[0, 1, 2, ..., 190]` — always monotonic increasing. The existing test logic should pass without changes.

- [ ] **Step 2: Add unit test for compute_fix_axis_par_dist_values**

Add test to `tests/integration.rs`:
```rust
#[test]
fn compute_fix_axis_par_dist_values_basic() {
    // offset=10, distance=5, count=4 → [10, 15, 20, 25]
    let values = Resolver::compute_fix_axis_par_dist_values(10.0, 5.0, 4);
    assert_eq!(values, vec![10.0, 15.0, 20.0, 25.0]);
}

#[test]
fn compute_fix_axis_par_dist_values_single() {
    let values = Resolver::compute_fix_axis_par_dist_values(100.0, 3.0, 1);
    assert_eq!(values, vec![100.0]);
}
```

- [ ] **Step 3: Add unit test for corrected fix_axis_par formula**

```rust
#[test]
fn compute_fix_axis_par_values_known() {
    // offset=0, shift=0 → 2^0=1 → [0, 1, 2, ...]
    let values = Resolver::compute_fix_axis_par_values(0.0, 0.0, 4);
    assert_eq!(values, vec![0.0, 1.0, 2.0, 3.0]);

    // offset=10, shift=3 → 2^3=8 → [10, 18, 26, 34]
    let values = Resolver::compute_fix_axis_par_values(10.0, 3.0, 4);
    assert_eq!(values, vec![10.0, 18.0, 26.0, 34.0]);

    // offset=0, shift=-1 → 2^-1=0.5 → [0, 0.5, 1.0, 1.5]
    let values = Resolver::compute_fix_axis_par_values(0.0, -1.0, 4);
    assert_eq!(values, vec![0.0, 0.5, 1.0, 1.5]);
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: All tests pass

### Task 4: Commit

- [ ] **Step 1: Create commit**

```bash
git add src/resolver.rs src/extractor.rs tests/integration.rs
git commit -m "fix: correct FIX_AXIS_PAR formula and add FIX_AXIS_PAR_DIST support

FIX_AXIS_PAR uses formula xi = offset + (i-1) * 2^shift (was incorrectly
using offset + shift * i with 0-indexing).

FIX_AXIS_PAR_DIST uses formula xi = offset + (i-1) * distance.

Both now use 1-indexed computation matching the ASAP2 specification."
```
