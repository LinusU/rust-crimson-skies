//! F21-B: the spyglass rig, and AC02 — "destroy or switch the spyglass
//! target mid-frame without stale entity access".
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-B`, non-negotiable behavior 3. Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! The selection in these tests is not hand-written: a real targeting session
//! is bound to entities, the session's own command edges select, the real
//! damage entry records the destruction, and `apply_target_consumers`
//! publishes the [`SpyglassReadout`] the rig reads. That chain matters — the
//! question AC02 asks is whether the rig can be left describing a target that
//! targeting has already dropped, and only the real producer can drop one.
//!
//! What "no stale entity access" means here, and how it is checked:
//!
//! * `CameraRig::resolve` takes a [`RigInputs`] and a published
//!   [`SpyglassReadout`]. Neither can reach an ECS entity, so the rig has no
//!   handle to dereference after a despawn, and the rig's own state holds an
//!   [`ActorId`] and nothing else.
//! * After the target is destroyed the frame reports **no** aim, and the rig's
//!   `framed_target()` is `None` — in the same frame that reports the clear.
//! * A published view *older* than one the rig already consumed is refused by
//!   name, so a consumer cannot feed the rig the view it read before the kill
//!   and put the magnification back up.
//! * A target the camera cannot be aimed at refuses *and* drops the framed
//!   actor, so a caller that keeps the last published frame after an error
//!   still cannot read a live magnified actor.

use std::time::Duration;

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use cs_app::camera::{
    CameraRig, RigAimError, RigError, RigFrame, RigInputs, SmoothingState, ViewRig,
    lower_camera_modes,
};
use cs_app::origin::OriginChange;
use cs_app::scene::SceneGeneration;
use cs_app::targeting::{
    TargetConsumers, TargetableBinding, TargetableState, TargetingSession, apply_selection_edges,
    apply_target_consumers, lower_rules, lower_selection_actions, sync_targetable_roster,
};
use cs_content::cameras::{AspectRatio, CameraModeKind, declared_synthetic_camera_modes};
use cs_content::target_rules::{
    declared_synthetic_selection_actions, declared_synthetic_target_rules,
};
use cs_sim::damage::ActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, Origin};
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, Radians, WorldPosition};

use crate::common::{aircraft_pose, assert_close, declared_mode_with, declared_set, placement};

/// The frame rate the rig tests run at. Nothing in the spyglass scenario
/// depends on it; a fixed one keeps the smoothing out of the way.
const FRAME: Duration = Duration::from_millis(16);

fn session() -> SessionId {
    SessionId::new(21).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(),
        serial,
    }
}

fn factions() -> (ContentId, ContentId) {
    (
        cs_sim::targeting::synthetic_player_faction(),
        cs_sim::targeting::synthetic_raider_faction(),
    )
}

/// A targeting session with the observer at the origin and three raiders
/// spread along `+X`, `+Y` and `-Z`.
fn bound_world(generation: SceneGeneration) -> World {
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let actions = lower_selection_actions(&declared_synthetic_selection_actions())
        .expect("the actions lower");
    let (player, raider) = factions();
    let mut world = World::new();
    world.insert_resource(TargetingSession::new(
        session(),
        lowered,
        actions,
        declared.subject().clone(),
        generation,
    ));
    for (serial, faction, position) in [
        (1_u64, player.clone(), [0.0, 0.0, 0.0]),
        (9, raider.clone(), [900.0, 0.0, 0.0]),
        (2, raider.clone(), [0.0, 900.0, 0.0]),
        (5, raider, [0.0, 0.0, -900.0]),
    ] {
        world.spawn((
            TargetableBinding {
                actor: actor(serial),
                rules: declared.subject().clone(),
                generation,
            },
            TargetableState::aircraft(faction, world_of(position)),
        ));
    }
    world
}

fn world_of(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

/// The entity an actor is bound to, so a test can despawn it.
fn entity_of(world: &mut World, actor: ActorId) -> Entity {
    let mut query = world.query::<(Entity, &TargetableBinding)>();
    query
        .iter(world)
        .find(|(_, binding)| binding.actor == actor)
        .map(|(entity, _)| entity)
        .expect("the actor is bound to an entity")
}

/// The next target the session's own bound edge picks.
fn select_next(world: &mut World, at: u64) -> ActorId {
    apply_selection_edges(
        world,
        actor(1),
        &[Action::Flight(FlightCommand::TargetNext)],
        cs_sim::targeting::SelectionFrame::at(Tick(at)),
    )
    .expect("registered")
    .selection
    .expect("a raider is selected")
}

/// Publishes the consumer views and returns the spyglass one the rig reads.
fn publish(world: &mut World, at: u64) -> cs_app::targeting::SpyglassReadout {
    apply_target_consumers(world, actor(1), Tick(at))
        .expect("registered")
        .spyglass
        .expect("a spyglass view is published")
}

/// One frame of rig inputs for a stationary observer looking down `-Z`.
fn inputs<'a>(at: u64, spyglass: Option<&'a cs_app::targeting::SpyglassReadout>) -> RigInputs<'a> {
    RigInputs {
        at: Tick(at),
        subject: actor(1),
        aircraft: aircraft_pose([0.0, 0.0, 0.0], Quaternion::IDENTITY),
        aspect: AspectRatio::SIXTEEN_NINE,
        elapsed: FRAME,
        look: None,
        spyglass,
        origin_change: OriginChange::Rebase,
    }
}

/// A rig in the spyglass view over the fixture's declared modes.
fn spyglass_rig() -> CameraRig {
    let mut rig = CameraRig::new(
        lower_camera_modes(&declared_synthetic_camera_modes()).expect("the fixture lowers"),
    )
    .expect("the fixture's default has a rig");
    rig.set_rig(ViewRig::Spyglass)
        .expect("the spyglass rig exists");
    rig
}

/// A spyglass rig that follows instantly.
///
/// A 16 ms frame at this response rate has `1 − exp(−k·dt)` equal to exactly 1
/// in f64, so a frame *is* the desired pose. That is what lets a test assert
/// an exact aim direction after a switch: at a realistic rate the camera is
/// still easing toward a target that moved, which is correct behaviour and
/// would make the assertion measure the smoothing law instead of the aim.
fn instant_spyglass_rig() -> CameraRig {
    let mut rig = CameraRig::with_response(
        lower_camera_modes(&declared_synthetic_camera_modes()).expect("the fixture lowers"),
        1.0e9,
    )
    .expect("a finite response rate");
    rig.set_rig(ViewRig::Spyglass)
        .expect("the spyglass rig exists");
    rig
}

/// AC02, first half: the spyglass shows the selected target, centred, with the
/// spyglass mode's **own** frustum and magnification.
///
/// The failure this discriminates: a spyglass that frames with the cockpit's
/// field of view and clipping planes magnifies nothing and clips its own
/// target away (F21 non-negotiable behavior 3, "obeys its own near/far
/// rendering requirements").
#[test]
fn accept_f21_b_the_spyglass_frames_the_selected_target_with_its_own_frustum() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    let mut rig = spyglass_rig();

    // Select through the session's own edge, then publish the views the rig
    // reads. `TargetNext` from the player picks the equal-distance raider in
    // roster order, which is raider 9 at `[900, 0, 0]`.
    let selected = select_next(&mut world, 20);
    let readout = publish(&mut world, 21);
    assert_eq!(
        readout.target.as_ref().map(|target| target.actor),
        Some(selected)
    );
    assert!(readout.cleared.is_none());

    let frame = rig
        .resolve(&inputs(21, Some(&readout)))
        .expect("the rig frames");
    assert_eq!(frame.rig, ViewRig::Spyglass);
    assert_eq!(frame.mode, CameraModeKind::Spyglass);
    assert_eq!(rig.framed_target(), Some(selected));

    let position = readout.target.as_ref().expect("a selected target").position;
    let aim = frame.spyglass.expect("a spyglass frame reports its aim");
    assert_eq!(aim.actor, Some(selected), "the selected actor is magnified");
    assert!(aim.has_target());
    assert_eq!(
        aim.aim,
        Some(position),
        "the aim is the target's own position"
    );
    assert_eq!(aim.dropped, None, "nothing was framed before this frame");
    assert!(!aim.switched);

    // The aim is real: the target is at the centre of the viewport, through
    // the frame's own projection and aspect. (The fixture's raiders are
    // spread along `+X`, `+Y` and `-Z`, so this also covers a target straight
    // overhead, where the canonical up axis is parallel to the view and the
    // rig has to fall back to the aircraft's own axes for the roll.)
    let framing = frame
        .framing_of(position)
        .expect("the target is in front of the eye");
    assert_close(framing.x(), 0.0, 1e-12, "the magnified target is centred");
    assert_close(framing.y(), 0.0, 1e-12, "on both axes");

    // And it is the spyglass's frustum: 20° vertical, 1 m near, 20 km far,
    // 4x — none of which is the cockpit's.
    assert_close(
        frame.projection.vertical_fov().0,
        Radians(20.0_f64.to_radians()).0,
        1e-12,
        "the spyglass uses its own field of view",
    );
    assert_eq!(frame.projection.near_m().0, 1.0);
    assert_eq!(frame.projection.far_m().0, 20_000.0);
    assert_eq!(frame.magnification.value(), 4.0);

    // The rig reads the view; it does not own it. The published consumers are
    // byte-identical before and after a frame, which is what "must not change
    // target authority" means for a consumer with no store handle.
    let before = world.resource::<TargetConsumers>().clone();
    let _ = rig
        .resolve(&inputs(21, Some(&readout)))
        .expect("a second frame at the same tick resolves");
    assert_eq!(
        world.resource::<TargetConsumers>(),
        &before,
        "resolving a frame changed nothing targeting published"
    );
    assert_eq!(
        world.resource::<TargetingSession>().selection().current(),
        Some(selected),
        "and the session's own selection is still the one targeting chose"
    );
}

/// AC02, second half: the target is destroyed *after* the rig framed it, and
/// the next frame must describe no target at all.
///
/// The failure this discriminates: a rig that caches the target's position —
/// or that keeps the last frame's aim — leaves the magnification on a wreck,
/// which is the exact thing F30-C's `cleared` record exists to prevent.
#[test]
fn accept_f21_b_destroying_the_spyglass_target_drops_the_magnification_in_the_same_frame() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    let mut rig = spyglass_rig();

    let selected = select_next(&mut world, 20);
    let readout = publish(&mut world, 21);
    let framed = rig
        .resolve(&inputs(21, Some(&readout)))
        .expect("the rig frames");
    assert_eq!(
        framed.spyglass.expect("an aim").actor,
        Some(selected),
        "the target is framed before it dies"
    );

    // The real roster observes the entity leaving the world, exactly as a
    // destroy, a despawn or mission accounting would.
    let entity = entity_of(&mut world, selected);
    world.despawn(entity);
    let removed = sync_targetable_roster(&mut world);
    assert_eq!(removed.removed, 1, "the roster unregistered the actor");
    assert!(
        !world
            .resource::<TargetingSession>()
            .store()
            .is_registered(&selected),
        "and the actor is no longer targetable"
    );

    let readout = publish(&mut world, 22);
    assert_eq!(
        readout.target, None,
        "targeting no longer describes the actor"
    );
    let cleared = readout.cleared.expect("and it names the clear");

    let frame = rig
        .resolve(&inputs(22, Some(&readout)))
        .expect("the rig resolves");
    let aim = frame.spyglass.expect("a spyglass frame reports its aim");
    assert_eq!(aim.actor, None, "nothing is magnified any more");
    assert_eq!(aim.aim, None, "and no position of the dead actor is framed");
    assert!(!aim.has_target());
    assert_eq!(
        aim.cleared,
        Some(cleared),
        "the clear is carried in the same frame"
    );
    assert_eq!(
        aim.dropped,
        Some(selected),
        "and the frame says which actor went"
    );
    assert_eq!(
        rig.framed_target(),
        None,
        "the rig's own state agrees: no live actor is framed"
    );

    // The view stayed the spyglass view, so its own clipping planes and
    // magnification are still the ones in force — the rig does not silently
    // swap the mode to hide the miss.
    assert_eq!(frame.mode, CameraModeKind::Spyglass);
    assert_eq!(frame.projection.near_m().0, 1.0);
    assert_eq!(frame.magnification.value(), 4.0);
}

/// AC02, the switch: the selection moves to another actor between frames. The
/// rig re-aims at the new actor and names the old one as dropped — it does not
/// keep framing where it was.
#[test]
fn accept_f21_b_switching_the_spyglass_target_reaims_and_names_the_dropped_actor() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    let mut rig = instant_spyglass_rig();

    let first = select_next(&mut world, 20);
    let readout = publish(&mut world, 21);
    rig.resolve(&inputs(21, Some(&readout)))
        .expect("the rig frames");

    // Walk the selection along to a different raider, through the session's
    // own edges, and publish again at the next tick.
    let second = select_next(&mut world, 22);
    assert_ne!(second, first, "the cycle moved to another actor");
    let readout = publish(&mut world, 23);
    assert_eq!(
        readout.target.as_ref().map(|target| target.actor),
        Some(second)
    );

    let frame = rig
        .resolve(&inputs(23, Some(&readout)))
        .expect("the rig re-aims");
    let aim = frame.spyglass.expect("an aim");
    assert_eq!(aim.actor, Some(second));
    assert!(
        aim.switched,
        "the framed actor changed rather than appeared"
    );
    assert_eq!(aim.dropped, Some(first), "the previous actor is named");
    assert_eq!(aim.cleared, None, "a switch is not a clear");
    assert_eq!(
        aim.aim,
        readout.target.as_ref().map(|target| target.position),
        "the aim is the new actor's own position"
    );
    let framing = frame
        .framing_of(aim.aim.expect("an aim position"))
        .expect("the new target is in front of the eye");
    assert_close(framing.x(), 0.0, 1e-12, "the new target is centred");
    assert_close(framing.y(), 0.0, 1e-12, "on both axes");
    assert_eq!(rig.framed_target(), Some(second));
}

/// AC02's stale half: a view published *before* a death cannot be replayed
/// into the rig to bring the magnification back up.
///
/// The failure this discriminates: a consumer that reuses the view it read
/// before the kill — the natural thing for a renderer that publishes at fixed
/// ticks and draws between them — would frame a wreck. Ticks are not
/// wall time, so re-reading the *same* tick every frame is legal; a *smaller*
/// one is not.
#[test]
fn accept_f21_b_a_spyglass_view_older_than_one_already_consumed_is_refused() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    let mut rig = spyglass_rig();

    let selected = select_next(&mut world, 20);
    let stale = publish(&mut world, 21);
    rig.resolve(&inputs(21, Some(&stale)))
        .expect("the rig frames");
    assert_eq!(rig.framed_target(), Some(selected));

    // The same tick again is fine: a render frame between two fixed ticks
    // legitimately re-reads the view it already has.
    let again = rig
        .resolve(&inputs(21, Some(&stale)))
        .expect("the same tick resolves again");
    assert_eq!(again.spyglass.expect("an aim").actor, Some(selected));

    // An older tick is not: this is the view from before a clear, and it must
    // not resurrect the magnification.
    let older = cs_app::targeting::SpyglassReadout {
        at: Tick(20),
        target: stale.target,
        cleared: None,
    };
    assert_eq!(
        rig.resolve(&inputs(22, Some(&older))),
        Err(RigError::StaleSpyglassReadout {
            read_at: Tick(20),
            consumed_through: Tick(21),
        }),
        "a view older than one already consumed is refused by name"
    );
    assert_eq!(
        rig.framed_target(),
        None,
        "and the refusal leaves no magnified actor behind"
    );

    // A newer tick is consumed again, so the rule is about ordering and not
    // about refusing every second view.
    let newer = publish(&mut world, 23);
    let frame = rig
        .resolve(&inputs(23, Some(&newer)))
        .expect("a newer view resolves");
    assert_eq!(
        frame.spyglass.expect("an aim").actor,
        newer.target.as_ref().map(|target| target.actor)
    );
}

/// An invalid target is handled rather than framed: a selection at the
/// camera's own position has no direction to aim at, and the rig refuses
/// *and* drops the actor so a caller holding the last frame still reads no
/// magnified actor.
#[test]
fn accept_f21_b_an_unaimable_spyglass_target_is_refused_and_not_framed() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    let mut rig = spyglass_rig();

    let selected = select_next(&mut world, 20);
    let readout = publish(&mut world, 21);
    rig.resolve(&inputs(21, Some(&readout)))
        .expect("the rig frames");
    assert_eq!(rig.framed_target(), Some(selected));

    // Walk the target onto the camera's own position: there is no direction
    // between a point and itself.
    let coincident = cs_app::targeting::SpyglassReadout {
        at: Tick(22),
        target: Some(cs_app::targeting::SpyglassTarget {
            actor: selected,
            class: cs_sim::targeting::TargetClass::Aircraft,
            allegiance: None,
            hostile: true,
            threatening: false,
            objective: false,
            position: world_of([0.0, 0.0, 0.0]),
            distance: cs_types::space::Meters(0.0),
        }),
        cleared: None,
    };
    assert_eq!(
        rig.resolve(&inputs(22, Some(&coincident))),
        Err(RigError::UnaimableTarget {
            actor: selected,
            reason: RigAimError::CoincidentWithCamera,
        }),
        "an unaimable target is refused by name"
    );
    assert_eq!(
        rig.framed_target(),
        None,
        "and the refusal leaves no live actor framed"
    );
}

/// F21 non-negotiable behavior 3: "it must not change target authority or
/// fire direction unless evidence says so". A mode that declares it does *not*
/// track a target gets no target at all — the rig follows the declaration
/// rather than the selection.
#[test]
fn accept_f21_b_a_spyglass_mode_that_declares_no_target_tracking_frames_nothing() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);

    // A set declares one mode per kind, so the mode under test is the only
    // spyglass the rig can ever resolve.
    let untracked = declared_set(
        vec![declared_mode_with(
            CameraModeKind::Spyglass,
            placement(CameraModeKind::Spyglass),
            false,
        )],
        CameraModeKind::Spyglass,
    );
    let mut rig =
        CameraRig::new(lower_camera_modes(&untracked).expect("the set lowers")).expect("a rig");

    let selected = select_next(&mut world, 20);
    let readout = publish(&mut world, 21);
    assert_eq!(
        readout.target.as_ref().map(|target| target.actor),
        Some(selected),
        "the session really does have a selection"
    );

    let frame = rig
        .resolve(&inputs(21, Some(&readout)))
        .expect("the rig resolves");
    let aim = frame.spyglass.expect("a spyglass frame reports its aim");
    assert_eq!(
        aim.actor, None,
        "a mode that declares no target tracking frames nothing"
    );
    assert_eq!(aim.aim, None);
    assert_eq!(rig.framed_target(), None);
    assert_eq!(
        frame.magnification.value(),
        4.0,
        "the view is still the spyglass"
    );
}

/// No published view at all — a session that has not reached its consumer
/// pass yet — is the same statement as "nothing is selected", and it drops a
/// framed actor rather than keeping it.
#[test]
fn accept_f21_b_a_missing_spyglass_view_drops_a_framed_actor() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    let mut rig = spyglass_rig();

    let selected = select_next(&mut world, 20);
    let readout = publish(&mut world, 21);
    rig.resolve(&inputs(21, Some(&readout)))
        .expect("the rig frames");
    assert_eq!(rig.framed_target(), Some(selected));

    let frame = rig.resolve(&inputs(22, None)).expect("the rig resolves");
    let aim = frame.spyglass.expect("an aim");
    assert_eq!(aim.actor, None, "no view means no magnified actor");
    assert_eq!(aim.dropped, Some(selected));
    assert_eq!(rig.framed_target(), None);
    assert_eq!(frame.smoothing, SmoothingState::Tracking);
}

/// A plane swap is a plane swap for the spyglass too: the new aircraft starts
/// with no magnified target, and the new observer's first view — which may
/// carry a *lower* tick, because a new session generation restarts the clock —
/// is consumed rather than refused as stale.
#[test]
fn accept_f21_b_a_plane_swap_drops_the_framed_actor_and_restarts_the_readout_tick() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    let mut rig = spyglass_rig();

    let selected = select_next(&mut world, 20);
    let readout = publish(&mut world, 21);
    rig.resolve(&inputs(21, Some(&readout)))
        .expect("the rig frames");
    assert_eq!(rig.framed_target(), Some(selected));

    // The same published view, read by a different aircraft.
    let mut swapped = inputs(22, Some(&readout));
    swapped.subject = actor(5);
    let frame = rig.resolve(&swapped).expect("the new aircraft resolves");
    assert_eq!(
        frame.spyglass.expect("an aim").actor,
        Some(selected),
        "and re-reads the view at the tick it was published for"
    );
    assert_eq!(rig.subject(), Some(actor(5)));

    // The swap itself dropped the previous aircraft's target, before this
    // frame's view arrived: a rig that kept it would be showing the previous
    // pilot a target the new one has not selected.
    let mut no_view = inputs(22, None);
    no_view.subject = actor(5);
    let frame = rig.resolve(&no_view).expect("the new aircraft resolves");
    assert_eq!(frame.spyglass.expect("an aim").actor, None);
    assert_eq!(rig.framed_target(), None);
    assert_eq!(
        frame.smoothing,
        SmoothingState::Tracking,
        "and the second frame for the new aircraft tracks rather than reseats again"
    );
}

/// The rig's origin is the mode set's, so a session can tell a verified
/// original binding from development content before it draws anything. This is
/// the fixture half of F21 non-negotiable behavior 1.
#[test]
fn accept_f21_b_the_rig_reports_its_mode_sets_origin() {
    let rig = spyglass_rig();
    assert_eq!(rig.origin(), &Origin::SyntheticFixture);
    assert!(
        !rig.origin().is_original(),
        "the synthetic mode set is never original installation data"
    );
    assert!(
        rig.cockpit_binding().is_none(),
        "the spyglass claims no binding"
    );
    assert_eq!(rig.mode(), CameraModeKind::Spyglass);
}

/// A frame is a value a consumer can hold without borrowing the rig, and it
/// carries everything needed to place a reticle: the pose, the projection and
/// the aspect were all resolved together.
#[test]
fn accept_f21_b_a_frame_carries_its_own_pose_projection_and_aspect() {
    let generation = SceneGeneration::default().next();
    let mut world = bound_world(generation);
    sync_targetable_roster(&mut world);
    let mut rig = spyglass_rig();

    let readout = publish(&mut world, 21);
    let frame: RigFrame = rig
        .resolve(&inputs(21, Some(&readout)))
        .expect("the rig resolves");
    let held = frame;
    assert_eq!(held, frame, "a frame is a value, not a view into the rig");
    assert_eq!(held.aspect, AspectRatio::SIXTEEN_NINE);
    assert_eq!(
        held.pose.position(),
        world_of([0.0, 0.0, 0.0]),
        "the spyglass eye is the body origin for this fixture"
    );
    assert_eq!(held.at, Tick(21));
    assert_close(
        held.projection.vertical_fov_at(held.aspect).0,
        held.projection.vertical_fov().0,
        1e-12,
        "the aspect-correct rule keeps the authored vertical field of view",
    );
    assert_close(
        held.projection.horizontal_fov_at(held.aspect).0,
        2.0 * (Radians(10.0_f64.to_radians()).0.tan() * AspectRatio::SIXTEEN_NINE.value()).atan(),
        1e-12,
        "and the horizontal one grows with the aspect",
    );
}
