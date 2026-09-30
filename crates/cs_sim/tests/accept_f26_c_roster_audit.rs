//! F26-C acceptance: roster-wide handling audit and deviation reports.
//!
//! Spec: `specs/F26-handling-probes-and-original-behavior-calibration.md`,
//! stage `### F26-C`, acceptance test AC03. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`. Ordinary build/test only.
//!
//! Every test audits through the production `audit_roster`, which flies the
//! production `ProbeRunner` and compares with the production `compare`. The
//! airframes are authored development data, so nothing here is an
//! original-fidelity claim.

use cs_sim::flight::synthetic_fixed_wing;
use cs_sim::flight::tuning::{AirframeTuning, LoadoutMass, ModelKind};
use cs_sim::probes::{
    AirframeAudit, AuditError, AuditStatus, ProbeKind, ProbeRunner, RosterRow,
    SYNTHETIC_HANDLING_AIRFRAME, Tolerance, UnavailableReason, VerdictStatus, audit_roster,
    synthetic_audit_roster, synthetic_reference_envelope,
};

const ID: &str = SYNTHETIC_HANDLING_AIRFRAME;

fn audit_one(row: RosterRow) -> AirframeAudit {
    audit_roster(&[row])
        .expect("a one-row roster is valid")
        .airframes
        .remove(0)
}

/// An envelope whose references are what `tuning` measures (5 % tolerance,
/// turn held out), over every maneuver `tuning` can be measured on.
fn fitted_row(tuning: &AirframeTuning, configuration: &str) -> RosterRow {
    let mut envelope = synthetic_reference_envelope();
    let baseline = ProbeRunner::new(ID, tuning.clone())
        .run_envelope(&envelope)
        .expect("valid envelope");
    envelope.entries.retain(|entry| {
        baseline
            .runs
            .iter()
            .any(|run| run.maneuver == entry.maneuver)
    });
    for entry in &mut envelope.entries {
        let run = baseline
            .runs
            .iter()
            .find(|run| run.maneuver == entry.maneuver)
            .expect("retained entries were flown");
        entry.reference = run.value;
        entry.tolerance = Tolerance {
            plus_minus: run.value.abs() * 0.05 + 1e-9,
            rationale: "test: 5 % of the baseline measurement".to_owned(),
        };
        entry.held_out = entry.maneuver == ProbeKind::Turn;
    }
    RosterRow::new(ID, configuration, Some(tuning.clone()), Some(envelope))
}

/// The minimum scenario (AC03): a missing reference trace reports unavailable,
/// not pass.
#[test]
fn accept_f26_c_missing_reference_trace_reports_unavailable_not_pass() {
    let audit = audit_one(RosterRow::new(
        ID,
        "stock",
        Some(synthetic_fixed_wing()),
        None,
    ));
    assert_eq!(audit.status, AuditStatus::Unavailable);
    assert_eq!(
        audit.unavailable,
        vec![UnavailableReason::NoReferenceEnvelope]
    );
    assert!(audit.deviations.is_empty(), "nothing was compared");
    assert!(!audit.supports_original_fidelity);

    let roster = audit_roster(&[RosterRow::new(
        ID,
        "stock",
        Some(synthetic_fixed_wing()),
        None,
    )])
    .expect("valid");
    assert!(!roster.all_pass());
    assert_eq!(roster.count(&AuditStatus::Pass), 0);
}

/// An envelope but nothing to fly is unavailable too.
#[test]
fn accept_f26_c_missing_tuning_is_unavailable() {
    let audit = audit_one(RosterRow::new(
        ID,
        "stock",
        None,
        Some(synthetic_reference_envelope()),
    ));
    assert_eq!(audit.status, AuditStatus::Unavailable);
    assert_eq!(audit.unavailable, vec![UnavailableReason::NoTuning]);
}

/// An envelope that does not bound every sheet maneuver is a coverage gap: the
/// measured entries may be inside their windows, but the row is not a pass.
#[test]
fn accept_f26_c_envelope_missing_a_maneuver_is_not_a_pass() {
    let mut row = fitted_row(&fully_measurable_tuning(), "stock");
    row.envelope
        .as_mut()
        .expect("fitted")
        .entries
        .retain(|entry| entry.maneuver != ProbeKind::Roll);
    let audit = audit_one(row);

    assert_eq!(audit.status, AuditStatus::Unavailable);
    assert!(
        audit
            .unavailable
            .iter()
            .any(|reason| matches!(reason, UnavailableReason::MissingManeuvers(m) if m.contains(&ProbeKind::Roll))),
        "{:?}",
        audit.unavailable
    );
    assert!(
        audit
            .deviations
            .iter()
            .all(|row| row.status == VerdictStatus::WithinEnvelope || row.measured.is_none()),
        "the measured entries are still reported"
    );
}

/// A row with a measured out-of-envelope entry fails, with the deviation
/// reported against the reference; an unmeasurable maneuver is listed and
/// never replaced by a default.
#[test]
fn accept_f26_c_out_of_envelope_entry_fails_with_signed_deviation() {
    let audit = audit_one(RosterRow::new(
        ID,
        "stock",
        Some(synthetic_fixed_wing()),
        Some(synthetic_reference_envelope()),
    ));
    assert_eq!(audit.status, AuditStatus::Fail);
    assert!(!audit.supports_original_fidelity);
    assert!(audit.combined_hash.is_some());

    let acceleration = audit
        .deviations
        .iter()
        .find(|row| row.maneuver == ProbeKind::Acceleration)
        .expect("reported");
    let measured = acceleration.measured.expect("acceleration is measurable");
    assert_eq!(
        acceleration.deviation,
        Some(measured - acceleration.reference)
    );
    assert!(matches!(
        acceleration.status,
        VerdictStatus::OutOfEnvelope { .. }
    ));

    let stall = audit
        .deviations
        .iter()
        .find(|row| row.maneuver == ProbeKind::StallRecovery)
        .expect("reported");
    assert_eq!(stall.measured, None);
    assert_eq!(stall.status, VerdictStatus::NoMeasurement);
    assert!(
        audit.unavailable.iter().any(|reason| matches!(
            reason,
            UnavailableReason::Unmeasured {
                maneuver: ProbeKind::StallRecovery,
                ..
            }
        )),
        "{:?}",
        audit.unavailable
    );
}

/// A fully covered, fully measured, fitted row passes, and the synthetic origin
/// still bars an original-fidelity claim. Also the deviation is the reference
/// being met: zero-ish, with the row's hash reproducible.
#[test]
fn accept_f26_c_fitted_row_passes_but_synthetic_origin_supports_no_fidelity_claim() {
    let tuning = fully_measurable_tuning();
    let first = audit_one(fitted_row(&tuning, "stock"));
    assert_eq!(first.status, AuditStatus::Pass, "{:?}", first.unavailable);
    assert!(first.unavailable.is_empty());
    assert!(!first.supports_original_fidelity);
    let second = audit_one(fitted_row(&tuning, "stock"));
    assert_eq!(first.combined_hash, second.combined_hash);
}

/// One row's failure never stops the others, and any non-pass row keeps the
/// roster from reading all-pass.
#[test]
fn accept_f26_c_rows_are_independent_and_unavailable_blocks_all_pass() {
    let roster = audit_roster(&synthetic_audit_roster()).expect("valid");
    assert_eq!(roster.airframes.len(), 3);
    assert_eq!(roster.airframes[0].status, AuditStatus::Fail);
    assert_eq!(roster.airframes[1].status, AuditStatus::Unavailable);
    assert_eq!(roster.airframes[2].status, AuditStatus::Unavailable);
    assert!(roster.airframes[0].combined_hash.is_some(), "still flown");
    assert!(!roster.all_pass());
    assert!(!roster.supports_original_fidelity_claim());
}

/// A refused run (model kind disagreement) is recorded against its row.
#[test]
fn accept_f26_c_refused_run_is_recorded_on_the_row() {
    let mut tuning = synthetic_fixed_wing();
    tuning.model_kind = ModelKind::Exceptional;
    let audit = audit_one(RosterRow::new(
        ID,
        "stock",
        Some(tuning),
        Some(synthetic_reference_envelope()),
    ));
    assert_eq!(audit.status, AuditStatus::Unavailable);
    assert!(
        matches!(
            audit.unavailable.as_slice(),
            [UnavailableReason::RunRefused(_)]
        ),
        "{:?}",
        audit.unavailable
    );
}

/// Loadout extremes are separate rows and are flown with their mass: the same
/// reference is deviated from differently by a heavy configuration.
#[test]
fn accept_f26_c_loadout_extreme_rows_are_flown_with_their_mass() {
    let stock = fitted_row(&fully_measurable_tuning(), "stock");
    let mut heavy = stock.clone();
    heavy.configuration = "max-armor".to_owned();
    heavy.loadout = LoadoutMass {
        armor_kg: 600.0,
        ..LoadoutMass::EMPTY
    };
    let roster = audit_roster(&[stock, heavy]).expect("distinct configurations");
    assert_eq!(roster.airframes[0].status, AuditStatus::Pass);
    assert_ne!(
        roster.airframes[0].combined_hash, roster.airframes[1].combined_hash,
        "the heavy row flew a different trace"
    );
    assert_eq!(
        roster.airframes[1].status,
        AuditStatus::Fail,
        "600 kg of armor leaves the stock reference"
    );
}

/// An empty roster must not read as all-pass; a repeated row is refused.
#[test]
fn accept_f26_c_empty_and_duplicate_rosters_are_refused() {
    assert_eq!(audit_roster(&[]), Err(AuditError::EmptyRoster));
    let row = RosterRow::new(ID, "stock", None, None);
    assert_eq!(
        audit_roster(&[row.clone(), row]),
        Err(AuditError::DuplicateRow {
            airframe_id: ID.to_owned(),
            configuration: "stock".to_owned(),
        })
    );
}

/// The assist profile and provenance are carried on the row.
#[test]
fn accept_f26_c_row_reports_assist_profile_and_provenance() {
    let audit = audit_one(RosterRow::new(
        ID,
        "stock",
        Some(synthetic_fixed_wing()),
        Some(synthetic_reference_envelope()),
    ));
    let assists = audit.assists.expect("a tuning was flown");
    assert!(!assists.enabled, "the calibrated profile has assists off");
    assert_eq!(
        audit.tuning_origin,
        Some(cs_types::content::Origin::SyntheticFixture)
    );
}

/// A variant of the synthetic airframe on which every sheet maneuver can be
/// measured: a late, shallow stall (0.8 rad, 80 % residual) is the only change
/// found that lets the stall-recovery probe recover within its authored
/// horizon. Authored development data.
fn fully_measurable_tuning() -> AirframeTuning {
    let mut tuning = synthetic_fixed_wing();
    tuning.stall.stall_angle_rad = 0.8;
    tuning.stall.residual_fraction = 0.8;
    tuning
}
