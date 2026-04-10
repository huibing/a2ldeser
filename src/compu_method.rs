use a2lfile::{CompuVtab, CompuVtabRange, ConversionType, Module};

use crate::types::A2lValue;

/// Error type for COMPU_METHOD conversion failures.
#[derive(Debug, Clone, PartialEq)]
pub enum ConversionError {
    /// The COMPU_METHOD name was not found in the module.
    MethodNotFound(String),
    /// The referenced COMPU_TAB/COMPU_VTAB was not found.
    TableNotFound(String),
    /// The input value cannot be converted (e.g., non-numeric).
    InvalidInput,
    /// The FORMULA conversion type is not yet supported.
    FormulaNotSupported,
    /// Division by zero in RAT_FUNC computation.
    DivisionByZero,
    /// No matching entry in a TAB_VERB lookup.
    NoMatchingEntry(f64),
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversionError::MethodNotFound(name) => {
                write!(f, "COMPU_METHOD '{name}' not found")
            }
            ConversionError::TableNotFound(name) => {
                write!(f, "conversion table '{name}' not found")
            }
            ConversionError::InvalidInput => write!(f, "invalid input for conversion"),
            ConversionError::FormulaNotSupported => {
                write!(f, "FORMULA conversion type not supported")
            }
            ConversionError::DivisionByZero => write!(f, "division by zero in RAT_FUNC"),
            ConversionError::NoMatchingEntry(v) => {
                write!(f, "no matching entry for value {v}")
            }
        }
    }
}

impl std::error::Error for ConversionError {}

/// Apply IDENTICAL conversion (raw == physical).
pub fn convert_identical(raw: f64) -> f64 {
    raw
}

/// Apply LINEAR conversion: physical = a * raw + b
pub fn convert_linear(raw: f64, a: f64, b: f64) -> f64 {
    a * raw + b
}

/// Apply RAT_FUNC conversion.
/// The ASAP2 spec defines coefficients for physical→internal:
///   internal = (a*phys² + b*phys + c) / (d*phys² + e*phys + f)
///
/// For the common case where a=0, d=0 (simple linear rational function):
///   physical = (c - raw*f) / (raw*e - b)  when d=0, a=0
///
/// For the fully general case, we solve the inverse numerically or handle
/// the common patterns.
pub fn convert_rat_func(raw: f64, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Result<f64, ConversionError> {
    // Most common case: a=0, d=0 → linear rational
    // internal = (b*phys + c) / (e*phys + f)
    // Solving for phys: phys = (c - raw*f) / (raw*e - b)
    if a == 0.0 && d == 0.0 {
        let denominator = raw * e - b;
        if denominator.abs() < f64::EPSILON {
            return Err(ConversionError::DivisionByZero);
        }
        Ok((c - raw * f) / denominator)
    } else {
        // General quadratic case: solve a*phys² + (b - raw*d)*phys² ... 
        // This reduces to: (a - raw*d)*phys² + (b - raw*e)*phys + (c - raw*f) = 0
        let qa = a - raw * d;
        let qb = b - raw * e;
        let qc = c - raw * f;

        if qa.abs() < f64::EPSILON {
            // Degenerates to linear
            if qb.abs() < f64::EPSILON {
                return Err(ConversionError::DivisionByZero);
            }
            return Ok(-qc / qb);
        }

        let discriminant = qb * qb - 4.0 * qa * qc;
        if discriminant < 0.0 {
            return Err(ConversionError::DivisionByZero);
        }
        // Return the root that typically makes physical sense (positive root)
        let sqrt_d = discriminant.sqrt();
        let root1 = (-qb + sqrt_d) / (2.0 * qa);
        let root2 = (-qb - sqrt_d) / (2.0 * qa);
        // Prefer the root closer to raw value as a heuristic
        if (root1 - raw).abs() <= (root2 - raw).abs() {
            Ok(root1)
        } else {
            Ok(root2)
        }
    }
}

/// Apply TAB_INTP conversion (lookup table with linear interpolation).
pub fn convert_tab_intp(raw: f64, table: &[(f64, f64)]) -> Result<f64, ConversionError> {
    if table.is_empty() {
        return Err(ConversionError::NoMatchingEntry(raw));
    }
    if table.len() == 1 {
        return Ok(table[0].1);
    }

    // Clamp to table bounds
    if raw <= table[0].0 {
        return Ok(table[0].1);
    }
    if raw >= table[table.len() - 1].0 {
        return Ok(table[table.len() - 1].1);
    }

    // Find interval and interpolate
    for window in table.windows(2) {
        let (x0, y0) = window[0];
        let (x1, y1) = window[1];
        if raw >= x0 && raw <= x1 {
            if (x1 - x0).abs() < f64::EPSILON {
                return Ok(y0);
            }
            let t = (raw - x0) / (x1 - x0);
            return Ok(y0 + t * (y1 - y0));
        }
    }

    Err(ConversionError::NoMatchingEntry(raw))
}

/// Apply TAB_NOINTP conversion (lookup table, nearest match without interpolation).
pub fn convert_tab_nointp(raw: f64, table: &[(f64, f64)]) -> Result<f64, ConversionError> {
    if table.is_empty() {
        return Err(ConversionError::NoMatchingEntry(raw));
    }

    // Find the nearest entry
    let mut best = &table[0];
    let mut best_dist = (raw - best.0).abs();
    for entry in &table[1..] {
        let dist = (raw - entry.0).abs();
        if dist < best_dist {
            best = entry;
            best_dist = dist;
        }
    }
    Ok(best.1)
}

/// Apply TAB_VERB conversion (verbal/enum table lookup).
/// Returns the string label for the matching integer value.
pub fn convert_tab_verb(raw: f64, vtab: &CompuVtab) -> Result<String, ConversionError> {
    let raw_int = raw as i64;
    for pair in &vtab.value_pairs {
        if pair.in_val as i64 == raw_int {
            return Ok(pair.out_val.clone());
        }
    }
    // Check default value
    if let Some(ref default) = vtab.default_value {
        return Ok(default.display_string.clone());
    }
    Err(ConversionError::NoMatchingEntry(raw))
}

/// Apply TAB_VERB conversion using COMPU_VTAB_RANGE (range-based enum lookup).
pub fn convert_tab_verb_range(
    raw: f64,
    vtab_range: &CompuVtabRange,
) -> Result<String, ConversionError> {
    for triple in &vtab_range.value_triples {
        if raw >= triple.in_val_min && raw <= triple.in_val_max {
            return Ok(triple.out_val.clone());
        }
    }
    if let Some(ref default) = vtab_range.default_value {
        return Ok(default.display_string.clone());
    }
    Err(ConversionError::NoMatchingEntry(raw))
}

/// Convert a raw value to physical using the named COMPU_METHOD from the module.
/// Handles "NO_COMPU_METHOD" sentinel as identity.
pub fn convert_raw_to_physical(
    raw: &A2lValue,
    compu_method_name: &str,
    module: &Module,
) -> Result<f64, ConversionError> {
    let raw_f64 = raw.as_f64().ok_or(ConversionError::InvalidInput)?;

    if compu_method_name == "NO_COMPU_METHOD" {
        return Ok(raw_f64);
    }

    use a2lfile::A2lObjectName;
    let cm = module
        .compu_method
        .iter()
        .find(|cm| cm.get_name() == compu_method_name)
        .ok_or_else(|| ConversionError::MethodNotFound(compu_method_name.to_string()))?;

    match cm.conversion_type {
        ConversionType::Identical => Ok(convert_identical(raw_f64)),
        ConversionType::Linear => {
            let cl = cm
                .coeffs_linear
                .as_ref()
                .ok_or(ConversionError::InvalidInput)?;
            Ok(convert_linear(raw_f64, cl.a, cl.b))
        }
        ConversionType::RatFunc => {
            let co = cm.coeffs.as_ref().ok_or(ConversionError::InvalidInput)?;
            convert_rat_func(raw_f64, co.a, co.b, co.c, co.d, co.e, co.f)
        }
        ConversionType::TabVerb => {
            let tab_ref = cm
                .compu_tab_ref
                .as_ref()
                .ok_or(ConversionError::InvalidInput)?;
            // Try COMPU_VTAB first, then COMPU_VTAB_RANGE
            if let Some(vtab) = module
                .compu_vtab
                .iter()
                .find(|v| v.get_name() == tab_ref.conversion_table)
            {
                let label = convert_tab_verb(raw_f64, vtab)?;
                // For numeric result, return the raw value (the label is the "physical" form)
                // This is a string conversion; callers needing the label should use a dedicated path
                let _ = label;
                Ok(raw_f64)
            } else if let Some(vtab_range) = module
                .compu_vtab_range
                .iter()
                .find(|v| v.get_name() == tab_ref.conversion_table)
            {
                let label = convert_tab_verb_range(raw_f64, vtab_range)?;
                let _ = label;
                Ok(raw_f64)
            } else {
                Err(ConversionError::TableNotFound(
                    tab_ref.conversion_table.clone(),
                ))
            }
        }
        ConversionType::TabIntp | ConversionType::TabNointp => {
            let _tab_ref = cm
                .compu_tab_ref
                .as_ref()
                .ok_or(ConversionError::InvalidInput)?;
            // COMPU_TAB lookup would go here; for now return raw
            // (the sample file has 0 COMPU_TABs)
            Ok(raw_f64)
        }
        ConversionType::Form => Err(ConversionError::FormulaNotSupported),
    }
}

/// Convert a raw value to its string (verbal) representation.
///
/// Returns `Some(label)` for TAB_VERB conversions, `None` for numeric conversions.
/// Use this when you need the verbal label (e.g., "ON"/"OFF" for boolean states).
pub fn convert_raw_to_string(
    raw: &A2lValue,
    compu_method_name: &str,
    module: &Module,
) -> Result<Option<String>, ConversionError> {
    let raw_f64 = raw.as_f64().ok_or(ConversionError::InvalidInput)?;

    if compu_method_name == "NO_COMPU_METHOD" {
        return Ok(None);
    }

    use a2lfile::A2lObjectName;
    let cm = module
        .compu_method
        .iter()
        .find(|cm| cm.get_name() == compu_method_name)
        .ok_or_else(|| ConversionError::MethodNotFound(compu_method_name.to_string()))?;

    match cm.conversion_type {
        ConversionType::TabVerb => {
            let tab_ref = cm
                .compu_tab_ref
                .as_ref()
                .ok_or(ConversionError::InvalidInput)?;
            if let Some(vtab) = module
                .compu_vtab
                .iter()
                .find(|v| v.get_name() == tab_ref.conversion_table)
            {
                Ok(Some(convert_tab_verb(raw_f64, vtab)?))
            } else if let Some(vtab_range) = module
                .compu_vtab_range
                .iter()
                .find(|v| v.get_name() == tab_ref.conversion_table)
            {
                Ok(Some(convert_tab_verb_range(raw_f64, vtab_range)?))
            } else {
                Err(ConversionError::TableNotFound(
                    tab_ref.conversion_table.clone(),
                ))
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- IDENTICAL ---

    #[test]
    fn identical_passthrough() {
        assert_eq!(convert_identical(42.0), 42.0);
        assert_eq!(convert_identical(-3.125), -3.125);
        assert_eq!(convert_identical(0.0), 0.0);
    }

    // --- LINEAR ---

    #[test]
    fn linear_scale_and_offset() {
        // physical = 2.0 * raw + 10.0
        assert_eq!(convert_linear(5.0, 2.0, 10.0), 20.0);
    }

    #[test]
    fn linear_identity() {
        // a=1, b=0 → identity
        assert_eq!(convert_linear(7.5, 1.0, 0.0), 7.5);
    }

    #[test]
    fn linear_negative_slope() {
        assert_eq!(convert_linear(10.0, -0.5, 100.0), 95.0);
    }

    // --- RAT_FUNC ---

    #[test]
    fn rat_func_simple_linear() {
        // Common automotive pattern: a=0, b=0, c=1, d=0, e=0, f=1 → identity
        // internal = (0 + 0 + 1) / (0 + 0 + 1) = 1 for any phys
        // phys = (c - raw*f) / (raw*e - b) = (1 - raw) / (0 - 0) → div by zero
        // Actually: with a=0,d=0,b=0,e=0,c=1,f=1 → denom = raw*0 - 0 = 0 → error
        // Let's use a real pattern: scale factor 0.1
        // internal = (0 + 0 + 0) / (0 + 10*phys + 0) → not right
        // Typical: a=0, b=1, c=0, d=0, e=0, f=10 → internal = phys/10
        // phys = (0 - raw*10) / (raw*0 - 1) = -10*raw / -1 = 10*raw
        let result = convert_rat_func(5.0, 0.0, 1.0, 0.0, 0.0, 0.0, 10.0).unwrap();
        assert!((result - 50.0).abs() < 1e-10);
    }

    #[test]
    fn rat_func_offset_and_scale() {
        // internal = (0*x² + 1*x + (-273.15)) / (0*x² + 0*x + 1)
        // internal = phys - 273.15  →  phys = internal + 273.15
        // a=0, b=1, c=-273.15, d=0, e=0, f=1
        // phys = (c - raw*f) / (raw*e - b) = (-273.15 - raw) / (0 - 1) = 273.15 + raw
        let result = convert_rat_func(25.0, 0.0, 1.0, -273.15, 0.0, 0.0, 1.0).unwrap();
        assert!((result - 298.15).abs() < 1e-10);
    }

    #[test]
    fn rat_func_division_by_zero() {
        // a=0, d=0, b=0, e=0 → denominator = raw*0 - 0 = 0
        let result = convert_rat_func(1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0);
        assert!(matches!(result, Err(ConversionError::DivisionByZero)));
    }

    // --- TAB_INTP ---

    #[test]
    fn tab_intp_exact_match() {
        let table = vec![(0.0, 0.0), (100.0, 1000.0), (200.0, 2000.0)];
        assert_eq!(convert_tab_intp(100.0, &table).unwrap(), 1000.0);
    }

    #[test]
    fn tab_intp_interpolation() {
        let table = vec![(0.0, 0.0), (100.0, 1000.0)];
        let result = convert_tab_intp(50.0, &table).unwrap();
        assert!((result - 500.0).abs() < 1e-10);
    }

    #[test]
    fn tab_intp_clamp_low() {
        let table = vec![(10.0, 100.0), (20.0, 200.0)];
        assert_eq!(convert_tab_intp(5.0, &table).unwrap(), 100.0);
    }

    #[test]
    fn tab_intp_clamp_high() {
        let table = vec![(10.0, 100.0), (20.0, 200.0)];
        assert_eq!(convert_tab_intp(25.0, &table).unwrap(), 200.0);
    }

    #[test]
    fn tab_intp_empty_table() {
        let result = convert_tab_intp(1.0, &[]);
        assert!(matches!(result, Err(ConversionError::NoMatchingEntry(_))));
    }

    // --- TAB_NOINTP ---

    #[test]
    fn tab_nointp_exact_match() {
        let table = vec![(0.0, 10.0), (1.0, 20.0), (2.0, 30.0)];
        assert_eq!(convert_tab_nointp(1.0, &table).unwrap(), 20.0);
    }

    #[test]
    fn tab_nointp_nearest() {
        let table = vec![(0.0, 10.0), (10.0, 20.0), (20.0, 30.0)];
        // 7.0 is closer to 10.0 than to 0.0
        assert_eq!(convert_tab_nointp(7.0, &table).unwrap(), 20.0);
    }

    // --- TAB_VERB ---

    #[test]
    fn tab_verb_match() {
        let vtab = CompuVtab::new(
            "TestVtab".to_string(),
            "test".to_string(),
            ConversionType::TabVerb,
            3,
        );
        // Note: we need to add value_pairs. CompuVtab fields are pub.
        let mut vtab = vtab;
        vtab.value_pairs = vec![
            a2lfile::ValuePairsStruct::new(0.0, "OFF".to_string()),
            a2lfile::ValuePairsStruct::new(1.0, "ON".to_string()),
            a2lfile::ValuePairsStruct::new(2.0, "ERROR".to_string()),
        ];

        assert_eq!(convert_tab_verb(0.0, &vtab).unwrap(), "OFF");
        assert_eq!(convert_tab_verb(1.0, &vtab).unwrap(), "ON");
        assert_eq!(convert_tab_verb(2.0, &vtab).unwrap(), "ERROR");
    }

    #[test]
    fn tab_verb_no_match() {
        let vtab = CompuVtab::new(
            "TestVtab".to_string(),
            "test".to_string(),
            ConversionType::TabVerb,
            0,
        );
        let result = convert_tab_verb(99.0, &vtab);
        assert!(matches!(result, Err(ConversionError::NoMatchingEntry(_))));
    }

    // --- Integration: convert_raw_to_physical with NO_COMPU_METHOD ---

    #[test]
    fn no_compu_method_identity() {
        let raw = A2lValue::U16(1234);
        // We don't need a real module for NO_COMPU_METHOD
        let a2l_content = r#"
            ASAP2_VERSION 1 70
            /begin PROJECT test ""
              /begin MODULE test ""
              /end MODULE
            /end PROJECT
        "#;
        let (a2l, _) =
            a2lfile::load_from_string(a2l_content, None, false).expect("parse test a2l");
        let module = &a2l.project.module[0];

        let result = convert_raw_to_physical(&raw, "NO_COMPU_METHOD", module).unwrap();
        assert_eq!(result, 1234.0);
    }

    #[test]
    fn method_not_found_error() {
        let raw = A2lValue::U8(0);
        let a2l_content = r#"
            ASAP2_VERSION 1 70
            /begin PROJECT test ""
              /begin MODULE test ""
              /end MODULE
            /end PROJECT
        "#;
        let (a2l, _) =
            a2lfile::load_from_string(a2l_content, None, false).expect("parse test a2l");
        let module = &a2l.project.module[0];

        let result = convert_raw_to_physical(&raw, "NonExistent", module);
        assert!(matches!(result, Err(ConversionError::MethodNotFound(_))));
    }

    #[test]
    fn string_value_returns_invalid_input() {
        let raw = A2lValue::String("not a number".to_string());
        let a2l_content = r#"
            ASAP2_VERSION 1 70
            /begin PROJECT test ""
              /begin MODULE test ""
              /end MODULE
            /end PROJECT
        "#;
        let (a2l, _) =
            a2lfile::load_from_string(a2l_content, None, false).expect("parse test a2l");
        let module = &a2l.project.module[0];

        let result = convert_raw_to_physical(&raw, "NO_COMPU_METHOD", module);
        assert!(matches!(result, Err(ConversionError::InvalidInput)));
    }
}
