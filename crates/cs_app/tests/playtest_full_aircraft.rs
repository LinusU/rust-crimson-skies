//! #665 (`PLAYTEST-FULL-AIRCRAFT`): the retail playtest draws the **whole** intact
//! `bloodhawk`, not its fuselage alone. Task test prefix:
//! `accept_playtest_full_aircraft_`.
//!
//! Feature sheets: `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`
//! (`### F11-B`), `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`.
//!
//! Every test drives production code: `read_playtest_sources` over the production
//! GameZ readers, `aircraft_graph` / `SceneGraph`, the F11-B `select_lod_variant`
//! rule, the F17-B upload, and the playtest's own `headless_app_with` plus
//! `retail::install`. The retail tests are `#[ignore]`d (`requires CS_GAME_DIR`)
//! and **fail** without the variable; they write frames only under `private/`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use avian3d::prelude::Position;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::prelude::{App, ChildOf, Children, Entity, GlobalTransform, Mesh3d, Transform, With};
use cs_app::playtest::headless_app_with;
use cs_app::playtest::propeller::PropellerSpin;
use cs_app::playtest::retail::{self, RetailContent, RetailRequest};
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::playtest_retail::{
    AircraftPart, HiddenLod, PlaytestAircraftReport, PlaytestConfig, UndrawnBinding,
    aircraft_graph, capture_part_footprint, playtest_adapter, playtest_app, read_playtest_sources,
    spawn_playtest_content, spawn_playtest_scene, teardown_playtest_scene,
};
use cs_app::world::retail::stored_render_mesh;

fn install_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    )
}

fn request() -> RetailRequest {
    RetailRequest::new(install_dir(), None, None).expect("the documented defaults are valid")
}

fn retail_app() -> App {
    let request = request();
    let sources = retail::read_sources(&request).expect("the installation reads");
    headless_app_with(|app| {
        retail::install(app, &sources, &request).expect("the original area installs");
    })
}

fn private_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(|root| {
            root.join("private/evidence/PLAYTEST-FULL-AIRCRAFT")
                .join(name)
        })
        .expect("the workspace root is two levels above the crate");
    std::fs::create_dir_all(&dir).expect("private/evidence is writable");
    dir
}

fn key(app: &mut App, code: KeyCode, state: ButtonState) {
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

fn parts_of(app: &mut App, body: Entity) -> Vec<Entity> {
    app.world()
        .get::<Children>(body)
        .map(|children| children.iter().copied().collect())
        .unwrap_or_default()
}

fn body_of(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("one player aircraft")
}

/// **The aircraft draws every intact binding of the selected set, and the count and
/// triangle total are the container's own.**
///
/// Fails against the fuselage-only `aircraft_graph` consumer #648 shipped: that
/// drew exactly one binding of 140 triangles.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_full_aircraft_draws_every_intact_binding_measured_from_the_container() {
    let config = PlaytestConfig::documented();
    let sources = read_playtest_sources(&install_dir(), &config.world_group).expect("reads");
    let mut app = playtest_app();
    app.update();
    let content = spawn_playtest_content(&mut app, &sources, &config).expect("spawns");
    let report = &content.aircraft;

    assert!(
        report.mesh_bindings() > 1,
        "the whole aircraft is more than the one fuselage binding: {}",
        report.mesh_bindings()
    );
    assert_eq!(content.aircraft_parts.len(), report.mesh_bindings());

    // The measured container: every drawn part's triangles are what the container
    // stores for its mesh, and the total is their sum.
    let mut total = 0usize;
    for part in &report.parts {
        let stored = sources
            .aircraft()
            .meshes()
            .get(part.mesh_index)
            .expect("stored mesh");
        let built = stored_render_mesh(stored)
            .expect("builds")
            .triangles()
            .len();
        assert_eq!(part.triangles, built, "{}", part.node_name);
        assert!(part.triangles > 0, "{} draws nothing", part.node_name);
        total += built;
    }
    assert_eq!(report.triangles(), total);
    assert!(
        total > 140,
        "more triangles than the fuselage-only aircraft's 140: {total}"
    );

    // Complete accounting against an independent walk of the container's graph:
    // every binding of the airframe is either drawn or listed with a reason.
    let adapter = playtest_adapter().expect("adapter");
    let graph = aircraft_graph(sources.aircraft(), &adapter).expect("graph");
    let root = graph
        .root(&config.aircraft_root_name)
        .expect("airframe root");
    let bound: BTreeSet<u32> = graph
        .subtree(root.id())
        .iter()
        .filter(|node| node.mesh().is_some())
        .map(|node| node.index())
        .collect();
    assert_eq!(report.airframe_bindings, bound.len());
    let drawn: BTreeSet<u32> = report.parts.iter().map(|part| part.node_slot).collect();
    let listed: BTreeSet<u32> = report.undrawn.iter().map(|b| b.node_slot).collect();
    assert_eq!(drawn.len(), report.parts.len(), "no binding is drawn twice");
    assert!(drawn.is_disjoint(&listed), "drawn and undrawn are disjoint");
    assert_eq!(
        drawn.union(&listed).copied().collect::<BTreeSet<_>>(),
        bound,
        "every mesh binding of the airframe is drawn or listed as undrawn"
    );
    assert!(
        report.undrawn.iter().all(|b| !b.reason.is_empty()),
        "every undrawn binding names its reason"
    );

    // The selection is the F11-B rule's: one band, and every other band is hidden
    // with a reason.
    let (slot, name) = report
        .selected_lod
        .as_ref()
        .expect("the intact node has LOD bands");
    assert!(report.lod_coverage.is_some());
    assert!(
        report.hidden_lods.iter().all(|lod| lod.node_slot != *slot),
        "{name} is selected, so it is not also hidden"
    );
    assert!(!report.hidden_lods.is_empty());

    // Wings: the composed extent spans well beyond the fuselage's own width, so
    // something other than the fuselage is drawn.
    assert!(
        report.extent_m[0] > 5.0 * 2.111_164_6,
        "span {:?}",
        report.extent_m
    );
}

/// **Every part follows the one flight body, without drift, through input, reset and
/// teardown.**
///
/// The one part whose local transform production writes after spawn is the drawn
/// propeller (`spin_propellers`, #710): it is turned about its measured hub, and
/// its `PropellerSpin` carries both the placement it was spawned at and the turn
/// drawn so far. The check therefore reads the expected local from that component
/// — the only writer — and every other part must still hold its authored
/// placement exactly.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_full_aircraft_parts_follow_the_single_flight_body() {
    let mut app = retail_app();
    run(&mut app, 3);
    let expected = app
        .world()
        .resource::<RetailContent>()
        .aircraft
        .mesh_bindings();
    let rotation = app.world().resource::<RetailContent>().visual_rotation;
    let locals: Vec<(u32, Transform)> = app
        .world()
        .resource::<RetailContent>()
        .parts
        .iter()
        .map(|part| (part.node_slot, part.oriented(rotation)))
        .collect();
    let body = body_of(&mut app);
    let parts = parts_of(&mut app, body);
    assert_eq!(parts.len(), expected, "one child per drawn binding");
    assert!(expected > 1);

    // Fly with input: pitch up and roll.
    key(&mut app, KeyCode::KeyS, ButtonState::Pressed);
    key(&mut app, KeyCode::KeyE, ButtonState::Pressed);
    let start = app.world().get::<Position>(body).expect("body pose").0;
    run(&mut app, 120);
    let moved = app.world().get::<Position>(body).expect("body pose").0;
    assert!(start.distance(moved) > 10.0, "the aircraft flew");

    let body_global: GlobalTransform = *app.world().get::<GlobalTransform>(body).expect("global");
    for part in &parts {
        let slot = app
            .world()
            .get::<AircraftPart>(*part)
            .expect("marker")
            .node_slot;
        let authored = locals
            .iter()
            .find(|(s, _)| *s == slot)
            .expect("a local for every part")
            .1;
        // Exactly one part's local transform is written after spawn, on purpose:
        // `spin_propellers` (#710) turns the drawn propeller about its measured
        // hub. Its own `PropellerSpin` carries the placement it was spawned at,
        // so the local this test expects is production's own composition of that
        // placement and the turn drawn so far; every other part must still hold
        // the authored placement byte for byte.
        let spun = app.world().get::<PropellerSpin>(*part);
        let local = match spun {
            Some(spin) => {
                assert_eq!(
                    spin.base(),
                    authored,
                    "part {slot}'s spin carries the placement it was spawned at"
                );
                spin.transform()
            }
            None => authored,
        };
        let got = app
            .world()
            .get::<GlobalTransform>(*part)
            .expect("part global");
        let want = body_global * GlobalTransform::from(local);
        let (got, want) = (got.to_matrix(), want.to_matrix());
        for (g, w) in got.to_cols_array().iter().zip(want.to_cols_array()) {
            assert!(g.is_finite(), "part {slot} is finite");
            assert!(
                (g - w).abs() < 2e-3,
                "part {slot} drifted from body pose x composed local: {g} vs {w}"
            );
        }
        assert_eq!(
            *app.world().get::<Transform>(*part).expect("local"),
            local,
            "part {slot}'s local is exactly what this test composed: the authored \
             placement, or the drawn propeller's own spin over it"
        );
        assert!(app.world().get::<Mesh3d>(*part).is_some());
    }

    // Reset replaces the body and every part with it; no orphan part survives.
    for state in [ButtonState::Pressed, ButtonState::Released] {
        key(&mut app, KeyCode::KeyR, state);
        app.update();
    }
    run(&mut app, 10);
    for old in &parts {
        assert!(
            app.world().get_entity(*old).is_err(),
            "a part of the old body survived the reset"
        );
    }
    let new_body = body_of(&mut app);
    assert_ne!(new_body, body);
    assert_eq!(parts_of(&mut app, new_body).len(), expected);
    let markers = app
        .world_mut()
        .query_filtered::<&ChildOf, With<AircraftPart>>()
        .iter(app.world())
        .filter(|parent| parent.parent() != new_body)
        .count();
    assert_eq!(markers, 0, "no aircraft part hangs under any other entity");
    assert_eq!(
        app.world_mut()
            .query::<&AircraftPart>()
            .iter(app.world())
            .count(),
        expected,
        "exactly one set of parts is alive"
    );
}

/// **The capture scene's aircraft is one parent with one child per binding, and its
/// teardown leaves nothing behind.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_full_aircraft_capture_scene_tears_down_every_part() {
    let config = PlaytestConfig::documented();
    let sources = read_playtest_sources(&install_dir(), &config.world_group).expect("reads");
    let mut app = playtest_app();
    app.update();
    let drawn = |app: &mut App| app.world_mut().query::<&Mesh3d>().iter(app.world()).count();
    let before = drawn(&mut app);
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("spawns");
    let parts = parts_of(&mut app, scene.aircraft_entity());
    assert_eq!(parts.len(), scene.aircraft().mesh_bindings());
    teardown_playtest_scene(&mut app, &scene);
    for part in &parts {
        assert!(app.world().get_entity(*part).is_err(), "orphan part");
    }
    assert_eq!(drawn(&mut app), before, "no drawn mesh entity left behind");
    assert_eq!(
        app.world_mut()
            .query::<&AircraftPart>()
            .iter(app.world())
            .count(),
        0,
        "no aircraft part left behind"
    );
}

/// **A real GPU chase view shows wing pixels outside the fuselage's footprint.**
///
/// Rendered three times — the whole aircraft, the fuselage parts alone, nothing —
/// and the footprints differenced, as #648 did for the aircraft as a whole. The
/// frame is written under `private/` and nothing original is committed.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_full_aircraft_gpu_chase_view_shows_wings_beyond_the_fuselage() {
    let config = PlaytestConfig::documented();
    let sources = read_playtest_sources(&install_dir(), &config.world_group).expect("reads");
    let mut app = playtest_app();
    app.update();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("spawns");
    // The fuselage is the non-wing, non-tail core of the selected band: the
    // authored `g442` / `g443` nodes (nose and rear fuselage). Wings are measured
    // as whatever the rest of the set adds outside that footprint.
    let core: Vec<u32> = scene
        .aircraft()
        .parts
        .iter()
        .filter(|part| part.node_name == "g442" || part.node_name == "g443")
        .map(|part| part.node_slot)
        .collect();
    assert_eq!(core.len(), 2, "the fuselage halves are in the drawn set");
    let dir = private_dir("chase");
    let footprint = capture_part_footprint(&mut app, &scene, 0, &core, &dir)
        .expect("the view renders on the GPU");
    println!(
        "PLAYTEST-FULL-AIRCRAFT view={} aircraft_px={} fuselage_px={} outside_fuselage_px={} png={}",
        footprint.view,
        footprint.aircraft_pixels,
        footprint.core_pixels,
        footprint.outside_core_pixels,
        footprint.png
    );
    assert!(footprint.core_pixels > 0, "the fuselage is in frame");
    assert!(
        footprint.outside_core_pixels > footprint.core_pixels / 10,
        "the other parts add a visible share of pixels outside the fuselage's footprint: {footprint:?}"
    );
    teardown_playtest_scene(&mut app, &scene);
}

/// **The startup `playtest sources` object lists the drawn count, the triangle
/// total, the LOD selection and every undrawn binding.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_full_aircraft_sources_line_lists_bindings_selection_and_undrawn() {
    let mut app = retail_app();
    run(&mut app, 2);
    let content = app.world().resource::<RetailContent>();
    let json = content.manifest_json();
    println!("playtest sources: {json}");
    let aircraft = &content.aircraft;
    assert!(json.contains(&format!(
        "\"aircraft_mesh_bindings\":{}",
        aircraft.mesh_bindings()
    )));
    assert!(json.contains(&format!("\"aircraft_triangles\":{}", aircraft.triangles())));
    assert!(json.contains("\"aircraft_selection\""));
    assert!(json.contains("\"selected_lod\":{"));
    assert!(json.contains("\"aircraft_undrawn\":["));
    for binding in &aircraft.undrawn {
        assert!(
            json.contains(&format!("\"slot\":{},", binding.node_slot)),
            "undrawn slot {} is listed",
            binding.node_slot
        );
    }
}

/// **The report fields are well-formed JSON whatever the content**, so the startup
/// line and the smoke `report.json` stay machine-readable. Needs no installation.
#[test]
fn accept_playtest_full_aircraft_report_json_lists_selection_and_undrawn() {
    let report = PlaytestAircraftReport {
        container_key: "zbd/planes.zbd".to_owned(),
        root_name: "bloodhawk".to_owned(),
        intact_node_slot: 7,
        intact_node_name: "healthy".to_owned(),
        lod_distance_m: 20.0,
        selected_lod: Some((9, "near\"est".to_owned())),
        lod_coverage: Some(cs_content::scene::LodCoverage::Overlap),
        hidden_lods: vec![HiddenLod {
            node_slot: 10,
            node_name: "l3".to_owned(),
            reason: "not selected".to_owned(),
        }],
        parts: Vec::new(),
        undrawn: vec![UndrawnBinding {
            node_slot: 11,
            node_name: "shadow".to_owned(),
            mesh_index: 4,
            reason: "outside the intact subtree".to_owned(),
        }],
        airframe_bindings: 2,
        extent_m: [1.0, 1.0, 1.0],
    };
    let json = format!("{{{}}}", report.json_fields());
    let balanced = |open: char, close: char| {
        // Quotes are escaped in the fixture's one string, so a raw count is exact
        // outside strings; the fixture holds no brackets inside a string.
        json.matches(open).count() == json.matches(close).count()
    };
    assert!(balanced('{', '}') && balanced('[', ']'), "{json}");
    for expected in [
        "\"aircraft_mesh_bindings\":0",
        "\"aircraft_triangles\":0",
        "\"aircraft_airframe_bindings\":2",
        "\"selected_lod\":{\"slot\":9,\"name\":\"near\\\"est\"}",
        "\"lod_coverage\":\"Overlap\"",
        "\"hidden_lods\":[{\"slot\":10,\"name\":\"l3\",\"reason\":\"not selected\"}]",
        "\"aircraft_undrawn\":[{\"slot\":11,\"name\":\"shadow\",\"mesh\":4,",
    ] {
        assert!(json.contains(expected), "{expected} missing from {json}");
    }
}
