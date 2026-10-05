//! #648 (`PLAYTEST-RETAIL-SCENE`): one original world area and one original
//! aircraft, spawned for a free-flight playtest and captured from a real GPU.
//!
//! Feature sheets: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-A`, `### F18-B`, `### F18-D`) and
//! `specs/F10-gamez-mesh-topology-and-material-records.md` (`### F10-C.02`,
//! `### F10-C.03`). Shared contracts: `docs/contracts/IDENTITY-CONTENT.md`,
//! `docs/contracts/CLI-EVIDENCE.md`. Task test prefix: `accept_playtest_retail_`.
//!
//! These tests drive **production code only**: the readers are
//! `cs_app::playtest_retail::read_playtest_sources` over the production GameZ
//! readers, the composition is `cs_content::scene::SceneGraph::build`, the spawn
//! is `cs_app::world::spawn_object`, the upload is the production F17-B adapter,
//! and the capture is the renderer. No test here builds its own importer, its own
//! transform conversion, its own mesh upload or its own render loop.
//!
//! What is pinned:
//!
//! * **the area is the documented subtree and nothing else.** `C1C` node slot
//!   517 (`piratezep`) and its 792 descendants, over the measured 793 nodes /
//!   401 mesh bindings / 8 673 triangles; a pinned slot holding a different record
//!   is refused by name rather than substituted.
//! * **the aircraft is one explicit original mesh**: the `bloodhawk` airframe's
//!   `fuse03` node, mesh-array slot 1 436, 140 triangles, 3 material groups.
//! * **visual and collider geometry are one value.** Every area record presents
//!   and collides from the same `Assets<Mesh>` handle, the collider is a triangle
//!   mesh, and its triangle count is the record's own.
//! * **the scene is finite, bounded and owned.** Every placement, bound, spawn
//!   and camera pose is finite; a spawn/teardown/spawn cycle leaves the same live
//!   entity count and the same asset count.
//! * **the capture is a measurement.** Each view is rendered twice — with and
//!   without the aircraft — and the difference is the aircraft's own pixels; a
//!   view that drew nothing, framed only sky or showed no aircraft is refused and
//!   leaves no file behind.
//!
//! The retail half is `#[ignore]`d (`requires CS_GAME_DIR`): it reads the owner's
//! installation read-only, writes its frames under `private/`, and commits no
//! original bytes and no screenshots.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use avian3d::prelude::Collider;
use bevy::mesh::Mesh;
use bevy::prelude::{Assets, Mesh3d, MeshMaterial3d};
use cs_app::playtest_retail::{
    AIRCRAFT_CONTAINER_KEY, CAPTURE_HEIGHT, CAPTURE_WIDTH, CaptureError,
    PLAYTEST_AIRCRAFT_MESH_NODE_NAME, PLAYTEST_AIRCRAFT_MESH_NODE_SLOT,
    PLAYTEST_AIRCRAFT_POSE_IS_DESIGNED, PLAYTEST_AIRCRAFT_ROOT_NAME, PLAYTEST_AREA_IS_DESIGNED,
    PLAYTEST_AREA_NODE_NAME, PLAYTEST_AREA_NODE_SLOT, PLAYTEST_COLLISION_IS_THE_DRAWN_MESH,
    PLAYTEST_LABEL, PLAYTEST_NEUTRAL_MATERIAL, PLAYTEST_UNIT_IS_DESIGNED,
    PLAYTEST_VIEWS_ARE_DESIGNED, PLAYTEST_WORLD_GROUP, PlaytestConfig, PlaytestError, VIEW_COUNT,
    camera_poses, playtest_adapter, read_playtest_sources, spawn_playtest_scene, spawn_pose,
    teardown_playtest_scene,
};
use cs_app::world::WorldMeshAssets;
use cs_content::world::{Aabb, WorldCollisionShape};
use cs_types::content::Resolved;
use cs_types::evidence::{ClaimId, ClaimStatus};

// ------------------------------------------------------------------ no retail --

/// A path that is not an installation, so the loader refuses **by name** rather
/// than producing an empty scene. This is the shape criterion 1's "fail
/// explicitly without retail files" takes when `CS_GAME_DIR` is absent or wrong.
const NOT_AN_INSTALL: &str = "/playtest-retail-no-such-installation";

/// **A missing installation is refused by name, not silently empty.**
///
/// The production loader is the only way into the scene, and its first step is
/// production discovery. Pointed at something that is not an installation it
/// returns [`PlaytestError::Discovery`] naming the path, so a run without
/// `CS_GAME_DIR` — or with the wrong one — fails immediately instead of building
/// a scene with no geometry in it.
#[test]
fn accept_playtest_retail_a_missing_installation_is_refused_by_name() {
    let error = read_playtest_sources(Path::new(NOT_AN_INSTALL), PLAYTEST_WORLD_GROUP)
        .expect_err("a path that is not an installation must be refused");
    match error {
        PlaytestError::Discovery(discovery) => {
            let text = discovery.to_string();
            assert!(
                text.contains(NOT_AN_INSTALL) || text.contains("installation"),
                "the refusal must name what could not be inventoried, got {text:?}"
            );
        }
        other => panic!("a missing installation is a discovery refusal, got {other}"),
    }
}

/// **An installation without the documented containers is refused, naming both.**
///
/// A directory that exists and is readable is the case where a silent empty scene
/// would be most tempting: discovery succeeds, the two containers are simply not
/// there. The loader must name the container it could not find, so a wrong
/// `CS_GAME_DIR` is distinguishable from a corrupt one.
#[test]
fn accept_playtest_retail_a_directory_without_the_containers_is_refused_by_key() {
    // Per process, not a fixed name: two runs of this selection on one host would
    // otherwise share the scratch directory, and one run's `remove_dir_all` would
    // take the other's `ZBD` away mid-`discover`, turning an `Absent` refusal into
    // a `Discovery` one.
    let root = std::env::temp_dir().join(format!(
        "playtest-retail-empty-install-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("ZBD")).expect("the scratch directory is writable");
    let error = read_playtest_sources(&root, PLAYTEST_WORLD_GROUP)
        .expect_err("an installation holding neither container must be refused");
    match error {
        PlaytestError::Absent { container } => {
            assert_eq!(
                container, "zbd/c1c/gamez.zbd",
                "the refusal names the container production discovery did not inventory"
            );
        }
        other => panic!("an absent container is an Absent refusal, got {other}"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------- the designed values --

/// **Every claim id this stage records is valid, distinct, and filed under the
/// class its own words say.**
///
/// `ObservedTool` for a container-derived fact and `Designed` for a development
/// choice are two different claims about two different kinds of fact; a stage that
/// filed a designed value as `ObservedTool` — or an original-run claim at all —
/// would be inflating its own evidence, so the classes are asserted here.
#[test]
fn accept_playtest_retail_every_designed_value_is_recorded_under_its_own_claim() {
    let ids = [
        PLAYTEST_AREA_IS_DESIGNED,
        PLAYTEST_UNIT_IS_DESIGNED,
        PLAYTEST_COLLISION_IS_THE_DRAWN_MESH,
        PLAYTEST_AIRCRAFT_POSE_IS_DESIGNED,
        PLAYTEST_VIEWS_ARE_DESIGNED,
        PLAYTEST_NEUTRAL_MATERIAL,
    ];
    for id in ids {
        assert!(
            ClaimId::new(id).is_ok(),
            "every claim this stage records must be a valid claim id: {id}"
        );
    }
    let distinct: BTreeSet<&str> = ids.into_iter().collect();
    assert_eq!(
        distinct.len(),
        6,
        "each designed decision has its own claim"
    );

    // The label a screenshot, a log line and a window title must all carry.
    assert_eq!(
        PLAYTEST_LABEL, "ORIGINAL ASSETS / DEVELOPMENT FREE FLIGHT / PROVISIONAL TUNING",
        "the visible label is the owner's, verbatim"
    );
    for forbidden in ["M01", "faithful", "campaign", "verified_original"] {
        assert!(
            !PLAYTEST_LABEL.contains(forbidden),
            "the label must never claim a mission or a verified class: {forbidden}"
        );
    }

    let config = PlaytestConfig::documented();
    assert_eq!(config.world_group, PLAYTEST_WORLD_GROUP);
    assert_eq!(config.area_node_slot, PLAYTEST_AREA_NODE_SLOT);
    assert_eq!(config.area_node_name, PLAYTEST_AREA_NODE_NAME);
    assert_eq!(config.aircraft_root_name, PLAYTEST_AIRCRAFT_ROOT_NAME);
    assert_eq!(
        config.aircraft_mesh_node_slot,
        PLAYTEST_AIRCRAFT_MESH_NODE_SLOT
    );
    assert_eq!(
        config.aircraft_mesh_node_name,
        PLAYTEST_AIRCRAFT_MESH_NODE_NAME
    );
    assert_eq!(config.capture_size(), (CAPTURE_WIDTH, CAPTURE_HEIGHT));
    assert_eq!(config, PlaytestConfig::default());
}

/// **The declared unit is the workspace's canonical source, at `Unknown` evidence.
///
/// The original's world-vertex unit and handedness are unmeasured (task #436), so
/// the scene's chosen reading must be a **declared** source whose calibration
/// class is `Unknown` — never a `VerifiedOriginal`, and never a bespoke adapter
/// built for one installation.
#[test]
fn accept_playtest_retail_the_declared_unit_is_canonical_and_uncalibrated() {
    let adapter = playtest_adapter().expect("the canonical source is declared");
    assert_eq!(adapter.source().label(), "canonical");
    assert_eq!(
        adapter.source().calibration().claim_status(),
        ClaimStatus::Unknown,
        "the stored unit is unmeasured, so the declared reading carries no calibration"
    );
    assert_eq!(adapter.source().convention().meters_per_unit(), 1.0);
    assert!(
        !adapter.source().origin().is_original(),
        "a designed reading of an installation's bytes is not itself measured from one"
    );
}

// ------------------------------------------------ the designed spawn and views --

/// A real measured area: c1c's pinned subtree's composed extent, to the units the
/// production readers reported.
fn measured_bounds() -> Aabb {
    Aabb::try_new(
        [
            -53.168_008_208_274_84,
            -160.598_781_585_693_36,
            -222.892_888_903_617_86,
        ],
        [
            53.167_592_406_272_89,
            57.130_611_419_677_734,
            616.608_032_226_562_5,
        ],
    )
    .expect("the measured composed extent is a valid box")
}

/// The measured extent of the pinned aircraft mesh, in canonical metres.
const MEASURED_AIRCRAFT_EXTENT: [f32; 3] = [2.111_164_6, 1.492_971_9, 10.233_251];

/// **The spawn is outside the geometry with room to fly, and its nose is on the
/// runtime's forward axis.**
///
/// Checked against the **measured** composed extent rather than a made-up box:
/// the spawn must be off the area's own side (so there is air to fly into), inside
/// its height range (so it is not above or below the geometry), and amidships, and
/// the rotation must be a half turn about the vertical axis — which is what maps
/// the measured stored nose (`+Z`) onto the runtime's forward axis (`−Z`).
#[test]
fn accept_playtest_retail_the_spawn_is_outside_the_measured_area_and_facing_forward() {
    let bounds = measured_bounds();
    let (spawn, rotation) = spawn_pose(&bounds).expect("a measured area yields a finite pose");
    let min = bounds.min();
    let max = bounds.max();
    let extent = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];

    assert!(
        (spawn[0] as f64) < min[0],
        "the spawn must start off the area's own side, not inside it: {:?} vs min {min:?}",
        spawn
    );
    assert!(
        (spawn[1] as f64) > min[1] && (spawn[1] as f64) < max[1],
        "the spawn must be inside the area's height range: {:?} vs {min:?}..{max:?}",
        spawn
    );
    assert!(
        (spawn[2] as f64) > min[2] && (spawn[2] as f64) < max[2],
        "the spawn must be amidships along the area's length: {:?} vs {min:?}..{max:?}",
        spawn
    );
    // The clear air in front of the nose, which is what "room to fly" has to mean.
    let to_area = min[0] - f64::from(spawn[0]);
    assert!(
        to_area > 0.25 * extent[0],
        "there must be a quarter of the area's own width of clear air ahead of the spawn: \
         {to_area} m"
    );

    // A half turn about +Y maps the stored +Z nose onto the runtime's -Z.
    let stored_nose = bevy::math::Vec3::Z;
    let rotated = rotation * stored_nose;
    assert!(
        (rotated + bevy::math::Vec3::Z).length() < 1e-5,
        "the declared rotation must map the measured stored nose (+Z) onto the runtime's \
         forward axis (-Z), got {rotated:?}"
    );
    assert!(
        (rotation * bevy::math::Vec3::Y - bevy::math::Vec3::Y).length() < 1e-5,
        "and it must be about the vertical axis, so it changes no altitude"
    );
}

/// **A degenerate area is refused by name instead of framed.**
///
/// An area whose every corner is one point has no extent to place a camera in, and
/// a camera whose framing distance is zero produces a frame that says nothing about
/// the scene. The refusal is named, so a caller reads why rather than receiving a
/// uniform PNG.
#[test]
fn accept_playtest_retail_a_degenerate_area_is_refused_by_name() {
    let point = Aabb::try_new([1.0, 2.0, 3.0], [1.0, 2.0, 3.0]).expect("a point is a valid box");
    match spawn_pose(&point) {
        Err(PlaytestError::DegenerateArea { extent }) => {
            assert_eq!(
                extent,
                [0.0, 0.0, 0.0],
                "the refusal carries the extent it saw"
            );
        }
        other => panic!("a degenerate area must be refused by name, got {other:?}"),
    }
}

/// **The camera views are three, finite, and each frames something real.**
///
/// Every eye and target must be finite (a non-finite pose would put the camera
/// somewhere undefined and measure a frame that says nothing), the two
/// aircraft-framing views must **look at the spawn** — that is what puts the
/// aircraft in frame — and the third must look at the area's centre from outside
/// it, so it frames the environment rather than the aircraft's own nose.
#[test]
fn accept_playtest_retail_the_three_camera_views_are_finite_and_frame_both_subjects() {
    let bounds = measured_bounds();
    let (spawn, _) = spawn_pose(&bounds).expect("a measured area yields a finite pose");
    let views = camera_poses(&bounds, spawn, MEASURED_AIRCRAFT_EXTENT)
        .expect("a measured area yields finite views");
    assert_eq!(
        views.len(),
        VIEW_COUNT,
        "three views: two of the aircraft, one of the area"
    );
    assert_eq!(
        views.iter().map(|view| view.name).collect::<Vec<_>>(),
        ["chase", "quarter", "overview"],
        "the view names are stable, so an artifact name is stable"
    );

    let centre = [
        (bounds.min()[0] + bounds.max()[0]) * 0.5,
        (bounds.min()[1] + bounds.max()[1]) * 0.5,
        (bounds.min()[2] + bounds.max()[2]) * 0.5,
    ];
    for view in &views {
        for value in view.eye.iter().chain(view.target.iter()) {
            assert!(value.is_finite(), "{view:?} carries a non-finite pose");
        }
        let distance =
            bevy::math::Vec3::from(view.eye).distance(bevy::math::Vec3::from(view.target));
        assert!(
            distance > 0.0 && distance.is_finite(),
            "{view:?} looks at its own eye, so the frame would be undefined"
        );
        // Outside the aircraft itself, so no view frames the inside of its own
        // subject: the bounding sphere's radius is the distance below which a
        // camera sits within the mesh it is looking at.
        let radius = 0.5
            * f64::from(MEASURED_AIRCRAFT_EXTENT[0])
                .hypot(f64::from(MEASURED_AIRCRAFT_EXTENT[1]))
                .hypot(f64::from(MEASURED_AIRCRAFT_EXTENT[2]));
        assert!(
            f64::from(distance) > radius,
            "{view:?} sits {distance} m from its subject, inside the aircraft's own bounding \
             sphere ({radius} m), so the camera would be inside the mesh it frames"
        );
    }
    // The two aircraft-framing views really are aircraft framings: a declared
    // multiple of the aircraft's own length and no further, so a 10 m aircraft is
    // a readable share of the frame rather than a speck.
    for view in &views[..2] {
        let distance = f64::from(
            bevy::math::Vec3::from(view.eye).distance(bevy::math::Vec3::from(view.target)),
        );
        let lengths = distance / f64::from(MEASURED_AIRCRAFT_EXTENT[2]);
        assert!(
            (1.0..8.0).contains(&lengths),
            "{} sits {lengths} aircraft lengths from the aircraft, outside the 1..8 band this \
             stage declares for an aircraft framing",
            view.name
        );
    }
    for view in &views[..2] {
        assert_eq!(
            view.target, spawn,
            "{} must look at the spawn, or the aircraft is not in frame",
            view.name
        );
    }
    for (axis, centre_axis) in centre.iter().enumerate() {
        assert!(
            (views[2].target[axis] as f64 - centre_axis).abs() < 1e-3,
            "the overview must look at the area's centre, or it frames nothing"
        );
        assert!(
            (views[2].eye[axis] as f64 - views[0].eye[axis] as f64).abs() > 1.0,
            "the overview must stand somewhere the chase view does not, or it is the same frame"
        );
    }
}

/// **A non-finite spawn or aircraft extent is refused by name.**
///
/// The framing function is total over finite inputs and refuses anything else:
/// a `NaN` eye produces a frame whose pixels mean nothing, and a reader would have
/// no way to tell that from a view that framed sky.
#[test]
fn accept_playtest_retail_a_non_finite_pose_input_is_refused_by_name() {
    let bounds = measured_bounds();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        match camera_poses(&bounds, [bad, 0.0, 0.0], MEASURED_AIRCRAFT_EXTENT) {
            Err(PlaytestError::NonFinite { what }) => {
                assert!(
                    what.contains("spawn"),
                    "the refusal names the component that was not finite: {what}"
                );
            }
            other => panic!("a non-finite spawn must be refused by name, got {other:?}"),
        }
        match camera_poses(&bounds, [0.0, 0.0, 0.0], [1.0, bad, 1.0]) {
            Err(PlaytestError::NonFinite { what }) => {
                assert!(
                    what.contains("aircraft"),
                    "the refusal names the component that was not finite: {what}"
                );
            }
            other => panic!("a non-finite aircraft extent must be refused, got {other:?}"),
        }
    }
}

/// How many updates the teardown check drives so Bevy's asset bookkeeping and
/// Avian's collider cache have released the scene's uploads.
///
/// Freeing an uploaded mesh is not a synchronous step in either framework:
/// despawning the entities drops the strong handles, Bevy publishes the
/// `AssetEvent` on a later frame, and Avian's `ColliderCache` — added by
/// `PhysicsPlugins` — holds the `ColliderConstructor`, and so one more
/// `Handle<Mesh>`, for every mesh a collider was built from, and releases it only
/// after reading that event. Three is what this check gives it; the assertion
/// afterwards is exact, so a longer release would fail rather than pass quietly.
const ASSET_RELEASE_UPDATES: usize = 3;

/// The triangle count of a derived collider's shape, when it is a triangle mesh.
///
/// Read through Avian's own `as_trimesh`, the same accessor the mesh-collider
/// acceptance tests use, so a collider that stopped being a `TriMesh` — a convex
/// hull or a bounding box is exactly what a substituted shape would be — is a
/// panic here naming the record rather than a silently passing count.
fn collider_triangles(collider: &Collider) -> usize {
    collider
        .shape()
        .as_trimesh()
        .expect("a mesh-derived collider is a triangle mesh")
        .indices()
        .len()
}

// ------------------------------------------------------------------- the retail --

/// `CS_GAME_DIR`, read once for the retail half.
fn install() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must be set for the retail half of this task"),
    )
}

/// Where one test's frames go: `private/evidence/<TASK-ID>/<name>/`, which Git
/// ignores, resolved against the **workspace root** rather than the test
/// process's working directory (which is the crate, and whose `private/` the
/// root `.gitignore` does not cover).
///
/// The per-test subdirectory is not cosmetic: two GPU tests run in parallel in one
/// test binary, each with its own Bevy app, and they would otherwise write and
/// delete the **same** PNG paths. Measured: with a shared directory one test's
/// refusal deleted the other test's artifact and the run failed on
/// `the artifact the capture reports exists on disk`.
fn capture_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|workspace| workspace.parent())
        .map(|root| {
            root.join("private/evidence/PLAYTEST-RETAIL-SCENE")
                .join(name)
        })
        .expect("the workspace root is two levels above the crate");
    std::fs::create_dir_all(&dir).expect("private/evidence is writable");
    dir
}

/// **One original `c1c` area and one original `bloodhawk` mesh spawn through the
/// production readers and adapters, share one geometry between visual and
/// collision, tear down cleanly, and are captured from the real GPU from at least
/// two camera views.**
///
/// The whole acceptance criterion set in one retail test, because each step needs
/// the previous one's live Bevy world and one production discovery pass is
/// expensive (it hashes the whole installation): one pass, one scene, one spawn,
/// one capture, one teardown, one reload.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_retail_retail_c1c_area_and_bloodhawk_mesh_spawn_and_capture() {
    let config = PlaytestConfig::documented();
    let sources =
        read_playtest_sources(&install(), &config.world_group).expect("both containers read");

    // -- what was read, from the production readers ---------------------------
    assert_eq!(sources.world().container_key(), "zbd/c1c/gamez.zbd");
    assert_eq!(sources.aircraft().container_key(), AIRCRAFT_CONTAINER_KEY);
    assert!(!sources.world().container_sha256().is_empty());
    assert!(!sources.aircraft().container_sha256().is_empty());
    assert!(!sources.installation().is_empty());

    let mut app = cs_app::playtest_retail::playtest_app();
    // One warm-up update **before** the baseline, because the app's own first
    // update creates a mesh of its own: measured on the pinned pair, a fresh
    // `playtest_app()` holds 3 meshes and holds 4 after one update (an extra
    // 2-triangle quad, not one of this scene's). A baseline taken before any
    // update would therefore blame the teardown for the app's own mesh.
    app.update();
    let engine_meshes_before = app.world().resource::<Assets<Mesh>>().len();
    let scene = spawn_playtest_scene(&mut app, &sources, &config)
        .expect("the documented area and aircraft spawn");

    // -- the area is the documented subtree, measured ------------------------
    let area = scene.area();
    assert_eq!(
        area.world.as_str(),
        "world/c1c",
        "the world's own id names the world namespace and its group"
    );
    assert_eq!(area.node_slot, PLAYTEST_AREA_NODE_SLOT);
    assert_eq!(area.node_name, PLAYTEST_AREA_NODE_NAME);
    assert_eq!(
        area.nodes, 793,
        "the pinned subtree's node count, measured over the production readers"
    );
    assert_eq!(
        area.mesh_records, 401,
        "how many of those nodes bind a mesh the container stores geometry for"
    );
    assert_eq!(
        area.triangles, 8_673,
        "the stored triangles the area draws, over the production F10-E builder"
    );
    assert!(
        area.refused.is_empty(),
        "no area record may be unplaceable: {:?}",
        area.refused
    );
    assert!(
        area.gaps.is_empty(),
        "no area record may be presented without its collider: {:?}",
        area.gaps
    );
    assert_eq!(
        scene.spawned().objects().len(),
        area.mesh_records,
        "every mesh record was spawned"
    );
    assert_eq!(
        scene.spawned().colliders().len(),
        area.mesh_records,
        "and every one of them collided"
    );
    assert_eq!(
        scene.spawned().presentation_gap_count(),
        0,
        "no record reports a second reason for the same missing upload"
    );

    // Every record's collision is the geometry it draws: the `FromMesh` shape, the
    // role the scene declared, and one asset behind both consumers.
    for object in scene.definition().objects() {
        assert_eq!(
            object.known_shape(),
            Some(WorldCollisionShape::FromMesh),
            "{} must collide from its own mesh",
            object.id()
        );
        assert_eq!(
            object.provenance().claim_id.as_str(),
            PLAYTEST_AREA_IS_DESIGNED,
            "a retail-derived value points at the bytes it was read from"
        );
        assert_eq!(
            object.provenance().class,
            ClaimStatus::ObservedTool,
            "reading container bytes is a tool observation, never an original run"
        );
        assert_eq!(
            object.provenance().source.as_ref(),
            Some(sources.world().span()),
            "and it names the container span a reader can go back to"
        );
        let Resolved::Known(known) = object.mesh() else {
            panic!("{} must resolve a mesh", object.id());
        };
        assert!(
            sources.world().mesh_index_of(&known.value).is_some(),
            "{} names a mesh of this container's own slot table",
            object.id()
        );
        assert!(object.id().as_str().starts_with("playtest.node-"));
    }

    // -- the aircraft is one explicit original mesh --------------------------
    let aircraft = scene.aircraft();
    assert_eq!(aircraft.container_key, AIRCRAFT_CONTAINER_KEY);
    assert_eq!(aircraft.root_name, PLAYTEST_AIRCRAFT_ROOT_NAME);
    assert_eq!(aircraft.node_slot, PLAYTEST_AIRCRAFT_MESH_NODE_SLOT);
    assert_eq!(aircraft.node_name, PLAYTEST_AIRCRAFT_MESH_NODE_NAME);
    assert_eq!(
        aircraft.mesh_index, 1_436,
        "the mesh-array slot the pinned node binds"
    );
    assert_eq!(
        aircraft.triangles, 140,
        "the stored triangles over the production F10-E builder"
    );
    assert_eq!(
        aircraft.groups, 3,
        "its stored material groups, merged into one upload"
    );
    assert!(
        (aircraft.extent_m[2] - 10.233_251).abs() < 1e-5,
        "the fuselage's stored length, which every declared camera distance is a multiple of: \
         {}",
        aircraft.extent_m[2]
    );
    assert!(
        (aircraft.extent_m[0] - 2.111_164_6).abs() < 1e-5,
        "and its width: {}",
        aircraft.extent_m[0]
    );
    assert!(
        (aircraft.extent_m[1] - 1.492_971_9).abs() < 1e-5,
        "and its height: {}",
        aircraft.extent_m[1]
    );
    assert_eq!(
        aircraft.composed_translation_m,
        [0.0, 0.0, 0.0],
        "the mesh is already in its airframe's own frame, so the spawn pose places it unchanged"
    );

    // -- the scene is finite, bounded and owned -------------------------------
    let entities_before = app.world().entities().len();
    for value in scene.spawn().iter() {
        assert!(value.is_finite(), "the spawn pose must be finite");
    }
    assert_eq!(scene.views().len(), VIEW_COUNT);
    assert!(
        scene.material().is_neutral(),
        "the material decision is the neutral development one, reported once"
    );
    assert_eq!(scene.material().claim, PLAYTEST_NEUTRAL_MATERIAL);
    assert_eq!(
        scene.material().covered.len(),
        2,
        "the one material decision covers both containers, named"
    );

    // -- colliders exist, are triangle meshes, and carry the record's own count
    let settled = cs_app::playtest_retail::settle_playtest_colliders(&mut app, &scene);
    assert_eq!(
        settled, area.mesh_records,
        "every spawned record's collider is derived after the settle updates"
    );
    for object in scene.spawned().objects() {
        let mesh = object
            .mesh
            .as_ref()
            .expect("every spawned record resolved its mesh");
        let entity = object
            .collider
            .as_ref()
            .expect("every record collided")
            .entity;
        let collider = app
            .world()
            .get::<Collider>(entity)
            .unwrap_or_else(|| panic!("{} has no derived collider", object.object));
        let triangles = collider_triangles(collider);
        assert_eq!(
            triangles, mesh.triangles,
            "{} collides from exactly the triangles it draws",
            object.object
        );
        // One asset behind both consumers: the collider's body entity is the node
        // whose `Mesh3d` the record draws.
        assert!(
            app.world().get::<Mesh3d>(entity).is_some(),
            "{} presents from the entity it collides on",
            object.object
        );
    }
    // The loader's cache holds one asset per **distinct** mesh the area names, and
    // the engine holds that plus the aircraft's own upload, so the engine's count
    // is strictly greater — a `>=` here would hold even if the aircraft were
    // drawn from a world record's asset.
    let loader_assets = app.world().resource::<WorldMeshAssets>().len();
    assert!(loader_assets > 1, "the area names many distinct meshes");
    let engine_meshes = app.world().resource::<Assets<Mesh>>().len();
    assert!(
        engine_meshes > loader_assets,
        "the loader's cache ({} meshes) plus the aircraft's own upload, so the engine holds more \
         than the world records alone: {}",
        loader_assets,
        engine_meshes
    );
    assert!(
        engine_meshes > engine_meshes_before,
        "and the scene really uploaded something: {engine_meshes_before} -> {engine_meshes}"
    );
    // The aircraft is drawn from its own asset, on its own entity.
    assert!(
        app.world().get::<Mesh3d>(scene.aircraft_entity()).is_some(),
        "the aircraft presents its own mesh"
    );
    assert!(
        app.world()
            .get::<MeshMaterial3d<bevy::pbr::StandardMaterial>>(scene.aircraft_entity())
            .is_some(),
        "and it is presented, which the world records' presentation path does not do itself"
    );

    // -- the capture: real frames from the same spawned content --------------
    let dir = capture_dir("area");
    let captures = cs_app::playtest_retail::capture_playtest_views(&mut app, &scene, &dir)
        .expect("every documented view renders, frames the area and frames the aircraft");
    assert_eq!(
        captures.len(),
        VIEW_COUNT,
        "at least two views, and three are documented"
    );
    for capture in &captures {
        assert!(
            capture.drew_environment(),
            "{} drew the area's geometry: {} permille covered, {} luminance levels",
            capture.view,
            capture.covered_permille,
            capture.distinct_luminance
        );
        assert!(
            capture.drew_aircraft(),
            "{} framed the aircraft: {} pixels changed when it was hidden",
            capture.view,
            capture.aircraft_pixels
        );
        assert_eq!(
            capture.width, config.capture_width,
            "the frame is the documented size"
        );
        assert_eq!(capture.height, config.capture_height);
        assert!(capture.png_bytes > 0);
        let path = PathBuf::from(&capture.png);
        assert!(
            path.to_string_lossy()
                .contains("/private/evidence/PLAYTEST-RETAIL-SCENE/"),
            "a frame is written under the workspace's private/, never into the tree: {}",
            capture.png
        );
        assert!(
            path.exists(),
            "the artifact the capture reports exists on disk"
        );
        assert_eq!(
            cs_assets::install::sha256(&std::fs::read(&path).unwrap()),
            capture.png_sha256,
            "the digest in the capture is of the file on disk"
        );
        assert!(
            capture.environment_pixels > 0,
            "{} measured the area's own contribution too",
            capture.view
        );
    }
    for capture in &captures {
        eprintln!(
            "PLAYTEST {} eye={:?} target={:?} covered={} permille={} aircraft={} \
             environment={} bytes={}",
            capture.view,
            capture.eye,
            capture.target,
            capture.covered_pixels,
            capture.covered_permille,
            capture.aircraft_pixels,
            capture.environment_pixels,
            capture.png_bytes,
        );
    }

    // Two distinct framings, not one frame written three times.
    assert!(
        captures[0].eye != captures[2].eye && captures[1].eye != captures[2].eye,
        "the views must be different framings"
    );

    // -- teardown and reload leave no stale ownership ------------------------
    let owned = scene.entities().len();
    let teardown = teardown_playtest_scene(&mut app, &scene);
    assert_eq!(
        teardown.entities, owned,
        "every entity this scene created is despawned by its teardown"
    );
    assert_eq!(
        teardown.released_world_mesh_assets, loader_assets,
        "the release dropped the cache the spawn filled, not an empty one: the count *after* the \
         release is zero by construction and would look the same either way"
    );
    assert_eq!(
        teardown.world_mesh_assets, 0,
        "and the world mesh assets go with it, so a reload starts from nothing"
    );
    assert_eq!(
        app.world().entities().len(),
        entities_before,
        "a teardown leaves the live entity count it found"
    );
    assert!(
        app.world().get_resource::<WorldMeshAssets>().is_none(),
        "the loader's owning handle is released, so nothing stale can be reused"
    );
    // Several updates, not one, because asset release is not synchronous in either
    // framework — see `ASSET_RELEASE_UPDATES`. Without them the "no stale
    // ownership" claim would be about a loader counter while the engine's own
    // meshes are still settling out of the asset stack.
    for _ in 0..ASSET_RELEASE_UPDATES {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Assets<Mesh>>().len(),
        engine_meshes_before,
        "every engine mesh this scene uploaded is freed once nothing holds it, so a reload \
         cannot reuse the last scene's geometry"
    );

    let reloaded = spawn_playtest_scene(&mut app, &sources, &config)
        .expect("the scene reloads from the same sources");
    assert_eq!(
        reloaded.spawned().objects().len(),
        scene.spawned().objects().len(),
        "a reload produces the same records, from the same sources"
    );
    assert_eq!(
        reloaded.aircraft().fingerprint,
        scene.aircraft().fingerprint,
        "and the same aircraft geometry, so the upload is deterministic"
    );
    assert_ne!(
        reloaded.aircraft_entity(),
        scene.aircraft_entity(),
        "the reload's aircraft is a new entity, not the despawned one"
    );
    teardown_playtest_scene(&mut app, &reloaded);
}

/// **The refusals a container that disagrees with the pinned choices produces are
/// named, and no substitute geometry is drawn.**
///
/// The pinned area slot and the pinned aircraft mesh slot are **choices**, so a
/// container that holds something else there must stop the scene by name. A scene
/// that quietly drew a different record would be a claim about a different area
/// wearing this one's name.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_retail_retail_a_pinned_slot_holding_another_record_is_refused_by_name() {
    let sources = read_playtest_sources(&install(), PLAYTEST_WORLD_GROUP).expect("both read");
    let mut app = cs_app::playtest_retail::playtest_app();
    // The app's own entity count, before any of this stage's refusals runs: Bevy's
    // default plugin set creates entities of its own (the render world, the window
    // surrogate, the camera targets), so "left nothing behind" is a comparison
    // against this baseline and not against zero.
    let baseline = app.world().entities().len();

    // An area slot that is not a node of the container at all.
    let mut wrong_area = PlaytestConfig::documented();
    wrong_area.area_node_slot = 999_999;
    match &spawn_playtest_scene(&mut app, &sources, &wrong_area) {
        Err(PlaytestError::AreaNode { slot, found }) => {
            assert_eq!(*slot, 999_999, "the refusal carries the slot it asked for");
            assert_eq!(found, &None, "and that the container holds nothing there");
        }
        Err(other) => panic!("an absent area slot must be refused by name, got {other:?}"),
        Ok(_) => panic!("an absent area slot must not spawn a scene"),
    }

    // A slot that **is** a node, but not the one the pinned name says.
    let mut renamed = PlaytestConfig::documented();
    renamed.area_node_name = "not-the-pinned-name".to_owned();
    match &spawn_playtest_scene(&mut app, &sources, &renamed) {
        Err(PlaytestError::AreaNode { found, .. }) => {
            assert_eq!(
                found.as_deref(),
                Some(PLAYTEST_AREA_NODE_NAME),
                "the refusal names what the slot really holds"
            );
        }
        Err(other) => panic!("a renamed area node must be refused by name, got {other:?}"),
        Ok(_) => panic!("a renamed area node must not spawn a scene"),
    }

    // An aircraft root the container does not store.
    let mut no_airframe = PlaytestConfig::documented();
    no_airframe.aircraft_root_name = "no-such-airframe".to_owned();
    match &spawn_playtest_scene(&mut app, &sources, &no_airframe) {
        Err(PlaytestError::AircraftNode { what, asked }) => {
            assert_eq!(*what, "airframe root");
            assert_eq!(asked.as_str(), "no-such-airframe");
        }
        Err(other) => panic!("an absent airframe root must be refused by name, got {other:?}"),
        Ok(_) => panic!("an absent airframe root must not spawn a scene"),
    }

    // A mesh slot inside the airframe that binds no mesh.
    let mut no_mesh = PlaytestConfig::documented();
    no_mesh.aircraft_mesh_node_slot = PLAYTEST_AREA_NODE_SLOT;
    match &spawn_playtest_scene(&mut app, &sources, &no_mesh) {
        Err(PlaytestError::AircraftNode { what, .. }) => {
            assert_eq!(
                *what, "aircraft mesh node",
                "the refusal names which node is missing"
            );
        }
        Err(other) => panic!("a mesh slot outside the airframe must be refused, got {other:?}"),
        Ok(_) => panic!("a mesh slot outside the airframe must not spawn a scene"),
    }

    assert_eq!(
        app.world().entities().len(),
        baseline,
        "not one of those refusals left an entity behind: a refusal that spawns half a scene \
         is the failure this assertion exists for"
    );
}

/// **The capture's refusals are named and leave no file behind.**
///
/// A capture that framed sky, or one that showed no aircraft, must produce an
/// error and **no artifact**: a PNG that exists has to mean the frame behind it
/// was measured. This is checked against a scene whose aircraft is hidden by
/// construction — a live scene with the aircraft moved out of every view's frustum
/// would do it too, and the capture's own diff is what detects it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_retail_retail_a_capture_that_shows_no_aircraft_leaves_no_file() {
    use bevy::prelude::Transform;

    let config = PlaytestConfig::documented();
    let sources = read_playtest_sources(&install(), &config.world_group).expect("both read");
    let mut app = cs_app::playtest_retail::playtest_app();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("the scene spawns");
    cs_app::playtest_retail::settle_playtest_colliders(&mut app, &scene);
    let dir = capture_dir("no-aircraft");

    // Move the aircraft far outside every view's frustum, which is what a scene
    // whose aircraft is not in frame looks like from the renderer's side.
    app.world_mut()
        .entity_mut(scene.aircraft_entity())
        .insert(Transform::from_xyz(1.0e9, 1.0e9, 1.0e9));
    let result = cs_app::playtest_retail::capture_playtest_views(&mut app, &scene, &dir);
    let error = result.expect_err("a frame with no aircraft in it must be refused");
    let PlaytestError::Capture(CaptureError::NoAircraft { view }) = error else {
        panic!("an aircraft-less frame is the NoAircraft refusal, got {error}");
    };
    assert!(!view.is_empty(), "the refusal names the view it refused");
    for capture in ["chase", "quarter", "overview"] {
        let path = dir.join(format!("playtest-retail-c1c-{capture}.png"));
        assert!(
            !path.exists(),
            "a refused view leaves no file behind: {}",
            path.display()
        );
    }
}
