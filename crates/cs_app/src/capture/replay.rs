//! Deterministic input and state capture: the production path that records a
//! replay and replays it (F59-B).
//!
//! Spec: `specs/F59-replays-captures-probes-and-acceptance-evidence.md`,
//! stage `### F59-B`. Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! F59-A built the record and said the state hashes are produced by a running
//! session. This is that session's capture path. It drives the production flight
//! world — the same [`PhysicsSession`], the same
//! [`FlightForcesPlugin`], the same [`spawn_flight_body`], the same
//! [`FlightModel`] — from a recorded [`CommandStream`], and measures every
//! tick's state back out of the world through [`super::state::StateProbe`].
//!
//! # The two halves
//!
//! * [`record`] flies a stream and returns the [`ReplayRecord`] that *promises*
//!   the state it measured. The promise is measured, not derived: it comes from
//!   the same probe the replay path uses, over the same world.
//! * [`replay`] flies the record's own stream again in a **fresh** world and
//!   reports two separate answers: the cross-build
//!   [`CompatibilityVerdict`] against the record, and AC01's
//!   [`EnvelopeComparison`] of the measured envelope against the promise. They
//!   are kept apart because they answer different questions — "is this the same
//!   run?" and "did it reproduce?" — and a change of content (AC02) must be
//!   reported as the first even when it also moved the state.
//!
//! # What is never faked
//!
//! * **The state hash is not a hash of the input.** Every digest comes from a
//!   pose read out of Avian and forces the tick's own law computed. A replay
//!   whose input changed but whose aircraft did not therefore compares
//!   *identical*, which is the truth rather than a missed divergence.
//! * **A tick's forces are that tick's.** The aircraft stamps its last output
//!   with the tick that measured it, and a reading is refused unless the two
//!   agree — a refused tick keeps the previous output, and stamping that as a
//!   fresh measurement would be the one fabrication this path must not make.
//! * **The content digest is not a constant.** It is the digest of the content
//!   rows the subject actually loaded ([`super::identity`]), so editing a loaded
//!   airframe coefficient is a content change and AC02 refuses the old replay.
//! * **An input the run cannot execute is refused.** The fixed-wing path has no
//!   weapon, ordnance or menu consumer, so a recorded edge for one of those is
//!   [`CaptureRunError::UnconsumedAction`] — never a silently dropped press.
//! * **No policy is defaulted.** [`replay`] takes the caller's
//!   [`CrossBuildPolicy`]. [`CrossBuildPolicy::Reject`] is F59's initial
//!   determinism target; [`CrossBuildPolicy::BestEffort`] returns a verdict that
//!   names the differences and certifies no determinism
//!   ([`CompatibilityVerdict::certifies_determinism`]), so a caller that reaches
//!   for it has to say why in whatever it does next.
//! * **No production profile is written.** The run's own bookkeeping goes
//!   through [`OverrideLog`]; a subject that asked for a production-profile
//!   write says so in its record rather than doing it behind the record's back.

use std::fmt;

use bevy::ecs::entity::Entity;
use cs_content::replay::{
    AuthoredChoices, BuildId, CompatibilityVerdict, CrossBuildPolicy, Divergence,
    EnvelopeComparison, MAX_ENVELOPE_ENTRIES, OverrideLog, PlatformTag, ReplayError, ReplayRecord,
    ReplaySeeds, ReplayVersion, StateEnvelope,
};
use cs_sim::control::{ControlBuffer, ControlError};
use cs_sim::flight::{
    AirframeTuning, AirframeTuningError, FlightInput, FlightInputError, FlightModel,
};
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::input::{CommandStream, FlightCommand};

use crate::physics::{
    BASELINE_FIXED_HZ, FlightAircraft, FlightAircraftError, FlightForcesPlugin, FlightSpawnError,
    FlightSpawnSpec, PhysicsSession, PhysicsSessionError, spawn_flight_body,
};

use super::identity::{
    AIRFRAME_CONTENT_KEY, LoadedContent, RunIdentity, airframe_content_digest, host_platform,
};
use super::state::{StateProbe, StateProbeError, StateReading};

/// The label the measured initial state is recorded under.
///
/// `@tick0` says *when* it was measured: before the first tick ran, from the
/// spawned body. A reader can therefore tell a start-of-run measurement from a
/// measurement taken after one step, which a bare digest would not say.
pub const INITIAL_STATE_LABEL_SUFFIX: &str = "@tick0";

/// The build coordinates a capture is recorded under.
///
/// These name the checkout and the toolchain rather than anything measurable at
/// runtime: the candidate tree is `CS_CANDIDATE_TREE` (`git rev-parse
/// HEAD^{tree}` on a clean checkout) and the toolchain is what the binary
/// reports. Reading them from the environment is F59-C's command wiring, so a
/// caller supplies them here and the value that ends up in the record is the one
/// the caller can prove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildContext {
    /// The candidate tree the binary was built from.
    pub tree: BuildId,
    /// The engine and toolchain version string.
    pub toolchain: String,
    /// The platform the build ran on.
    pub platform: PlatformTag,
}

impl BuildContext {
    /// A build context for an explicit platform.
    ///
    /// The toolchain string is checked when the record is assembled rather than
    /// here, so the refusal a caller sees names the record it invalidates rather
    /// than a context it merely built.
    #[must_use]
    pub fn new(tree: BuildId, toolchain: &str, platform: PlatformTag) -> Self {
        Self {
            tree,
            toolchain: toolchain.to_owned(),
            platform,
        }
    }

    /// A build context for the platform this binary runs on.
    #[must_use]
    pub fn on_host(tree: BuildId, toolchain: &str) -> Self {
        Self::new(tree, toolchain, host_platform())
    }
}

/// What one replayed run is.
///
/// Everything here is what the run *loaded* or *declared*, never what it
/// produced: the measured state comes back as a [`ReplayRecord`]. Holding the
/// subject apart from the record is what lets a caller ask "would this content
/// still replay that record?", which is AC02's question.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplaySubject {
    /// What is being replayed: a catalog id.
    pub subject: ContentId,
    /// The content the run loaded, digested into the record's content half.
    pub loaded: LoadedContent,
    /// The airframe tuning the run flies.
    pub tuning: AirframeTuning,
    /// Where and how the aircraft starts.
    pub spawn: FlightSpawnSpec,
    /// The fixed simulation rate, in ticks per second.
    pub fixed_hz: u32,
    /// The run's root seed, when it had one.
    pub seed: Option<u64>,
    /// The authored choices the run pins.
    pub choices: AuthoredChoices,
    /// The run's purpose, debug overrides and profile-write request.
    pub overrides: OverrideLog,
}

impl ReplaySubject {
    /// A flight subject: `tuning` as the one loaded content row, an ordinary
    /// play purpose and the engine's baseline fixed rate.
    ///
    /// The loaded row is keyed by
    /// [`AIRFRAME_CONTENT_KEY`](super::identity::AIRFRAME_CONTENT_KEY) and its
    /// digest is [`airframe_content_digest`] of the same tuning the run flies,
    /// so the content half of the record describes the airframe the world
    /// actually hosts.
    #[must_use]
    pub fn flight(subject: ContentId, tuning: AirframeTuning, spawn: FlightSpawnSpec) -> Self {
        let loaded =
            LoadedContent::of_record(AIRFRAME_CONTENT_KEY, airframe_content_digest(&tuning));
        Self {
            subject,
            loaded,
            tuning,
            spawn,
            fixed_hz: BASELINE_FIXED_HZ,
            seed: None,
            choices: AuthoredChoices::none(),
            overrides: OverrideLog::ordinary_play(),
        }
    }

    /// The same subject flying a different airframe tuning.
    ///
    /// The loaded content row follows the tuning, because a run that loads a
    /// different airframe has loaded different content: this is the change
    /// AC02 refuses.
    #[must_use]
    pub fn with_tuning(mut self, tuning: AirframeTuning) -> Self {
        self.loaded =
            LoadedContent::of_record(AIRFRAME_CONTENT_KEY, airframe_content_digest(&tuning));
        self.tuning = tuning;
        self
    }

    /// The same subject at `fixed_hz`.
    ///
    /// The rate is part of the declared rules, so this is a `Rules` difference
    /// rather than a content one.
    #[must_use]
    pub const fn with_fixed_hz(mut self, fixed_hz: u32) -> Self {
        self.fixed_hz = fixed_hz;
        self
    }

    /// The same subject under `overrides`.
    #[must_use]
    pub fn with_overrides(mut self, overrides: OverrideLog) -> Self {
        self.overrides = overrides;
        self
    }

    /// The same subject starting from `seed`.
    #[must_use]
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// The same subject pinning `choices`.
    #[must_use]
    pub fn with_choices(mut self, choices: AuthoredChoices) -> Self {
        self.choices = choices;
        self
    }
}

/// Why a deterministic capture run was refused.
#[derive(Debug)]
pub enum CaptureRunError {
    /// A fixed rate of zero was declared; there would be no fixed timestep.
    ZeroTickRate,
    /// The requested run measured no tick, so it promises nothing.
    NoTicksMeasured,
    /// The airframe tuning was rejected before any world was built.
    Tuning(AirframeTuningError),
    /// The spawn conditions were rejected; nothing was spawned.
    Spawn(FlightSpawnError),
    /// The session could not be built, stepped or read.
    Session(PhysicsSessionError),
    /// The aircraft refused its record, its command or its tick.
    Flight(FlightAircraftError),
    /// The tick's command was outside the model's declared ranges.
    Input(FlightInputError),
    /// The recorded frame was refused by the production input boundary.
    Control(ControlError),
    /// A state reading was refused.
    Probe(StateProbeError),
    /// The assembled record broke one of its own rules.
    Record(ReplayError),
    /// The recorded stream carries a tick this run does not reach, so replaying
    /// it would silently drop the input rather than execute it.
    StreamTickOutsideRun {
        /// The recorded tick.
        tick: Tick,
        /// The first tick the run drives.
        first: Tick,
        /// The last tick the run reaches.
        last: Tick,
    },
    /// The recorded stream carries a one-shot action this run has no consumer
    /// for.
    ///
    /// The fixed-wing capture path hosts no weapon, ordnance or menu system, so
    /// an edge for one of those cannot be executed here. Reporting it is the
    /// only honest outcome: dropping it would leave a replay that reproduces the
    /// state of a run whose press never happened.
    UnconsumedAction {
        /// The tick whose boundary received the action.
        tick: Tick,
        /// The action's stable label.
        action: String,
    },
    /// The aircraft left the world, so its state cannot be measured.
    BodyLost {
        /// The tick whose measurement found nothing.
        tick: Tick,
    },
    /// The tick ran no law, so it has no forces of its own to measure.
    ///
    /// [`StateReading::after_tick`] is where the same check refuses a stale
    /// measurement; this variant names the case where the aircraft has produced
    /// no output at all.
    UnmeasuredTick {
        /// The tick the reading was stamped for.
        tick: Tick,
        /// The tick the aircraft actually stamped its last output with, if any.
        measured_at: Option<Tick>,
    },
}

impl fmt::Display for CaptureRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroTickRate => write!(f, "a capture run must declare a nonzero fixed rate"),
            Self::NoTicksMeasured => {
                write!(f, "the run measured no tick, so it promises no state")
            }
            Self::Tuning(error) => write!(f, "airframe tuning: {error}"),
            Self::Spawn(error) => write!(f, "flight spawn: {error}"),
            Self::Session(error) => write!(f, "physics session: {error}"),
            Self::Flight(error) => write!(f, "aircraft: {error}"),
            Self::Input(error) => write!(f, "flight command: {error}"),
            Self::Control(error) => write!(f, "input boundary: {error}"),
            Self::Probe(error) => write!(f, "state probe: {error}"),
            Self::Record(error) => write!(f, "replay record: {error}"),
            Self::StreamTickOutsideRun { tick, first, last } => write!(
                f,
                "the recorded stream has a frame at tick {} outside the run's {}..={}",
                tick.0, first.0, last.0
            ),
            Self::UnconsumedAction { tick, action } => write!(
                f,
                "the recorded stream's {action} edge at tick {} has no consumer in this run",
                tick.0
            ),
            Self::BodyLost { tick } => {
                write!(f, "the aircraft left the world at tick {}", tick.0)
            }
            Self::UnmeasuredTick {
                tick,
                measured_at: Some(measured_at),
            } => write!(
                f,
                "tick {} ran no flight law; the aircraft's last output was measured at tick {}",
                tick.0, measured_at.0
            ),
            Self::UnmeasuredTick {
                tick,
                measured_at: None,
            } => write!(
                f,
                "tick {} ran no flight law; the aircraft has measured no tick yet",
                tick.0
            ),
        }
    }
}

impl std::error::Error for CaptureRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Tuning(error) => Some(error),
            Self::Spawn(error) => Some(error),
            Self::Session(error) => Some(error),
            Self::Flight(error) => Some(error),
            Self::Input(error) => Some(error),
            Self::Control(error) => Some(error),
            Self::Probe(error) => Some(error),
            Self::Record(error) => Some(error),
            Self::ZeroTickRate
            | Self::NoTicksMeasured
            | Self::StreamTickOutsideRun { .. }
            | Self::UnconsumedAction { .. }
            | Self::UnmeasuredTick { .. }
            | Self::BodyLost { .. } => None,
        }
    }
}

impl From<StateProbeError> for CaptureRunError {
    fn from(error: StateProbeError) -> Self {
        Self::Probe(error)
    }
}

/// What one replay produced.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayOutcome {
    /// The cross-build comparison under the policy the caller chose.
    pub verdict: CompatibilityVerdict,
    /// The per-tick state hashes this run measured.
    pub observed: StateEnvelope,
    /// AC01's comparison of `observed` against the record's promise.
    pub comparison: EnvelopeComparison,
    /// How many fixed ticks the replay actually ran.
    pub ticks: u64,
}

impl ReplayOutcome {
    /// Whether the measured envelope is the promised one, tick for tick.
    #[must_use]
    pub fn reproduces_promised_state(&self) -> bool {
        self.comparison.is_identical()
    }

    /// Where the measured envelope first parted company with the promise.
    #[must_use]
    pub fn divergence(&self) -> Option<Divergence> {
        self.comparison.divergence
    }
}

/// One recorded run: its subject, its build and the ticks it flew.
#[derive(Clone, Debug, PartialEq)]
pub struct RunRequest<'a> {
    /// What to fly and what it loads.
    pub subject: &'a ReplaySubject,
    /// The build coordinates the record names.
    pub build: &'a BuildContext,
    /// The recorded, quantized input stream to fly.
    pub stream: &'a CommandStream,
    /// How many fixed ticks to run.
    pub ticks: u64,
}

/// One recorded run: the promise and the readings behind it.
///
/// The readings are the *raw* measurements — the pose Avian reported and the
/// forces the tick's own law computed — so a caller (or a review) can inspect
/// what each promised hash was taken over instead of taking the hash's word for
/// it. `readings[0]` is the run's initial state and the rest are ticks
/// `1..=record.last_tick` in order.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedRun {
    /// The record the run produced.
    pub record: ReplayRecord,
    /// Every measured reading, oldest first, starting with the spawn state.
    pub readings: Vec<StateReading>,
}

/// Records a run: flies `stream` for `ticks` ticks and returns the promise.
///
/// The returned record's `promised` envelope is the state the world actually
/// produced, measured by [`StateProbe`]; nothing here writes a state hash from
/// the input or from a fixture description. Use [`record_run`] when the raw
/// readings behind the promise are wanted too.
///
/// # Errors
///
/// Every [`CaptureRunError`]. The two that matter most for a caller are
/// [`CaptureRunError::StreamTickOutsideRun`], which means the stream and the
/// requested tick count disagree, and [`CaptureRunError::UnconsumedAction`],
/// which means the stream carries a press this run cannot execute.
pub fn record(request: &RunRequest<'_>) -> Result<ReplayRecord, CaptureRunError> {
    record_run(request).map(|run| run.record)
}

/// Records a run and returns the measurements the promise was taken over.
///
/// # Errors
///
/// As [`record`].
pub fn record_run(request: &RunRequest<'_>) -> Result<RecordedRun, CaptureRunError> {
    fly(request)
}

/// Replays a record in a fresh world and reports what happened.
///
/// Two answers, deliberately separate:
///
/// * `verdict` compares the record against the run `subject` and `build` would
///   produce, under `policy`. A content change (AC02) is reported here as
///   [`CompatibilityDifference::Content`](cs_content::replay::CompatibilityDifference::Content)
///   and `certifies_determinism()` is false, even though the state moved too.
/// * `comparison` is AC01: the measured envelope against the promise, tick for
///   tick, with the first divergence named.
///
/// `policy` is required rather than defaulted. F59's initial determinism target
/// is [`CrossBuildPolicy::Reject`];
/// [`CrossBuildPolicy::BestEffort`] produces a verdict that names the
/// differences and certifies nothing, so a caller that asks for it is making a
/// statement about a comparison rather than about determinism.
///
/// # Errors
///
/// Every [`CaptureRunError`]. [`ReplayError`](CaptureRunError::Record) first, for
/// a record that breaks its own rules — an inverted tick range above all, since
/// the tick span this function flies is derived from it. A record whose stream
/// cannot be flown in `ticks` fixed ticks is
/// [`CaptureRunError::StreamTickOutsideRun`], and one that asks for a different
/// rate than `subject` flies at is refused by the record's own comparison.
pub fn replay(
    record: &ReplayRecord,
    subject: &ReplaySubject,
    build: &BuildContext,
    policy: CrossBuildPolicy,
) -> Result<ReplayOutcome, CaptureRunError> {
    // The record is a document that may have come from anywhere, so it is
    // checked against its own rules before anything is derived from it: the
    // tick span below is a subtraction over its declared range.
    record.validate().map_err(CaptureRunError::Record)?;
    let ticks = record.last_tick.0 - record.first_tick.0;
    let request = RunRequest {
        subject,
        build,
        stream: &record.stream,
        ticks,
    };
    let candidate = fly(&request)?;
    Ok(ReplayOutcome {
        verdict: record.compatibility_with(&candidate.record, policy),
        observed: candidate.record.promised.clone(),
        comparison: record.verdict_against(&candidate.record.promised),
        ticks,
    })
}

/// Flies one request and returns the record and the readings behind it.
///
/// Recording and replaying share this one function on purpose: a replay that
/// took a different path from the recording would be comparing a promise to
/// something other than a replay of it.
fn fly(request: &RunRequest<'_>) -> Result<RecordedRun, CaptureRunError> {
    let subject = request.subject;
    if subject.fixed_hz == 0 {
        return Err(CaptureRunError::ZeroTickRate);
    }
    subject.tuning.validate().map_err(CaptureRunError::Tuning)?;
    check_stream_range(request.stream, request.ticks)?;

    let mut session = PhysicsSession::builder()
        .fixed_hz(subject.fixed_hz)
        .configure(|app| {
            app.add_plugins(FlightForcesPlugin);
        })
        .build();
    let body = spawn_flight_body(
        session
            .world_mut()
            .expect("a session built by this function is active"),
        FlightModel::new(subject.tuning.clone()),
        &subject.spawn,
    )
    .map_err(CaptureRunError::Spawn)?;

    let mut controls = ControlBuffer::new();
    let mut probe = StateProbe::new();
    // Bounded by what an envelope can hold: a caller asking for more ticks than
    // that is refused by the probe one tick later, and an unbounded
    // pre-allocation would turn a large `--ticks` into an allocation failure
    // instead of a named refusal.
    let mut readings =
        Vec::with_capacity(request.ticks.min(MAX_ENVELOPE_ENTRIES as u64) as usize + 1);
    let start = StateReading::at_spawn(
        session
            .pose(body)
            .ok_or(CaptureRunError::BodyLost { tick: Tick(0) })?,
    );
    probe.start(&start)?;
    readings.push(start);

    for step in 1..=request.ticks {
        let tick = Tick(step);
        if let Some(frame) = request.stream.record(tick) {
            controls
                .apply_frame(frame)
                .map_err(CaptureRunError::Control)?;
        }
        // `begin_tick` hands this boundary every queued edge. This run hosts no
        // weapon, ordnance or menu system, so any edge it returns has no
        // consumer here and the first one is refused by name rather than
        // dropped: a replay that quietly skipped a press would promise the state
        // of a run whose press never happened.
        if let Some(action) = controls.begin_tick(tick).into_iter().next() {
            return Err(CaptureRunError::UnconsumedAction {
                tick,
                action: action.label().to_owned(),
            });
        }
        let command = flight_command(&controls)?;
        set_command(&mut session, body, tick, command)?;
        session.step(1).map_err(CaptureRunError::Session)?;
        let pose = session
            .pose(body)
            .ok_or(CaptureRunError::BodyLost { tick })?;
        // The pose is this tick's read-back, but `last_output` is only this
        // tick's *measurement* when the law stamped it with this tick: a
        // refused tick keeps the previous one, so the tick it was measured at
        // is read and handed to the reading rather than assumed.
        let measured = session
            .world()
            .and_then(|world| world.get::<FlightAircraft>(body))
            .ok_or(CaptureRunError::BodyLost { tick })?;
        let measured_at = measured.last_output_tick().map(Tick);
        let output = measured
            .last_output()
            .ok_or(CaptureRunError::UnmeasuredTick {
                tick,
                measured_at: None,
            })?;
        let reading = StateReading::after_tick(tick, measured_at, pose, output)?;
        probe.measure(&reading)?;
        readings.push(reading);
    }

    if probe.is_empty() {
        return Err(CaptureRunError::NoTicksMeasured);
    }

    let identity = RunIdentity::new(
        &subject.loaded,
        &subject.tuning,
        subject.fixed_hz,
        request.build.tree.clone(),
        &request.build.toolchain,
        request.build.platform.clone(),
    )
    .map_err(CaptureRunError::Record)?;

    let label = format!("{}{}", subject.subject, INITIAL_STATE_LABEL_SUFFIX);
    let record = ReplayRecord {
        schema: ReplayVersion::CURRENT,
        subject: subject.subject.clone(),
        fingerprint: identity.fingerprint(),
        initial_state: probe
            .initial_state(&label)
            .map_err(CaptureRunError::Record)?,
        stream: request.stream.clone(),
        // The record's seed root is a `u64` with no "absent" spelling, so a run
        // that declared no seed records `0`. The flight path consumes no random
        // stream at all, so no value of the root reaches the state a replay
        // compares; a stage that adds a consumer must use
        // [`ReplaySeeds::derive`] with its own domain constant so the streams it
        // consumed are named rather than re-derived.
        seeds: ReplaySeeds::new(subject.seed.unwrap_or(0), Vec::new())
            .map_err(CaptureRunError::Record)?,
        tick_rate: subject.fixed_hz,
        first_tick: Tick(0),
        last_tick: Tick(request.ticks),
        choices: subject.choices.clone(),
        promised: probe.envelope().clone(),
        overrides: subject.overrides.clone(),
        extra: Vec::new(),
    };
    record.validate().map_err(CaptureRunError::Record)?;
    Ok(RecordedRun { record, readings })
}

/// Refuses a stream whose frames fall outside the ticks the run drives.
///
/// A frame past the last tick, or at tick 0 before the first boundary, would
/// never be applied. Silently dropping it would produce a record whose promised
/// stream is longer than the run it came from, and a replay that quietly differs
/// from the recording.
fn check_stream_range(stream: &CommandStream, ticks: u64) -> Result<(), CaptureRunError> {
    let first = Tick(1);
    let last = Tick(ticks);
    for frame in stream.records() {
        let tick = frame.frame_tick();
        if tick < first || tick > last {
            return Err(CaptureRunError::StreamTickOutsideRun { tick, first, last });
        }
    }
    Ok(())
}

/// The tick's flight command, built from the input boundary's held axes.
///
/// The four continuous axes map straight onto the model's normalized command
/// ranges. Throttle is the one that does not line up: an axis sample is
/// `[-1, 1]` and the model's throttle is `[0, 1]`, so it is mapped by the
/// affine `(axis + 1) / 2`. That convention is recorded here because it is a
/// design decision, not a measurement — the original game's own throttle axis
/// mapping is unmeasured and is not claimed here. An axis nobody recorded stays
/// neutral, which is what a held axis means when no frame replaced it.
fn flight_command(controls: &ControlBuffer) -> Result<FlightInput, CaptureRunError> {
    let axis = |command: FlightCommand| f64::from(controls.axis(command).unwrap_or(0.0));
    let throttle = (axis(FlightCommand::Throttle) + 1.0) * 0.5;
    FlightInput::try_new(
        axis(FlightCommand::Pitch),
        axis(FlightCommand::Roll),
        axis(FlightCommand::Yaw),
        throttle,
        false,
    )
    .map_err(CaptureRunError::Input)
}

/// Writes the tick's command onto the aircraft.
fn set_command(
    session: &mut PhysicsSession,
    body: Entity,
    tick: Tick,
    command: FlightInput,
) -> Result<(), CaptureRunError> {
    let Some(world) = session.world_mut() else {
        return Err(CaptureRunError::Session(PhysicsSessionError::Inactive));
    };
    let Some(mut aircraft) = world.get_mut::<FlightAircraft>(body) else {
        return Err(CaptureRunError::BodyLost { tick });
    };
    aircraft
        .set_command(command)
        .map_err(CaptureRunError::Flight)
}
