//! The targeting application boundary and session wiring (F30-A, F30-B).
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stages `### F30-A` and `### F30-B`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module sits between the declared targeting schema
//! ([`cs_content::target_rules`]) and the session store
//! ([`cs_sim::targeting`]), which cannot see each other — `cs_sim` must
//! not depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_rules`] — the F30-A conversion boundary: a validated
//!   [`cs_content::target_rules::DeclaredTargetRules`] becomes the
//!   [`cs_sim::targeting::TargetPolicy`] and
//!   [`cs_sim::targeting::AllegianceTable`] a `TargetStore` opens a
//!   session with. Every `Resolved::Unknown` **refuses** rather than
//!   guessing: an unevidenced relation is not the same statement as "no
//!   relation" (the runtime reports that as `None` already), and a
//!   session never runs under a guessed threat window or assistance flag.
//! * [`lower_selection_actions`] — the F30-B conversion boundary for the
//!   declared action table: a
//!   [`cs_content::target_rules::DeclaredSelectionActions`] becomes the
//!   [`cs_sim::targeting::SelectionBinding`] that maps command edges onto
//!   selection actions, refusing an unknown action for the same reason.
//! * [`TargetableBinding`] — the ECS record tying an entity to its
//!   session-qualified [`cs_sim::damage::ActorId`] and the declared rules
//!   the binding was spawned under, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a
//!   stale binding looking live.
//! * [`TargetableState`] — the record an entity presents to targeting,
//!   including its **canonical** f64 [`WorldPosition`](cs_types::space::WorldPosition).
//!   Nothing here reads a local f32 pose, so a rebase cannot move a target,
//!   reorder a cycle or invalidate a selection (F30 non-negotiable 5).
//! * [`TargetingSession`] — the resource that owns one session's
//!   [`cs_sim::targeting::TargetStore`], its selection, its lowered
//!   command-edge table and the last [`cs_sim::targeting::TargetPhase`].
//!   It holds no targeting *rules*: those are the store's.
//! * The three producer entries a session drives —
//!   [`sync_targetable_roster`] (who is targetable and where),
//!   [`apply_selection_edges`] (which action a command edge runs) and
//!   [`apply_target_damage`] (which hits became attacks and which
//!   lifecycle transitions ended targetability).
//!
//! The HUD, spyglass and weapon consumers of the phase record are F30-C:
//! nothing here draws a reticle.

use std::collections::BTreeSet;

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cs_content::target_rules::{
    DeclaredAction, DeclaredAllegiance, DeclaredSelectionActions, DeclaredTargetRules,
    TargetRuleSet,
};
use cs_sim::damage::{ActorId, DamageEvent, DamageEventKind, HitEvent, HitEventId, LifecycleKind};
use cs_sim::targeting::{
    Allegiance, AllegianceTable, CycleDirection, SelectionAction, SelectionBinding, SelectionFrame,
    TargetClass, TargetError, TargetFilter, TargetPhase, TargetPolicy, TargetRecord,
    TargetSelection, TargetStore, ThreatFeed,
};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

use crate::scene::SceneGeneration;

/// What [`lower_rules`] produces: the runtime policy and the allegiance
/// table a `TargetStore` opens its session with, plus the two assistance
/// options its consumers own.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredTargetRules {
    /// The lowered policy knobs.
    pub policy: TargetPolicy,
    /// The lowered declared relations.
    pub allegiance: AllegianceTable,
    /// Whether a lead indicator is offered — a display aid only, kept
    /// separate from aim assistance (F30 non-negotiable 3).
    pub lead_indicator: bool,
    /// Whether aim assistance is offered. An option with declared
    /// evidence, never an automatic hit correction.
    pub aim_assistance: bool,
}

/// Why declared target rules could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetLowerError {
    /// A `TargetRuleSet` field is `Resolved::Unknown`: no session may run
    /// targeting under a guessed rule value.
    UnknownRule {
        /// Which field is unknown.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
    /// A declared relation's allegiance is `Resolved::Unknown`. Refused,
    /// because lowering it to "no relation" would silently reclassify a
    /// pair the importer flagged as unmeasured — exactly the silent
    /// friendly/enemy ambiguity F30 non-negotiable 1 forbids.
    UnknownRelation {
        /// The relation's index in the declared record's list.
        relation: usize,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the allegiance is unknown.
        reason: String,
    },
    /// A declared selection action is `Resolved::Unknown`. Refused, because
    /// lowering it to "no action" would quietly turn a declared target key
    /// into an inert one instead of saying the action is unmeasured.
    UnknownAction {
        /// The command edge the unknown binding names.
        command: cs_types::input::FlightCommand,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the action is unknown.
        reason: String,
    },
}

impl std::fmt::Display for TargetLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRule {
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "target rule {field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::UnknownRelation {
                relation,
                claim_id,
                reason,
            } => write!(
                f,
                "declared relation #{relation} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::UnknownAction {
                command,
                claim_id,
                reason,
            } => write!(
                f,
                "the action bound to {command} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
        }
    }
}

impl std::error::Error for TargetLowerError {}

fn lower_allegiance(allegiance: DeclaredAllegiance) -> Allegiance {
    match allegiance {
        DeclaredAllegiance::Hostile => Allegiance::Hostile,
        DeclaredAllegiance::Neutral => Allegiance::Neutral,
        DeclaredAllegiance::Friendly => Allegiance::Friendly,
    }
}

fn lower_rule<T>(field: &'static str, value: &Resolved<T>) -> Result<T, TargetLowerError>
where
    T: Clone,
{
    match value {
        Resolved::Known(known) => Ok(known.value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(TargetLowerError::UnknownRule {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

fn lower_rule_set(rules: &TargetRuleSet) -> Result<TargetPolicy, TargetLowerError> {
    Ok(TargetPolicy {
        threat_window_ticks: lower_rule("threat_window", &rules.threat_window)?,
        crosshair_cone: lower_rule("crosshair_cone", &rules.crosshair_cone)?,
    })
}

/// Lowers declared target rules into the runtime policy and allegiance
/// table a `TargetStore` opens with.
///
/// Relations map field-wise — directed pairs stay directed, the declared
/// allegiance maps onto the runtime vocabulary — and every
/// `Resolved::Unknown` refuses, so nothing is resolved, guessed or
/// repaired at this boundary. The two assistance options lower onto the
/// record for the F30-B/C consumers that own the HUD and aim paths; an
/// unknown flag fails the lowering rather than defaulting at the consumer.
///
/// # Errors
///
/// [`TargetLowerError::UnknownRule`] or
/// [`TargetLowerError::UnknownRelation`] on any unresolved declared value.
pub fn lower_rules(rules: &DeclaredTargetRules) -> Result<LoweredTargetRules, TargetLowerError> {
    let policy = lower_rule_set(rules.rules())?;
    let lead_indicator = lower_rule("lead_indicator", &rules.rules().lead_indicator)?;
    let aim_assistance = lower_rule("aim_assistance", &rules.rules().aim_assistance)?;

    let mut allegiance = AllegianceTable::new();
    for (index, relation) in rules.relations().iter().enumerate() {
        let value = match &relation.allegiance {
            Resolved::Known(known) => known.value,
            Resolved::Unknown { claim_id, reason } => {
                return Err(TargetLowerError::UnknownRelation {
                    relation: index,
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                });
            }
        };
        allegiance.declare(
            relation.from.clone(),
            relation.to.clone(),
            lower_allegiance(value),
        );
    }

    Ok(LoweredTargetRules {
        policy,
        allegiance,
        lead_indicator,
        aim_assistance,
    })
}

fn lower_action(action: DeclaredAction) -> SelectionAction {
    match action {
        DeclaredAction::NextHostile => SelectionAction::Cycle {
            direction: CycleDirection::Next,
            filter: TargetFilter::Allegiance(Allegiance::Hostile),
        },
        DeclaredAction::PreviousHostile => SelectionAction::Cycle {
            direction: CycleDirection::Previous,
            filter: TargetFilter::Allegiance(Allegiance::Hostile),
        },
        DeclaredAction::NearestHostile => SelectionAction::Nearest {
            filter: TargetFilter::Allegiance(Allegiance::Hostile),
        },
        DeclaredAction::NearestObjective => SelectionAction::Nearest {
            filter: TargetFilter::Objective,
        },
        DeclaredAction::NearestAlly => SelectionAction::Nearest {
            filter: TargetFilter::Allegiance(Allegiance::Friendly),
        },
        DeclaredAction::NearestNonAircraft => SelectionAction::Nearest {
            filter: TargetFilter::NotClass(TargetClass::Aircraft),
        },
        DeclaredAction::NearestAttacker => SelectionAction::NearestAttacker,
        DeclaredAction::UnderCrosshair => SelectionAction::UnderCrosshair,
        DeclaredAction::Clear => SelectionAction::Clear,
    }
}

/// Lowers a declared action table into the runtime
/// [`cs_sim::targeting::SelectionBinding`] a session runs command edges
/// through.
///
/// Each declared binding maps one command edge onto one runtime action, in
/// the order the table declares, so the same table always produces the same
/// binding. A `Resolved::Unknown` action refuses, exactly as an unknown
/// relation refuses in [`lower_rules`]: a command edge with no evidenced
/// action is *not* the same statement as a command edge that is not a
/// target command, and guessing which one it is would bind a key to
/// behavior the data never stated.
///
/// # Errors
///
/// [`TargetLowerError::UnknownAction`] on an unresolved declared action.
pub fn lower_selection_actions(
    actions: &DeclaredSelectionActions,
) -> Result<SelectionBinding, TargetLowerError> {
    let mut binding = SelectionBinding::new();
    for declared in actions.bindings() {
        let action = match &declared.action {
            Resolved::Known(known) => lower_action(known.value),
            Resolved::Unknown { claim_id, reason } => {
                return Err(TargetLowerError::UnknownAction {
                    command: declared.command,
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                });
            }
        };
        binding.bind(declared.command, action);
    }
    Ok(binding)
}

/// Component: marks an entity as targetable under one session's rules.
///
/// `actor` is the session-qualified [`ActorId`] the `TargetStore`
/// registered (its `session` is the session generation), `rules` the
/// catalog subject of the `DeclaredTargetRules` the session opened with,
/// and `generation` the scene generation the binding was spawned under —
/// so a reload stamps new bindings and stale ones are identified by
/// mismatch, never by surviving pointers (the `STATE-TRANSACTIONS`
/// session-generation discipline; the same rule
/// [`crate::scene::SceneNodeBinding`] and [`crate::damage::DamageActorBinding`]
/// follow).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TargetableBinding {
    /// The targeting actor this entity presents.
    pub actor: ActorId,
    /// The catalog id of the declared rules the session runs under.
    pub rules: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

/// Component: the record an entity presents to targeting.
///
/// The pose is canonical f64 ([`WorldPosition`]) rather than a local frame
/// value: a [`OriginShift`](crate::origin::OriginShift) changes the local
/// frame and leaves the world identity alone, so a rebase cannot move a
/// target, reorder a cycle or invalidate a selection behind the store's
/// back (F30 non-negotiable 5). The session's movement system writes the
/// field from the entity's own pose owner
/// ([`SpatialAnchor`](crate::origin::SpatialAnchor)'s `world()`); targeting
/// reads only this value and never a f32 local pose.
///
/// [`ActorId`] is not here — [`TargetableBinding`] owns identity, and the
/// roster sync joins the two.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TargetableState {
    /// The faction catalog id the actor belongs to; a capture or a scripted
    /// change writes it and the store re-derives every allegiance from it in
    /// the same phase boundary.
    pub faction: ContentId,
    /// What kind of actor this is.
    pub class: TargetClass,
    /// Whether mission rules flag the actor as an objective target.
    pub objective: bool,
    /// Whether the actor is revealed to sensors/HUD.
    pub revealed: bool,
    /// Whether the current script phase makes the actor eligible.
    pub phase_eligible: bool,
    /// The actor's canonical world position.
    pub position: WorldPosition,
}

impl TargetableState {
    /// The record of a live, revealed, phase-eligible aircraft that is not
    /// an objective target, at `position` — the ordinary spawn case.
    #[must_use]
    pub const fn aircraft(faction: ContentId, position: WorldPosition) -> Self {
        Self {
            faction,
            class: TargetClass::Aircraft,
            objective: false,
            revealed: true,
            phase_eligible: true,
            position,
        }
    }
}

/// Resource: one session's targeting authority as the ECS owns it.
///
/// The store, the observer's selection, the lowered command-edge table and
/// the last phase record live here; the *rules* live in the store, and the
/// declared records live in `cs_content`. A restart or an aircraft swap
/// installs a **new** session resource, so registrations, relation
/// overrides and recorded attacks never carry into the next generation
/// (the `STATE-TRANSACTIONS` session-generation discipline, the same rule
/// [`crate::scene::SceneGeneration`] and [`crate::input::session`] follow).
#[derive(Resource, Clone, Debug)]
pub struct TargetingSession {
    session: SessionId,
    store: TargetStore,
    selection: TargetSelection,
    bindings: SelectionBinding,
    subject: ContentId,
    generation: SceneGeneration,
    last_phase: Option<TargetPhase>,
}

impl TargetingSession {
    /// A session opening with `lowered` rules and a lowered command-edge
    /// table, under the catalog `subject` the bindings name, for the
    /// session generation `session` — the same generation the
    /// [`ActorId`]s its entities carry.
    #[must_use]
    pub fn new(
        session: SessionId,
        lowered: LoweredTargetRules,
        bindings: SelectionBinding,
        subject: ContentId,
        generation: SceneGeneration,
    ) -> Self {
        Self {
            session,
            store: TargetStore::new(session.get(), lowered.policy, lowered.allegiance),
            selection: TargetSelection::new(),
            bindings,
            subject,
            generation,
            last_phase: None,
        }
    }

    /// The session generation the store and every [`ActorId`] it accepts
    /// belong to.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The session's store: the roster, allegiance table and threat ledger.
    #[must_use]
    pub const fn store(&self) -> &TargetStore {
        &self.store
    }

    /// Mutable access to the store, for the producers that register actors
    /// and change relations.
    pub const fn store_mut(&mut self) -> &mut TargetStore {
        &mut self.store
    }

    /// The observer's current selection.
    #[must_use]
    pub const fn selection(&self) -> &TargetSelection {
        &self.selection
    }

    /// The lowered command-edge table.
    #[must_use]
    pub const fn bindings(&self) -> &SelectionBinding {
        &self.bindings
    }

    /// Mutable access to the command-edge table, for a rebind.
    pub const fn bindings_mut(&mut self) -> &mut SelectionBinding {
        &mut self.bindings
    }

    /// The catalog subject of the declared records this session runs under.
    #[must_use]
    pub const fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// The scene generation whose bindings this session accepts.
    #[must_use]
    pub const fn generation(&self) -> SceneGeneration {
        self.generation
    }

    /// The last phase record this session derived, if any. Consumers read
    /// the record rather than the store: it is the only view in which the
    /// reticle and the hostility gate agree.
    #[must_use]
    pub const fn last_phase(&self) -> Option<&TargetPhase> {
        self.last_phase.as_ref()
    }
}

/// Why a targeting entry could not run.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetingError {
    /// No [`TargetingSession`] resource is installed, so the entry has no
    /// store to act on. Reported rather than panicked: a schedule that
    /// forgets to install the session must fail visibly at the call that
    /// needed it.
    NoSession,
    /// The store refused the work.
    Store(TargetError),
}

impl std::fmt::Display for TargetingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSession => {
                write!(
                    f,
                    "no TargetingSession is installed, so targeting cannot run"
                )
            }
            Self::Store(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for TargetingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoSession => None,
            Self::Store(error) => Some(error),
        }
    }
}

impl From<TargetError> for TargetingError {
    fn from(error: TargetError) -> Self {
        Self::Store(error)
    }
}

/// What one [`sync_targetable_roster`] pass did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RosterReport {
    /// Actors newly registered with the store.
    pub registered: usize,
    /// Actors whose pose or classification was written from the ECS.
    pub updated: usize,
    /// Actors unregistered because their entity left the world.
    pub removed: usize,
    /// Bindings this session ignored: another scene generation, or an
    /// actor from another session generation.
    pub ignored: Vec<ActorId>,
    /// Entities that carry a binding but no [`TargetableState`], so their
    /// record could not be built. Reported rather than registered with a
    /// guessed position.
    pub incomplete: Vec<Entity>,
}

/// Keeps the session's roster in step with the entities that present a
/// [`TargetableBinding`] for this session's scene generation.
///
/// Three statements, all from the ECS and none from a caller's belief:
///
/// * a bound entity with a state and an anchor is registered on first sight
///   and its pose and classification are written on every later pass, so a
///   capture or a reveal change reaches the store without a second call;
/// * a registered actor whose entity is gone (despawned, or only bound
///   under an older generation) is unregistered — the *entity left the
///   world* transaction, which is not the same statement as a recorded
///   destruction and does not touch attack evidence elsewhere;
/// * a binding from another scene generation or another session generation
///   is reported and ignored, so a reload can never splice a stale entity
///   into a live roster.
pub fn sync_targetable_roster(world: &mut World) -> RosterReport {
    let Some(targeting) = world.get_resource::<TargetingSession>() else {
        return RosterReport::default();
    };
    let scene_generation = targeting.generation();
    let session = targeting.store().session();

    let mut present: BTreeSet<ActorId> = BTreeSet::new();
    let mut report = RosterReport::default();
    let mut candidates = Vec::new();
    let mut query = world.query::<(Entity, &TargetableBinding, Option<&TargetableState>)>();
    for (entity, binding, state) in query.iter(world) {
        if binding.generation != scene_generation || binding.actor.session.get() != session {
            report.ignored.push(binding.actor);
            continue;
        }
        let Some(state) = state else {
            report.incomplete.push(entity);
            continue;
        };
        present.insert(binding.actor);
        candidates.push((binding.actor, state.clone()));
    }

    let Some(mut targeting) = world.get_resource_mut::<TargetingSession>() else {
        return report;
    };
    let store = targeting.store_mut();
    for (actor, state) in candidates {
        let record = TargetRecord {
            actor,
            faction: state.faction,
            class: state.class,
            objective: state.objective,
            revealed: state.revealed,
            phase_eligible: state.phase_eligible,
            position: state.position,
        };
        if !store.is_registered(&actor) {
            // A refused registration names an actor this store already
            // holds under a different record, which the update branch below
            // would have to reconcile; count it as ignored rather than
            // inventing a second roster entry.
            if store.register(record.clone()).is_ok() {
                report.registered += 1;
            } else {
                report.ignored.push(actor);
            }
            continue;
        }
        // The pose and the classification are written through the store's
        // own transactions, so no consumer can observe a half-updated
        // record: a capture, a reclassification, a reveal or a phase change
        // reaches the store in the same pass that observes it, and every
        // field of the entity's record has a transaction to travel through.
        store
            .set_pose(actor, record.position)
            .expect("a registered actor accepts a pose");
        let held = store
            .record(&actor)
            .cloned()
            .expect("the actor is registered");
        if held.class != record.class {
            store
                .set_class(actor, record.class)
                .expect("a registered actor accepts a class");
        }
        if held.objective != record.objective {
            store
                .set_objective(actor, record.objective)
                .expect("a registered actor accepts an objective flag");
        }
        if held.faction != record.faction {
            store
                .set_faction(actor, record.faction)
                .expect("a registered actor accepts a faction");
        }
        if held.revealed != record.revealed {
            store
                .set_revealed(actor, record.revealed)
                .expect("a registered actor accepts a reveal state");
        }
        if held.phase_eligible != record.phase_eligible {
            store
                .set_phase_eligible(actor, record.phase_eligible)
                .expect("a registered actor accepts a phase state");
        }
        report.updated += 1;
    }

    for actor in store.registered() {
        if !present.contains(&actor) {
            store.unregister(actor);
            report.removed += 1;
        }
    }
    report
}

/// What one [`apply_selection_edges`] pass did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionReport {
    /// The commands that were bound to a selection action, in the order the
    /// edges arrived.
    pub acted: Vec<FlightCommand>,
    /// Flight edges that are not target commands — control, weapons,
    /// ordnance — which changed no selection.
    pub ignored: Vec<FlightCommand>,
    /// The observer's selection after the pass, whether or not an edge acted
    /// on it: the session's selection, not the last pick.
    pub selection: Option<ActorId>,
    /// The phase record the pass derived.
    pub phase: Option<TargetPhase>,
}

/// Runs one frame's edges through the session's command-edge table and
/// derives the phase record the consumers read.
///
/// Each bound edge runs [`cs_sim::targeting::TargetStore::act`] with the
/// frame's tick and crosshair ray, in arrival order, so two edges in one
/// frame walk the cycle twice — a real key press, not a coalesced one. The
/// phase record is derived once at the end of the pass, so a consumer
/// cannot read a selection from before the last edge and an allegiance from
/// after it.
///
/// # Errors
///
/// [`TargetingError::NoSession`] when no session is installed,
/// [`TargetError::UnknownActor`] when `observer` is not registered and
/// [`TargetError::MissingCrosshair`] when an under-crosshair edge fires in a
/// frame that carries no ray. The selection is left as the last successful
/// edge left it, and no phase record is written.
pub fn apply_selection_edges(
    world: &mut World,
    observer: ActorId,
    edges: &[Action],
    frame: SelectionFrame<'_>,
) -> Result<SelectionReport, TargetingError> {
    let Some(mut targeting) = world.get_resource_mut::<TargetingSession>() else {
        return Err(TargetingError::NoSession);
    };
    let TargetingSession {
        store,
        selection,
        bindings,
        last_phase,
        ..
    } = &mut *targeting;

    let mut report = SelectionReport::default();
    for edge in edges {
        let Action::Flight(command) = edge else {
            continue;
        };
        let Some(action) = bindings.action(*command).cloned() else {
            report.ignored.push(*command);
            continue;
        };
        store.act(observer, selection, &action, frame)?;
        report.acted.push(*command);
    }

    let phase = store.phase(observer, selection, frame.now)?;
    report.selection = phase.selection;
    *last_phase = Some(phase.clone());
    report.phase = Some(phase);
    Ok(report)
}

/// The damage system's record of one tick: the hits it was handed and the
/// events it emitted for them.
#[derive(Clone, Copy, Debug)]
pub struct TargetDamageTick<'a> {
    /// The hits handed to the resolver this tick.
    pub hits: &'a [HitEvent],
    /// The resolution the resolver emitted for them.
    pub events: &'a [DamageEvent],
}

/// What one [`apply_target_damage`] pass did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TargetDamageReport {
    /// Lifecycle transitions recorded for the target store.
    pub lifecycle: usize,
    /// Lifecycle transitions this roster had nothing to record, because the
    /// damage resolver and the target roster are separate registries: an
    /// actor can be destroyed without ever having been targetable. Counted,
    /// never refused — see [`apply_target_damage`].
    pub ignored: Vec<ActorId>,
    /// The hits the resolver actually applied, which are the only ones that
    /// can evidence an attack.
    pub applied: usize,
    /// Hits the resolver refused or blocked: no attack, no threat cue.
    pub not_applied: usize,
    /// What the applied hits did to the threat ledger.
    pub feed: ThreatFeed,
}

/// Feeds one tick of the damage system's record into the target store.
///
/// Two statements, both from the resolver's own output:
///
/// * every [`DamageEventKind::Lifecycle`] is recorded through
///   [`cs_sim::targeting::TargetStore::record_lifecycle`], so a destroyed
///   actor stops being eligible and the next phase clears a selection that
///   held it (AC03's contract half). A transition naming an actor this roster
///   never listed is **counted** in [`TargetDamageReport::ignored`] instead
///   of refused: the damage resolver and the target roster are separate
///   registries, so an unregistered actor can be destroyed without anything
///   ever having been able to select it, and refusing the batch over it
///   would discard the whole tick's threat feed — every tick, for an actor
///   no consumer could see;
/// * a hit that the resolver *applied* becomes an attack, evidenced by the
///   hit's own [`HitEventId`] — while a refused or blocked hit mints
///   nothing, because an attack that did not land is not an attack
///   (non-negotiable 4). Attribution comes from the submitted hit, so a
///   hazard hit still credits nobody.
///
/// # Errors
///
/// [`TargetingError::NoSession`] when no session is installed, and
/// [`TargetError::ForeignSession`] when an event or a hit carries another
/// generation: that is a session that was not torn down, and it refuses the
/// batch rather than half-applying it. An *unknown* actor is never an error
/// here — a lifecycle transition with nothing to record is counted in
/// [`TargetDamageReport::ignored`] and a hit naming an untracked actor or
/// victim is counted in [`ThreatFeed::untracked`], exactly as
/// [`cs_sim::targeting::TargetStore::record_hits`] decides.
pub fn apply_target_damage(
    world: &mut World,
    tick: &TargetDamageTick<'_>,
) -> Result<TargetDamageReport, TargetingError> {
    let mut report = TargetDamageReport::default();
    let applied: BTreeSet<HitEventId> = tick
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            DamageEventKind::HitApplied { hit, .. } => Some(*hit),
            _ => None,
        })
        .collect();

    let landed: Vec<HitEvent> = tick
        .hits
        .iter()
        .filter(|hit| applied.contains(&hit.id))
        .cloned()
        .collect();
    report.applied = landed.len();
    report.not_applied = tick.hits.len() - landed.len();

    let Some(mut targeting) = world.get_resource_mut::<TargetingSession>() else {
        return Err(TargetingError::NoSession);
    };
    let store = targeting.store_mut();
    for event in tick.events {
        let DamageEventKind::Lifecycle { actor, kind } = &event.kind else {
            continue;
        };
        if *kind == LifecycleKind::Destroyed && store.gone(actor).is_some() {
            // Two authoritative reporters may observe the same transition;
            // recording it twice is not a second death.
            continue;
        }
        match store.record_lifecycle(*actor, *kind) {
            Ok(()) => report.lifecycle += 1,
            Err(TargetError::UnknownActor { .. }) => report.ignored.push(*actor),
            Err(error) => return Err(error.into()),
        }
    }
    report.feed = store.record_hits(&landed)?;
    Ok(report)
}
