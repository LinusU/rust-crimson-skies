//! Acceptance scenario F24-A for the provenance-carrying tuning schema:
//! the declared fixed-wing field table and the declared synthetic airframe.
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stage `### F24-A`. Task test prefix: `accept_f24_a_`.
//!
//! These tests use only [`cs_content`]'s public API, so they fail to compile
//! if the schema or the declared record is removed, and they fail at run time
//! if the record stops validating against its own schema.

use cs_content::flight_tuning::{
    DeclaredAirframeTuning, DeclaredTuningError, TuningSchema, declared_synthetic_airframe,
};
use cs_types::content::{Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a valid claim id")
}

/// The declared schema and the declared synthetic airframe are in agreement:
/// every field is stated, in range, and the record is explicitly synthetic.
#[test]
fn accept_f24_a_declared_schema_and_synthetic_airframe_agree() {
    let schema = TuningSchema::fixed_wing();
    let record: DeclaredAirframeTuning = declared_synthetic_airframe();

    assert!(schema.field("reference_area_m2").is_some());
    assert_eq!(record.validate(&schema), Ok(()));
    assert_eq!(record.missing_required(&schema), Vec::<&str>::new());
    assert_eq!(record.origin, Origin::SyntheticFixture);
    assert!(!record.origin.is_original());
    assert_eq!(record.values.len(), schema.len());
}

/// A required field that is explicitly unknown is refused, not read as zero.
#[test]
fn accept_f24_a_required_unknown_is_refused() {
    let schema = TuningSchema::fixed_wing();
    let mut record = declared_synthetic_airframe();
    for value in &mut record.values {
        if value.field == "engine.max_thrust_n" {
            value.value = Resolved::unknown(claim("f24a.it.unknown-thrust"), "not measured")
                .expect("a reason is present");
        }
    }
    assert_eq!(
        record.validate(&schema),
        Err(DeclaredTuningError::RequiredFieldUnknown {
            name: "engine.max_thrust_n"
        })
    );
    assert_eq!(record.known_value("engine.max_thrust_n"), None);
}

/// An out-of-range known value is refused by name instead of being clamped.
#[test]
fn accept_f24_a_out_of_range_value_is_refused() {
    let schema = TuningSchema::fixed_wing();
    let mut record = declared_synthetic_airframe();
    for value in &mut record.values {
        if value.field == "mass.mass_kg" {
            value.value = Resolved::Known(Known::new(
                0.0,
                Provenance::designed(claim("f24a.it.zero-mass")),
            ));
        }
    }
    assert_eq!(
        record.validate(&schema),
        Err(DeclaredTuningError::OutOfRange {
            name: "mass.mass_kg".to_owned(),
            value: 0.0,
            min: 1.0,
            max: 200_000.0,
        })
    );
}
