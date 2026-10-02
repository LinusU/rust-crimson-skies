//! Target queries, allegiance, selection actions and threat contracts
//! (F30-A, F30-B).
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stages `### F30-A` and `### F30-B`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Stage **F30-A** defines the typed contract — the inputs a session feeds
//! the targeting authority, the queries it answers and the records it
//! returns — plus the minimal synthetic fixture the acceptance tests
//! drive.
//!
//! Stage **F30-B** adds the production path that runs it:
//!
//! * [`SelectionAction`] and [`SelectionBinding`] bind
//!   `cs_types::input::FlightCommand` edges — `TargetNext`/`TargetPrev` and
//!   any other declared target edge — onto the F30-A
//!   [`SelectionRequest`] vocabulary, and [`TargetStore::act`] resolves one
//!   against the phase's [`SelectionFrame`], so the tick and the crosshair
//!   ray come from the session rather than from a caller guessing them.
//! * [`AttackEvent::from_hit`] and [`TargetStore::record_hits`] are the
//!   threat state's real feed: the damage system's own [`HitEvent`]s become
//!   ledger entries, and a hit that credits nobody mints nothing
//!   (non-negotiable 4).
//! * [`Reticle`] and [`TargetPhase`] are what one phase boundary produces:
//!   a single record that clears an ineligible selection and carries the
//!   selected actor's class, live allegiance, hostility gate and threat
//!   state, so the reticle the HUD draws and the hostility the combat AI
//!   reads cannot straddle a boundary (AC02's production half).
//!
//! Stage **F30-C** adds what those consumers need to be told, and only that:
//! [`TargetPhase::cleared`] carries the [`ClearedSelection`] a consumer
//! otherwise could only infer by comparing two records — which actor went away
//! and [`SelectionClearReason`] why — and [`TargetStore::present`] separates
//! "still in the world" from [`TargetStore::eligible`], so a consumer can warn
//! about an attacker that lost sensor contact without warning about a wreck.
//! The original-data verification of ordering, reveal and assistance rules is
//! F30-D.
//!
//! Stage **F30-D** verified the original-side vocabulary and the rules AC04
//! names, and changed no production behavior: the original's shipped string
//! image *names* eleven target commands and a twelve-label padlock assist
//! family, and names no reveal or lead/aim-assistance concept at all, while the
//! original's target **order**, reveal rules and assistance *behavior* stayed
//! unmeasured and are recorded as fidelity limitations in
//! `docs/findings/2026-10-02-f30-d-target-order-reveal-and-assistance.md`.
//!
//! # Pieces
//!
//! * [`TargetStore`] is the per-session targeting authority, mirroring
//!   [`crate::damage::DamageResolver`]'s session confinement: one store per
//!   session generation, every actor identity session-qualified, a foreign
//!   generation refused by name
//!   (`docs/contracts/STATE-TRANSACTIONS.md`: "Results, previous targets
//!   and delayed callbacks are always generation-qualified").
//! * [`AllegianceTable`] holds the declared faction relations as directed
//!   pairs. An undeclared pair reports `None`, never a guessed hostility —
//!   a target whose faction relation is unknown is neither an enemy nor a
//!   friend (F30 non-negotiable 1, AGENTS "unknown means unknown").
//! * [`TargetRecord`] is the per-actor snapshot: stable [`ActorId`],
//!   current faction [`ContentId`], [`TargetClass`], objective flag,
//!   reveal state, script-phase eligibility and canonical
//!   [`WorldPosition`] (f64 world coordinates — origin-safe, so target
//!   identity survives a world rebase; F30 non-negotiable 5).
//! * [`SelectionRequest`]/[`TargetSelection`] are the typed selection
//!   vocabulary and per-observer state; [`TargetStore::apply`] evaluates
//!   one request deterministically, and [`TargetStore::prune`] clears a
//!   selection whose target stopped being eligible before any consumer
//!   renders it (AC03's contract half).
//! * [`AttackEvent`]/[`ThreatCue`] are the threat ledger: cues come only
//!   from recorded authoritative attack events stamped with the damage
//!   system's [`HitEventId`] — never from "every enemy in radius"
//!   (F30 non-negotiable 4).
//! * [`TargetInfo`] is the read-only snapshot a query hands the HUD and
//!   spyglass. It carries what those consumers display — allegiance,
//!   class, distance — and confers no combat authority: it is a copy of
//!   store state, not a handle into it.
//! * [`SelectionAction`]/[`SelectionBinding`] are the F30-B action
//!   vocabulary and the command-edge table that reaches it, and
//!   [`TargetStore::phase`] is the one call a session's consumers make per
//!   phase boundary.
//! * [`ClearedSelection`]/[`SelectionClearReason`] are the F30-C vocabulary
//!   for why a held selection went away, derived by
//!   [`TargetStore::phase`] in the same read as the reticle, and
//!   [`TargetStore::present`] is the "still in the world" question a
//!   consumer asks of a threat cue's attacker — distinct from
//!   [`TargetStore::eligible`], which also requires contact and phase
//!   eligibility.
//!
//! # Determinism
//!
//! Ordering is a total order over `(distance², ActorId)`: distance ties —
//! the AC01 "equal-distance targets" case — break on the stable
//! session-qualified actor id, so the cycle sequence is a function of the
//! roster, never of ECS iteration or insertion order (non-negotiable 2).
//! The phase record is derived in that same order, so a reticle, a
//! hostility gate and a threat cue all read the same snapshot.
//!
//! # Designed vocabulary, not original data
//!
//! Which selection actions the original 2000 PC game exposes, how its
//! cycle orders targets, its crosshair cone, reveal rules and assistance
//! behavior are unmeasured (F30 "Research boundary"; F30-D's retail stage).
//! Every vocabulary value, filter and fixture number here is newly
//! authored engine design, recorded in
//! `docs/findings/2026-09-30-f30-a-target-queries-and-allegiance-contracts.md`
//! and, for the F30-B action table and threat feed, in
//! `docs/findings/2026-10-02-f30-b-selection-actions-and-threat-state.md`,
//! and, for the F30-C consumer contract, in
//! `docs/findings/2026-10-02-f30-c-hud-spyglass-and-weapon-guidance.md`.
//! The declared, provenance-carrying half is
//! `cs_content::target_rules`; the lowering boundary and ECS bindings are
//! `cs_app::targeting`.
//!
//! ## What F30-D measured, and what stayed unknown
//!
//! F30-D measured the original's shipped **action vocabulary** — the original
//! names eleven target commands (a clear, an under-reticule pick and a
//! next/previous/nearest triple for each of three named classes) and a
//! twelve-label *padlock* assist family, and names no reveal or visibility
//! concept and no lead/aim-assistance option at all. The finding is
//! `docs/findings/2026-10-02-f30-d-target-order-reveal-and-assistance.md`.
//!
//! What a name is **not** is a behavior, and the three things this module
//! designs stayed unmeasured: the order the original's cycle walks, the
//! original's reveal rules, and what the padlock modes do. Nothing here may be
//! presented as measured original behavior: [`CrosshairQuery`]'s cone and
//! occlusion report, [`TargetPolicy::crosshair_cone`],
//! [`TargetPolicy::threat_window_ticks`] and every fixture number are project
//! design, and the consumer gate
//! (`cs_app::targeting::AssistanceOffer::presentable`) is what keeps a designed
//! value from being drawn as an original one.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`): no Bevy, no renderer, no file access.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::input::FlightCommand;
use cs_types::net::SessionId;
use cs_types::space::{Meters, Radians, UnitVec3, WorldPosition};

use crate::damage::{ActorId, HitEvent, HitEventId, LifecycleKind};

/// What kind of actor a target is.
///
/// The targeting classification axis mirrors
/// `cs_content::damage::GraphSubjectKind`'s aircraft / world object /
/// capital ship split — the same subject kinds the damage schema declares —
/// because "non-aircraft" selection and class-specific cues need class
/// membership, not damage rules. Whether the original game's cycle
/// distinguishes exactly these classes is unmeasured (F30-D); this is
/// designed engine vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TargetClass {
    /// A player or AI aircraft.
    Aircraft,
    /// A world object (vehicle, structure, destructible prop).
    WorldObject,
    /// A capital ship or zeppelin-class actor.
    CapitalShip,
}

impl TargetClass {
    /// Every class, in a stable order.
    pub const ALL: &'static [TargetClass] = &[Self::Aircraft, Self::WorldObject, Self::CapitalShip];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Aircraft => "aircraft",
            Self::WorldObject => "world_object",
            Self::CapitalShip => "capital_ship",
        }
    }
}

impl fmt::Display for TargetClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How an observing faction relates to a target's faction.
///
/// `Hostile` drives the enemy reticle and AI hostility, `Friendly` the
/// ally path, `Neutral` neither. An *undeclared* relation is none of
/// these: [`AllegianceTable::get`] returns `Option`, so a missing
/// declaration is an explicit unknown — it never silently counts as
/// hostile or friendly (F30 non-negotiable 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Allegiance {
    /// The factions are enemies.
    Hostile,
    /// The factions are neither enemies nor allies.
    Neutral,
    /// The factions are allies.
    Friendly,
}

impl Allegiance {
    /// Every allegiance, in a stable order.
    pub const ALL: &'static [Allegiance] = &[Self::Hostile, Self::Neutral, Self::Friendly];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Hostile => "hostile",
            Self::Neutral => "neutral",
            Self::Friendly => "friendly",
        }
    }
}

impl fmt::Display for Allegiance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The session's declared faction relations: directed `(from, to)` pairs.
///
/// Relations are directed records — declaring that raiders are hostile to
/// the player faction does not assert the reverse; a symmetric pairing is
/// two records. A faction is always [`Allegiance::Friendly`] to itself;
/// that single invariant is part of the contract, not data.
///
/// [`AllegianceTable::declare`] is also the scripted-faction-change
/// transaction: mission systems upsert a pair mid-session when the script
/// flips a relation, and every query after the change reads the new value
/// in the same phase boundary (AC02's contract half).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllegianceTable {
    declared: BTreeMap<(ContentId, ContentId), Allegiance>,
}

impl AllegianceTable {
    /// An empty table: every cross-faction relation is undeclared.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares (or redeclares) the relation of `from` toward `to`.
    ///
    /// A self-pair is a no-op — self-allegiance is the contract's own
    /// [`Allegiance::Friendly`] invariant and cannot be redeclared
    /// hostile by data.
    pub fn declare(&mut self, from: ContentId, to: ContentId, allegiance: Allegiance) {
        if from == to {
            return;
        }
        self.declared.insert((from, to), allegiance);
    }

    /// The declared relation of `from` toward `to`; `None` when the pair
    /// was never declared. `from == to` is always `Friendly`.
    #[must_use]
    pub fn get(&self, from: &ContentId, to: &ContentId) -> Option<Allegiance> {
        if from == to {
            return Some(Allegiance::Friendly);
        }
        self.declared.get(&(from.clone(), to.clone())).copied()
    }

    /// The number of declared directed pairs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.declared.len()
    }

    /// Whether no pair is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.declared.is_empty()
    }
}

/// The session's declared targeting policy, lowered from
/// `cs_content::target_rules::TargetRuleSet` by `cs_app::targeting`.
///
/// The values are declared rules — each arrives `Resolved` on the content
/// side and an unresolved rule refuses to lower rather than guessing a
/// window or a cone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetPolicy {
    /// How long an authoritative attack keeps its attacker a live threat:
    /// the number of ticks after `at` for which [`TargetStore::threats`]
    /// still reports the attacker.
    pub threat_window_ticks: u64,
    /// The declared default half-angle, in radians, an under-crosshair
    /// selection accepts when its producer does not supply a cone. The
    /// [`CrosshairQuery`] itself always carries the cone it ran with; this
    /// is the fallback the request layer substitutes.
    pub crosshair_cone: Radians,
}

/// One registered targetable actor: the snapshot the queries read.
///
/// The record is the typed input spawn and script producers hand the
/// store; every field is public because there is no hidden state — the
/// update paths are the explicit typed transactions on [`TargetStore`]
/// (`set_pose`, `set_faction`, `set_revealed`, `set_phase_eligible`,
/// `record_lifecycle`), never a field write behind its back.
///
/// `position` is a canonical f64 [`WorldPosition`]: targeting reads world
/// identity directly, so a rebase of the local render/physics frame does
/// not move a target or reorder a cycle (F30 non-negotiable 5).
#[derive(Clone, Debug, PartialEq)]
pub struct TargetRecord {
    /// The actor's session-qualified identity.
    pub actor: ActorId,
    /// The faction catalog id (`ContentKind::Faction`) the actor belongs
    /// to; changed only through [`TargetStore::set_faction`].
    pub faction: ContentId,
    /// What kind of actor this is.
    pub class: TargetClass,
    /// Whether mission rules flag the actor as an objective target.
    pub objective: bool,
    /// Whether the actor is revealed to sensors/HUD. A hidden actor is
    /// registered but not eligible for selection until revealed
    /// (non-negotiable 1: reveal state is part of validity).
    pub revealed: bool,
    /// Whether the current script phase makes the actor eligible. A
    /// phase-gated actor is present in the world but excluded from
    /// selection until its phase opens (non-negotiable 1).
    pub phase_eligible: bool,
    /// The actor's canonical world position.
    pub position: WorldPosition,
}

/// Which lifecycle transitions end targetability.
///
/// Destruction, despawn and mission removal remove the actor from the
/// eligible set. [`LifecycleKind::PilotBailout`] and
/// [`LifecycleKind::OwnershipCaptured`] deliberately do not: a bailed-out
/// airframe is still a physical object in the world, and a capture changes
/// *who owns* the actor — which [`TargetStore::set_faction`] records —
/// not whether it exists. Which of these the original game treats as
/// targetable is unmeasured; this split is the designed contract.
const fn ends_targeting(kind: LifecycleKind) -> bool {
    matches!(
        kind,
        LifecycleKind::Destroyed | LifecycleKind::Despawned | LifecycleKind::MissionRemoved
    )
}

/// Which targets a selection request considers.
///
/// The filter is explicit data carried by the request — an allegiance
/// filter matches only a *declared* relation, so an undeclared pair never
/// slips into a hostile or friendly cycle (non-negotiable 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetFilter {
    /// Every eligible actor other than the observer.
    Any,
    /// Only actors whose declared allegiance to the observer's faction is
    /// the given one. `Allegiance::Neutral` matches only a declared
    /// neutral pair — never an undeclared one.
    Allegiance(Allegiance),
    /// Only actors of the given class.
    Class(TargetClass),
    /// Only actors not of the given class — the "non-aircraft" axis.
    NotClass(TargetClass),
    /// Only actors mission rules flagged as objective targets.
    Objective,
}

impl TargetFilter {
    /// The stable label used in reports and action descriptions.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Any => "any".to_owned(),
            Self::Allegiance(allegiance) => format!("{allegiance}"),
            Self::Class(class) => class.label().to_owned(),
            Self::NotClass(class) => format!("not_{}", class.label()),
            Self::Objective => "objective".to_owned(),
        }
    }
}

/// The direction a cycle request walks the ordered target list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CycleDirection {
    /// Toward the next farther target in the ordering, wrapping.
    Next,
    /// Toward the previous nearer target in the ordering, wrapping.
    Previous,
}

impl CycleDirection {
    /// Every direction, in a stable order.
    pub const ALL: &'static [CycleDirection] = &[Self::Next, Self::Previous];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::Previous => "previous",
        }
    }
}

impl fmt::Display for CycleDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// An authoritative attack, for the threat ledger.
///
/// `evidence` is the producing system's authoritative event id — the
/// damage system's [`HitEventId`]
/// (`EventId(session, tick, producer, sequence)`) — so a threat cue always
/// traces to the attack that caused it and a stale-generation event can
/// never mint a cue (non-negotiable 4; STATE-TRANSACTIONS).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackEvent {
    /// The actor that attacked.
    pub attacker: ActorId,
    /// The actor that was attacked.
    pub victim: ActorId,
    /// The simulation tick the attack happened at.
    pub at: Tick,
    /// The authoritative event this attack is evidenced by.
    pub evidence: HitEventId,
}

impl AttackEvent {
    /// The attack a landed hit evidences, or `None` when the hit credits
    /// nobody.
    ///
    /// A hit with no attributable source (`HitEvent::attacker` is `None` —
    /// the world, a hazard) damages without crediting an actor, so it mints
    /// no threat cue; neither does a self-hit, because an actor is not a
    /// threat against itself. Everything else becomes a ledger entry whose
    /// evidence is the hit's own id and whose `at` is that id's tick — the
    /// damage system's clock, not a caller's.
    #[must_use]
    pub fn from_hit(hit: &HitEvent) -> Option<Self> {
        let attacker = hit.attacker?;
        (attacker != hit.target).then_some(Self {
            attacker,
            victim: hit.target,
            at: hit.id.tick,
            evidence: hit.id,
        })
    }
}

/// What one batch of authoritative hits did to the threat ledger.
///
/// The counters are the batch's whole accounting: a session can tell a
/// quiet tick from a tick whose hits all missed the roster, and neither
/// looks like a minting of cues.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThreatFeed {
    /// Hits that minted a new ledger entry.
    pub recorded: usize,
    /// Hits already in the ledger under the same evidence id and
    /// attacker — a redelivered batch, counted once.
    pub repeated: usize,
    /// Hits that credited nobody: no attributable source, or a self-hit.
    pub unattributed: usize,
    /// Hits naming an actor this store does not track, which is a normal
    /// outcome — damage records exist for parts the target roster never
    /// listed.
    pub untracked: usize,
}

/// One live threat against an actor: an attacker that produced an
/// authoritative attack inside the declared window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreatCue {
    /// The actor that attacked.
    pub attacker: ActorId,
    /// The most recent tick it attacked at.
    pub last_attack: Tick,
}

/// A validated under-crosshair query: a ray in canonical world space, the
/// acceptance half-angle and the occlusion evidence.
///
/// `occluded` is the set of actors an authoritative occlusion producer
/// (the physics/camera side — F30-C/D wires the real sweep) reports as
/// blocked from `origin`; it arrives as data because targeting is not the
/// occlusion authority.
#[derive(Clone, Debug, PartialEq)]
pub struct CrosshairQuery {
    /// The ray origin, in canonical world space.
    pub origin: WorldPosition,
    /// The ray direction.
    pub direction: UnitVec3,
    /// The acceptance half-angle in radians, within `[0, π]`.
    pub cone: Radians,
    /// Actors reported occluded from `origin` by the occlusion producer.
    pub occluded: BTreeSet<ActorId>,
}

/// Why a [`CrosshairQuery`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CrosshairError {
    /// The cone was NaN or infinite.
    NonFiniteCone,
    /// The cone fell outside `[0, π]`; `radians` is the rejected value.
    ConeOutOfRange {
        /// The rejected angle.
        radians: f64,
    },
}

impl fmt::Display for CrosshairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteCone => write!(f, "the crosshair cone must be finite"),
            Self::ConeOutOfRange { radians } => {
                write!(f, "the crosshair cone {radians} rad is outside [0, π]")
            }
        }
    }
}

impl std::error::Error for CrosshairError {}

impl CrosshairQuery {
    /// Builds a query, refusing a non-finite or out-of-range cone.
    ///
    /// # Errors
    ///
    /// [`CrosshairError::NonFiniteCone`] or
    /// [`CrosshairError::ConeOutOfRange`].
    pub fn try_new(
        origin: WorldPosition,
        direction: UnitVec3,
        cone: Radians,
        occluded: BTreeSet<ActorId>,
    ) -> Result<Self, CrosshairError> {
        if !cone.0.is_finite() {
            return Err(CrosshairError::NonFiniteCone);
        }
        if !(0.0..=std::f64::consts::PI).contains(&cone.0) {
            return Err(CrosshairError::ConeOutOfRange { radians: cone.0 });
        }
        Ok(Self {
            origin,
            direction,
            cone,
            occluded,
        })
    }
}

/// One selection request against the store — the typed vocabulary the
/// command edges map onto (F30-B wires `FlightCommand::TargetNext` and the
/// other declared bindings).
///
/// The variant set is designed engine vocabulary covering the actions the
/// spec's deliverable names — enemy/objective, ally and non-aircraft via
/// [`TargetFilter`], nearest-attacker, under-crosshair and clear. Which of
/// these the original game exposes, and under which exact semantics, is
/// unverified until F30-D measures it.
#[derive(Clone, Debug, PartialEq)]
pub enum SelectionRequest {
    /// Walk the ordered eligible list in `direction`, wrapping. With no
    /// current selection, `Next` picks the first (nearest) entry and
    /// `Previous` the last.
    Cycle {
        /// Which way to walk.
        direction: CycleDirection,
        /// Which actors the cycle considers.
        filter: TargetFilter,
    },
    /// Select the nearest eligible actor matching `filter`.
    Nearest {
        /// Which actors are considered.
        filter: TargetFilter,
    },
    /// Select the nearest eligible actor with a live threat cue against
    /// the observer at tick `now` — actual attack evidence, not proximity.
    NearestAttacker {
        /// The tick the query runs at.
        now: Tick,
    },
    /// Select the eligible, unoccluded actor nearest the crosshair ray
    /// within its cone (AC04's contract half).
    UnderCrosshair(CrosshairQuery),
    /// Drop the current selection.
    Clear,
}

/// What a weapon-guidance consumer is told about the selected target.
///
/// Guidance is *derived from* the reticle and confers no combat authority: it
/// names the target the session selected and the bearing a consumer would draw
/// an aid toward, and it holds no aim correction, no damage and no way to
/// influence a shot. The weapon path (F27-B) owns ballistics; this record only
/// says which target the session is pointing at and where it is.
///
/// The record is produced by [`TargetStore::guidance`] from the *same* phase
/// read that produced the reticle, so guidance and reticle cannot disagree
/// about allegiance or about whether the target is still eligible.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponGuidance {
    /// The tick the guidance was derived at.
    pub at: Tick,
    /// The selected actor the aid would be drawn for.
    pub target: ActorId,
    /// The target's canonical world position.
    pub position: WorldPosition,
    /// The unit vector from the observer to the target: the bearing a lead
    /// indicator or assistance cue points along. It is a *direction to the
    /// target*, not a lead solution — computing one needs the target's
    /// velocity and the projectile's ballistics, which targeting does not own
    /// and which are unmeasured (F30-D).
    pub bearing: UnitVec3,
    /// The target's distance from the observer.
    pub distance: Meters,
    /// The target's live declared allegiance to the observer's faction.
    pub allegiance: Option<Allegiance>,
    /// Whether the target is a declared hostile. Guidance eligibility is gated
    /// on this: a friendly, neutral or *undeclared* pair is never offered an
    /// aid, so the weapon path cannot be nudged toward an ally by a default.
    pub hostile: bool,
    /// Whether this actor produced an authoritative attack against the
    /// observer inside the declared threat window.
    pub threatening: bool,
}

/// One observer's selection state.
///
/// The state carries no eligibility of its own: [`TargetStore::prune`]
/// re-derives validity from the roster, so a selection that stops being
/// eligible is cleared before a consumer can render it stale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TargetSelection {
    current: Option<ActorId>,
}

impl TargetSelection {
    /// An empty selection.
    #[must_use]
    pub const fn new() -> Self {
        Self { current: None }
    }

    /// The currently selected actor, if any.
    #[must_use]
    pub const fn current(&self) -> Option<ActorId> {
        self.current
    }

    /// Drops the selection.
    pub fn clear(&mut self) {
        self.current = None;
    }

    fn set(&mut self, actor: ActorId) {
        self.current = Some(actor);
    }
}

/// One bound target action: what a target command edge does when it fires.
///
/// The difference from [`SelectionRequest`] is who supplies the phase's
/// context. A request carries the tick or the crosshair ray its caller
/// already has; an action names *what* to select and lets
/// [`TargetStore::act`] take `now` and the ray from the
/// [`SelectionFrame`] the session is running. That is what lets a command
/// edge be a plain piece of declared data instead of code that closes over
/// a tick.
///
/// The variant set is the deliverable's action list: the two cycle
/// directions over any filter (the enemy/objective/ally/non-aircraft
/// cycles), nearest of a filter, nearest attacker, under-crosshair and
/// clear. Which of them the original game binds, and to which keys, is
/// unverified until F30-D measures it.
#[derive(Clone, Debug, PartialEq)]
pub enum SelectionAction {
    /// Walk the eligible list matching `filter` in `direction`, wrapping.
    Cycle {
        /// Which way to walk.
        direction: CycleDirection,
        /// Which actors the cycle considers.
        filter: TargetFilter,
    },
    /// Select the nearest eligible actor matching `filter`.
    Nearest {
        /// Which actors are considered.
        filter: TargetFilter,
    },
    /// Select the nearest eligible actor with a live threat cue against
    /// the observer at the frame's tick.
    NearestAttacker,
    /// Select the eligible, unoccluded actor under the frame's crosshair
    /// ray. Refused with [`TargetError::MissingCrosshair`] when the frame
    /// carries no ray — an absent ray is not "nothing under the
    /// crosshair".
    UnderCrosshair,
    /// Drop the current selection.
    Clear,
}

impl SelectionAction {
    /// The stable label used in reports and persisted bindings.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Cycle { direction, filter } => {
                format!("{}_{}", direction.label(), filter.label())
            }
            Self::Nearest { filter } => format!("nearest_{}", filter.label()),
            Self::NearestAttacker => "nearest_attacker".to_owned(),
            Self::UnderCrosshair => "under_crosshair".to_owned(),
            Self::Clear => "clear".to_owned(),
        }
    }
}

impl fmt::Display for SelectionAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// The per-phase context an action resolves against: the tick the phase
/// runs at and the crosshair ray the camera producer reported, if any.
///
/// The crosshair ray is `None` when the producer had none to report (no
/// targetable geometry in front of the camera, a frame where the ray was
/// refused). [`TargetStore::act`] then refuses an
/// [`SelectionAction::UnderCrosshair`] rather than answering "no target"
/// for a ray it never saw.
#[derive(Clone, Copy, Debug)]
pub struct SelectionFrame<'a> {
    /// The tick the phase runs at.
    pub now: Tick,
    /// The crosshair ray the producer reported this phase.
    pub crosshair: Option<&'a CrosshairQuery>,
}

impl<'a> SelectionFrame<'a> {
    /// A frame at `now` with no crosshair ray.
    #[must_use]
    pub const fn at(now: Tick) -> Self {
        Self {
            now,
            crosshair: None,
        }
    }

    /// The same frame with the producer's crosshair ray attached.
    #[must_use]
    pub const fn with_crosshair(self, query: &'a CrosshairQuery) -> Self {
        Self {
            now: self.now,
            crosshair: Some(query),
        }
    }
}

/// The session's target command edges: which
/// [`cs_types::input::FlightCommand`] runs which [`SelectionAction`].
///
/// The table is the F30-B binding of the engine's command vocabulary onto
/// the selection vocabulary, and the record an importer's declared action
/// table lowers into. It is a [`BTreeMap`], so iterating it is a total
/// order over commands and two runs with the same edges act in the same
/// order however the caller collected them (non-negotiable 2).
///
/// A command with no entry is not a targeting command: it is left to
/// control, weapons and ordnance, and firing it changes no selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionBinding {
    actions: BTreeMap<FlightCommand, SelectionAction>,
}

impl SelectionBinding {
    /// An empty table: no command runs a selection action.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `command` to `action`, returning the action it replaced.
    pub fn bind(
        &mut self,
        command: FlightCommand,
        action: SelectionAction,
    ) -> Option<SelectionAction> {
        self.actions.insert(command, action)
    }

    /// The action `command` runs, if it is a targeting command.
    #[must_use]
    pub fn action(&self, command: FlightCommand) -> Option<&SelectionAction> {
        self.actions.get(&command)
    }

    /// Every bound command with its action, in stable command order.
    pub fn bindings(&self) -> impl Iterator<Item = (FlightCommand, &SelectionAction)> {
        self.actions
            .iter()
            .map(|(command, action)| (*command, action))
    }

    /// How many commands are bound.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Whether no command is bound.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// The designed default: the two declared target edges walk a cycle
    /// over `filter`, away from and toward the observer.
    ///
    /// This is newly authored engine design with no original key evidence
    /// (F30-D's retail stage) — the fixture, not a claim about the
    /// original game.
    #[must_use]
    pub fn cycling(filter: TargetFilter) -> Self {
        let mut binding = Self::new();
        binding.bind(
            FlightCommand::TargetNext,
            SelectionAction::Cycle {
                direction: CycleDirection::Next,
                filter,
            },
        );
        binding.bind(
            FlightCommand::TargetPrev,
            SelectionAction::Cycle {
                direction: CycleDirection::Previous,
                filter,
            },
        );
        binding
    }
}

/// A read-only snapshot of one registered actor for HUD, spyglass and
/// assistance consumers.
///
/// The snapshot is a copy — it confers no combat authority and cannot
/// mutate the roster. `allegiance` is evaluated against the observer's
/// faction at query time, so a faction change mid-session is reflected on
/// the next read (non-negotiable 1): a target that became friendly is
/// never still reported as an enemy.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetInfo {
    /// The actor's session-qualified identity.
    pub actor: ActorId,
    /// What kind of actor it is.
    pub class: TargetClass,
    /// Its current faction.
    pub faction: ContentId,
    /// The observer's declared allegiance toward it — `None` when the
    /// relation was never declared (an explicit unknown, not a guess).
    pub allegiance: Option<Allegiance>,
    /// Whether mission rules flag it as an objective target.
    pub objective: bool,
    /// Whether it is currently eligible for selection.
    pub eligible: bool,
    /// Its canonical world position.
    pub position: WorldPosition,
    /// Its distance from the observer.
    pub distance: Meters,
}

/// What one phase boundary reports about the observer's selection: what a
/// reticle draws, and what the combat AI's hostility gate reads.
///
/// The two live in one record on purpose. A faction change is applied to
/// the store, and the *next* phase derives both from the same read, so the
/// reticle and the AI can never disagree about whether the selected actor
/// is an enemy — the failure AC02 names (non-negotiable 1). Nothing here is
/// stored: a reticle cannot be held across a phase and rendered stale.
#[derive(Clone, Debug, PartialEq)]
pub struct Reticle {
    /// The selected actor.
    pub target: ActorId,
    /// What kind of actor it is.
    pub class: TargetClass,
    /// Its current faction.
    pub faction: ContentId,
    /// The declared relation to the observer's faction, re-derived in this
    /// phase. `None` is an *undeclared* pair: neither enemy nor friend, and
    /// never a hostility.
    pub allegiance: Option<Allegiance>,
    /// Whether the combat AI may engage: a **declared** hostile relation.
    ///
    /// Hostility is a gate, not a weight
    /// ([`crate::ai::combat`](crate::ai)), so a neutral, a friendly and an
    /// undeclared pair are all non-hostile here, and the AI's own gate makes
    /// the same call from the same [`Allegiance`].
    pub hostile: bool,
    /// Whether this actor produced an authoritative attack against the
    /// observer inside the declared threat window — the threat cue the
    /// HUD's warning reads (non-negotiable 4).
    pub threatening: bool,
    /// Whether mission rules flag the actor as an objective target.
    pub objective: bool,
    /// Whether the actor is revealed to sensors/HUD.
    pub revealed: bool,
    /// Its canonical world position — the point a reticle projects and a
    /// candidate view is built from, in the same f64 world space the
    /// ordering used, so a rebase cannot move it.
    pub position: WorldPosition,
    /// Its distance from the observer.
    pub distance: Meters,
}

/// Why a held selection stopped being eligible (F30-C).
///
/// The reason is a statement about the roster, not about a display: every
/// variant is a fact the store already holds, read in the order the actor can
/// lose eligibility. Which of these the original game shows a reason for is
/// unmeasured; the vocabulary is designed engine semantics, and a consumer is
/// free to display none of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectionClearReason {
    /// A lifecycle transition ended the actor's targetability: the
    /// [`LifecycleKind`] that was recorded.
    Ended(LifecycleKind),
    /// The actor is no longer revealed to sensors/HUD.
    NotRevealed,
    /// The current script phase no longer makes the actor eligible.
    PhaseIneligible,
    /// The actor is no longer registered: its entity left the world.
    LeftWorld,
}

impl SelectionClearReason {
    /// The stable label used in reports.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ended(kind) => kind.label(),
            Self::NotRevealed => "not_revealed",
            Self::PhaseIneligible => "phase_ineligible",
            Self::LeftWorld => "left_world",
        }
    }
}

impl fmt::Display for SelectionClearReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A selection this phase boundary dropped, and the reason.
///
/// Carried on [`TargetPhase`] so the reticle, the spyglass and the guidance
/// consumer learn *that* a target went away and *why* in the same single read
/// of the roster, before any of them renders (AC03).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClearedSelection {
    /// The actor the observer had selected before this boundary.
    pub actor: ActorId,
    /// Why it stopped being eligible.
    pub reason: SelectionClearReason,
}

/// One phase boundary's targeting state: the selection after pruning, why it
/// changed, the reticle record for it and the observer's live threat cues.
///
/// [`TargetStore::phase`] is the single call a session's consumers make per
/// boundary, and every field is derived in it. A consumer therefore never
/// reads the selection and the allegiance in two calls that a faction change
/// could come between, and never has to compare two records to learn that its
/// target is gone.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetPhase {
    /// The tick the phase ran at.
    pub at: Tick,
    /// The observer's selection after pruning: `None` when there was none,
    /// or when the held target stopped being eligible (AC03's contract
    /// half, applied before any consumer reads the record).
    pub selection: Option<ActorId>,
    /// The selection this boundary dropped, when it dropped one: `None` when
    /// there was no selection, when the selection is unchanged, or when the
    /// boundary moved to a different eligible target. A re-selection is not a
    /// clear — the consumer's previous target simply changed.
    pub cleared: Option<ClearedSelection>,
    /// The reticle record for [`selection`](Self::selection); `None` when
    /// there is no selection.
    pub reticle: Option<Reticle>,
    /// The live threat cues against the observer, most recent first.
    ///
    /// A cue is recorded evidence of an attack, so it survives its attacker
    /// leaving the world; a consumer that *displays* cues decides with
    /// [`TargetStore::present`] whether the attacker can still be one.
    pub threats: Vec<ThreatCue>,
}

/// Why a [`TargetStore`] operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetError {
    /// An input carried a session generation other than the store's —
    /// state never leaks across generations.
    ForeignSession {
        /// The store's session.
        expected: u64,
        /// The session the input carried.
        found: u64,
    },
    /// The actor is already registered.
    DuplicateActor {
        /// The actor that exists already.
        actor: ActorId,
    },
    /// The named actor is not registered with this store.
    UnknownActor {
        /// The actor that was named.
        actor: ActorId,
    },
    /// An [`SelectionAction::UnderCrosshair`] was run in a
    /// [`SelectionFrame`] that carries no crosshair ray. Refused rather
    /// than answered: a frame without a ray is missing evidence, not
    /// evidence that nothing is under the crosshair.
    MissingCrosshair,
    /// A guidance query needs a selected target and the phase carries none.
    /// Refused rather than answered with a default aid: "no target" and "an
    /// aid that silently corrects nothing" are different statements, and the
    /// consumer decides which it can render.
    NoSelection,
    /// The selected actor is not a **declared** hostile — it is friendly,
    /// neutral, or its faction relation was never declared. The query is
    /// refused rather than answered, so no consumer can offer an aid toward
    /// an ally on the strength of a default (F30 non-negotiable 1 and 3).
    NotHostile {
        /// The selected actor the aid would have been drawn for.
        actor: ActorId,
        /// Its declared relation to the observer's faction, `None` when the
        /// pair was never declared.
        allegiance: Option<Allegiance>,
    },
    /// A guidance query asked for a target that is not registered.
    UnknownSelection {
        /// The actor the caller named.
        actor: ActorId,
    },
    /// The selected actor sits exactly on the observer, so the guidance
    /// bearing has no direction to report. Refused rather than defaulted: a
    /// consumer would otherwise draw an aid toward an arbitrary axis.
    DegenerateBearing {
        /// The selected actor whose bearing is undefined.
        actor: ActorId,
    },
}

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => write!(
                f,
                "input belongs to session {found}, but this store owns session {expected}"
            ),
            Self::DuplicateActor { actor } => write!(f, "{actor} is already registered"),
            Self::UnknownActor { actor } => write!(f, "{actor} is not registered"),
            Self::MissingCrosshair => write!(
                f,
                "the under-crosshair action needs a crosshair ray, and this phase carries none"
            ),
            Self::NoSelection => write!(
                f,
                "weapon guidance needs a selected target, and this phase carries none"
            ),
            Self::NotHostile { actor, allegiance } => match allegiance {
                Some(allegiance) => write!(
                    f,
                    "weapon guidance refuses {actor}: it is a declared {allegiance}, not a declared hostile"
                ),
                None => write!(
                    f,
                    "weapon guidance refuses {actor}: its faction relation to the observer was never declared"
                ),
            },
            Self::UnknownSelection { actor } => {
                write!(f, "weapon guidance names {actor}, which is not registered")
            }
            Self::DegenerateBearing { actor } => write!(
                f,
                "weapon guidance for {actor} has no bearing: the target is on the observer"
            ),
        }
    }
}

impl std::error::Error for TargetError {}

/// The per-session targeting authority. See the module docs.
///
/// The store owns the roster, the allegiance table and the threat ledger
/// for one session generation. A restart or aircraft swap is a *new*
/// store: registrations, relation overrides and recorded attacks never
/// carry into the next generation (STATE-TRANSACTIONS session
/// generations).
#[derive(Clone, Debug)]
pub struct TargetStore {
    session: u64,
    policy: TargetPolicy,
    allegiance: AllegianceTable,
    records: BTreeMap<ActorId, TargetEntry>,
    /// The threat ledger, keyed by victim.
    attacks: BTreeMap<ActorId, Vec<AttackEvent>>,
}

/// A roster entry: the declared record plus the lifecycle transition that
/// ended its targetability, if one did.
#[derive(Clone, Debug)]
struct TargetEntry {
    record: TargetRecord,
    gone: Option<LifecycleKind>,
}

impl TargetStore {
    /// A store for session `session` under `policy`, seeded with the
    /// declared `allegiance` table.
    #[must_use]
    pub fn new(session: u64, policy: TargetPolicy, allegiance: AllegianceTable) -> Self {
        Self {
            session,
            policy,
            allegiance,
            records: BTreeMap::new(),
            attacks: BTreeMap::new(),
        }
    }

    /// The session generation this store owns.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The policy the store runs under.
    #[must_use]
    pub const fn policy(&self) -> TargetPolicy {
        self.policy
    }

    /// Registers an actor's target record.
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] when the actor belongs to another
    /// session generation, [`TargetError::DuplicateActor`] when it is
    /// already registered.
    pub fn register(&mut self, record: TargetRecord) -> Result<(), TargetError> {
        if record.actor.session.get() != self.session {
            return Err(TargetError::ForeignSession {
                expected: self.session,
                found: record.actor.session.get(),
            });
        }
        if self.records.contains_key(&record.actor) {
            return Err(TargetError::DuplicateActor {
                actor: record.actor,
            });
        }
        self.records
            .insert(record.actor, TargetEntry { record, gone: None });
        Ok(())
    }

    /// Whether the actor is registered.
    #[must_use]
    pub fn is_registered(&self, actor: &ActorId) -> bool {
        self.records.contains_key(actor)
    }

    /// One registered actor's record; `None` when unregistered.
    #[must_use]
    pub fn record(&self, actor: &ActorId) -> Option<&TargetRecord> {
        self.records.get(actor).map(|entry| &entry.record)
    }

    /// The lifecycle transition that ended the actor's targetability, if
    /// one was recorded.
    #[must_use]
    pub fn gone(&self, actor: &ActorId) -> Option<LifecycleKind> {
        self.records.get(actor).and_then(|entry| entry.gone)
    }

    /// Updates the actor's canonical world position.
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] or [`TargetError::UnknownActor`].
    pub fn set_pose(&mut self, actor: ActorId, position: WorldPosition) -> Result<(), TargetError> {
        self.entry_mut(actor)?.record.position = position;
        Ok(())
    }

    /// Records an ownership/faction change: the actor's allegiance to
    /// every observer is re-derived from the new faction on the next
    /// query, in the same phase boundary (non-negotiable 1, AC02's
    /// contract half).
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] or [`TargetError::UnknownActor`].
    pub fn set_faction(&mut self, actor: ActorId, faction: ContentId) -> Result<(), TargetError> {
        self.entry_mut(actor)?.record.faction = faction;
        Ok(())
    }

    /// Updates the actor's reveal state. A hidden actor is not eligible
    /// for selection until revealed.
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] or [`TargetError::UnknownActor`].
    pub fn set_revealed(&mut self, actor: ActorId, revealed: bool) -> Result<(), TargetError> {
        self.entry_mut(actor)?.record.revealed = revealed;
        Ok(())
    }

    /// Updates the actor's script-phase eligibility.
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] or [`TargetError::UnknownActor`].
    pub fn set_phase_eligible(
        &mut self,
        actor: ActorId,
        phase_eligible: bool,
    ) -> Result<(), TargetError> {
        self.entry_mut(actor)?.record.phase_eligible = phase_eligible;
        Ok(())
    }

    /// Updates the actor's classification.
    ///
    /// Classification is a property of *what the actor is*, not of a phase:
    /// a scripted action that reclassifies an airframe as a capital ship, or
    /// a loadout change that turns a gun mount into ordnance, changes what
    /// the class and non-aircraft actions select. It is a typed transaction
    /// like every other record field, so a reclassification reaches the next
    /// query through the same path as a faction change.
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] or [`TargetError::UnknownActor`].
    pub fn set_class(&mut self, actor: ActorId, class: TargetClass) -> Result<(), TargetError> {
        self.entry_mut(actor)?.record.class = class;
        Ok(())
    }

    /// Updates whether mission rules flag the actor as an objective target.
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] or [`TargetError::UnknownActor`].
    pub fn set_objective(&mut self, actor: ActorId, objective: bool) -> Result<(), TargetError> {
        self.entry_mut(actor)?.record.objective = objective;
        Ok(())
    }

    /// Records a lifecycle transition for the actor.
    ///
    /// Destruction, despawn and mission removal end targetability;
    /// bailout and capture do not (see [`ends_targeting`]). Recording is
    /// idempotent — two authoritative reporters may observe the same
    /// transition.
    ///
    /// A transition that does **not** end targetability never touches a
    /// recorded ending: a `PilotBailout` or a `Captured` reported after a
    /// destruction leaves the actor out of the world rather than bringing it
    /// back into the selectable set (F30-D, pinned by
    /// `accept_f30_d_crosshair_selection_needs_eligibility_and_free_sight_together`).
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] or [`TargetError::UnknownActor`].
    pub fn record_lifecycle(
        &mut self,
        actor: ActorId,
        kind: LifecycleKind,
    ) -> Result<(), TargetError> {
        let entry = self.entry_mut(actor)?;
        if ends_targeting(kind) {
            entry.gone = Some(kind);
        }
        Ok(())
    }

    /// Declares or redeclares a directed faction relation — the scripted
    /// relation-change transaction. See [`AllegianceTable::declare`].
    pub fn set_allegiance(&mut self, from: ContentId, to: ContentId, allegiance: Allegiance) {
        self.allegiance.declare(from, to, allegiance);
    }

    /// The declared allegiance of `from` toward `to`; `None` when the
    /// pair was never declared. `from == to` is `Friendly`.
    #[must_use]
    pub fn allegiance(&self, from: &ContentId, to: &ContentId) -> Option<Allegiance> {
        self.allegiance.get(from, to)
    }

    /// Records an authoritative attack in the threat ledger.
    ///
    /// Only a typed [`AttackEvent`] mints a threat cue — proximity to an
    /// enemy never does (non-negotiable 4). The event's attacker, victim
    /// and evidence must all belong to this session; redelivery of the
    /// same evidence id for the same pair is idempotent.
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] when any part of the event carries
    /// another generation, [`TargetError::UnknownActor`] when attacker or
    /// victim is unregistered.
    pub fn record_attack(&mut self, event: AttackEvent) -> Result<(), TargetError> {
        for actor in [event.attacker, event.victim] {
            if actor.session.get() != self.session {
                return Err(TargetError::ForeignSession {
                    expected: self.session,
                    found: actor.session.get(),
                });
            }
        }
        if event.evidence.session.get() != self.session {
            return Err(TargetError::ForeignSession {
                expected: self.session,
                found: event.evidence.session.get(),
            });
        }
        if !self.records.contains_key(&event.attacker) {
            return Err(TargetError::UnknownActor {
                actor: event.attacker,
            });
        }
        if !self.records.contains_key(&event.victim) {
            return Err(TargetError::UnknownActor {
                actor: event.victim,
            });
        }
        let ledger = self.attacks.entry(event.victim).or_default();
        if !ledger
            .iter()
            .any(|attack| attack.evidence == event.evidence && attack.attacker == event.attacker)
        {
            ledger.push(event);
        }
        Ok(())
    }

    /// The production threat feed: turns a tick's authoritative hits into
    /// ledger entries and reports what the batch did.
    ///
    /// The hits are the damage system's own [`HitEvent`]s, so a cue is
    /// always evidence of a hit somebody landed rather than of proximity
    /// or of a request to select (non-negotiable 4). The whole batch is
    /// checked against this store's session first, so a hit from another
    /// generation refuses the batch instead of half-recording it; hits
    /// naming actors the roster does not track are counted, not refused —
    /// damage records exist for parts targeting never listed.
    ///
    /// # Errors
    ///
    /// [`TargetError::ForeignSession`] when a hit's evidence, its target
    /// or its attacker belongs to another generation.
    pub fn record_hits(&mut self, hits: &[HitEvent]) -> Result<ThreatFeed, TargetError> {
        for hit in hits {
            for session in [
                Some(hit.id.session.get()),
                Some(hit.target.session.get()),
                hit.attacker.map(|attacker| attacker.session.get()),
            ]
            .into_iter()
            .flatten()
            {
                if session != self.session {
                    return Err(TargetError::ForeignSession {
                        expected: self.session,
                        found: session,
                    });
                }
            }
        }

        let mut feed = ThreatFeed::default();
        for hit in hits {
            let Some(attack) = AttackEvent::from_hit(hit) else {
                feed.unattributed += 1;
                continue;
            };
            if !self.records.contains_key(&attack.attacker)
                || !self.records.contains_key(&attack.victim)
            {
                feed.untracked += 1;
                continue;
            }
            let ledger = self.attacks.entry(attack.victim).or_default();
            if ledger
                .iter()
                .any(|held| held.evidence == attack.evidence && held.attacker == attack.attacker)
            {
                feed.repeated += 1;
            } else {
                ledger.push(attack);
                feed.recorded += 1;
            }
        }
        Ok(feed)
    }

    /// Every registered actor, in stable actor order.
    #[must_use]
    pub fn registered(&self) -> Vec<ActorId> {
        self.records.keys().copied().collect()
    }

    /// Removes the actor from the roster and from the threat ledger, in
    /// both directions.
    ///
    /// This is the *entity left the world* transaction, which is not the
    /// same statement as a recorded [`LifecycleKind::Destroyed`]: a
    /// destroyed actor keeps its attack evidence in the ledger (the record
    /// of what killed it is evidence, and the F30-A acceptance case asserts
    /// it), whereas an unregistered actor is not in the world at all and can
    /// never be a live threat cue or a selectable target.
    ///
    /// Unregistering an actor that is not registered is a no-op, so two
    /// systems noticing the same departure cannot fail each other.
    pub fn unregister(&mut self, actor: ActorId) {
        if actor.session.get() != self.session {
            return;
        }
        self.records.remove(&actor);
        self.attacks.remove(&actor);
        for ledger in self.attacks.values_mut() {
            ledger.retain(|attack| attack.attacker != actor);
        }
    }

    /// The live threat cues against `victim` at tick `now`: one
    /// [`ThreatCue`] per attacker that produced an authoritative attack
    /// inside the declared [`TargetPolicy::threat_window_ticks`], most
    /// recent first, ties broken by actor id.
    #[must_use]
    pub fn threats(&self, victim: ActorId, now: Tick) -> Vec<ThreatCue> {
        let mut latest: BTreeMap<ActorId, Tick> = BTreeMap::new();
        if let Some(attacks) = self.attacks.get(&victim) {
            for attack in attacks {
                // now >= at is the normal case; an at after now still
                // counts (saturating → 0 <= window) — a recorded attack is
                // evidence regardless of clock skew between producers.
                if now.0.saturating_sub(attack.at.0) <= self.policy.threat_window_ticks {
                    let entry = latest.entry(attack.attacker).or_insert(attack.at);
                    if attack.at > *entry {
                        *entry = attack.at;
                    }
                }
            }
        }
        let mut cues: Vec<ThreatCue> = latest
            .into_iter()
            .map(|(attacker, last_attack)| ThreatCue {
                attacker,
                last_attack,
            })
            .collect();
        cues.sort_by(|a, b| {
            b.last_attack
                .cmp(&a.last_attack)
                .then(a.attacker.cmp(&b.attacker))
        });
        cues
    }

    /// Whether the actor is currently eligible for selection: registered,
    /// not ended by a lifecycle transition, revealed and phase-eligible
    /// (non-negotiable 1 and 2 — cycling includes only eligible live
    /// actors).
    #[must_use]
    pub fn eligible(&self, actor: &ActorId) -> bool {
        self.records.get(actor).is_some_and(|entry| {
            entry.gone.is_none() && entry.record.revealed && entry.record.phase_eligible
        })
    }

    /// Whether the actor is still **in the world**: registered and not ended
    /// by a lifecycle transition.
    ///
    /// This is deliberately weaker than [`TargetStore::eligible`]. An actor
    /// that lost sensor contact or whose script phase closed is still a thing
    /// in the sky that can shoot at the observer; one whose entity left the
    /// world or that was destroyed is not. A consumer that must not warn about
    /// a wreck reads this, and one that must not select a hidden actor reads
    /// [`TargetStore::eligible`] (F30-C).
    #[must_use]
    pub fn present(&self, actor: &ActorId) -> bool {
        self.records
            .get(actor)
            .is_some_and(|entry| entry.gone.is_none())
    }

    /// Why the actor stopped being eligible, or `None` when it is still
    /// eligible.
    ///
    /// An actor the roster does not hold reports
    /// [`SelectionClearReason::LeftWorld`]: the roster forgets an actor only
    /// through [`TargetStore::unregister`], so a selection naming an unknown
    /// actor can only mean that its entity left the world. The variants are
    /// tested in a fixed order — lifecycle transition, then reveal, then
    /// script phase — so one store always reports one reason for one actor.
    #[must_use]
    pub fn clear_reason(&self, actor: &ActorId) -> Option<SelectionClearReason> {
        let Some(entry) = self.records.get(actor) else {
            return Some(SelectionClearReason::LeftWorld);
        };
        if let Some(kind) = entry.gone {
            return Some(SelectionClearReason::Ended(kind));
        }
        if !entry.record.revealed {
            return Some(SelectionClearReason::NotRevealed);
        }
        if !entry.record.phase_eligible {
            return Some(SelectionClearReason::PhaseIneligible);
        }
        None
    }

    /// Whether `record` matches `filter` as seen by `observer_faction`.
    fn matches(
        &self,
        filter: &TargetFilter,
        observer_faction: &ContentId,
        record: &TargetRecord,
    ) -> bool {
        match filter {
            TargetFilter::Any => true,
            TargetFilter::Allegiance(want) => {
                self.allegiance.get(observer_faction, &record.faction) == Some(*want)
            }
            TargetFilter::Class(class) => record.class == *class,
            TargetFilter::NotClass(class) => record.class != *class,
            TargetFilter::Objective => record.objective,
        }
    }

    /// The eligible actors matching `filter` as seen by `observer`, in the
    /// declared total order: ascending distance from the observer, ties on
    /// the stable session-qualified actor id.
    ///
    /// The order is computed from canonical f64 world positions and actor
    /// ids only — insertion and ECS iteration order can never change it
    /// (non-negotiable 2; AC01's equal-distance case is exactly the
    /// actor-id tie-break).
    ///
    /// # Errors
    ///
    /// [`TargetError::UnknownActor`] when `observer` is unregistered.
    pub fn ordered(
        &self,
        observer: ActorId,
        filter: TargetFilter,
    ) -> Result<Vec<ActorId>, TargetError> {
        let observer_record = self
            .record(&observer)
            .ok_or(TargetError::UnknownActor { actor: observer })?;
        let from = observer_record.position;
        let mut ranked: Vec<(f64, ActorId)> = self
            .records
            .values()
            .filter(|entry| entry.record.actor != observer)
            .filter(|entry| self.eligible(&entry.record.actor))
            .filter(|entry| self.matches(&filter, &observer_record.faction, &entry.record))
            .map(|entry| {
                (
                    distance_squared(from, entry.record.position),
                    entry.record.actor,
                )
            })
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        Ok(ranked.into_iter().map(|(_, actor)| actor).collect())
    }

    /// A read-only snapshot of `actor` as seen by `observer`: class,
    /// faction, live allegiance, eligibility and distance. `None` when
    /// either actor is unregistered.
    #[must_use]
    pub fn info(&self, observer: ActorId, actor: ActorId) -> Option<TargetInfo> {
        let observer_record = self.record(&observer)?;
        let entry = self.records.get(&actor)?;
        Some(TargetInfo {
            actor,
            class: entry.record.class,
            faction: entry.record.faction.clone(),
            allegiance: self
                .allegiance
                .get(&observer_record.faction, &entry.record.faction),
            objective: entry.record.objective,
            eligible: self.eligible(&actor),
            position: entry.record.position,
            distance: Meters(
                distance_squared(observer_record.position, entry.record.position).sqrt(),
            ),
        })
    }

    /// Drops `selection`'s current target if it stopped being eligible —
    /// called before any consumer (reticle, spyglass) reads it, so a
    /// destroyed, hidden or phase-gated target is cleared safely rather
    /// than rendered stale (AC03's contract half).
    pub fn prune(&self, selection: &mut TargetSelection) {
        if selection
            .current
            .is_some_and(|actor| !self.eligible(&actor))
        {
            selection.clear();
        }
    }

    /// Evaluates one [`SelectionRequest`] for `observer`, updating
    /// `selection` and returning the resulting target.
    ///
    /// Determinism: every variant resolves through [`TargetStore::ordered`]
    /// or the equally total crosshair order, so the same roster and
    /// request always produce the same answer.
    ///
    /// # Errors
    ///
    /// [`TargetError::UnknownActor`] when `observer` is unregistered.
    pub fn apply(
        &self,
        observer: ActorId,
        selection: &mut TargetSelection,
        request: &SelectionRequest,
    ) -> Result<Option<ActorId>, TargetError> {
        match request {
            SelectionRequest::Clear => {
                selection.clear();
                Ok(None)
            }
            SelectionRequest::Cycle { direction, filter } => {
                let ordered = self.ordered(observer, *filter)?;
                let picked = match (direction, selection.current()) {
                    (_, None) if ordered.is_empty() => None,
                    (CycleDirection::Next, None) => ordered.first().copied(),
                    (CycleDirection::Previous, None) => ordered.last().copied(),
                    (direction, Some(current)) => {
                        match ordered.iter().position(|actor| *actor == current) {
                            Some(index) => Some(match direction {
                                CycleDirection::Next => ordered[(index + 1) % ordered.len()],
                                CycleDirection::Previous => {
                                    ordered[(index + ordered.len() - 1) % ordered.len()]
                                }
                            }),
                            // The held selection is outside this filter's
                            // set: restart the walk from an end.
                            None => match direction {
                                CycleDirection::Next => ordered.first().copied(),
                                CycleDirection::Previous => ordered.last().copied(),
                            },
                        }
                    }
                };
                match picked {
                    Some(actor) => selection.set(actor),
                    None => selection.clear(),
                }
                Ok(picked)
            }
            SelectionRequest::Nearest { filter } => {
                let picked = self.ordered(observer, *filter)?.first().copied();
                match picked {
                    Some(actor) => selection.set(actor),
                    None => selection.clear(),
                }
                Ok(picked)
            }
            SelectionRequest::NearestAttacker { now } => {
                let observer_position = self
                    .record(&observer)
                    .ok_or(TargetError::UnknownActor { actor: observer })?
                    .position;
                let mut ranked: Vec<(f64, ActorId)> = self
                    .threats(observer, *now)
                    .into_iter()
                    .filter(|cue| self.eligible(&cue.attacker))
                    .filter_map(|cue| {
                        self.record(&cue.attacker).map(|record| {
                            (
                                distance_squared(observer_position, record.position),
                                cue.attacker,
                            )
                        })
                    })
                    .collect();
                ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                let picked = ranked.first().map(|(_, actor)| *actor);
                match picked {
                    Some(actor) => selection.set(actor),
                    None => selection.clear(),
                }
                Ok(picked)
            }
            SelectionRequest::UnderCrosshair(query) => {
                let picked = self.under_crosshair(observer, query)?;
                match picked {
                    Some(actor) => selection.set(actor),
                    None => selection.clear(),
                }
                Ok(picked)
            }
        }
    }

    /// Runs one bound [`SelectionAction`] for `observer` against the phase's
    /// context, updating `selection` and returning the resulting target.
    ///
    /// This is the F30-B production path from a command edge to a
    /// selection: the action supplies *what* to select, the
    /// [`SelectionFrame`] supplies the phase's tick and crosshair ray, and
    /// the result is the same deterministic answer
    /// [`TargetStore::apply`] gives for the equivalent request.
    ///
    /// # Errors
    ///
    /// [`TargetError::UnknownActor`] when `observer` is unregistered,
    /// [`TargetError::MissingCrosshair`] when the action needs a crosshair
    /// ray the frame does not carry.
    pub fn act(
        &self,
        observer: ActorId,
        selection: &mut TargetSelection,
        action: &SelectionAction,
        frame: SelectionFrame<'_>,
    ) -> Result<Option<ActorId>, TargetError> {
        let request = match action {
            SelectionAction::Cycle { direction, filter } => SelectionRequest::Cycle {
                direction: *direction,
                filter: *filter,
            },
            SelectionAction::Nearest { filter } => SelectionRequest::Nearest { filter: *filter },
            SelectionAction::NearestAttacker => {
                SelectionRequest::NearestAttacker { now: frame.now }
            }
            SelectionAction::UnderCrosshair => SelectionRequest::UnderCrosshair(
                frame
                    .crosshair
                    .ok_or(TargetError::MissingCrosshair)?
                    .clone(),
            ),
            SelectionAction::Clear => SelectionRequest::Clear,
        };
        self.apply(observer, selection, &request)
    }

    /// Builds this phase's crosshair query for the producer's ray, using
    /// the declared default cone when the producer does not override it.
    ///
    /// The declared [`TargetPolicy::crosshair_cone`] is the fallback, not a
    /// value the producer may ignore: a producer with its own cone passes
    /// it, and a producer that only knows a ray takes the declared one. The
    /// cone is validated here, so a corrupt declared or supplied angle is
    /// refused at the boundary rather than inside a sort.
    ///
    /// # Errors
    ///
    /// [`CrosshairError`] on a non-finite or out-of-range cone.
    pub fn crosshair_query(
        &self,
        origin: WorldPosition,
        direction: UnitVec3,
        cone: Option<Radians>,
        occluded: BTreeSet<ActorId>,
    ) -> Result<CrosshairQuery, CrosshairError> {
        CrosshairQuery::try_new(
            origin,
            direction,
            cone.unwrap_or(self.policy.crosshair_cone),
            occluded,
        )
    }

    /// One phase boundary: prunes a selection whose target stopped being
    /// eligible, then derives the reticle record and the observer's threat
    /// cues from the same reads.
    ///
    /// The pruning runs *before* the record is derived, so a destroyed,
    /// hidden or phase-gated target is never described by a phase that
    /// reports it (AC03's contract half). The reason the selection went is
    /// read from the roster *before* the prune, so the record says why as
    /// well as that — a consumer learns its target is gone from this one call
    /// instead of comparing two records across the boundary. The reticle's
    /// allegiance and its hostility verdict are two fields of one read of the
    /// roster, so a faction change applied before this call reaches both in
    /// the same boundary and no consumer can see one without the other
    /// (AC02).
    ///
    /// Calling it twice for one boundary is safe and the second call reports
    /// no clear: the first already applied it.
    ///
    /// # Errors
    ///
    /// [`TargetError::UnknownActor`] when `observer` is unregistered.
    pub fn phase(
        &self,
        observer: ActorId,
        selection: &mut TargetSelection,
        now: Tick,
    ) -> Result<TargetPhase, TargetError> {
        let observer_record = self
            .record(&observer)
            .ok_or(TargetError::UnknownActor { actor: observer })?;
        let observer_faction = &observer_record.faction;
        let observer_position = observer_record.position;
        let held = selection.current();
        let cleared = held.and_then(|actor| {
            self.clear_reason(&actor)
                .map(|reason| ClearedSelection { actor, reason })
        });
        self.prune(selection);
        let threats = self.threats(observer, now);
        let reticle = selection.current().and_then(|target| {
            let record = self.record(&target)?;
            let allegiance = self.allegiance(observer_faction, &record.faction);
            let threatening = threats.iter().any(|cue| cue.attacker == target);
            Some(Reticle {
                target,
                class: record.class,
                faction: record.faction.clone(),
                allegiance,
                hostile: allegiance == Some(Allegiance::Hostile),
                threatening,
                objective: record.objective,
                revealed: record.revealed,
                position: record.position,
                distance: Meters(distance_squared(observer_position, record.position).sqrt()),
            })
        });
        Ok(TargetPhase {
            at: now,
            selection: selection.current(),
            cleared,
            reticle,
            threats,
        })
    }

    /// Derives the weapon-guidance record for `observer`'s current selection
    /// at `now`.
    ///
    /// Guidance is derived from the same reads as [`TargetStore::phase`] — the
    /// same pruning, the same reticle — and carries no authority: it names a
    /// target and a bearing toward it, and there is deliberately no field here
    /// for a lead solution, an aim correction or a damage amount. Computing a
    /// lead point needs the target's velocity and the projectile's measured
    /// ballistics, neither of which targeting owns and neither of which is
    /// measured (F30-D), so producing one here would be a fabricated value
    /// wearing the appearance of original behavior (F30 non-negotiable 3).
    ///
    /// The eligibility gate is the reticle's own `hostile` verdict, applied
    /// here rather than in a consumer: an aid is offered for a **declared**
    /// hostile only, so a friendly, neutral or *undeclared* pair can never
    /// receive one however the consumer reads the record. Refusing rather than
    /// returning a record with `hostile: false` keeps the refusal with the
    /// roster fact that produced it.
    ///
    /// # Errors
    ///
    /// [`TargetError::UnknownActor`] when `observer` is unregistered,
    /// [`TargetError::NoSelection`] when nothing is selected (a cleared or
    /// absent selection is not a target to assist),
    /// [`TargetError::UnknownSelection`] when `selection` names an actor the
    /// roster does not hold — which only an already-orphaned selection can do —
    /// and [`TargetError::NotHostile`] when the selected actor is not a
    /// declared hostile.
    pub fn guidance(
        &self,
        observer: ActorId,
        selection: &mut TargetSelection,
        now: Tick,
    ) -> Result<WeaponGuidance, TargetError> {
        let observer_record = self
            .record(&observer)
            .ok_or(TargetError::UnknownActor { actor: observer })?;
        let observer_position = observer_record.position;
        self.prune(selection);
        let Some(target) = selection.current() else {
            return Err(TargetError::NoSelection);
        };
        let Some(record) = self.record(&target) else {
            return Err(TargetError::UnknownSelection { actor: target });
        };
        let allegiance = self.allegiance(&observer_record.faction, &record.faction);
        if allegiance != Some(Allegiance::Hostile) {
            return Err(TargetError::NotHostile {
                actor: target,
                allegiance,
            });
        }
        let threatening = self
            .threats(observer, now)
            .iter()
            .any(|cue| cue.attacker == target);
        let [ox, oy, oz] = observer_position.to_array();
        let [tx, ty, tz] = record.position.to_array();
        let [x, y, z] = [tx - ox, ty - oy, tz - oz];
        let length = x.hypot(y).hypot(z);
        // A target sitting exactly on the observer has no bearing, and
        // `UnitVec3::try_new` validates finiteness and unit length — the
        // normalized offset is both, so this cannot fail for a validated
        // position. Refused rather than defaulted: the consumer would
        // otherwise draw an aid toward an arbitrary axis.
        let bearing = UnitVec3::try_new([x / length, y / length, z / length])
            .map_err(|_| TargetError::DegenerateBearing { actor: target })?;
        Ok(WeaponGuidance {
            at: now,
            target,
            position: record.position,
            bearing,
            distance: Meters(distance_squared(observer_position, record.position).sqrt()),
            allegiance,
            hostile: true,
            threatening,
        })
    }

    /// The eligible, unoccluded actor — never the observer — nearest the
    /// query's ray direction within its cone: maximize `cos(angle)`, break
    /// angular ties on ascending distance, then on actor id, so the pick
    /// is total and deterministic.
    fn under_crosshair(
        &self,
        observer: ActorId,
        query: &CrosshairQuery,
    ) -> Result<Option<ActorId>, TargetError> {
        if self.record(&observer).is_none() {
            return Err(TargetError::UnknownActor { actor: observer });
        }
        let cone_cos = query.cone.0.cos();
        let [dx, dy, dz] = query.direction.to_array();
        let mut ranked: Vec<(f64, f64, ActorId)> = self
            .records
            .values()
            .filter(|entry| entry.record.actor != observer)
            .filter(|entry| self.eligible(&entry.record.actor))
            .filter(|entry| !query.occluded.contains(&entry.record.actor))
            .filter_map(|entry| {
                let [px, py, pz] = entry.record.position.to_array();
                let [ox, oy, oz] = query.origin.to_array();
                let to = [px - ox, py - oy, pz - oz];
                let len_sq = to[0] * to[0] + to[1] * to[1] + to[2] * to[2];
                if len_sq == 0.0 {
                    // The target sits exactly on the origin: it has no
                    // direction, so it is never an angular pick.
                    return None;
                }
                let inv = 1.0 / len_sq.sqrt();
                let cos = to[0] * inv * dx + to[1] * inv * dy + to[2] * inv * dz;
                (cos >= cone_cos).then_some((cos, len_sq, entry.record.actor))
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then(a.1.total_cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        Ok(ranked.first().map(|(_, _, actor)| *actor))
    }

    fn entry_mut(&mut self, actor: ActorId) -> Result<&mut TargetEntry, TargetError> {
        if actor.session.get() != self.session {
            return Err(TargetError::ForeignSession {
                expected: self.session,
                found: actor.session.get(),
            });
        }
        self.records
            .get_mut(&actor)
            .ok_or(TargetError::UnknownActor { actor })
    }
}

/// Squared canonical distance between two world positions (f64, so the
/// ordering is exact and survives origin rebases).
fn distance_squared(a: WorldPosition, b: WorldPosition) -> f64 {
    let [ax, ay, az] = a.to_array();
    let [bx, by, bz] = b.to_array();
    let (dx, dy, dz) = (ax - bx, ay - by, az - bz);
    dx * dx + dy * dy + dz * dz
}

// ----------------------------------------------------------- fixture ------

fn synthetic_faction(key: &str) -> ContentId {
    ContentId::from_source(cs_types::content::ContentKind::Faction, key)
        .expect("fixture faction id is valid")
}

/// The fixture's player-side faction.
#[must_use]
pub fn synthetic_player_faction() -> ContentId {
    synthetic_faction("synthetic.nathan")
}

/// The fixture's hostile faction.
#[must_use]
pub fn synthetic_raider_faction() -> ContentId {
    synthetic_faction("synthetic.raiders")
}

/// The fixture's neutral faction.
#[must_use]
pub fn synthetic_trader_faction() -> ContentId {
    synthetic_faction("synthetic.traders")
}

/// The minimal synthetic allegiance table: the player's faction and the
/// raiders hostile in both directions, traders neutral toward everyone
/// (declared both ways — a relation is directed), raiders hostile to
/// traders.
///
/// Newly authored fixture data with designed provenance — never a stand-in
/// for the original game's faction matrix, which is unmeasured.
#[must_use]
pub fn synthetic_allegiance_table() -> AllegianceTable {
    let mut table = AllegianceTable::new();
    let (player, raiders, traders) = (
        synthetic_player_faction(),
        synthetic_raider_faction(),
        synthetic_trader_faction(),
    );
    table.declare(player.clone(), raiders.clone(), Allegiance::Hostile);
    table.declare(raiders.clone(), player.clone(), Allegiance::Hostile);
    table.declare(player.clone(), traders.clone(), Allegiance::Neutral);
    table.declare(traders.clone(), player.clone(), Allegiance::Neutral);
    table.declare(raiders.clone(), traders.clone(), Allegiance::Hostile);
    table.declare(traders.clone(), raiders.clone(), Allegiance::Hostile);
    table
}

/// The synthetic policy: a 120-tick threat window and a 10-degree
/// crosshair cone. Both are designed fixture values, not original data.
#[must_use]
pub fn synthetic_target_policy() -> TargetPolicy {
    TargetPolicy {
        threat_window_ticks: 120,
        crosshair_cone: Radians(std::f64::consts::PI / 18.0),
    }
}

/// The synthetic command-edge table: the two declared target edges walk a
/// cycle over the fixture's declared hostiles — newly authored design, not
/// a claim about the original game's keys.
#[must_use]
pub fn synthetic_selection_binding() -> SelectionBinding {
    SelectionBinding::cycling(TargetFilter::Allegiance(Allegiance::Hostile))
}

#[allow(clippy::too_many_arguments)]
fn record(
    session: u64,
    serial: u64,
    faction: ContentId,
    class: TargetClass,
    position: [f64; 3],
    revealed: bool,
    phase_eligible: bool,
    objective: bool,
) -> TargetRecord {
    TargetRecord {
        actor: ActorId {
            session: SessionId::new(session).expect("a nonzero session generation"),
            serial,
        },
        faction,
        class,
        objective,
        revealed,
        phase_eligible,
        position: WorldPosition::try_new(position).expect("fixture position is finite"),
    }
}

/// The minimal synthetic roster for `session`: the player observer at the
/// origin plus six targets — three raider aircraft exactly 100 m away on
/// three different axes (the AC01 equal-distance case, with serials
/// deliberately out of coordinate order so insertion order cannot leak
/// into the cycle), a friendly wingman, a neutral trader flagged as the
/// objective, and a hidden raider that is present but never eligible.
///
/// Serials: player 1; raiders 9 (+X), 2 (+Y), 5 (−Z); wingman 3; trader 4;
/// hidden raider 7.
#[must_use]
pub fn synthetic_roster(session: u64) -> Vec<TargetRecord> {
    let (player, raiders, traders) = (
        synthetic_player_faction(),
        synthetic_raider_faction(),
        synthetic_trader_faction(),
    );
    vec![
        record(
            session,
            1,
            player.clone(),
            TargetClass::Aircraft,
            [0.0, 0.0, 0.0],
            true,
            true,
            false,
        ),
        record(
            session,
            9,
            raiders.clone(),
            TargetClass::Aircraft,
            [100.0, 0.0, 0.0],
            true,
            true,
            false,
        ),
        record(
            session,
            2,
            raiders.clone(),
            TargetClass::Aircraft,
            [0.0, 100.0, 0.0],
            true,
            true,
            false,
        ),
        record(
            session,
            5,
            raiders.clone(),
            TargetClass::Aircraft,
            [0.0, 0.0, -100.0],
            true,
            true,
            false,
        ),
        record(
            session,
            3,
            player.clone(),
            TargetClass::Aircraft,
            [50.0, 0.0, 0.0],
            true,
            true,
            false,
        ),
        record(
            session,
            4,
            traders.clone(),
            TargetClass::WorldObject,
            [0.0, 60.0, 0.0],
            true,
            true,
            true,
        ),
        record(
            session,
            7,
            raiders.clone(),
            TargetClass::Aircraft,
            [10.0, 0.0, 0.0],
            false,
            true,
            false,
        ),
    ]
}
