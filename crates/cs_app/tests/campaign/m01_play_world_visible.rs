//! Acceptance stage M01-PLAY-WORLD-VISIBLE (Rally #1433): `cs --cs-path
//! <install> --mission M01` draws M01's own world around the player.
//!
//! The defect: [`cs_app::world::residency::load_world`] spawns every world
//! object with the shared whole-mesh `Mesh3d` the collider is derived from and
//! **no material**, so the windowed mission composition drew only the player
//! airframe against an empty background — no terrain, no sea, no horizon. The
//! retail playtest scene draws its area by additionally spawning one child per
//! stored material group carrying the group's own `Mesh3d` +
//! `MeshMaterial3d`; the mission composition never did.
//!
//! Three members:
//!
//! * **The attach path is real and runs in CI.** A synthetic member builds the
//!   production harbor world through [`build_headless`], inserts the drawn
//!   parts the composition would have bound, steps, and asserts each world
//!   object's node gained its material-bound children — and that the
//!   whole-mesh node still carries **no** material, so the drawn geometry can
//!   only come from the parts. Removing
//!   [`cs_app::mission_session::MissionWorldDrawPlugin`] fails it.
//! * **M01's stage binds the world's materials.** A retail member plans M01,
//!   builds the stage, and asserts [`MissionWorldParts`] bound the definition's
//!   meshes and that the loaded world's nodes carry material-bound children.
//!   `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`; CI has no
//!   original data, so the implementing and reviewing agents run it with
//!   `--include-ignored`.
//! * **The window's own frame holds world pixels.** A retail member runs the
//!   real `cs --mission M01` capture and reads the `world` trace record's
//!   `covered_permille`, the fraction of the spawn frame that is the world's
//!   own pixels. `#[ignore = "requires CS_GAME_DIR and a window-capable GPU
//!   host"]`; it is the check the capture exists for.
//!
//! Nothing here claims the original's appearance: the material binding is the
//! #666 designed reading over the group's own textures, and the designed
//! horizon cue is labelled designed.

use std::collections::BTreeMap;

use bevy::asset::Handle;
use bevy::prelude::{Entity, MeshMaterial3d, StandardMaterial, With};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MIN_CLEAR_COVERAGE_PERMILLE, MIN_WORLD_COVERAGE_PERMILLE, MissionWorldParts, build_headless,
    stage_for,
};
use cs_app::playtest_textures::PlaytestPart;
use cs_app::world::contacts::WorldVisual;
use cs_types::content::{ContentId, Resolved};

use crate::common::label;
use crate::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// The world nodes that carry at least one material-bound drawn child: the
/// count [`build_world_draw`]'s parts make visible.
///
/// `MeshMaterial3d<StandardMaterial>` entities whose parent is a world node
/// ([`WorldVisual`]) are exactly the world's drawn parts; the player airframe's
/// own material children hang under the flight body, not a world node, so they
/// are not counted.
fn drawn_world_children(app: &mut bevy::prelude::App) -> usize {
    let world = app.world_mut();
    let parents: Vec<Entity> = {
        let mut query = world
            .query_filtered::<&bevy::prelude::ChildOf, With<MeshMaterial3d<StandardMaterial>>>();
        query
            .iter(world)
            .map(|child_of| child_of.parent())
            .collect()
    };
    parents
        .iter()
        .filter(|parent| world.get::<WorldVisual>(**parent).is_some())
        .count()
}

/// One synthetic drawn part for `mesh`: an untextured material-bound group,
/// authored here so the attach path can be driven without an installation.
fn synthetic_part() -> PlaytestPart {
    PlaytestPart {
        mesh: Handle::default(),
        material: Handle::default(),
        stored_material: 0,
        triangles: 3,
        image: None,
    }
}

/// **The composition attaches the drawn parts of every loaded world object.**
///
/// The production harbor world loads through [`build_headless`]; the drawn
/// parts the composition would have bound are inserted for the one activated
/// object, the app steps once, and the object's node must hold exactly those
/// parts as material-bound children — while the node itself keeps its
/// material-less whole mesh (the collider's own geometry). A composition whose
/// attach system is removed leaves the node with no drawn child, which is the
/// #1433 defect.
#[test]
fn accept_m01_play_world_the_composition_attaches_the_world_s_drawn_parts() {
    let scratch = Scratch::new("world-draw");
    let stage = synthetic_stage(scratch.path());
    let object = stage
        .world
        .objects()
        .iter()
        .find(|object| stage.instance.activates(object.id()))
        .expect("the synthetic stage activates one world object");
    let Resolved::Known(mesh) = object.mesh() else {
        panic!("the synthetic stage's activated object must name a mesh");
    };
    let mesh_id: ContentId = mesh.value.clone();

    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    let parts = vec![synthetic_part(), synthetic_part(), synthetic_part()];
    app.insert_resource(MissionWorldParts::from_parts(BTreeMap::from([(
        mesh_id, parts,
    )])));
    app.update();

    assert_eq!(
        drawn_world_children(&mut app),
        3,
        "the loaded world object's node must hold every drawn part as a material-bound child"
    );
    // The whole-mesh node keeps no material of its own: the collider is
    // derived from it and the drawn geometry is only the parts.
    let world = app.world_mut();
    let mut query = world
        .query_filtered::<Entity, (With<WorldVisual>, With<MeshMaterial3d<StandardMaterial>>)>();
    assert_eq!(
        query.iter(world).count(),
        0,
        "a world node must not itself carry a material; its parts do"
    );
}

/// **M01's own stage binds the world's materials, and its loaded nodes draw.**
///
/// The retail member: plan M01, build the production stage, compose headless,
/// step, and assert the stage carries the world's draw sources, the parts
/// bound more than one mesh, and the loaded world's nodes carry
/// material-bound children.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_world_retail_m01_stage_binds_and_draws_its_world() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    assert!(
        plan.launchable(),
        "every launch surface must be satisfied: {}",
        plan.gaps()
            .map(|gap| format!("{}: {}", gap.surface.label(), gap.verdict.describe()))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    assert!(
        stage.world_draw.is_some(),
        "M01's stage must carry the world's draw sources"
    );

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let parts = app.world().resource::<MissionWorldParts>();
    assert!(
        parts.mesh_count() > 1,
        "M01's world names several meshes; the draw must bind them ({} bound)",
        parts.mesh_count()
    );
    assert!(
        parts.part_count() > 0,
        "the bound meshes must produce at least one drawn part"
    );
    assert!(
        parts.report().textured_parts > 0,
        "M01's world draws from its own texture archive; at least one part must be textured"
    );
    assert!(
        drawn_world_children(&mut app) > 0,
        "M01's loaded world nodes must carry material-bound drawn children"
    );
}

/// **The window's spawn frame holds the world's own pixels.**
///
/// Runs the real `cs --mission M01` capture and reads the `world` trace
/// record: the fraction of the frame around the spawn that is geometry the
/// world load drew, counted against the window's clear colour with the
/// horizon's dome and sea and the player hidden — and the clear colour the probe must
/// still have found there, which is what proves the sky's own gradient was
/// not counted as terrain. The check the capture exists for: a frame with no
/// world pixels is the #1433 defect.
#[test]
#[ignore = "requires CS_GAME_DIR and a window-capable GPU host"]
fn accept_m01_play_world_windowed_mission_capture_draws_world_pixels() {
    let dir = std::env::var("CS_M01_PLAY_WORLD_OUT").map_or_else(
        |_| std::env::temp_dir().join(format!("cs_m01_play_world_{}", std::process::id())),
        std::path::PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cs"))
        .args(["--cs-path"])
        .arg(game_dir())
        .args(["--mission", "M01"])
        .env("CS_MISSION_CAPTURE", "1")
        .env("CS_EVIDENCE_DIR", &dir)
        .env("CS_CAPABILITIES", "retail,gpu,audio")
        .output()
        .expect("the cs binary runs");
    assert!(
        output.status.success(),
        "the mission capture must pass: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let trace = std::fs::read_to_string(dir.join("trace.jsonl")).expect("trace.jsonl");
    let world = trace
        .lines()
        .find(|line| line.contains("\"kind\":\"world\""))
        .expect("the trace must hold the world probe record");
    let field = |name: &str| -> u32 {
        world
            .split(name)
            .nth(1)
            .and_then(|rest| rest.split([',', '}']).next())
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_else(|| panic!("the world record has no {name}: {world}"))
    };
    let covered = field("\"covered_permille\":");
    assert!(
        covered >= MIN_WORLD_COVERAGE_PERMILLE,
        "the spawn frame must hold the world's own pixels: {world}"
    );
    let clear = field("\"clear_permille\":");
    assert!(
        clear >= MIN_CLEAR_COVERAGE_PERMILLE,
        "the probe frame must still hold the window's clear colour, or the sky was not hidden \
         and its own gradient would be the 'world' pixels: {world}"
    );
    assert!(
        dir.join("trace.jsonl").exists()
            && std::fs::read_dir(&dir).is_ok_and(|mut entries| entries.any(|entry| entry
                .map(|entry| entry.file_name().to_string_lossy().contains("_world.png"))
                .unwrap_or(false))),
        "the world probe's own frame must be written under {}",
        dir.display()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
