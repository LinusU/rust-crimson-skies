//! Acceptance scenario F30-C: the HUD, spyglass and weapon-guidance consumer
//! views, derived from one phase record at the tick the consumers render.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-C`. Task test prefix: `accept_f30_c_`.
//!
//! These tests drive production code only: `cs_app::targeting`'s
//! [`apply_target_consumers`], [`teardown_target_consumers`],
//! [`TargetConsumers`], [`TargetingSession`], [`lower_rules`] and the F30-B
//! producer entries, over a real `bevy::ecs::world::World`. The destruction
//! that clears the selection is the **real** [`DamageResolver`]'s: the phase
//! record never describes an actor the damage system did not destroy.
//!
//! The minimum scenario is AC03 at this layer: a target destroyed between the
//! selection pass and the render is cleared before the spyglass view describes
//! it, and the view says why.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use std::collections::BTreeSet;

use bevy::ecs::world::World;
use cs_app::scene::SceneGeneration;
use cs_app::targeting::{
    ClearedTarget, ConsumerBinding, GuidanceWithheld, TargetDamageTick, TargetableBinding,
    TargetableState, TargetingError, TargetingSession, apply_selection_edges,
    apply_target_consumers, apply_target_damage, lower_rules, lower_selection_actions,
    sync_targetable_roster, teardown_target_consumers,
};
use cs_content::target_rules::{
    DeclaredRelation, TargetRuleSet, declared_synthetic_selection_actions,
    declared_synthetic_target_rules,
};
use cs_sim::ai::combat::{
    CandidateVerdict, CombatRequest, CombatRole, RoleAssignment, synthetic_arsenal,
    synthetic_combat_planner,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageNodeKey, DamagePolicy, DamageResolver, HitEvent,
    HitEventId, LifecycleKind, SYNTHETIC_HULL_INTEGRITY, SYNTHETIC_HULL_NODE, SYNTHETIC_MOUNT_NODE,
    synthetic_airframe_graph,
};
use cs_sim::targeting::{
    Allegiance, SelectionClearReason, TargetClass, TargetError, TargetPolicy, WeaponGuidance,
};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

/// The session generation the roster, the damage resolver and the observers all
/// share.
const SESSION: u64 = 7;
/// The resolver's producer serial.
const PRODUCER: u32 = 3;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn pos(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a valid claim id")
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

/// A world with the session resource installed and every fixture actor bound
/// to an entity, under one scene generation.
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

fn entity_of(world: &mut World, actor: ActorId) -> bevy::ecs::entity::Entity {
    let mut query = world.query::<(bevy::ecs::entity::Entity, &TargetableBinding)>();
    query
        .iter(world)
        .find(|(_, binding)| binding.actor == actor)
        .map(|(entity, _)| entity)
        .expect("the actor is bound to an entity")
}

/// The AI's verdict on the actor the HUD reticle describes, decided by the
/// real combat planner from the reticle's own allegiance — the same gate the
/// consumer view publishes.
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

/// A hit for the real resolver, stamped in the fixture's session.
fn hit(
    sequence: u32,
    node: &DamageNodeKey,
    attacker: ActorId,
    victim: ActorId,
    damage: f64,
    tick: u64,
) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: session_id(),
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
}

/// The minimum scenario, AC03: the selected target is destroyed *after* the
/// selection pass published its record and *before* the consumer pass, and the
/// consumer pass clears it before the spyglass view can describe it.
///
/// The failure this discriminates: a consumer pass that reuses the selection
/// pass's record renders the destroyed actor, because nothing re-derives the
/// phase at the tick the consumers render.
#[test]
fn accept_f30_c_destroyed_selection_clears_before_the_spyglass_view() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);

    // Select a raider through the real bound `TargetNext` edge.
    let report = apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    let selected = report.selection.expect("a raider is selected");
    assert_eq!(selected, actor(2));

    // The consumer pass publishes a view of the selection.
    let view = apply_target_consumers(&mut world, actor(1), Tick(21)).expect("registered");
    let spyglass = view.spyglass.clone().expect("a spyglass view");
    assert_eq!(spyglass.target.expect("a target to frame").actor, selected);
    assert!(spyglass.cleared.is_none());
    assert!(view.guidance.expect("a guidance view").aid.is_some());
    assert!(
        world
            .resource::<cs_app::targeting::TargetConsumers>()
            .spyglass()
            .is_some(),
        "the published resource carries the view the rig reads"
    );

    // The **real** damage system destroys the selected actor: a lethal hull hit
    // from raider 9, resolved by the resolver and handed to the damage entry.
    let mut resolver = DamageResolver::new(session_id(), PRODUCER);
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
    let hull = DamageNodeKey::new(SYNTHETIC_HULL_NODE).expect("a valid node key");
    let lethal = [hit(
        0,
        &hull,
        actor(9),
        selected,
        SYNTHETIC_HULL_INTEGRITY + 1.0,
        60,
    )];
    let resolution = resolver
        .resolve(Tick(60), &lethal)
        .expect("the resolver accepts the batch");
    let damage = apply_target_damage(
        &mut world,
        &TargetDamageTick {
            hits: &lethal,
            events: &resolution.events,
        },
    )
    .expect("in session");
    assert_eq!(
        damage.lifecycle, 1,
        "the resolver's own destruction was recorded"
    );
    assert_eq!(
        world.resource::<TargetingSession>().store().gone(&selected),
        Some(LifecycleKind::Destroyed),
        "the destruction is the damage system's own transition"
    );

    // The render pass. Every consumer view must be empty, and the clear is
    // named with its reason.
    let view = apply_target_consumers(&mut world, actor(1), Tick(61)).expect("registered");
    let cleared = ClearedTarget {
        actor: selected,
        reason: SelectionClearReason::Ended(LifecycleKind::Destroyed),
    };
    assert_eq!(view.hud.clone().expect("a hud view").reticle, None);
    assert_eq!(
        view.hud.expect("a hud view").cleared,
        Some(cleared),
        "the HUD view says which actor went and why"
    );
    let spyglass = view.spyglass.expect("a spyglass view");
    assert_eq!(
        spyglass.target, None,
        "the spyglass has nothing to frame: the destroyed actor is not described"
    );
    assert!(!spyglass.has_target());
    assert_eq!(spyglass.cleared, Some(cleared));
    assert_eq!(
        view.guidance.expect("a guidance view").aid,
        None,
        "and the weapon path is offered no aid against a wreck"
    );

    // The published resource agrees with the report, which is what a consumer
    // actually reads.
    let consumers = world.resource::<cs_app::targeting::TargetConsumers>();
    assert_eq!(
        consumers.bound(),
        Some(ConsumerBinding {
            session: session_id(),
            observer: actor(1)
        })
    );
    assert_eq!(consumers.spyglass().expect("bound").target, None);
    assert_eq!(consumers.hud().expect("bound").reticle, None);
}

/// The three views are one record: the reticle the HUD draws, the target the
/// spyglass frames and the aid the weapon path may offer all name the same
/// actor with the same allegiance, and the AI's own gate agrees — a split read
/// could not produce that.
#[test]
fn accept_f30_c_three_views_are_one_read_of_the_roster() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");

    let view = apply_target_consumers(&mut world, actor(1), Tick(20)).expect("registered");
    let hud = view.hud.expect("a hud view");
    let spyglass = view.spyglass.expect("a spyglass view");
    let guidance = view.guidance.expect("a guidance view");
    let reticle = hud.reticle.clone().expect("a reticle");
    let framed = spyglass.target.clone().expect("a framed target");
    let aid = guidance.aid.clone().expect("an aid");

    assert_eq!(reticle.target, framed.actor);
    assert_eq!(reticle.target, aid.target);
    assert_eq!(reticle.allegiance, framed.allegiance);
    assert_eq!(reticle.allegiance, aid.allegiance);
    assert_eq!(reticle.position, framed.position);
    assert_eq!(reticle.position, aid.position);
    assert_eq!(reticle.distance, framed.distance);
    assert_eq!(reticle.distance, aid.distance);
    assert_eq!(reticle.hostile, framed.hostile);
    assert_eq!(reticle.hostile, aid.hostile);
    assert!(reticle.hostile);
    assert_eq!(ai_verdict(&reticle), CandidateVerdict::Eligible);
    assert_eq!(
        guidance.withheld, None,
        "a declared hostile with the fixture's lead indicator on is offered an aid"
    );

    // The aid is a bearing toward the target, not a lead solution: it names no
    // aim offset and no damage.
    let expected = UnitVec3::try_new([0.0, 1.0, 0.0]).expect("a unit direction");
    assert_eq!(aid.bearing, expected, "the aid points at the target");
    assert_eq!(
        aid.at,
        Tick(20),
        "the aid carries the tick it was derived at"
    );
    assert!(!aid.threatening, "nothing has attacked the player yet");

    // The declared lead indicator is on but `designed`, so it is *not*
    // presentable: the engine's own default may not be drawn as original
    // behavior (F30 non-negotiable 3).
    assert!(guidance.lead_indicator.enabled);
    assert!(!guidance.lead_indicator.presentable);
    assert_eq!(
        guidance.lead_indicator.provenance.class,
        ClaimStatus::Designed,
        "and the view carries the option's evidence class"
    );
    assert!(
        !guidance.offers_lead_indicator(),
        "a designed default is offered to the weapon path but not presented as original"
    );
    assert!(
        !guidance.offers_aim_assistance(),
        "the declared aim assistance is off in the fixture"
    );
    assert!(!guidance.aim_assistance.enabled);
}

/// A capture written on the actor's record reaches all three views in the same
/// pass: the reticle, the spyglass and the aid all report a friendly, the AI
/// gate refuses, and an aid toward an ally is withheld by name rather than
/// offered (non-negotiable 1 and 3).
#[test]
fn accept_f30_c_capture_updates_all_three_views_in_one_pass() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    let view = apply_target_consumers(&mut world, actor(1), Tick(20)).expect("registered");
    assert!(
        view.hud
            .expect("a hud view")
            .reticle
            .expect("a reticle")
            .hostile
    );
    assert!(view.guidance.expect("a guidance view").aid.is_some());

    // The mission writes the capture on the entity's own record.
    let captured = entity_of(&mut world, actor(2));
    world
        .get_mut::<TargetableState>(captured)
        .expect("the actor is bound")
        .faction = cs_sim::targeting::synthetic_player_faction();
    sync_targetable_roster(&mut world);

    let view = apply_target_consumers(&mut world, actor(1), Tick(21)).expect("registered");
    let reticle = view
        .hud
        .clone()
        .expect("a hud view")
        .reticle
        .expect("the selection survived the capture");
    assert_eq!(
        reticle.allegiance,
        Some(Allegiance::Friendly),
        "the capture reached the reticle in this pass"
    );
    assert!(!reticle.hostile);
    assert_eq!(
        ai_verdict(&reticle),
        CandidateVerdict::Rejected(cs_sim::ai::combat::RejectReason::NotHostile {
            allegiance: Allegiance::Friendly,
        }),
        "and the AI gate refuses the actor the reticle now calls a friend"
    );

    let framed = view
        .spyglass
        .clone()
        .expect("a spyglass view")
        .target
        .expect("the spyglass still frames the actor");
    assert_eq!(framed.allegiance, Some(Allegiance::Friendly));
    assert!(!framed.hostile, "the spyglass view agrees with the reticle");

    let guidance = view.guidance.expect("a guidance view");
    assert_eq!(
        guidance.aid, None,
        "no aid is offered toward an actor that is now a declared friendly"
    );
    assert_eq!(
        guidance.withheld,
        Some(GuidanceWithheld::NotHostile),
        "and the reason is named rather than collapsed into an empty aid"
    );
}

/// The HUD's threat list draws cues for attackers that are still in the world
/// and withdraws the ones whose attacker left, reporting both. The withdrawal
/// uses `present`, not `eligible`: an attacker that lost sensor contact is
/// still dangerous, so its cue stays.
#[test]
fn accept_f30_c_hud_threat_list_withdraws_cues_whose_attacker_left_the_world() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);

    let mut resolver = DamageResolver::new(session_id(), PRODUCER);
    for serial in [1_u64, 5, 9] {
        resolver
            .register_actor(
                actor(serial),
                synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("registered");
    }
    let mount = DamageNodeKey::new(SYNTHETIC_MOUNT_NODE).expect("a valid node key");
    let hull = DamageNodeKey::new(SYNTHETIC_HULL_NODE).expect("a valid node key");
    // Two attackers land a hit on the player in one batch.
    let hits = [
        hit(0, &mount, actor(5), actor(1), 4.0, 60),
        hit(1, &mount, actor(9), actor(1), 4.0, 60),
    ];
    let resolution = resolver
        .resolve(Tick(60), &hits)
        .expect("the resolver accepts the batch");
    let report = apply_target_damage(
        &mut world,
        &TargetDamageTick {
            hits: &hits,
            events: &resolution.events,
        },
    )
    .expect("in session");
    assert_eq!(report.feed.recorded, 2, "both landed hits are attacks");

    let view = apply_target_consumers(&mut world, actor(1), Tick(61)).expect("registered");
    let hud = view.hud.expect("a hud view");
    assert_eq!(
        hud.threats.len(),
        2,
        "both attackers are still in the world"
    );
    assert!(hud.is_threatening());
    assert!(hud.withdrawn.is_empty());

    // Raider 5 loses sensor contact: it is still flying, so its cue stays.
    let hidden = entity_of(&mut world, actor(5));
    world
        .get_mut::<TargetableState>(hidden)
        .expect("bound")
        .revealed = false;
    sync_targetable_roster(&mut world);
    let view = apply_target_consumers(&mut world, actor(1), Tick(62)).expect("registered");
    let hud = view.hud.expect("a hud view");
    assert_eq!(
        hud.threats.len(),
        2,
        "an attacker that merely lost contact is still a live threat"
    );
    assert!(
        !world
            .resource::<TargetingSession>()
            .store()
            .eligible(&actor(5)),
        "and it is no longer selectable, which is a different question"
    );

    // Raider 5 is destroyed by the real resolver: the cue is withdrawn from
    // the HUD list, named in `withdrawn`, and the ledger keeps the evidence.
    let lethal = [hit(
        2,
        &hull,
        actor(9),
        actor(5),
        SYNTHETIC_HULL_INTEGRITY + 1.0,
        61,
    )];
    let resolution = resolver
        .resolve(Tick(61), &lethal)
        .expect("the resolver accepts the batch");
    apply_target_damage(
        &mut world,
        &TargetDamageTick {
            hits: &lethal,
            events: &resolution.events,
        },
    )
    .expect("in session");

    let view = apply_target_consumers(&mut world, actor(1), Tick(63)).expect("registered");
    let hud = view.hud.expect("a hud view");
    assert_eq!(
        hud.threats.len(),
        1,
        "a destroyed attacker is not on the warning list"
    );
    assert_eq!(
        hud.withdrawn.len(),
        1,
        "and the withdrawal is reported rather than silently dropped"
    );
    assert_eq!(hud.withdrawn[0].attacker, actor(5));
    assert_eq!(
        hud.withdrawn[0].last_attack,
        Tick(60),
        "at the tick it attacked"
    );
    assert!(
        world
            .resource::<TargetingSession>()
            .store()
            .threats(actor(1), Tick(63))
            .iter()
            .any(|cue| cue.attacker == actor(5)),
        "the ledger keeps the attack as evidence"
    );
}

/// A hidden or phase-gated selection clears too, and the view says which
/// reason it was — the same enumeration the roster holds, not a UI guess.
#[test]
fn accept_f30_c_reveal_and_phase_changes_clear_the_views_with_their_reason() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");

    // The selected raider loses sensor contact.
    let hidden = entity_of(&mut world, actor(2));
    world
        .get_mut::<TargetableState>(hidden)
        .expect("bound")
        .revealed = false;
    sync_targetable_roster(&mut world);

    let view = apply_target_consumers(&mut world, actor(1), Tick(21)).expect("registered");
    let cleared = ClearedTarget {
        actor: actor(2),
        reason: SelectionClearReason::NotRevealed,
    };
    let spyglass = view.spyglass.clone().expect("a spyglass view");
    assert_eq!(spyglass.cleared, Some(cleared));
    assert_eq!(spyglass.target, None, "an unrevealed target is not framed");
    assert_eq!(view.hud.expect("a hud view").cleared, Some(cleared));
    assert_eq!(view.guidance.expect("a guidance view").aid, None);

    // Reveal it again and close its script phase instead: the reason follows
    // the roster.
    world
        .get_mut::<TargetableState>(hidden)
        .expect("bound")
        .revealed = true;
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(22)),
    )
    .expect("registered");
    let phased = entity_of(&mut world, actor(2));
    world
        .get_mut::<TargetableState>(phased)
        .expect("bound")
        .phase_eligible = false;
    sync_targetable_roster(&mut world);

    let view = apply_target_consumers(&mut world, actor(1), Tick(23)).expect("registered");
    assert_eq!(
        view.spyglass.expect("a spyglass view").cleared,
        Some(ClearedTarget {
            actor: actor(2),
            reason: SelectionClearReason::PhaseIneligible,
        })
    );

    // An entity that leaves the world is unregistered, and its selection
    // clears with `LeftWorld`.
    world
        .get_mut::<TargetableState>(phased)
        .expect("bound")
        .phase_eligible = true;
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(24)),
    )
    .expect("registered");
    world.entity_mut(phased).despawn();
    sync_targetable_roster(&mut world);
    let view = apply_target_consumers(&mut world, actor(1), Tick(25)).expect("registered");
    assert_eq!(
        view.spyglass.expect("a spyglass view").cleared,
        Some(ClearedTarget {
            actor: actor(2),
            reason: SelectionClearReason::LeftWorld,
        }),
        "an actor the roster no longer holds reports that it left the world"
    );
}

/// Teardown: an explicit end-of-session call clears the views for that
/// generation, refuses to clear a view bound to a different one, and reports
/// whether it cleared anything.
#[test]
fn accept_f30_c_teardown_clears_its_own_generation_only() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_target_consumers(&mut world, actor(1), Tick(20)).expect("registered");

    // A teardown for another session generation is refused: it must not blank
    // the live view.
    let other = SessionId::new(SESSION + 1).expect("a nonzero generation");
    assert!(
        !teardown_target_consumers(&mut world, other),
        "a teardown for another generation changes nothing"
    );
    assert!(
        world
            .resource::<cs_app::targeting::TargetConsumers>()
            .bound()
            .is_some(),
        "the live view survives a foreign teardown"
    );

    assert!(
        teardown_target_consumers(&mut world, session_id()),
        "the live generation's teardown clears the views"
    );
    let consumers = world.resource::<cs_app::targeting::TargetConsumers>();
    assert!(consumers.bound().is_none());
    assert!(consumers.hud().is_none());
    assert!(consumers.spyglass().is_none());
    assert!(consumers.guidance().is_none());

    // A teardown with nothing published clears nothing and says so.
    assert!(!teardown_target_consumers(&mut world, session_id()));
}

/// An aircraft swap: a pass for a different observer rebinds and drops the
/// previous binding's target rather than carrying it over, and a failed pass
/// publishes nothing so a reader never sees the previous one.
#[test]
fn accept_f30_c_rebind_and_failed_pass_leave_no_stale_view() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    let view = apply_target_consumers(&mut world, actor(1), Tick(20)).expect("registered");
    assert!(!view.rebound, "the first pass binds rather than rebinds");
    assert!(view.spyglass.expect("a spyglass view").has_target());

    // A pass for an observer that is not registered is refused *by name*, and
    // publishes nothing: the previous view is unbound, not left up.
    assert_eq!(
        apply_target_consumers(&mut world, actor(42), Tick(21)),
        Err(TargetingError::Store(TargetError::UnknownActor {
            actor: actor(42)
        }))
    );
    let consumers = world.resource::<cs_app::targeting::TargetConsumers>();
    assert!(
        consumers.bound().is_none(),
        "the failed pass unbound the views"
    );
    assert!(
        consumers.spyglass().is_none(),
        "so a consumer reading the resource finds nothing to render"
    );
    assert!(
        world.resource::<TargetingSession>().last_phase().is_none(),
        "and the session publishes no phase record either"
    );

    // The retry: the same observer passes again and republishes from scratch.
    let view = apply_target_consumers(&mut world, actor(1), Tick(22)).expect("registered");
    assert!(view.spyglass.expect("a spyglass view").has_target());
    assert_eq!(
        world
            .resource::<cs_app::targeting::TargetConsumers>()
            .bound(),
        Some(ConsumerBinding {
            session: session_id(),
            observer: actor(1)
        })
    );

    // A different observer in the same session rebinds: the views describe the
    // new observer and report it.
    let view = apply_target_consumers(&mut world, actor(3), Tick(23)).expect("registered");
    assert!(view.rebound, "a different observer rebinds the views");
    assert_eq!(
        view.spyglass.expect("a spyglass view").target,
        None,
        "the wingman's own selection is empty, and the previous observer's target is gone"
    );
}

/// No session at all: the entry refuses visibly rather than panicking or
/// quietly publishing nothing, and it never invents a view.
#[test]
fn accept_f30_c_entry_refuses_without_a_session() {
    let mut world = World::new();
    assert_eq!(
        apply_target_consumers(&mut world, actor(1), Tick(1)),
        Err(TargetingError::NoSession)
    );
    assert!(!teardown_target_consumers(&mut world, session_id()));
}

/// A session that goes away must take its published views with it. The views
/// outlive the [`TargetingSession`] resource that derived them, so a pass
/// refused for `NoSession` has to unbind before reporting — otherwise the last
/// frame's target stays on screen with no session behind it.
#[test]
fn accept_f30_c_a_vanished_session_leaves_no_published_view() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    let view = apply_target_consumers(&mut world, actor(1), Tick(21)).expect("registered");
    assert!(view.spyglass.expect("a spyglass view").has_target());

    // The session resource is removed, as a mission teardown that has not run
    // `teardown_target_consumers` would leave it.
    let session = world
        .remove_resource::<TargetingSession>()
        .expect("the session is installed");
    assert_eq!(
        apply_target_consumers(&mut world, actor(1), Tick(22)),
        Err(TargetingError::NoSession)
    );
    let consumers = world.resource::<cs_app::targeting::TargetConsumers>();
    assert!(
        consumers.bound().is_none(),
        "the refused pass unbound the views: nothing on screen belongs to a session that is gone"
    );
    assert!(consumers.hud().is_none());
    assert!(consumers.spyglass().is_none());
    assert!(consumers.guidance().is_none());

    // Restoring the session republishes from scratch: the retry path is the
    // first pass's, and the held selection came back with the resource.
    world.insert_resource(session);
    let view = apply_target_consumers(&mut world, actor(1), Tick(23)).expect("registered");
    assert!(
        view.spyglass.expect("a spyglass view").has_target(),
        "the restored session republishes its own selection"
    );
}

/// A new session generation under the same observer serial is a rebind, not a
/// continuation. An `ActorId` is generation-qualified, so the new generation's
/// observer is a different id and the old generation's target box must not
/// follow it.
#[test]
fn accept_f30_c_a_new_session_generation_rebinds_the_views() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    let view = apply_target_consumers(&mut world, actor(1), Tick(21)).expect("registered");
    assert!(!view.rebound);
    let first = view.spyglass.expect("a spyglass view");
    assert!(first.has_target(), "the first generation frames a raider");

    // The next generation: a different `SessionId`, the same observer serial,
    // and its own roster.
    let next = SessionId::new(SESSION + 1).expect("a nonzero generation");
    let next_actor = |serial: u64| ActorId {
        session: next,
        serial,
    };
    let declared = declared_synthetic_target_rules();
    world.insert_resource(TargetingSession::new(
        next,
        lower_rules(&declared).expect("the fixture rules lower"),
        lower_selection_actions(&declared_synthetic_selection_actions()).expect("actions lower"),
        declared.subject().clone(),
        generation,
    ));
    for (serial, faction, class, objective, position) in roster() {
        let mut state = TargetableState::aircraft(faction, pos(position));
        state.class = class;
        state.objective = objective;
        world.spawn((
            TargetableBinding {
                actor: next_actor(serial),
                rules: declared.subject().clone(),
                generation,
            },
            state,
        ));
    }
    sync_targetable_roster(&mut world);

    let view = apply_target_consumers(&mut world, next_actor(1), Tick(30)).expect("registered");
    assert!(
        view.rebound,
        "an `ActorId` is generation-qualified, so a new session under the same serial is a \
         different observer and rebinds"
    );
    assert_eq!(
        view.spyglass.expect("a spyglass view").target,
        None,
        "the new generation starts with its own selection, not the old one's"
    );
    assert_eq!(
        world
            .resource::<cs_app::targeting::TargetConsumers>()
            .bound(),
        Some(ConsumerBinding {
            session: next,
            observer: next_actor(1)
        }),
        "and the views are attributed to the generation that derived them"
    );
}

/// The two assistance options stay separate options with their own evidence
/// classification, and a *measured* option is presentable where a designed one
/// is not. Nothing here is a hit correction: an unknown option refuses to lower
/// at all, and no magnitude exists on either record.
#[test]
fn accept_f30_c_assistance_options_carry_their_own_evidence() {
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let assistance = lowered.assistance();
    assert!(assistance.lead_indicator.enabled);
    assert!(!assistance.aim_assistance.enabled);
    assert_eq!(
        assistance.lead_indicator.provenance.class,
        ClaimStatus::Designed
    );
    assert_eq!(
        assistance.aim_assistance.provenance.class,
        ClaimStatus::Designed
    );
    assert!(
        !assistance.lead_indicator.presentable(),
        "a designed flag is not presentable as original behavior"
    );
    assert_eq!(
        assistance.lead_indicator.provenance.claim_id,
        assistance.aim_assistance.provenance.claim_id,
        "the fixture's two options come from one declared claim"
    );

    // A `verified_original` option is presentable — and it must name the span
    // it observed, which `Provenance::new` already enforces.
    let measured = Provenance::new(
        claim("f30c.original-lead-indicator"),
        ClaimStatus::VerifiedOriginal,
        Some(
            SourceSpan::new(
                ContentHash::from_bytes([0u8; 32]),
                "crimson.sky",
                None,
                0,
                4,
                None,
            )
            .expect("a valid span"),
        ),
    )
    .expect("a verified_original claim must name its span");
    let on = cs_app::targeting::AssistanceOption {
        enabled: true,
        provenance: measured.clone(),
    };
    assert!(on.presentable(), "a measured option may be presented");
    assert!(
        !cs_app::targeting::AssistanceOption {
            enabled: false,
            provenance: measured,
        }
        .presentable(),
        "but only while it is on"
    );

    // The options are lowered from two separate `Resolved` fields: an unknown
    // one refuses rather than defaulting, so a session can never run with a
    // guessed flag.
    let player = cs_sim::targeting::synthetic_player_faction();
    let raiders = cs_sim::targeting::synthetic_raider_faction();
    let relations = vec![
        DeclaredRelation {
            from: player.clone(),
            to: raiders.clone(),
            allegiance: Resolved::Known(Known::new(
                cs_content::target_rules::DeclaredAllegiance::Hostile,
                Provenance::designed(claim("f30c.synthetic.target-range")),
            )),
        },
        DeclaredRelation {
            from: raiders,
            to: player,
            allegiance: Resolved::Known(Known::new(
                cs_content::target_rules::DeclaredAllegiance::Hostile,
                Provenance::designed(claim("f30c.synthetic.target-range")),
            )),
        },
    ];
    let unknown = |field: &'static str| TargetRuleSet {
        threat_window: Resolved::Known(Known::new(120, Provenance::designed(claim("f30c.t")))),
        crosshair_cone: Resolved::Known(Known::new(
            cs_types::space::Radians(std::f64::consts::PI / 18.0),
            Provenance::designed(claim("f30c.t")),
        )),
        lead_indicator: Resolved::Known(Known::new(true, Provenance::designed(claim("f30c.t")))),
        aim_assistance: if field == "aim_assistance" {
            Resolved::Unknown {
                claim_id: claim("f30c.aim-assistance"),
                reason: "the original assistance behavior was not measured".to_owned(),
            }
        } else {
            Resolved::Unknown {
                claim_id: claim("f30c.lead-indicator"),
                reason: "no original evidence for a lead indicator".to_owned(),
            }
        },
    };
    for rules in [unknown("lead_indicator"), unknown("aim_assistance")] {
        let rules = cs_content::target_rules::DeclaredTargetRules::try_new(
            ContentId::from_source(ContentKind::IaScenario, "synthetic.unknown-assistance")
                .expect("a valid subject id"),
            Origin::SyntheticFixture,
            vec![
                cs_sim::targeting::synthetic_player_faction(),
                cs_sim::targeting::synthetic_raider_faction(),
            ],
            relations.clone(),
            rules,
            Provenance::designed(claim("f30c.synthetic.target-range")),
        )
        .expect("the record is structurally valid");
        assert!(
            matches!(
                lower_rules(&rules),
                Err(cs_app::targeting::TargetLowerError::UnknownRule { .. })
            ),
            "an unknown assistance option refuses to lower rather than defaulting"
        );
    }

    // The guidance record the store derives carries no magnitude: it is a
    // target, a bearing, a distance and the live hostility, nothing a shot
    // could be corrected by.
    let aid = WeaponGuidance {
        at: Tick(1),
        target: actor(2),
        position: pos([0.0, 100.0, 0.0]),
        bearing: UnitVec3::try_new([0.0, 1.0, 0.0]).expect("a unit direction"),
        distance: cs_types::space::Meters(100.0),
        allegiance: Some(Allegiance::Hostile),
        hostile: true,
        threatening: false,
    };
    assert_eq!(
        aid.target,
        actor(2),
        "the aid names a target and nothing else"
    );
}

/// An aid is the store's *eligibility* answer, not the declared option, and the
/// two are reported separately: a session that declares both assistance options
/// off still gets the store's verdict for its selected declared hostile, so a
/// weapon path reads "may an aid apply here" and "is an aid declared and
/// evidenced" as two independent facts instead of one flag that hides a missing
/// option.
#[test]
fn accept_f30_c_an_aid_is_the_store_verdict_not_the_declared_option() {
    let declared = declared_synthetic_target_rules();
    let mut rules = declared.rules().clone();
    rules.lead_indicator = Resolved::Known(Known::new(
        false,
        Provenance::designed(claim("f30c.no-assistance")),
    ));
    rules.aim_assistance = Resolved::Known(Known::new(
        false,
        Provenance::designed(claim("f30c.no-assistance")),
    ));
    let declared = cs_content::target_rules::DeclaredTargetRules::try_new(
        declared.subject().clone(),
        declared.origin().clone(),
        declared.factions().to_vec(),
        declared.relations().to_vec(),
        rules,
        declared.provenance().clone(),
    )
    .expect("the record is structurally valid");

    let generation = SceneGeneration::default().next();
    let mut world = World::new();
    world.insert_resource(TargetingSession::new(
        session_id(),
        lower_rules(&declared).expect("the rules lower"),
        lower_selection_actions(&declared_synthetic_selection_actions()).expect("actions lower"),
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
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");

    let view = apply_target_consumers(&mut world, actor(1), Tick(21)).expect("registered");
    let guidance = view.guidance.expect("a guidance view");
    assert_eq!(
        guidance
            .aid
            .as_ref()
            .expect("the store's verdict for the selected declared hostile")
            .target,
        actor(2),
        "the aid is not withheld because an option is off"
    );
    assert_eq!(guidance.withheld, None, "and nothing is refused");
    assert!(!guidance.lead_indicator.enabled);
    assert!(!guidance.aim_assistance.enabled);
    assert!(
        !guidance.offers_lead_indicator() && !guidance.offers_aim_assistance(),
        "but neither option is declared, so the weapon path is offered neither"
    );
}

/// The store's guidance query is refused rather than answered with a default
/// aid: an unregistered observer, an absent selection and a target sitting on
/// the observer each name themselves. The policy is the store's own, so the
/// consumer view's declared cone and window come from the lowered rules rather
/// than a constant in this module.
#[test]
fn accept_f30_c_guidance_query_refuses_rather_than_defaulting() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    let store = world.resource::<TargetingSession>().store();
    assert_eq!(store.session(), SESSION);
    let policy: TargetPolicy = store.policy();
    assert_eq!(policy.threat_window_ticks, 120, "the declared window");
    assert_eq!(
        policy.crosshair_cone.0,
        std::f64::consts::PI / 18.0,
        "the declared cone"
    );

    let mut selection = cs_sim::targeting::TargetSelection::new();
    assert_eq!(
        store.guidance(actor(1), &mut selection, Tick(1)),
        Err(TargetError::NoSelection),
        "nothing selected is refused, not answered with a default aid"
    );
    assert_eq!(
        store.guidance(actor(42), &mut selection, Tick(1)),
        Err(TargetError::UnknownActor { actor: actor(42) })
    );

    // Select a target through the store's own evaluation, then put it exactly
    // on the observer: the bearing is undefined and the query says so rather
    // than picking an axis.
    assert_eq!(
        store
            .apply(
                actor(1),
                &mut selection,
                &cs_sim::targeting::SelectionRequest::Nearest {
                    filter: cs_sim::targeting::TargetFilter::Allegiance(Allegiance::Hostile),
                },
            )
            .expect("registered"),
        Some(actor(2)),
        "the store's own evaluation selects a raider"
    );
    world
        .get_resource_mut::<TargetingSession>()
        .expect("installed")
        .store_mut()
        .set_pose(actor(2), pos([0.0, 0.0, 0.0]))
        .expect("registered");
    let store = world.resource::<TargetingSession>().store();
    assert_eq!(
        store.guidance(actor(1), &mut selection, Tick(1)),
        Err(TargetError::DegenerateBearing { actor: actor(2) }),
        "a target on the observer has no bearing, and the query names it"
    );
    // And the refusal repeats: a refused query is not a partially applied one.
    assert_eq!(
        store.guidance(actor(1), &mut selection, Tick(2)),
        Err(TargetError::DegenerateBearing { actor: actor(2) })
    );
}

/// The consumer pass reads the roster's canonical f64 world positions: a
/// world rebase that shifts the local render frame cannot move what the views
/// describe (non-negotiable 5).
#[test]
fn accept_f30_c_views_survive_an_origin_shift() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    let before = apply_target_consumers(&mut world, actor(1), Tick(20)).expect("registered");
    let framed = before
        .spyglass
        .clone()
        .expect("a spyglass view")
        .target
        .expect("a framed target");

    // Two entities in the scene, anchored in the local frame the renderer uses.
    let origin = cs_app::origin::WorldOrigin::new(
        cs_app::origin::OriginEpoch::default(),
        pos([0.0, 0.0, 0.0]),
    );
    let mut anchors = [
        cs_app::origin::SpatialAnchor::new(&origin, pos([0.0, 0.0, 0.0]))
            .expect("the player anchors"),
        cs_app::origin::SpatialAnchor::new(&origin, framed.position).expect("the raider anchors"),
    ];
    let local_before = anchors.map(|anchor| anchor.local());

    // The renderer rebases a megametre away: every local coordinate changes and
    // nothing in the world does.
    let shift =
        cs_app::origin::OriginShift::rebase(origin, pos([1e6, 1e6, 1e6])).expect("a valid rebase");
    shift.apply(&mut anchors).expect("the transaction applies");
    assert_eq!(anchors[0].world(), pos([0.0, 0.0, 0.0]));
    assert_eq!(anchors[1].world(), framed.position);
    assert!(
        anchors
            .iter()
            .zip(local_before)
            .any(|(anchor, local)| anchor.local() != local),
        "the rebase really moved the local frame, so it participated"
    );

    let after = apply_target_consumers(&mut world, actor(1), Tick(21)).expect("registered");
    let refamed = after
        .spyglass
        .clone()
        .expect("a spyglass view")
        .target
        .expect("still framed");
    assert_eq!(
        refamed.position, framed.position,
        "the world point is stable"
    );
    assert_eq!(refamed.distance, framed.distance);
    assert_eq!(refamed.actor, framed.actor);
    assert_eq!(
        after
            .guidance
            .expect("a guidance view")
            .aid
            .expect("an aid")
            .position,
        framed.position
    );
}

/// The HUD's target readout is a copy: mutating it changes nothing in the
/// store, and a consumer that wants to select goes through the command edge.
#[test]
fn accept_f30_c_hud_view_confers_no_combat_authority() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNext]),
        cs_sim::targeting::SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    apply_target_consumers(&mut world, actor(1), Tick(20)).expect("registered");

    // Take the published HUD view, rewrite the reticle to an actor of the
    // consumer's choosing. The store must not notice: the view is a copy, and
    // a consumer that wants to select goes through a command edge.
    let mut view = world
        .resource::<cs_app::targeting::TargetConsumers>()
        .hud()
        .expect("a hud view")
        .clone();
    let mut reticle = view
        .reticle
        .clone()
        .expect("a reticle for the selected actor");
    reticle.target = actor(9);
    reticle.faction = cs_sim::targeting::synthetic_player_faction();
    reticle.allegiance = Some(Allegiance::Friendly);
    reticle.hostile = false;
    view.reticle = Some(reticle);
    // `view` is a local copy: writing to it proves the resource held its own
    // value, because nothing the caller can reach writes through it.
    assert_eq!(
        world
            .resource::<cs_app::targeting::TargetConsumers>()
            .hud()
            .expect("a hud view")
            .reticle
            .as_ref()
            .expect("a reticle")
            .target,
        actor(2),
        "the published view still describes the store's selection"
    );

    let store = world.resource::<TargetingSession>().store();
    assert_eq!(
        store.record(&actor(2)).expect("registered").faction,
        cs_sim::targeting::synthetic_raider_faction(),
        "the store's record is untouched by a rewritten view"
    );
    assert_eq!(
        store
            .ordered(
                actor(1),
                cs_sim::targeting::TargetFilter::Allegiance(Allegiance::Hostile)
            )
            .expect("registered"),
        vec![actor(2), actor(5), actor(9)],
        "and the declared hostile cycle is unchanged"
    );
    assert_eq!(
        world
            .resource::<TargetingSession>()
            .last_phase()
            .expect("published")
            .reticle
            .as_ref()
            .expect("a reticle")
            .target,
        actor(2),
        "the published phase record is a copy too"
    );
}

/// A spyglass that is not the player's own view, and an observer whose own
/// record is missing, are both refused by name rather than described with a
/// default view.
#[test]
fn accept_f30_c_no_default_view_without_an_observer() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);

    // A store with no observers at all: the entry refuses instead of
    // publishing an empty spyglass frame.
    assert_eq!(
        apply_target_consumers(&mut world, actor(99), Tick(1)),
        Err(TargetingError::Store(TargetError::UnknownActor {
            actor: actor(99)
        }))
    );
    assert!(
        world
            .resource::<cs_app::targeting::TargetConsumers>()
            .bound()
            .is_none()
    );

    // Nothing is selected: the spyglass and the guidance both say *why* there
    // is nothing, rather than rendering a default frame.
    let view = apply_target_consumers(&mut world, actor(1), Tick(2)).expect("registered");
    let spyglass = view.spyglass.expect("a spyglass view");
    assert!(!spyglass.has_target());
    assert!(
        spyglass.cleared.is_none(),
        "nothing was cleared; nothing was held"
    );
    let guidance = view.guidance.expect("a guidance view");
    assert!(!guidance.has_aid());
    assert_eq!(guidance.withheld, Some(GuidanceWithheld::NoTarget));
    assert!(!view.rebound);
}

/// A crosshair ray from the camera producer reaches the views: the same
/// under-crosshair edge that selects through the F30-B path produces a frame
/// every consumer can read (AC04's contract half, at this layer).
#[test]
fn accept_f30_c_crosshair_selection_reaches_the_views() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);

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

    let mut session = world
        .remove_resource::<TargetingSession>()
        .expect("installed");
    session.bindings_mut().bind(
        FlightCommand::DropOrdnance,
        cs_sim::targeting::SelectionAction::UnderCrosshair,
    );
    world.insert_resource(session);

    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::DropOrdnance]),
        cs_sim::targeting::SelectionFrame::at(Tick(40)).with_crosshair(&ray),
    )
    .expect("registered");

    let view = apply_target_consumers(&mut world, actor(1), Tick(41)).expect("registered");
    assert_eq!(
        view.spyglass
            .clone()
            .expect("a spyglass view")
            .target
            .expect("a framed target")
            .actor,
        actor(3),
        "the ray's pick is what the spyglass frames"
    );
    assert_eq!(
        view.hud
            .clone()
            .expect("a hud view")
            .reticle
            .expect("a reticle")
            .target,
        actor(3)
    );
    let guidance = view.guidance.expect("a guidance view");
    assert_eq!(
        guidance.withheld,
        Some(GuidanceWithheld::NotHostile),
        "the ray selected the wingman, a declared friendly, so no aid is offered for it"
    );
    assert!(
        !guidance.has_aid(),
        "and the weapon path is offered nothing toward an ally"
    );
}

/// The declared relation table and the declared policy the session runs under
/// come from the declared record, so the consumer view's window and cone are
/// lowered data rather than constants of this stage.
#[test]
fn accept_f30_c_session_runs_the_lowered_rules_not_fixture_constants() {
    // A session with a different declared window: the views' threat list
    // follows the declared window, not a number written into the consumer.
    let declared = declared_synthetic_target_rules();
    let mut rules = declared.rules().clone();
    // A zero-tick window: only an attack in the very tick is a live cue.
    rules.threat_window = Resolved::Known(Known::new(0, Provenance::designed(claim("f30c.t"))));
    let declared = cs_content::target_rules::DeclaredTargetRules::try_new(
        declared.subject().clone(),
        declared.origin().clone(),
        declared.factions().to_vec(),
        declared.relations().to_vec(),
        rules,
        declared.provenance().clone(),
    )
    .expect("the record is structurally valid");
    let generation = SceneGeneration::default().next();
    let mut world = World::new();
    world.insert_resource(TargetingSession::new(
        session_id(),
        lower_rules(&declared).expect("the rules lower"),
        lower_selection_actions(&declared_synthetic_selection_actions()).expect("actions lower"),
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
    sync_targetable_roster(&mut world);

    // Raider 5 attacks at tick 60.
    let mut resolver = DamageResolver::new(session_id(), PRODUCER);
    for serial in [1_u64, 5] {
        resolver
            .register_actor(
                actor(serial),
                synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("registered");
    }
    let mount = DamageNodeKey::new(SYNTHETIC_MOUNT_NODE).expect("a valid node key");
    let hits = [hit(0, &mount, actor(5), actor(1), 4.0, 60)];
    let resolution = resolver
        .resolve(Tick(60), &hits)
        .expect("the resolver accepts the batch");
    apply_target_damage(
        &mut world,
        &TargetDamageTick {
            hits: &hits,
            events: &resolution.events,
        },
    )
    .expect("in session");

    // At the attack tick the cue is live even with a zero-tick window.
    let view = apply_target_consumers(&mut world, actor(1), Tick(60)).expect("registered");
    assert_eq!(
        view.hud.expect("a hud view").threats.len(),
        1,
        "an attack in this tick is inside a zero-tick window"
    );
    // One tick later it is not, and the view carries the declared window.
    let view = apply_target_consumers(&mut world, actor(1), Tick(61)).expect("registered");
    assert!(
        view.hud.expect("a hud view").threats.is_empty(),
        "and one tick later the declared window has closed it"
    );
}
