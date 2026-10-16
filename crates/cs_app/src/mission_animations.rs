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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use bevy::ecs::world::World;
use cs_script::ir::{DirectiveOperation, Value};
use cs_script::runtime::{ExecutionKey, MissionState};
use cs_sim::objectives::runtime::TickInput;
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::net::SessionId;

use crate::animation::events::EventClass;
use crate::animation::mission::{
    AnimationNameIndex, AnimationTarget, NamedAnimationResolution, PlayRefusal, PlaybackGap,
    RecordPlayback, StartupAnimation,
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
    spelled_name: Option<String>,
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

    /// The animation name the event's payload spelled, when it stores one —
    /// the measured name field of the control-call opcodes
    /// ([`crate::animation::SPELLED_NAME_CLAIM`]).
    #[must_use]
    pub fn spelled_name(&self) -> Option<&str> {
        self.spelled_name.as_deref()
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

/// What one activation answered — [`MissionAnimationPlayer::start`] per row,
/// [`MissionAnimationPlayer::activate`] for one name-keyed row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationOutcome {
    /// The record is now running on this activation.
    Started,
    /// The identity was already running, so this activation did nothing: one
    /// activation per identity, never two at once.
    AlreadyRunning,
    /// The row was refused; the player's own ledger holds its refusals
    /// ([`MissionAnimationPlayer::refused_record`]).
    Refused,
}

/// One `WAKE_ANIM` directive the mission host consumed and what it started.
///
/// A `WAKE_ANIM` site names one animation; this is the record of the one
/// activation that name reached, at the tick the host consumed the directive
/// on. It is the directive side of the join — the record's own identity,
/// carrier and span are on the running record the player holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WakeActivation {
    /// The name as the directive spelled it.
    name: String,
    /// The committed tick the host consumed the directive on.
    at: Tick,
    /// What the record player answered.
    outcome: ActivationOutcome,
}

impl WakeActivation {
    /// The name as the directive spelled it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The committed tick the host consumed the directive on.
    #[must_use]
    pub const fn at(&self) -> Tick {
        self.at
    }

    /// Whether this activation started the record (as opposed to finding it
    /// already running).
    #[must_use]
    pub const fn started(&self) -> bool {
        matches!(self.outcome, ActivationOutcome::Started)
    }

    /// What the record player answered.
    #[must_use]
    pub const fn outcome(&self) -> ActivationOutcome {
        self.outcome
    }
}

/// Why a `WAKE_ANIM` directive started nothing, named.
///
/// Every refusal is by name — the name the site spelled, or an empty one when
/// the site's own arguments were not the measured shape — with the reader's own
/// or this project's own reason text and, for an ambiguous name, every record
/// it matched. A name that resolves but whose row the join refuses is also
/// here, with [`WAKE_ANIM_ROW_REFUSED_REASON`]; the record player's own refusal
/// ledger then says which agreement or block refused it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WakeRefusal {
    name: String,
    reason: &'static str,
    matches: Vec<(crate::animation::survey::CarrierKind, usize)>,
    at: Tick,
}

impl WakeRefusal {
    /// The name the site spelled, verbatim; empty when the site's own
    /// arguments were not the measured shape.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Why nothing was started, verbatim.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }

    /// Every record the name matched, when it matched more than one.
    #[must_use]
    pub fn matches(&self) -> &[(crate::animation::survey::CarrierKind, usize)] {
        &self.matches
    }

    /// The committed tick the directive was consumed on.
    #[must_use]
    pub const fn at(&self) -> Tick {
        self.at
    }
}

/// Why a `WAKE_ANIM` site's arguments started nothing: they are not the
/// one-text shape the measured sites spell.
pub const WAKE_ANIM_ARGUMENTS_REASON: &str = "the WAKE_ANIM site's arguments are not the one-animation-name shape the measured \
     installation spells (the operation's own measured parse is an animation name and an optional \
     node name), so no name was resolved and nothing was started";

/// Why a `WAKE_ANIM` name that resolved to a record started nothing: the join
/// refused the row.
pub const WAKE_ANIM_ROW_REFUSED_REASON: &str = "the WAKE_ANIM name resolved to exactly one record, but the join refused to play \
     it; the record player's own refusal ledger carries which name agreement or sequence block \
     refused it";

/// The mission host's `WAKE_ANIM` consumer.
///
/// It applies each `WakeAnimation` emission the session's directive log carries
/// **exactly once**, deduplicating by execution key the way the mission
/// countdown deduplicates the timer directives
/// ([`cs_sim::mission::MissionState`] keeps the log for the whole session, so
/// only the key says which entry is new), resolves the name the site spelled
/// through the scope's [`AnimationNameIndex`], and starts the record through
/// the record player at the tick being stepped.
///
/// Nothing is started twice and nothing is silently skipped: a name that binds
/// nothing, a name that binds several records and a site whose arguments are
/// not the measured shape are all kept as [`WakeRefusal`]s.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WakeAnimationLedger {
    applied: BTreeSet<ExecutionKey>,
    activations: Vec<WakeActivation>,
    refused: Vec<WakeRefusal>,
}

impl WakeAnimationLedger {
    /// Consumes every `WakeAnimation` emission of `state` this ledger has not
    /// already applied, and answers the activations this call produced in
    /// directive-key order.
    ///
    /// `names` is the mission scope's own name index — the join the name is
    /// resolved through — and `player` the record player the record is started
    /// in. Nothing is published here: the player's next `advance` publishes the
    /// statements the record has reached, and the host's stage-4 fold is the
    /// only thing that observes the record's state.
    pub fn consume(
        &mut self,
        state: &MissionState,
        names: &AnimationNameIndex,
        player: &mut MissionAnimationPlayer,
        tick: Tick,
    ) -> Vec<WakeActivation> {
        let mut activations = Vec::new();
        for emission in state.directives() {
            if emission.operation != DirectiveOperation::WakeAnimation {
                continue;
            }
            let key = emission.key.execution_key();
            if !self.applied.insert(key) {
                continue;
            }
            let Some(name) = wake_animation_name(&emission.args) else {
                self.refused.push(WakeRefusal {
                    name: String::new(),
                    reason: WAKE_ANIM_ARGUMENTS_REASON,
                    matches: Vec::new(),
                    at: tick,
                });
                continue;
            };
            match names.resolve(&name) {
                NamedAnimationResolution::Bound(row) => {
                    let outcome = player.activate(tick, &row);
                    if outcome == ActivationOutcome::Refused {
                        self.refused.push(WakeRefusal {
                            name: name.clone(),
                            reason: WAKE_ANIM_ROW_REFUSED_REASON,
                            matches: Vec::new(),
                            at: tick,
                        });
                    }
                    activations.push(WakeActivation {
                        name,
                        at: tick,
                        outcome,
                    });
                }
                NamedAnimationResolution::Unbound { reason, matches } => {
                    self.refused.push(WakeRefusal {
                        name,
                        reason,
                        matches,
                        at: tick,
                    });
                }
            }
        }
        self.activations.extend(activations.iter().cloned());
        activations
    }

    /// How many `WakeAnimation` emissions this ledger has applied.
    #[must_use]
    pub fn applied(&self) -> usize {
        self.applied.len()
    }

    /// Every activation, in the order the host consumed the directives.
    #[must_use]
    pub fn activations(&self) -> &[WakeActivation] {
        &self.activations
    }

    /// Every name a directive started nothing for, in the order it was
    /// consumed.
    #[must_use]
    pub fn refused(&self) -> &[WakeRefusal] {
        &self.refused
    }
}

/// The animation name one `WAKE_ANIM` site spelled, or `None` when the site's
/// arguments are not the measured one-text shape.
///
/// The measured sites spell exactly one text (`[text activate_dropoff_node]`);
/// no other shape is interpreted, because the operation's own parse stores an
/// animation name and an optional node name and **which** of those a site's
/// stored arguments is has not been measured for every shape. An empty name is
/// not a name either.
fn wake_animation_name(args: &[Value]) -> Option<String> {
    match args {
        [Value::Str(name)] if !name.is_empty() => Some(name.clone()),
        _ => None,
    }
}

/// The claim the `CALL_ANIMATION` activation rule is recorded under.
///
/// A `CALL_ANIMATION` statement's payload spells the animation it calls
/// (measured, [`crate::animation::SPELLED_NAME_CLAIM`]). **Designed, not
/// measured**: this consumer starts the spelled record the tick its statement
/// publishes, once per publication, the same way the landing path's own
/// designed chain claims its `animation_refs` start. What the 2000 engine did
/// with a call — whether the named record started then, queued, or replaced a
/// running sibling — is unmeasured and stays this claim's unknown.
pub const CALL_ANIMATION_ACTIVATION_CLAIM: &str = "m01-anim.call-animation-activation-designed";

/// The stored opcode of a `CALL_ANIMATION` event (`crate::animation::events`
/// statement table). Only calls activate: `STOP_ANIMATION` and
/// `INVALIDATE_ANIMATION` spell the same measured name field but what they do
/// to the named record is unmeasured, so this consumer publishes them and acts
/// on neither.
pub const CALL_ANIMATION_OPCODE: u8 = 24;

/// One `CALL_ANIMATION` statement's spelled name this host consumed and what
/// it started — the published-statement side of the join, so a reviewer can
/// tie every activation to the record, sequence and event that made the call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallActivation {
    /// The startup identity whose record published the call.
    caller: String,
    /// The sequence block the call sits in.
    sequence: usize,
    /// The call event's index inside that block.
    event_index: usize,
    /// The name the event's payload spelled, verbatim.
    name: String,
    /// The committed tick the statement published on.
    at: Tick,
    /// What the record player answered.
    outcome: ActivationOutcome,
}

impl CallActivation {
    /// The startup identity whose record published the call.
    #[must_use]
    pub fn caller(&self) -> &str {
        &self.caller
    }

    /// The sequence block the call sits in.
    #[must_use]
    pub const fn sequence(&self) -> usize {
        self.sequence
    }

    /// The call event's index inside that block.
    #[must_use]
    pub const fn event_index(&self) -> usize {
        self.event_index
    }

    /// The name the event's payload spelled, verbatim.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The committed tick the statement published on.
    #[must_use]
    pub const fn at(&self) -> Tick {
        self.at
    }

    /// Whether this activation started the record.
    #[must_use]
    pub const fn started(&self) -> bool {
        matches!(self.outcome, ActivationOutcome::Started)
    }

    /// What the record player answered.
    #[must_use]
    pub const fn outcome(&self) -> ActivationOutcome {
        self.outcome
    }
}

/// Why a `CALL_ANIMATION`'s spelled name started nothing, named.
///
/// The same by-name refusals the `WAKE_ANIM` consumer records: a name no
/// record binds, a name several records bind, and a bound row the join
/// refuses — each kept with the publishing record's identity and the
/// statement's own locator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallRefusal {
    /// The startup identity whose record published the call.
    caller: String,
    /// The sequence block the call sits in.
    sequence: usize,
    /// The call event's index inside that block.
    event_index: usize,
    /// The name the event's payload spelled, verbatim.
    name: String,
    /// Why nothing was started, verbatim.
    reason: &'static str,
    /// Every record the name matched, when it matched more than one.
    matches: Vec<(crate::animation::survey::CarrierKind, usize)>,
    /// The committed tick the statement published on.
    at: Tick,
}

impl CallRefusal {
    /// The startup identity whose record published the call.
    #[must_use]
    pub fn caller(&self) -> &str {
        &self.caller
    }

    /// The sequence block the call sits in.
    #[must_use]
    pub const fn sequence(&self) -> usize {
        self.sequence
    }

    /// The call event's index inside that block.
    #[must_use]
    pub const fn event_index(&self) -> usize {
        self.event_index
    }

    /// The name the event's payload spelled, verbatim.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Why nothing was started, verbatim.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }

    /// Every record the name matched, when it matched more than one.
    #[must_use]
    pub fn matches(&self) -> &[(crate::animation::survey::CarrierKind, usize)] {
        &self.matches
    }

    /// The committed tick the statement published on.
    #[must_use]
    pub const fn at(&self) -> Tick {
        self.at
    }
}

/// The mission host's `CALL_ANIMATION` consumer.
///
/// Each `CALL_ANIMATION` statement a tick's record report publishes names the
/// animation it calls — a measured payload field
/// ([`crate::animation::SPELLED_NAME_CLAIM`]) — and this ledger starts that
/// name through the record player at the tick the statement published on
/// ([`CALL_ANIMATION_ACTIVATION_CLAIM`], designed). A statement publishes
/// exactly once per record run, so no deduplication is needed beyond the
/// player's own one-activation-per-identity gate.
///
/// Only opcode `24` is consumed: `STOP_ANIMATION` and
/// `INVALIDATE_ANIMATION` spell the same measured name field but what they do
/// to the named record is unmeasured, so their names travel on the statement
/// and nothing acts on them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallAnimationLedger {
    activations: Vec<CallActivation>,
    refused: Vec<CallRefusal>,
}

impl CallAnimationLedger {
    /// Consumes every `CALL_ANIMATION` statement `report` published, resolving
    /// each spelled name through `names` and starting the bound row in
    /// `player` at `tick`. Answers the activations this call produced, in
    /// published order.
    pub fn consume(
        &mut self,
        report: &TickReport,
        names: &AnimationNameIndex,
        player: &mut MissionAnimationPlayer,
        tick: Tick,
    ) -> Vec<CallActivation> {
        let mut activations = Vec::new();
        for statement in report.statements() {
            if statement.opcode() != CALL_ANIMATION_OPCODE {
                continue;
            }
            let Some(name) = statement.spelled_name().map(str::to_owned) else {
                continue;
            };
            let caller = statement.identity().to_owned();
            let (sequence, event_index) = (statement.sequence(), statement.event_index());
            match names.resolve(&name) {
                NamedAnimationResolution::Bound(row) => {
                    let outcome = player.activate(tick, &row);
                    if outcome == ActivationOutcome::Refused {
                        self.refused.push(CallRefusal {
                            caller: caller.clone(),
                            sequence,
                            event_index,
                            name: name.clone(),
                            reason: WAKE_ANIM_ROW_REFUSED_REASON,
                            matches: Vec::new(),
                            at: tick,
                        });
                    }
                    activations.push(CallActivation {
                        caller,
                        sequence,
                        event_index,
                        name,
                        at: tick,
                        outcome,
                    });
                }
                NamedAnimationResolution::Unbound { reason, matches } => {
                    self.refused.push(CallRefusal {
                        caller,
                        sequence,
                        event_index,
                        name,
                        reason,
                        matches,
                        at: tick,
                    });
                }
            }
        }
        self.activations.extend(activations.iter().cloned());
        activations
    }

    /// Every activation, in the order the statements published.
    #[must_use]
    pub fn activations(&self) -> &[CallActivation] {
        &self.activations
    }

    /// Every spelled name a call started nothing for, in publish order.
    #[must_use]
    pub fn refused(&self) -> &[CallRefusal] {
        &self.refused
    }
}

/// The installation's own statement spelling a record's node-arming statements
/// carry (`OBJECT_ACTIVE_STATE`).
pub const OBJECT_ACTIVE_STATE_STATEMENT: &str = "OBJECT_ACTIVE_STATE";

/// The claim the designed node-arming surface is recorded under.
///
/// **Designed, not measured.** Whether the original evaluates an animation's
/// `ACTIVATION_PREREQUISITE` / `OBJECT_ACTIVE_LIST before a call is explicitly
/// unmeasured (`f20-anim.activation-prerequisite-unevaluated`,
/// [`crate::animation::programs`]), and the event payloads' node fields are not
/// decoded either, so which node one `OBJECT_ACTIVE_STATE` statement addresses
/// is unmeasured too. This surface therefore records what is measured — the
/// statement's own spelling and timing, the record that published it, and the
/// node names the record's own node-reference table states — and claims
/// nothing about the original's arming rule.
pub const OBJECT_ACTIVE_ARMING_CLAIM: &str = "m01-anim.object-active-state-arming-designed";

/// One active-state statement a running animation record published, with the
/// node names the record's own node-reference table states.
///
/// This is the designed arming surface: what a woken record said while it ran,
/// as the landing path's input. `nodes` is the **record's own** node table,
/// verbatim and in stored order (entry `0` is the empty name every measured
/// table starts with), because which of those names a given statement addressed
/// is not decoded — see [`OBJECT_ACTIVE_ARMING_CLAIM`].
#[derive(Clone, Debug, PartialEq)]
pub struct ArmedNode {
    animation: String,
    statement: &'static str,
    opcode: u8,
    tick: Tick,
    nodes: Vec<String>,
    span: SourceSpan,
}

impl ArmedNode {
    /// The record that published the statement.
    #[must_use]
    pub fn animation(&self) -> &str {
        &self.animation
    }

    /// The statement's own spelling ([`OBJECT_ACTIVE_STATE_STATEMENT`]).
    #[must_use]
    pub const fn statement(&self) -> &'static str {
        self.statement
    }

    /// The stored opcode.
    #[must_use]
    pub const fn opcode(&self) -> u8 {
        self.opcode
    }

    /// The committed tick the statement was published on.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The publishing record's own node-reference names, verbatim and in
    /// stored order.
    #[must_use]
    pub fn nodes(&self) -> &[String] {
        &self.nodes
    }

    /// The publishing record's own bytes.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// The claim under which this designed surface is recorded.
    #[must_use]
    pub const fn claim_id(&self) -> &'static str {
        OBJECT_ACTIVE_ARMING_CLAIM
    }
}

/// Reads the active-state statements one committed tick's report published, as
/// the designed arming input the landing path consumes.
///
/// A statement is armed exactly when a record published it in this report and
/// the name index still resolves the publishing record (the index is built from
/// the same join the player is), so this never fabricates a node table for a
/// record nobody bound. A record no report published is simply not in the
/// answer: nothing is armed before its own statement runs.
#[must_use]
pub fn armed_nodes_from_report(report: &TickReport, names: &AnimationNameIndex) -> Vec<ArmedNode> {
    let mut armed = Vec::new();
    for statement in report.statements() {
        if statement.statement() != OBJECT_ACTIVE_STATE_STATEMENT {
            continue;
        }
        let Some(facts) = names
            .row(statement.identity())
            .and_then(|row| row.bound_record().cloned())
        else {
            continue;
        };
        armed.push(ArmedNode {
            animation: statement.identity().to_owned(),
            statement: statement.statement(),
            opcode: statement.opcode(),
            tick: statement.tick(),
            nodes: facts.nodes().to_vec(),
            span: facts.span().clone(),
        });
    }
    armed
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
            match self.begin(at, row) {
                ActivationOutcome::Started => report.started.push(identity),
                ActivationOutcome::AlreadyRunning => report.already_running.push(identity),
                ActivationOutcome::Refused => report.refused.push(identity),
            }
        }
        report
    }

    /// Starts the record one joined row describes, exactly as [`Self::start`]
    /// does for one startup row.
    ///
    /// This is the activation path a name a `WAKE_ANIM` directive spells
    /// reaches: one activation per name — a repeated directive cannot start a
    /// second timeline over the first — a row the join refused kept in the
    /// same refusal ledger, and nothing published here. The first
    /// [`Self::advance`] after the activation publishes the statements the
    /// record has reached by its own tick, exactly as for a startup row, and
    /// the row's own `event()` (`wake_animation` for a name-keyed row) is what
    /// its record reports as its activation source.
    pub fn activate(&mut self, at: Tick, row: &StartupAnimation) -> ActivationOutcome {
        self.begin(at, row)
    }

    /// The one activation both entry points run: the identity gate, the join's
    /// verdicts and the running-set insert, in one place so a startup row and
    /// a name-keyed row can never be started by two different rules.
    fn begin(&mut self, at: Tick, row: &StartupAnimation) -> ActivationOutcome {
        let identity = row.identity().to_owned();
        if self.running.contains_key(&identity) {
            return ActivationOutcome::AlreadyRunning;
        }
        if !row.is_playable() {
            self.refuse(row, row.refusals().to_vec());
            return ActivationOutcome::Refused;
        }
        // `is_playable()` is `refusals().is_empty()`, and the join pushes
        // `Undeclared` for a row no member declares, so a playable row
        // always has exactly one declaring site and one bound record. The
        // arms exist so a future join rule cannot start a record whose
        // member nobody named or whose bytes nobody holds.
        let Some((archive, member)) = declaring_member(row) else {
            self.refuse(
                row,
                vec![PlayRefusal::Undeclared {
                    reason: crate::animation::UNDECLARED_REASON,
                }],
            );
            return ActivationOutcome::Refused;
        };
        let Some(playback) = row.playback().cloned() else {
            self.refuse(row, vec![PlaybackGap::Absent.refusal()]);
            return ActivationOutcome::Refused;
        };
        let Some(facts) = row.bound_record() else {
            self.refuse(row, vec![PlaybackGap::Absent.refusal()]);
            return ActivationOutcome::Refused;
        };
        let statements = playback.events().count();
        // The row's most recent verdict supersedes an earlier refusal of
        // the same identity: an identity is never refused and running at
        // once, so a host that re-offers a playable row clears the ledger
        // entry the join refused before.
        self.refused.remove(&identity);
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
        ActivationOutcome::Started
    }

    /// Keeps one refused row in the ledger under its own identity, with the
    /// refusals the join (or this activation) recorded for it.
    fn refuse(&mut self, row: &StartupAnimation, refusals: Vec<PlayRefusal>) {
        self.refused.insert(
            row.identity().to_owned(),
            RefusedRecord {
                identity: row.identity().to_owned(),
                event: row.event().to_owned(),
                span: row.bound_record().map(|facts| facts.span.clone()),
                refusals,
            },
        );
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
                            spelled_name: event.spelled_name.clone(),
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
                    spelled_name: reached.spelled_name,
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
    /// The animation name the event's payload spelled, when it stores one.
    spelled_name: Option<String>,
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
