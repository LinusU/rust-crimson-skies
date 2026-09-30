//! Target queries, allegiance and threat contracts (F30-A).
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Stage **F30-A** defines the typed contract — the inputs a session feeds
//! the targeting authority, the queries it answers and the records it
//! returns — plus the minimal synthetic fixture the acceptance tests
//! drive. It does not implement the original selection-action bindings
//! (F30-B maps `cs_types::input::FlightCommand::TargetNext`/`TargetPrev`
//! edges and the rest of the declared request vocabulary onto
//! [`SelectionRequest`]), the HUD/spyglass/weapon consumers (F30-C) or the
//! original-data verification of ordering, reveal and assistance rules
//! (F30-D).
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
//!
//! # Determinism
//!
//! Ordering is a total order over `(distance², ActorId)`: distance ties —
//! the AC01 "equal-distance targets" case — break on the stable
//! session-qualified actor id, so the cycle sequence is a function of the
//! roster, never of ECS iteration or insertion order (non-negotiable 2).
//!
//! # Designed vocabulary, not original data
//!
//! Which selection actions the original 2000 PC game exposes, how its
//! cycle orders targets, its crosshair cone, reveal rules and assistance
//! behavior are unmeasured (F30 "Research boundary"; F30-D's retail stage).
//! Every vocabulary value, filter and fixture number here is newly
//! authored engine design, recorded in
//! `docs/findings/2026-09-30-f30-a-target-queries-and-allegiance-contracts.md`.
//! The declared, provenance-carrying half is
//! `cs_content::target_rules`; the lowering boundary and ECS bindings are
//! `cs_app::targeting`.
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
use cs_types::space::{Meters, Radians, UnitVec3, WorldPosition};

use crate::damage::{ActorId, HitEventId, LifecycleKind};

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

/// The direction a cycle request walks the ordered target list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CycleDirection {
    /// Toward the next farther target in the ordering, wrapping.
    Next,
    /// Toward the previous nearer target in the ordering, wrapping.
    Previous,
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
        if record.actor.session != self.session {
            return Err(TargetError::ForeignSession {
                expected: self.session,
                found: record.actor.session,
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

    /// Records a lifecycle transition for the actor.
    ///
    /// Destruction, despawn and mission removal end targetability;
    /// bailout and capture do not (see [`ends_targeting`]). Recording is
    /// idempotent — two authoritative reporters may observe the same
    /// transition.
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
            if actor.session != self.session {
                return Err(TargetError::ForeignSession {
                    expected: self.session,
                    found: actor.session,
                });
            }
        }
        if event.evidence.session != self.session {
            return Err(TargetError::ForeignSession {
                expected: self.session,
                found: event.evidence.session,
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

    /// The eligible, unoccluded actor nearest the query's ray direction
    /// within its cone: maximize `cos(angle)`, tie-break on actor id, so
    /// the pick is total and deterministic.
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
        let mut ranked: Vec<(f64, ActorId)> = self
            .records
            .values()
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
                (cos >= cone_cos).then_some((cos, entry.record.actor))
            })
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        Ok(ranked.first().map(|(_, actor)| *actor))
    }

    fn entry_mut(&mut self, actor: ActorId) -> Result<&mut TargetEntry, TargetError> {
        if actor.session != self.session {
            return Err(TargetError::ForeignSession {
                expected: self.session,
                found: actor.session,
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
        actor: ActorId { session, serial },
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
