//! The roster-wide handling audit and deviation report (F26-C).
//!
//! Spec: `specs/F26-handling-probes-and-original-behavior-calibration.md`,
//! stage `### F26-C`, acceptance test AC03. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! [`audit_roster`] is the integration of the F26-B producer ([`ProbeRunner`])
//! and the F26-A consumer ([`compare`]) over every airframe row a caller
//! supplies. One row is one airframe configuration (stock, a custom loadout
//! extreme, a forced mission configuration): every row gets an audit result,
//! so no airframe is silently absent.
//!
//! **Unavailable is not pass.** A row with no reference envelope, no tuning, a
//! refused run, an envelope that skips a sheet maneuver or an entry that could
//! not be measured is [`AuditStatus::Unavailable`], and only
//! [`AuditStatus::Pass`] counts towards [`RosterAudit::all_pass`]. A row with
//! any out-of-envelope entry is [`AuditStatus::Fail`], which outranks
//! unavailable because it is the more specific finding.
//!
//! Each row carries per-entry deviations (reference versus measured, with the
//! tolerance) and the assist profile with its provenance, so a deliberate
//! modern assist is visible next to the original reference (sheet rule 4).
//! One row failing never stops the others: every row is independent, and a
//! refusal is recorded against the row that caused it.
//!
//! **The roster itself is unknown.** No retail airframe list or tuning has
//! been imported, so callers pass the rows; [`synthetic_audit_roster`] is the
//! declared synthetic stand-in and makes no original-fidelity claim.

use cs_types::content::Origin;

use crate::flight::synthetic_fixed_wing;
use crate::flight::tuning::{AirframeTuning, AssistProfile, DamageState, LoadoutMass};

use super::comparison::{ProbeVerdict, VerdictStatus, compare};
use super::envelope::ReferenceEnvelope;
use super::maneuver::{ProbeKind, ProbeQuantity};
use super::runner::{ProbeError, ProbeRunner};
use super::synthetic::{SYNTHETIC_HANDLING_AIRFRAME, synthetic_reference_envelope};

/// One airframe configuration to audit.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterRow {
    /// The airframe id.
    pub airframe_id: String,
    /// The configuration label, e.g. `stock` or `max-armor`; with the id it
    /// identifies the row.
    pub configuration: String,
    /// The tuning to fly, when one exists.
    pub tuning: Option<AirframeTuning>,
    /// The reference envelope to compare with, when one exists.
    pub envelope: Option<ReferenceEnvelope>,
    /// The loadout mass flown.
    pub loadout: LoadoutMass,
    /// The damage state flown.
    pub damage: DamageState,
}

impl RosterRow {
    /// A stock row: empty loadout, pristine damage.
    #[must_use]
    pub fn new(
        airframe_id: impl Into<String>,
        configuration: impl Into<String>,
        tuning: Option<AirframeTuning>,
        envelope: Option<ReferenceEnvelope>,
    ) -> Self {
        Self {
            airframe_id: airframe_id.into(),
            configuration: configuration.into(),
            tuning,
            envelope,
            loadout: LoadoutMass::EMPTY,
            damage: DamageState::PRISTINE,
        }
    }
}

/// Why a row could not be given a pass or a fail.
#[derive(Clone, Debug, PartialEq)]
pub enum UnavailableReason {
    /// No reference envelope exists for the row: there is nothing to compare.
    NoReferenceEnvelope,
    /// No tuning exists for the row: there is nothing to fly.
    NoTuning,
    /// The tuning failed its own validation.
    InvalidTuning(String),
    /// The runner refused the whole envelope.
    RunRefused(ProbeError),
    /// The comparison refused the envelope or trace.
    CompareRefused(String),
    /// The envelope does not bound every sheet maneuver.
    MissingManeuvers(Vec<ProbeKind>),
    /// One maneuver could not be flown or measured.
    Unmeasured {
        /// The maneuver.
        maneuver: ProbeKind,
        /// The reason text.
        reason: String,
    },
}

impl std::fmt::Display for UnavailableReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoReferenceEnvelope => write!(f, "no reference envelope"),
            Self::NoTuning => write!(f, "no tuning"),
            Self::InvalidTuning(error) => write!(f, "invalid tuning: {error}"),
            Self::RunRefused(error) => write!(f, "probe run refused: {error}"),
            Self::CompareRefused(error) => write!(f, "comparison refused: {error}"),
            Self::MissingManeuvers(missing) => {
                let labels: Vec<&str> = missing.iter().map(|kind| kind.label()).collect();
                write!(f, "envelope lacks maneuvers: {}", labels.join(", "))
            }
            Self::Unmeasured { maneuver, reason } => {
                write!(f, "{} not measured: {reason}", maneuver.label())
            }
        }
    }
}

/// The outcome of auditing one row.
#[derive(Clone, Debug, PartialEq)]
pub enum AuditStatus {
    /// Every entry measured inside its window and the envelope covers every
    /// maneuver.
    Pass,
    /// At least one entry was measured outside its window.
    Fail,
    /// The row could not be fully assessed; never a pass.
    Unavailable,
}

impl AuditStatus {
    /// The stable report label.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One entry's deviation from the original reference.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviationRow {
    /// The maneuver.
    pub maneuver: ProbeKind,
    /// The quantity.
    pub quantity: ProbeQuantity,
    /// The unit of `reference`, `measured` and `deviation`.
    pub unit: &'static str,
    /// Whether the envelope held this entry out of the fit.
    pub held_out: bool,
    /// The reference value.
    pub reference: f64,
    /// The tolerance half-width.
    pub tolerance: f64,
    /// The measured value, when one exists.
    pub measured: Option<f64>,
    /// `measured - reference`, when measured.
    pub deviation: Option<f64>,
    /// The verdict.
    pub status: VerdictStatus,
}

/// The assist profile a row flew with: a deliberate modern assist, its off
/// switch and where it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AssistReport {
    /// Whether any assist contributed. `false` is the off switch engaged.
    pub enabled: bool,
    /// The bank/level gain, in N·m per radian.
    pub bank_level_gain_nm_per_rad: f64,
}

impl From<AssistProfile> for AssistReport {
    fn from(profile: AssistProfile) -> Self {
        Self {
            enabled: profile.enabled,
            bank_level_gain_nm_per_rad: profile.bank_level_gain_nm_per_rad,
        }
    }
}

/// The audit of one row.
#[derive(Clone, Debug, PartialEq)]
pub struct AirframeAudit {
    /// The airframe id.
    pub airframe_id: String,
    /// The configuration label.
    pub configuration: String,
    /// The outcome.
    pub status: AuditStatus,
    /// Why the row is unavailable (empty otherwise; may also explain gaps on a
    /// failing row).
    pub unavailable: Vec<UnavailableReason>,
    /// One deviation per envelope entry; empty when there was no comparison.
    pub deviations: Vec<DeviationRow>,
    /// Where the tuning came from, when there was one.
    pub tuning_origin: Option<Origin>,
    /// Where the envelope came from, when there was one.
    pub envelope_origin: Option<Origin>,
    /// The assists the row flew with, when there was a tuning.
    pub assists: Option<AssistReport>,
    /// The combined probe hash, when a run completed.
    pub combined_hash: Option<u64>,
    /// Whether this row can back a claim about original behavior: it passes
    /// and both sides are original-observed.
    pub supports_original_fidelity: bool,
}

/// Why the audit itself could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditError {
    /// No rows were supplied; an empty roster must not read as all-pass.
    EmptyRoster,
    /// Two rows share an airframe id and configuration.
    DuplicateRow {
        /// The airframe id.
        airframe_id: String,
        /// The configuration label.
        configuration: String,
    },
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRoster => write!(f, "the roster has no rows"),
            Self::DuplicateRow {
                airframe_id,
                configuration,
            } => write!(
                f,
                "the roster lists {airframe_id} ({configuration}) more than once"
            ),
        }
    }
}

impl std::error::Error for AuditError {}

/// The audit of a whole roster.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterAudit {
    /// One audit per row, in roster order.
    pub airframes: Vec<AirframeAudit>,
}

impl RosterAudit {
    /// How many rows have `status`.
    #[must_use]
    pub fn count(&self, status: &AuditStatus) -> usize {
        self.airframes
            .iter()
            .filter(|audit| &audit.status == status)
            .count()
    }

    /// Whether every row passed. Unavailable rows make this false.
    #[must_use]
    pub fn all_pass(&self) -> bool {
        !self.airframes.is_empty()
            && self
                .airframes
                .iter()
                .all(|audit| audit.status == AuditStatus::Pass)
    }

    /// Whether every row supports an original-fidelity claim.
    #[must_use]
    pub fn supports_original_fidelity_claim(&self) -> bool {
        !self.airframes.is_empty()
            && self
                .airframes
                .iter()
                .all(|audit| audit.supports_original_fidelity)
    }
}

/// Audits every row. Rows are independent: a refusal on one is recorded on it
/// and the rest still run.
///
/// # Errors
///
/// [`AuditError`] for an empty roster or a repeated `(airframe, configuration)`.
pub fn audit_roster(rows: &[RosterRow]) -> Result<RosterAudit, AuditError> {
    if rows.is_empty() {
        return Err(AuditError::EmptyRoster);
    }
    for (index, row) in rows.iter().enumerate() {
        if rows[..index].iter().any(|earlier| {
            earlier.airframe_id == row.airframe_id && earlier.configuration == row.configuration
        }) {
            return Err(AuditError::DuplicateRow {
                airframe_id: row.airframe_id.clone(),
                configuration: row.configuration.clone(),
            });
        }
    }
    Ok(RosterAudit {
        airframes: rows.iter().map(audit_row).collect(),
    })
}

fn audit_row(row: &RosterRow) -> AirframeAudit {
    let mut audit = AirframeAudit {
        airframe_id: row.airframe_id.clone(),
        configuration: row.configuration.clone(),
        status: AuditStatus::Unavailable,
        unavailable: Vec::new(),
        deviations: Vec::new(),
        tuning_origin: row.tuning.as_ref().map(|tuning| tuning.origin.clone()),
        envelope_origin: row
            .envelope
            .as_ref()
            .map(|envelope| envelope.origin.clone()),
        assists: row
            .tuning
            .as_ref()
            .map(|tuning| AssistReport::from(tuning.assists)),
        combined_hash: None,
        supports_original_fidelity: false,
    };

    if row.envelope.is_none() {
        audit
            .unavailable
            .push(UnavailableReason::NoReferenceEnvelope);
    }
    if row.tuning.is_none() {
        audit.unavailable.push(UnavailableReason::NoTuning);
    }
    let (Some(tuning), Some(envelope)) = (&row.tuning, &row.envelope) else {
        return audit;
    };
    if let Err(error) = tuning.validate() {
        audit
            .unavailable
            .push(UnavailableReason::InvalidTuning(error.to_string()));
        return audit;
    }

    let runner = ProbeRunner::new(row.airframe_id.clone(), tuning.clone())
        .with_loadout(row.loadout)
        .with_damage(row.damage);
    let run = match runner.run_envelope(envelope) {
        Ok(run) => run,
        Err(error) => {
            audit.unavailable.push(UnavailableReason::RunRefused(error));
            return audit;
        }
    };
    audit.combined_hash = Some(run.combined_hash);
    for (maneuver, error) in &run.unmeasured {
        audit.unavailable.push(UnavailableReason::Unmeasured {
            maneuver: *maneuver,
            reason: error.to_string(),
        });
    }
    let missing = envelope.missing_maneuvers();
    if !missing.is_empty() {
        audit
            .unavailable
            .push(UnavailableReason::MissingManeuvers(missing));
    }

    let assessment = match compare(envelope, &run.trace) {
        Ok(assessment) => assessment,
        Err(error) => {
            audit
                .unavailable
                .push(UnavailableReason::CompareRefused(error.to_string()));
            return audit;
        }
    };
    audit.deviations = assessment.verdicts.iter().map(deviation_row).collect();
    audit.status = if !assessment.failures().is_empty() {
        AuditStatus::Fail
    } else if assessment.passes() && audit.unavailable.is_empty() {
        AuditStatus::Pass
    } else {
        AuditStatus::Unavailable
    };
    audit.supports_original_fidelity =
        audit.status == AuditStatus::Pass && assessment.supports_original_fidelity_claim();
    audit
}

fn deviation_row(verdict: &ProbeVerdict) -> DeviationRow {
    DeviationRow {
        maneuver: verdict.maneuver,
        quantity: verdict.quantity,
        unit: verdict.quantity.unit(),
        held_out: verdict.held_out,
        reference: verdict.reference,
        tolerance: verdict.tolerance.plus_minus,
        measured: verdict.measured,
        deviation: verdict.measured.map(|value| value - verdict.reference),
        status: verdict.status,
    }
}

/// The declared synthetic roster: one flown row (whose authored references are
/// not met, and which has one unmeasurable maneuver), one airframe with a
/// tuning but no reference envelope, and one with an envelope but no tuning.
///
/// Authored development data, not original; it demonstrates the three states
/// the audit must keep apart.
#[must_use]
pub fn synthetic_audit_roster() -> Vec<RosterRow> {
    vec![
        RosterRow::new(
            SYNTHETIC_HANDLING_AIRFRAME,
            "stock",
            Some(synthetic_fixed_wing()),
            Some(synthetic_reference_envelope()),
        ),
        RosterRow::new(
            "fixture.synthetic-no-reference",
            "stock",
            Some(synthetic_fixed_wing()),
            None,
        ),
        RosterRow::new(
            "fixture.synthetic-no-tuning",
            "stock",
            None,
            Some(synthetic_reference_envelope()),
        ),
    ]
}
