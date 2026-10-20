//! Acceptance for Rally #1727 `PARITY2-M01-TARGET-INDICATORS`: the measured
//! `Q`/`W`/`E`/`R`/`T` target-selection controls, their filters, the
//! selected-target indicator's state rules and the spyglass window gate.
//!
//! The measured side is the engine image (`docs/findings/2026-10-20-m01-
//! target-indicators.md`): command ids 36–46, the exact-modifier key rows,
//! enemy-or-objective / ally-aircraft / non-aircraft classes, marker-only in
//! view vs arrow+label(+ring) off view, the `0x4a5f40` colours and the
//! `min(2000, near + 0.8*(far-near))` gate with its `0.875` hysteresis.
//!
//! Every fixture here is newly authored synthetic data — no original game
//! content is exercised.

use bevy::ecs::world::World;
use bevy::prelude::{ButtonInput, GlobalTransform, KeyCode};

use cs_app::mission_actors::{MissionGroundTarget, ScriptedAircraftActor};
use cs_app::mission_session::{
    MissionCockpitActions, mint_mission_target_keys, mission_action_map, mission_selection_binding,
    original_target_command, spyglass_window_threshold, sync_mission_target_poses,
    target_indicator_color,
};
use cs_app::mission_start::{ScriptedAircraft, SpawnDispatch};
use cs_app::scene::SceneGeneration;
use cs_app::targeting::{
    TargetableBinding, TargetableState, TargetingSession, apply_selection_edges, lower_rules,
    sync_targetable_roster,
};
use cs_content::target_rules::declared_synthetic_target_rules;
use cs_sim::damage::ActorId;
use cs_sim::targeting::{
    CycleDirection, SelectionAction, SelectionFrame, TargetClass, TargetFilter, TargetRecord,
};
use cs_types::Tick;
use cs_types::content::Provenance;
use cs_types::evidence::ClaimId;
use cs_types::input::{Action, BindingSource, BindingTarget, FlightCommand, Key};
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 42;

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn pos(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

/// The measured key-to-command table (`0x4936c0`–`0x493d00`, static): `Q`
/// and `T` bind no chords, `E`/`W`/`R` each bind next/previous/nearest under
/// plain/`Shift`/`Ctrl`, and the modifier matching is exact — a chord the
/// table does not name fires nothing.
#[test]
fn accept_parity2_m01_target_indicators_measured_key_table_is_exact() {
    use FlightCommand::*;
    let command = |key, shift, ctrl| original_target_command(key, shift, ctrl);
    assert_eq!(command(Key::Q, false, false), Some(TargetUnderReticule));
    assert_eq!(command(Key::T, false, false), Some(TargetNothing));
    assert_eq!(command(Key::E, false, false), Some(TargetNextEnemy));
    assert_eq!(command(Key::E, true, false), Some(TargetPrevEnemy));
    assert_eq!(command(Key::E, false, true), Some(TargetNearestEnemy));
    assert_eq!(command(Key::W, false, false), Some(TargetNextAlly));
    assert_eq!(command(Key::W, true, false), Some(TargetPrevAlly));
    assert_eq!(command(Key::W, false, true), Some(TargetNearestAlly));
    assert_eq!(command(Key::R, false, false), Some(TargetNextGround));
    assert_eq!(command(Key::R, true, false), Some(TargetPrevGround));
    assert_eq!(command(Key::R, false, true), Some(TargetNearestGround));
    // Exact matching: chords the table has no row for fire nothing.
    for (key, shift, ctrl) in [
        (Key::Q, true, false),
        (Key::Q, false, true),
        (Key::T, true, false),
        (Key::T, false, true),
        (Key::E, true, true),
        (Key::W, true, true),
        (Key::R, true, true),
        (Key::A, false, false),
    ] {
        assert_eq!(command(key, shift, ctrl), None, "{key:?} {shift} {ctrl}");
    }
}

/// The eleven measured commands carry the original table's ids 36–46.
#[test]
fn accept_parity2_m01_target_indicators_original_command_ids() {
    use FlightCommand::*;
    let ids = |command: FlightCommand| cs_types::input::original_command_ids(command);
    for (command, id) in [
        (TargetNextEnemy, 36),
        (TargetPrevEnemy, 37),
        (TargetNearestEnemy, 38),
        (TargetNextAlly, 39),
        (TargetPrevAlly, 40),
        (TargetNearestAlly, 41),
        (TargetNextGround, 42),
        (TargetPrevGround, 43),
        (TargetNearestGround, 44),
        (TargetUnderReticule, 45),
        (TargetNothing, 46),
    ] {
        assert_eq!(ids(command), &[id], "{command:?}");
    }
}

/// The session's command-edge table lowers the eleven commands onto the
/// measured classes: enemy-or-objective, allied aircraft and non-aircraft.
#[test]
fn accept_parity2_m01_target_indicators_selection_binding_classes() {
    let binding = mission_selection_binding();
    use CycleDirection::{Next, Previous};
    use FlightCommand::*;
    let cycle = |direction, filter| SelectionAction::Cycle { direction, filter };
    let nearest = |filter| SelectionAction::Nearest { filter };
    let cases: [(FlightCommand, SelectionAction); 11] = [
        (
            TargetNextEnemy,
            cycle(Next, TargetFilter::HostileOrObjective),
        ),
        (
            TargetPrevEnemy,
            cycle(Previous, TargetFilter::HostileOrObjective),
        ),
        (
            TargetNearestEnemy,
            nearest(TargetFilter::HostileOrObjective),
        ),
        (TargetNextAlly, cycle(Next, TargetFilter::Allied)),
        (TargetPrevAlly, cycle(Previous, TargetFilter::Allied)),
        (TargetNearestAlly, nearest(TargetFilter::Allied)),
        (
            TargetNextGround,
            cycle(Next, TargetFilter::NotClass(TargetClass::Aircraft)),
        ),
        (
            TargetPrevGround,
            cycle(Previous, TargetFilter::NotClass(TargetClass::Aircraft)),
        ),
        (
            TargetNearestGround,
            nearest(TargetFilter::NotClass(TargetClass::Aircraft)),
        ),
        (TargetUnderReticule, SelectionAction::UnderCrosshair),
        (TargetNothing, SelectionAction::Clear),
    ];
    for (command, action) in cases {
        assert_eq!(binding.action(command), Some(&action), "{command:?}");
    }
}

/// The mission's action map frees every measured targeting key: `Q`/`W`/`E`
/// (the playtest's flight axes), `R` (designed reset), `T` (designed target
/// cycle) carry nothing; `Shift`/`Ctrl` stay pure modifiers, and the flight
/// axes sit on the original's own arrow keys.
#[test]
fn accept_parity2_m01_target_indicators_action_map_frees_target_keys() {
    let map = mission_action_map();
    let binds = |key: Key| {
        map.bindings()
            .iter()
            .any(|binding| binding.source == BindingSource::Key(key))
    };
    for key in [
        Key::Q,
        Key::W,
        Key::E,
        Key::R,
        Key::T,
        Key::LeftShift,
        Key::LeftControl,
    ] {
        assert!(!binds(key), "{key:?} must stay free for the targeting rows");
    }
    let axis = |key: Key| {
        map.bindings()
            .iter()
            .filter_map(|binding| match (binding.source, binding.target) {
                (BindingSource::Key(bound), BindingTarget::Axis { command, .. })
                    if bound == key =>
                {
                    Some(command)
                }
                _ => None,
            })
            .next()
    };
    for (key, command) in [
        (Key::ArrowUp, FlightCommand::Pitch),
        (Key::ArrowDown, FlightCommand::Pitch),
        (Key::ArrowLeft, FlightCommand::Roll),
        (Key::ArrowRight, FlightCommand::Roll),
    ] {
        assert_eq!(axis(key), Some(command), "{key:?} must bind its axis");
    }
    let edge = |key: Key| {
        map.bindings()
            .iter()
            .filter_map(|binding| match (binding.source, binding.target) {
                (BindingSource::Key(bound), BindingTarget::Command(command)) if bound == key => {
                    Some(command)
                }
                _ => None,
            })
            .next()
    };
    assert_eq!(edge(Key::C), Some(FlightCommand::ThrottleStepUp));
    assert_eq!(edge(Key::X), Some(FlightCommand::FireSecondary));
    // The playtest's own rows survive: guns on Space, throttle down on F.
    assert_eq!(edge(Key::Space), Some(FlightCommand::FirePrimary));
    assert_eq!(edge(Key::F), Some(FlightCommand::ThrottleStepDown));
}

/// The key minter mints the measured commands from the raw keyboard state —
/// letter plus held modifiers, exact — into the same cockpit queue the map's
/// delivered edges drain through.
#[test]
fn accept_parity2_m01_target_indicators_key_minter() {
    let mint = |press: KeyCode, held: &[KeyCode]| {
        let mut world = World::new();
        let mut keys = ButtonInput::<KeyCode>::default();
        for key in held {
            keys.press(*key);
        }
        keys.clear_just_pressed(KeyCode::ShiftLeft);
        keys.clear_just_pressed(KeyCode::ControlLeft);
        keys.press(press);
        world.insert_resource(keys);
        mint_mission_target_keys(&mut world);
        world
            .get_resource::<MissionCockpitActions>()
            .map(|queued| queued.actions.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        mint(KeyCode::KeyE, &[]),
        vec![Action::Flight(FlightCommand::TargetNextEnemy)]
    );
    assert_eq!(
        mint(KeyCode::KeyE, &[KeyCode::ShiftLeft]),
        vec![Action::Flight(FlightCommand::TargetPrevEnemy)]
    );
    assert_eq!(
        mint(KeyCode::KeyR, &[KeyCode::ControlLeft]),
        vec![Action::Flight(FlightCommand::TargetNearestGround)]
    );
    assert_eq!(
        mint(KeyCode::KeyQ, &[]),
        vec![Action::Flight(FlightCommand::TargetUnderReticule)]
    );
    assert_eq!(
        mint(KeyCode::KeyT, &[]),
        vec![Action::Flight(FlightCommand::TargetNothing)]
    );
    // The exact-modifier rule: a chord with no table row mints nothing.
    assert!(mint(KeyCode::KeyQ, &[KeyCode::ShiftLeft]).is_empty());
    assert!(mint(KeyCode::KeyE, &[KeyCode::ShiftLeft, KeyCode::ControlLeft]).is_empty());
    // ...and the letter is claimed either way: an unconsumed press would
    // fall through to the playtest's raw `R` reset, which the mission map
    // no longer owns.
    let pressed = |press: KeyCode, held: &[KeyCode]| {
        let mut world = World::new();
        let mut keys = ButtonInput::<KeyCode>::default();
        for key in held {
            keys.press(*key);
            keys.clear_just_pressed(*key);
        }
        keys.press(press);
        world.insert_resource(keys);
        mint_mission_target_keys(&mut world);
        world.resource::<ButtonInput<KeyCode>>().just_pressed(press)
    };
    assert!(!pressed(KeyCode::KeyR, &[]), "the minted press is consumed");
    assert!(
        !pressed(KeyCode::KeyR, &[KeyCode::ShiftLeft, KeyCode::ControlLeft]),
        "the unminted chord is consumed too"
    );
    assert!(!pressed(KeyCode::KeyQ, &[KeyCode::ShiftLeft]));
}

/// A world with the session under the measured binding and a roster of a
/// hostile aircraft, a friendly wingman and a non-aircraft objective.
fn bound_world() -> World {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let mut world = World::new();
    world.insert_resource(TargetingSession::new(
        session_id(),
        lowered,
        mission_selection_binding(),
        declared.subject().clone(),
        generation,
    ));
    let raiders = cs_sim::targeting::synthetic_raider_faction();
    let player = cs_sim::targeting::synthetic_player_faction();
    let traders = cs_sim::targeting::synthetic_trader_faction();
    for (serial, faction, class, objective, position) in [
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
        (3, player, TargetClass::Aircraft, false, [50.0, 0.0, 0.0]),
        (4, traders, TargetClass::WorldObject, true, [0.0, 60.0, 0.0]),
    ] {
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
    world
}

fn edges(commands: &[FlightCommand]) -> Vec<Action> {
    commands.iter().copied().map(Action::Flight).collect()
}

fn selected(world: &World) -> Option<ActorId> {
    world.resource::<TargetingSession>().selection().current()
}

/// The measured class semantics end to end through `apply_selection_edges`:
/// `E` picks hostile **or objective** targets, `W` the allied aircraft, `R`
/// the non-aircraft one, `Q` the actor under the crosshair and `T` nothing.
#[test]
fn accept_parity2_m01_target_indicators_selection_classes_end_to_end() {
    let mut world = bound_world();
    let mut tick = Tick(20);
    let mut act = |world: &mut World, commands: &[FlightCommand]| {
        let frame = SelectionFrame::at(tick);
        tick = Tick(tick.0 + 1);
        apply_selection_edges(world, actor(1), &edges(commands), frame).expect("registered")
    };
    // E — next enemy-or-objective — never picks the wingman (actor 3).
    act(&mut world, &[FlightCommand::TargetNextEnemy]);
    let enemy = selected(&world).expect("an enemy-or-objective selection");
    assert_ne!(enemy, actor(1), "never the observer");
    assert_ne!(enemy, actor(3), "never the friendly wingman");
    // W — the ally class — picks the wingman.
    act(&mut world, &[FlightCommand::TargetNextAlly]);
    assert_eq!(selected(&world), Some(actor(3)));
    // R — the non-aircraft class — picks the objective world object.
    act(&mut world, &[FlightCommand::TargetNextGround]);
    assert_eq!(selected(&world), Some(actor(4)));
    // The objective is in the enemy-or-objective class too.
    let frame = SelectionFrame::at(tick);
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNearestEnemy]),
        frame,
    )
    .expect("registered");
    assert_eq!(
        selected(&world),
        Some(actor(4)),
        "the nearest enemy-or-objective is the 60 m objective, not a 100 m raider"
    );
    // Q — under reticule — selects the actor the crosshair ray meets.
    let crosshair = cs_sim::targeting::CrosshairQuery::try_new(
        pos([0.0, 0.0, 0.0]),
        UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit axis"),
        cs_types::space::Radians(std::f64::consts::PI / 18.0),
        std::collections::BTreeSet::new(),
    )
    .expect("a valid cone");
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetUnderReticule]),
        SelectionFrame {
            now: tick,
            crosshair: Some(&crosshair),
        },
    )
    .expect("registered");
    assert_eq!(selected(&world), Some(actor(3)), "dead ahead at 50 m");
    // T — target nothing — clears the selection.
    apply_selection_edges(
        &mut world,
        actor(1),
        &edges(&[FlightCommand::TargetNothing]),
        SelectionFrame::at(tick),
    )
    .expect("registered");
    assert_eq!(selected(&world), None);
}

/// The measured window gate: `min(2000, near + 0.8*(far-near))`, and the
/// `0.875` multiply while no window exists (`0x49dd13`, static).
#[test]
fn accept_parity2_m01_target_indicators_spyglass_gate_constants() {
    // Default-like planes: 0.1 + 0.8 * 999.9 ≈ 800.02, inside the cap.
    let base = 0.1 + 0.8 * (1000.0 - 0.1);
    assert_eq!(spyglass_window_threshold(0.1, 1000.0, true), base);
    assert_eq!(spyglass_window_threshold(0.1, 1000.0, false), base * 0.875);
    // A far plane past the cap: the gate never exceeds 2000 m.
    assert_eq!(spyglass_window_threshold(0.1, 100_000.0, true), 2000.0);
    assert_eq!(
        spyglass_window_threshold(0.1, 100_000.0, false),
        2000.0 * 0.875
    );
}

/// The measured marker colours (`0x4a5f40`, static): a vehicle-node target
/// is hostile red `(200,0,0)` or blue — every rostered class is a vehicle
/// node, so the measured node-less side-compare (green) never answers here.
#[test]
fn accept_parity2_m01_target_indicators_marker_colours() {
    let reticle = |hostile: bool, objective: bool| cs_sim::targeting::Reticle {
        target: actor(9),
        class: TargetClass::Aircraft,
        faction: cs_sim::targeting::synthetic_raider_faction(),
        allegiance: Some(cs_sim::targeting::Allegiance::Hostile),
        hostile,
        threatening: false,
        objective,
        revealed: true,
        position: pos([0.0, 0.0, 0.0]),
        distance: cs_types::space::Meters(100.0),
    };
    let blue = {
        let (r, g, b) = cs_app::mission_session::OBJECTIVE_BLUE;
        bevy::prelude::Color::srgb(r, g, b)
    };
    assert_eq!(target_indicator_color(&reticle(false, true)), blue);
    assert_eq!(
        target_indicator_color(&reticle(true, false)),
        bevy::prelude::Color::srgb(200.0 / 255.0, 0.0, 0.0)
    );
    // A non-hostile node target — an ally, a neutral ship — draws blue, like
    // the objective sites: the measured rule's green is the node-less
    // side-compare branch the roster cannot reach.
    assert_eq!(target_indicator_color(&reticle(false, false)), blue);
}

/// The pose sync keeps the store's records on the live entity poses: the
/// roster passes stamp the bind pose at registration and never move it, so
/// without one pass per tick the cycle's distance rank, the under-reticule
/// cone and the spyglass gate all measure where actors spawned.
#[test]
fn accept_parity2_m01_target_indicators_pose_sync_updates_records() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let mut world = World::new();
    world.insert_resource(TargetingSession::new(
        session_id(),
        lowered,
        mission_selection_binding(),
        declared.subject().clone(),
        generation,
    ));
    world
        .resource_mut::<TargetingSession>()
        .store_mut()
        .register(TargetRecord {
            actor: actor(9),
            faction: cs_sim::targeting::synthetic_raider_faction(),
            class: TargetClass::Aircraft,
            objective: false,
            revealed: true,
            phase_eligible: true,
            position: pos([0.0, 0.0, 0.0]),
        })
        .expect("a fresh actor registers");
    world
        .resource_mut::<TargetingSession>()
        .store_mut()
        .register(TargetRecord {
            actor: actor(4),
            faction: cs_sim::targeting::synthetic_trader_faction(),
            class: TargetClass::WorldObject,
            objective: true,
            revealed: true,
            phase_eligible: true,
            position: pos([0.0, 0.0, 0.0]),
        })
        .expect("a fresh actor registers");

    let record = |name: &str| ScriptedAircraft {
        name: name.to_owned(),
        index: 0,
        field_count: 81,
        netids: None,
        stored_pose: None,
        team: Some(1),
        group: None,
        enabled: Some(true),
        primary_target: None,
        title: None,
        starts_asleep: None,
        dispatch: SpawnDispatch::VehicleRecord("kestrel".to_owned()),
        provenance: Provenance::designed(ClaimId::new("parity2-m01.pose-sync").expect("claim id")),
    };
    // A live aircraft entity whose rendered pose has left its bind pose.
    world.spawn((
        ScriptedAircraftActor {
            record: record("kestrel_1"),
            vehicle_record: "kestrel".to_owned(),
            damage_actor: actor(9),
            generation,
            asleep: false,
        },
        GlobalTransform::from_xyz(120.0, 30.0, -40.0),
    ));
    // A second-generation entity is not this session's to write.
    world.spawn((
        ScriptedAircraftActor {
            record: record("kestrel_2"),
            vehicle_record: "kestrel".to_owned(),
            damage_actor: actor(9),
            generation: generation.next(),
            asleep: false,
        },
        GlobalTransform::from_xyz(-1.0, -1.0, -1.0),
    ));
    // A bound ground target's stored canonical position.
    world.spawn(MissionGroundTarget {
        node: "site1".to_owned(),
        declared: Some(0),
        damage_actor: actor(4),
        generation,
        position_m: [400.0, 2.0, 900.0],
        healthy: None,
        destroyed: None,
        coupling: Vec::new(),
        objective: true,
        other_target: false,
        description: None,
        help_label: None,
        roster: None,
        allegiance_open: None,
    });

    sync_mission_target_poses(&mut world);

    let held = |actor: ActorId| {
        world
            .resource::<TargetingSession>()
            .store()
            .record(&actor)
            .map(|record| record.position.to_array())
    };
    assert_eq!(held(actor(9)), Some([120.0, 30.0, -40.0]));
    assert_eq!(held(actor(4)), Some([400.0, 2.0, 900.0]));
}
