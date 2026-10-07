//! The mission session's consumer of a mission scope's animation records, and
//! the composed step that carries the animation log into the mission session
//! (task #718, `M01-LC-ACTOR-ANIM-CONSUMERS`).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-D`, non-negotiable behaviors 1, 2 and 5. Shared contracts:
//! `docs/contracts/IDENTITY-CONTENT.md` (session generations) and
//! `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering"). Findings:
//! `docs/findings/2026-10-04-m01-lc-world-actors.md` (#632, the member → actor
//! binding), `docs/findings/2026-10-05-m01-lc-actor-anim-playback.md` (#678,
//! the join) and `docs/findings/2026-10-06-f20-event-grammar.md` (#690, the
//! grammar those records' events were read with).
//!
//! # The gap this closes
//!
//! Two measured halves had no consumer:
//!
//! * `bind_mission_animation` ([`crate::animation::mission`]) resolves one
//!   mission scope's startup animations to the `.zrd` member that declares
//!   them and the `mis_anim.zbd` / `cam_anim.zbd` record that stores them, and
//!   after #690 every one of M01's seven rows reports a decoded duration and a
//!   per-tick statement report — but **nothing started one, advanced one or
//!   finished one**, so no mission ever played a carrier's members;
//! * the F20-C [`AnimationLog`](crate::animation::AnimationLog) reached
//!   [`MissionMarkerConsumer`](crate::mission_markers::MissionMarkerConsumer)
//!   and [`ObjectiveSession`](crate::objectives::ObjectiveSession) only when a
//!   host composed those calls by hand, so the mission session had no single
//!   step that ran both halves of one committed tick.
//!
//! This module is both consumers, in the shape this crate already uses for the
//! mission layer: plain structs and functions the mission host calls per
//! committed tick, not a Bevy resource (the session they drive is host-owned).
//!
//! * [`MissionAnimationPlayer`] starts the rows of one startup event, advances
//!   them **once per committed tick**, and reports the statements each record
//!   starts — with the declaring archive and member, and with every name the
//!   row addresses (the declaring member's object selectors first, so the
//!   world record each selector selected travels with it), so the report *is*
//!   the measured member → actor binding in motion.
//! * [`step_mission_animations`] is the composed step: it advances the record
//!   player for a committed tick and then steps the mission session with the
//!   animation log's markers through
//!   [`step_mission_with_markers`](crate::mission_markers::step_mission_with_markers),
//!   so a host has one call for one committed tick and every refusal says
//!   which half already ran.
//!
//! # What "plays" means here, and what it does not
//!
//! A record's event stream stores **statements** — an opcode, its authored
//! target fields and its timing — and no keyframes (measured over all 56 994
//! retail blocks by #690). So a running record publishes the record's measured
//! activity per tick ([`RecordStatement`]) and never a transform: the claim
//! that stands in for one is
//! [`POSE_TRANSFORM_NOT_DECODED_CLAIM`](crate::animation::POSE_TRANSFORM_NOT_DECODED_CLAIM),
//! and no node pose or world position is produced from an event here. What a
//! mission can honestly do today is start the record, keep its timeline and
//! see which statements it has started — the spellings, classes and timings are
//! measured, the effect of executing one is not (the classes are this
//! project's grouping, not a claim about what the original engine did).
//!
//! A row nobody can play is **never started**: [`MissionAnimationPlayer::start`]
//! records its [`PlayRefusal`]s — with their claim ids and the record's own
//! source locator — and the identity never reaches the running set (F20
//! non-negotiable behavior 2). The world actors a mission's own archive places
//! are the same rule from the other side: their identity is measured
//! ([`crate::animation::WorldActorPlacement`]), their placement is refused
//! under [`PLACEMENT_FIELDS_CLAIM`](crate::animation::PLACEMENT_FIELDS_CLAIM),
//! and this module spawns nothing.
//!
//! # What is not claimed
//!
//! * **No original run.** `retail` is read access to the owner's installation;
//!   nothing here is `verified_original` and nothing says how the 2000 engine
//!   advanced these records.
//! * **The original's tick rate is unmeasured** (`f20-anim.tick-rate-unmeasured`),
//!   so the player takes the **caller's** [`ticks_per_second`](MissionAnimationPlayer::new)
//!   as its own timeline, exactly like `RecordPlayback::poses`: it maps the
//!   original's stored time unit onto the host's ticks and never claims the
//!   two are the same clock.
//! * **No cue label and no mission signal.** A record's statements are not
//!   converted into [`MarkerEffect`](cs_sim::animated_object::MarkerEffect)s:
//!   a cue label nobody authored would be an invented mission symbol, which
//!   [`crate::mission_markers`] refuses even for the designed vocabulary.

use std::collections::BTreeMap;
use std::fmt;

use bevy::ecs::world::World;
use cs_sim::objectives::runtime::TickInput;
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::net::SessionId;

use crate::animation::events::EventClass;
use crate::animation::mission::{
    AnimationTarget, PlayRefusal, PlaybackGap, RecordPlayback, StartupAnimation,
};
use crate::animation::survey::CarrierKind;
use crate::mission_markers::{
    MissionMarkerConsumer, MissionStep, MissionStepRefusal, step_mission_with_markers,
};
use crate::objectives::ObjectiveSession;

/// Why a player could not be built, started or advanced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerError {
    /// A tick rate of zero would map every stored time onto tick `0` and could
    /// never advance: refused here rather than producing a player that
    /// silently publishes nothing.
    ZeroTickRate,
    /// [`MissionAnimationPlayer::retry`] was asked to serve the generation it
    /// already serves — the same rule
    /// [`MissionMarkerConsumer::retry`](crate::mission_markers::MissionMarkerConsumer::retry)
    /// applies, because clearing the ledger of the live mission would restart
    /// every record it is already advancing.
    SameSession {
        /// The generation that is already served.
        served: SessionId,
    },
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroTickRate => write!(
                f,
                "a tick rate of zero cannot map the record's stored time onto a host tick"
            ),
            Self::SameSession { served } => write!(
                f,
                "{served} is the generation already served, so a retry would restart the \
                 records of a live mission"
            ),
        }
    }
}

impl std::error::Error for PlayerError {}

/// A record the mission session started and is advancing.
///
/// Everything except the timeline is read verbatim out of the original bytes
/// by the join: the declaring archive and member are the `.zrd` site that
/// declared the animation, the targets are the names that site and the record
/// address (with what each selected in the mission's world container), the
/// carrier and index are the record that stored it, and the span is those
/// bytes as a reviewer can read them.
#[derive(Clone, Debug, PartialEq)]
pub struct RunningRecord {
    /// The startup identity (`pzep_engines_start`).
    identity: String,
    /// The startup event that started it (`NEW_GAME_START`).
    event: String,
    /// The archive that declares the animation (`zbd/zrdr.zbd`).
    archive: String,
    /// The member that declares it (`pirate_zep_nacelles.zrd`).
    member: String,
    /// Every name this row addresses, in join order: the declaring member's
    /// object selectors first — the actors #632's table names — then the
    /// record's own object, root and node-table names, each keeping what it
    /// selected in the mission's world container. This is the actor half of
    /// the binding, carried with the member half.
    targets: Vec<AnimationTarget>,
    /// Which carrier stores the record.
    carrier: CarrierKind,
    /// The record's index inside that carrier.
    record_index: usize,
    /// The record's own bytes.
    span: SourceSpan,
    /// The tick the host started the record on.
    started_at: Tick,
    /// The record's measured duration, in the original's stored time unit.
    duration_time: f32,
    /// The stored time the last advance reached; `None` before the first
    /// advance of this record.
    reached_time: Option<f32>,
    /// The decoded blocks the timeline walks, in stored order.
    playback: RecordPlayback,
    /// Which of the record's statements have been published, in the order
    /// [`RecordPlayback::events`] walks them, so one statement can never be
    /// published twice within one activation.
    published: Vec<bool>,
    /// How many statements have been published so far.
    statements: usize,
}

impl RunningRecord {
    /// The startup identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// The startup event that started this record.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// The archive that declares the animation this record implements.
    #[must_use]
    pub fn archive(&self) -> &str {
        &self.archive
    }

    /// The member that declares it — the `zrdr.zbd` member that drives this
    /// actor's animation.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// Every name this record's row addresses, in join order — the declaring
    /// member's object selectors first, so a running session reads the member
    /// → actor binding (`#632`'s table) straight off the record: member,
    /// archive, and the world record each selector selected.
    #[must_use]
    pub fn targets(&self) -> &[AnimationTarget] {
        &self.targets
    }

    /// Which carrier stores the record.
    #[must_use]
    pub const fn carrier(&self) -> CarrierKind {
        self.carrier
    }

    /// The record's index inside that carrier.
    #[must_use]
    pub const fn record_index(&self) -> usize {
        self.record_index
    }

    /// The record's own bytes, as a reviewer can read them.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// The tick the host started the record on.
    #[must_use]
    pub const fn started_at(&self) -> Tick {
        self.started_at
    }

    /// The record's measured duration, in the original's stored time unit.
    #[must_use]
    pub const fn duration_time(&self) -> f32 {
        self.duration_time
    }

    /// The stored time the last advance reached, or `None` when this record
    /// has not been advanced yet.
    #[must_use]
    pub const fn reached_time(&self) -> Option<f32> {
        self.reached_time
    }

    /// How many statements this record has published so far.
    #[must_use]
    pub const fn statements(&self) -> usize {
        self.statements
    }
}

/// A row the player refused to start, kept with every refusal it carries.
#[derive(Clone, Debug, PartialEq)]
pub struct RefusedRecord {
    identity: String,
    event: String,
    span: Option<SourceSpan>,
    refusals: Vec<PlayRefusal>,
}

impl RefusedRecord {
    /// The startup identity nobody started.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// The startup event the row belonged to.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// The record's own bytes, when a record was bound at all.
    #[must_use]
    pub fn span(&self) -> Option<&SourceSpan> {
        self.span.as_ref()
    }

    /// Every refusal the join recorded, in stored order.
    #[must_use]
    pub fn refusals(&self) -> &[PlayRefusal] {
        &self.refusals
    }

    /// The claim ids the refusals carry, in stored order. A refusal about
    /// content that was read and does not match carries none, and that
    /// absence is part of the answer.
    #[must_use]
    pub fn claim_ids(&self) -> Vec<&str> {
        self.refusals
            .iter()
            .filter_map(|refusal| refusal.claim_id().map(|claim| claim.as_str()))
            .collect()
    }
}

/// A record that reached the end of its measured duration.
#[derive(Clone, Debug, PartialEq)]
pub struct FinishedRecord {
    identity: String,
    event: String,
    archive: String,
    member: String,
    targets: Vec<AnimationTarget>,
    carrier: CarrierKind,
    record_index: usize,
    span: SourceSpan,
    started_at: Tick,
    finished_at: Tick,
    duration_time: f32,
    statements: usize,
}

impl FinishedRecord {
    /// The startup identity that finished.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// The startup event that started it.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// The archive that declares the animation.
    #[must_use]
    pub fn archive(&self) -> &str {
        &self.archive
    }

    /// The member that declares it.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// Every name this record's row addressed, in join order — kept after the
    /// finish so the binding stays readable once the timeline has ended.
    #[must_use]
    pub fn targets(&self) -> &[AnimationTarget] {
        &self.targets
    }

    /// Which carrier stores the record.
    #[must_use]
    pub const fn carrier(&self) -> CarrierKind {
        self.carrier
    }

    /// The record's index inside that carrier.
    #[must_use]
    pub const fn record_index(&self) -> usize {
        self.record_index
    }

    /// The record's own bytes.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// The tick the record started on.
    #[must_use]
    pub const fn started_at(&self) -> Tick {
        self.started_at
    }

    /// The tick the record reached the end of its measured duration.
    #[must_use]
    pub const fn finished_at(&self) -> Tick {
        self.finished_at
    }

    /// The measured duration, in the original's stored time unit.
    #[must_use]
    pub const fn duration_time(&self) -> f32 {
        self.duration_time
    }

    /// How many statements the record published over its whole run.
    #[must_use]
    pub const fn statements(&self) -> usize {
        self.statements
    }
}

/// One statement a running record started at a committed tick.
///
/// This is the record's measured activity — the installation's own statement
/// spelling, its class and its timing — and **not** a transform: see the
/// module documentation. The session stamp is the generation the statement
/// belongs to, so a consumer can refuse a statement from a mission that has
/// already been torn down.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordStatement {
    session: SessionId,
    identity: String,
    archive: String,
    member: String,
    tick: Tick,
    sequence: usize,
    sequence_name: String,
    event_index: usize,
    opcode: u8,
    statement: &'static str,
    class: EventClass,
    start_time: f32,
    end_time: f32,
}

impl RecordStatement {
    /// The session generation this statement belongs to.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The startup identity whose record started the statement.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// The archive that declares the animation.
    #[must_use]
    pub fn archive(&self) -> &str {
        &self.archive
    }

    /// The member that declares it.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// The committed tick the statement was published on.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The index of the sequence block the statement belongs to.
    #[must_use]
    pub const fn sequence(&self) -> usize {
        self.sequence
    }

    /// The block's name, verbatim.
    #[must_use]
    pub fn sequence_name(&self) -> &str {
        &self.sequence_name
    }

    /// The event's index inside its own block, in stored order.
    #[must_use]
    pub const fn event_index(&self) -> usize {
        self.event_index
    }

    /// The stored opcode.
    #[must_use]
    pub const fn opcode(&self) -> u8 {
        self.opcode
    }

    /// The installation's own statement spelling.
    #[must_use]
    pub const fn statement(&self) -> &'static str {
        self.statement
    }

    /// This project's class of that spelling.
    #[must_use]
    pub const fn class(&self) -> EventClass {
        self.class
    }

    /// When the statement starts, in the original's stored time unit.
    #[must_use]
    pub const fn start_time(&self) -> f32 {
        self.start_time
    }

    /// When the statement ends, in the original's stored time unit.
    #[must_use]
    pub const fn end_time(&self) -> f32 {
        self.end_time
    }
}

/// What one [`MissionAnimationPlayer::start`] answered.
#[derive(Clone, Debug, PartialEq)]
pub struct StartupReport {
    event: String,
    at: Tick,
    started: Vec<String>,
    refused: Vec<String>,
    already_running: Vec<String>,
}

impl StartupReport {
    /// The startup event the host asked for.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// The tick the rows were started on.
    #[must_use]
    pub const fn at(&self) -> Tick {
        self.at
    }

    /// The identities that are now running, in stored order.
    #[must_use]
    pub fn started(&self) -> &[String] {
        &self.started
    }

    /// The identities refused this start, in stored order.
    #[must_use]
    pub fn refused(&self) -> &[String] {
        &self.refused
    }

    /// The identities that were already running, so this start did nothing for
    /// them: one activation per identity, never two at once.
    #[must_use]
    pub fn already_running(&self) -> &[String] {
        &self.already_running
    }
}

/// What one committed-tick advance published.
#[derive(Clone, Debug, PartialEq)]
pub struct TickReport {
    tick: Tick,
    statements: Vec<RecordStatement>,
    finished: Vec<FinishedRecord>,
}

impl TickReport {
    /// The committed tick the report belongs to.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Every statement the running records started at this tick: the records
    /// in identity order, each with the statements it reached in stored order.
    #[must_use]
    pub fn statements(&self) -> &[RecordStatement] {
        &self.statements
    }

    /// The records that reached the end of their measured duration at this
    /// tick, in the order they were removed.
    #[must_use]
    pub fn finished(&self) -> &[FinishedRecord] {
        &self.finished
    }
}

/// Why an advance published nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickAdvanceRefusal {
    /// The tick is not after the one the player already advanced through.
    ///
    /// This is the same rule the fixed-tick playback follows: one advance per
    /// committed tick change, nothing at all for a repeated one, so a host
    /// that retries a tick cannot publish a statement twice.
    NotAfter {
        /// The tick the host offered.
        tick: Tick,
        /// The tick already advanced through, when one has been.
        advanced_through: Option<Tick>,
    },
}

impl fmt::Display for TickAdvanceRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAfter {
                tick,
                advanced_through,
            } => match advanced_through {
                Some(through) => write!(
                    f,
                    "tick {} is not after {}, so the player already advanced through it",
                    tick.0, through.0
                ),
                None => write!(f, "tick {} is not after the player's first tick", tick.0),
            },
        }
    }
}

impl std::error::Error for TickAdvanceRefusal {}

/// What one [`MissionAnimationPlayer::retry`] released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerTeardown {
    /// The generation the new player serves.
    pub session: SessionId,
    /// How many records were still running.
    pub running: usize,
    /// How many rows were still refused.
    pub refused: usize,
    /// How many records had finished.
    pub finished: usize,
}

/// The mission session's consumer of one mission scope's animation records.
///
/// One player serves one session generation, owns that generation's record
/// timeline and keeps the two ledgers a mission needs to stay correct: the
/// rows it refused (never started) and the records it has finished. It is a
/// plain struct owned by the mission host beside the
/// [`MissionMarkerConsumer`](crate::mission_markers::MissionMarkerConsumer)
/// and [`ObjectiveSession`](crate::objectives::ObjectiveSession) it is stepped
/// with, not a Bevy resource.
///
/// The caller states `ticks_per_second`: the original's tick rate is
/// unmeasured (`f20-anim.tick-rate-unmeasured`), so the player maps the
/// record's stored time onto **the host's** timeline and never claims the two
/// clocks are the same.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionAnimationPlayer {
    served: SessionId,
    ticks_per_second: u32,
    running: BTreeMap<String, RunningRecord>,
    refused: BTreeMap<String, RefusedRecord>,
    finished: Vec<FinishedRecord>,
    advanced_through: Option<Tick>,
}

impl MissionAnimationPlayer {
    /// A player for the `served` generation on the caller's timeline.
    ///
    /// # Errors
    ///
    /// [`PlayerError::ZeroTickRate`] when `ticks_per_second` is zero.
    pub fn new(served: SessionId, ticks_per_second: u32) -> Result<Self, PlayerError> {
        if ticks_per_second == 0 {
            return Err(PlayerError::ZeroTickRate);
        }
        Ok(Self {
            served,
            ticks_per_second,
            running: BTreeMap::new(),
            refused: BTreeMap::new(),
            finished: Vec::new(),
            advanced_through: None,
        })
    }

    /// The session generation this player serves.
    #[must_use]
    pub const fn served(&self) -> SessionId {
        self.served
    }

    /// The caller's tick rate: how many host ticks one unit of the original's
    /// stored time is worth.
    #[must_use]
    pub const fn ticks_per_second(&self) -> u32 {
        self.ticks_per_second
    }

    /// The tick the player has advanced through, when it has advanced.
    #[must_use]
    pub const fn advanced_through(&self) -> Option<Tick> {
        self.advanced_through
    }

    /// The records currently running, in identity order.
    pub fn running(&self) -> impl Iterator<Item = &RunningRecord> {
        self.running.values()
    }

    /// The rows this player refused to start and has not since started, in
    /// identity order.
    ///
    /// The ledger holds the join's **current** verdict for an identity: a row
    /// refused on one `start` and played by a later one clears the entry (it
    /// is no longer refused), so an identity is never refused and running at
    /// once. A `start` that finds the identity already running leaves both
    /// ledgers as they are — the live activation is the verdict — and records
    /// that in [`StartupReport::already_running`].
    pub fn refused(&self) -> impl Iterator<Item = &RefusedRecord> {
        self.refused.values()
    }

    /// The records that finished, in the order they finished.
    pub fn finished(&self) -> impl Iterator<Item = &FinishedRecord> {
        self.finished.iter()
    }

    /// How many records are currently running.
    #[must_use]
    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    /// How many rows this player kept as refusals.
    #[must_use]
    pub fn refused_count(&self) -> usize {
        self.refused.len()
    }

    /// How many records have finished.
    #[must_use]
    pub fn finished_count(&self) -> usize {
        self.finished.len()
    }

    /// The running record with this identity.
    #[must_use]
    pub fn running_record(&self, identity: &str) -> Option<&RunningRecord> {
        self.running.get(identity)
    }

    /// The refusal this identity was kept under.
    #[must_use]
    pub fn refused_record(&self, identity: &str) -> Option<&RefusedRecord> {
        self.refused.get(identity)
    }

    /// Starts every row of one startup event that the join says is playable,
    /// and keeps every row it cannot start as a refusal.
    ///
    /// Rows whose own event is not `event` belong to another startup event and
    /// are not started. A row that is already running is reported in
    /// [`StartupReport::already_running`] and left alone: one activation per
    /// identity, so a repeated `NEW_GAME_START` cannot start a second timeline
    /// over the first (F20 non-negotiable behavior 5). A row whose join
    /// refused never enters the running set, whatever its bytes said, and a
    /// record that already **finished** may be started again — that is a new
    /// activation with a fresh timeline, and the finished one stays in
    /// [`Self::finished`] as its own record.
    ///
    /// Nothing is published here; the first [`Self::advance`] publishes the
    /// statements the record has reached by its own tick. Start a record at
    /// the tick the player is about to advance: a record started at a tick the
    /// player has already advanced through publishes everything it crossed in
    /// one batch at the next advance.
    #[must_use]
    pub fn start(&mut self, event: &str, at: Tick, rows: &[StartupAnimation]) -> StartupReport {
        let mut report = StartupReport {
            event: event.to_owned(),
            at,
            started: Vec::new(),
            refused: Vec::new(),
            already_running: Vec::new(),
        };
        for row in rows.iter().filter(|row| row.event() == event) {
            let identity = row.identity().to_owned();
            if self.running.contains_key(&identity) {
                report.already_running.push(identity);
                continue;
            }
            if !row.is_playable() {
                report.refused.push(identity.clone());
                self.refused.insert(
                    identity.clone(),
                    RefusedRecord {
                        identity,
                        event: row.event().to_owned(),
                        span: row.bound_record().map(|facts| facts.span.clone()),
                        refusals: row.refusals().to_vec(),
                    },
                );
                continue;
            }
            // `is_playable()` is `refusals().is_empty()`, and the join pushes
            // `Undeclared` for a row no member declares, so a playable row
            // always has exactly one declaring site and one bound record. The
            // arms exist so a future join rule cannot start a record whose
            // member nobody named or whose bytes nobody holds.
            let Some((archive, member)) = declaring_member(row) else {
                report.refused.push(identity.clone());
                self.refused.insert(
                    identity.clone(),
                    RefusedRecord {
                        identity,
                        event: row.event().to_owned(),
                        span: row.bound_record().map(|facts| facts.span.clone()),
                        refusals: vec![PlayRefusal::Undeclared {
                            reason: crate::animation::UNDECLARED_REASON,
                        }],
                    },
                );
                continue;
            };
            let Some(playback) = row.playback().cloned() else {
                report.refused.push(identity.clone());
                self.refused.insert(
                    identity.clone(),
                    RefusedRecord {
                        identity,
                        event: row.event().to_owned(),
                        span: row.bound_record().map(|facts| facts.span.clone()),
                        refusals: vec![PlaybackGap::Absent.refusal()],
                    },
                );
                continue;
            };
            let facts = match row.bound_record() {
                Some(facts) => facts,
                None => {
                    report.refused.push(identity.clone());
                    self.refused.insert(
                        identity.clone(),
                        RefusedRecord {
                            identity,
                            event: row.event().to_owned(),
                            span: None,
                            refusals: vec![PlaybackGap::Absent.refusal()],
                        },
                    );
                    continue;
                }
            };
            let statements = playback.events().count();
            // The row's most recent verdict supersedes an earlier refusal of
            // the same identity: an identity is never refused and running at
            // once, so a host that re-offers a playable row clears the ledger
            // entry the join refused before.
            self.refused.remove(&identity);
            report.started.push(identity.clone());
            self.running.insert(
                identity.clone(),
                RunningRecord {
                    identity,
                    event: row.event().to_owned(),
                    archive,
                    member,
                    targets: row.targets().to_vec(),
                    carrier: facts.carrier,
                    record_index: facts.index,
                    span: facts.span.clone(),
                    started_at: at,
                    duration_time: playback.duration_time(),
                    reached_time: None,
                    published: vec![false; statements],
                    playback,
                    statements: 0,
                },
            );
        }
        report
    }

    /// Advances every running record once for one committed tick.
    ///
    /// A statement whose measured `START_TIME` the record's timeline has now
    /// reached is published the first time it is reached and never again, in
    /// stored order, and a record that has reached the end of its measured
    /// duration moves to [`Self::finished`] in the same call, **after** its
    /// last statements are published. A record whose start tick is after this
    /// tick is left untouched: its timeline has not begun.
    ///
    /// # Errors
    ///
    /// [`TickAdvanceRefusal::NotAfter`] when `tick` is not after the tick the
    /// player already advanced through; nothing is published for such a tick.
    pub fn advance(&mut self, tick: Tick) -> Result<TickReport, TickAdvanceRefusal> {
        if self.advanced_through.is_some_and(|through| tick <= through) {
            return Err(TickAdvanceRefusal::NotAfter {
                tick,
                advanced_through: self.advanced_through,
            });
        }
        self.advanced_through = Some(tick);
        let rate = self.ticks_per_second;
        let served = self.served;
        let mut statements = Vec::new();
        let mut finished = Vec::new();
        let identities: Vec<String> = self.running.keys().cloned().collect();
        for identity in identities {
            let Some(record) = self.running.get_mut(&identity) else {
                continue;
            };
            if tick < record.started_at {
                continue;
            }
            let elapsed = tick.0.saturating_sub(record.started_at.0);
            let time = elapsed as f32 / rate as f32;
            // Pass one only reads: which statements this tick's time reaches
            // and had not been published yet. The flags are written in the
            // second pass, so no borrow of the record's blocks ever overlaps a
            // mutation of it.
            let mut reached: Vec<ReachedStatement> = Vec::new();
            let mut index = 0_usize;
            for (sequence, block) in record.playback.sequences().iter().enumerate() {
                for (event_index, event) in block.events.iter().enumerate() {
                    let published = record.published.get(index).is_some_and(|already| *already);
                    if !published && event.start_time() <= time {
                        reached.push(ReachedStatement {
                            index,
                            sequence,
                            sequence_name: block.name.clone(),
                            event_index,
                            opcode: event.opcode,
                            statement: event.statement,
                            class: event.class,
                            start_time: event.start_time(),
                            end_time: event.end_time(),
                        });
                    }
                    index += 1;
                }
            }
            let (identity_text, archive, member) = (
                record.identity.clone(),
                record.archive.clone(),
                record.member.clone(),
            );
            for reached in reached {
                if let Some(flag) = record.published.get_mut(reached.index) {
                    *flag = true;
                }
                statements.push(RecordStatement {
                    session: served,
                    identity: identity_text.clone(),
                    archive: archive.clone(),
                    member: member.clone(),
                    tick,
                    sequence: reached.sequence,
                    sequence_name: reached.sequence_name,
                    event_index: reached.event_index,
                    opcode: reached.opcode,
                    statement: reached.statement,
                    class: reached.class,
                    start_time: reached.start_time,
                    end_time: reached.end_time,
                });
                record.statements += 1;
            }
            record.reached_time = Some(time);
            if time >= record.duration_time
                && let Some(record) = self.running.remove(&identity)
            {
                finished.push(FinishedRecord {
                    identity: record.identity,
                    event: record.event,
                    archive: record.archive,
                    member: record.member,
                    targets: record.targets,
                    carrier: record.carrier,
                    record_index: record.record_index,
                    span: record.span,
                    started_at: record.started_at,
                    finished_at: tick,
                    duration_time: record.duration_time,
                    statements: record.statements,
                });
            }
        }
        // The player's own ledger keeps every record this call finished, in
        // the same order the report names them.
        self.finished.extend(finished.iter().cloned());
        Ok(TickReport {
            tick,
            statements,
            finished,
        })
    }

    /// Starts a new session generation, reporting what the old one still held.
    ///
    /// # Errors
    ///
    /// [`PlayerError::SameSession`] for the generation already served; the
    /// player is left untouched, so a refused retry cannot restart the records
    /// of a live mission.
    pub fn retry(&mut self, served: SessionId) -> Result<PlayerTeardown, PlayerError> {
        if served == self.served {
            return Err(PlayerError::SameSession { served });
        }
        let teardown = PlayerTeardown {
            session: served,
            running: self.running.len(),
            refused: self.refused.len(),
            finished: self.finished.len(),
        };
        self.running.clear();
        self.refused.clear();
        self.finished.clear();
        self.advanced_through = None;
        self.served = served;
        Ok(teardown)
    }
}

/// One statement one record reached in [`MissionAnimationPlayer::advance`],
/// read in the pass that only reads the record's blocks and written in the
/// pass that only writes its ledger.
#[derive(Clone, Debug)]
struct ReachedStatement {
    /// The statement's flat index over the record's own events.
    index: usize,
    /// The index of the sequence block it belongs to.
    sequence: usize,
    /// The block's name, verbatim.
    sequence_name: String,
    /// The event's index inside that block, in stored order.
    event_index: usize,
    /// The stored opcode.
    opcode: u8,
    /// The installation's own statement spelling.
    statement: &'static str,
    /// This project's class of that spelling.
    class: EventClass,
    /// When the statement starts, in the original's stored time unit.
    start_time: f32,
    /// When the statement ends, in the original's stored time unit.
    end_time: f32,
}

/// The declaring archive and member of a row the join resolved.
fn declaring_member(row: &StartupAnimation) -> Option<(String, String)> {
    let site = row.declaration().resolution().single()?;
    Some((site.archive().to_owned(), site.member().to_owned()))
}

/// One committed tick of the mission session: the animation log's markers
/// raised into the mission runtime, and the mission's own animation records
/// advanced for the same tick.
#[derive(Debug)]
pub struct MissionAnimationStep {
    /// What the mission half answered.
    pub mission: MissionStep,
    /// What the record half published for this tick.
    pub records: TickReport,
}

/// Why a composed step did not complete, and what the other half already did.
#[derive(Debug)]
pub enum MissionAnimationStepRefusal {
    /// The record player refused the tick, so the mission half was never
    /// called: nothing at all was applied and the host may retry the whole
    /// step.
    Records(TickAdvanceRefusal),
    /// The mission session refused the tick **after** the record player had
    /// already advanced it. The box keeps the `Err` of
    /// [`step_mission_animations`] small; what it carries says the same: the
    /// record report for the tick the mission refused, and the marker delivery
    /// the host retries the **mission** half with — with those signals, the way
    /// [`step_mission_with_markers`](crate::mission_markers::step_mission_with_markers)
    /// documents — while the host must not offer this tick to the player again.
    Mission(Box<MissionTickRefusal>),
}

/// The record half of a composed tick that the mission half refused.
#[derive(Debug)]
pub struct MissionTickRefusal {
    /// What the record half published for the tick the mission refused.
    pub records: TickReport,
    /// What the mission half refused.
    pub refusal: MissionStepRefusal,
}

impl fmt::Display for MissionAnimationStepRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Records(refusal) => write!(f, "the animation record player refused: {refusal}"),
            Self::Mission(half) => write!(
                f,
                "the mission session refused after the records advanced: {}",
                half.refusal
            ),
        }
    }
}

impl std::error::Error for MissionAnimationStepRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Records(refusal) => Some(refusal),
            Self::Mission(half) => Some(&half.refusal),
        }
    }
}

/// The composed mission step: advance the mission's own animation records,
/// then raise the animation log's markers into the mission session.
///
/// This is the one call a mission host makes per committed tick:
///
/// 1. the record half runs first, advancing [`MissionAnimationPlayer`] for
///    this tick and publishing the statements the running records reached;
/// 2. the mission half then runs through [`step_mission_with_markers`], so
///    the animation log's markers reach [`ObjectiveSession`] for the same
///    tick.
///
/// The order matters for retries. The objective session refuses a tick it did
/// not accept and asks the host to retry it with the marker delivery the
/// refusal carries; because the record half ran first, that retry is a plain
/// [`step_mission_with_markers`] call and **must not** be offered to the
/// player again — [`MissionAnimationStepRefusal::Mission`] carries the record
/// report for exactly this case, so the tick's statements stay with the host
/// that already received them.
///
/// # Errors
///
/// [`MissionAnimationStepRefusal::Records`] when the player refuses the tick
/// (nothing applied at all),
/// [`MissionAnimationStepRefusal::Mission`] when the objective session
/// refuses it (the record report carried).
pub fn step_mission_animations(
    world: &mut World,
    player: &mut MissionAnimationPlayer,
    markers: &mut MissionMarkerConsumer,
    objectives: &mut ObjectiveSession,
    facts: &TickInput<'_>,
) -> Result<MissionAnimationStep, MissionAnimationStepRefusal> {
    let records = player
        .advance(facts.tick)
        .map_err(MissionAnimationStepRefusal::Records)?;
    match step_mission_with_markers(world, markers, objectives, facts) {
        Ok(mission) => Ok(MissionAnimationStep { mission, records }),
        Err(refusal) => Err(MissionAnimationStepRefusal::Mission(Box::new(
            MissionTickRefusal { records, refusal },
        ))),
    }
}
