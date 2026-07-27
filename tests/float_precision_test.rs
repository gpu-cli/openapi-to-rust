//! `format: float` maps to `f64` by default.
//!
//! JSON carries no binary32. A price written on the wire as `0.03` parses
//! losslessly into `f64`, but through `f32` it becomes `0.029999999329447746`.
//! The declared format describes the server's storage, not the transport, so
//! mapping it literally discards precision the response actually carried.
//!
//! Observed live on RunPod's catalog, whose prices declare `float` while its
//! billing endpoints declare `double`.

use openapi_to_rust::{TypeMapper, TypeMappingConfig};

#[test]
fn float_format_defaults_to_f64() {
    let mapper = TypeMapper::new(TypeMappingConfig::default());
    assert_eq!(mapper.number_format(Some("float")).rust_type, "f64");
    assert_eq!(mapper.number_format(Some("double")).rust_type, "f64");
    assert_eq!(mapper.number_format(None).rust_type, "f64");
}

#[test]
fn float_precision_f32_opt_in_is_honored() {
    let config = TypeMappingConfig {
        float_precision: openapi_to_rust::type_mapping::FloatPrecision::F32,
        ..Default::default()
    };
    let mapper = TypeMapper::new(config);
    assert_eq!(mapper.number_format(Some("float")).rust_type, "f32");
    // `double` is unaffected by the opt-in.
    assert_eq!(mapper.number_format(Some("double")).rust_type, "f64");
}

/// Conservative mode reproduces pre-Q2 output, which mapped `float` literally.
#[test]
fn conservative_mode_maps_float_literally() {
    let mapper = TypeMapper::new(TypeMappingConfig::conservative());
    assert_eq!(mapper.number_format(Some("float")).rust_type, "f32");
}

/// The precision claim itself, so the rationale in the docs stays honest.
#[test]
fn f32_round_trip_loses_the_wire_value() {
    let wire = "0.03";
    let as_f64: f64 = wire.parse().unwrap();
    let through_f32 = wire.parse::<f32>().unwrap() as f64;

    assert_eq!(as_f64.to_string(), "0.03");
    assert_ne!(through_f32.to_string(), "0.03");
    assert!(through_f32.to_string().starts_with("0.0299999993"));
}
