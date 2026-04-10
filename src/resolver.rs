//! A2L object resolver — resolves cross-references in the A2L module to produce
//! fully-resolved descriptions of CURVE, MAP, VALUE, and other characteristics.
//!
//! This module walks the reference graph:
//!   Characteristic → AxisDescr → AxisPts → CompuMethod → CompuVtab
//!                  → RecordLayout
//!                  → CompuMethod
//!
//! It does NOT read binary data from HEX files. It resolves the *metadata*
//! so that a binary reader knows exactly what to extract.

use a2lfile::{
    A2lObjectName, AxisDescrAttribute, CharacteristicType, DataType, Module,
};

// ========================================================================
// Error types
// ========================================================================

/// Errors that can occur during A2L reference resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveError {
    /// A named object was not found in the module.
    NotFound { kind: &'static str, name: String },
    /// A characteristic has an unexpected type for the requested operation.
    WrongType {
        name: String,
        expected: &'static str,
        actual: String,
    },
    /// An axis descriptor is missing required information.
    IncompleteAxis { characteristic: String, detail: String },
    /// A conversion error occurred during resolution.
    Conversion(crate::compu_method::ConversionError),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound { kind, name } => {
                write!(f, "{kind} '{name}' not found")
            }
            ResolveError::WrongType {
                name,
                expected,
                actual,
            } => {
                write!(f, "'{name}' is {actual}, expected {expected}")
            }
            ResolveError::IncompleteAxis {
                characteristic,
                detail,
            } => {
                write!(f, "incomplete axis on '{characteristic}': {detail}")
            }
            ResolveError::Conversion(e) => write!(f, "conversion error: {e}"),
        }
    }
}

impl std::error::Error for ResolveError {}

impl From<crate::compu_method::ConversionError> for ResolveError {
    fn from(e: crate::compu_method::ConversionError) -> Self {
        ResolveError::Conversion(e)
    }
}

// ========================================================================
// Resolved data structures
// ========================================================================

/// Describes how an axis's breakpoints are defined.
#[derive(Debug, Clone, PartialEq)]
pub enum AxisSource {
    /// FIX_AXIS_PAR: computed from offset + shift * index.
    FixAxisPar {
        offset: f64,
        shift: f64,
        count: u16,
    },
    /// FIX_AXIS_PAR_LIST: explicit list of breakpoints in the A2L file.
    FixAxisParList { values: Vec<f64> },
    /// COM_AXIS: axis breakpoints come from a separate AXIS_PTS object.
    ComAxis {
        axis_pts_name: String,
        axis_pts_address: u32,
        max_axis_points: u16,
        deposit_name: String,
    },
    /// STD_AXIS: axis breakpoints are embedded in the characteristic's record.
    StdAxis { max_axis_points: u16 },
}

/// Fully resolved axis information for a CURVE or MAP characteristic.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAxis {
    /// Axis attribute (X, Y, etc.)
    pub attribute: AxisDescrAttribute,
    /// Conversion method name (or "NO_COMPU_METHOD").
    pub conversion: String,
    /// Physical unit string.
    pub unit: String,
    /// Maximum number of axis points.
    pub max_axis_points: u16,
    /// How the axis breakpoints are sourced.
    pub source: AxisSource,
}

/// Record layout summary relevant to data extraction.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLayout {
    pub name: String,
    pub fnc_values_datatype: Option<DataType>,
}

/// Fully resolved CURVE (1D lookup table) metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCurve {
    pub name: String,
    pub long_identifier: String,
    pub address: u32,
    pub conversion: String,
    pub unit: String,
    pub layout: ResolvedLayout,
    pub x_axis: ResolvedAxis,
}

/// Fully resolved MAP (2D lookup table) metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMap {
    pub name: String,
    pub long_identifier: String,
    pub address: u32,
    pub conversion: String,
    pub unit: String,
    pub layout: ResolvedLayout,
    pub x_axis: ResolvedAxis,
    pub y_axis: ResolvedAxis,
}

/// Fully resolved VALUE (scalar calibration) metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedValue {
    pub name: String,
    pub long_identifier: String,
    pub address: u32,
    pub conversion: String,
    pub unit: String,
    pub layout: ResolvedLayout,
}

/// Unified resolved characteristic.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedCharacteristic {
    Value(ResolvedValue),
    Curve(ResolvedCurve),
    Map(ResolvedMap),
}

// ========================================================================
// Resolver
// ========================================================================

/// Resolves A2L cross-references within a Module.
pub struct Resolver<'a> {
    module: &'a Module,
}

impl<'a> Resolver<'a> {
    pub fn new(module: &'a Module) -> Self {
        Self { module }
    }

    /// Resolve a characteristic by name into its fully-resolved form.
    pub fn resolve_characteristic(
        &self,
        name: &str,
    ) -> Result<ResolvedCharacteristic, ResolveError> {
        let ch = self
            .module
            .characteristic
            .iter()
            .find(|c| c.get_name() == name)
            .ok_or_else(|| ResolveError::NotFound {
                kind: "Characteristic",
                name: name.to_string(),
            })?;

        let layout = self.resolve_layout(&ch.deposit)?;
        let unit = self.lookup_unit(&ch.conversion);

        match ch.characteristic_type {
            CharacteristicType::Value | CharacteristicType::ValBlk => {
                Ok(ResolvedCharacteristic::Value(ResolvedValue {
                    name: ch.get_name().to_string(),
                    long_identifier: ch.long_identifier.clone(),
                    address: ch.address,
                    conversion: ch.conversion.clone(),
                    unit,
                    layout,
                }))
            }
            CharacteristicType::Curve => {
                let x_axis = self.resolve_axis(ch.get_name(), &ch.axis_descr, 0)?;
                Ok(ResolvedCharacteristic::Curve(ResolvedCurve {
                    name: ch.get_name().to_string(),
                    long_identifier: ch.long_identifier.clone(),
                    address: ch.address,
                    conversion: ch.conversion.clone(),
                    unit,
                    layout,
                    x_axis,
                }))
            }
            CharacteristicType::Map => {
                let x_axis = self.resolve_axis(ch.get_name(), &ch.axis_descr, 0)?;
                let y_axis = self.resolve_axis(ch.get_name(), &ch.axis_descr, 1)?;
                Ok(ResolvedCharacteristic::Map(ResolvedMap {
                    name: ch.get_name().to_string(),
                    long_identifier: ch.long_identifier.clone(),
                    address: ch.address,
                    conversion: ch.conversion.clone(),
                    unit,
                    layout,
                    x_axis,
                    y_axis,
                }))
            }
            CharacteristicType::Ascii => {
                Ok(ResolvedCharacteristic::Value(ResolvedValue {
                    name: ch.get_name().to_string(),
                    long_identifier: ch.long_identifier.clone(),
                    address: ch.address,
                    conversion: ch.conversion.clone(),
                    unit,
                    layout,
                }))
            }
            _ => Err(ResolveError::WrongType {
                name: name.to_string(),
                expected: "Value, Curve, Map, or Ascii",
                actual: format!("{:?}", ch.characteristic_type),
            }),
        }
    }

    /// Resolve a record layout by name.
    fn resolve_layout(&self, name: &str) -> Result<ResolvedLayout, ResolveError> {
        let rl = self
            .module
            .record_layout
            .iter()
            .find(|r| r.get_name() == name)
            .ok_or_else(|| ResolveError::NotFound {
                kind: "RecordLayout",
                name: name.to_string(),
            })?;

        Ok(ResolvedLayout {
            name: rl.get_name().to_string(),
            fnc_values_datatype: rl.fnc_values.as_ref().map(|f| f.datatype.clone()),
        })
    }

    /// Resolve axis descriptor at the given index (0=X, 1=Y).
    fn resolve_axis(
        &self,
        char_name: &str,
        axis_descrs: &[a2lfile::AxisDescr],
        index: usize,
    ) -> Result<ResolvedAxis, ResolveError> {
        let ad = axis_descrs.get(index).ok_or_else(|| {
            ResolveError::IncompleteAxis {
                characteristic: char_name.to_string(),
                detail: format!("missing axis descriptor at index {index}"),
            }
        })?;

        let unit = self.lookup_unit(&ad.conversion);
        let source = self.resolve_axis_source(char_name, ad)?;

        Ok(ResolvedAxis {
            attribute: ad.attribute.clone(),
            conversion: ad.conversion.clone(),
            unit,
            max_axis_points: ad.max_axis_points,
            source,
        })
    }

    /// Determine the axis source (FixAxis, ComAxis, StdAxis).
    fn resolve_axis_source(
        &self,
        _char_name: &str,
        ad: &a2lfile::AxisDescr,
    ) -> Result<AxisSource, ResolveError> {
        // Check for FIX_AXIS_PAR
        if let Some(ref fap) = ad.fix_axis_par {
            return Ok(AxisSource::FixAxisPar {
                offset: fap.offset as f64,
                shift: fap.shift as f64,
                count: fap.number_apo,
            });
        }

        // Check for FIX_AXIS_PAR_LIST
        if let Some(ref fapl) = ad.fix_axis_par_list {
            return Ok(AxisSource::FixAxisParList {
                values: fapl.axis_pts_value_list.clone(),
            });
        }

        // Check for COM_AXIS (axis_pts_ref present)
        if let Some(ref apr) = ad.axis_pts_ref {
            let ap = self
                .module
                .axis_pts
                .iter()
                .find(|a| a.get_name() == apr.axis_points)
                .ok_or_else(|| ResolveError::NotFound {
                    kind: "AxisPts",
                    name: apr.axis_points.clone(),
                })?;

            let deposit_name = ap.deposit_record.clone();

            return Ok(AxisSource::ComAxis {
                axis_pts_name: ap.get_name().to_string(),
                axis_pts_address: ap.address,
                max_axis_points: ap.max_axis_points,
                deposit_name,
            });
        }

        // Default: STD_AXIS (axis embedded in the characteristic record)
        Ok(AxisSource::StdAxis {
            max_axis_points: ad.max_axis_points,
        })
    }

    /// Compute FixAxisPar breakpoint values: value[i] = offset + shift * i.
    pub fn compute_fix_axis_par_values(offset: f64, shift: f64, count: u16) -> Vec<f64> {
        (0..count)
            .map(|i| offset + shift * i as f64)
            .collect()
    }

    /// Look up the physical unit for a conversion method name.
    fn lookup_unit(&self, compu_method_name: &str) -> String {
        if compu_method_name == "NO_COMPU_METHOD" {
            return String::new();
        }
        self.module
            .compu_method
            .iter()
            .find(|cm| cm.get_name() == compu_method_name)
            .map(|cm| cm.unit.clone())
            .unwrap_or_default()
    }

    /// List all characteristics of a given type.
    pub fn list_characteristics(
        &self,
        char_type: CharacteristicType,
    ) -> Vec<&a2lfile::Characteristic> {
        self.module
            .characteristic
            .iter()
            .filter(|c| c.characteristic_type == char_type)
            .collect()
    }

    /// Resolve all curves in the module.
    pub fn resolve_all_curves(&self) -> Vec<Result<ResolvedCurve, ResolveError>> {
        self.list_characteristics(CharacteristicType::Curve)
            .into_iter()
            .map(|c| {
                match self.resolve_characteristic(c.get_name())? {
                    ResolvedCharacteristic::Curve(curve) => Ok(curve),
                    _ => unreachable!(),
                }
            })
            .collect()
    }

    /// Resolve all maps in the module.
    pub fn resolve_all_maps(&self) -> Vec<Result<ResolvedMap, ResolveError>> {
        self.list_characteristics(CharacteristicType::Map)
            .into_iter()
            .map(|c| {
                match self.resolve_characteristic(c.get_name())? {
                    ResolvedCharacteristic::Map(map) => Ok(map),
                    _ => unreachable!(),
                }
            })
            .collect()
    }
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_module() -> a2lfile::A2lFile {
        let content = r#"
            ASAP2_VERSION 1 70
            /begin PROJECT test ""
              /begin MODULE test ""
                /begin COMPU_METHOD cm_identity "" IDENTICAL "%5.3" ""
                /end COMPU_METHOD
                /begin COMPU_METHOD cm_ratio "" RAT_FUNC "%8.4" "rpm"
                  COEFFS 0 1 0 0 0 1
                /end COMPU_METHOD
                /begin RECORD_LAYOUT rl_scalar
                  FNC_VALUES 1 FLOAT32_IEEE COLUMN_DIR DIRECT
                /end RECORD_LAYOUT
                /begin RECORD_LAYOUT rl_curve
                  FNC_VALUES 1 FLOAT32_IEEE COLUMN_DIR DIRECT
                /end RECORD_LAYOUT
                /begin RECORD_LAYOUT rl_map
                  FNC_VALUES 1 FLOAT32_IEEE COLUMN_DIR DIRECT
                /end RECORD_LAYOUT
              /end MODULE
            /end PROJECT
        "#;
        let (a2l, _) = a2lfile::load_from_string(content, None, false).expect("parse");
        a2l
    }

    #[test]
    fn resolve_nonexistent_characteristic() {
        let a2l = minimal_module();
        let r = Resolver::new(&a2l.project.module[0]);
        let result = r.resolve_characteristic("no_such_thing");
        assert!(matches!(
            result,
            Err(ResolveError::NotFound { kind: "Characteristic", .. })
        ));
    }

    #[test]
    fn resolve_nonexistent_record_layout() {
        let a2l = minimal_module();
        let r = Resolver::new(&a2l.project.module[0]);
        let result = r.resolve_layout("no_such_layout");
        assert!(matches!(
            result,
            Err(ResolveError::NotFound { kind: "RecordLayout", .. })
        ));
    }

    #[test]
    fn fix_axis_par_values_basic() {
        let vals = Resolver::compute_fix_axis_par_values(0.0, 10.0, 5);
        assert_eq!(vals, vec![0.0, 10.0, 20.0, 30.0, 40.0]);
    }

    #[test]
    fn fix_axis_par_values_with_offset() {
        let vals = Resolver::compute_fix_axis_par_values(100.0, 0.5, 3);
        assert_eq!(vals, vec![100.0, 100.5, 101.0]);
    }

    #[test]
    fn fix_axis_par_values_zero_count() {
        let vals = Resolver::compute_fix_axis_par_values(0.0, 1.0, 0);
        assert!(vals.is_empty());
    }

    #[test]
    fn lookup_unit_no_compu_method() {
        let a2l = minimal_module();
        let r = Resolver::new(&a2l.project.module[0]);
        assert_eq!(r.lookup_unit("NO_COMPU_METHOD"), "");
    }

    #[test]
    fn lookup_unit_existing_method() {
        let a2l = minimal_module();
        let r = Resolver::new(&a2l.project.module[0]);
        assert_eq!(r.lookup_unit("cm_ratio"), "rpm");
    }

    #[test]
    fn lookup_unit_missing_method() {
        let a2l = minimal_module();
        let r = Resolver::new(&a2l.project.module[0]);
        assert_eq!(r.lookup_unit("nonexistent"), "");
    }

    #[test]
    fn resolve_layout_from_module() {
        let a2l = minimal_module();
        let r = Resolver::new(&a2l.project.module[0]);
        let layout = r.resolve_layout("rl_scalar").unwrap();
        assert_eq!(layout.name, "rl_scalar");
        assert_eq!(layout.fnc_values_datatype, Some(DataType::Float32Ieee));
    }

    #[test]
    fn resolve_error_display() {
        let e = ResolveError::NotFound {
            kind: "Characteristic",
            name: "foo".into(),
        };
        assert_eq!(format!("{e}"), "Characteristic 'foo' not found");

        let e = ResolveError::WrongType {
            name: "bar".into(),
            expected: "Curve",
            actual: "Value".into(),
        };
        assert_eq!(format!("{e}"), "'bar' is Value, expected Curve");
    }
}
