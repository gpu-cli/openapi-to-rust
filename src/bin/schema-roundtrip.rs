//! Emit compiled round-trip tests for one OpenAPI document.
//!
//! The full corpus gate calls this after generating each scratch crate:
//!
//! ```text
//! schema-roundtrip SPEC OUTPUT_RS STATS_FILE
//! ```

use openapi_to_rust::schema_roundtrip::build_round_trip_plan;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let spec_path = PathBuf::from(args.next().ok_or("missing SPEC argument")?);
    let output_path = PathBuf::from(args.next().ok_or("missing OUTPUT_RS argument")?);
    let stats_path = PathBuf::from(args.next().ok_or("missing STATS_FILE argument")?);
    if args.next().is_some() {
        return Err("usage: schema-roundtrip SPEC OUTPUT_RS STATS_FILE".into());
    }

    let body = fs::read_to_string(&spec_path)?;
    let input_label = spec_path.to_string_lossy();
    let spec = openapi_to_rust::spec_source::parse_spec(&body, &input_label)?;
    let plan = build_round_trip_plan(&spec, 4)?;

    for path in [&output_path, &stats_path] {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&output_path, &plan.source)?;
    fs::write(&stats_path, plan.stats.to_shell())?;

    println!(
        "schema-roundtrip: {} schema(s), {} sample(s), {} skipped",
        plan.stats.tested_schemas, plan.stats.samples, plan.stats.skipped_schemas
    );
    for skipped in plan.skipped.iter().take(12) {
        println!("  skip {}: {}", skipped.schema, skipped.reason);
    }
    if plan.skipped.len() > 12 {
        println!("  ... {} more skipped", plan.skipped.len() - 12);
    }
    Ok(())
}
