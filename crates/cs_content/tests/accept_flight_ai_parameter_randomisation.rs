//! Acceptance tests for task #805: the original's +-5 % AI airframe parameter
//! randomisation (`0x4771c6`).
//!
//! Test prefix: `accept_flight_ai_parameter_randomisation_`. The synthetic
//! tests build an imported record by hand; the retail test reads
//! `$CS_GAME_DIR` and is `#[ignore]`d without it.

use cs_content::original_airframe::{
    AI_RANDOMISATION_BAND, AIRFRAME_FIELDS, AirframeController, OriginalAirframeParameters,
    OriginalDocuments, OriginalEngine, OriginalFieldValue,
};
use cs_sim::flight::OriginalAirframe;
use cs_types::content::{Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

fn provenance() -> Provenance {
    Provenance::new(
        ClaimId::new("test.airframe").expect("a claim id"),
        ClaimStatus::Documented,
        None,
    )
    .expect("a provenance")
}

/// A record whose values are distinct and positive.
fn record() -> OriginalAirframeParameters {
    let values = AIRFRAME_FIELDS
        .iter()
        .enumerate()
        .map(|(index, field)| OriginalFieldValue {
            field,
            value: 10.0 + index as f64,
            key_path: format!("dynamics/{field}"),
            source_record: None,
            provenance: provenance(),
        })
        .collect();
    OriginalAirframeParameters {
        record: "synthetic".to_owned(),
        inheritance_chain: vec!["synthetic".to_owned()],
        mode: None,
        is_autogyro: false,
        initial_fuel: Resolved::unknown(
            ClaimId::new("test.fuel").expect("a claim id"),
            "synthetic",
        )
        .expect("an unknown"),
        engine: OriginalEngine {
            id: 0,
            name: "synthetic".to_owned(),
            factor: 1.0,
            provenance: provenance(),
        },
        values,
    }
}

#[test]
fn accept_flight_ai_parameter_randomisation_ai_values_move_inside_the_band() {
    let imported = record();
    let rolled = imported
        .randomised(AirframeController::Ai, 7)
        .expect("the roll succeeds");
    assert_eq!(rolled.rolls.len(), AIRFRAME_FIELDS.len());
    let mut moved = 0;
    for roll in &rolled.rolls {
        let original = imported.value(roll.field).expect("imported").value;
        assert_eq!(roll.imported, original);
        let ratio = rolled.parameters.value(roll.field).expect("rolled").value / original;
        assert!(
            (1.0 - AI_RANDOMISATION_BAND..1.0 + AI_RANDOMISATION_BAND).contains(&ratio),
            "{} left the band: {ratio}",
            roll.field
        );
        if ratio != 1.0 {
            moved += 1;
        }
    }
    assert!(moved >= AIRFRAME_FIELDS.len() - 1, "only {moved} moved");
    OriginalAirframe::from_values(&rolled.parameters.field_values())
        .expect("a rolled record is still a valid airframe");
}

#[test]
fn accept_flight_ai_parameter_randomisation_player_keeps_the_exact_imported_values() {
    let imported = record();
    let rolled = imported
        .randomised(AirframeController::Player, 7)
        .expect("no roll");
    assert_eq!(rolled.parameters, imported);
    assert_eq!(rolled.seed, None);
    assert!(rolled.evidence_overrides().is_empty());
}

#[test]
fn accept_flight_ai_parameter_randomisation_is_deterministic_and_seed_dependent() {
    let imported = record();
    let a = imported
        .randomised(AirframeController::Ai, 1)
        .expect("roll");
    let b = imported
        .randomised(AirframeController::Ai, 1)
        .expect("roll");
    let c = imported
        .randomised(AirframeController::Ai, 2)
        .expect("roll");
    assert_eq!(a, b);
    assert_ne!(a.rolls, c.rolls);
}

#[test]
fn accept_flight_ai_parameter_randomisation_evidence_shows_seed_and_values() {
    let rolled = record()
        .randomised(AirframeController::Ai, 99)
        .expect("roll");
    assert_eq!(rolled.seed, Some(99));
    let lines = rolled.evidence_overrides();
    assert_eq!(lines.len(), 1 + AIRFRAME_FIELDS.len());
    assert!(lines[0].contains("seed 99"), "{}", lines[0]);
    for roll in &rolled.rolls {
        assert!(
            lines.iter().any(|line| line.starts_with(roll.field)
                && line.contains(&roll.rolled.to_string())),
            "{} missing from the evidence lines",
            roll.field
        );
    }
}

#[test]
fn accept_flight_ai_parameter_randomisation_refuses_a_non_finite_result_by_name() {
    let mut imported = record();
    imported
        .values
        .iter_mut()
        .find(|entry| entry.field == "ref_area")
        .expect("ref_area")
        .value = f64::INFINITY;
    let error = imported
        .randomised(AirframeController::Ai, 3)
        .expect_err("a non-finite value is refused");
    assert!(error.to_string().contains("ref_area"), "{error}");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flight_ai_parameter_randomisation_retail_ai_record_stays_a_valid_airframe() {
    let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set"));
    let documents = OriginalDocuments::read(&root).expect("the installation reads");
    for record in ["bloodhawk", "devastator", "pbloodhawk"] {
        let imported = documents.airframe(record).expect("the record imports");
        let player = imported
            .randomised(AirframeController::Player, 5)
            .expect("player");
        assert_eq!(player.parameters, imported);
        for seed in 0..64 {
            let rolled = imported
                .randomised(AirframeController::Ai, seed)
                .expect("roll");
            assert_ne!(rolled.parameters.field_values(), imported.field_values());
            OriginalAirframe::from_values(&rolled.parameters.field_values())
                .expect("a rolled retail record is a valid airframe");
        }
    }
}
