//! Integration tests using the real sample A2L file.
//!
//! These tests require `refs/zc-blanc_rear_c-target-xcp.a2l` to be present.

use a2lfile::*;
use a2ldeser::compu_method::*;
use a2ldeser::types::A2lValue;
use std::sync::OnceLock;

/// Lazily load the A2L file once for all tests.
fn sample_a2l() -> &'static A2lFile {
    static A2L: OnceLock<A2lFile> = OnceLock::new();
    A2L.get_or_init(|| {
        let (a2l, _) = a2lfile::load(
            &std::ffi::OsString::from("refs/zc-blanc_rear_c-target-xcp.a2l"),
            None,
            false,
        )
        .expect("could not load sample A2L file");
        a2l
    })
}

fn module() -> &'static Module {
    &sample_a2l().project.module[0]
}

// ========================================================================
// Loading & Structure Tests
// ========================================================================

#[test]
fn loads_without_panic() {
    let _ = sample_a2l();
}

#[test]
fn has_expected_object_counts() {
    let m = module();
    assert_eq!(m.measurement.len(), 6519);
    assert_eq!(m.characteristic.len(), 10751);
    assert_eq!(m.compu_method.len(), 995);
    assert_eq!(m.compu_vtab.len(), 432);
    assert_eq!(m.record_layout.len(), 51);
    assert_eq!(m.axis_pts.len(), 191);
}

#[test]
fn characteristic_type_breakdown() {
    let m = module();
    let values = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Value)
        .count();
    let curves = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Curve)
        .count();
    let maps = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Map)
        .count();
    let val_blks = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::ValBlk)
        .count();
    let ascii = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Ascii)
        .count();

    assert_eq!(values, 9374);
    assert_eq!(curves, 355);
    assert_eq!(maps, 344);
    assert_eq!(val_blks, 673);
    assert_eq!(ascii, 5);
}

// ========================================================================
// Measurement Access Tests
// ========================================================================

#[test]
fn measurement_by_name_exists() {
    let m = module();
    let meas = m.measurement.iter()
        .find(|m| m.get_name() == "g_ethxcp_tx_counter");
    assert!(meas.is_some(), "expected measurement g_ethxcp_tx_counter");

    let meas = meas.unwrap();
    assert_eq!(meas.datatype, DataType::Uword);
    assert_eq!(meas.conversion, "NO_COMPU_METHOD");
    assert_eq!(meas.lower_limit, 0.0);
    assert_eq!(meas.upper_limit, 65535.0);
}

#[test]
fn measurement_ecu_address() {
    let m = module();
    let meas = m.measurement.iter()
        .find(|m| m.get_name() == "g_ethxcp_tx_counter")
        .unwrap();
    assert_eq!(meas.ecu_address.as_ref().unwrap().address, 0x9000B432);
}

// ========================================================================
// Characteristic Access Tests
// ========================================================================

#[test]
fn value_characteristic_lookup() {
    let m = module();
    let ch = m.characteristic.iter()
        .find(|c| c.get_name() == "g_xcp_enable_status")
        .expect("expected characteristic g_xcp_enable_status");

    assert_eq!(ch.characteristic_type, CharacteristicType::Value);
    assert_eq!(ch.deposit, "Scalar_ULONG");
    assert_eq!(ch.conversion, "NO_COMPU_METHOD");
    assert_eq!(ch.lower_limit, 0.0);
    assert_eq!(ch.upper_limit, 1.0);
}

#[test]
fn curve_has_one_axis_descriptor() {
    let m = module();
    let curve = m.characteristic.iter()
        .find(|c| {
            c.characteristic_type == CharacteristicType::Curve
                && c.get_name() == "SnrDE_AsuT_tTransf_CUR_x"
        })
        .expect("expected CURVE SnrDE_AsuT_tTransf_CUR_x");

    assert_eq!(curve.axis_descr.len(), 1, "CURVE should have exactly 1 axis descriptor");
    assert_eq!(curve.deposit, "Lookup1D_FLOAT32_IEEE");

    let axis = &curve.axis_descr[0];
    assert_eq!(axis.attribute, AxisDescrAttribute::FixAxis);
    assert_eq!(axis.max_axis_points, 191);
    assert!(axis.fix_axis_par.is_some());

    let fap = axis.fix_axis_par.as_ref().unwrap();
    assert_eq!(fap.offset, 0.0);
    assert_eq!(fap.shift, 0.0);
    assert_eq!(fap.number_apo, 191);
}

#[test]
fn map_has_two_axis_descriptors() {
    let m = module();
    let map = m.characteristic.iter()
        .find(|c| {
            c.characteristic_type == CharacteristicType::Map
                && c.get_name() == "LsDut_tThrdLeOffsetDAT_MAP_v"
        })
        .expect("expected MAP LsDut_tThrdLeOffsetDAT_MAP_v");

    assert_eq!(map.axis_descr.len(), 2, "MAP should have exactly 2 axis descriptors");
    assert_eq!(map.deposit, "Lookup2D_FLOAT32_IEEE");

    // Both axes are ComAxis with 8 points
    assert_eq!(map.axis_descr[0].attribute, AxisDescrAttribute::ComAxis);
    assert_eq!(map.axis_descr[0].max_axis_points, 8);
    assert_eq!(map.axis_descr[1].attribute, AxisDescrAttribute::ComAxis);
    assert_eq!(map.axis_descr[1].max_axis_points, 8);
}

#[test]
fn map_asymmetric_axes() {
    let m = module();
    let map = m.characteristic.iter()
        .find(|c| c.get_name() == "LsDut_tThrdLeBlwPctDatOffset_MAP_v")
        .expect("expected MAP with asymmetric axes");

    assert_eq!(map.axis_descr[0].max_axis_points, 8);
    assert_eq!(map.axis_descr[1].max_axis_points, 7);
}

// ========================================================================
// CompuMethod Resolution Tests
// ========================================================================

#[test]
fn compu_method_identical_exists() {
    let m = module();
    let cm = m.compu_method.iter()
        .find(|cm| cm.conversion_type == ConversionType::Identical);
    assert!(cm.is_some(), "expected at least one IDENTICAL CompuMethod");
}

#[test]
fn compu_method_rat_func_identity_coeffs() {
    let m = module();
    // xcp_cal_map_table_RAT_FUNC has b=1, f=1 (identity pattern)
    let cm = m.compu_method.iter()
        .find(|cm| cm.get_name() == "xcp_cal_map_table_RAT_FUNC")
        .expect("expected xcp_cal_map_table_RAT_FUNC");

    assert_eq!(cm.conversion_type, ConversionType::RatFunc);
    let co = cm.coeffs.as_ref().unwrap();
    assert_eq!((co.a, co.b, co.c, co.d, co.e, co.f), (0.0, 1.0, 0.0, 0.0, 0.0, 1.0));
}

#[test]
fn compu_method_rat_func_scale_by_3() {
    let m = module();
    let cm = m.compu_method.iter()
        .find(|cm| cm.get_name() == "fct_timestamps_param_CM_uint32")
        .expect("expected fct_timestamps_param_CM_uint32");

    assert_eq!(cm.conversion_type, ConversionType::RatFunc);
    let co = cm.coeffs.as_ref().unwrap();
    // a=0, b=3, c=0, d=0, e=0, f=1 → internal = 3*phys → phys = internal/3
    assert_eq!((co.a, co.b, co.c, co.d, co.e, co.f), (0.0, 3.0, 0.0, 0.0, 0.0, 1.0));
}

#[test]
fn compu_method_tab_verb_links_to_vtab() {
    let m = module();
    let cm = m.compu_method.iter()
        .find(|cm| cm.get_name() == "ASCIICoding")
        .expect("expected ASCIICoding CompuMethod");

    assert_eq!(cm.conversion_type, ConversionType::TabVerb);
    let tab_ref = cm.compu_tab_ref.as_ref().unwrap();
    assert_eq!(tab_ref.conversion_table, "ASCIICodingVT");

    // Verify the referenced VTAB exists
    let vtab = m.compu_vtab.iter()
        .find(|v| v.get_name() == "ASCIICodingVT");
    assert!(vtab.is_some(), "referenced CompuVtab ASCIICodingVT should exist");
    assert_eq!(vtab.unwrap().number_value_pairs, 37);
}

// ========================================================================
// CompuVtab Tests
// ========================================================================

#[test]
fn compu_vtab_fault_disable() {
    let m = module();
    let vtab = m.compu_vtab.iter()
        .find(|v| v.get_name() == "VTAB_FOR_CM_fault_dis")
        .expect("expected VTAB_FOR_CM_fault_dis");

    assert_eq!(vtab.number_value_pairs, 2);
    assert_eq!(vtab.value_pairs[0].in_val, 0.0);
    assert_eq!(vtab.value_pairs[0].out_val, "Enable");
    assert_eq!(vtab.value_pairs[1].in_val, 1.0);
    assert_eq!(vtab.value_pairs[1].out_val, "Disable");
}

#[test]
fn convert_tab_verb_with_real_vtab() {
    let m = module();
    let vtab = m.compu_vtab.iter()
        .find(|v| v.get_name() == "VTAB_FOR_CM_fault_dis")
        .unwrap();

    assert_eq!(convert_tab_verb(0.0, vtab).unwrap(), "Enable");
    assert_eq!(convert_tab_verb(1.0, vtab).unwrap(), "Disable");
    assert!(convert_tab_verb(99.0, vtab).is_err());
}

// ========================================================================
// COMPU_METHOD Conversion Integration Tests
// ========================================================================

#[test]
fn convert_no_compu_method_passthrough() {
    let m = module();
    let raw = A2lValue::U16(12345);
    let result = convert_raw_to_physical(&raw, "NO_COMPU_METHOD", m).unwrap();
    assert_eq!(result, 12345.0);
}

#[test]
fn convert_rat_func_identity_via_module() {
    let m = module();
    // xcp_cal_map_table_RAT_FUNC: a=0,b=1,c=0,d=0,e=0,f=1 → phys = raw
    let raw = A2lValue::F32(42.5);
    let result = convert_raw_to_physical(&raw, "xcp_cal_map_table_RAT_FUNC", m).unwrap();
    // phys = (c - raw*f) / (raw*e - b) = (0 - 42.5*1) / (42.5*0 - 1) = -42.5 / -1 = 42.5
    assert!((result - 42.5).abs() < 1e-10);
}

#[test]
fn convert_rat_func_scale_by_3_via_module() {
    let m = module();
    // fct_timestamps_param_CM_uint32: a=0,b=3,c=0,d=0,e=0,f=1 → phys = raw/3
    let raw = A2lValue::U32(900);
    let result = convert_raw_to_physical(&raw, "fct_timestamps_param_CM_uint32", m).unwrap();
    // phys = (0 - 900*1) / (900*0 - 3) = -900 / -3 = 300
    assert!((result - 300.0).abs() < 1e-10);
}

#[test]
fn convert_nonexistent_method_errors() {
    let m = module();
    let raw = A2lValue::U8(0);
    let result = convert_raw_to_physical(&raw, "DOES_NOT_EXIST", m);
    assert!(matches!(result, Err(ConversionError::MethodNotFound(_))));
}

// ========================================================================
// RecordLayout Tests
// ========================================================================

#[test]
fn record_layout_scalar_exists() {
    let m = module();
    let rl = m.record_layout.iter()
        .find(|r| r.get_name() == "Scalar_ULONG");
    assert!(rl.is_some());
    let rl = rl.unwrap();
    assert!(rl.fnc_values.is_some());
    assert!(rl.axis_pts_x.is_none());
}

#[test]
fn record_layout_lookup1d_exists() {
    let m = module();
    let rl = m.record_layout.iter()
        .find(|r| r.get_name() == "Lookup1D_FLOAT32_IEEE");
    assert!(rl.is_some(), "expected Lookup1D_FLOAT32_IEEE record layout");
}

#[test]
fn record_layout_lookup2d_exists() {
    let m = module();
    let rl = m.record_layout.iter()
        .find(|r| r.get_name() == "Lookup2D_FLOAT32_IEEE");
    assert!(rl.is_some(), "expected Lookup2D_FLOAT32_IEEE record layout");
}

// ========================================================================
// Type Enum with Real DataTypes
// ========================================================================

#[test]
fn a2l_value_from_measurement_datatype() {
    let m = module();
    let meas = m.measurement.iter()
        .find(|m| m.get_name() == "g_ethxcp_tx_counter")
        .unwrap();

    // Uword → U16, simulate reading 2 bytes
    let test_bytes = 1234u16.to_le_bytes();
    let val = A2lValue::from_bytes(&meas.datatype, &test_bytes).unwrap();
    assert_eq!(val, A2lValue::U16(1234));
}

#[test]
fn a2l_value_sizes_match_datatypes() {
    // Verify our size calculations match what the A2L file uses
    assert_eq!(A2lValue::datatype_size(&DataType::Ubyte), 1);
    assert_eq!(A2lValue::datatype_size(&DataType::Uword), 2);
    assert_eq!(A2lValue::datatype_size(&DataType::Ulong), 4);
    assert_eq!(A2lValue::datatype_size(&DataType::Float32Ieee), 4);
    assert_eq!(A2lValue::datatype_size(&DataType::Float64Ieee), 8);
}
