/// Comprehensive value type enum covering all possible A2L data types.
/// Preserves type fidelity from the A2L RECORD_LAYOUT and DataType definitions.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum A2lValue {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
    String(String),
    /// 1D array of homogeneous values
    Array(Vec<A2lValue>),
    /// 2D array (row-major) for MAP types
    Array2D {
        rows: usize,
        cols: usize,
        data: Vec<A2lValue>,
    },
}

impl A2lValue {
    /// Convert this value to f64 for computation (e.g., COMPU_METHOD application).
    /// Returns None for String, Array, and Array2D variants.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            A2lValue::U8(v) => Some(*v as f64),
            A2lValue::I8(v) => Some(*v as f64),
            A2lValue::U16(v) => Some(*v as f64),
            A2lValue::I16(v) => Some(*v as f64),
            A2lValue::U32(v) => Some(*v as f64),
            A2lValue::I32(v) => Some(*v as f64),
            A2lValue::U64(v) => Some(*v as f64),
            A2lValue::I64(v) => Some(*v as f64),
            A2lValue::F32(v) => Some(*v as f64),
            A2lValue::F64(v) => Some(*v),
            A2lValue::String(_) | A2lValue::Array(_) | A2lValue::Array2D { .. } => None,
        }
    }

    /// Create an A2lValue from a DataType enum and raw bytes.
    /// Returns None if the byte slice is too short.
    pub fn from_bytes(datatype: &a2lfile::DataType, bytes: &[u8]) -> Option<Self> {
        use a2lfile::DataType;
        match datatype {
            DataType::Ubyte => bytes.first().map(|&b| A2lValue::U8(b)),
            DataType::Sbyte => bytes.first().map(|&b| A2lValue::I8(b as i8)),
            DataType::Uword => bytes
                .get(..2)
                .map(|b| A2lValue::U16(u16::from_le_bytes([b[0], b[1]]))),
            DataType::Sword => bytes
                .get(..2)
                .map(|b| A2lValue::I16(i16::from_le_bytes([b[0], b[1]]))),
            DataType::Ulong => bytes
                .get(..4)
                .map(|b| A2lValue::U32(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
            DataType::Slong => bytes
                .get(..4)
                .map(|b| A2lValue::I32(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
            DataType::AUint64 => bytes
                .get(..8)
                .map(|b| A2lValue::U64(u64::from_le_bytes(b.try_into().unwrap()))),
            DataType::AInt64 => bytes
                .get(..8)
                .map(|b| A2lValue::I64(i64::from_le_bytes(b.try_into().unwrap()))),
            DataType::Float16Ieee => {
                // f16 not natively supported; store as f32 after conversion
                bytes.get(..2).map(|b| {
                    let half = u16::from_le_bytes([b[0], b[1]]);
                    A2lValue::F32(f16_to_f32(half))
                })
            }
            DataType::Float32Ieee => bytes
                .get(..4)
                .map(|b| A2lValue::F32(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
            DataType::Float64Ieee => bytes
                .get(..8)
                .map(|b| A2lValue::F64(f64::from_le_bytes(b.try_into().unwrap()))),
        }
    }

    /// Returns the byte size of a DataType.
    pub fn datatype_size(datatype: &a2lfile::DataType) -> usize {
        use a2lfile::DataType;
        match datatype {
            DataType::Ubyte | DataType::Sbyte => 1,
            DataType::Uword | DataType::Sword | DataType::Float16Ieee => 2,
            DataType::Ulong | DataType::Slong | DataType::Float32Ieee => 4,
            DataType::AUint64 | DataType::AInt64 | DataType::Float64Ieee => 8,
        }
    }
}

/// IEEE 754 half-precision (binary16) to single-precision (binary32) conversion.
fn f16_to_f32(half: u16) -> f32 {
    let sign = ((half >> 15) & 1) as u32;
    let exponent = ((half >> 10) & 0x1F) as u32;
    let mantissa = (half & 0x3FF) as u32;

    if exponent == 0 {
        if mantissa == 0 {
            // Zero
            f32::from_bits(sign << 31)
        } else {
            // Subnormal f16 → normalized f32
            let mut m = mantissa;
            let mut e: i32 = -14;
            while (m & 0x400) == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x3FF;
            let f32_exp = (e + 127) as u32;
            f32::from_bits((sign << 31) | (f32_exp << 23) | (m << 13))
        }
    } else if exponent == 31 {
        // Inf or NaN
        let f32_mantissa = mantissa << 13;
        f32::from_bits((sign << 31) | (0xFF << 23) | f32_mantissa)
    } else {
        // Normalized
        let f32_exp = (exponent as i32 - 15 + 127) as u32;
        f32::from_bits((sign << 31) | (f32_exp << 23) | (mantissa << 13))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2lfile::DataType;

    #[test]
    fn value_u8_roundtrip() {
        let val = A2lValue::from_bytes(&DataType::Ubyte, &[0xFF]).unwrap();
        assert_eq!(val, A2lValue::U8(255));
        assert_eq!(val.as_f64(), Some(255.0));
    }

    #[test]
    fn value_i8_negative() {
        let val = A2lValue::from_bytes(&DataType::Sbyte, &[0x80]).unwrap();
        assert_eq!(val, A2lValue::I8(-128));
        assert_eq!(val.as_f64(), Some(-128.0));
    }

    #[test]
    fn value_u16_le() {
        let val = A2lValue::from_bytes(&DataType::Uword, &[0x01, 0x00]).unwrap();
        assert_eq!(val, A2lValue::U16(1));
    }

    #[test]
    fn value_i16_negative() {
        let bytes = (-1000i16).to_le_bytes();
        let val = A2lValue::from_bytes(&DataType::Sword, &bytes).unwrap();
        assert_eq!(val, A2lValue::I16(-1000));
    }

    #[test]
    fn value_u32_le() {
        let bytes = 100_000u32.to_le_bytes();
        let val = A2lValue::from_bytes(&DataType::Ulong, &bytes).unwrap();
        assert_eq!(val, A2lValue::U32(100_000));
    }

    #[test]
    fn value_i32_negative() {
        let bytes = (-50_000i32).to_le_bytes();
        let val = A2lValue::from_bytes(&DataType::Slong, &bytes).unwrap();
        assert_eq!(val, A2lValue::I32(-50_000));
    }

    #[test]
    fn value_u64() {
        let bytes = u64::MAX.to_le_bytes();
        let val = A2lValue::from_bytes(&DataType::AUint64, &bytes).unwrap();
        assert_eq!(val, A2lValue::U64(u64::MAX));
    }

    #[test]
    fn value_i64_negative() {
        let bytes = i64::MIN.to_le_bytes();
        let val = A2lValue::from_bytes(&DataType::AInt64, &bytes).unwrap();
        assert_eq!(val, A2lValue::I64(i64::MIN));
    }

    #[test]
    fn value_f32() {
        let bytes = std::f32::consts::PI.to_le_bytes();
        let val = A2lValue::from_bytes(&DataType::Float32Ieee, &bytes).unwrap();
        assert_eq!(val, A2lValue::F32(std::f32::consts::PI));
        assert!((val.as_f64().unwrap() - std::f64::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn value_f64() {
        let bytes = std::f64::consts::E.to_le_bytes();
        let val = A2lValue::from_bytes(&DataType::Float64Ieee, &bytes).unwrap();
        assert_eq!(val, A2lValue::F64(std::f64::consts::E));
    }

    #[test]
    fn value_f16_one() {
        // IEEE 754 half-precision 1.0 = 0x3C00
        let val = A2lValue::from_bytes(&DataType::Float16Ieee, &[0x00, 0x3C]).unwrap();
        if let A2lValue::F32(f) = val {
            assert!((f - 1.0).abs() < 1e-6, "expected 1.0, got {f}");
        } else {
            panic!("expected F32 variant");
        }
    }

    #[test]
    fn value_f16_zero() {
        let val = A2lValue::from_bytes(&DataType::Float16Ieee, &[0x00, 0x00]).unwrap();
        assert_eq!(val, A2lValue::F32(0.0));
    }

    #[test]
    fn value_from_bytes_too_short() {
        assert!(A2lValue::from_bytes(&DataType::Uword, &[0x01]).is_none());
        assert!(A2lValue::from_bytes(&DataType::Ulong, &[0x01, 0x02]).is_none());
        assert!(A2lValue::from_bytes(&DataType::Float64Ieee, &[0; 4]).is_none());
    }

    #[test]
    fn value_string_no_f64() {
        let val = A2lValue::String("hello".to_string());
        assert_eq!(val.as_f64(), None);
    }

    #[test]
    fn value_array_no_f64() {
        let val = A2lValue::Array(vec![A2lValue::U8(1), A2lValue::U8(2)]);
        assert_eq!(val.as_f64(), None);
    }

    #[test]
    fn datatype_sizes() {
        assert_eq!(A2lValue::datatype_size(&DataType::Ubyte), 1);
        assert_eq!(A2lValue::datatype_size(&DataType::Sbyte), 1);
        assert_eq!(A2lValue::datatype_size(&DataType::Uword), 2);
        assert_eq!(A2lValue::datatype_size(&DataType::Sword), 2);
        assert_eq!(A2lValue::datatype_size(&DataType::Float16Ieee), 2);
        assert_eq!(A2lValue::datatype_size(&DataType::Ulong), 4);
        assert_eq!(A2lValue::datatype_size(&DataType::Slong), 4);
        assert_eq!(A2lValue::datatype_size(&DataType::Float32Ieee), 4);
        assert_eq!(A2lValue::datatype_size(&DataType::AUint64), 8);
        assert_eq!(A2lValue::datatype_size(&DataType::AInt64), 8);
        assert_eq!(A2lValue::datatype_size(&DataType::Float64Ieee), 8);
    }
}
