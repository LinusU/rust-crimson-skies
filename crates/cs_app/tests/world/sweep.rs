//! AC01: a swept body through a narrow synthetic arch at high speed, without
//! a collision mismatch — covered from both sides.
//!
//! * a probe through the opening must travel the whole way untouched, so no
//!   collider was invented where the record draws a hole (no phantom wall);
//! * a probe aimed at a leg must be stopped by it, so no simplification
//!   closed or opened the gap by the wrong amount (no ghost opening);
//! * the visual entity, the collider entity and the authored instance must
//!   carry the same transform and the same box, so the two cannot disagree
//!   about *where* the opening is.

use bevy::prelude::{GlobalTransform, Time, Transform, Vec3};
use bevy::time::Fixed;
use cs_app::world::{
    SkipReason, WorldFixture, canonical_matrix,
    fixture::{ARCH_HALF_X_M, WATER_HALF_M, WATER_POS_M},
};
use cs_content::world::{SectorId, WorldCollisionRole, WorldObjectId};

use crate::common;

/// How long the probes are flown: far enough to clear the arch entirely.
const TICKS: u64 = 15;

/// Half the tolerance, in meters, for comparing a runtime transform against
/// the authored record: the f64 record narrows to the runtime's f32 frame.
const POSE_TOLERANCE_M: f32 = 1e-3;

/// **AC01 (positive half):** a 0.5 m box crosses the narrow opening at
/// 400 m/s — more than three meters per tick, seven times the arch's own
/// thickness — and is never touched.
///
/// Observable failure if a collider is built where the record draws the hole
/// (a "simplification" that filled the opening): the swept sweep finds it,
/// the contact log names the filler, and the probe's straight-line distance
/// is short. Observable failure if the spawn put any collider at a wrong
/// pose: the probe clips a leg it should clear.
#[test]
fn accept_f18_a_swept_body_flies_through_the_narrow_arch_at_high_speed() {
    let mut fixture = common::through_opening();
    let start = fixture.probe_position().expect("the probe was spawned");
    let dt = fixture
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f32();

    fixture.step(TICKS);

    assert!(
        fixture.contacts().is_empty(),
        "a body through the opening must touch nothing, but the log recorded {:?}",
        fixture
            .contacts()
            .iter()
            .map(|contact| contact.object.as_str())
            .collect::<Vec<_>>()
    );

    let end = fixture.probe_position().expect("the probe still exists");
    assert!(
        end.x > 10.0,
        "the probe must clear the arch, it ended at {end:?}"
    );

    let expected = start + Vec3::X * common::PROBE_SPEED_M_S as f32 * (TICKS as f32 * dt);
    let drift = (end - expected).length();
    assert!(
        drift < 0.01,
        "an untouched sweep must travel exactly `speed * time`; drifted {drift} m to {end:?}"
    );

    let velocity = fixture.probe_velocity().expect("the probe still exists");
    assert!(
        (velocity.x - common::PROBE_SPEED_M_S as f32).abs() < 1.0,
        "nothing should have slowed the probe, velocity is {velocity:?}"
    );
}

/// **AC01 (failure half):** the same body aimed at the right arch leg is
/// stopped by it.
///
/// The start position is chosen so that two consecutive tick samples straddle
/// the 1 m thick leg with a gap larger than probe-plus-wall: a discrete test
/// would step straight over it. The probe also carries
/// `SpeculativeMargin::ZERO` (see `cs_app::world::fixture::spawn_swept_probe`),
/// so nothing but the swept sweep can stop it.
///
/// Observable failure if `SweptCcd` is not wired into the spawned probe, if
/// the leg's collider is missing, or if its box was built at the wrong size
/// or place — the probe tunnels through solid geometry and the contact log
/// stays empty.
#[test]
fn accept_f18_a_a_probe_aimed_at_an_arch_leg_is_stopped_by_it() {
    let mut fixture = common::into_leg();
    let start = fixture.probe_position().expect("the probe was spawned");

    // Two discrete samples either side of the wall, with nothing between.
    let step = common::PROBE_SPEED_M_S as f32 / cs_app::physics::BASELINE_FIXED_HZ as f32;
    let before_wall = start.x + step * 8.0;
    let after_wall = start.x + step * 9.0;
    assert!(
        before_wall < -ARCH_HALF_X_M as f32 && after_wall > ARCH_HALF_X_M as f32,
        "the fixture must actually straddle the wall: {before_wall} .. {after_wall}"
    );

    fixture.step(TICKS);

    let leg = WorldObjectId::new("arch.leg_right").expect("the id is valid");
    let touched: Vec<&WorldObjectId> = fixture
        .contacts()
        .iter()
        .map(|contact| &contact.object)
        .collect();
    assert!(
        touched.contains(&&leg),
        "the swept probe must collide with the leg it flew into; contacts: {:?}",
        touched.iter().map(|id| id.as_str()).collect::<Vec<_>>()
    );

    let contact = fixture
        .contacts()
        .iter()
        .find(|contact| contact.object == leg)
        .expect("the leg contact is in the log");
    assert_eq!(
        contact.role,
        WorldCollisionRole::Solid,
        "the leg must report the role its record declared"
    );
    assert_eq!(
        contact.sectors,
        [SectorId::new("arch").expect("the id is valid")],
        "the contact must name the sector the object belongs to"
    );
    assert_eq!(
        contact.other,
        fixture.probe().expect("the probe was spawned"),
        "the contact must name the body that hit the world"
    );

    let end = fixture.probe_position().expect("the probe still exists");
    assert!(
        end.x < 1.0,
        "the probe must never get past the wall, it ended at {end:?}"
    );
    assert!(
        end.x > start.x,
        "the probe must actually reach the wall, it barely moved: {end:?}"
    );
}

/// The visual entity, the collider entity and the authored record agree —
/// transform *and* box — for every instance the world spawned.
///
/// Observable failure if the two entities are built from different
/// derivations: the comparison is against the authored canonical matrix, not
/// against each other, so a spawn that offsets, rotates or rescales only the
/// collider lands outside the tolerance and the mismatch is reported by name.
#[test]
fn accept_f18_a_visual_and_collision_instances_agree_with_the_authored_record() {
    let mut fixture = WorldFixture::arch();
    // One tick so Avian computes the broad-phase bounds of every collider.
    fixture.step(1);

    let spawned = fixture.spawned().clone();
    let definition = fixture.definition().clone();
    assert_eq!(
        spawned.colliders().len(),
        6,
        "six instances carry a built collider: three arch parts, ground, water and the sensor"
    );
    assert!(
        !spawned.skipped().is_empty(),
        "the fixture also carries gaps"
    );

    for collider in spawned.colliders() {
        let object = definition
            .object(&collider.object)
            .expect("a spawned collider names an object of the definition");
        let authored = canonical_matrix(object.transform());

        let visual_entity = spawned
            .visual_for(&collider.object)
            .expect("every collider has a visual twin");
        let visual = fixture
            .world()
            .get::<GlobalTransform>(visual_entity)
            .expect("the visual entity has a global transform");
        assert!(
            visual
                .to_matrix()
                .to_cols_array()
                .iter()
                .zip(authored.to_cols_array())
                .all(|(a, b)| (a - b).abs() <= POSE_TOLERANCE_M),
            "object `{}` visual transform drifted from the authored record: got {:?} want {:?}",
            collider.object,
            visual.to_matrix().to_cols_array(),
            authored.to_cols_array()
        );

        let runtime = fixture
            .world()
            .get::<Transform>(collider.entity)
            .expect("the collider entity has a transform");
        let runtime_matrix = runtime.to_matrix();
        assert!(
            runtime_matrix
                .to_cols_array()
                .iter()
                .zip(authored.to_cols_array())
                .all(|(a, b)| (a - b).abs() <= POSE_TOLERANCE_M),
            "object `{}` collider transform drifted from the authored record",
            collider.object
        );

        // The box itself: read back from the collider the narrow phase
        // uses, so a spawn that built the wrong size cannot pass.
        let authored_half = object
            .known_shape()
            .and_then(|shape| shape.cuboid_half_extents())
            .expect("every spawned collider has a known box");
        let actual_half = common::collider_box_half_extents(fixture.world(), collider.entity);
        assert!(
            (actual_half
                - Vec3::new(
                    authored_half[0] as f32,
                    authored_half[1] as f32,
                    authored_half[2] as f32
                ))
            .abs()
            .max_element()
                < 1e-3,
            "object `{}` collider box is {actual_half:?} but the record says {authored_half:?}",
            collider.object
        );

        // Its placement: the broad-phase bound must sit where the authored
        // matrix puts the box (its size may be inflated by Avian's contact
        // margin, its centre may not move).
        let (expected_min, expected_max) = common::authored_aabb(object.transform(), authored_half);
        let (centre, _) = common::collider_bounds(fixture.world(), collider.entity);
        let expected_centre = (expected_max + expected_min) * 0.5;
        let centre_error = (centre - expected_centre).abs().max_element();
        assert!(
            centre_error < 1e-2,
            "object `{}` collider box is centred {centre_error} m away from the record: got {centre:?} want {expected_centre:?}",
            collider.object
        );
    }
}

/// Water is a bounded patch with a surface role, never an infinite collision
/// plane (F18 non-negotiable behavior 2), and a rotated instance still lands
/// where the record put it.
#[test]
fn accept_f18_a_water_is_a_bounded_patch_and_a_rotated_instance_lands_where_authored() {
    let mut fixture = WorldFixture::arch();
    fixture.step(1);

    let spawned = fixture.spawned().clone();
    let definition = fixture.definition().clone();
    let water = WorldObjectId::new("water.patch").expect("the id is valid");
    let entity = spawned
        .collider_for(&water)
        .expect("the water patch carries a collider");

    let object = definition.object(&water).expect("the water patch exists");
    let authored_half = object
        .known_shape()
        .and_then(|shape| shape.cuboid_half_extents())
        .expect("the water patch has a known box");

    // The collider really is the authored 4x0.1x4 patch: water never becomes
    // a bigger plane than the record draws.
    let actual_half = common::collider_box_half_extents(fixture.world(), entity);
    let expected_half = Vec3::new(
        WATER_HALF_M[0] as f32,
        WATER_HALF_M[1] as f32,
        WATER_HALF_M[2] as f32,
    );
    assert!(
        (actual_half - expected_half).abs().max_element() < 1e-3,
        "the water collider must stay the authored patch, got {actual_half:?} want {expected_half:?}"
    );

    let (centre, half_size) = common::collider_bounds(fixture.world(), entity);
    let expected_centre = Vec3::new(
        WATER_POS_M[0] as f32,
        WATER_POS_M[1] as f32,
        WATER_POS_M[2] as f32,
    );
    assert!(
        (centre - expected_centre).abs().max_element() < 1e-2,
        "the water patch must sit where the record put it, got {centre:?} want {expected_centre:?}"
    );

    // A 4x4 patch rotated by 30 degrees has a half-width of
    // (cos30 + sin30) * 4 ≈ 5.46 m: wider than it is authored, exactly as a
    // rotation implies, and nowhere near world-sized. Avian may inflate the
    // broad-phase bound by its contact margin, never by metres.
    let (expected_min, expected_max) = common::authored_aabb(object.transform(), authored_half);
    assert!(
        (expected_max.x - expected_min.x - 10.928).abs() < 0.01,
        "the reference bounds must be the rotated patch, got {expected_min:?}..{expected_max:?}"
    );
    assert!(
        half_size.x < 5.5 && half_size.y < 0.2 && half_size.z < 5.5,
        "water must stay a bounded patch, its bound half-size is {half_size:?}"
    );
    assert!(
        (half_size.x - 5.464).abs() < 0.01,
        "a rotated patch must grow to the rotated width, got {half_size:?}"
    );

    let contact_free = fixture.contacts().is_empty();
    assert!(
        contact_free,
        "two static world objects overlapping is authoring, not gameplay: it must not be logged"
    );
}

/// Instances the record cannot classify are reported, never defaulted: the
/// sign keeps its visual but is never given a collider, and the hangar keeps
/// its role but is never given a box nobody measured.
#[test]
fn accept_f18_a_unresolved_instances_are_reported_instead_of_guessed() {
    let fixture = WorldFixture::arch();
    let spawned = fixture.spawned();
    let definition = fixture.definition();

    let sign = WorldObjectId::new("sign.unevidenced_role").expect("the id is valid");
    assert!(
        spawned.visual_for(&sign).is_some(),
        "an unevidenced instance is still presented"
    );
    assert!(
        spawned.collider_for(&sign).is_none(),
        "an unevidenced collision role must never become a collider"
    );

    let hangar = WorldObjectId::new("hangar.unevidenced_shape").expect("the id is valid");
    assert!(
        spawned.collider_for(&hangar).is_none(),
        "an unevidenced shape must never become a box someone guessed"
    );

    let reasons: Vec<(WorldObjectId, SkipReason)> = spawned
        .skipped()
        .iter()
        .map(|skipped| (skipped.object.clone(), skipped.reason))
        .collect();
    assert_eq!(
        reasons,
        vec![
            (sign.clone(), SkipReason::UnknownCollisionRole),
            (hangar.clone(), SkipReason::UnknownCollisionShape),
        ],
        "every gap must be listed with the reason it is a gap"
    );

    assert_eq!(
        spawned.non_colliding().to_vec(),
        vec![WorldObjectId::new("banner.non_colliding").expect("the id is valid")],
        "the explicit `None` role is reported as a deliberate answer, not as a skip"
    );

    assert_eq!(definition.unresolved_collision().len(), 1);
    assert_eq!(definition.unresolved_shape().len(), 1);
    assert_eq!(definition.unresolved_surface().len(), 1);
}
