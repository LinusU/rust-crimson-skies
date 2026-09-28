//! Reproducible original-game reference capture records
//! (`REF-CAPTURE-PROTOCOL`, task #357).
//!
//! This module owns the **record** an operator fills in while observing the
//! original game: which original ran, how it was watched, what was measured, in
//! which units, on which clock, and which private artifact the measurement came
//! from. It is protocol, not acquisition: nothing here runs the original
//! executable, and no field carries a default that could be mistaken for an
//! original measurement. Unknown is always spelled out as
//! [`Measurement::Unknown`] with a reason, never silently defaulted — a record
//! that has not been captured yet must read as *unavailable*, not as a pass
//! (CLI-EVIDENCE: "missing data is unavailable not pass").
//!
//! Three outcomes, never a bare pass/fail ([`CaptureReport`]):
//!
//! * [`CaptureReport::is_valid`] — every required field is present and
//!   consistent, every artifact re-hashes to its declared digest.
//! * [`CaptureReport::is_invalid`] — the record is defective: missing
//!   fingerprints, an ambiguous timebase, non-finite values, mismatched
//!   artifact hashes, undeclared units, or a source/claim contradiction such as
//!   a synthetic record labeled original. Defects are reported even when data
//!   is also missing.
//! * [`CaptureReport::is_unavailable`] — the record is honestly incomplete:
//!   capture data (samples and the observations they carry, artifacts, the
//!   identities a behavior claim is about, observer, capture method) is not
//!   there yet. Never a pass, never silently promoted.
//!
//! Provenance is explicit and separable: [`SampleSource`] says how the samples
//! were produced, [`RecordClaim`] says what the record is offered as, and the
//! two must agree ([`CaptureError::SourceClaimMismatch`]). A record's series
//! carries its own source so an original-observed series can never be merged
//! into a remake one ([`CaptureError::MixedSampleSources`]).
//!
//! Two worksheets share this record shape — the **first-mission branch**
//! worksheet and the **baseline flight probe** worksheet
//! ([`Worksheet`]) — and a [`ReferenceSet`] reserves one maneuver as the
//! **holdout** ([`HoldoutReservation`]): fitting acceleration alone can never
//! establish handling fidelity, because [`fidelity_comparison`] stays
//! `Unavailable` until the reserved maneuver has been captured as original
//! data ([`UnavailableReason::HoldoutNotCaptured`]).
//!
//! The file-access trace requested by Rally #341 (`F04-D-original-order`) is
//! shared as metadata ([`FileAccessTraceRef`]): which files the original
//! opened is valuable provenance, but a file trace records *what was read*, not
//! *how the aircraft flew*, so it can never be the basis of a behavior claim —
//! for the original **or** for this reimplementation
//! ([`CaptureError::NoFlightObservingBasis`]).
//!
//! No original unit, spawn coordinate, tick rate or timing is hardcoded here:
//! units ([`UnitDeclaration`]), the timebase ([`Timebase`]) and every identity
//! are declared per record by whoever captured it.

use std::fmt;
use std::path::{Path, PathBuf};

use cs_assets::install::Sha256;
use cs_types::evidence::ContentHash;

/// The quantity name under which a record's time unit must be declared.
///
/// Units are declared per quantity ([`UnitDeclaration`]); time is a quantity
/// like any other, so a record whose samples carry `t` must declare the pair
/// (`TIME_QUANTITY`, [`Timebase::unit`]).
pub const TIME_QUANTITY: &str = "time";

/// A measurement that is either present or explicitly not established.
///
/// `Unknown` always carries a reason: a record may only omit a measurement by
/// saying what is missing and why, which is what keeps "not captured yet"
/// distinct from "captured and equal to zero".
#[derive(Clone, Debug, PartialEq)]
pub enum Measurement<T> {
    /// The value was captured and is present.
    Known(T),
    /// The value was not captured; `reason` says what is missing.
    Unknown { reason: String },
}

impl<T> Measurement<T> {
    /// The captured value, when present.
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown { .. } => None,
        }
    }

    /// Whether the value was captured.
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known(_))
    }

    /// The recorded reason the value is missing, when it is missing.
    pub fn unknown_reason(&self) -> Option<&str> {
        match self {
            Self::Known(_) => None,
            Self::Unknown { reason } => Some(reason),
        }
    }
}

/// Where a capture's samples were produced.
///
/// The three sources are separate evidence classes and never mix inside one
/// record ([`CaptureError::MixedSampleSources`]); synthetic data is always
/// labeled as such and can never be promoted to original evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleSource {
    /// Observed on the running original executable.
    OriginalObserved,
    /// Produced by this reimplementation.
    RemakeSample,
    /// Authored synthetic data with no runtime behind it.
    SyntheticFixture,
}

impl SampleSource {
    /// The record-vocabulary name of the source.
    pub const fn label(self) -> &'static str {
        match self {
            Self::OriginalObserved => "original_observed",
            Self::RemakeSample => "remake_sample",
            Self::SyntheticFixture => "synthetic_fixture",
        }
    }
}

impl fmt::Display for SampleSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What a record is offered as evidence for.
///
/// The claim must agree with [`CaptureRecord::source`]: offering anything but
/// an original-observed record as original-game behavior is the acceptance
/// case "a synthetic record labeled original", refused by
/// [`CaptureError::SourceClaimMismatch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordClaim {
    /// Evidence about how the original game behaves.
    OriginalBehavior,
    /// Evidence about how this reimplementation behaves.
    RemakeBehavior,
    /// Worksheet or calibration scratch; not behavior evidence.
    WorksheetOnly,
}

impl RecordClaim {
    /// The record-vocabulary name of the claim.
    pub const fn label(self) -> &'static str {
        match self {
            Self::OriginalBehavior => "original_behavior",
            Self::RemakeBehavior => "remake_behavior",
            Self::WorksheetOnly => "worksheet_only",
        }
    }
}

impl fmt::Display for RecordClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One of the original-identity fields a record must carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FingerprintField {
    /// Which original edition produced the samples.
    Edition,
    /// SHA-256 of the original executable that produced the samples.
    Executable,
    /// SHA-256 of the original installation the samples came from.
    Installation,
}

impl FingerprintField {
    /// The field name used in diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Edition => "edition",
            Self::Executable => "executable_sha256",
            Self::Installation => "installation_sha256",
        }
    }
}

impl fmt::Display for FingerprintField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The original an original-observed capture is fingerprinted against.
///
/// All three fields are required when [`CaptureRecord::source`] is
/// [`SampleSource::OriginalObserved`] ([`CaptureError::MissingFingerprint`]);
/// an asserted-but-blank edition counts as missing, because an empty name
/// identifies no edition. For every other source they are `Unknown` with a
/// reason: a synthetic fixture never fingerprints an original it did not run.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalIdentity {
    /// Which original edition produced the samples.
    pub edition: Measurement<String>,
    /// SHA-256 of the original executable that produced the samples.
    pub executable_sha256: Measurement<ContentHash>,
    /// SHA-256 of the original installation the samples came from.
    pub installation_sha256: Measurement<ContentHash>,
}

/// Settings the capture ran under: difficulty, assists, and free-form notes.
///
/// Recorded, never defaulted: an unknown difficulty is `Unknown` with a reason,
/// because difficulty changes pacing and cannot be assumed.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsRecord {
    /// Difficulty the capture ran under.
    pub difficulty: Measurement<String>,
    /// Assistive options that were on (bank/level assist, auto-level, ...).
    pub assists: Measurement<Vec<String>>,
    /// Free-form setting notes (resolution, view distance, key bindings).
    pub notes: Vec<String>,
}

/// One maneuver of the probe vocabulary.
///
/// The names come from the F26 probe list (spec-derived); no timing, speed or
/// unit from the original is implied by picking one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManeuverKind {
    /// Straight full-throttle acceleration.
    Acceleration,
    /// Throttle-cut deceleration.
    CoastDown,
    /// Steady climb.
    Climb,
    /// Steady dive.
    Dive,
    /// Sustained turn.
    Turn,
    /// Roll rate.
    Roll,
    /// Yaw rate.
    Yaw,
    /// Stall entry and recovery.
    StallRecovery,
    /// Damaged-control response.
    Damage,
    /// Boost engagement and burn.
    Boost,
    /// Another maneuver, named by the operator.
    Other(String),
}

impl ManeuverKind {
    /// The maneuver name used in diagnostics.
    pub fn label(&self) -> &str {
        match self {
            Self::Acceleration => "acceleration",
            Self::CoastDown => "coast-down",
            Self::Climb => "climb",
            Self::Dive => "dive",
            Self::Turn => "turn",
            Self::Roll => "roll",
            Self::Yaw => "yaw",
            Self::StallRecovery => "stall-recovery",
            Self::Damage => "damage",
            Self::Boost => "boost",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for ManeuverKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which of the two operator worksheets a record belongs to.
///
/// Both worksheets share the record shape; they differ in what they pin down.
/// Neither hardcodes an original unit, spawn coordinate or timing — those are
/// captured per record or recorded as [`Measurement::Unknown`].
#[derive(Clone, Debug, PartialEq)]
pub enum Worksheet {
    /// First-mission branch observation: which branch of the original mission
    /// program was taken, under which recorded context.
    FirstMissionBranch {
        /// The branch identity as recorded (source location or operator label).
        branch: Measurement<String>,
        /// The recorded context the branch was taken under (spawn/initial
        /// conditions as observed), never an assumed coordinate.
        spawn_context: Measurement<String>,
    },
    /// One baseline flight probe for a declared airframe/loadout.
    BaselineFlight {
        /// Which probe maneuver this record captured.
        maneuver: ManeuverKind,
    },
}

/// How a record may be used when it joins a [`ReferenceSet`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleRole {
    /// May be fitted: it is calibration data.
    Calibration,
    /// Reserved for the final comparison; it must never enter a fit.
    Holdout,
}

impl SampleRole {
    /// The role name used in diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Calibration => "calibration",
            Self::Holdout => "holdout",
        }
    }
}

impl fmt::Display for SampleRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The measured timing uncertainty of one sample time, in [`Timebase::unit`].
#[derive(Clone, Debug, PartialEq)]
pub struct TimingUncertainty {
    /// Half-width of the measured uncertainty, in [`Timebase::unit`].
    pub plus_minus: f64,
    /// How the uncertainty was measured (frame pacing probe, tool timestamp
    /// spread, ...). Must be non-empty: an uncertainty with no stated method is
    /// an unmeasurable claim, so it is refused as ambiguous.
    pub method: String,
}

/// The clock a record's sample times live on.
///
/// Nothing is assumed: clock, origin, unit, rate and uncertainty are all
/// declared. Once a record has samples, all five must be `Known` and coherent,
/// otherwise the times cannot be placed on a timeline and the record is
/// refused as [`CaptureError::AmbiguousTimebase`].
#[derive(Clone, Debug, PartialEq)]
pub struct Timebase {
    /// What clock the times come from (capture-tool wall clock, original frame
    /// counter, ...).
    pub clock: Measurement<String>,
    /// What `t = 0` means (mission start, first input, recording start, ...).
    pub origin: Measurement<String>,
    /// The declared unit of every [`Sample::t`] (e.g. a second-like unit, a
    /// frame count). Never assumed; see also [`TIME_QUANTITY`].
    pub unit: String,
    /// Nominal samples per second of the capture, when measured.
    pub nominal_rate_hz: Measurement<f64>,
    /// The measured timing uncertainty of each sample time.
    pub uncertainty: Measurement<TimingUncertainty>,
}

/// Where a declared unit came from.
#[derive(Clone, Debug, PartialEq)]
pub enum UnitProvenance {
    /// The capture tool reported this unit directly.
    ReportedByCaptureTool { tool: String },
    /// Converted from another unit by a stated factor (factor in units of
    /// `from_unit` -> this unit).
    Converted { from_unit: String, factor: f64 },
    /// Not established; `reason` says why.
    Unknown { reason: String },
}

/// One declared quantity/unit pair with its conversion provenance.
///
/// A sample observation is only accepted when its exact (quantity, unit) pair
/// is declared here — undeclared units are refused
/// ([`CaptureError::UndeclaredUnit`]) instead of being guessed.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitDeclaration {
    /// What is being measured (e.g. `world_speed`, [`TIME_QUANTITY`]).
    pub quantity: String,
    /// The unit the samples of that quantity carry.
    pub unit: String,
    /// Where the unit came from.
    pub provenance: UnitProvenance,
}

/// One control input, in the ranges of the FLIGHT-PHYSICS contract.
///
/// A capture that reports raw axes converts them here and records the
/// conversion in [`UnitDeclaration`]; the ranges are this project's contract,
/// not a claim about the original's input encoding.
#[derive(Clone, Debug, PartialEq)]
pub struct FlightInput {
    /// Throttle in `[0, 1]`.
    pub throttle: f64,
    /// Pitch in `[-1, 1]`.
    pub pitch: f64,
    /// Roll in `[-1, 1]`.
    pub roll: f64,
    /// Yaw in `[-1, 1]`.
    pub yaw: f64,
    /// Boost held at this sample.
    pub boost: bool,
}

/// One observed quantity at one sample time, in its declared unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    /// What was observed (must be declared in [`CaptureRecord::units`]).
    pub quantity: String,
    /// The unit the value carries (must be the declared one).
    pub unit: String,
    /// The observed value; must be finite.
    pub value: f64,
}

/// One sample of a capture: when, what the pilot commanded, what was observed.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    /// Sample time in [`Timebase::unit`]; must be finite.
    pub t: f64,
    /// The control input at that time.
    pub input: FlightInput,
    /// Observed quantities at that time.
    pub observations: Vec<Observation>,
}

/// A record's input/time series and the source its samples came from.
///
/// The series repeats the source on purpose: merging an original-observed
/// series into a remake one (or the reverse) is a provenance break and is
/// refused by [`CaptureError::MixedSampleSources`].
#[derive(Clone, Debug, PartialEq)]
pub struct SampleSeries {
    /// Where these samples were produced; must equal [`CaptureRecord::source`].
    pub source: SampleSource,
    /// The samples, in time order (order is not itself validated).
    pub samples: Vec<Sample>,
}

/// What a private artifact behind a record is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtifactRole {
    /// Recorded video of the run.
    Video,
    /// Tool telemetry or measurement log.
    TelemetryLog,
    /// Input recording.
    InputLog,
    /// Screenshot of an instrument or event.
    Screenshot,
    /// A trace or report file.
    TraceLog,
    /// Another kind of private artifact, named by the operator.
    Other(String),
}

impl ArtifactRole {
    /// The role name used in diagnostics.
    pub fn label(&self) -> &str {
        match self {
            Self::Video => "video",
            Self::TelemetryLog => "telemetry_log",
            Self::InputLog => "input_log",
            Self::Screenshot => "screenshot",
            Self::TraceLog => "trace_log",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for ArtifactRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A reference to one private artifact: its spelling under the artifact root
/// and the SHA-256 it must re-hash to.
///
/// Paths are `/`-separated relative spellings under a private directory; an
/// absolute path, a `..` or a backslash is refused
/// ([`CaptureError::UnsafeArtifactPath`]) before anything is opened, so a
/// record can never make the validator read outside the artifact root.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactRef {
    /// Relative spelling of the artifact under the artifact root.
    pub relative_path: String,
    /// The SHA-256 the artifact's bytes must hash to.
    pub sha256: ContentHash,
    /// What kind of artifact it is.
    pub role: ArtifactRole,
}

/// The file-access trace requested by Rally #341 (`F04-D-original-order`),
/// shared as record metadata.
///
/// Only paths, hashes and tool/platform identity are shared — never the traced
/// file contents. The trace's digest is re-verified like any artifact
/// ([`validate_capture`]). A file trace answers *which files the original
/// opened*; it cannot answer *how the aircraft flew*, so it may accompany a
/// record but can never be the observation that supports a behavior claim
/// ([`CaptureError::NoFlightObservingBasis`]).
#[derive(Clone, Debug, PartialEq)]
pub struct FileAccessTraceRef {
    /// The task the trace belongs to (e.g. `Rally #341`).
    pub task: String,
    /// The tool that produced the trace (e.g. a Process Monitor capture).
    pub tool: String,
    /// The platform the trace was taken on.
    pub platform: String,
    /// Relative spelling of the trace under the artifact root.
    pub relative_path: String,
    /// The SHA-256 the trace's bytes must hash to.
    pub sha256: ContentHash,
    /// World/mission groups the trace covers, as recorded.
    pub covers: Vec<String>,
}

/// What the record was observed with.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureBasis {
    /// Watching the running program (owner `human_play`): video, screen
    /// capture, direct observation. Observes flight.
    RuntimeObservation { detail: String },
    /// A capture tool's telemetry: frame captures, input/state logs.
    /// Observes flight.
    InstrumentedCapture { detail: String },
    /// A file-access trace (Rally #341): which files were opened. Does **not**
    /// observe flight.
    FileAccessTrace { trace: FileAccessTraceRef },
    /// Authored synthetic data with no runtime behind it. Does **not** observe
    /// flight.
    SyntheticSimulation { detail: String },
}

impl CaptureBasis {
    /// Whether this basis can show how the aircraft flew.
    ///
    /// Only a runtime observation or an instrumented capture can; a file trace
    /// records file access and synthetic data records nothing about a runtime.
    pub const fn observes_flight(&self) -> bool {
        matches!(
            self,
            Self::RuntimeObservation { .. } | Self::InstrumentedCapture { .. }
        )
    }
}

/// The capture equipment, software and procedure, as recorded.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureMethod {
    /// Recording equipment (camera, capture card, display rate as recorded).
    pub equipment: String,
    /// Capture software and its version.
    pub software: String,
    /// The written procedure the operator followed.
    pub procedure: String,
}

/// One capture record: an operator worksheet row, ready to be validated.
///
/// Every field is present in the type and every "not captured yet" is an
/// explicit [`Measurement::Unknown`], so a partially filled worksheet is
/// visible as missing data instead of reading as a complete record.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRecord {
    /// Stable identifier of this record within its [`ReferenceSet`].
    pub id: String,
    /// How the samples were produced.
    pub source: SampleSource,
    /// What the record is offered as evidence for; must agree with `source`.
    pub claim: RecordClaim,
    /// Whether this record may be fitted or is reserved as the holdout.
    pub role: SampleRole,
    /// The original the capture is fingerprinted against.
    pub identity: OriginalIdentity,
    /// Difficulty, assists and setting notes the capture ran under.
    pub settings: SettingsRecord,
    /// Original mission identity as recorded.
    pub mission: Measurement<String>,
    /// Original airframe identity as recorded.
    pub airframe: Measurement<String>,
    /// Original loadout identity as recorded.
    pub loadout: Measurement<String>,
    /// Which operator worksheet this record belongs to.
    pub worksheet: Worksheet,
    /// The clock the sample times live on.
    pub timebase: Timebase,
    /// Declared quantity/unit pairs with conversion provenance.
    pub units: Vec<UnitDeclaration>,
    /// The input/time series and its source.
    pub series: SampleSeries,
    /// What the record was observed with.
    pub basis: Vec<CaptureBasis>,
    /// Who observed the capture.
    pub observer: Measurement<String>,
    /// How the capture was made.
    pub capture_method: Measurement<CaptureMethod>,
    /// Private artifacts behind this record and their required digests.
    pub artifacts: Vec<ArtifactRef>,
    /// Free-form notes, including fixture/limitation notes.
    pub notes: Vec<String>,
}

impl CaptureRecord {
    /// The reserved maneuver of this record, when it is a baseline flight
    /// probe.
    pub fn worksheet_maneuver(&self) -> Option<&ManeuverKind> {
        match &self.worksheet {
            Worksheet::BaselineFlight { maneuver } => Some(maneuver),
            Worksheet::FirstMissionBranch { .. } => None,
        }
    }
}

/// The maneuver held back from every fit so a tuned model cannot pass by
/// matching the data it was fitted to.
///
/// Reserve it *before* selecting tolerances or fitting: FLIGHT-PHYSICS
/// requires a held-out maneuver, and a set without captured holdout data never
/// reaches [`FidelityComparison::Ready`].
#[derive(Clone, Debug, PartialEq)]
pub struct HoldoutReservation {
    /// The maneuver reserved from fitting.
    pub maneuver: ManeuverKind,
    /// Why this maneuver was reserved (recorded, never assumed).
    pub rationale: String,
}

/// A set of capture records plus the maneuver reserved from fitting.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceSet {
    /// The reserved holdout maneuver.
    pub holdout: HoldoutReservation,
    /// The records of this set, calibration and holdout alike.
    pub records: Vec<CaptureRecord>,
}

/// Why a record is refused — a defect, not merely missing data.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureError {
    /// An original-observed record does not fingerprint the original it claims
    /// to have observed.
    MissingFingerprint {
        /// Which fingerprint is absent.
        field: FingerprintField,
    },
    /// The record has samples but its timebase cannot place them: a field is
    /// `Unknown`, empty, or does not determine a timeline (a non-positive
    /// rate, an uncertainty without a method).
    AmbiguousTimebase {
        /// Which field of the timebase is unusable.
        detail: String,
    },
    /// A value that must be finite is not (sample time, input, observation,
    /// rate, uncertainty, conversion factor).
    NonFinite {
        /// The field that carried the non-finite value.
        field: String,
    },
    /// An artifact re-hashed to something other than its declared digest.
    ArtifactHashMismatch {
        /// The artifact's relative spelling.
        path: String,
        /// The digest the record declared.
        declared: String,
        /// The digest the bytes actually hash to.
        observed: String,
    },
    /// An artifact spelling would escape the artifact root.
    UnsafeArtifactPath {
        /// The refused spelling.
        path: String,
    },
    /// A sample observation names a (quantity, unit) pair that is not declared
    /// in the record's unit table — units are declared, never guessed.
    UndeclaredUnit {
        /// The quantity that was observed.
        quantity: String,
        /// The unit that was not declared for it.
        unit: String,
    },
    /// A declared conversion is unusable (factor not greater than zero).
    InvalidConversionFactor {
        /// The quantity whose conversion is refused.
        quantity: String,
        /// The unit whose conversion is refused.
        unit: String,
    },
    /// The record's samples and its claim disagree — the acceptance case is a
    /// synthetic record offered as original-game evidence.
    SourceClaimMismatch {
        /// How the samples were produced.
        source: SampleSource,
        /// What the record is offered as.
        claim: RecordClaim,
    },
    /// The record and its series name different sources: an original-observed
    /// series may never ride along in a remake record, or the reverse.
    MixedSampleSources {
        /// The source the record states.
        record: SampleSource,
        /// The source its series states.
        series: SampleSource,
    },
    /// The record claims flight/mission behavior — for the original or for
    /// this reimplementation — but nothing it was observed with can show
    /// flight: a file-access trace (Rally #341) records which files were
    /// opened, never how the aircraft flew, and authored synthetic data
    /// records no runtime at all. A behavior claim needs a flight-observing
    /// basis ([`CaptureBasis::observes_flight`]).
    NoFlightObservingBasis {
        /// The claim that has no flight-observing basis behind it.
        claim: RecordClaim,
    },
    /// The record carries the reserved holdout maneuver but is marked as
    /// calibration, so the holdout would leak into the fit.
    HoldoutUsedForCalibration {
        /// The reserved maneuver that was used for fitting.
        maneuver: String,
    },
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFingerprint { field } => {
                write!(f, "missing fingerprint: {field} is not recorded")
            }
            Self::AmbiguousTimebase { detail } => {
                write!(f, "ambiguous timebase: {detail}")
            }
            Self::NonFinite { field } => write!(f, "non-finite value in {field}"),
            Self::ArtifactHashMismatch {
                path,
                declared,
                observed,
            } => write!(
                f,
                "artifact {path} hashes to {observed}, record declares {declared}"
            ),
            Self::UnsafeArtifactPath { path } => {
                write!(f, "artifact path {path} escapes the artifact root")
            }
            Self::UndeclaredUnit { quantity, unit } => {
                write!(f, "unit {unit:?} for quantity {quantity:?} is not declared")
            }
            Self::InvalidConversionFactor { quantity, unit } => {
                write!(
                    f,
                    "declared conversion for {quantity:?} in {unit:?} has no positive factor"
                )
            }
            Self::SourceClaimMismatch { source, claim } => write!(
                f,
                "record claims {claim} but its samples come from {source}"
            ),
            Self::MixedSampleSources { record, series } => write!(
                f,
                "record source {record} differs from its series source {series}"
            ),
            Self::NoFlightObservingBasis { claim } => write!(
                f,
                "claim {claim} has no flight-observing basis: a file-access trace \
                 records file access and authored synthetic data records no \
                 runtime, so neither shows how the aircraft flew"
            ),
            Self::HoldoutUsedForCalibration { maneuver } => write!(
                f,
                "reserved holdout maneuver {maneuver} is marked as calibration data"
            ),
        }
    }
}

/// Why a structurally sound record still cannot be used — the data is not
/// there. Never reported as a pass.
#[derive(Clone, Debug, PartialEq)]
pub enum UnavailableReason {
    /// The record has no samples yet.
    NoSamples {
        /// The record that has no samples.
        record: String,
    },
    /// The record has samples but not one observed quantity among them, so
    /// nothing was actually measured: an input/time series with no observation
    /// carries no capture data to compare.
    NoObservations {
        /// The record whose samples carry no observation.
        record: String,
    },
    /// The record references no private artifact, so nothing can be re-checked.
    NoArtifacts {
        /// The record that references no artifact.
        record: String,
    },
    /// Artifacts are referenced but no artifact root was supplied to the
    /// validator, so their hashes are unchecked.
    ArtifactRootNotSupplied {
        /// The record whose artifacts are unchecked.
        record: String,
        /// The unchecked artifact spelling.
        path: String,
    },
    /// A referenced artifact is not readable under the root.
    ArtifactMissing {
        /// The record whose artifact is missing.
        record: String,
        /// The unreadable artifact spelling.
        path: String,
    },
    /// Required capture context is not recorded: the observer or capture
    /// method, the capture basis, or — for a record offered as behavior
    /// evidence — the identities the claim is about (mission, airframe,
    /// loadout, difficulty, assists) and the subject of its own worksheet row
    /// (branch and spawn context for a first-mission row).
    MissingCaptureContext {
        /// The record missing its capture context.
        record: String,
        /// What exactly is missing.
        detail: String,
    },
    /// The set holds no calibration record at all.
    NoCalibrationRecords,
    /// The reserved holdout maneuver has no captured record.
    HoldoutNotCaptured {
        /// The reserved maneuver.
        maneuver: String,
    },
    /// The reserved holdout exists but at least one of its records is not
    /// original-observed, so it cannot support a claim about the original.
    HoldoutNotOriginal {
        /// The reserved maneuver.
        maneuver: String,
    },
    /// A calibration record of the reference set is not original-observed, so
    /// fitting on it would promote synthetic or remake data into an
    /// original-fidelity comparison.
    CalibrationNotOriginal {
        /// The record that is not original evidence.
        record: String,
    },
}

impl fmt::Display for UnavailableReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSamples { record } => write!(f, "record {record}: no samples captured yet"),
            Self::NoObservations { record } => {
                write!(f, "record {record}: samples carry no observed quantity")
            }
            Self::NoArtifacts { record } => {
                write!(f, "record {record}: no private artifact referenced")
            }
            Self::ArtifactRootNotSupplied { record, path } => write!(
                f,
                "record {record}: artifact {path} unchecked, no artifact root supplied"
            ),
            Self::ArtifactMissing { record, path } => {
                write!(f, "record {record}: artifact {path} is not readable")
            }
            Self::MissingCaptureContext { record, detail } => {
                write!(f, "record {record}: capture context missing: {detail}")
            }
            Self::NoCalibrationRecords => write!(f, "set: no calibration record"),
            Self::HoldoutNotCaptured { maneuver } => {
                write!(f, "set: reserved holdout {maneuver} not captured")
            }
            Self::HoldoutNotOriginal { maneuver } => write!(
                f,
                "set: reserved holdout {maneuver} is not original-observed evidence"
            ),
            Self::CalibrationNotOriginal { record } => {
                write!(
                    f,
                    "record {record}: calibration data is not original-observed"
                )
            }
        }
    }
}

/// The outcome of validating one capture record.
///
/// `invalid` and `unavailable` are collected independently and both are always
/// reported; consumers must look at [`Self::is_invalid`] first. A record is
/// valid only when **both** lists are empty, so missing capture data can never
/// be mistaken for a pass.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureReport {
    /// The record the report is about.
    pub record: String,
    /// Defects: the record must be corrected, not merely filled in.
    pub invalid: Vec<CaptureError>,
    /// Missing data: the record is honest but not yet usable.
    pub unavailable: Vec<UnavailableReason>,
}

impl CaptureReport {
    /// Every required field is present and consistent.
    pub fn is_valid(&self) -> bool {
        self.invalid.is_empty() && self.unavailable.is_empty()
    }

    /// The record is defective.
    pub fn is_invalid(&self) -> bool {
        !self.invalid.is_empty()
    }

    /// The record is structurally sound but its capture data is missing.
    ///
    /// False when the record is also defective: defects take precedence, and
    /// neither state is a pass.
    pub fn is_unavailable(&self) -> bool {
        self.invalid.is_empty() && !self.unavailable.is_empty()
    }

    /// One stderr-suitable diagnostic line per finding, defects first.
    pub fn diagnostic_lines(&self) -> Vec<String> {
        self.invalid
            .iter()
            .map(|error| format!("record {}: invalid: {error}", self.record))
            .chain(
                self.unavailable
                    .iter()
                    .map(|reason| format!("record {}: unavailable: {reason}", self.record)),
            )
            .collect()
    }
}

/// What a validation run was given to check artifacts against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidationContext {
    /// Root directory the artifact spellings are relative to. `None` leaves
    /// every referenced artifact unchecked — and reports it as unavailable
    /// rather than as a pass.
    pub artifact_root: Option<PathBuf>,
}

impl ValidationContext {
    /// A context that resolves artifacts under `root`.
    pub fn with_artifact_root(root: impl Into<PathBuf>) -> Self {
        Self {
            artifact_root: Some(root.into()),
        }
    }

    /// A context with no artifact root: artifact hashes cannot be checked.
    pub fn without_artifacts() -> Self {
        Self::default()
    }
}

/// Whether a `/`-separated relative spelling stays inside the artifact root.
///
/// Refused: empty spellings, absolute spellings, `.`/`..` components and
/// backslashes (spellings are `/`-separated by definition). This runs before
/// any file is opened.
fn safe_relative_spelling(spelling: &str) -> bool {
    if spelling.is_empty() || spelling.starts_with('/') || spelling.contains('\\') {
        return false;
    }
    spelling
        .split('/')
        .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// Streams a file through SHA-256 in 64 KiB reads, the way installation
/// discovery hashes its files, so the digest describes the bytes read.
fn hash_file(path: &Path) -> std::io::Result<ContentHash> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize())
}

/// Whether a text measurement is present and non-blank.
fn text_missing(measurement: &Measurement<String>) -> bool {
    match measurement {
        Measurement::Unknown { .. } => true,
        Measurement::Known(text) => text.trim().is_empty(),
    }
}

/// Records a required text measurement that is absent or blank as missing
/// capture context, naming the field and — when there is one — the reason the
/// value was never captured. A present, non-blank value adds nothing.
fn require_text(report: &mut CaptureReport, field: &str, measurement: &Measurement<String>) {
    let detail = match measurement {
        Measurement::Unknown { reason } => format!("{field} ({reason})"),
        Measurement::Known(text) if text.trim().is_empty() => format!("{field} is blank"),
        Measurement::Known(_) => return,
    };
    report
        .unavailable
        .push(UnavailableReason::MissingCaptureContext {
            record: report.record.clone(),
            detail,
        });
}

/// Fingerprints an original-observed record must carry.
///
/// An absent fingerprint is `Unknown`, and an asserted-but-blank edition
/// counts as absent too: an empty name fingerprints no edition.
fn check_fingerprints(record: &CaptureRecord, invalid: &mut Vec<CaptureError>) {
    if record.source != SampleSource::OriginalObserved {
        return;
    }
    for (present, field) in [
        (
            !text_missing(&record.identity.edition),
            FingerprintField::Edition,
        ),
        (
            record.identity.executable_sha256.is_known(),
            FingerprintField::Executable,
        ),
        (
            record.identity.installation_sha256.is_known(),
            FingerprintField::Installation,
        ),
    ] {
        if !present {
            invalid.push(CaptureError::MissingFingerprint { field });
        }
    }
}

/// The claim must agree with how the samples were produced.
fn check_source_and_claim(record: &CaptureRecord, invalid: &mut Vec<CaptureError>) {
    let supported = match record.claim {
        RecordClaim::OriginalBehavior => record.source == SampleSource::OriginalObserved,
        RecordClaim::RemakeBehavior => record.source == SampleSource::RemakeSample,
        RecordClaim::WorksheetOnly => true,
    };
    if !supported {
        invalid.push(CaptureError::SourceClaimMismatch {
            source: record.source,
            claim: record.claim,
        });
    }
    if record.series.source != record.source {
        invalid.push(CaptureError::MixedSampleSources {
            record: record.source,
            series: record.series.source,
        });
    }
}

/// Basis, observer, capture method and — for a behavior claim — the identities
/// the claim is about: missing context is unavailable, a behavior claim with
/// no flight-observing basis is a defect.
///
/// The basis rule covers **every** behavior claim, not just one about the
/// original: a file-access trace records which files were opened and authored
/// synthetic data records no runtime, so neither can back a claim about how
/// the aircraft flew, whoever it is claimed for. A `worksheet_only` record
/// offers no behavior, so it is held to no identity requirement — its missing
/// samples and artifacts still report it as unavailable.
fn check_capture_context(record: &CaptureRecord, report: &mut CaptureReport) {
    if record.basis.is_empty() {
        report
            .unavailable
            .push(UnavailableReason::MissingCaptureContext {
                record: report.record.clone(),
                detail: "no capture basis recorded".to_owned(),
            });
    } else if record.claim != RecordClaim::WorksheetOnly
        && !record.basis.iter().any(CaptureBasis::observes_flight)
    {
        report.invalid.push(CaptureError::NoFlightObservingBasis {
            claim: record.claim,
        });
    }

    require_text(report, "observer identity", &record.observer);
    match &record.capture_method {
        Measurement::Unknown { reason } => {
            report
                .unavailable
                .push(UnavailableReason::MissingCaptureContext {
                    record: report.record.clone(),
                    detail: format!("capture method ({reason})"),
                });
        }
        Measurement::Known(method) => {
            let blank = [
                ("equipment", method.equipment.trim()),
                ("software", method.software.trim()),
                ("procedure", method.procedure.trim()),
            ]
            .iter()
            .filter(|(_, value)| value.is_empty())
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
            if !blank.is_empty() {
                report
                    .unavailable
                    .push(UnavailableReason::MissingCaptureContext {
                        record: report.record.clone(),
                        detail: format!("capture method has blank {}", blank.join(", ")),
                    });
            }
        }
    }

    if record.claim == RecordClaim::WorksheetOnly {
        return;
    }
    require_text(report, "mission identity", &record.mission);
    require_text(report, "airframe identity", &record.airframe);
    require_text(report, "loadout identity", &record.loadout);
    require_text(report, "difficulty", &record.settings.difficulty);
    if let Measurement::Unknown { reason } = &record.settings.assists {
        report
            .unavailable
            .push(UnavailableReason::MissingCaptureContext {
                record: report.record.clone(),
                detail: format!("assists ({reason})"),
            });
    }
    if let Worksheet::FirstMissionBranch {
        branch,
        spawn_context,
    } = &record.worksheet
    {
        require_text(report, "branch identity", branch);
        require_text(report, "spawn context", spawn_context);
    }
}

/// The timebase: only a record with samples makes timing claims, and then all
/// five fields must be present and coherent.
///
/// An unfilled timebase on a record without samples is not checked here — the
/// missing samples themselves report the record as unavailable.
fn check_timebase(record: &CaptureRecord, invalid: &mut Vec<CaptureError>) {
    let timebase = &record.timebase;

    for (field, measurement) in [
        ("timebase.clock", &timebase.clock),
        ("timebase.origin", &timebase.origin),
    ] {
        match measurement {
            Measurement::Unknown { .. } => invalid.push(CaptureError::AmbiguousTimebase {
                detail: format!(
                    "samples exist but {} is not established",
                    field.strip_prefix("timebase.").unwrap_or(field)
                ),
            }),
            Measurement::Known(text) if text.trim().is_empty() => {
                invalid.push(CaptureError::AmbiguousTimebase {
                    detail: format!("{field} is asserted but empty"),
                });
            }
            Measurement::Known(_) => {}
        }
    }
    if !timebase.nominal_rate_hz.is_known() {
        invalid.push(CaptureError::AmbiguousTimebase {
            detail: "samples exist but no nominal sample rate is established".to_owned(),
        });
    }
    if !timebase.uncertainty.is_known() {
        invalid.push(CaptureError::AmbiguousTimebase {
            detail: "samples exist but no measured timing uncertainty is established".to_owned(),
        });
    }

    if timebase.unit.trim().is_empty() {
        invalid.push(CaptureError::AmbiguousTimebase {
            detail: "samples exist but the time unit is not declared".to_owned(),
        });
    }

    if let Measurement::Known(rate) = timebase.nominal_rate_hz {
        if !rate.is_finite() {
            invalid.push(CaptureError::NonFinite {
                field: "timebase.nominal_rate_hz".to_owned(),
            });
        } else if rate <= 0.0 {
            invalid.push(CaptureError::AmbiguousTimebase {
                detail: format!("nominal sample rate {rate} does not determine a timeline"),
            });
        }
    }

    if let Some(uncertainty) = timebase.uncertainty.known() {
        if !uncertainty.plus_minus.is_finite() {
            invalid.push(CaptureError::NonFinite {
                field: "timebase.uncertainty.plus_minus".to_owned(),
            });
        } else if uncertainty.plus_minus < 0.0 {
            invalid.push(CaptureError::AmbiguousTimebase {
                detail: format!("timing uncertainty {} is negative", uncertainty.plus_minus),
            });
        }
        if uncertainty.method.trim().is_empty() {
            invalid.push(CaptureError::AmbiguousTimebase {
                detail: "timing uncertainty is asserted without a measurement method".to_owned(),
            });
        }
    }
}

/// Units: the time unit and every observed (quantity, unit) pair must be
/// declared, and a declared conversion must have a usable factor.
fn check_units(record: &CaptureRecord, invalid: &mut Vec<CaptureError>) {
    let declared = |quantity: &str, unit: &str| {
        record
            .units
            .iter()
            .any(|declaration| declaration.quantity == quantity && declaration.unit == unit)
    };

    if !declared(TIME_QUANTITY, &record.timebase.unit) {
        invalid.push(CaptureError::UndeclaredUnit {
            quantity: TIME_QUANTITY.to_owned(),
            unit: record.timebase.unit.clone(),
        });
    }
    for (index, sample) in record.series.samples.iter().enumerate() {
        for (offset, observation) in sample.observations.iter().enumerate() {
            if !declared(&observation.quantity, &observation.unit) {
                invalid.push(CaptureError::UndeclaredUnit {
                    quantity: observation.quantity.clone(),
                    unit: observation.unit.clone(),
                });
            }
            if !observation.value.is_finite() {
                invalid.push(CaptureError::NonFinite {
                    field: format!("samples[{index}].observations[{offset}].value"),
                });
            }
        }
        if !sample.t.is_finite() {
            invalid.push(CaptureError::NonFinite {
                field: format!("samples[{index}].t"),
            });
        }
        for (name, value) in [
            ("throttle", sample.input.throttle),
            ("pitch", sample.input.pitch),
            ("roll", sample.input.roll),
            ("yaw", sample.input.yaw),
        ] {
            if !value.is_finite() {
                invalid.push(CaptureError::NonFinite {
                    field: format!("samples[{index}].input.{name}"),
                });
            }
        }
    }

    for declaration in &record.units {
        if let UnitProvenance::Converted { factor, .. } = &declaration.provenance {
            if !factor.is_finite() {
                invalid.push(CaptureError::NonFinite {
                    field: format!("units[{}].conversion_factor", declaration.quantity),
                });
            } else if *factor <= 0.0 {
                invalid.push(CaptureError::InvalidConversionFactor {
                    quantity: declaration.quantity.clone(),
                    unit: declaration.unit.clone(),
                });
            }
        }
    }
}

/// Private files a record points at: spelling safety first, then re-hashing
/// against the artifact root.
///
/// Both the record's artifacts and any file-access trace shared as basis
/// metadata are checked — a declared digest that is never verified would be
/// decoration. A missing file is unavailable data; a digest that disagrees is
/// a defect.
fn check_artifacts(record: &CaptureRecord, ctx: &ValidationContext, report: &mut CaptureReport) {
    let mut referenced: Vec<(&str, ContentHash)> = record
        .artifacts
        .iter()
        .map(|artifact| (artifact.relative_path.as_str(), artifact.sha256))
        .collect();
    for basis in &record.basis {
        if let CaptureBasis::FileAccessTrace { trace } = basis {
            referenced.push((trace.relative_path.as_str(), trace.sha256));
        }
    }

    for (spelling, declared) in referenced {
        if !safe_relative_spelling(spelling) {
            report.invalid.push(CaptureError::UnsafeArtifactPath {
                path: spelling.to_owned(),
            });
            continue;
        }
        let Some(root) = ctx.artifact_root.as_ref() else {
            report
                .unavailable
                .push(UnavailableReason::ArtifactRootNotSupplied {
                    record: report.record.clone(),
                    path: spelling.to_owned(),
                });
            continue;
        };
        match hash_file(&root.join(spelling)) {
            Err(_) => report.unavailable.push(UnavailableReason::ArtifactMissing {
                record: report.record.clone(),
                path: spelling.to_owned(),
            }),
            Ok(observed) if observed != declared => {
                report.invalid.push(CaptureError::ArtifactHashMismatch {
                    path: spelling.to_owned(),
                    declared: declared.to_hex(),
                    observed: observed.to_hex(),
                });
            }
            Ok(_) => {}
        }
    }
}

/// Validates one capture record against the protocol rules.
///
/// The result is three-valued: a defect ([`CaptureReport::is_invalid`]),
/// missing capture data ([`CaptureReport::is_unavailable`]), or a fully valid
/// record ([`CaptureReport::is_valid`]). Both lists are filled independently,
/// so a defective record with missing data still reports its defects.
///
/// `ctx.artifact_root` is where artifact spellings resolve; without it every
/// referenced artifact is reported unchecked (unavailable), never as matching.
pub fn validate_capture(record: &CaptureRecord, ctx: &ValidationContext) -> CaptureReport {
    let mut report = CaptureReport {
        record: record.id.clone(),
        invalid: Vec::new(),
        unavailable: Vec::new(),
    };

    check_fingerprints(record, &mut report.invalid);
    check_source_and_claim(record, &mut report.invalid);
    check_capture_context(record, &mut report);

    if record.series.samples.is_empty() {
        report.unavailable.push(UnavailableReason::NoSamples {
            record: record.id.clone(),
        });
    } else {
        check_timebase(record, &mut report.invalid);
        check_units(record, &mut report.invalid);
        if record
            .series
            .samples
            .iter()
            .all(|sample| sample.observations.is_empty())
        {
            report.unavailable.push(UnavailableReason::NoObservations {
                record: record.id.clone(),
            });
        }
    }
    if record.artifacts.is_empty() {
        report.unavailable.push(UnavailableReason::NoArtifacts {
            record: record.id.clone(),
        });
    }
    check_artifacts(record, ctx, &mut report);

    report
}

/// What a record set can support for a handling-fidelity comparison.
///
/// `Ready` is structural only: it means a holdout comparison *can be run*,
/// never that fidelity was established or that a tolerance was met. F26 owns
/// the comparison itself; this gate decides whether original reference data
/// exists to compare against.
#[derive(Clone, Debug, PartialEq)]
pub enum FidelityComparison {
    /// Every calibration record and every record of the reserved holdout are
    /// original-observed and defect-free.
    Ready {
        /// Ids of the records that may be fitted.
        calibration: Vec<String>,
        /// The reserved holdout maneuver that was captured.
        holdout: String,
    },
    /// Reference data is missing: the comparison cannot be run, and this is
    /// never reported as a pass.
    Unavailable(Vec<UnavailableReason>),
    /// The set is defective and must be corrected first.
    Invalid(Vec<CaptureError>),
}

impl FidelityComparison {
    /// Whether a holdout comparison can be run at all.
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// One stderr-suitable diagnostic line per finding.
    pub fn diagnostic_lines(&self) -> Vec<String> {
        match self {
            Self::Ready { .. } => Vec::new(),
            Self::Unavailable(reasons) => reasons
                .iter()
                .map(|reason| format!("unavailable: {reason}"))
                .collect(),
            Self::Invalid(errors) => errors
                .iter()
                .map(|error| format!("invalid: {error}"))
                .collect(),
        }
    }
}

/// Gates a record set for a handling-fidelity comparison.
///
/// Every record is validated first; any defect makes the whole set
/// [`FidelityComparison::Invalid`]. The set is `Ready` only when it holds at
/// least one calibration record **and** a captured record for the reserved
/// holdout maneuver, with **every** participating record — calibration and
/// holdout alike — original-observed: fitting on synthetic or remake data and
/// validating on an original holdout would promote data of one class into a
/// claim about the other, so it reads `Unavailable`
/// ([`UnavailableReason::CalibrationNotOriginal`],
/// [`UnavailableReason::HoldoutNotOriginal`]). Fitting acceleration alone
/// therefore stays `Unavailable`: without the held-out maneuver there is
/// nothing the fit did not see.
///
/// The reservation is enforced, not merely documented: a record carrying the
/// reserved maneuver but marked `Calibration` is a defect
/// ([`CaptureError::HoldoutUsedForCalibration`]).
pub fn fidelity_comparison(set: &ReferenceSet, ctx: &ValidationContext) -> FidelityComparison {
    let mut invalid = Vec::new();
    let mut unavailable = Vec::new();
    let mut calibration = Vec::new();
    let mut holdout = Vec::new();

    for record in &set.records {
        let report = validate_capture(record, ctx);
        invalid.extend(report.invalid);
        unavailable.extend(report.unavailable);

        let reserved = record.worksheet_maneuver() == Some(&set.holdout.maneuver);
        match record.role {
            SampleRole::Calibration if reserved => {
                invalid.push(CaptureError::HoldoutUsedForCalibration {
                    maneuver: set.holdout.maneuver.label().to_owned(),
                });
            }
            SampleRole::Calibration => {
                if record.source != SampleSource::OriginalObserved {
                    unavailable.push(UnavailableReason::CalibrationNotOriginal {
                        record: record.id.clone(),
                    });
                }
                calibration.push(record.id.clone());
            }
            SampleRole::Holdout if reserved => holdout.push(record),
            SampleRole::Holdout => {}
        }
    }

    if !invalid.is_empty() {
        return FidelityComparison::Invalid(invalid);
    }
    if calibration.is_empty() {
        unavailable.push(UnavailableReason::NoCalibrationRecords);
    }
    if holdout.is_empty() {
        unavailable.push(UnavailableReason::HoldoutNotCaptured {
            maneuver: set.holdout.maneuver.label().to_owned(),
        });
    } else if holdout
        .iter()
        .any(|record| record.source != SampleSource::OriginalObserved)
    {
        unavailable.push(UnavailableReason::HoldoutNotOriginal {
            maneuver: set.holdout.maneuver.label().to_owned(),
        });
    }
    if !unavailable.is_empty() {
        return FidelityComparison::Unavailable(unavailable);
    }
    FidelityComparison::Ready {
        calibration,
        holdout: set.holdout.maneuver.label().to_owned(),
    }
}

/// A well-formed **synthetic** capture record: the shape a complete record has,
/// with every value labeled as authored fixture data.
///
/// It validates clean when `artifact` re-hashes to its declared digest under
/// the artifact root. Its samples, units, clock and uncertainty are authored
/// fixture values, recorded as such in [`CaptureRecord::notes`]; nothing in it
/// is an original measurement, and its [`RecordClaim::WorksheetOnly`] keeps it
/// from ever being offered as behavior evidence.
pub fn synthetic_capture_record(artifact: ArtifactRef) -> CaptureRecord {
    let fixture_note = "synthetic fixture: authored sample values, units, clock and \
                        uncertainty; not original measurements"
        .to_owned();
    /// The identity of a fixture that never involved an original installation.
    fn not_captured<T>() -> Measurement<T> {
        Measurement::Unknown {
            reason: "synthetic fixture: no original installation involved".to_owned(),
        }
    }

    CaptureRecord {
        id: "fixture.synthetic-baseline".to_owned(),
        source: SampleSource::SyntheticFixture,
        claim: RecordClaim::WorksheetOnly,
        role: SampleRole::Calibration,
        identity: OriginalIdentity {
            edition: not_captured(),
            executable_sha256: not_captured(),
            installation_sha256: not_captured(),
        },
        settings: SettingsRecord {
            difficulty: Measurement::Unknown {
                reason: "synthetic fixture: no difficulty selected".to_owned(),
            },
            assists: Measurement::Unknown {
                reason: "synthetic fixture: no assists configured".to_owned(),
            },
            notes: Vec::new(),
        },
        mission: Measurement::Unknown {
            reason: "synthetic fixture: no original mission identity".to_owned(),
        },
        airframe: Measurement::Unknown {
            reason: "synthetic fixture: no original airframe identity".to_owned(),
        },
        loadout: Measurement::Unknown {
            reason: "synthetic fixture: no original loadout identity".to_owned(),
        },
        worksheet: Worksheet::BaselineFlight {
            maneuver: ManeuverKind::Acceleration,
        },
        timebase: Timebase {
            clock: Measurement::Known("fixture capture clock (authored)".to_owned()),
            origin: Measurement::Known("first sample (authored)".to_owned()),
            unit: "s".to_owned(),
            nominal_rate_hz: Measurement::Known(2.0),
            uncertainty: Measurement::Known(TimingUncertainty {
                plus_minus: 0.001,
                method: "fixture: authored, not measured".to_owned(),
            }),
        },
        units: vec![
            UnitDeclaration {
                quantity: TIME_QUANTITY.to_owned(),
                unit: "s".to_owned(),
                provenance: UnitProvenance::ReportedByCaptureTool {
                    tool: "authored fixture".to_owned(),
                },
            },
            UnitDeclaration {
                quantity: "world_speed".to_owned(),
                unit: "m/s".to_owned(),
                provenance: UnitProvenance::Unknown {
                    reason: "synthetic fixture: no conversion established".to_owned(),
                },
            },
        ],
        series: SampleSeries {
            source: SampleSource::SyntheticFixture,
            samples: vec![
                Sample {
                    t: 0.0,
                    input: FlightInput {
                        throttle: 0.0,
                        pitch: 0.0,
                        roll: 0.0,
                        yaw: 0.0,
                        boost: false,
                    },
                    observations: vec![Observation {
                        quantity: "world_speed".to_owned(),
                        unit: "m/s".to_owned(),
                        value: 0.0,
                    }],
                },
                Sample {
                    t: 0.5,
                    input: FlightInput {
                        throttle: 1.0,
                        pitch: 0.0,
                        roll: 0.0,
                        yaw: 0.0,
                        boost: false,
                    },
                    observations: vec![Observation {
                        quantity: "world_speed".to_owned(),
                        unit: "m/s".to_owned(),
                        value: 30.0,
                    }],
                },
            ],
        },
        basis: vec![CaptureBasis::SyntheticSimulation {
            detail: "authored fixture samples; no runtime behind them".to_owned(),
        }],
        observer: Measurement::Known("fixture observer".to_owned()),
        capture_method: Measurement::Known(CaptureMethod {
            equipment: "authored fixture".to_owned(),
            software: "authored fixture".to_owned(),
            procedure: "authored fixture".to_owned(),
        }),
        artifacts: vec![artifact],
        notes: vec![fixture_note],
    }
}
