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
//! * The F30-C consumer half: [`TargetConsumers`] and its three views —
//!   [`HudTargetReadout`] (the reticle and the threat list the HUD reads),
//!   [`SpyglassReadout`] (the target the spyglass magnifies) and
//!   [`GuidanceReadout`] (what the weapon path may offer an aid for) — plus
//!   the entry [`apply_target_consumers`] that derives all three from one
//!   phase record, and [`teardown_target_consumers`] that drops them when a
//!   session ends.
//!
//! Nothing here draws a reticle. The views are the *data* each consumer needs
//! from targeting; the HUD's glyphs, the spyglass rig and the weapon path are
//! owned by F46-B, F21-B and F27-B respectively, and a view carries no draw
//! call and no combat authority.

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
    Allegiance, AllegianceTable, CycleDirection, Reticle, SelectionAction, SelectionBinding,
    SelectionClearReason, SelectionFrame, TargetClass, TargetError, TargetFilter, TargetPhase,
    TargetPolicy, TargetRecord, TargetSelection, TargetStore, ThreatCue, ThreatFeed,
    WeaponGuidance,
};
use cs_types::Tick;
use cs_types::content::{ContentId, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::{Meters, WorldPosition};

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
    /// Where `lead_indicator` was declared. Carried so the F30-C guidance
    /// consumer can present the option's evidence class rather than a bare
    /// flag (F30 non-negotiable 3).
    pub lead_indicator_provenance: Provenance,
    /// Where `aim_assistance` was declared, for the same reason.
    pub aim_assistance_provenance: Provenance,
}

impl LoweredTargetRules {
    /// The session's two assistance options, each paired with the
    /// [`Provenance`] of its declared value.
    ///
    /// This is the only place the raw booleans become an assistance record:
    /// from here on the guidance consumer reads
    /// [`AssistanceOption::presentable`] instead of a bare flag, so the
    /// evidence classification reaches the consumer instead of being dropped
    /// at the lowering boundary.
    #[must_use]
    pub fn assistance(&self) -> AssistanceOptions {
        AssistanceOptions {
            lead_indicator: AssistanceOption {
                enabled: self.lead_indicator,
                provenance: self.lead_indicator_provenance.clone(),
            },
            aim_assistance: AssistanceOption {
                enabled: self.aim_assistance,
                provenance: self.aim_assistance_provenance.clone(),
            },
        }
    }
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
    lower_rule_with_provenance(field, value).map(|(value, _)| value)
}

/// Lowers a declared value and keeps its [`Provenance`] beside it.
///
/// The F30-C guidance consumer needs the evidence classification of each
/// assistance option, so the lowering boundary hands the pair on instead of
/// discarding it. The unknown branch is identical to [`lower_rule`]'s: an
/// unevidenced option still refuses rather than defaulting.
fn lower_rule_with_provenance<T>(
    field: &'static str,
    value: &Resolved<T>,
) -> Result<(T, Provenance), TargetLowerError>
where
    T: Clone,
{
    match value {
        Resolved::Known(known) => Ok((known.value.clone(), known.provenance.clone())),
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
    let (lead_indicator, lead_indicator_provenance) =
        lower_rule_with_provenance("lead_indicator", &rules.rules().lead_indicator)?;
    let (aim_assistance, aim_assistance_provenance) =
        lower_rule_with_provenance("aim_assistance", &rules.rules().aim_assistance)?;

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
        lead_indicator_provenance,
        aim_assistance_provenance,
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

/// One assistance option as a session holds it: the lowered value and where
/// the value came from.
///
/// The two options are separate records because they are separate features
/// (F30 non-negotiable 3). The [`Provenance`] travels with the option all the
/// way to the guidance consumer, so a consumer that draws a lead indicator or
/// an assistance cue can see that the flag is `designed` project vocabulary
/// and never present it as measured original behavior. Neither option carries
/// a magnitude: an option is on or off, and the aid itself belongs to the
/// weapon path that owns ballistics (F27-B), not to targeting.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistanceOption {
    /// The lowered declared value.
    pub enabled: bool,
    /// Where the declared value came from.
    pub provenance: Provenance,
}

impl AssistanceOption {
    /// Whether a consumer may act on this option at all: the declared value
    /// is on **and** the evidence behind it is a class that can be presented
    /// (`verified_original`, `documented`, `observed_tool` or `inferred`).
    ///
    /// A `designed`, `unknown`, `contradicted` or `synthetic_fixture` value is
    /// deliberately *not* presentable: the engine's own defaults exist to make
    /// the engine playable, and a consumer that drew them as original behavior
    /// would be making a fidelity claim no evidence supports. The bit is
    /// available on the record so a consumer can show or hide it; it is not a
    /// statement that the flag is wrong.
    #[must_use]
    pub const fn presentable(&self) -> bool {
        self.enabled
            && matches!(
                self.provenance.class,
                ClaimStatus::VerifiedOriginal
                    | ClaimStatus::Documented
                    | ClaimStatus::ObservedTool
                    | ClaimStatus::Inferred
            )
    }
}

/// The session's two assistance options, kept apart.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistanceOptions {
    /// Whether a lead indicator is offered — a display aid.
    pub lead_indicator: AssistanceOption,
    /// Whether aim assistance is offered — aim correction, an option with its
    /// own evidence, never an automatic hit correction.
    pub aim_assistance: AssistanceOption,
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
    assistance: AssistanceOptions,
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
            assistance: lowered.assistance(),
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

    /// The session's two assistance options with their declared provenance —
    /// what the F30-C guidance consumer reads to decide whether it may offer
    /// either aid.
    #[must_use]
    pub const fn assistance(&self) -> &AssistanceOptions {
        &self.assistance
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

// ------------------------------------------------------------ consumers ----

/// One threat cue the HUD view does not draw, and which one it was.
///
/// The ledger keeps a cue whose attacker left the world — the attack itself is
/// evidence, and `cs_sim::targeting` deliberately does not purge it — but a
/// warning drawn over a wreck is a lie, so the view withdraws the cue and
/// names it here rather than dropping it silently (F30 non-negotiable 4:
/// cues are for actual attacks, by actors that still exist).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WithdrawnCue {
    /// The attacker whose cue is no longer displayed.
    pub attacker: ActorId,
    /// The tick it last attacked at, as the ledger recorded it.
    pub last_attack: Tick,
}

/// One actor the consumers' previous target was and no longer is.
///
/// This is the F30-C half of AC03: `cs_sim::targeting::TargetPhase` reports
/// that the selection went and why, and every view carries it, so a consumer
/// that had the actor framed learns it in the same read that tells it there is
/// nothing to frame. A consumer that only saw `target: None` could not tell a
/// cleared target from one that was never there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClearedTarget {
    /// The actor the observer had selected.
    pub actor: ActorId,
    /// Why it stopped being eligible.
    pub reason: SelectionClearReason,
}

/// The HUD's target readout for one phase: the reticle it draws and the
/// threat cues it warns about.
///
/// `reticle` is the phase's own [`cs_sim::targeting::Reticle`], so the HUD
/// draws exactly the record the AI's hostility gate was decided from. It is a
/// copy: mutating it changes nothing in the store, and a HUD that wants to
/// *select* something goes through [`apply_selection_edges`], never through
/// this view.
#[derive(Clone, Debug, PartialEq)]
pub struct HudTargetReadout {
    /// The tick this readout was derived at.
    pub at: Tick,
    /// The reticle record, when a target is selected.
    pub reticle: Option<Reticle>,
    /// The live threat cues whose attackers are still in the world, in the
    /// ledger's own order (most recent attack first).
    pub threats: Vec<ThreatCue>,
    /// Cues the ledger still holds whose attackers are no longer in the
    /// world: reported, never drawn.
    pub withdrawn: Vec<WithdrawnCue>,
    /// The selection this readout cleared, when it cleared one.
    pub cleared: Option<ClearedTarget>,
}

impl HudTargetReadout {
    /// Whether this readout warns about anything.
    #[must_use]
    pub fn is_threatening(&self) -> bool {
        !self.threats.is_empty()
    }
}

/// What the spyglass would magnify for one phase.
///
/// The spyglass is a *view*, and its target is the session's selection: it
/// never picks one. That is why the record carries
/// [`cleared`](Self::cleared) — a rig that had the actor framed must be told
/// the actor went away in the same read that says there is nothing to frame,
/// so it cannot leave the last magnification up (AC03).
#[derive(Clone, Debug, PartialEq)]
pub struct SpyglassReadout {
    /// The tick this readout was derived at.
    pub at: Tick,
    /// The actor the spyglass would magnify; `None` when nothing is selected
    /// or the selected actor stopped being eligible.
    pub target: Option<SpyglassTarget>,
    /// The selection this readout cleared, when it cleared one.
    pub cleared: Option<ClearedTarget>,
}

impl SpyglassReadout {
    /// Whether there is anything to frame.
    #[must_use]
    pub fn has_target(&self) -> bool {
        self.target.is_some()
    }
}

/// The target the spyglass frames: its canonical position and its live
/// classification.
///
/// `position` is canonical f64 world space — the same value the ordering
/// ranked on — so the rig's magnification is computed from the same point and
/// a world rebase cannot shift it (F30 non-negotiable 5). `hostile` is the
/// reticle's own verdict, not a second read of the allegiance table, so the
/// view cannot disagree with the AI gate.
#[derive(Clone, Debug, PartialEq)]
pub struct SpyglassTarget {
    /// The selected actor.
    pub actor: ActorId,
    /// What kind of actor it is.
    pub class: TargetClass,
    /// The declared relation to the observer's faction, re-derived this phase.
    pub allegiance: Option<Allegiance>,
    /// Whether the combat AI may engage: a declared hostile relation.
    pub hostile: bool,
    /// Whether this actor attacked the observer inside the threat window.
    pub threatening: bool,
    /// Whether mission rules flag the actor as an objective target.
    pub objective: bool,
    /// The actor's canonical world position.
    pub position: WorldPosition,
    /// Its distance from the observer.
    pub distance: Meters,
}

/// One declared assistance option as a consumer sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistanceOffer {
    /// The declared option's lowered value.
    pub enabled: bool,
    /// Whether a consumer may act on it — see
    /// [`AssistanceOption::presentable`]. A designed default is not
    /// presentable, so the engine's own flag can never be drawn as original
    /// behavior.
    pub presentable: bool,
    /// Where the declared value came from.
    pub provenance: Provenance,
}

impl AssistanceOffer {
    /// Whether this option is on **and** a consumer may act on it.
    #[must_use]
    pub const fn offered(&self) -> bool {
        self.enabled && self.presentable
    }
}

/// Why no aid is offered this phase.
///
/// Named rather than collapsed into an empty aid: "no target", "the declared
/// options do not cover this" and "the target is not one we assist against"
/// are different statements, and a weapon path that cannot tell them apart
/// defaults to firing unassisted at a target it never had.
#[derive(Clone, Debug, PartialEq)]
pub enum GuidanceWithheld {
    /// Nothing is selected, so there is no target to aid.
    NoTarget,
    /// A target is selected, but it is not a *declared* hostile: friendly,
    /// neutral or an undeclared pair. An aid toward an ally is never offered,
    /// and an undeclared pair is never treated as hostile for the purpose of
    /// offering one — that gate is the store's, so no consumer can widen it.
    NotHostile,
    /// The store refused the guidance query — the observer is unregistered, or
    /// the selected actor sits on the observer and has no bearing. Carried
    /// with the store's own message so the reason is never lost.
    Refused {
        /// The store's refusal, rendered.
        reason: String,
    },
}

/// The weapon path's readout for one phase: the aid it may offer, if any.
///
/// This is the **eligibility** record, not the aid. The two declared options
/// stay separate (F30 non-negotiable 3), each carries its own [`Provenance`]
/// so a consumer knows whether the option behind it is measured or designed,
/// and neither carries a magnitude: a lead point or an aim correction belongs
/// to the weapon path that owns ballistics (F27-B), and the original assistance
/// behavior is unmeasured (F30-D). Nothing in this record can change a shot.
#[derive(Clone, Debug, PartialEq)]
pub struct GuidanceReadout {
    /// The tick this readout was derived at.
    pub at: Tick,
    /// The aid the weapon path may offer, derived by the store from the same
    /// selection the reticle describes. `None` when the store **refused** the
    /// query — nothing is selected, or the selected actor is not a declared
    /// hostile, or the query could not be answered — and read
    /// [`withheld`](Self::withheld) to learn which. It is `None` never because a
    /// declared option is off: the two options are separate records below, and
    /// [`offers_lead_indicator`](Self::offers_lead_indicator) and
    /// [`offers_aim_assistance`](Self::offers_aim_assistance) are what join
    /// "the option is on and presentable" to "there is a target to apply it to".
    pub aid: Option<WeaponGuidance>,
    /// The lead-indicator option as it stands.
    pub lead_indicator: AssistanceOffer,
    /// The aim-assistance option, the same three fields.
    pub aim_assistance: AssistanceOffer,
    /// Why no aid is offered, when none is.
    pub withheld: Option<GuidanceWithheld>,
    /// The selection this readout cleared, when it cleared one.
    pub cleared: Option<ClearedTarget>,
}

impl GuidanceReadout {
    /// Whether an aid is offered at all this phase.
    #[must_use]
    pub fn has_aid(&self) -> bool {
        self.aid.is_some()
    }

    /// Whether the declared lead-indicator option is on, presentable and has a
    /// target to apply to.
    #[must_use]
    pub fn offers_lead_indicator(&self) -> bool {
        self.lead_indicator.offered() && self.aid.is_some()
    }

    /// Whether the declared aim-assistance option is on, presentable and has a
    /// target to apply to.
    #[must_use]
    pub fn offers_aim_assistance(&self) -> bool {
        self.aim_assistance.offered() && self.aid.is_some()
    }
}

/// The three consumer views a session publishes, as one record.
///
/// They are **one record derived together**, not three independent queries,
/// because the failure they share is disagreement: a reticle that says hostile
/// while the weapon path sees friendly, or a spyglass framing a target the
/// reticle has already dropped. [`apply_target_consumers`] is the only writer,
/// and it publishes all three or none.
///
/// The binding is the `(session, observer)` the views describe. A view read
/// without a binding cannot be attributed to a session, which is why a rebound
/// or cleared pass drops the binding rather than leaving a stale pairing.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct TargetConsumers {
    bound: Option<ConsumerBinding>,
    hud: Option<HudTargetReadout>,
    spyglass: Option<SpyglassReadout>,
    guidance: Option<GuidanceReadout>,
}

/// The session generation and observer the published views describe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumerBinding {
    /// The session generation the views were derived in.
    pub session: SessionId,
    /// The observer they describe.
    pub observer: ActorId,
}

impl TargetConsumers {
    /// The `(session, observer)` the published views belong to; `None` when
    /// nothing is published.
    #[must_use]
    pub const fn bound(&self) -> Option<ConsumerBinding> {
        self.bound
    }

    /// The HUD's target readout, when one is published.
    #[must_use]
    pub const fn hud(&self) -> Option<&HudTargetReadout> {
        self.hud.as_ref()
    }

    /// The spyglass's readout, when one is published.
    #[must_use]
    pub const fn spyglass(&self) -> Option<&SpyglassReadout> {
        self.spyglass.as_ref()
    }

    /// The weapon path's readout, when one is published.
    #[must_use]
    pub const fn guidance(&self) -> Option<&GuidanceReadout> {
        self.guidance.as_ref()
    }

    /// Drops every published view and the binding, so no consumer can read a
    /// record belonging to a session or observer that no longer exists.
    pub fn clear(&mut self) {
        self.bound = None;
        self.hud = None;
        self.spyglass = None;
        self.guidance = None;
    }
}

/// What one [`apply_target_consumers`] pass published.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConsumerReport {
    /// The HUD view published.
    pub hud: Option<HudTargetReadout>,
    /// The spyglass view published.
    pub spyglass: Option<SpyglassReadout>,
    /// The weapon-guidance view published.
    pub guidance: Option<GuidanceReadout>,
    /// The views were rebound to a different `(session, observer)` this pass,
    /// so the previous binding's target was dropped rather than carried over.
    pub rebound: bool,
}

/// Derives the HUD, spyglass and weapon-guidance views from one phase record.
///
/// This is the F30-C consumer entry, and it exists separately from
/// [`apply_selection_edges`] for one reason: it derives its **own**
/// [`cs_sim::targeting::TargetPhase`] at `at` instead of reading the selection
/// pass's record. Consumers render after the tick's damage and roster work, so
/// a target destroyed between the selection pass and the render must not be
/// described by a record that predates its own death (AC03).
///
/// Everything else follows from deriving once: the reticle the HUD draws, the
/// target the spyglass frames and the aid the weapon path may offer are three
/// views of **one** read of the roster, so this design cannot produce a HUD
/// that says hostile while the weapon path sees friendly.
///
/// Teardown, rebinding and error propagation:
///
/// * the views are bound to `(session, observer)`; a pass for a different pair
///   rebinds and reports [`ConsumerReport::rebound`], so neither an aircraft
///   swap nor a replaced session can carry a target box across generations;
/// * a pass that cannot derive a record publishes nothing and unbinds instead
///   of leaving the previous pass's views up, so a failed render shows no
///   target rather than a stale one, and the next successful pass republishes
///   from scratch — the retry path is the same code as the first pass;
/// * [`teardown_target_consumers`] is the explicit end-of-session path and
///   refuses to clear views bound to a *different* session.
///
/// # Errors
///
/// [`TargetingError::NoSession`] when no [`TargetingSession`] is installed,
/// and [`TargetingError::Store`] (carrying [`TargetError::UnknownActor`]) when
/// `observer` is not registered with this session's store. In both cases the
/// published views are unbound before the error is returned, including the
/// `NoSession` case where the views outlive the session resource they were
/// derived from.
pub fn apply_target_consumers(
    world: &mut World,
    observer: ActorId,
    at: Tick,
) -> Result<ConsumerReport, TargetingError> {
    // Read the previous binding before the mutable borrow: a pass for a
    // different `(session, observer)` rebinds, and a rebind must drop the
    // previous binding's target rather than carry it across a generation.
    //
    // Comparing the observer alone *is* the pair comparison, because an
    // [`ActorId`] is generation-qualified: a session replaced under the same
    // serial is a different actor id and rebinds like an aircraft swap. A pass
    // with no [`TargetingSession`] at all is not a rebind — it refuses, and the
    // refusal unbinds the previous session's views.
    let rebound = world
        .get_resource::<TargetConsumers>()
        .and_then(TargetConsumers::bound)
        .is_some_and(|bound| bound.observer != observer);

    // One read of the store produces the phase the reticle, the spyglass and
    // the guidance all come from, and the guidance query reads the same
    // selection again immediately after — so the aid can never name a target
    // the reticle has already dropped.
    //
    // Both refusals — no session at all, and a store that will not describe the
    // observer — are collected here rather than returned from inside the borrow,
    // so the single unbind below covers them both: the views already published
    // belong to a session that cannot describe this observer any more, and a
    // consumer reading the resource after either error must find no target
    // rather than the previous pass's.
    let derived = match world.get_resource_mut::<TargetingSession>() {
        None => Err(TargetingError::NoSession),
        Some(mut targeting) => {
            let session = targeting.session();
            let assistance = targeting.assistance().clone();
            let TargetingSession {
                store,
                selection,
                last_phase,
                ..
            } = &mut *targeting;
            if rebound && store.is_registered(&observer) {
                // The held selection is the previous observer's, so it goes
                // with the binding. A new aircraft starts with no target rather
                // than with the last pilot's. The session's [`TargetSelection`]
                // is one selection for the whole session, so a pass for a
                // different observer is an aircraft swap: the target box on
                // screen belongs to the aircraft that is gone, and carrying it
                // would show one pilot another's target (the
                // `STATE-TRANSACTIONS` rule that a previous aircraft's state
                // never survives the swap).
                //
                // The `is_registered` guard matters: a rebind to an observer
                // the store does not hold is refused by the phase below, and a
                // failed pass must not have mutated the session on its way out.
                selection.clear();
            }
            let result = store.phase(observer, selection, at).map(|phase| {
                // The guidance query reads the same selection again
                // immediately after the phase, so the aid can never name a
                // target the reticle has already dropped. Its refusals are the
                // interesting part, not failures: `NotHostile` is the gate
                // working, and it is carried into `withheld` rather than
                // thrown.
                let (aid, refused) = match store.guidance(observer, selection, at) {
                    Ok(aid) => (Some(aid), None),
                    Err(error) => (None, Some(error)),
                };
                let (threats, withdrawn) = split_threats(store, &phase.threats);
                (phase, aid, refused, threats, withdrawn)
            });
            match result {
                Ok(derived) => {
                    *last_phase = Some(derived.0.clone());
                    Ok((session, assistance, derived))
                }
                Err(error) => {
                    *last_phase = None;
                    Err(TargetingError::Store(error))
                }
            }
        }
    };
    let (session, assistance, (phase, aid, refused, threats, withdrawn)) = match derived {
        Ok(derived) => derived,
        Err(error) => {
            // A failed pass publishes nothing: unbind before reporting, so a
            // consumer reading the resource after the error finds no target
            // rather than the previous pass's. The next successful pass
            // republishes — the retry path is the same code as the first pass.
            unbind_consumers(world);
            return Err(error);
        }
    };
    let cleared = phase.cleared.map(|cleared| ClearedTarget {
        actor: cleared.actor,
        reason: cleared.reason,
    });

    let offer = |option: &AssistanceOption| AssistanceOffer {
        enabled: option.enabled,
        presentable: option.presentable(),
        provenance: option.provenance.clone(),
    };
    let lead_indicator = offer(&assistance.lead_indicator);
    let aim_assistance = offer(&assistance.aim_assistance);

    let hud = HudTargetReadout {
        at,
        reticle: phase.reticle.clone(),
        threats,
        withdrawn,
        cleared,
    };
    let spyglass = SpyglassReadout {
        at,
        target: phase.reticle.as_ref().map(|reticle| SpyglassTarget {
            actor: reticle.target,
            class: reticle.class,
            allegiance: reticle.allegiance,
            hostile: reticle.hostile,
            threatening: reticle.threatening,
            objective: reticle.objective,
            position: reticle.position,
            distance: reticle.distance,
        }),
        cleared,
    };
    // The store's own refusal is the first thing reported, because it is the
    // roster speaking: a selected actor that is not a declared hostile is
    // `NotHostile`, and no declared option changes that.
    let withheld = match (&aid, &refused) {
        (Some(_), _) => None,
        (None, Some(TargetError::NotHostile { .. })) => Some(GuidanceWithheld::NotHostile),
        (None, _) if phase.reticle.is_none() => Some(GuidanceWithheld::NoTarget),
        // Any other refusal — a degenerate bearing, an orphaned selection — is
        // a broken query rather than a decision about the target, and it is
        // carried verbatim so the weapon path can report it.
        (None, Some(error)) => Some(GuidanceWithheld::Refused {
            reason: error.to_string(),
        }),
        // Unreachable while `refused` is `Some` exactly when `aid` is `None`,
        // and kept so that a future change to the query reports a missing
        // answer as a refusal rather than quietly as "no aid".
        (None, None) => Some(GuidanceWithheld::Refused {
            reason: "the store answered no guidance query".to_owned(),
        }),
    };
    let guidance = GuidanceReadout {
        at,
        aid,
        lead_indicator,
        aim_assistance,
        withheld,
        cleared,
    };

    let report = ConsumerReport {
        hud: Some(hud.clone()),
        spyglass: Some(spyglass.clone()),
        guidance: Some(guidance.clone()),
        rebound,
    };
    world.insert_resource(TargetConsumers {
        bound: Some(ConsumerBinding { session, observer }),
        hud: Some(hud),
        spyglass: Some(spyglass),
        guidance: Some(guidance),
    });
    Ok(report)
}

/// Splits the phase's threat cues into the ones a HUD may draw and the ones it
/// may not, in the ledger's own order.
///
/// The split is [`TargetStore::present`], not
/// [`TargetStore::eligible`]: an attacker that lost sensor contact or whose
/// script phase closed is still flying and still dangerous, and a warning is
/// exactly what an unseen attacker is worth. Only an attacker that left the
/// world — destroyed, despawned, removed from mission accounting, or
/// unregistered entirely — is withdrawn. Both halves are reported, so a cue is
/// never silently lost and the ledger keeps the evidence either way.
fn split_threats(store: &TargetStore, cues: &[ThreatCue]) -> (Vec<ThreatCue>, Vec<WithdrawnCue>) {
    let mut live = Vec::with_capacity(cues.len());
    let mut withdrawn = Vec::new();
    for cue in cues {
        if store.present(&cue.attacker) {
            live.push(*cue);
        } else {
            withdrawn.push(WithdrawnCue {
                attacker: cue.attacker,
                last_attack: cue.last_attack,
            });
        }
    }
    (live, withdrawn)
}

/// Unbinds the published views, installing an empty resource if none exists,
/// so a reader never finds a resource it cannot tell from a live one.
fn unbind_consumers(world: &mut World) {
    match world.get_resource_mut::<TargetConsumers>() {
        Some(mut consumers) => consumers.clear(),
        None => {
            world.insert_resource(TargetConsumers::default());
        }
    }
}

/// Drops the published HUD, spyglass and guidance views for `session`.
///
/// This is the explicit end-of-session path: a caller that knows a generation
/// is over — a mission restart, an aircraft swap, a disconnect — calls it so
/// no consumer reads a record from a store that no longer exists. It refuses
/// to clear views bound to a **different** session, so a late teardown for the
/// previous generation cannot blank the current one, and reports whether it
/// cleared anything.
pub fn teardown_target_consumers(world: &mut World, session: SessionId) -> bool {
    let Some(mut consumers) = world.get_resource_mut::<TargetConsumers>() else {
        return false;
    };
    if consumers
        .bound()
        .is_some_and(|bound| bound.session != session)
    {
        return false;
    }
    if consumers.bound().is_none() {
        // Nothing is published for this session (or for any), so there is
        // nothing to clear and the caller is told so rather than being given a
        // teardown that appears to have done work.
        return false;
    }
    consumers.clear();
    true
}
