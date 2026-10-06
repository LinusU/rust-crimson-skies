//! Task #709 (`PLAYTEST-NOSE-FIX`) acceptance tests. Prefix: `accept_playtest_nose_`.
//!
//! The owner's playtest reported that "the plane is flying backwards": the drawn
//! `bloodhawk` travelled tail first. The cause was the airframe's **nose reading**
//! — the propeller's position was taken for the nose, and the `bloodhawk` is a
//! rear-propeller layout whose disc sits behind its rudder. The stored nose is
//! `−Z`, measured from the container's own tail surfaces composing aft of the
//! cockpit node in all eleven scene airframes
//! (`docs/findings/2026-10-06-t709-airframe-nose-mapping.md`), and
//! `nose_mapping` is the one rule that lands whatever stored nose an airframe has
//! on `BODY_FORWARD`.
//!
//! Every test here drives production code: `nose_mapping`, the playtest's own
//! `headless_app_with` plus `retail::install`, the production `spawn_aircraft`
//! spawn, and the production `area_graph` reader. The retail half is `#[ignore]`d
//! (`requires CS_GAME_DIR`) and **fails** without the variable; nothing original
//! is committed and frames are written only under `private/`.
//!
//! These tests share the `tests/playtest_retail.rs` binary rather than linking
//! another copy of the engine (the reason #666's `textures` tests do too).

use std::path::PathBuf;

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::math::{Quat, Vec3};
use bevy::mesh::Mesh;
use bevy::prelude::{App, Entity, Handle, Transform, With};
use cs_app::playtest::headless_app_with;
use cs_app::playtest::retail::{self, RetailContent, RetailRequest};
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::playtest_retail::{
    AircraftPart, AircraftPartAsset, PLAYTEST_AREA_NODE_NAME, PLAYTEST_AREA_NODE_SLOT,
    PLAYTEST_WORLD_GROUP, PlaytestConfig, PlaytestError, STORED_AIRCRAFT_NOSE_AXIS, area_graph,
    capture_playtest_views, nose_mapping, playtest_adapter, playtest_app, read_playtest_sources,
    spawn_playtest_scene, teardown_playtest_scene,
};
use cs_sim::flight::BODY_FORWARD;

/// The authored nodes the cruise test reads its two reference points from, and
/// why each is the reference it is: `pf_canopy` is the canopy over the cockpit,
/// and `l_rudder1` is the rudder under the container's own authored `tail` node.
/// Both are drawn bindings of the selected LOD band, so both exist as spawned
/// entities with their composed placement.
const NOSE_REFERENCE_NODE: &str = "pf_canopy";
const TAIL_REFERENCE_NODE: &str = "l_rudder1";

fn forward() -> Vec3 {
    let [x, y, z] = BODY_FORWARD;
    Vec3::new(x as f32, y as f32, z as f32)
}

fn install_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    )
}

fn request() -> RetailRequest {
    RetailRequest::new(install_dir(), None, None).expect("the documented defaults are valid")
}

/// The headless retail free flight: the same plugin, input session, F24 flight
/// driver, Avian world and chase camera as the window, minus window and GPU.
/// The playtest's own Startup runs `spawn_aircraft` over this content.
fn retail_app() -> App {
    let request = request();
    let sources = retail::read_sources(&request).expect("the installation reads");
    headless_app_with(|app| {
        retail::install(app, &sources, &request).expect("the original area installs");
    })
}

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

/// Where this task's frames go: `private/evidence/PLAYTEST-NOSE-FIX/<name>/`,
/// Git-ignored, never inside the installation.
fn private_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .map(|root| root.join("private/evidence/PLAYTEST-NOSE-FIX").join(name))
        .expect("the workspace root is two levels above the crate");
    std::fs::create_dir_all(&dir).expect("private/evidence is writable");
    dir
}

// ------------------------------------------------------------- the rule --

/// **A synthetic airframe whose nose sits on either stored axis is drawn with
/// that end leading — one rule, not a constant.**
///
/// `nose_mapping` is the production rule and the drawn placement is production
/// code (`AircraftPartAsset::oriented`), not this test's own arithmetic. Given a
/// stored nose of `−Z` (what the container measures) or `+Z` (an airframe stored
/// the other way round), the composed transform puts that nose reference on
/// `BODY_FORWARD`, and it does so by a **yaw** — never a pitch, a roll or a
/// mirror.
#[test]
fn accept_playtest_nose_a_synthetic_nose_on_either_stored_axis_is_drawn_leading() {
    let body_forward = forward();
    for stored in [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0]] {
        let rotation = nose_mapping(stored).expect("a horizontal stored nose maps");
        let stored_nose = Vec3::new(stored[0], 0.0, stored[2]).normalize();
        let mapped = rotation * stored_nose;
        assert!(
            (mapped - body_forward).length() < 1e-5,
            "the stored nose {stored:?} must land on BODY_FORWARD {body_forward:?}, got {mapped:?}"
        );
        assert!(
            (rotation * Vec3::Y - Vec3::Y).length() < 1e-5,
            "the mapping is a yaw about the vertical axis for {stored:?}, so it never pitches \
             or rolls the airframe"
        );
        assert!(
            rotation.is_normalized(),
            "and it is a rotation, so it never mirrors the airframe"
        );

        // The drawn placement, through the production composition: a nose
        // reference two metres along the stored axis ends up two metres ahead of
        // the body origin, on the forward axis.
        let part = AircraftPartAsset {
            node_slot: 1,
            node_name: "nose_reference".to_owned(),
            mesh: Handle::<Mesh>::default(),
            pieces: Vec::new(),
            local: Transform::from_translation(Vec3::new(stored[0], 0.0, stored[2]) * 2.0),
        };
        let drawn = part.oriented(rotation).translation;
        assert!(
            (drawn - body_forward * 2.0).length() < 1e-5,
            "the drawn nose reference of the {stored:?} airframe must sit ahead of the body on \
             the forward axis, got {drawn:?}"
        );
    }

    // The measured convention is exactly the identity: the airframe is stored
    // nose-forward, so nothing is turned.
    assert_eq!(
        nose_mapping(STORED_AIRCRAFT_NOSE_AXIS).expect("the measured axis maps"),
        Quat::IDENTITY,
        "the measured stored nose (-Z) needs no turn at all"
    );

    // An axis the rule cannot turn is refused by name, never guessed.
    match nose_mapping([0.0, 1.0, 0.0]) {
        Err(PlaytestError::NoseAxis { axis }) => assert_eq!(axis, [0.0, 1.0, 0.0]),
        other => panic!("a vertical stored nose must be refused by name, got {other:?}"),
    }
    match nose_mapping([0.0, f32::NAN, 0.0]) {
        Err(PlaytestError::NoseAxis { axis }) => assert!(axis[1].is_nan()),
        other => panic!("a non-finite stored nose must be refused by name, got {other:?}"),
    }
}

// ------------------------------------------------------------- the cruise --

/// **Over retail content, after level cruise, the drawn nose reference lies
/// ahead of the tail along the body's own velocity.**
///
/// The production spawn (`playtest::scene::spawn_aircraft`, over the
/// `RetailContent` the playtest installed) is driven for real fixed ticks with
/// no input, then the two reference points are read from the **spawned part
/// entities** — `pf_canopy` (the canopy) and `l_rudder1` (the rudder, under the
/// container's authored `tail` node) — and compared along the body's velocity
/// after being carried into the world frame by the body's own rotation.
///
/// This is the owner's observation, made checkable: with the stored `+Z` nose
/// reading this failed, because the half turn put the canopy behind the rudder
/// while the body flew on.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_nose_the_drawn_nose_leads_the_tail_in_level_cruise() {
    let mut app = retail_app();
    // Two seconds of level cruise at the playtest's own frame rate: no input, so
    // the command stays the cruise command the session starts with.
    run(&mut app, 120);
    let ticks = cs_app::playtest::fixed_ticks(&app);
    assert!(ticks >= 60, "the cruise ran fixed ticks, got {ticks}");

    let (position, rotation, velocity) = {
        let mut query = app
            .world_mut()
            .query_filtered::<(&Position, &Rotation, &LinearVelocity), With<PlaytestAircraft>>();
        let (position, rotation, velocity) =
            query.single(app.world()).expect("one player aircraft");
        (*position, *rotation, *velocity)
    };
    let speed = velocity.0.length();
    assert!(
        speed > 40.0,
        "level cruise keeps its speed, got {speed} m/s at {position:?}"
    );
    let direction = velocity.0 / speed;
    assert!(
        direction.z < -0.9,
        "the body must still be flying its spawn heading (-Z), got {direction:?}"
    );

    let body: Entity = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("one player aircraft");
    let drawn: Vec<(u32, Vec3)> = {
        let mut query = app.world_mut().query::<(&AircraftPart, &Transform)>();
        query
            .iter(app.world())
            .map(|(part, transform)| (part.node_slot, transform.translation))
            .collect()
    };
    let slot_of = |name: &str| -> u32 {
        let content = app.world().resource::<RetailContent>();
        content
            .parts
            .iter()
            .find(|part| part.node_name == name)
            .unwrap_or_else(|| {
                panic!(
                    "the retail airframe draws {name}: drawn nodes are {:?}",
                    content
                        .parts
                        .iter()
                        .map(|part| part.node_name.as_str())
                        .collect::<Vec<_>>()
                )
            })
            .node_slot
    };
    let reference = |name: &str| -> Vec3 {
        let slot = slot_of(name);
        drawn
            .iter()
            .find_map(|(node_slot, translation)| (*node_slot == slot).then_some(*translation))
            .unwrap_or_else(|| panic!("the drawn binding of {name} (slot {slot}) is spawned"))
    };
    let nose = reference(NOSE_REFERENCE_NODE);
    let tail = reference(TAIL_REFERENCE_NODE);

    // Body frame to world frame: the parts hang off the body, the velocity is a
    // world quantity, so the offset has to travel with the body's attitude.
    let nose_ahead = rotation.0 * (nose - tail);
    let along = nose_ahead.dot(velocity.0);
    println!(
        "PLAYTEST-NOSE-FIX ticks={ticks} speed={speed:.3} body={body:?} \
         nose_reference={nose:?} tail_reference={tail:?} along_velocity={along:.3}"
    );
    assert!(
        along > 0.0,
        "the drawn nose reference ({NOSE_REFERENCE_NODE} at {nose:?}) must lie ahead of the tail \
         ({TAIL_REFERENCE_NODE} at {tail:?}) along the body's velocity {:?}, got {along:.3} — the \
         aircraft is travelling tail first",
        velocity.0
    );
}

// ------------------------------------------------------------ the airship --

/// **The read path does not mirror the airship: every composed transform of the
/// area is orientation preserving, and a known left/right pair keeps its sides.**
///
/// The area and the aircraft go through the same coordinate adapter, so a fix
/// that mirrored one would mirror the other. Two measurements, both from the
/// production `area_graph` over the retail container: the composed linear map of
/// every one of the 793 nodes of the pinned `piratezep` subtree has a positive
/// determinant, and the authored `front_door_left` / `front_door_right` pair
/// straddles the hull centreline with the left-named door on the negative-X side
/// and the right-named one opposite it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_nose_the_airship_is_not_mirrored_by_the_read() {
    let sources =
        read_playtest_sources(&install_dir(), PLAYTEST_WORLD_GROUP).expect("the containers read");
    let adapter = playtest_adapter().expect("the canonical source is declared");
    let (graph, root) = area_graph(sources.world(), PLAYTEST_AREA_NODE_SLOT, &adapter)
        .expect("the documented area graph is built");
    assert_eq!(
        graph.node(&root).map(|node| node.name()),
        Some(PLAYTEST_AREA_NODE_NAME),
        "the pinned area node"
    );
    let members = graph.subtree(&root);
    assert_eq!(members.len(), 793, "the measured subtree's node count");

    let mut mirrored = Vec::new();
    for node in &members {
        let linear = node.world_transform().linear();
        let determinant = linear[0][0]
            * (linear[1][1] * linear[2][2] - linear[1][2] * linear[2][1])
            - linear[0][1] * (linear[1][0] * linear[2][2] - linear[1][2] * linear[2][0])
            + linear[0][2] * (linear[1][0] * linear[2][1] - linear[1][1] * linear[2][0]);
        if determinant.is_nan() || determinant <= 0.0 {
            mirrored.push((node.index(), node.name().to_owned(), determinant));
        }
    }
    assert!(
        mirrored.is_empty(),
        "a composed transform with a non-positive determinant mirrors its geometry: {mirrored:?}"
    );

    let door = |name: &str| -> Vec3 {
        let node = members
            .iter()
            .find(|node| node.name() == name)
            .unwrap_or_else(|| panic!("the area holds the authored {name} node"));
        let [x, y, z] = node.world_transform().translation();
        Vec3::new(x as f32, y as f32, z as f32)
    };
    let left = door("front_door_left");
    let right = door("front_door_right");
    println!(
        "PLAYTEST-NOSE-FIX airship nodes={} determinant_sign=positive front_door_left={left:?} \
         front_door_right={right:?}",
        members.len()
    );
    assert!(
        left.x < 0.0 && right.x > 0.0,
        "the authored left door must compose on the negative-X side and the right door on the \
         positive-X side, got left {left:?} and right {right:?}"
    );
    assert!(
        (left.x + right.x).abs() < 1e-3
            && (left.y - right.y).abs() < 1e-3
            && (left.z - right.z).abs() < 1e-3,
        "the pair straddles the hull centreline symmetrically: left {left:?}, right {right:?}"
    );
}

// ------------------------------------------------------------ the capture --

/// **A real GPU chase capture of the retail scene is written under `private/`
/// with its digest, so the finding can show the nose leading.**
///
/// The same production call the #648 scene test drives — `spawn_playtest_scene`
/// then `capture_playtest_views` — into this task's own directory, so the frames
/// this branch produces can be put next to the frames `main` produced with the
/// old `+Z` nose reading (both recorded, with digests, in
/// `docs/findings/2026-10-06-t709-airframe-nose-mapping.md`).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_nose_gpu_chase_capture_shows_the_nose_leading() {
    let config = PlaytestConfig::documented();
    let sources =
        read_playtest_sources(&install_dir(), &config.world_group).expect("the containers read");
    let mut app = playtest_app();
    app.update();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("the scene spawns");
    let dir = private_dir("after");
    let captures =
        capture_playtest_views(&mut app, &scene, &dir).expect("every documented view renders");
    for capture in &captures {
        assert!(
            capture.drew_aircraft(),
            "{} framed the aircraft",
            capture.view
        );
        assert!(capture.drew_environment(), "{} drew the area", capture.view);
        println!(
            "PLAYTEST-NOSE-FIX capture view={} path={} sha256={} aircraft_px={}",
            capture.view,
            capture.png,
            capture.png_sha256.to_hex(),
            capture.aircraft_pixels
        );
    }
    let chase = captures
        .iter()
        .find(|capture| capture.view == "chase")
        .expect("the chase view is one of the documented views");
    assert!(
        PathBuf::from(&chase.png).exists(),
        "the chase frame exists on disk: {}",
        chase.png
    );
    teardown_playtest_scene(&mut app, &scene);
}
