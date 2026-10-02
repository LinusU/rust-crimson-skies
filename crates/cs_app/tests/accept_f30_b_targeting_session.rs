//! Acceptance scenario F30-B: the ECS production path — the session
//! resource, the roster sync from bound entities, the command edges and the
//! damage tick.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-B`. Task test prefix: `accept_f30_b_`.
//!
//! These tests drive production code only: [`cs_app::targeting`]'s
//! [`lower_selection_actions`], [`TargetingSession`], [`TargetableBinding`],
//! [`TargetableState`], [`sync_targetable_roster`],
//! [`apply_selection_edges`] and [`apply_target_damage`], over a real
//! `bevy::world::World`. The damage half is driven by the **real**
//! [`DamageResolver`] resolving the fixture airframe, so the events the
//! entry reads are the events the damage system emits — including the
//! refusal for a hit that named a node the graph does not have.
//!
//! The minimum scenario is AC02 at this layer: a capture written on an
//! entity's record reaches the reticle and the AI hostility gate in the same
//! phase the pass derived.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use std::collections::BTreeSet;

use bevy::ecs::world::World;
use cs_app::scene::SceneGeneration;
use cs_app::targeting::{
    TargetDamageTick, TargetableBinding, TargetableState, TargetingError, TargetingSession,
    apply_selection_edges, apply_target_damage, lower_rules, lower_selection_actions,
    sync_targetable_roster,
};
use cs_content::target_rules::{
    DeclaredAction, DeclaredSelectionAction, DeclaredSelectionActions,
    declared_synthetic_selection_actions, declared_synthetic_target_rules,
};
use cs_sim::ai::combat::{
    CandidateVerdict, CombatRequest, CombatRole, RejectReason, RoleAssignment, synthetic_arsenal,
    synthetic_combat_planner,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageEventKind, DamageNodeKey, DamagePolicy,
    DamageResolver, HitEvent, HitEventId, RefusalReason, SYNTHETIC_HULL_INTEGRITY,
    SYNTHETIC_HULL_NODE, SYNTHETIC_MOUNT_NODE, synthetic_airframe_graph,
};
use cs_sim::targeting::{
    Allegiance, CycleDirection, SelectionAction, SelectionFrame, TargetClass, TargetFilter,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

/// The session generation the roster, the damage resolver and the combat
/// planner all share: the combat fixture's own session.
const SESSION: u64 = 7;
/// The resolver's producer serial.
const PRODUCER: u32 = 3;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION,
        serial,
    }
}

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn pos(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

fn claim() -> ClaimId {
    ClaimId::new("f30b.session-test").expect("a valid claim id")
}

/// The fixture's actors as targeting records: the player at the origin, the
/// three equal-distance raiders, the wingman and the objective trader.
fn roster() -> Vec<(u64, ContentId, TargetClass, bool, [f64; 3])> {
    let player = cs_sim::targeting::synthetic_player_faction();
    let raiders = cs_sim::targeting::synthetic_raider_faction();
    let traders = cs_sim::targeting::synthetic_trader_faction();
    vec![
        (
            1,
            player.clone(),
            TargetClass::Aircraft,
            false,
            [0.0, 0.0, 0.0],
        ),
        (
            9,
            raiders.clone(),
            TargetClass::Aircraft,
            false,
            [100.0, 0.0, 0.0],
        ),
        (
            2,
            raiders.clone(),
            TargetClass::Aircraft,
            false,
            [0.0, 100.0, 0.0],
        ),
        (5, raiders, TargetClass::Aircraft, false, [0.0, 0.0, -100.0]),
        (3, player, TargetClass::Aircraft, false, [50.0, 0.0, 0.0]),
        (4, traders, TargetClass::WorldObject, true, [0.0, 60.0, 0.0]),
    ]
}

/// A world with the session resource installed and every fixture actor
/// bound to an entity, under one scene generation.
fn bound_world(generation: SceneGeneration) -> World {
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let bindings = lower_selection_actions(&declared_synthetic_selection_actions())
        .expect("the fixture actions lower");
    let mut world = World::new();
    world.insert_resource(TargetingSession::new(
        session_id(),
        lowered,
        bindings,
        declared.subject().clone(),
        generation,
    ));
    for (serial, faction, class, objective, position) in roster() {
        let mut state = TargetableState::aircraft(faction, pos(position));
        state.class = class;
        state.objective = objective;
        world.spawn((
            TargetableBinding {
                actor: actor(serial),
                rules: declared.subject().clone(),
                generation,
            },
            state,
        ));
    }
    world
}

fn edges(commands: &[FlightCommand]) -> Vec<Action> {
    commands.iter().copied().map(Action::Flight).collect()
}

/// The entity presenting one actor's binding. The binding is the ECS's
/// actor-to-entity mapping, so the test looks the entity up the same way a
/// session system would rather than keeping its own table.
fn entity_of(world: &mut World, actor: ActorId) -> bevy::ecs::entity::Entity {
    let mut query = world.query::<(bevy::ecs::entity::Entity, &TargetableBinding)>();
    query
        .iter(world)
        .find(|(_, binding)| binding.actor == actor)
        .map(|(entity, _)| entity)
        .expect("the actor is bound to an entity")
}

/// The AI's verdict on the actor a reticle describes, decided by the real
/// combat planner from the reticle's own allegiance.
fn ai_verdict(reticle: &cs_sim::targeting::Reticle) -> CandidateVerdict {
    let planner = synthetic_combat_planner();
    let assignment = RoleAssignment::new(actor(1), CombatRole::FighterAttack);
    let arsenal = synthetic_arsenal();
    let candidate = cs_sim::ai::combat::CandidateView::new(
        reticle.target,
        reticle.position,
        reticle.allegiance,
        reticle.objective,
    );
    planner
        .decide(&CombatRequest {
            observer: actor(1),
            now: Tick(20),
            observer_position: pos([0.0, 0.0, 0.0]),
            assignment: &assignment,
            formation: None,
            protected_alive: None,
            candidates: &[candidate],
            arsenal: Some(&arsenal),
            profile: None,
        })
        .expect("the request is well-formed")
        .trace
        .candidate(reticle.target)
        .expect("the candidate is in the trace")
        .verdict
}

/// The declared table lowers into the runtime command-edge table: the two
/// cycle edges and the nearest-attacker binding reach the store's binding,
/// and a command the preset does not bind stays a non-target command.
#[test]
fn accept_f30_b_declared_actions_lower_into_the_command_edge_table() {
    let binding = lower_selection_actions(&declared_synthetic_selection_actions())
        .expect("the fixture actions lower");
    assert_eq!(binding.len(), 3);
    assert_eq!(
        binding.action(FlightCommand::TargetNext),
        Some(&SelectionAction::Cycle {
            direction: CycleDirection::Next,
            filter: TargetFilter::Allegiance(Allegiance::Hostile)
        })
    );
    assert_eq!(
        binding.action(FlightCommand::TargetPrev),
        Some(&SelectionAction::Cycle {
            direction: CycleDirection::Previous,
            filter: TargetFilter::Allegiance(Allegiance::Hostile)
        })
    );
    assert_eq!(
        binding.action(FlightCommand::CycleWeapon),
        Some(&SelectionAction::NearestAttacker),
        "a non-target key may still carry a target action when the preset says so"
    );
    assert!(binding.action(FlightCommand::TargetNext).is_some());
    assert!(
        binding.action(FlightCommand::FirePrimary).is_none(),
        "an unbound edge is not a targeting command"
    );

    // The declared action vocabulary reaches every action the sheet names.
    let declared = |command: FlightCommand, action: DeclaredAction| DeclaredSelectionAction {
        command,
        action: Resolved::Known(cs_types::content::Known::new(
            action,
            Provenance::designed(claim()),
        )),
        evidence: Provenance::designed(claim()),
    };
    // One action per *discrete* command: a target action on an axis is
    // refused by the table, which the content test covers.
    let discrete: Vec<FlightCommand> = FlightCommand::ALL
        .iter()
        .copied()
        .filter(|command| !command.is_continuous())
        .collect();
    assert!(
        discrete.len() >= DeclaredAction::ALL.len(),
        "there are enough discrete commands for the whole vocabulary"
    );
    let table = DeclaredSelectionActions::try_new(
        ContentId::from_source(ContentKind::IaPreset, "synthetic.every-action")
            .expect("a valid subject id"),
        Origin::SyntheticFixture,
        DeclaredAction::ALL
            .iter()
            .enumerate()
            .map(|(index, action)| declared(discrete[index], *action))
            .collect(),
        Provenance::designed(claim()),
    )
    .expect("one action per discrete command is a valid table");
    let binding = lower_selection_actions(&table).expect("every declared action lowers");
    assert_eq!(binding.len(), DeclaredAction::ALL.len());
    for (index, action) in DeclaredAction::ALL.iter().enumerate() {
        assert!(
            binding.action(discrete[index]).is_some(),
            "every declared action reached the runtime table on its own command \
             ({action} -> {})",
            discrete[index]
        );
    }
    assert_eq!(
        binding.action(discrete[7]),
        Some(&SelectionAction::UnderCrosshair),
        "the under-crosshair action lowered as declared"
    );
    assert_eq!(
        binding.action(discrete[8]),
        Some(&SelectionAction::Clear),
        "and the clear action with it"
    );
}

/// The whole producer path: the roster comes from the bound entities, the
/// command edges walk the declared cycle, and the phase record the session
/// publishes is the one the reticle and the AI read. A non-target edge
/// changes no selection, and a second edge in the same frame advances the
/// cycle — a key press, not a coalesced one.
#[test]
fn accept_f30_b_session_runs_edges_and_publishes_the_phase_record() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);

    let report = sync_targetable_roster(&mut world);
    assert_eq!(report.registered, 6, "every bound entity is registered");
    assert_eq!(report.updated, 0);
    assert_eq!(report.removed, 0);
    assert!(report.ignored.is_empty());
    assert!(report.incomplete.is_empty());
    assert_eq!(
        world
            .resource::<TargetingSession>()
            .store()
            .registered()
            .len(),
        6,
        "the store holds exactly the bound entities"
    );

    let report = apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext, FlightCommand::TargetNext]),
        SelectionFrame::at(Tick(20)),
    )
    .expect("the observer is registered");
    assert_eq!(report.acted, vec![FlightCommand::TargetNext; 2]);
    assert!(report.ignored.is_empty());
    assert_eq!(
        report.selection,
        Some(actor(5)),
        "two presses walked the equal-distance cycle 2, 5"
    );

    let phase = world
        .resource::<TargetingSession>()
        .last_phase()
        .cloned()
        .expect("the pass published a phase record");
    assert_eq!(phase.at, Tick(20));
    assert_eq!(phase.selection, Some(actor(5)));
    let reticle = phase.reticle.expect("a selected target has a reticle");
    assert_eq!(reticle.allegiance, Some(Allegiance::Hostile));
    assert!(reticle.hostile);
    assert_eq!(reticle.class, TargetClass::Aircraft);
    assert_eq!(
        ai_verdict(&reticle),
        CandidateVerdict::Eligible,
        "the AI gate reads the reticle the HUD draws"
    );

    // A non-target edge is reported and changes nothing.
    let report = apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::FirePrimary]),
        SelectionFrame::at(Tick(21)),
    )
    .expect("the observer is registered");
    assert!(report.acted.is_empty());
    assert_eq!(report.ignored, vec![FlightCommand::FirePrimary]);
    assert_eq!(
        report.selection,
        Some(actor(5)),
        "the selection is untouched"
    );
    assert_eq!(
        report.phase.expect("a phase record").at,
        Tick(21),
        "and the phase is still derived, so consumers read a current record"
    );

    // An unregistered observer is refused by name, and no phase record is
    // written for it.
    assert_eq!(
        apply_selection_edges(
            &mut world,
            actor(42),
            &edges(&[FlightCommand::TargetNext]),
            SelectionFrame::at(Tick(22)),
        ),
        Err(TargetingError::Store(
            cs_sim::targeting::TargetError::UnknownActor { actor: actor(42) }
        ))
    );
    assert_eq!(
        world
            .resource::<TargetingSession>()
            .last_phase()
            .expect("the earlier record stands")
            .at,
        Tick(21),
        "a refused pass does not overwrite the published record"
    );

    // The under-crosshair action refuses a frame with no ray, and the
    // selection is left as it was.
    let binding = cs_sim::targeting::SelectionBinding::new();
    let _ = binding;
    let mut session = world
        .remove_resource::<TargetingSession>()
        .expect("installed");
    session
        .bindings_mut()
        .bind(FlightCommand::DropOrdnance, SelectionAction::UnderCrosshair);
    world.insert_resource(session);
    assert!(matches!(
        apply_selection_edges(
            &mut world,
            actor(1),
            &edges(&[FlightCommand::DropOrdnance]),
            SelectionFrame::at(Tick(23)),
        ),
        Err(TargetingError::Store(
            cs_sim::targeting::TargetError::MissingCrosshair
        ))
    ));
    assert_eq!(
        world.resource::<TargetingSession>().selection().current(),
        Some(actor(5))
    );
}

/// AC02 at the ECS layer: a capture written on an entity's record reaches
/// the reticle and the AI hostility gate in the very phase the sync and the
/// edge pass derived. The consumer never reads the store between the two.
#[test]
fn accept_f30_b_capture_on_an_entity_reclassifies_the_same_phase() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    let before = world
        .resource::<TargetingSession>()
        .last_phase()
        .cloned()
        .expect("a phase record");
    let reticle = before.reticle.expect("a reticle");
    assert_eq!(reticle.target, actor(2));
    assert!(reticle.hostile);
    assert_eq!(ai_verdict(&reticle), CandidateVerdict::Eligible);

    // The mission writes the capture on the entity's own record.
    let captured = entity_of(&mut world, actor(2));
    world
        .get_mut::<TargetableState>(captured)
        .expect("the actor is bound")
        .faction = cs_sim::targeting::synthetic_player_faction();

    let report = sync_targetable_roster(&mut world);
    assert_eq!(report.updated, 6, "every bound entity was re-read");
    assert_eq!(report.registered, 0);
    // No edge this pass: the phase still updates, because the pass derives
    // it whether or not a key was pressed.
    let report = apply_selection_edges(&mut world, actor(1), &[], SelectionFrame::at(Tick(21)))
        .expect("registered");
    assert!(report.acted.is_empty());
    let after = report.phase.expect("a phase record");
    let reticle = after.reticle.expect("the selection survived the capture");
    assert_eq!(reticle.target, actor(2));
    assert_eq!(
        reticle.allegiance,
        Some(Allegiance::Friendly),
        "the capture reached the reticle in the same phase"
    );
    assert!(!reticle.hostile);
    assert_eq!(
        ai_verdict(&reticle),
        CandidateVerdict::Rejected(RejectReason::NotHostile {
            allegiance: Allegiance::Friendly
        }),
        "and the AI gate refuses the very actor the reticle now calls a friend"
    );
}

/// The roster sync is a transaction with three outcomes: an entity that
/// left the world is unregistered, a binding from another scene generation
/// is ignored, and a binding with no record is reported rather than
/// registered with a guessed position.
#[test]
fn accept_f30_b_roster_sync_ignores_stale_and_incomplete_bindings() {
    let generation = SceneGeneration::default().next();
    let stale = SceneGeneration::default();
    let mut world = bound_world(generation);

    // A leftover binding from the previous scene generation, and a binding
    // whose record never arrived.
    world.spawn((
        TargetableBinding {
            actor: actor(2),
            rules: declared_synthetic_target_rules().subject().clone(),
            generation: stale,
        },
        TargetableState::aircraft(
            cs_sim::targeting::synthetic_raider_faction(),
            pos([500.0, 0.0, 0.0]),
        ),
    ));
    let incomplete = world
        .spawn(TargetableBinding {
            actor: actor(7),
            rules: declared_synthetic_target_rules().subject().clone(),
            generation,
        })
        .id();

    let report = sync_targetable_roster(&mut world);
    assert_eq!(report.registered, 6);
    assert_eq!(
        report.ignored,
        vec![actor(2)],
        "the stale generation is ignored"
    );
    assert_eq!(report.incomplete, vec![incomplete]);
    assert_eq!(
        world
            .resource::<TargetingSession>()
            .store()
            .record(&actor(2))
            .expect("the live entity's actor is registered")
            .position
            .to_array(),
        [0.0, 100.0, 0.0],
        "the ignored stale binding did not overwrite the live actor's record"
    );

    // The record arrives: the entity joins the roster as a seventh actor.
    world
        .entity_mut(incomplete)
        .insert(TargetableState::aircraft(
            cs_sim::targeting::synthetic_raider_faction(),
            pos([10.0, 0.0, 0.0]),
        ));
    let report = sync_targetable_roster(&mut world);
    assert_eq!(report.registered, 1);
    assert!(report.incomplete.is_empty());

    // The entity leaves the world: the actor is unregistered, so it can no
    // longer be selected or cycled through.
    let despawned = entity_of(&mut world, actor(5));
    world.entity_mut(despawned).despawn();
    let report = sync_targetable_roster(&mut world);
    assert_eq!(report.removed, 1);
    assert!(
        !world
            .resource::<TargetingSession>()
            .store()
            .is_registered(&actor(5))
    );
}

/// The threat state's real producer edge: the damage system's own tick. A
/// landed, attributable hit becomes an attack; a hit the resolver *refused*
/// (it named a node the graph does not have) mints nothing, and the
/// destruction the resolver emits ends targetability, which clears a
/// selection in the next phase.
#[test]
fn accept_f30_b_damage_tick_feeds_threats_and_lifecycle() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);

    let mut resolver = DamageResolver::new(SESSION, PRODUCER);
    for serial in [1_u64, 2, 5, 9] {
        resolver
            .register_actor(
                actor(serial),
                synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("the actor registers in the resolver's session");
    }

    let mount = DamageNodeKey::new(SYNTHETIC_MOUNT_NODE).expect("a valid node key");
    // The hull is the fixture airframe's only lethal node, so only a hit
    // that depletes it destroys the actor — the destruction under test is
    // the resolver's own, not a record the targeting side writes.
    let hull = DamageNodeKey::new(SYNTHETIC_HULL_NODE).expect("a valid node key");
    let ghost = DamageNodeKey::new("no_such_node").expect("a valid node key");
    let hit = |sequence: u32,
               node: &DamageNodeKey,
               attacker: ActorId,
               victim: ActorId,
               damage,
               tick: u64| {
        HitEvent::try_new(
            HitEventId {
                session: SESSION,
                tick: Tick(tick),
                producer: PRODUCER,
                sequence,
            },
            Some(attacker),
            victim,
            node.clone(),
            DamageChannel::Internal,
            damage,
        )
        .expect("the fixture hit is well-formed")
    };
    // Raider 5 shoots the player; a second hit names a node the airframe
    // graph does not have, so the resolver refuses it before any damage.
    let hits = [
        hit(0, &mount, actor(5), actor(1), 4.0, 30),
        hit(1, &ghost, actor(9), actor(1), 4.0, 30),
    ];
    let lethal = SYNTHETIC_HULL_INTEGRITY + 1.0;
    let resolution = resolver
        .resolve(Tick(30), &hits)
        .expect("the resolver accepts the batch");
    assert!(
        resolution
            .events
            .iter()
            .any(|event| matches!(&event.kind, DamageEventKind::HitApplied { .. })),
        "the resolver applied the landed hit"
    );
    assert!(
        resolution.events.iter().any(|event| matches!(
            &event.kind,
            DamageEventKind::HitRefused {
                reason: RefusalReason::UnknownNode,
                ..
            }
        )),
        "and refused the hit naming a node that does not exist"
    );

    let report = apply_target_damage(
        &mut world,
        &TargetDamageTick {
            hits: &hits,
            events: &resolution.events,
        },
    )
    .expect("the batch is in session");
    assert_eq!(report.applied, 1, "only the landed hit is an attack");
    assert_eq!(report.not_applied, 1, "a refused hit is not an attack");
    assert_eq!(report.lifecycle, 0);
    assert_eq!(report.feed.recorded, 1);
    assert_eq!(report.feed.untracked, 0);

    let phase = apply_selection_edges(&mut world, actor(1), &[], SelectionFrame::at(Tick(31)))
        .expect("registered")
        .phase
        .expect("a phase record");
    assert_eq!(phase.threats.len(), 1, "raider 5 is a live threat");
    assert_eq!(phase.threats[0].attacker, actor(5));
    assert!(
        phase.reticle.is_none(),
        "nothing is selected yet, so no reticle"
    );

    // The nearest-attacker action — bound to the weapon-cycle edge by the
    // fixture preset — now finds the attacker the damage system evidenced.
    let report = apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::CycleWeapon]),
        SelectionFrame::at(Tick(31)),
    )
    .expect("registered");
    assert_eq!(report.selection, Some(actor(5)));
    let reticle = report
        .phase
        .expect("a phase record")
        .reticle
        .expect("a reticle");
    assert!(
        reticle.threatening,
        "the reticle warns about the actor that actually shot"
    );

    // A lethal hit: the resolver emits the destruction, the entry records
    // it, and the next phase clears the selection rather than describing a
    // destroyed actor.
    let resolution = resolver
        .resolve(Tick(32), &[hit(2, &hull, actor(9), actor(5), lethal, 32)])
        .expect("the resolver accepts the batch");
    let report = apply_target_damage(
        &mut world,
        &TargetDamageTick {
            hits: &[],
            events: &resolution.events,
        },
    )
    .expect("in session");
    assert_eq!(report.lifecycle, 1, "the destruction was recorded");
    assert_eq!(report.applied, 0, "the batch carried no submitted hits");

    let phase = apply_selection_edges(&mut world, actor(1), &[], SelectionFrame::at(Tick(33)))
        .expect("registered")
        .phase
        .expect("a phase record");
    assert_eq!(phase.selection, None, "the destroyed selection cleared");
    assert_eq!(phase.reticle, None, "and no reticle describes it");
    assert_eq!(
        world.resource::<TargetingSession>().store().gone(&actor(5)),
        Some(cs_sim::damage::LifecycleKind::Destroyed),
        "the destruction is the damage system's own transition"
    );
    assert_eq!(
        phase.threats.len(),
        1,
        "and the threat cue is still its own record"
    );
}

/// The entries refuse to run without a session rather than panicking or
/// quietly doing nothing.
#[test]
fn accept_f30_b_entries_report_a_missing_session() {
    let mut world = World::new();
    assert!(sync_targetable_roster(&mut world).registered == 0);
    assert_eq!(
        apply_selection_edges(
            &mut world,
            actor(1),
            &edges(&[FlightCommand::TargetNext]),
            SelectionFrame::at(Tick(1))
        ),
        Err(TargetingError::NoSession)
    );
    assert_eq!(
        apply_target_damage(
            &mut world,
            &TargetDamageTick {
                hits: &[],
                events: &[],
            }
        ),
        Err(TargetingError::NoSession)
    );
}

/// A crosshair ray is producer evidence, and the store validates the cone
/// the declared rules carry: the frame's ray is the only thing an
/// under-crosshair action reads.
#[test]
fn accept_f30_b_crosshair_ray_comes_from_the_producer() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);

    let mut session = world
        .remove_resource::<TargetingSession>()
        .expect("installed");
    session
        .bindings_mut()
        .bind(FlightCommand::DropOrdnance, SelectionAction::UnderCrosshair);
    let declared_cone = session.store().policy().crosshair_cone;
    world.insert_resource(session);

    let ray = world
        .resource::<TargetingSession>()
        .store()
        .crosshair_query(
            pos([0.0, 0.0, 0.0]),
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction"),
            None,
            BTreeSet::new(),
        )
        .expect("the declared cone is valid");
    assert_eq!(
        ray.cone, declared_cone,
        "a producer with no cone of its own takes the declared one"
    );

    let report = apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::DropOrdnance]),
        SelectionFrame::at(Tick(40)).with_crosshair(&ray),
    )
    .expect("registered");
    assert_eq!(
        report.selection,
        Some(actor(3)),
        "the ray resolves to the nearer of the two eligible actors on it"
    );

    // The occlusion set the producer reports is the same evidence: occlude
    // the wingman and the same ray reaches the raider behind it.
    let ray = world
        .resource::<TargetingSession>()
        .store()
        .crosshair_query(
            pos([0.0, 0.0, 0.0]),
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction"),
            None,
            BTreeSet::from([actor(3)]),
        )
        .expect("the declared cone is valid");
    let report = apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::DropOrdnance]),
        SelectionFrame::at(Tick(41)).with_crosshair(&ray),
    )
    .expect("registered");
    assert_eq!(report.selection, Some(actor(9)));
}
