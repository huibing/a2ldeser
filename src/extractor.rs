//! End-to-end value extraction: A2L metadata + HEX binary data + COMPU_METHOD conversion.
//!
//! The `Extractor` combines:
//! - `Resolver` — resolves cross-references to get addresses, data types, layouts
//! - `HexMemory` — reads raw bytes from flash
//! - `A2lValue` — interprets raw bytes as typed values
//! - `convert_raw_to_physical` — applies COMPU_METHOD conversion
//!
//! Measurements are RAM variables and CANNOT be read from HEX files.

use a2lfile::{A2lObjectName, IndexMode, Module};

use crate::compu_method::{self, ConversionError};
use crate::hex_reader::{HexError, HexMemory};
use crate::resolver::{
    AxisSource, ResolveError, ResolvedAxis, ResolvedCharacteristic, Resolver,
};
use crate::types::A2lValue;

// ========================================================================
// Error types
// ========================================================================

/// Errors from end-to-end value extraction.
#[derive(Debug)]
pub enum ExtractError {
    /// A2L reference resolution failed.
    Resolve(ResolveError),
    /// HEX memory read failed.
    Hex(HexError),
    /// Value conversion failed.
    Conversion(ConversionError),
    /// Record layout is missing fnc_values (no data type info).
    NoFncValues { layout: String },
    /// AXIS_PTS record layout is missing axis_pts_x.
    NoAxisPtsX { layout: String },
    /// Unsupported index mode (e.g., AlternateCurves).
    UnsupportedIndexMode { layout: String, mode: String },
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::Resolve(e) => write!(f, "resolve: {e}"),
            ExtractError::Hex(e) => write!(f, "hex read: {e}"),
            ExtractError::Conversion(e) => write!(f, "conversion: {e}"),
            ExtractError::NoFncValues { layout } => {
                write!(f, "record layout '{layout}' has no fnc_values")
            }
            ExtractError::NoAxisPtsX { layout } => {
                write!(f, "axis_pts layout '{layout}' has no axis_pts_x")
            }
            ExtractError::UnsupportedIndexMode { layout, mode } => {
                write!(f, "unsupported index mode '{mode}' in layout '{layout}'")
            }
        }
    }
}

impl std::error::Error for ExtractError {}

impl From<ResolveError> for ExtractError {
    fn from(e: ResolveError) -> Self {
        ExtractError::Resolve(e)
    }
}

impl From<HexError> for ExtractError {
    fn from(e: HexError) -> Self {
        ExtractError::Hex(e)
    }
}

impl From<ConversionError> for ExtractError {
    fn from(e: ConversionError) -> Self {
        ExtractError::Conversion(e)
    }
}

// ========================================================================
// Extracted data structures
// ========================================================================

/// A physical value that may be numeric or verbal (string).
#[derive(Debug, Clone, PartialEq)]
pub enum PhysicalValue {
    /// Numeric physical value (from IDENTICAL, LINEAR, RAT_FUNC, etc.).
    Numeric(f64),
    /// Verbal label (from TAB_VERB / COMPU_VTAB).
    Verbal(String),
}

impl PhysicalValue {
    /// Get the numeric value, if this is a `Numeric` variant.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PhysicalValue::Numeric(v) => Some(*v),
            PhysicalValue::Verbal(_) => None,
        }
    }

    /// Get the verbal label, if this is a `Verbal` variant.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PhysicalValue::Numeric(_) => None,
            PhysicalValue::Verbal(s) => Some(s),
        }
    }
}

/// Extracted scalar VALUE characteristic.
#[derive(Debug, Clone)]
pub struct ExtractedValue {
    pub name: String,
    pub raw: A2lValue,
    pub physical: PhysicalValue,
    pub unit: String,
}

/// Extracted CURVE (1D lookup table) characteristic.
#[derive(Debug, Clone)]
pub struct ExtractedCurve {
    pub name: String,
    /// X axis breakpoints (physical values).
    pub x_axis: Vec<f64>,
    pub x_unit: String,
    /// Function values (physical).
    pub values: Vec<PhysicalValue>,
    pub unit: String,
}

/// Extracted MAP (2D lookup table) characteristic.
#[derive(Debug, Clone)]
pub struct ExtractedMap {
    pub name: String,
    /// X axis breakpoints (physical values).
    pub x_axis: Vec<f64>,
    pub x_unit: String,
    /// Y axis breakpoints (physical values).
    pub y_axis: Vec<f64>,
    pub y_unit: String,
    /// Function values in row-major order: values[y_idx][x_idx].
    pub values: Vec<Vec<PhysicalValue>>,
    pub unit: String,
}

/// Extracted VAL_BLK (1D array of calibration values) characteristic.
#[derive(Debug, Clone)]
pub struct ExtractedValBlk {
    pub name: String,
    /// Converted physical values.
    pub values: Vec<PhysicalValue>,
    pub unit: String,
}

/// Extracted ASCII (string) characteristic.
#[derive(Debug, Clone)]
pub struct ExtractedAscii {
    pub name: String,
    /// Decoded string (UTF-8, trailing NULs stripped).
    pub text: String,
}

// ========================================================================
// Extractor
// ========================================================================

/// Combines A2L metadata resolution, HEX binary reads, and COMPU_METHOD conversion
/// to extract fully-converted physical values from ECU flash data.
pub struct Extractor<'a> {
    module: &'a Module,
    hex: &'a HexMemory,
    resolver: Resolver<'a>,
}

impl<'a> Extractor<'a> {
    pub fn new(module: &'a Module, hex: &'a HexMemory) -> Self {
        Self {
            module,
            hex,
            resolver: Resolver::new(module),
        }
    }

    /// Access the underlying resolver.
    pub fn resolver(&self) -> &Resolver<'a> {
        &self.resolver
    }

    // ====================================================================
    // Scalar VALUE extraction
    // ====================================================================

    /// Extract a scalar VALUE characteristic by name.
    pub fn extract_value(&self, name: &str) -> Result<ExtractedValue, ExtractError> {
        let resolved = self.resolver.resolve_characteristic(name)?;
        match resolved {
            ResolvedCharacteristic::Value(v) => {
                let datatype = v
                    .layout
                    .fnc_values_datatype
                    .as_ref()
                    .ok_or_else(|| ExtractError::NoFncValues {
                        layout: v.layout.name.clone(),
                    })?;

                let size = A2lValue::datatype_size(datatype);
                let bytes = self.hex.read_bytes(v.address, size)?;
                let raw = A2lValue::from_bytes(datatype, &bytes)
                    .ok_or_else(|| ExtractError::Hex(HexError::AddressNotFound {
                        address: v.address,
                        length: size,
                    }))?;
                let physical = self.convert_value(&raw, &v.conversion)?;

                Ok(ExtractedValue {
                    name: v.name,
                    raw,
                    physical,
                    unit: v.unit,
                })
            }
            _ => Err(ExtractError::Resolve(ResolveError::WrongType {
                name: name.to_string(),
                expected: "Value",
                actual: format!("{resolved:?}"),
            })),
        }
    }

    // ====================================================================
    // CURVE extraction
    // ====================================================================

    /// Extract a CURVE (1D lookup) characteristic by name.
    pub fn extract_curve(&self, name: &str) -> Result<ExtractedCurve, ExtractError> {
        let resolved = self.resolver.resolve_characteristic(name)?;
        match resolved {
            ResolvedCharacteristic::Curve(c) => {
                let datatype = c
                    .layout
                    .fnc_values_datatype
                    .as_ref()
                    .ok_or_else(|| ExtractError::NoFncValues {
                        layout: c.layout.name.clone(),
                    })?;

                // Reject alternate index modes (interleaved data layouts)
                if let Some(mode) = &c.layout.index_mode {
                    match mode {
                        IndexMode::RowDir | IndexMode::ColumnDir => {} // both equivalent for 1D
                        other => {
                            return Err(ExtractError::UnsupportedIndexMode {
                                layout: c.layout.name.clone(),
                                mode: format!("{other:?}"),
                            });
                        }
                    }
                }

                let x_count = self.axis_point_count(&c.x_axis)?;
                let x_axis = self.read_axis_values(&c.x_axis, x_count)?;
                let x_physical = self.convert_axis_values(&x_axis, &c.x_axis.conversion)?;

                let elem_size = A2lValue::datatype_size(datatype);
                let total_size = x_count * elem_size;
                let bytes = self.hex.read_bytes(c.address, total_size)?;

                let mut values = Vec::with_capacity(x_count);
                for i in 0..x_count {
                    let raw = A2lValue::from_bytes(datatype, &bytes[i * elem_size..])
                        .ok_or_else(|| ExtractError::Hex(HexError::AddressNotFound {
                            address: c.address + (i * elem_size) as u32,
                            length: elem_size,
                        }))?;
                    values.push(self.convert_value(&raw, &c.conversion)?);
                }

                Ok(ExtractedCurve {
                    name: c.name,
                    x_axis: x_physical,
                    x_unit: c.x_axis.unit,
                    values,
                    unit: c.unit,
                })
            }
            _ => Err(ExtractError::Resolve(ResolveError::WrongType {
                name: name.to_string(),
                expected: "Curve",
                actual: format!("{resolved:?}"),
            })),
        }
    }

    // ====================================================================
    // MAP extraction
    // ====================================================================

    /// Extract a MAP (2D lookup) characteristic by name.
    pub fn extract_map(&self, name: &str) -> Result<ExtractedMap, ExtractError> {
        let resolved = self.resolver.resolve_characteristic(name)?;
        match resolved {
            ResolvedCharacteristic::Map(m) => {
                let datatype = m
                    .layout
                    .fnc_values_datatype
                    .as_ref()
                    .ok_or_else(|| ExtractError::NoFncValues {
                        layout: m.layout.name.clone(),
                    })?;

                let x_count = self.axis_point_count(&m.x_axis)?;
                let y_count = self.axis_point_count(&m.y_axis)?;
                let x_axis = self.read_axis_values(&m.x_axis, x_count)?;
                let y_axis = self.read_axis_values(&m.y_axis, y_count)?;
                let x_physical = self.convert_axis_values(&x_axis, &m.x_axis.conversion)?;
                let y_physical = self.convert_axis_values(&y_axis, &m.y_axis.conversion)?;

                let elem_size = A2lValue::datatype_size(datatype);
                let total_size = x_count * y_count * elem_size;
                let bytes = self.hex.read_bytes(m.address, total_size)?;

                let index_mode = m.layout.index_mode.as_ref()
                    .unwrap_or(&IndexMode::RowDir);

                let values = match index_mode {
                    IndexMode::RowDir => {
                        // Row-major: val[y][x] = byte[(y * x_count + x) * elem_size]
                        self.read_2d_values(
                            &bytes, datatype, elem_size,
                            x_count, y_count, &m.conversion,
                            |x, y| (y * x_count + x) * elem_size,
                        )?
                    }
                    IndexMode::ColumnDir => {
                        // Column-major: val[y][x] = byte[(x * y_count + y) * elem_size]
                        self.read_2d_values(
                            &bytes, datatype, elem_size,
                            x_count, y_count, &m.conversion,
                            |x, y| (x * y_count + y) * elem_size,
                        )?
                    }
                    other => {
                        return Err(ExtractError::UnsupportedIndexMode {
                            layout: m.layout.name.clone(),
                            mode: format!("{other:?}"),
                        });
                    }
                };

                Ok(ExtractedMap {
                    name: m.name,
                    x_axis: x_physical,
                    x_unit: m.x_axis.unit,
                    y_axis: y_physical,
                    y_unit: m.y_axis.unit,
                    values,
                    unit: m.unit,
                })
            }
            _ => Err(ExtractError::Resolve(ResolveError::WrongType {
                name: name.to_string(),
                expected: "Map",
                actual: format!("{resolved:?}"),
            })),
        }
    }

    // ====================================================================
    // VAL_BLK extraction
    // ====================================================================

    /// Extract a VAL_BLK (1D array) characteristic by name.
    pub fn extract_val_blk(&self, name: &str) -> Result<ExtractedValBlk, ExtractError> {
        let resolved = self.resolver.resolve_characteristic(name)?;
        match resolved {
            ResolvedCharacteristic::ValBlk(vb) => {
                let datatype = vb
                    .layout
                    .fnc_values_datatype
                    .as_ref()
                    .ok_or_else(|| ExtractError::NoFncValues {
                        layout: vb.layout.name.clone(),
                    })?;

                let elem_size = A2lValue::datatype_size(datatype);
                let total_size = elem_size * vb.count as usize;
                let bytes = self.hex.read_bytes(vb.address, total_size)?;

                let mut values = Vec::with_capacity(vb.count as usize);
                for i in 0..vb.count as usize {
                    let offset = i * elem_size;
                    let raw = A2lValue::from_bytes(datatype, &bytes[offset..])
                        .ok_or_else(|| ExtractError::Hex(HexError::AddressNotFound {
                            address: vb.address + offset as u32,
                            length: elem_size,
                        }))?;
                    values.push(self.convert_value(&raw, &vb.conversion)?);
                }

                Ok(ExtractedValBlk {
                    name: vb.name,
                    values,
                    unit: vb.unit,
                })
            }
            _ => Err(ExtractError::Resolve(ResolveError::WrongType {
                name: name.to_string(),
                expected: "ValBlk",
                actual: format!("{resolved:?}"),
            })),
        }
    }

    // ====================================================================
    // ASCII extraction
    // ====================================================================

    /// Extract an ASCII (string) characteristic by name.
    pub fn extract_ascii(&self, name: &str) -> Result<ExtractedAscii, ExtractError> {
        let resolved = self.resolver.resolve_characteristic(name)?;
        match resolved {
            ResolvedCharacteristic::Ascii(a) => {
                let bytes = self.hex.read_bytes(a.address, a.length as usize)?;
                // Strip trailing NULs, then interpret as UTF-8 (lossy)
                let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
                let text = String::from_utf8_lossy(&bytes[..end]).into_owned();

                Ok(ExtractedAscii {
                    name: a.name,
                    text,
                })
            }
            _ => Err(ExtractError::Resolve(ResolveError::WrongType {
                name: name.to_string(),
                expected: "Ascii",
                actual: format!("{resolved:?}"),
            })),
        }
    }

    // ====================================================================
    // Measurement guard
    // ====================================================================

    /// Attempt to extract a measurement value — always fails because
    /// measurements are RAM variables not present in flash HEX files.
    pub fn extract_measurement(&self, name: &str) -> Result<ExtractedValue, ExtractError> {
        Err(self
            .resolver
            .read_measurement_from_hex(name, self.hex)
            .unwrap_err()
            .into())
    }

    // ====================================================================
    // Internal helpers
    // ====================================================================

    /// Read a 2D grid of values from flat bytes using a custom offset function.
    fn read_2d_values(
        &self,
        bytes: &[u8],
        datatype: &a2lfile::DataType,
        elem_size: usize,
        x_count: usize,
        y_count: usize,
        conversion: &str,
        offset_fn: impl Fn(usize, usize) -> usize,
    ) -> Result<Vec<Vec<PhysicalValue>>, ExtractError> {
        let mut values = Vec::with_capacity(y_count);
        for y in 0..y_count {
            let mut row = Vec::with_capacity(x_count);
            for x in 0..x_count {
                let offset = offset_fn(x, y);
                let raw = A2lValue::from_bytes(datatype, &bytes[offset..])
                    .ok_or_else(|| ExtractError::Hex(HexError::AddressNotFound {
                        address: offset as u32,
                        length: elem_size,
                    }))?;
                row.push(self.convert_value(&raw, conversion)?);
            }
            values.push(row);
        }
        Ok(values)
    }

    /// Convert a raw A2lValue using the named COMPU_METHOD.
    fn convert_value(
        &self,
        raw: &A2lValue,
        compu_method_name: &str,
    ) -> Result<PhysicalValue, ExtractError> {
        // Try verbal conversion first
        if let Some(label) =
            compu_method::convert_raw_to_string(raw, compu_method_name, self.module)?
        {
            return Ok(PhysicalValue::Verbal(label));
        }
        // Numeric conversion
        let phys = compu_method::convert_raw_to_physical(raw, compu_method_name, self.module)?;
        Ok(PhysicalValue::Numeric(phys))
    }

    /// Determine the number of axis points for a resolved axis.
    fn axis_point_count(&self, axis: &ResolvedAxis) -> Result<usize, ExtractError> {
        match &axis.source {
            AxisSource::FixAxisPar { count, .. } => Ok(*count as usize),
            AxisSource::FixAxisParList { values } => Ok(values.len()),
            AxisSource::ComAxis {
                max_axis_points, ..
            } => Ok(*max_axis_points as usize),
            AxisSource::StdAxis {
                max_axis_points, ..
            } => Ok(*max_axis_points as usize),
        }
    }

    /// Read axis breakpoint values as raw f64 values (before COMPU_METHOD conversion).
    fn read_axis_values(
        &self,
        axis: &ResolvedAxis,
        count: usize,
    ) -> Result<Vec<f64>, ExtractError> {
        match &axis.source {
            AxisSource::FixAxisPar {
                offset, shift, count: n,
            } => Ok(Resolver::compute_fix_axis_par_values(*offset, *shift, *n)),
            AxisSource::FixAxisParList { values } => Ok(values.clone()),
            AxisSource::ComAxis {
                axis_pts_address,
                deposit_name,
                ..
            } => self.read_axis_pts_data(*axis_pts_address, deposit_name, count),
            AxisSource::StdAxis { .. } => {
                // STD_AXIS: axis breakpoints embedded in the characteristic record.
                // This requires knowing the exact byte layout — not yet implemented.
                // For now, return placeholder indices.
                Ok((0..count).map(|i| i as f64).collect())
            }
        }
    }

    /// Read AXIS_PTS breakpoints from HEX at the given address using the deposit layout.
    fn read_axis_pts_data(
        &self,
        address: u32,
        deposit_name: &str,
        count: usize,
    ) -> Result<Vec<f64>, ExtractError> {
        let rl = self
            .module
            .record_layout
            .iter()
            .find(|r| r.get_name() == deposit_name)
            .ok_or_else(|| {
                ExtractError::Resolve(ResolveError::NotFound {
                    kind: "RecordLayout",
                    name: deposit_name.to_string(),
                })
            })?;

        let axis_type = rl
            .axis_pts_x
            .as_ref()
            .map(|ax| &ax.datatype)
            .ok_or_else(|| ExtractError::NoAxisPtsX {
                layout: deposit_name.to_string(),
            })?;

        let elem_size = A2lValue::datatype_size(axis_type);
        let total_size = count * elem_size;
        let bytes = self.hex.read_bytes(address, total_size)?;

        let mut values = Vec::with_capacity(count);
        for i in 0..count {
            let raw = A2lValue::from_bytes(axis_type, &bytes[i * elem_size..])
                .unwrap_or(A2lValue::F64(0.0));
            let val = raw.as_f64().unwrap_or(0.0);
            values.push(val);
        }

        Ok(values)
    }

    /// Convert a vector of raw axis values using the named COMPU_METHOD.
    fn convert_axis_values(
        &self,
        raw_values: &[f64],
        compu_method_name: &str,
    ) -> Result<Vec<f64>, ExtractError> {
        raw_values
            .iter()
            .map(|&raw| {
                let raw_val = A2lValue::F64(raw);
                let phys =
                    compu_method::convert_raw_to_physical(&raw_val, compu_method_name, self.module)?;
                Ok(phys)
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

    #[test]
    fn physical_value_numeric() {
        let v = PhysicalValue::Numeric(42.0);
        assert_eq!(v.as_f64(), Some(42.0));
        assert_eq!(v.as_str(), None);
    }

    #[test]
    fn physical_value_verbal() {
        let v = PhysicalValue::Verbal("ON".into());
        assert_eq!(v.as_f64(), None);
        assert_eq!(v.as_str(), Some("ON"));
    }

    #[test]
    fn extract_error_display() {
        let e = ExtractError::NoFncValues {
            layout: "rl_test".into(),
        };
        assert!(format!("{e}").contains("rl_test"));

        let e = ExtractError::NoAxisPtsX {
            layout: "rl_axis".into(),
        };
        assert!(format!("{e}").contains("rl_axis"));
    }

    #[test]
    fn extract_error_from_resolve() {
        let re = ResolveError::NotFound {
            kind: "Characteristic",
            name: "foo".into(),
        };
        let ee: ExtractError = re.into();
        assert!(format!("{ee}").contains("foo"));
    }

    #[test]
    fn extract_error_from_hex() {
        let he = HexError::AddressNotFound {
            address: 0x1000,
            length: 4,
        };
        let ee: ExtractError = he.into();
        assert!(format!("{ee}").contains("1000"));
    }

    #[test]
    fn unsupported_index_mode_error() {
        let e = ExtractError::UnsupportedIndexMode {
            layout: "rl_test".into(),
            mode: "AlternateCurves".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("AlternateCurves"));
        assert!(msg.contains("rl_test"));
    }

    #[test]
    fn row_dir_offset_calculation() {
        // 3x2 map (3 columns, 2 rows), 4 bytes each
        let x_count = 3;
        let y_count = 2;
        let elem_size = 4;
        let row_dir = |x: usize, y: usize| (y * x_count + x) * elem_size;

        // Row 0: [0, 4, 8]
        assert_eq!(row_dir(0, 0), 0);
        assert_eq!(row_dir(1, 0), 4);
        assert_eq!(row_dir(2, 0), 8);
        // Row 1: [12, 16, 20]
        assert_eq!(row_dir(0, 1), 12);
        assert_eq!(row_dir(1, 1), 16);
        assert_eq!(row_dir(2, 1), 20);
    }

    #[test]
    fn column_dir_offset_calculation() {
        // 3x2 map (3 columns, 2 rows), 4 bytes each
        let x_count = 3;
        let y_count = 2;
        let elem_size = 4;
        let col_dir = |x: usize, y: usize| (x * y_count + y) * elem_size;

        // Col 0: y=0 at 0, y=1 at 4
        assert_eq!(col_dir(0, 0), 0);
        assert_eq!(col_dir(0, 1), 4);
        // Col 1: y=0 at 8, y=1 at 12
        assert_eq!(col_dir(1, 0), 8);
        assert_eq!(col_dir(1, 1), 12);
        // Col 2: y=0 at 16, y=1 at 20
        assert_eq!(col_dir(2, 0), 16);
        assert_eq!(col_dir(2, 1), 20);
    }
}
