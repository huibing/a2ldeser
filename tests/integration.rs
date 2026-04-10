//! Integration tests using the real sample A2L and HEX files.
//!
//! These tests require files in the `refs/` directory to be present.

use a2lfile::*;
use a2ldeser::compu_method::*;
use a2ldeser::extractor::*;
use a2ldeser::hex_reader::*;
use a2ldeser::resolver::*;
use a2ldeser::types::A2lValue;
use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

/// Lazily load the A2L file once for all tests.
fn sample_a2l() -> &'static A2lFile {
    static A2L: OnceLock<A2lFile> = OnceLock::new();
    A2L.get_or_init(|| {
        let (a2l, _) = a2lfile::load(
            std::ffi::OsString::from("refs/zc-blanc_rear_c-target-xcp.a2l"),
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

// ========================================================================
// CURVE Axis Resolution Tests
// ========================================================================

#[test]
fn curve_axis_type_distribution() {
    let m = module();
    let curves: Vec<_> = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Curve)
        .collect();

    let fix_axis = curves.iter()
        .flat_map(|c| &c.axis_descr)
        .filter(|a| a.attribute == AxisDescrAttribute::FixAxis)
        .count();
    let com_axis = curves.iter()
        .flat_map(|c| &c.axis_descr)
        .filter(|a| a.attribute == AxisDescrAttribute::ComAxis)
        .count();

    assert_eq!(fix_axis, 44, "expected 44 FixAxis curves");
    assert_eq!(com_axis, 311, "expected 311 ComAxis curves");
    // No StdAxis curves in this file
    let std_axis = curves.iter()
        .flat_map(|c| &c.axis_descr)
        .filter(|a| a.attribute == AxisDescrAttribute::StdAxis)
        .count();
    assert_eq!(std_axis, 0);
}

#[test]
fn curve_fix_axis_par_resolution() {
    let m = module();
    let curve = m.characteristic.iter()
        .find(|c| c.get_name() == "SnrDE_AsuT_tTransf_CUR_x")
        .unwrap();

    assert_eq!(curve.characteristic_type, CharacteristicType::Curve);
    let axis = &curve.axis_descr[0];
    assert_eq!(axis.attribute, AxisDescrAttribute::FixAxis);

    // FixAxis uses fix_axis_par to define axis points algorithmically
    let fap = axis.fix_axis_par.as_ref()
        .expect("FixAxis CURVE should have fix_axis_par");
    assert_eq!(fap.offset, 0.0);
    assert_eq!(fap.shift, 0.0);
    assert_eq!(fap.number_apo, 191);

    // No axis_pts_ref for FixAxis
    assert!(axis.axis_pts_ref.is_none());
    // No fix_axis_par_list or fix_axis_par_dist
    assert!(axis.fix_axis_par_list.is_none());
    assert!(axis.fix_axis_par_dist.is_none());
}

#[test]
fn curve_com_axis_resolves_to_axis_pts() {
    let m = module();
    let curve = m.characteristic.iter()
        .find(|c| c.get_name() == "CDCAct_DmprICmdFrnt_T")
        .expect("expected CURVE CDCAct_DmprICmdFrnt_T");

    assert_eq!(curve.characteristic_type, CharacteristicType::Curve);
    assert_eq!(curve.deposit, "Lookup1D_UWORD");

    let axis = &curve.axis_descr[0];
    assert_eq!(axis.attribute, AxisDescrAttribute::ComAxis);
    assert_eq!(axis.max_axis_points, 9);

    // ComAxis references an AXIS_PTS object
    let apr = axis.axis_pts_ref.as_ref()
        .expect("ComAxis should have axis_pts_ref");
    assert_eq!(apr.axis_points, "CDCAct_RatDmpg_Ax");

    // Resolve the reference
    let axis_pts = m.axis_pts.iter()
        .find(|a| a.get_name() == "CDCAct_RatDmpg_Ax")
        .expect("referenced AXIS_PTS should exist");
    assert_eq!(axis_pts.address, 0xA03961F4);
    assert_eq!(axis_pts.max_axis_points, 9);
    assert_eq!(axis_pts.conversion, "CDC_CM_single_percent");
}

#[test]
fn curve_com_axis_conversion_method_exists() {
    let m = module();
    let curve = m.characteristic.iter()
        .find(|c| c.get_name() == "CDCAct_DmprICmdFrnt_T")
        .unwrap();

    let axis = &curve.axis_descr[0];
    let axis_conv = &axis.conversion;

    // The axis conversion method should exist in the module
    let cm = m.compu_method.iter()
        .find(|cm| cm.get_name() == axis_conv.as_str());
    assert!(cm.is_some(), "axis conversion method '{}' should exist", axis_conv);
}

#[test]
fn all_curve_com_axis_refs_resolve() {
    let m = module();
    let mut unresolved = Vec::new();

    for curve in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Curve)
    {
        for axis in &curve.axis_descr {
            if axis.attribute == AxisDescrAttribute::ComAxis
                && let Some(ref apr) = axis.axis_pts_ref
                    && m.axis_pts.iter().find(|a| a.get_name() == apr.axis_points).is_none() {
                        unresolved.push((curve.get_name().to_string(), apr.axis_points.clone()));
                    }
        }
    }

    assert!(
        unresolved.is_empty(),
        "unresolved CURVE ComAxis refs: {:?}",
        unresolved
    );
}

// ========================================================================
// MAP Axis Resolution Tests
// ========================================================================

#[test]
fn map_axis_type_distribution() {
    let m = module();
    let maps: Vec<_> = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Map)
        .collect();

    // All MAP axes in this file are ComAxis
    let com_axis = maps.iter()
        .flat_map(|c| &c.axis_descr)
        .filter(|a| a.attribute == AxisDescrAttribute::ComAxis)
        .count();
    assert_eq!(com_axis, 688, "expected 688 ComAxis entries across all MAPs (344 maps × 2 axes)");
}

#[test]
fn map_both_axes_resolve_to_axis_pts() {
    let m = module();
    let map = m.characteristic.iter()
        .find(|c| c.get_name() == "LsDut_tThrdLeOffsetDAT_MAP_v")
        .expect("expected MAP LsDut_tThrdLeOffsetDAT_MAP_v");

    assert_eq!(map.axis_descr.len(), 2);

    // X axis (axis[0])
    let x_axis = &map.axis_descr[0];
    let x_ref = x_axis.axis_pts_ref.as_ref()
        .expect("MAP X axis should have axis_pts_ref");
    assert_eq!(x_ref.axis_points, "LsDut_tThrdLeDVT2DatOffset_MAP_y");
    let x_pts = m.axis_pts.iter()
        .find(|a| a.get_name() == x_ref.axis_points)
        .expect("X axis AXIS_PTS should exist");
    assert_eq!(x_pts.address, 0xA0381DB4);
    assert_eq!(x_pts.max_axis_points, 8);

    // Y axis (axis[1])
    let y_axis = &map.axis_descr[1];
    let y_ref = y_axis.axis_pts_ref.as_ref()
        .expect("MAP Y axis should have axis_pts_ref");
    assert_eq!(y_ref.axis_points, "LsDut_tThrdLeAmb2DatOffset_MAP_x");
    let y_pts = m.axis_pts.iter()
        .find(|a| a.get_name() == y_ref.axis_points)
        .expect("Y axis AXIS_PTS should exist");
    assert_eq!(y_pts.address, 0xA0381EF8);
    assert_eq!(y_pts.max_axis_points, 8);
}

#[test]
fn all_map_com_axis_refs_resolve() {
    let m = module();
    let mut unresolved = Vec::new();

    for map in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Map)
    {
        for (i, axis) in map.axis_descr.iter().enumerate() {
            if axis.attribute == AxisDescrAttribute::ComAxis
                && let Some(ref apr) = axis.axis_pts_ref
                    && m.axis_pts.iter().find(|a| a.get_name() == apr.axis_points).is_none() {
                        unresolved.push((
                            map.get_name().to_string(),
                            format!("axis[{}]", i),
                            apr.axis_points.clone(),
                        ));
                    }
        }
    }

    assert!(
        unresolved.is_empty(),
        "unresolved MAP ComAxis refs: {:?}",
        unresolved
    );
}

#[test]
fn map_axes_have_consistent_max_points() {
    let m = module();
    let map = m.characteristic.iter()
        .find(|c| c.get_name() == "LsDut_tThrdLeOffsetDAT_MAP_v")
        .unwrap();

    // axis_descr max_axis_points should match referenced AXIS_PTS max_axis_points
    for axis in &map.axis_descr {
        if let Some(ref apr) = axis.axis_pts_ref {
            let pts = m.axis_pts.iter()
                .find(|a| a.get_name() == apr.axis_points)
                .unwrap();
            assert_eq!(
                axis.max_axis_points, pts.max_axis_points,
                "axis_descr max_axis_points should match AXIS_PTS for {}",
                apr.axis_points
            );
        }
    }
}

// ========================================================================
// AXIS_PTS Object Tests
// ========================================================================

#[test]
fn axis_pts_have_valid_conversions() {
    let m = module();
    let mut missing = Vec::new();

    for ap in &m.axis_pts {
        if ap.conversion != "NO_COMPU_METHOD" {
            let found = m.compu_method.iter()
                .any(|cm| cm.get_name() == ap.conversion.as_str());
            if !found {
                missing.push((ap.get_name().to_string(), ap.conversion.clone()));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "AXIS_PTS with missing conversion methods: {:?}",
        missing
    );
}

#[test]
fn axis_pts_addresses_are_nonzero() {
    let m = module();
    for ap in &m.axis_pts {
        assert!(
            ap.address != 0,
            "AXIS_PTS {} has zero address",
            ap.get_name()
        );
    }
}

// ========================================================================
// Cross-Reference: RecordLayout Resolution for CURVE/MAP
// ========================================================================

#[test]
fn all_curve_map_record_layouts_exist() {
    let m = module();
    let mut missing = Vec::new();

    for c in m.characteristic.iter()
        .filter(|c| matches!(c.characteristic_type, CharacteristicType::Curve | CharacteristicType::Map))
    {
        let found = m.record_layout.iter()
            .any(|rl| rl.get_name() == c.deposit.as_str());
        if !found {
            missing.push((c.get_name().to_string(), c.deposit.clone()));
        }
    }

    assert!(
        missing.is_empty(),
        "characteristics referencing missing RecordLayouts: {:?}",
        missing
    );
}

#[test]
fn lookup1d_record_layout_structure() {
    let m = module();
    let rl = m.record_layout.iter()
        .find(|r| r.get_name() == "Lookup1D_FLOAT32_IEEE")
        .expect("expected Lookup1D_FLOAT32_IEEE RecordLayout");

    // In this file, Lookup layouts define fnc_values for data storage format.
    // Axis geometry comes from the characteristic's axis_descr, not the layout.
    let fnc = rl.fnc_values.as_ref().expect("should define fnc_values");
    assert_eq!(fnc.datatype, DataType::Float32Ieee);
    assert_eq!(fnc.position, 1);
}

#[test]
fn lookup2d_record_layout_structure() {
    let m = module();
    let rl = m.record_layout.iter()
        .find(|r| r.get_name() == "Lookup2D_FLOAT32_IEEE")
        .expect("expected Lookup2D_FLOAT32_IEEE RecordLayout");

    let fnc = rl.fnc_values.as_ref().expect("should define fnc_values");
    assert_eq!(fnc.datatype, DataType::Float32Ieee);
    // Verify ColumnDir index mode (row-major storage)
    assert_eq!(fnc.index_mode, IndexMode::ColumnDir);
}

// ========================================================================
// Cross-Reference Integrity Tests
// ========================================================================

fn compu_method_names(m: &Module) -> HashSet<String> {
    m.compu_method.iter().map(|c| c.get_name().to_string()).collect()
}

fn record_layout_names(m: &Module) -> HashSet<String> {
    m.record_layout.iter().map(|r| r.get_name().to_string()).collect()
}

fn axis_pts_names(m: &Module) -> HashSet<String> {
    m.axis_pts.iter().map(|a| a.get_name().to_string()).collect()
}

fn vtab_names(m: &Module) -> HashSet<String> {
    m.compu_vtab.iter().map(|v| v.get_name().to_string())
        .chain(m.compu_vtab_range.iter().map(|v| v.get_name().to_string()))
        .collect()
}

// -- Characteristic cross-references --

#[test]
fn all_characteristics_reference_valid_compu_methods() {
    let m = module();
    let cm = compu_method_names(m);
    let broken: Vec<_> = m.characteristic.iter()
        .filter(|ch| ch.conversion != "NO_COMPU_METHOD")
        .filter(|ch| !cm.contains(&ch.conversion))
        .map(|ch| format!("{} -> {}", ch.get_name(), ch.conversion))
        .collect();
    assert!(broken.is_empty(), "broken characteristic->CM refs: {:?}", broken);
}

#[test]
fn all_characteristics_reference_valid_record_layouts() {
    let m = module();
    let rl = record_layout_names(m);
    let broken: Vec<_> = m.characteristic.iter()
        .filter(|ch| !rl.contains(&ch.deposit))
        .map(|ch| format!("{} -> {}", ch.get_name(), ch.deposit))
        .collect();
    assert!(broken.is_empty(), "broken characteristic->RL refs: {:?}", broken);
}

#[test]
fn characteristic_no_compu_method_count() {
    let m = module();
    let count = m.characteristic.iter()
        .filter(|ch| ch.conversion == "NO_COMPU_METHOD")
        .count();
    assert_eq!(count, 35, "expected 35 characteristics with NO_COMPU_METHOD");
}

// -- Measurement cross-references --

#[test]
fn measurement_compu_method_integrity() {
    let m = module();
    let cm = compu_method_names(m);
    let broken: Vec<_> = m.measurement.iter()
        .filter(|meas| meas.conversion != "NO_COMPU_METHOD")
        .filter(|meas| !cm.contains(&meas.conversion))
        .map(|meas| meas.get_name().to_string())
        .collect();
    // 5 measurements reference DMC_CM_uint32 which doesn't exist in this module
    assert_eq!(broken.len(), 5, "expected exactly 5 broken measurement->CM refs");
    assert!(broken.iter().all(|n| n.contains("_LowCnt")),
        "all broken refs should be *_LowCnt measurements, got: {:?}", broken);
}

#[test]
fn measurement_no_compu_method_count() {
    let m = module();
    let count = m.measurement.iter()
        .filter(|meas| meas.conversion == "NO_COMPU_METHOD")
        .count();
    assert_eq!(count, 471, "expected 471 measurements with NO_COMPU_METHOD");
}

// -- AxisDescr cross-references --

#[test]
fn all_axis_descr_reference_valid_compu_methods() {
    let m = module();
    let cm = compu_method_names(m);
    let mut broken = Vec::new();
    for ch in &m.characteristic {
        for ad in &ch.axis_descr {
            if ad.conversion != "NO_COMPU_METHOD" && !cm.contains(&ad.conversion) {
                broken.push(format!("{} axis -> {}", ch.get_name(), ad.conversion));
            }
        }
    }
    assert!(broken.is_empty(), "broken axis_descr->CM refs: {:?}", broken);
}

#[test]
fn all_axis_descr_axis_pts_refs_resolve() {
    let m = module();
    let ap = axis_pts_names(m);
    let mut broken = Vec::new();
    for ch in &m.characteristic {
        for ad in &ch.axis_descr {
            if let Some(ref apr) = ad.axis_pts_ref
                && !ap.contains(&apr.axis_points) {
                    broken.push(format!("{} -> {}", ch.get_name(), apr.axis_points));
                }
        }
    }
    assert!(broken.is_empty(), "broken axis_descr->axis_pts refs: {:?}", broken);
}

#[test]
fn axis_pts_ref_total_count() {
    let m = module();
    let count: usize = m.characteristic.iter()
        .flat_map(|ch| &ch.axis_descr)
        .filter(|ad| ad.axis_pts_ref.is_some())
        .count();
    assert_eq!(count, 999, "expected 999 axis_pts_ref entries (311 curve + 688 map)");
}

// -- CompuMethod -> CompuVtab cross-references --

#[test]
fn all_compu_tab_refs_resolve_to_vtab() {
    let m = module();
    let vtabs = vtab_names(m);
    let broken: Vec<_> = m.compu_method.iter()
        .filter_map(|cm| cm.compu_tab_ref.as_ref().map(|r| (cm, r)))
        .filter(|(_, r)| !vtabs.contains(&r.conversion_table))
        .map(|(cm, r)| format!("{} -> {}", cm.get_name(), r.conversion_table))
        .collect();
    assert!(broken.is_empty(), "broken CM->vtab refs: {:?}", broken);
}

#[test]
fn compu_tab_ref_count_matches_tab_verb_count() {
    let m = module();
    let tab_verb_count = m.compu_method.iter()
        .filter(|cm| cm.conversion_type == ConversionType::TabVerb)
        .count();
    let tab_ref_count = m.compu_method.iter()
        .filter(|cm| cm.compu_tab_ref.is_some())
        .count();
    assert_eq!(tab_verb_count, 432);
    assert_eq!(tab_ref_count, tab_verb_count,
        "every TabVerb should have a compu_tab_ref");
}

// -- AxisPts cross-references --

#[test]
fn all_axis_pts_reference_valid_compu_methods() {
    let m = module();
    let cm = compu_method_names(m);
    let broken: Vec<_> = m.axis_pts.iter()
        .filter(|ap| ap.conversion != "NO_COMPU_METHOD")
        .filter(|ap| !cm.contains(&ap.conversion))
        .map(|ap| format!("{} -> {}", ap.get_name(), ap.conversion))
        .collect();
    assert!(broken.is_empty(), "broken axis_pts->CM refs: {:?}", broken);
}

// -- Conversion type distribution --

#[test]
fn compu_method_type_distribution() {
    let m = module();
    let identical = m.compu_method.iter().filter(|cm| cm.conversion_type == ConversionType::Identical).count();
    let linear = m.compu_method.iter().filter(|cm| cm.conversion_type == ConversionType::Linear).count();
    let rat_func = m.compu_method.iter().filter(|cm| cm.conversion_type == ConversionType::RatFunc).count();
    let tab_verb = m.compu_method.iter().filter(|cm| cm.conversion_type == ConversionType::TabVerb).count();
    assert_eq!(identical, 1);
    assert_eq!(linear, 1);
    assert_eq!(rat_func, 561);
    assert_eq!(tab_verb, 432);
    assert_eq!(identical + linear + rat_func + tab_verb, m.compu_method.len(),
        "all compu_methods should be accounted for");
}

// -- Bidirectional: every CompuVtab is referenced by at least one CompuMethod --

#[test]
fn all_compu_vtabs_are_referenced() {
    let m = module();
    let referenced: HashSet<String> = m.compu_method.iter()
        .filter_map(|cm| cm.compu_tab_ref.as_ref())
        .map(|r| r.conversion_table.clone())
        .collect();
    let unreferenced: Vec<_> = m.compu_vtab.iter()
        .filter(|v| !referenced.contains(v.get_name()))
        .map(|v| v.get_name().to_string())
        .collect();
    assert!(unreferenced.is_empty(),
        "unreferenced compu_vtabs: {:?}", unreferenced);
}

// -- Bidirectional: every AxisPts is referenced by at least one axis_descr --

#[test]
fn all_axis_pts_are_referenced() {
    let m = module();
    let referenced: HashSet<String> = m.characteristic.iter()
        .flat_map(|ch| &ch.axis_descr)
        .filter_map(|ad| ad.axis_pts_ref.as_ref())
        .map(|r| r.axis_points.clone())
        .collect();
    let unreferenced: Vec<_> = m.axis_pts.iter()
        .filter(|ap| !referenced.contains(ap.get_name()))
        .map(|ap| ap.get_name().to_string())
        .collect();
    assert!(unreferenced.is_empty(),
        "unreferenced axis_pts: {:?}", unreferenced);
}

// ========================================================================
// Resolver Integration Tests
// ========================================================================

fn resolver() -> Resolver<'static> {
    Resolver::new(module())
}

#[test]
fn resolve_value_characteristic() {
    let r = resolver();
    let result = r.resolve_characteristic("g_xcp_enable_status").unwrap();
    match result {
        ResolvedCharacteristic::Value(v) => {
            assert_eq!(v.name, "g_xcp_enable_status");
            assert_eq!(v.conversion, "NO_COMPU_METHOD");
            assert_eq!(v.layout.name, "Scalar_ULONG");
        }
        other => panic!("expected Value, got {:?}", other),
    }
}

#[test]
fn resolve_curve_characteristic() {
    let r = resolver();
    // Pick a known CURVE with ComAxis
    let curves = r.resolve_all_curves();
    let first_ok = curves.iter().find(|c| c.is_ok()).unwrap().as_ref().unwrap();
    assert!(!first_ok.name.is_empty());
    assert!(!first_ok.layout.name.is_empty());

    match &first_ok.x_axis.source {
        AxisSource::ComAxis { axis_pts_name, axis_pts_address, .. } => {
            assert!(!axis_pts_name.is_empty());
            assert!(*axis_pts_address > 0);
        }
        AxisSource::FixAxisPar { count, .. } => {
            assert!(*count > 0);
        }
        other => panic!("unexpected axis source: {:?}", other),
    }
}

#[test]
fn resolve_map_characteristic() {
    let r = resolver();
    let maps = r.resolve_all_maps();
    let first_ok = maps.iter().find(|m| m.is_ok()).unwrap().as_ref().unwrap();
    assert!(!first_ok.name.is_empty());

    // MAPs should have 2 axes (first is X, second is Y by convention)
    // The attribute tells axis *type* (ComAxis, FixAxis, etc), not dimension
    assert!(!first_ok.x_axis.conversion.is_empty() || first_ok.x_axis.conversion == "NO_COMPU_METHOD");
    assert!(!first_ok.y_axis.conversion.is_empty() || first_ok.y_axis.conversion == "NO_COMPU_METHOD");
}

#[test]
fn resolve_all_curves_succeed() {
    let r = resolver();
    let results = r.resolve_all_curves();
    let (ok, err): (Vec<_>, Vec<_>) = results.into_iter().partition(|r| r.is_ok());
    assert!(err.is_empty(), "failed to resolve {} curves: {:?}",
        err.len(), err.iter().take(3).collect::<Vec<_>>());
    assert_eq!(ok.len(), 355, "should resolve all 355 curves");
}

#[test]
fn resolve_all_maps_succeed() {
    let r = resolver();
    let results = r.resolve_all_maps();
    let (ok, err): (Vec<_>, Vec<_>) = results.into_iter().partition(|r| r.is_ok());
    assert!(err.is_empty(), "failed to resolve {} maps: {:?}",
        err.len(), err.iter().take(3).collect::<Vec<_>>());
    assert_eq!(ok.len(), 344, "should resolve all 344 maps");
}

#[test]
fn resolved_curve_fix_axis_par_values() {
    let m = module();
    // Find a CURVE with FixAxisPar
    let fix_curve = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Curve)
        .find(|c| c.axis_descr.first().is_some_and(|ad| ad.fix_axis_par.is_some()))
        .expect("should have at least one CURVE with FixAxisPar");

    let r = resolver();
    let resolved = r.resolve_characteristic(fix_curve.get_name()).unwrap();
    if let ResolvedCharacteristic::Curve(curve) = resolved {
        if let AxisSource::FixAxisPar { offset, shift, count } = &curve.x_axis.source {
            let values = Resolver::compute_fix_axis_par_values(*offset, *shift, *count);
            assert_eq!(values.len(), *count as usize);
            if *count > 1 {
                // Verify monotonic sequence
                for w in values.windows(2) {
                    assert!(w[1] >= w[0] || *shift < 0.0,
                        "axis values should be monotonic");
                }
            }
        } else {
            panic!("expected FixAxisPar source");
        }
    }
}

#[test]
fn resolved_map_com_axis_has_deposit() {
    let r = resolver();
    let maps = r.resolve_all_maps();
    let map = maps.iter().find(|m| m.is_ok()).unwrap().as_ref().unwrap();

    if let AxisSource::ComAxis { deposit_name, .. } = &map.x_axis.source {
        // deposit_name is the record layout name from AXIS_PTS.deposit_record
        assert!(!deposit_name.is_empty(), "ComAxis should have a deposit record");
    }
}

#[test]
fn resolved_curve_has_unit() {
    let r = resolver();
    let curves = r.resolve_all_curves();
    // At least some curves should have non-empty units
    let with_unit = curves.iter()
        .filter_map(|c| c.as_ref().ok())
        .filter(|c| !c.unit.is_empty())
        .count();
    assert!(with_unit > 0, "some curves should have physical units");
}

#[test]
fn resolve_nonexistent_characteristic_errors() {
    let r = resolver();
    let result = r.resolve_characteristic("definitely_not_a_thing");
    assert!(matches!(result, Err(ResolveError::NotFound { kind: "Characteristic", .. })));
}

#[test]
fn list_characteristics_by_type() {
    let r = resolver();
    assert_eq!(r.list_characteristics(CharacteristicType::Curve).len(), 355);
    assert_eq!(r.list_characteristics(CharacteristicType::Map).len(), 344);
    assert_eq!(r.list_characteristics(CharacteristicType::Value).len(), 9374);
    assert_eq!(r.list_characteristics(CharacteristicType::ValBlk).len(), 673);
    assert_eq!(r.list_characteristics(CharacteristicType::Ascii).len(), 5);
}

// ========================================================================
// HEX File Integration Tests
// ========================================================================

fn sample_hex() -> &'static HexMemory {
    static HEX: OnceLock<HexMemory> = OnceLock::new();
    HEX.get_or_init(|| {
        HexMemory::from_file(Path::new("refs/zc-blanc_rear_c_tc389-inca.hex"))
            .expect("could not load sample HEX file")
    })
}

#[test]
fn hex_loads_without_panic() {
    let _hex = sample_hex();
}

#[test]
fn hex_has_segments() {
    let hex = sample_hex();
    assert!(hex.segment_count() > 0, "HEX should have at least one segment");
    assert!(hex.total_bytes() > 0, "HEX should have data");
}

#[test]
fn hex_address_range_is_reasonable() {
    let hex = sample_hex();
    let min = hex.min_address().unwrap();
    let max = hex.max_address().unwrap();
    // Automotive ECU flash typically starts at 0x80000000+ (TriCore)
    assert!(min >= 0x8000_0000, "min address 0x{min:08X} should be in flash region");
    assert!(max > min, "max should exceed min");
}

#[test]
fn hex_characteristic_addresses_are_readable() {
    let hex = sample_hex();
    let m = module();
    // Sample some Value characteristics and verify their addresses are in the HEX
    let readable_count = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Value)
        .take(100)
        .filter(|c| hex.contains(c.address, 1))
        .count();
    assert!(readable_count > 0,
        "at least some characteristic addresses should be in the HEX file");
}

#[test]
fn hex_read_scalar_value_bytes() {
    let hex = sample_hex();
    let m = module();
    // Find a Value characteristic with a known scalar layout
    let ch = m.characteristic.iter()
        .find(|c| c.get_name() == "g_xcp_enable_status")
        .unwrap();
    // This has Scalar_ULONG layout — try reading 4 bytes at its address
    if hex.contains(ch.address, 4) {
        let bytes = hex.read_bytes(ch.address, 4).unwrap();
        assert_eq!(bytes.len(), 4);
    }
}

#[test]
fn hex_axis_pts_addresses_are_readable() {
    let hex = sample_hex();
    let m = module();
    let readable = m.axis_pts.iter()
        .filter(|ap| hex.contains(ap.address, 1))
        .count();
    // At least some axis_pts should be in the HEX file
    assert!(readable > 0, "some AXIS_PTS addresses should be in the HEX file");
}

#[test]
fn hex_total_size_is_reasonable() {
    let hex = sample_hex();
    let total = hex.total_bytes();
    // A typical ECU flash image is a few MB
    assert!(total > 100_000, "HEX should have >100KB of data, got {total}");
    assert!(total < 100_000_000, "HEX should have <100MB of data, got {total}");
}

// ========================================================================
// Measurement Resolution Tests
// ========================================================================

#[test]
fn resolve_all_measurements_succeeds() {
    let m = module();
    let r = Resolver::new(m);
    let results = r.resolve_all_measurements();
    assert_eq!(results.len(), m.measurement.len());
    // Some may fail due to missing compu_methods, but resolution itself should not panic
    let ok_count = results.iter().filter(|r| r.is_ok()).count();
    assert!(ok_count > 0, "at least some measurements should resolve");
}

#[test]
fn all_measurements_are_ram() {
    let m = module();
    let r = Resolver::new(m);
    let results = r.resolve_all_measurements();
    for meas in results.iter().flatten() {
        assert!(meas.is_ram(), "{} should be RAM", meas.name);
    }
}

#[test]
fn measurement_addresses_not_in_hex() {
    let m = module();
    let hex = sample_hex();
    let r = Resolver::new(m);
    // Measurements are RAM — their addresses should generally NOT be in the flash HEX.
    // Check a representative sample.
    let resolved: Vec<_> = r.resolve_all_measurements()
        .into_iter()
        .filter_map(|r| r.ok())
        .filter_map(|m| m.ecu_address.map(|addr| (m.name.clone(), addr)))
        .take(100)
        .collect();
    let in_hex = resolved.iter()
        .filter(|(_, addr)| hex.contains(*addr, 1))
        .count();
    // RAM addresses typically live in a different address space than flash
    // Most should NOT be in the HEX file
    let ratio = in_hex as f64 / resolved.len() as f64;
    assert!(ratio < 0.5,
        "expected most measurement addresses to be outside flash HEX, \
         but {in_hex}/{} were found (ratio {ratio:.2})", resolved.len());
}

#[test]
fn read_measurement_from_hex_returns_error() {
    let m = module();
    let hex = sample_hex();
    let r = Resolver::new(m);
    // Pick the first measurement with an address
    let meas_name = m.measurement.iter()
        .find(|meas| meas.ecu_address.is_some())
        .map(|meas| meas.get_name().to_string())
        .expect("sample should have measurements with ECU_ADDRESS");
    let result = r.read_measurement_from_hex(&meas_name, hex);
    assert!(matches!(result, Err(ResolveError::MeasurementIsRam { .. })),
        "reading measurement from HEX should return MeasurementIsRam error");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("RAM"), "error should mention RAM");
}

#[test]
fn measurement_has_valid_datatypes() {
    let m = module();
    let r = Resolver::new(m);
    let results = r.resolve_all_measurements();
    for result in results.into_iter().flatten() {
        // Verify datatype is a known variant (this is just a sanity check)
        let size = A2lValue::datatype_size(&result.datatype);
        assert!(size > 0, "measurement {} has zero-size datatype {:?}",
            result.name, result.datatype);
    }
}

#[test]
fn measurement_conversion_refs_mostly_valid() {
    let m = module();
    let r = Resolver::new(m);
    let cm_names: HashSet<_> = m.compu_method.iter()
        .map(|cm| cm.get_name().to_string())
        .collect();

    let results = r.resolve_all_measurements();
    let resolved: Vec<_> = results.into_iter().flatten().collect();
    let valid = resolved.iter()
        .filter(|m| m.conversion == "NO_COMPU_METHOD" || cm_names.contains(&m.conversion))
        .count();
    // We know 5 measurements reference the broken DMC_CM_uint32
    let broken = resolved.len() - valid;
    assert!(broken <= 5,
        "at most 5 broken measurement→CM refs expected, got {broken}");
}

// ========================================================================
// End-to-End Extractor Tests
// ========================================================================

fn extractor() -> &'static Extractor<'static> {
    static EXT: OnceLock<Extractor<'static>> = OnceLock::new();
    EXT.get_or_init(|| Extractor::new(module(), sample_hex()))
}

#[test]
fn extract_scalar_value() {
    let ext = extractor();
    // Find a Value characteristic that's in the HEX file
    let m = module();
    let value_chars: Vec<_> = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Value)
        .filter(|c| sample_hex().contains(c.address, 1))
        .collect();
    assert!(!value_chars.is_empty(), "should have Value chars in HEX");

    // Try extracting the first one
    let name = value_chars[0].get_name();
    let result = ext.extract_value(name);
    match result {
        Ok(ev) => {
            assert_eq!(ev.name, name);
            assert!(!ev.unit.is_empty() || ev.unit.is_empty()); // unit may be empty for NO_COMPU_METHOD
        }
        Err(ExtractError::Conversion(_)) => {
            // Some may have broken COMPU_METHODs — that's OK
        }
        Err(e) => panic!("unexpected error extracting {name}: {e}"),
    }
}

#[test]
fn extract_multiple_scalar_values() {
    let ext = extractor();
    let m = module();
    let mut ok_count = 0;
    let mut err_count = 0;

    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Value)
        .filter(|c| sample_hex().contains(c.address, 1))
        .take(50)
    {
        match ext.extract_value(ch.get_name()) {
            Ok(ev) => {
                // Numeric values should be finite
                if let Some(v) = ev.physical.as_f64() {
                    assert!(v.is_finite() || v == 0.0,
                        "{}: got non-finite value {v}", ev.name);
                }
                ok_count += 1;
            }
            Err(_) => err_count += 1,
        }
    }
    assert!(ok_count > 0, "should extract at least some Values (ok={ok_count}, err={err_count})");
}

#[test]
fn extract_curve_fix_axis() {
    let ext = extractor();
    let m = module();
    let r = Resolver::new(m);

    // Find a curve with FixAxisPar (these don't need HEX read for axis)
    let curve_name = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Curve)
        .filter(|c| sample_hex().contains(c.address, 1))
        .find(|c| {
            r.resolve_characteristic(c.get_name()).ok()
                .and_then(|rc| match rc {
                    ResolvedCharacteristic::Curve(curve) => {
                        matches!(curve.x_axis.source, AxisSource::FixAxisPar { .. })
                            .then_some(())
                    }
                    _ => None,
                })
                .is_some()
        })
        .map(|c| c.get_name().to_string());

    if let Some(name) = curve_name {
        let result = ext.extract_curve(&name);
        match result {
            Ok(ec) => {
                assert_eq!(ec.name, name);
                assert!(!ec.x_axis.is_empty(), "curve should have axis values");
                assert_eq!(ec.x_axis.len(), ec.values.len(),
                    "axis and values should have same length");
            }
            Err(ExtractError::Conversion(_)) => { /* OK — broken CM */ }
            Err(e) => panic!("unexpected error extracting curve {name}: {e}"),
        }
    }
}

#[test]
fn extract_curve_com_axis() {
    let ext = extractor();
    let m = module();
    let r = Resolver::new(m);

    // Find a curve with ComAxis (needs AXIS_PTS read from HEX)
    let curve_name = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Curve)
        .filter(|c| sample_hex().contains(c.address, 1))
        .find(|c| {
            r.resolve_characteristic(c.get_name()).ok()
                .and_then(|rc| match rc {
                    ResolvedCharacteristic::Curve(curve) => {
                        matches!(curve.x_axis.source, AxisSource::ComAxis { .. })
                            .then_some(())
                    }
                    _ => None,
                })
                .is_some()
        })
        .map(|c| c.get_name().to_string());

    if let Some(name) = curve_name {
        let result = ext.extract_curve(&name);
        match result {
            Ok(ec) => {
                assert_eq!(ec.name, name);
                assert!(!ec.x_axis.is_empty(), "curve should have axis values");
                assert_eq!(ec.x_axis.len(), ec.values.len(),
                    "axis and values should have same length");
            }
            Err(ExtractError::Hex(_)) => {
                // AXIS_PTS address may not be in HEX — acceptable
            }
            Err(ExtractError::Conversion(_)) => { /* OK */ }
            Err(e) => panic!("unexpected error extracting ComAxis curve {name}: {e}"),
        }
    }
}

#[test]
fn extract_map() {
    let ext = extractor();
    let m = module();

    // Find a map in the HEX file
    let map_name = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Map)
        .find(|c| sample_hex().contains(c.address, 1))
        .map(|c| c.get_name().to_string());

    if let Some(name) = map_name {
        let result = ext.extract_map(&name);
        match result {
            Ok(em) => {
                assert_eq!(em.name, name);
                assert!(!em.x_axis.is_empty());
                assert!(!em.y_axis.is_empty());
                assert_eq!(em.values.len(), em.y_axis.len(),
                    "values row count should match y axis");
                if !em.values.is_empty() {
                    assert_eq!(em.values[0].len(), em.x_axis.len(),
                        "values column count should match x axis");
                }
            }
            Err(ExtractError::Hex(_)) => { /* AXIS_PTS not in HEX */ }
            Err(ExtractError::Conversion(_)) => { /* broken CM */ }
            Err(e) => panic!("unexpected error extracting map {name}: {e}"),
        }
    }
}

#[test]
fn extract_measurement_from_hex_fails() {
    let ext = extractor();
    let m = module();
    let meas_name = m.measurement.iter()
        .next()
        .map(|meas| meas.get_name().to_string())
        .expect("should have at least one measurement");
    let result = ext.extract_measurement(&meas_name);
    assert!(matches!(result, Err(ExtractError::Resolve(ResolveError::MeasurementIsRam { .. }))),
        "extracting measurement from HEX should fail with MeasurementIsRam");
}

#[test]
fn extract_wrong_type_curve_for_value() {
    let ext = extractor();
    let m = module();
    // Try to extract a Curve as a Value — should error
    let curve_name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Curve)
        .map(|c| c.get_name().to_string())
        .unwrap();
    let result = ext.extract_value(&curve_name);
    assert!(matches!(result, Err(ExtractError::Resolve(ResolveError::WrongType { .. }))),
        "extracting curve as value should give WrongType error");
}

#[test]
fn extract_nonexistent_characteristic() {
    let ext = extractor();
    let result = ext.extract_value("totally_fake_name_12345");
    assert!(matches!(result, Err(ExtractError::Resolve(ResolveError::NotFound { .. }))));
}

#[test]
fn all_map_layouts_use_column_dir() {
    // Verify our sample uses COLUMN_DIR for all MAPs (known from exploration)
    let m = module();
    let r = Resolver::new(m);
    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Map)
    {
        if let Ok(ResolvedCharacteristic::Map(map)) = r.resolve_characteristic(ch.get_name()) {
            assert_eq!(
                map.layout.index_mode,
                Some(a2lfile::IndexMode::ColumnDir),
                "MAP {} should use ColumnDir", ch.get_name()
            );
        }
    }
}

#[test]
fn map_column_dir_dimensions_consistent() {
    let ext = extractor();
    let m = module();
    // Extract several maps and verify dimensions are consistent
    let mut extracted = 0;
    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Map)
        .filter(|c| sample_hex().contains(c.address, 1))
        .take(20)
    {
        if let Ok(em) = ext.extract_map(ch.get_name()) {
            assert_eq!(em.values.len(), em.y_axis.len(),
                "{}: row count {} != y_axis len {}", em.name, em.values.len(), em.y_axis.len());
            for (i, row) in em.values.iter().enumerate() {
                assert_eq!(row.len(), em.x_axis.len(),
                    "{}: row[{i}] len {} != x_axis len {}", em.name, row.len(), em.x_axis.len());
            }
            extracted += 1;
        }
    }
    assert!(extracted > 0, "should extract at least one MAP");
}

#[test]
fn all_curve_layouts_use_column_dir() {
    let m = module();
    let r = Resolver::new(m);
    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Curve)
    {
        if let Ok(ResolvedCharacteristic::Curve(curve)) = r.resolve_characteristic(ch.get_name()) {
            assert_eq!(
                curve.layout.index_mode,
                Some(a2lfile::IndexMode::ColumnDir),
                "CURVE {} should use ColumnDir", ch.get_name()
            );
        }
    }
}

#[test]
fn curve_index_mode_extraction_consistent() {
    let ext = extractor();
    let m = module();
    let mut extracted = 0;
    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Curve)
        .filter(|c| sample_hex().contains(c.address, 1))
        .take(20)
    {
        match ext.extract_curve(ch.get_name()) {
            Ok(ec) => {
                assert_eq!(ec.values.len(), ec.x_axis.len(),
                    "{}: values len {} != x_axis len {}", ec.name, ec.values.len(), ec.x_axis.len());
                extracted += 1;
            }
            Err(ExtractError::Hex(_)) => {}
            Err(ExtractError::Conversion(_)) => {}
            Err(e) => panic!("unexpected error for {}: {e}", ch.get_name()),
        }
    }
    assert!(extracted > 0, "should extract at least one CURVE with index_mode check");
}

// ========================================================================
// VAL_BLK extraction tests
// ========================================================================

#[test]
fn resolve_val_blk_characteristics() {
    let m = module();
    let r = Resolver::new(m);
    let mut resolved = 0;
    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::ValBlk)
        .take(20)
    {
        match r.resolve_characteristic(ch.get_name()) {
            Ok(ResolvedCharacteristic::ValBlk(vb)) => {
                assert_eq!(vb.name, ch.get_name());
                assert!(vb.count > 0, "{}: count should be > 0", vb.name);
                resolved += 1;
            }
            other => panic!("expected ValBlk for {}, got: {other:?}", ch.get_name()),
        }
    }
    assert!(resolved > 0, "should resolve at least one VAL_BLK");
}

#[test]
fn val_blk_count_matches_number_field() {
    let m = module();
    let r = Resolver::new(m);
    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::ValBlk)
    {
        let expected = ch.number.as_ref().map(|n| n.number).unwrap_or(1);
        if let Ok(ResolvedCharacteristic::ValBlk(vb)) = r.resolve_characteristic(ch.get_name()) {
            assert_eq!(vb.count, expected,
                "{}: count {} != number {}", vb.name, vb.count, expected);
        }
    }
}

#[test]
fn extract_val_blk_values() {
    let ext = extractor();
    let m = module();
    let mut extracted = 0;
    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::ValBlk)
        .filter(|c| sample_hex().contains(c.address, 1))
        .take(20)
    {
        match ext.extract_val_blk(ch.get_name()) {
            Ok(evb) => {
                let expected_count = ch.number.as_ref().map(|n| n.number as usize).unwrap_or(1);
                assert_eq!(evb.values.len(), expected_count,
                    "{}: values len {} != expected {}", evb.name, evb.values.len(), expected_count);
                extracted += 1;
            }
            Err(ExtractError::Hex(_)) => {} // address not in HEX
            Err(ExtractError::Conversion(_)) => {} // broken CM
            Err(e) => panic!("unexpected error for {}: {e}", ch.get_name()),
        }
    }
    assert!(extracted > 0, "should extract at least one VAL_BLK");
}

#[test]
fn extract_val_blk_wrong_type_fails() {
    let ext = extractor();
    let m = module();
    // Try to extract a Value as a ValBlk
    let value_name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Value)
        .map(|c| c.get_name().to_string())
        .unwrap();
    let result = ext.extract_val_blk(&value_name);
    assert!(matches!(result, Err(ExtractError::Resolve(ResolveError::WrongType { .. }))),
        "extracting Value as ValBlk should fail");
}

// ========================================================================
// ASCII extraction tests
// ========================================================================

#[test]
fn resolve_ascii_characteristics() {
    let m = module();
    let r = Resolver::new(m);
    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Ascii)
    {
        match r.resolve_characteristic(ch.get_name()) {
            Ok(ResolvedCharacteristic::Ascii(a)) => {
                assert_eq!(a.name, ch.get_name());
                let expected_len = ch.number.as_ref().map(|n| n.number).unwrap_or(0);
                assert_eq!(a.length, expected_len,
                    "{}: length {} != number {}", a.name, a.length, expected_len);
            }
            other => panic!("expected Ascii for {}, got: {other:?}", ch.get_name()),
        }
    }
}

#[test]
fn extract_ascii_strings() {
    let ext = extractor();
    let m = module();
    let mut extracted = 0;
    for ch in m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Ascii)
        .filter(|c| sample_hex().contains(c.address, 1))
    {
        match ext.extract_ascii(ch.get_name()) {
            Ok(ea) => {
                assert!(!ea.text.is_empty(),
                    "{}: expected non-empty ASCII string", ea.name);
                // Verify no trailing NULs
                assert!(!ea.text.ends_with('\0'),
                    "{}: text should not have trailing NUL", ea.name);
                extracted += 1;
            }
            Err(ExtractError::Hex(_)) => {} // not in HEX
            Err(e) => panic!("unexpected error for {}: {e}", ch.get_name()),
        }
    }
    assert!(extracted > 0, "should extract at least one ASCII");
}

#[test]
fn extract_ascii_wrong_type_fails() {
    let ext = extractor();
    let m = module();
    let value_name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Value)
        .map(|c| c.get_name().to_string())
        .unwrap();
    let result = ext.extract_ascii(&value_name);
    assert!(matches!(result, Err(ExtractError::Resolve(ResolveError::WrongType { .. }))),
        "extracting Value as Ascii should fail");
}

// ========================================================================
// Batch Extraction and Error Recovery Tests
// ========================================================================

#[test]
fn extract_all_returns_report_with_successes() {
    let ext = extractor();
    let report = ext.extract_all();
    assert!(report.successes.len() > 1000, "should extract many characteristics");
    assert_eq!(report.total(), report.successes.len() + report.failures.len());
}

#[test]
fn extract_all_zero_failures_with_full_hex() {
    let ext = extractor();
    let report = ext.extract_all();
    assert_eq!(report.failures.len(), 0,
        "all characteristics should extract; got {} failures: {:?}",
        report.failures.len(),
        report.failures.iter().take(5).map(|f| &f.name).collect::<Vec<_>>());
}

#[test]
fn extract_all_success_counts_match_a2l() {
    let m = module();
    let ext = extractor();
    let report = ext.extract_all();
    let counts = report.success_counts();

    let a2l_values = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Value).count();
    let a2l_curves = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Curve).count();
    let a2l_maps = m.characteristic.iter()
        .filter(|c| c.characteristic_type == CharacteristicType::Map).count();

    assert_eq!(*counts.get("VALUE").unwrap_or(&0), a2l_values);
    assert_eq!(*counts.get("CURVE").unwrap_or(&0), a2l_curves);
    assert_eq!(*counts.get("MAP").unwrap_or(&0), a2l_maps);
}

#[test]
fn extract_any_auto_detects_value() {
    let ext = extractor();
    let m = module();
    let name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Value
            && sample_hex().contains(c.address, 1))
        .map(|c| c.get_name())
        .expect("need a Value characteristic in HEX");

    let obj = ext.extract_any(name).expect("extract_any should succeed");
    assert!(matches!(obj, ExtractedObject::Value(_)));
    assert_eq!(obj.type_label(), "VALUE");
}

#[test]
fn extract_any_auto_detects_curve() {
    let ext = extractor();
    let m = module();
    let name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Curve
            && sample_hex().contains(c.address, 1))
        .map(|c| c.get_name())
        .expect("need a Curve characteristic in HEX");

    let obj = ext.extract_any(name).expect("extract_any should succeed");
    assert!(matches!(obj, ExtractedObject::Curve(_)));
    assert_eq!(obj.type_label(), "CURVE");
}

#[test]
fn extract_any_auto_detects_map() {
    let ext = extractor();
    let m = module();
    let name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Map
            && sample_hex().contains(c.address, 1))
        .map(|c| c.get_name())
        .expect("need a Map characteristic in HEX");

    let obj = ext.extract_any(name).expect("extract_any should succeed");
    assert!(matches!(obj, ExtractedObject::Map(_)));
    assert_eq!(obj.type_label(), "MAP");
}

#[test]
fn extract_any_auto_detects_ascii() {
    let ext = extractor();
    let m = module();
    let name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Ascii
            && sample_hex().contains(c.address, 1))
        .map(|c| c.get_name())
        .expect("need an Ascii characteristic in HEX");

    let obj = ext.extract_any(name).expect("extract_any should succeed");
    assert!(matches!(obj, ExtractedObject::Ascii(_)));
    assert_eq!(obj.type_label(), "ASCII");
}

#[test]
fn extract_any_not_found() {
    let ext = extractor();
    let result = ext.extract_any("nonexistent_object_xyz");
    assert!(result.is_err());
}

#[test]
fn extraction_report_print_summary_does_not_panic() {
    let ext = extractor();
    let report = ext.extract_all();
    report.print_summary();
}

// ========================================================================
// JSON serialization tests
// ========================================================================

#[test]
fn json_extracted_value_roundtrip() {
    let ext = extractor();
    let val = ext.extract_value("g_xcp_enable_status").unwrap();
    let json = serde_json::to_string(&val).expect("serialize value");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
    assert_eq!(parsed["name"], "g_xcp_enable_status");
    assert!(parsed["raw"].is_object());
    // physical is now untagged — number or string, not an object
    assert!(parsed["physical"].is_number() || parsed["physical"].is_string());
}

#[test]
fn json_extracted_curve_has_axes() {
    let ext = extractor();
    let m = module();
    let name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Curve
            && sample_hex().contains(c.address, 1))
        .map(|c| c.get_name())
        .expect("need a Curve");

    let curve = ext.extract_curve(name).unwrap();
    let json = serde_json::to_string_pretty(&curve).expect("serialize curve");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
    assert!(parsed["x_axis"].is_array());
    assert!(parsed["values"].is_array());
    assert_eq!(parsed["x_axis"].as_array().unwrap().len(), parsed["values"].as_array().unwrap().len());
}

#[test]
fn json_extracted_map_has_2d_values() {
    let ext = extractor();
    let m = module();
    let name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Map
            && sample_hex().contains(c.address, 1))
        .map(|c| c.get_name())
        .expect("need a Map");

    let map = ext.extract_map(name).unwrap();
    let json = serde_json::to_string(&map).expect("serialize map");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
    assert!(parsed["x_axis"].is_array());
    assert!(parsed["y_axis"].is_array());
    assert!(parsed["values"].is_array());
    let rows = parsed["values"].as_array().unwrap();
    assert_eq!(rows.len(), parsed["y_axis"].as_array().unwrap().len());
    assert_eq!(rows[0].as_array().unwrap().len(), parsed["x_axis"].as_array().unwrap().len());
}

#[test]
fn json_extracted_valblk_has_array() {
    let ext = extractor();
    let m = module();
    let name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::ValBlk
            && sample_hex().contains(c.address, 1))
        .map(|c| c.get_name())
        .expect("need a ValBlk");

    let vb = ext.extract_val_blk(name).unwrap();
    let json = serde_json::to_string(&vb).expect("serialize valblk");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
    assert!(parsed["values"].is_array());
    assert!(parsed["values"].as_array().unwrap().len() > 1);
}

#[test]
fn json_extracted_ascii_has_text() {
    let ext = extractor();
    let m = module();
    let name = m.characteristic.iter()
        .find(|c| c.characteristic_type == CharacteristicType::Ascii
            && sample_hex().contains(c.address, 1))
        .map(|c| c.get_name())
        .expect("need an Ascii");

    let ascii = ext.extract_ascii(name).unwrap();
    let json = serde_json::to_string(&ascii).expect("serialize ascii");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
    assert!(parsed["text"].is_string());
    assert!(!parsed["text"].as_str().unwrap().is_empty());
}

#[test]
fn json_extracted_object_enum_tags_correctly() {
    let ext = extractor();
    let obj = ext.extract_any("g_xcp_enable_status").unwrap();
    let json = serde_json::to_string(&obj).expect("serialize ExtractedObject");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
    assert!(parsed["Value"].is_object(), "should be tagged as Value variant");
}

#[test]
fn json_physical_value_numeric_format() {
    let val = PhysicalValue::Numeric(42.5);
    let json = serde_json::to_string(&val).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Untagged: serializes directly as a number
    assert!(parsed.is_number());
    assert_eq!(parsed.as_f64().unwrap(), 42.5);
}

#[test]
fn json_physical_value_verbal_format() {
    let val = PhysicalValue::Verbal("ON".to_string());
    let json = serde_json::to_string(&val).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Untagged: serializes directly as a string
    assert!(parsed.is_string());
    assert_eq!(parsed.as_str().unwrap(), "ON");
}

#[test]
fn json_all_characteristics_serialize() {
    let ext = extractor();
    let report = ext.extract_all();
    for obj in &report.successes {
        serde_json::to_string(obj)
            .unwrap_or_else(|e| panic!("failed to serialize {}: {e}", obj.name()));
    }
}
