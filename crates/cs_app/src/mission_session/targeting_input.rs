//! The mission's measured target-selection input (PARITY2-M01-TARGET-INDICATORS,
//! #1727).
//!
//! The original's own keyboard default table binds `Q`/`W`/`E`/`R`/`T` to its
//! eleven targeting commands — `E`/`W`/`R` plus the `Shift` previous and
//! `Ctrl` nearest chords for the enemy/ally/non-aircraft classes, `Q` for the
//! target under the reticule and `T` for target nothing
//! ([`cs_types::input::ORIGINAL_DEFAULT_BINDINGS`], measured at the table
//! builder span `0x4936c0`–`0x493d00`; the F22-J writeup is
//! `docs/findings/2026-10-14-f22-j-original-default-control-bindings.md`).
//! In the original's layout the flight axes live on the arrow keys and the
//! throttle on `-`/`=`, so those letters carry targeting and nothing else.
//!
//! Two seams implement it here, because the F22 boundary cannot express it:
//!
//! * [`mission_action_map`] is the composed session's action map — the
//!   playtest's designed defaults re-laid so the measured targeting letters
//!   carry nothing the map can deliver, and the flight axes sit on the
//!   original's own arrow keys. The map has **no chord sources** (a
//!   [`BindingSource`] is one physical input, never key-plus-modifier), so
//!   the map itself binds no targeting command.
//! * [`mint_mission_target_keys`] mints the eleven commands' edges from the
//!   frame's raw keyboard state — the `just_pressed` letter plus the held
//!   `Shift`/`Ctrl` state — by the same exact packed-code matching the
//!   original's modifier handler performs
//!   ([`cs_types::input::ORIGINAL_MODIFIER_MATCH_SPAN`]: when any modifier
//!   combination of a key is bound, the full code with the held modifiers
//!   must match exactly, so `Shift+Q` fires nothing and `Shift+Ctrl+E`
//!   fires nothing). The minted [`Action::Flight`] edges join the frame's
//!   delivered edges in [`MissionCockpitActions`], behind what the action
//!   map delivered — the same queue the weapon publisher drains, so tests
//!   drive selection through the same seam as the keys.
//!
//! Which command runs which selection action is
//! [`mission_selection_binding`] — the original command set lowered onto the
//! store's [`SelectionAction`] vocabulary. The class semantics are the
//! measured ones: enemy *or objective* (the controls-screen label, and the
//! owner observed `E` cycling M01's objective sites), friendly or neutral
//! **aircraft** for the ally class, and not-aircraft for the ground class.
//!
//! And [`sync_mission_target_poses`] keeps the store's records on the
//! actors' live poses: the roster passes stamp a record at its bind pose
//! and never move it, so without one pass per tick the cycle ranks, the
//! under-reticule cone and the spyglass distance gate all measure where
//! actors spawned rather than where they are.

use std::collections::BTreeSet;

use bevy::prelude::{ButtonInput, Camera3d, GlobalTransform, KeyCode, Vec3, With, Without, World};

use cs_sim::damage::ActorId;
use cs_sim::targeting::{
    CrosshairQuery, CycleDirection, SelectionAction, SelectionBinding, TargetClass, TargetFilter,
};
use cs_types::input::{
    Action, ActionMap, Binding, BindingSource, BindingTarget, FlightCommand, Key,
};
use cs_types::space::{Radians, UnitVec3, WorldPosition};

use crate::input::platform::key_from_bevy;
use crate::mission_actors::{MissionGroundTarget, MissionZeppelinActor, ScriptedAircraftActor};
use crate::playtest::command::{KEY_DEFLECTION, playtest_action_map};

use super::compose::MissionPlayerBody;
use super::weapons::{MissionCockpitActions, MissionWeapons};

/// The measured selection action each mission targeting command runs.
///
/// The table is the original's command set (ids 36–46 in
/// [`cs_types::input::ORIGINAL_DEFAULT_BINDINGS`]) lowered onto the F30-B
/// action vocabulary:
///
/// * `E`/`Shift+E`/`Ctrl+E` — cycle/previous/nearest over
///   [`TargetFilter::HostileOrObjective`];
/// * `W`/`Shift+W`/`Ctrl+W` — cycle/previous/nearest over
///   [`TargetFilter::Allied`];
/// * `R`/`Shift+R`/`Ctrl+R` — cycle/previous/nearest over
///   [`TargetFilter::NotClass`]`(Aircraft)`;
/// * `Q` — [`SelectionAction::UnderCrosshair`];
/// * `T` — [`SelectionAction::Clear`].
#[must_use]
pub fn mission_selection_binding() -> SelectionBinding {
    use CycleDirection::{Next, Previous};
    use FlightCommand::*;
    use TargetFilter as F;
    let mut binding = SelectionBinding::new();
    let cycle = |direction, filter| SelectionAction::Cycle { direction, filter };
    let nearest = |filter| SelectionAction::Nearest { filter };
    binding.bind(TargetNextEnemy, cycle(Next, F::HostileOrObjective));
    binding.bind(TargetPrevEnemy, cycle(Previous, F::HostileOrObjective));
    binding.bind(TargetNearestEnemy, nearest(F::HostileOrObjective));
    binding.bind(TargetNextAlly, cycle(Next, F::Allied));
    binding.bind(TargetPrevAlly, cycle(Previous, F::Allied));
    binding.bind(TargetNearestAlly, nearest(F::Allied));
    binding.bind(
        TargetNextGround,
        cycle(Next, F::NotClass(TargetClass::Aircraft)),
    );
    binding.bind(
        TargetPrevGround,
        cycle(Previous, F::NotClass(TargetClass::Aircraft)),
    );
    binding.bind(
        TargetNearestGround,
        nearest(F::NotClass(TargetClass::Aircraft)),
    );
    binding.bind(TargetUnderReticule, SelectionAction::UnderCrosshair);
    binding.bind(TargetNothing, SelectionAction::Clear);
    binding
}

/// The command the original's targeting row for `key` runs under the held
/// modifiers, or `None` when no row binds that exact packed code.
///
/// The measured table binds `Q` (`0x010`) to target-under-reticule and `T`
/// (`0x014`) to target-nothing with **no** chord rows, and `E`/`W`/`R` each
/// to three — plain next, `Shift` previous, `Ctrl` nearest. The original's
/// modifier matching is exact: the packed code of the held modifiers plus
/// the scancode must name a row, so a `Shift`/`Ctrl` chord on `Q` or `T`,
/// and `Shift+Ctrl` on any key, delivers nothing.
#[must_use]
pub fn original_target_command(key: Key, shift: bool, ctrl: bool) -> Option<FlightCommand> {
    use FlightCommand::*;
    Some(match (key, shift, ctrl) {
        (Key::Q, false, false) => TargetUnderReticule,
        (Key::T, false, false) => TargetNothing,
        (Key::E, false, false) => TargetNextEnemy,
        (Key::E, true, false) => TargetPrevEnemy,
        (Key::E, false, true) => TargetNearestEnemy,
        (Key::W, false, false) => TargetNextAlly,
        (Key::W, true, false) => TargetPrevAlly,
        (Key::W, false, true) => TargetNearestAlly,
        (Key::R, false, false) => TargetNextGround,
        (Key::R, true, false) => TargetPrevGround,
        (Key::R, false, true) => TargetNearestGround,
        _ => return None,
    })
}

/// Mints the frame's measured targeting-key edges into the cockpit queue.
///
/// Runs in `PreUpdate` after [`collect_mission_cockpit_actions`], so the
/// minted edges sit behind the frame's delivered edges in the one queue the
/// fixed-tick publisher drains. A press during a frame the player has no
/// control is minted the same way and cleared with every other action by
/// the publisher's own control gate — the original's table is only the
/// mapping, not a second authority.
///
/// A letter whose press minted a command is **consumed**: its
/// `just_pressed` edge is cleared so no raw-key consumer runs the press a
/// second time. The playtest's own meta layer reads `R` from the keyboard
/// state directly (`crate::playtest`'s `meta_controls` → reset →
/// [`PlaytestState::resets`], which [`mission_host_restart`] watches) — an
/// uncleared `R` would select the next non-aircraft target *and* restart
/// the mission on one press.
///
/// [`collect_mission_cockpit_actions`]: super::weapons::collect_mission_cockpit_actions
/// [`PlaytestState::resets`]: crate::playtest::PlaytestState::resets
/// [`mission_host_restart`]: super::host::mission_host_restart
pub fn mint_mission_target_keys(world: &mut World) {
    let Some(keys) = world.get_resource::<ButtonInput<KeyCode>>() else {
        return;
    };
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let mut minted = Vec::new();
    let mut consumed = Vec::new();
    for code in [
        KeyCode::KeyQ,
        KeyCode::KeyW,
        KeyCode::KeyE,
        KeyCode::KeyR,
        KeyCode::KeyT,
    ] {
        if !keys.just_pressed(code) {
            continue;
        }
        let Some(key) = key_from_bevy(code) else {
            continue;
        };
        if let Some(command) = original_target_command(key, shift, ctrl) {
            minted.push(Action::Flight(command));
            consumed.push(code);
        }
    }
    if minted.is_empty() {
        return;
    }
    world
        .get_resource_or_insert_with(MissionCockpitActions::default)
        .actions
        .extend(minted);
    if let Some(mut keys) = world.get_resource_mut::<ButtonInput<KeyCode>>() {
        for code in consumed {
            keys.clear_just_pressed(code);
        }
    }
}

/// The mission's action map: the playtest's designed defaults re-laid to the
/// original's own keyboard layout for the rows this task needs.
///
/// The playtest map binds `W`/`Q`/`E` to the pitch and roll axes, `T` to a
/// designed target cycle and `R`'s throttle step to `Left Shift` — every one
/// of them a key the original binds to a targeting command or uses as a
/// targeting-chord modifier. This map frees exactly those sources:
///
/// * the flight axes move to the **arrow keys** — the original's own pitch
///   and roll rows (commands 1–4, scancodes `0xc8`/`0xd0`/`0xcb`/`0xcd`), at
///   the playtest map's disclosed [`KEY_DEFLECTION`];
/// * the throttle step the playtest moved to `Left Shift` moves to `C`,
///   unbound in both the original's table and the designed map, so `Shift`
///   stays a pure modifier;
/// * secondary fire moves from `Left Control` to `X` — the original's own
///   row for the same command (command 20, scancode `0x2d`), so `Ctrl` stays
///   a pure modifier;
/// * `Q`/`W`/`E`/`R`/`T` carry nothing: their targeting edges are minted by
///   [`mint_mission_target_keys`] with the measured modifier matching, which
///   a [`BindingSource`] cannot express.
///
/// Everything else — `A`/`D` yaw, `F` throttle down, `Space` guns, digits,
/// the UI keys — is the playtest's designed map, disclosed as designed.
///
/// # Panics
///
/// Never in practice: the edits remove a source before rebinding it, so the
/// validated playtest map stays conflict-free.
#[must_use]
pub fn mission_action_map() -> ActionMap {
    let freed = |source: BindingSource| {
        !matches!(
            source,
            BindingSource::Key(
                Key::W
                    | Key::Q
                    | Key::E
                    | Key::R
                    | Key::T
                    | Key::ArrowLeft
                    | Key::ArrowRight
                    | Key::LeftShift
                    | Key::LeftControl
                    | Key::X
            )
        )
    };
    let mut bindings: Vec<Binding> = playtest_action_map()
        .bindings()
        .iter()
        .copied()
        .filter(|binding| freed(binding.source))
        .collect();
    let axis = |key, command, scale| Binding {
        source: BindingSource::Key(key),
        target: BindingTarget::Axis {
            command,
            scale: scale * KEY_DEFLECTION,
        },
    };
    let edge = |key, command| Binding {
        source: BindingSource::Key(key),
        target: BindingTarget::Command(command),
    };
    // The original's own flight layout: up/down arrows pitch, left/right
    // roll (measured default table commands 1–4).
    bindings.push(axis(Key::ArrowUp, FlightCommand::Pitch, -1.0));
    bindings.push(axis(Key::ArrowDown, FlightCommand::Pitch, 1.0));
    bindings.push(axis(Key::ArrowLeft, FlightCommand::Roll, -1.0));
    bindings.push(axis(Key::ArrowRight, FlightCommand::Roll, 1.0));
    bindings.push(edge(Key::C, FlightCommand::ThrottleStepUp));
    bindings.push(edge(Key::X, FlightCommand::FireSecondary));
    ActionMap::try_new(bindings).expect("the mission action map stays conflict free")
}

/// The finite world position of a rendered transform, or `None` when it is
/// not a finite point.
fn finite_world(at: Vec3) -> Option<WorldPosition> {
    WorldPosition::try_new([f64::from(at.x), f64::from(at.y), f64::from(at.z)]).ok()
}

/// Writes every rostered actor's live pose into the targeting store's
/// records, ahead of the tick's selection pass.
///
/// The store's records are the roster's own — `roster_mission_zeppelins`,
/// `roster_mission_ground_targets` and the AI arming pass each stamp the
/// *bind* pose at registration and never move it. A record that never moves
/// reports a stale bearing, distance and under-reticule test: an `E` cycle
/// would rank by where actors spawned rather than where they are, `Q`
/// would cone-test a place the actor left, and the ring's distance gate
/// would read a range measured from the launch point. One pass per
/// controlled tick writes each entity's rendered pose — the same frame the
/// camera and the indicator read, since the mission session has no
/// [`crate::origin::WorldOrigin`] rebase — so the selection pass runs on
/// where the actors actually are.
///
/// An entity whose actor the roster passes have not registered yet is
/// their refusal to name, not this pass's; nothing is invented for it.
pub fn sync_mission_target_poses(world: &mut World) {
    let Some(targeting) = world.get_resource::<crate::targeting::TargetingSession>() else {
        return;
    };
    let generation = targeting.generation();
    let player = world
        .get_resource::<MissionWeapons>()
        .map(|weapons| weapons.player());

    let mut poses: Vec<(ActorId, WorldPosition)> = Vec::new();
    if let Some(player_actor) = player {
        let mut bodies = world.query_filtered::<&GlobalTransform, With<MissionPlayerBody>>();
        if let Some(position) = bodies
            .iter(world)
            .next()
            .and_then(|transform| finite_world(transform.translation()))
        {
            poses.push((player_actor, position));
        }
    }
    {
        let mut aircraft = world.query::<(&ScriptedAircraftActor, &GlobalTransform)>();
        for (actor, transform) in aircraft.iter(world) {
            if actor.generation == generation
                && let Some(position) = finite_world(transform.translation())
            {
                poses.push((actor.damage_actor, position));
            }
        }
    }
    {
        let mut zeppelins = world.query::<(&MissionZeppelinActor, &GlobalTransform)>();
        for (actor, transform) in zeppelins.iter(world) {
            if actor.generation == generation
                && let Some(position) = finite_world(transform.translation())
            {
                poses.push((actor.damage_actor, position));
            }
        }
    }
    {
        let mut grounds = world.query::<&MissionGroundTarget>();
        for target in grounds.iter(world) {
            if target.generation == generation
                && let Ok(position) = WorldPosition::try_new(target.position_m)
            {
                poses.push((target.damage_actor, position));
            }
        }
    }
    if poses.is_empty() {
        return;
    }
    let mut targeting = world.resource_mut::<crate::targeting::TargetingSession>();
    let store = targeting.store_mut();
    for (actor, position) in poses {
        let _ = store.set_pose(actor, position);
    }
}

/// The crosshair ray the reticle-select action resolves against: the scene
/// camera's forward axis through the viewport centre.
///
/// The cone is the mission's designed crosshair cone — the same
/// `π/18` radians [`super::weapons::designed_target_rules`] declares; the
/// original's acceptance angle is unmeasured and nothing here pretends to
/// have it. The occluded set is empty: no occlusion producer exists yet, so
/// an under-reticule select is cone-and-distance only, disclosed rather than
/// invented.
#[must_use]
pub fn mission_crosshair(world: &mut World) -> Option<CrosshairQuery> {
    let mut cameras = world.query_filtered::<
        &GlobalTransform,
        (With<Camera3d>, Without<crate::render::OverlayCamera>),
    >();
    let transform = cameras.iter(world).next()?;
    let at = transform.translation();
    let origin =
        WorldPosition::try_new([f64::from(at.x), f64::from(at.y), f64::from(at.z)]).ok()?;
    let forward = transform.forward();
    let direction = UnitVec3::try_new([
        f64::from(forward.x),
        f64::from(forward.y),
        f64::from(forward.z),
    ])
    .ok()?;
    CrosshairQuery::try_new(
        origin,
        direction,
        Radians(std::f64::consts::PI / 18.0),
        BTreeSet::new(),
    )
    .ok()
}
