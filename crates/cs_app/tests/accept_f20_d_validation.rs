//! Acceptance scenarios F20-D: validation of every mission-critical original
//! animation family, and the stage's skip scenario through the production
//! path.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-D`. Task test prefix: `accept_f20_d_`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! What is exercised, in three bands:
//!
//! * **Synthetic, unignored** — the AC04 minimum scenario (a mission-marked
//!   clip skipped through the *scheduled* production entry — the
//!   [`CommittedSessionTick`] stamp plus
//!   [`advance_animation_on_session_tick`] — reaches its final semantic state
//!   exactly once), the multi-marker and looping-skip rules, the capture
//!   refusals that do not need an adapter, and the survey's own contract on a
//!   synthetic installation (every blocker kind is named, never a dropped
//!   row).
//! * **Retail** — `#[ignore = "requires CS_GAME_DIR"]`: the survey runs over
//!   the owner's original installation and asserts the measured inventory:
//!   every world group carries a `cam_anim.zbd`, every launchable directory a
//!   `mis_anim.zbd`, all of them validating the documented signature and
//!   version, each paired with its `*.zrd` record in the sibling `zrdr.zbd`,
//!   and the campaign's declared missions covered. The container payloads
//!   stay undecoded — that is recorded, not claimed otherwise.
//! * **GPU** — `#[ignore = "requires a GPU adapter…"]`: an evaluated
//!   [`PoseSample`] produced by the playing clip is drawn on the real
//!   renderer through [`capture_animated_pose`], for a synthetic mesh and for
//!   a mesh read out of the retail installation.
//!
//! The retail/GPU tests **fail loudly** when their capability is absent —
//! nothing in this file passes vacuously.
//!
//! **Task #633 (`M01-LC-ANIM-CARRIERS`, prefix `accept_m01_lc_anim_carriers_`)
//! is in this file too**, in its own section at the end: the survey above
//! counts carriers and digests their sibling reader members, and #633 is the
//! half that reads *contents* — the carrier's own front index (the animation
//! family has no trailer, so `cs_formats::zbd::anim` reads it), the payload
//! header in front of its records, and the join from the scope's paired
//! `mis_anim.zrd`/`cam_anim.zrd` definition files to the member rows those
//! names resolve to. It lives here rather than in a new test binary because
//! every run that added one at this crate's tests root died on the CI runner's
//! disk (task #637); `F20`'s own owner path already covers this file.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use bevy::ecs::world::World;
use bevy::prelude::GlobalTransform;
use cs_app::animation::{
    AnimationInstance, AnimationLog, AnimationPlayback, AnimationRefusal, CarrierBlocker,
    CarrierKind, CommittedSessionTick, NodeAnimatedPose, PoseCaptureError, PoseCaptureRequest,
    advance_animation, advance_animation_on_session_tick, bind_animated_node,
    capture_animated_pose, survey_animation_families,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration, SceneNodeBinding};
use cs_app::world::survey_world_groups;
use cs_content::animation::{
    AnimationChannel, AnimationClip, EventMarker, Interpolation, LoopMode, MarkerEffect,
    SYNTHETIC_DOOR_MARKER, SYNTHETIC_DOOR_OPEN_TICK, SYNTHETIC_PROPELLER_GAMEPLAY_MARKER,
    SYNTHETIC_PROPELLER_NODE, SYNTHETIC_PROPELLER_PRESENTATION_MARKER, TransformChannel,
    TransformKey, TransformSample, declared_synthetic_door_clip, declared_synthetic_propeller_clip,
};
use cs_content::mesh::RenderMesh;
use cs_content::scene::SceneNodeId;
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_sim::animated_object::{AnimationEvent, PoseSample};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, Radians, UnitVec3};

// -------------------------------------------------------------- helpers ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this test"),
    )
}

fn content_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn node(key: &str) -> ContentId {
    content_id(ContentKind::SceneNode, key)
}

fn scene_node(key: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(node(key)).expect("a scene-node id")
}

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

fn generation() -> SceneGeneration {
    SceneGeneration::default().next()
}

/// The private evidence directory the GPU captures write their PNGs into.
fn evidence_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../private/evidence/F20-D")
        .canonicalize()
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../private/evidence/F20-D")
                .to_path_buf()
        })
}

/// A world holding the resources the playback needs, nothing more: the
/// session-keyed playback and the empty event log it publishes into.
fn playback_world() -> World {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(7)));
    world.insert_resource(AnimationLog::new());
    world
}

/// Spawns a scene node entity — the same components the spawn path's
/// products carry — and binds it through the production spawn entry.
fn bound_node(
    world: &mut World,
    clip: &AnimationClip,
    node_key: &str,
    instance: AnimationInstance,
    at: Tick,
) -> bevy::ecs::entity::Entity {
    let entity = world
        .spawn((
            SceneNodeBinding {
                node: node(node_key),
                generation: generation(),
            },
            NodeVisualTransform(GlobalTransform::IDENTITY),
        ))
        .id();
    bind_animated_node(
        world,
        clip,
        entity,
        &node(node_key),
        instance,
        generation(),
        at,
    )
    .expect("the binding verifies");
    entity
}

/// The production scheduled advance, driven by writing the committed tick
/// stamp — what the session driver itself writes each fixed tick, and what
/// a cinematic skip jumps forward.
fn commit_and_advance(world: &mut World, tick: u64) {
    world.insert_resource(CommittedSessionTick(Tick(tick)));
    advance_animation_on_session_tick(world);
}

fn fired_events(world: &World) -> Vec<AnimationEvent> {
    world
        .get_resource::<AnimationLog>()
        .map(|log| log.events().to_vec())
        .unwrap_or_default()
}

fn markers(world: &World, key: &str) -> usize {
    fired_events(world)
        .iter()
        .filter(|event| event.marker == key)
        .count()
}

/// An asymmetric synthetic mesh, built through the one production
/// constructor ([`RenderMesh::build`]) from a raw stored record: a thin
/// blade, long on `x`, so a rotation about `z` changes the frame it draws.
fn blade_mesh() -> RenderMesh {
    RenderMesh::build(&RawMesh {
        positions: vec![
            [-2.0, 0.0, -0.4],
            [2.0, 0.0, -0.4],
            [2.0, 0.1, 0.4],
            [-2.0, 0.1, 0.4],
        ],
        normals: Vec::new(),
        polygons: [[0, 1, 2], [0, 2, 3]]
            .into_iter()
            .map(|triangle| RawPolygon {
                kind: PrimitiveKind::Polygon,
                raw_flags: 0,
                material: 0,
                corners: triangle
                    .into_iter()
                    .map(|position| RawCorner {
                        position,
                        normal: None,
                        uv: Some([0.0, 0.0]),
                        color: None,
                    })
                    .collect(),
            })
            .collect(),
    })
    .expect("authored blade geometry has a decodable outline")
}

/// The same record with no drawable triangle at all.
fn empty_mesh() -> RenderMesh {
    RenderMesh::build(&RawMesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
        normals: Vec::new(),
        polygons: Vec::new(),
    })
    .expect("a mesh with no polygon has nothing to validate")
}

// -------------------------------------- AC04: the mission-marker skip -----

/// AC04 — skip an animation containing a mission marker; the final semantic
/// state is reached exactly once.
///
/// Driven through the production *scheduled* entry, the same one the
/// session's own fixed-tick driver uses: a skip is the committed tick stamp
/// jumping forward, which is what a cinematic skip does to the session's
/// clock. The door clip carries the one-shot gameplay marker
/// `door_opened` at tick 10 of a 30-tick `LoopMode::Once` clip.
#[test]
fn accept_f20_d_skipping_a_mission_marked_clip_reaches_its_final_state_exactly_once() {
    let mut world = playback_world();
    let clip = declared_synthetic_door_clip();
    let door = bound_node(
        &mut world,
        &clip,
        "synthetic.hangar.door",
        instance(1),
        Tick(0),
    );

    // The clip plays one committed tick: the marker is still ahead.
    commit_and_advance(&mut world, 1);
    assert_eq!(markers(&world, SYNTHETIC_DOOR_MARKER), 0);

    // The skip: the committed stamp jumps past the whole clip.
    commit_and_advance(&mut world, 500);

    // The marker fired exactly once, stamped with the skip's own tick and
    // the session's event identity.
    let events = fired_events(&world);
    assert_eq!(
        events.len(),
        1,
        "the skip fired the marker once, not per tick crossed"
    );
    let event = &events[0];
    assert_eq!(event.marker, SYNTHETIC_DOOR_MARKER);
    assert_eq!(event.id.tick, Tick(500));
    assert_eq!(event.id.session, session(7));
    assert!(event.effect.is_gameplay());
    assert_eq!(event.pass, 0);

    // The final semantic state is applied, once: the terminal pose sits on
    // the bound node and the instance reports itself finished.
    let applied = world
        .get::<NodeAnimatedPose>(door)
        .expect("the bound node carries the applied pose");
    let open = PoseSample::try_new(
        Quaternion::from_axis_angle(UnitVec3::UP, Radians(std::f64::consts::FRAC_PI_2))
            .expect("a quarter turn is unit length"),
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    )
    .expect("the fixture pose is finite");
    assert_eq!(
        applied.0, open,
        "the skipped clip applied its terminal pose, not an intermediate one"
    );
    let playback = world.resource::<AnimationPlayback>();
    assert_eq!(
        playback.is_finished(clip.id(), instance(1)),
        Some(true),
        "the skipped `Once` clip is finished"
    );
    let advances_after_skip = playback.advances();

    // "Exactly once" — a repeated stamp is the schedule's own no-op rule:
    // no advance pass runs at all, let alone a second application.
    commit_and_advance(&mut world, 500);
    commit_and_advance(&mut world, 500);
    assert_eq!(
        world.resource::<AnimationPlayback>().advances(),
        advances_after_skip,
        "a repeated committed tick advances nothing"
    );

    // A later stamp still reaches nothing new: the clip is at its end.
    commit_and_advance(&mut world, 900);
    assert_eq!(markers(&world, SYNTHETIC_DOOR_MARKER), 1);
    assert_eq!(
        fired_events(&world).len(),
        1,
        "no second marker, no repeated event"
    );
    assert!(
        world.resource::<AnimationLog>().refusals().is_empty(),
        "the skip itself reports no refusal"
    );

    // And a stamp behind the head is the documented hold — published once,
    // not replayed: the marker count and the terminal pose do not move.
    commit_and_advance(&mut world, 4);
    let refusals = world.resource::<AnimationLog>().refusals();
    assert_eq!(
        refusals.len(),
        1,
        "a backwards committed tick publishes the hold once"
    );
    assert!(matches!(refusals[0], AnimationRefusal::Held { .. }));
    assert_eq!(markers(&world, SYNTHETIC_DOOR_MARKER), 1);
    assert_eq!(world.get::<NodeAnimatedPose>(door).expect("pose").0, open);
}

/// A skip past several markers fires each exactly once, in authored tick
/// order — the marker stream of a jump is the same stream a slow advance
/// would have published.
#[test]
fn accept_f20_d_skipping_past_several_markers_fires_each_once_in_tick_order() {
    let designed = || Provenance::designed(ClaimId::new("f20d.two-marker").expect("claim id"));
    let clip = AnimationClip::try_new(
        content_id(ContentKind::AnimationTrack, "synthetic.two_marker"),
        Origin::SyntheticFixture,
        40,
        LoopMode::Once,
        vec![AnimationChannel::Transform(TransformChannel {
            target: scene_node("synthetic.gate"),
            interpolation: Interpolation::Step,
            keys: vec![
                TransformKey {
                    tick: 0,
                    pose: TransformSample::IDENTITY,
                },
                TransformKey {
                    tick: 30,
                    pose: TransformSample::try_new(
                        Quaternion::IDENTITY,
                        [4.0, 0.0, 0.0],
                        [1.0, 1.0, 1.0],
                    )
                    .expect("the fixture pose is finite"),
                },
            ],
        })],
        vec![
            EventMarker {
                tick: 5,
                key: "phase_one".to_owned(),
                effect: Resolved::Known(Known::new(
                    MarkerEffect::Gameplay {
                        cue: "synthetic.phase_one".to_owned(),
                    },
                    designed(),
                )),
            },
            EventMarker {
                tick: 12,
                key: "phase_two".to_owned(),
                effect: Resolved::Known(Known::new(
                    MarkerEffect::Gameplay {
                        cue: "synthetic.phase_two".to_owned(),
                    },
                    designed(),
                )),
            },
            EventMarker {
                tick: 20,
                key: "sweep_done".to_owned(),
                effect: Resolved::Known(Known::new(
                    MarkerEffect::Presentation {
                        cue: "synthetic.sweep_done".to_owned(),
                    },
                    designed(),
                )),
            },
        ],
        designed(),
    )
    .expect("the declared clip is valid");

    let mut world = playback_world();
    bound_node(&mut world, &clip, "synthetic.gate", instance(1), Tick(0));

    // The skip: straight past all three markers and the clip's own end.
    commit_and_advance(&mut world, 200);
    let events = fired_events(&world);
    assert_eq!(
        events
            .iter()
            .map(|event| event.marker.as_str())
            .collect::<Vec<_>>(),
        vec!["phase_one", "phase_two", "sweep_done"],
        "every crossed marker fired once, in authored order"
    );
    assert!(
        events.iter().all(|event| event.id.tick == Tick(200)),
        "a skipped marker is stamped with the tick it was committed on"
    );

    // Repeated stamps reach nothing new — "exactly once" per marker.
    commit_and_advance(&mut world, 200);
    commit_and_advance(&mut world, 400);
    let events = fired_events(&world);
    assert_eq!(events.len(), 3, "no marker fires twice");
}

/// A looping clip skipped across several passes: the one-shot gameplay
/// marker still fires exactly once for the activation, the presentation
/// marker fires once per crossed pass, and the evaluated pose lands on the
/// reached position rather than the terminal one.
#[test]
fn accept_f20_d_a_looping_clip_skipped_across_passes_fires_its_oneshot_marker_once() {
    let mut world = playback_world();
    let clip = declared_synthetic_propeller_clip();
    bound_node(
        &mut world,
        &clip,
        SYNTHETIC_PROPELLER_NODE,
        instance(2),
        Tick(0),
    );

    // Skip 13 ticks of a 4-tick loop: three full passes plus a tick, so the
    // head lands mid-pass — position 1, pass 3.
    commit_and_advance(&mut world, 13);

    let gameplay = markers(&world, SYNTHETIC_PROPELLER_GAMEPLAY_MARKER);
    assert_eq!(
        gameplay, 1,
        "the one-shot gameplay marker fired once across the whole skip"
    );
    // The presentation marker carries no one-shot promise: it fires once per
    // pass its instant was crossed in — passes 0..=3 of the jump.
    let presentations = markers(&world, SYNTHETIC_PROPELLER_PRESENTATION_MARKER);
    assert_eq!(presentations, 4, "one presentation event per crossed pass");

    // The state the skip reached is the wrapped position, not the end.
    let playback = world.resource::<AnimationPlayback>();
    assert_eq!(playback.is_finished(clip.id(), instance(2)), Some(false));

    // Continuing past the skip publishes no new one-shot event.
    commit_and_advance(&mut world, 40);
    assert_eq!(
        markers(&world, SYNTHETIC_PROPELLER_GAMEPLAY_MARKER),
        1,
        "the one-shot marker stays fired exactly once"
    );
}

// ------------------------------------------- the capture's own refusals ---
//
// These refuse before a renderer is ever built, so they need no adapter.

/// A pose that cannot be carried into the render affine is refused by name
/// and writes no file — never a capture of an approximated pose.
#[test]
fn accept_f20_d_a_pose_that_does_not_fit_the_render_affine_is_refused() {
    let render = blade_mesh();
    let unknowns: &[cs_content::mesh::MeshPresentationUnknown] = &[];
    let dir = std::env::temp_dir().join(format!("f20d-unrepresentable-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the refusal directory");
    let png = dir.join("should-not-exist.png");
    let pose = PoseSample::try_new(Quaternion::IDENTITY, [1.0e300, 0.0, 0.0], [1.0, 1.0, 1.0])
        .expect("the f64 pose is finite and valid");
    let result = capture_animated_pose(&PoseCaptureRequest {
        label: "synthetic.unrepresentable",
        render: &render,
        unknowns,
        pose,
        png: &png,
    });
    assert!(
        matches!(result, Err(PoseCaptureError::UnrepresentablePose { .. })),
        "expected UnrepresentablePose, got {result:?}"
    );
    assert!(
        !png.exists(),
        "a refused capture leaves no PNG that could read as evidence"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A mesh with no drawable triangle is refused by name and writes no file.
#[test]
fn accept_f20_d_an_empty_mesh_is_refused_rather_than_captured() {
    let empty = empty_mesh();
    let dir = std::env::temp_dir().join(format!("f20d-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the refusal directory");
    let png = dir.join("should-not-exist.png");
    let result = capture_animated_pose(&PoseCaptureRequest {
        label: "synthetic.empty",
        render: &empty,
        unknowns: &[],
        pose: PoseSample::IDENTITY,
        png: &png,
    });
    assert!(
        matches!(result, Err(PoseCaptureError::EmptyMesh { .. })),
        "expected EmptyMesh, got {result:?}"
    );
    assert!(!png.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------- the survey contract ---
//
// A synthetic installation under a temporary directory: just enough of the
// layout for production discovery to run — a `ZBD/<group>` directory with
// its `cam_anim.zbd`/`zrdr.zbd`, a campaign mission dir `M01`, a non-campaign
// launchable dir `IA1`, and a content-root `zrdr.zbd` with an unpaired
// animation member.

/// The bytes of one animation container: the documented signature, the
/// retail version word, and a marker payload (undecoded, as on the retail
/// installation).
fn carrier_bytes(payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0817_0616_u32.to_le_bytes());
    bytes.extend_from_slice(&53_u32.to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// A `zrdr.zbd` in the version-one trailer layout the production reader
/// reads: member bytes, then 148-byte index entries, then the 8-byte
/// trailer (`version = 1`, `count`).
fn reader_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut index = Vec::new();
    for (name, content) in members {
        let start = bytes.len() as u32;
        bytes.extend_from_slice(content);
        index.extend_from_slice(&start.to_le_bytes());
        index.extend_from_slice(&(content.len() as u32).to_le_bytes());
        let mut name_bytes = [0_u8; 64];
        name_bytes[..name.len()].copy_from_slice(name.as_bytes());
        index.extend_from_slice(&name_bytes);
        index.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&index);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// Writes `files` under a fresh directory and returns its path.
///
/// The directory is deliberately left behind for the OS to reap: the
/// manifest only reads it, and deleting it eagerly while a survey holds no
/// handle to it is pointless ceremony.
fn write_install(label: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "f20d-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    for (name, bytes) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().expect("a file under the root"))
            .expect("create the install layout");
        std::fs::File::create(&path)
            .and_then(|mut file| file.write_all(bytes))
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }
    root
}

/// A complete minimal installation: every expected carrier present and
/// valid, one non-campaign launchable directory, and an unpaired
/// `anim.zrd` in the content-root reader.
fn complete_install(label: &str) -> PathBuf {
    write_install(
        label,
        &[
            ("ZBD/zrdr.zbd", &reader_bytes(&[("anim.zrd", b"root-anim")])),
            (
                "ZBD/C1/zrdr.zbd",
                &reader_bytes(&[("cam_anim.zrd", b"camera-anim")]),
            ),
            ("ZBD/C1/cam_anim.zbd", &carrier_bytes(b"camera")),
            (
                "ZBD/C1/M01/zrdr.zbd",
                &reader_bytes(&[
                    ("mis_anim.zrd", b"mission-anim"),
                    ("startanims.zrd", b"start-anims"),
                ]),
            ),
            ("ZBD/C1/M01/mis_anim.zbd", &carrier_bytes(b"m01")),
            (
                "ZBD/C1/IA1/zrdr.zbd",
                &reader_bytes(&[("mis_anim.zrd", b"ia-anim")]),
            ),
            ("ZBD/C1/IA1/mis_anim.zbd", &carrier_bytes(b"ia1")),
        ],
    )
}

/// The survey validates every carrier of a complete installation and
/// records the whole `*anim*` member census — paired records and extras.
#[test]
fn accept_f20_d_survey_of_a_synthetic_installation_validates_every_carrier() {
    let install = complete_install("complete");
    let survey = survey_animation_families(&install).expect("the survey runs");

    assert!(
        survey.is_clean(),
        "a complete installation reports no blockers: {survey:#?}"
    );
    assert!(survey.strays.is_empty());

    // Three scopes: the world group (camera) and the two launchable dirs.
    assert_eq!(survey.scopes.len(), 3);
    assert_eq!(survey.scopes_of(CarrierKind::Camera).count(), 1);
    assert_eq!(survey.scopes_of(CarrierKind::Mission).count(), 2);

    let camera = survey
        .scope("zbd/c1")
        .expect("the world-group scope row")
        .state
        .as_ref()
        .expect("the carrier validated");
    assert_eq!(camera.version, 53, "the documented retail version word");
    assert_eq!(camera.animation_members.len(), 1);
    assert_eq!(camera.animation_members[0].name, "cam_anim.zrd");

    // Every carrier carries its paired member; mission readers carry the
    // extra animation members too (`startanims.zrd` here).
    for carrier in survey.carriers() {
        assert!(
            carrier.paired_member().is_some(),
            "{} has no paired member",
            carrier.container_key
        );
        assert!(carrier.size_bytes > 0 && !carrier.sha256.to_hex().is_empty());
    }
    let m01 = survey
        .scope("zbd/c1/m01")
        .expect("the campaign mission scope row")
        .state
        .as_ref()
        .expect("the carrier validated");
    assert_eq!(
        m01.animation_members
            .iter()
            .map(|member| member.name.as_str())
            .collect::<Vec<_>>(),
        vec!["mis_anim.zrd", "startanims.zrd"],
        "the member census is complete, not just the paired name"
    );

    // The content-root reader's `anim.zrd` is recorded as unpaired rather
    // than silently skipped.
    assert_eq!(survey.unpaired_members.len(), 1);
    assert_eq!(survey.unpaired_members[0].member.name, "anim.zrd");

    // Every carrier hashed into a payload family, none lost.
    let carriers_in_families: usize = survey
        .payload_families()
        .iter()
        .map(|family| family.carriers.len())
        .sum();
    assert_eq!(carriers_in_families, 3);
}

/// A scope whose carrier file is absent reports the blocker by name — the
/// row exists; it does not silently vanish.
#[test]
fn accept_f20_d_a_scope_missing_its_animation_carrier_is_a_named_blocker() {
    let install = write_install(
        "missing-carrier",
        &[
            ("ZBD/C1/zrdr.zbd", &reader_bytes(&[("cam_anim.zrd", b"c")])),
            ("ZBD/C1/cam_anim.zbd", &carrier_bytes(b"camera")),
            // The mission dir's reader is present (so the layout expects the
            // scope) but its `mis_anim.zbd` is not.
            (
                "ZBD/C1/M01/zrdr.zbd",
                &reader_bytes(&[("mis_anim.zrd", b"m")]),
            ),
        ],
    );
    let survey = survey_animation_families(&install).expect("the survey runs");
    assert!(!survey.is_clean());
    match &survey
        .scope("zbd/c1/m01")
        .expect("the row exists — a blocked scope is reported, not dropped")
        .state
    {
        Err(CarrierBlocker::MissingCarrier { expected_key }) => {
            assert_eq!(expected_key, "zbd/c1/m01/mis_anim.zbd");
        }
        other => panic!("expected MissingCarrier, got {other:?}"),
    }
    // The camera scope is unaffected: a blocker names its scope only.
    assert!(survey.scope("zbd/c1").expect("camera row").state.is_ok());
}

/// A carrier whose bytes fail the documented signature probe is a named
/// blocker — the survey validates the header, it does not trust the name.
#[test]
fn accept_f20_d_a_carrier_with_a_wrong_header_is_a_named_blocker() {
    let install = write_install(
        "bad-header",
        &[
            ("ZBD/C1/zrdr.zbd", &reader_bytes(&[("cam_anim.zrd", b"c")])),
            ("ZBD/C1/cam_anim.zbd", &carrier_bytes(b"camera")),
            (
                "ZBD/C1/M01/zrdr.zbd",
                &reader_bytes(&[("mis_anim.zrd", b"m")]),
            ),
            (
                "ZBD/C1/M01/mis_anim.zbd",
                &[0xde, 0xad, 0xbe, 0xef, 0x35, 0x00, 0x00, 0x00],
            ),
        ],
    );
    let survey = survey_animation_families(&install).expect("the survey runs");
    match &survey.scope("zbd/c1/m01").expect("the row exists").state {
        Err(CarrierBlocker::DispatchRefused { key, code }) => {
            assert_eq!(key, "zbd/c1/m01/mis_anim.zbd");
            assert_eq!(code, &"header_mismatch");
        }
        other => panic!("expected DispatchRefused, got {other:?}"),
    }
}

/// A sibling reader that lacks the paired `mis_anim.zrd` member is a named
/// blocker — the record half of the family is part of the validation.
#[test]
fn accept_f20_d_a_reader_without_the_paired_member_is_a_named_blocker() {
    let install = write_install(
        "absent-member",
        &[
            ("ZBD/C1/zrdr.zbd", &reader_bytes(&[("cam_anim.zrd", b"c")])),
            ("ZBD/C1/cam_anim.zbd", &carrier_bytes(b"camera")),
            (
                "ZBD/C1/M01/zrdr.zbd",
                &reader_bytes(&[("startanims.zrd", b"extra"), ("objectives.zrd", b"o")]),
            ),
            ("ZBD/C1/M01/mis_anim.zbd", &carrier_bytes(b"m01")),
        ],
    );
    let survey = survey_animation_families(&install).expect("the survey runs");
    match &survey.scope("zbd/c1/m01").expect("the row exists").state {
        Err(CarrierBlocker::MemberAbsent { reader_key, member }) => {
            assert_eq!(reader_key, "zbd/c1/m01/zrdr.zbd");
            assert_eq!(member, &"mis_anim.zrd");
        }
        other => panic!("expected MemberAbsent, got {other:?}"),
    }
}

/// A carrier whose directory has no `zrdr.zbd` at all still produces a scope
/// row — the carrier file itself declares the scope — and the missing
/// reader is the named blocker.
#[test]
fn accept_f20_d_a_carrier_without_a_sibling_reader_is_a_named_blocker() {
    let install = write_install(
        "no-reader",
        &[
            ("ZBD/C1/zrdr.zbd", &reader_bytes(&[("cam_anim.zrd", b"c")])),
            ("ZBD/C1/cam_anim.zbd", &carrier_bytes(b"camera")),
            ("ZBD/C1/M01/mis_anim.zbd", &carrier_bytes(b"m01")),
        ],
    );
    let survey = survey_animation_families(&install).expect("the survey runs");
    match &survey
        .scope("zbd/c1/m01")
        .expect("the carrier's presence creates the scope row")
        .state
    {
        Err(CarrierBlocker::MissingReader { expected_key }) => {
            assert_eq!(expected_key, "zbd/c1/m01/zrdr.zbd");
        }
        other => panic!("expected MissingReader, got {other:?}"),
    }
}

// --------------------------------------------------------------- retail ---

/// The retail inventory: every mission-critical animation carrier of the
/// owner's original installation is present, validated against the
/// documented signature/version, fingerprinted, and paired with its `*.zrd`
/// member; the survey is clean end to end.
///
/// This is F20-D's `retail` half. It does **not** decode the payloads —
/// nothing in production does — so it asserts the carrier records, the
/// member census and the digest families, and the undecoded payload state
/// is what the findings record.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_f20_d_retail_every_animation_carrier_is_present_validated_and_fingerprinted() {
    let survey =
        survey_animation_families(&game_dir()).expect("the survey runs over the installation");

    let blockers: Vec<String> = survey
        .blockers()
        .map(|(scope, blocker)| format!("{scope}: {blocker}"))
        .collect();
    assert!(
        blockers.is_empty(),
        "the original installation validates every carrier; blockers: {blockers:?}"
    );
    assert!(
        survey.strays.is_empty(),
        "no animation container sits outside the layout: {:?}",
        survey.strays
    );

    // The measured inventory: one camera carrier per world group, one
    // mission carrier per launchable directory — mission, instant-action
    // and multiplayer scopes alike.
    let mission: Vec<_> = survey
        .carriers()
        .filter(|carrier| carrier.kind == CarrierKind::Mission)
        .collect();
    let camera: Vec<_> = survey
        .carriers()
        .filter(|carrier| carrier.kind == CarrierKind::Camera)
        .collect();
    assert_eq!(mission.len(), 53, "one mis_anim.zbd per launchable scope");
    assert_eq!(camera.len(), 8, "one cam_anim.zbd per world group");

    for carrier in survey.carriers() {
        // All carriers are header+role matches (the observed positions), and
        // all validated the retail version word.
        assert_eq!(carrier.version, 53, "{}", carrier.container_key);
        assert!(
            carrier.paired_member().is_some(),
            "{}: the sibling reader carries the paired record",
            carrier.container_key
        );
    }

    // The mission member census: every mission reader carries `mis_anim.zrd`
    // and `startanims.zrd`; the extras the census found are recorded, not
    // dropped.
    let extra_members: BTreeSet<String> = mission
        .iter()
        .flat_map(|carrier| {
            carrier
                .animation_members
                .iter()
                .map(|member| member.name.to_ascii_lowercase())
        })
        .collect();
    assert!(extra_members.contains("mis_anim.zrd"));
    assert!(
        extra_members.contains("startanims.zrd"),
        "every mission reader carries the startanims record too"
    );

    // The content-root reader's animation members pair with no carrier and
    // are recorded unpaired (`anim.zrd`, `map_anims.zrd`).
    let unpaired: BTreeSet<String> = survey
        .unpaired_members
        .iter()
        .map(|member| member.member.name.to_ascii_lowercase())
        .collect();
    assert!(unpaired.contains("anim.zrd") && unpaired.contains("map_anims.zrd"));

    // The digest census is a byte-level fact: identical payloads group, and
    // every carrier is in exactly one family.
    let families = survey.payload_families();
    assert!(!families.is_empty());
    let accounted: usize = families.iter().map(|family| family.carriers.len()).sum();
    assert_eq!(accounted, 61);
    for family in &families {
        assert_eq!(family.sha256.to_hex().len(), 64);
    }
}

/// The campaign's own declaration: every mission directory the campaign
/// layout names carries a validated `mis_anim.zbd` paired with its
/// `mis_anim.zrd` — the mission-critical carrier by the game's own mission
/// list, not just by directory walking.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_f20_d_retail_every_declared_campaign_mission_carries_its_animation() {
    let dir = game_dir();
    let layout = cs_content::campaign_bindings::campaign_layout(&dir)
        .expect("the campaign layout walks the installation");
    let survey = survey_animation_families(&dir).expect("the survey runs");

    // The measured campaign: 24 missions across the chapters.
    assert_eq!(layout.len(), 24);
    for entry in &layout {
        assert!(
            entry.mission.program_present,
            "ch{} m{:02} declares its program archive",
            entry.mission.chapter, entry.mission.mission_number
        );
        let scope = entry
            .mission
            .program_asset
            .rsplit_once('/')
            .map(|(dir, _)| dir.to_ascii_lowercase())
            .expect("the program asset sits in the mission directory");
        let row = survey
            .scope(&scope)
            .unwrap_or_else(|| panic!("the campaign mission {scope} has a scope row"));
        let carrier = row.state.as_ref().unwrap_or_else(|blocker| {
            panic!("campaign mission {scope} validates its carrier: {blocker}")
        });
        assert_eq!(carrier.kind, CarrierKind::Mission);
        assert_eq!(carrier.version, 53);
        assert!(carrier.paired_member().is_some());
    }
}

// ------------------------------------------------------------------ gpu ---

/// An evaluated pose reaches a real rendered frame: the clip plays through
/// the production session path, the pose the binding applied is read back
/// out of [`NodeAnimatedPose`], and the capture draws the mesh at it on the
/// real adapter. Two ticks of the same playing clip produce two distinct
/// frames — the transform track's output is visible in the image, not just
/// in the component.
#[test]
#[ignore = "requires a GPU adapter: the capture renders on the real device"]
fn accept_f20_d_a_clip_evaluated_pose_reaches_a_distinct_rendered_frame() {
    let dir = evidence_dir();
    std::fs::create_dir_all(&dir).expect("the evidence directory exists");

    let mut world = playback_world();
    let clip = declared_synthetic_propeller_clip();
    let entity = bound_node(
        &mut world,
        &clip,
        SYNTHETIC_PROPELLER_NODE,
        instance(1),
        Tick(0),
    );
    let render = blade_mesh();
    let unknowns: &[cs_content::mesh::MeshPresentationUnknown] = &[];

    let mut capture = |tick: u64, name: &str| {
        advance_animation(&mut world, Tick(tick));
        let pose = world
            .get::<NodeAnimatedPose>(entity)
            .expect("the bound node carries the evaluated pose")
            .0;
        capture_animated_pose(&PoseCaptureRequest {
            label: name,
            render: &render,
            unknowns,
            pose,
            png: &dir.join(format!("{name}.png")),
        })
        .expect("the posed mesh draws on the real adapter")
    };

    // Tick 1 of the propeller is a quarter turn about the forward axis: the
    // blade stands across the view instead of lying in it, so the two frames
    // differ by construction.
    let first = capture(0, "f20d-propeller-t0");
    let quarter = capture(1, "f20d-propeller-t1-quarterturn");
    assert!(first.drew_geometry() && quarter.drew_geometry());
    assert!(
        first.png_sha256 != quarter.png_sha256,
        "two ticks of the playing clip draw two different frames"
    );
    println!(
        "f20d gpu: adapter={} t0={}px t1={}px",
        first.adapter, first.covered_pixels, quarter.covered_pixels
    );
}

/// A pose is drawn at its own orientation, not merely *framed* by it: two
/// poses that put the mesh in the same posed AABB — so the capture computes
/// the same camera for both — but orient it differently must still return two
/// different frames.
///
/// This is the discriminating check for the pose reaching the render
/// pipeline. If the pose is written somewhere the transform propagation
/// overwrites (for example a bare `GlobalTransform` beside the identity
/// `Transform` that `Mesh3d` requires), both captures draw the mesh at the
/// origin and come back byte-identical even though the camera that framed
/// them moved — so identical digests here are a failure, not a coincidence.
#[test]
#[ignore = "requires a GPU adapter: the capture renders on the real device"]
fn accept_f20_d_a_pose_draws_its_orientation_even_when_the_bounds_are_unchanged() {
    let dir = evidence_dir();
    std::fs::create_dir_all(&dir).expect("the evidence directory exists");
    let render = blade_mesh();
    let unknowns: &[cs_content::mesh::MeshPresentationUnknown] = &[];

    // `blade_mesh` is long on `x` and thin on `z`. A quarter turn either way
    // about `UP` swaps those extents, so both poses share one posed AABB —
    // identical centre and radius, hence identical camera framing — while the
    // blade's thin edge tilts to opposite sides of the view.
    let quarter_turn = |turn: f64| {
        PoseSample::try_new(
            Quaternion::from_axis_angle(UnitVec3::UP, Radians(turn))
                .expect("a quarter turn about a unit axis"),
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        )
        .expect("the fixture pose is finite")
    };
    let capture = |name: &str, pose: PoseSample| {
        capture_animated_pose(&PoseCaptureRequest {
            label: name,
            render: &render,
            unknowns,
            pose,
            png: &dir.join(format!("{name}.png")),
        })
        .unwrap_or_else(|error| panic!("{name} could not be captured: {error}"))
    };

    let up = capture(
        "f20d-orientation-up",
        quarter_turn(std::f64::consts::FRAC_PI_2),
    );
    let down = capture(
        "f20d-orientation-down",
        quarter_turn(-std::f64::consts::FRAC_PI_2),
    );
    assert!(up.drew_geometry() && down.drew_geometry());
    assert_ne!(
        up.png_sha256, down.png_sha256,
        "opposite quarter turns frame the mesh identically but draw it in different \
         orientations; identical digests mean the pose never reached the drawn frame"
    );
    println!(
        "f20d gpu: adapter={} orientation up={}px down={}px",
        up.adapter, up.covered_pixels, down.covered_pixels
    );
}

/// The retail end of the `gpu` capability: a real mesh read out of the
/// original installation, driven by a playing clip through the same
/// production path, draws two measured frames that differ.
#[test]
#[ignore = "requires CS_GAME_DIR and a GPU adapter"]
fn accept_f20_d_retail_geometry_driven_by_a_playing_clip_draws_two_distinct_frames() {
    let dir = evidence_dir();
    std::fs::create_dir_all(&dir).expect("the evidence directory exists");
    let found = survey_world_groups(&game_dir()).expect("the world survey runs");

    // The largest presentable mesh of the first surveyed group: real retail
    // geometry read through the production survey's own selection rule.
    let group = found.groups.first().expect("a surveyed world group");
    let container = group.container().expect("the group's container read");
    let (mesh_index, render) = container.largest_presentable().unwrap_or_else(|| {
        panic!(
            "{}: no stored mesh went through the upload adapter",
            group.world()
        )
    });

    let mut world = playback_world();
    let clip = declared_synthetic_door_clip();
    let entity = bound_node(
        &mut world,
        &clip,
        "synthetic.hangar.door",
        instance(1),
        Tick(0),
    );

    // The pose the playing clip applied, drawn through the production
    // capture — empty unknowns because the capture is a geometry witness,
    // the same declared choice the world capture makes.
    let unknowns: &[cs_content::mesh::MeshPresentationUnknown] = &[];
    let capture = |world: &mut World, tick: u64, name: &str| {
        advance_animation(world, Tick(tick));
        let pose = world
            .get::<NodeAnimatedPose>(entity)
            .expect("the bound node carries the evaluated pose")
            .0;
        capture_animated_pose(&PoseCaptureRequest {
            label: name,
            render,
            unknowns,
            pose,
            png: &dir.join(format!("{name}.png")),
        })
        .unwrap_or_else(|error| panic!("{name} could not be captured: {error}"))
    };

    let closed = capture(&mut world, 0, "f20d-retail-mesh-closed");
    let open = capture(
        &mut world,
        SYNTHETIC_DOOR_OPEN_TICK,
        "f20d-retail-mesh-open",
    );
    assert!(closed.drew_geometry() && open.drew_geometry());
    assert!(
        closed.png_sha256 != open.png_sha256,
        "the clip's evaluated pose change is visible in the rendered frame"
    );
    println!(
        "f20d gpu: adapter={} mesh={}:{mesh_index} closed={}px open={}px",
        closed.adapter,
        group.world(),
        closed.covered_pixels,
        open.covered_pixels
    );
}

// ==================== task #633: the carrier's own members ==================
//
// `M01-LC-ANIM-CARRIERS`: `zbd/<group>/<mission>/mis_anim.zbd` and
// `zbd/<group>/cam_anim.zbd` dispatch as `ZbdFamily::Animation`, and the
// section above validated and fingerprinted all 61 of them without reading a
// payload byte. This section drives the production code that does:
//
//   * `cs_formats::zbd::anim::read_animation_index` — the family's **own** front
//     index (two declared tables: the sibling containers it refers to, and the
//     animation-definition sources whose records it carries). These files are
//     *not* reader archives: they have no version-one trailer, and the tests
//     below prove the two readers refuse each other's bytes.
//   * `AnimationIndex::payload` — the fixed 68-byte block in front of the
//     animation records, with the declared record count and the gravity.
//   * `cs_app::animation::carrier::bind_animation_carrier` — the binding from
//     the scope's paired `mis_anim.zrd` / `cam_anim.zrd` document (decoded with
//     the production `.zrd` reader) to the carrier's member rows, plus the
//     `startanims.zrd` startup identities, which are read and deliberately not
//     bound.

use cs_app::animation::carrier::{
    ANIMATION_DEFINITION_FILE_KEY, ANIMATION_DEFINITIONS_KEY, ANIMATION_LIST_KEY,
    ANIMATION_PATH_KEY, BindingBlocker, CarrierBinding, GRAVITY_KEY, PATH_ROOT_SEPARATOR,
    STARTUP_MEMBER, SiblingReader, UNRESOLVED_REASON_NO_MEMBER, UNRESOLVED_REASON_NO_RECORD_NAMES,
    bind_animation_carrier, survey_animation_bindings,
};
use cs_content::stunts::ZrdValue;
use cs_formats::zbd::{
    ANIMATION_SIGNATURE, AnimationIndexError, AnimationRowAnomaly, RECORDS_NOT_DECODED_REASON,
    indexed_by_animation_header, read_animation_index,
};
use cs_formats::{ParseContext, ZbdFamily, ZbdProbe, dispatch};
use cs_types::install::RelativePath;

/// The world group whose carriers the retail section pins.
const C1C: &str = "zbd/c1c";

/// One row of a synthetic index table: a path and a stamp.
type Row = (&'static str, u32);

/// A synthetic animation container: the documented header, the two index
/// tables in their measured field sizes, and a payload.
fn anim_container_bytes(externals: &[Row], members: &[Row], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&ANIMATION_SIGNATURE.to_le_bytes());
    bytes.extend_from_slice(&53_u32.to_le_bytes());
    bytes.extend_from_slice(&(externals.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    for (path, stamp) in externals {
        let mut field = [0_u8; 128];
        field[..path.len()].copy_from_slice(path.as_bytes());
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&stamp.to_le_bytes());
    }
    for (path, stamp) in members {
        let mut field = [0_u8; 80];
        field[..path.len()].copy_from_slice(path.as_bytes());
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&stamp.to_le_bytes());
    }
    bytes.extend_from_slice(payload);
    bytes
}

/// A synthetic payload in the measured shape: the 68-byte header, 40 zero
/// bytes, and `record_count` records of the measured 272-byte fixed part (the
/// first named `first_record`, the rest `record_<n>`), with no table and no
/// sequence, so the walk tiles it exactly.
fn carrier_payload(record_count: u16, first_record: &str) -> Vec<u8> {
    let mut specs = vec![RecordSpec::named(first_record)];
    specs.extend((1..record_count).map(|n| RecordSpec::named(&format!("record_{n}"))));
    record_payload(&specs, &[])
}

/// One synthetic animation record, in the measured layout.
#[derive(Clone, Default)]
struct RecordSpec {
    name: String,
    unknowns: u32,
    objects: u8,
    nodes: u8,
    lights: u8,
    puffers: u8,
    dynamic_sounds: u8,
    static_sounds: u8,
    effects: u8,
    prerequisites: u8,
    animation_refs: u8,
    index_words: u8,
    reset: Option<Vec<u8>>,
    damage: Option<Vec<u8>>,
    sequences: Vec<(String, Vec<u8>)>,
}

impl RecordSpec {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }

    /// The bytes this record occupies, and nothing else.
    fn bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0_u8; 272];
        bytes[..self.name.len()].copy_from_slice(self.name.as_bytes());
        bytes[36..40].copy_from_slice(&self.unknowns.to_le_bytes());
        // The unknowns pointer, the reset pointer and the damage pointer are
        // nonzero exactly when the block they point at is present.
        if self.unknowns != 0 {
            bytes[32..36].copy_from_slice(&0x0436_0000_u32.to_le_bytes());
        }
        bytes[162] = 4;
        bytes[163] = 2;
        if self.reset.is_some() {
            bytes[208..212].copy_from_slice(&0x0436_1000_u32.to_le_bytes());
        }
        if self.damage.is_some() {
            bytes[212..216].copy_from_slice(&0x0436_2000_u32.to_le_bytes());
        }
        bytes[216] = self.sequences.len() as u8;
        bytes[217] = self.objects;
        bytes[218] = self.nodes;
        bytes[219] = self.lights;
        bytes[220] = self.puffers;
        bytes[221] = self.dynamic_sounds;
        bytes[222] = self.static_sounds;
        bytes[223] = self.effects;
        bytes[224] = self.prerequisites;
        bytes[226] = self.animation_refs;
        bytes[227] = self.index_words;
        let tables: [(usize, usize, u8); 9] = [
            (36, self.unknowns as usize, 0xa1),
            (92, usize::from(self.objects), 0xa2),
            (44, usize::from(self.nodes), 0xa3),
            (44, usize::from(self.lights), 0xa4),
            (44, usize::from(self.puffers), 0xa5),
            (44, usize::from(self.dynamic_sounds), 0xa6),
            (40, usize::from(self.static_sounds), 0xa7),
            (48, usize::from(self.prerequisites), 0xa8),
            (72, usize::from(self.animation_refs), 0xa9),
        ];
        for (entry, count, fill) in tables {
            for index in 0..count {
                let mut field = vec![0_u8; entry];
                // A name at the entry's own name field: node entries carry it
                // four bytes in, the others at the start.
                let label = format!("t{fill:02x}_{index}");
                let at = if fill == 0xa3 { 4 } else { 0 };
                field[at..at + label.len()].copy_from_slice(label.as_bytes());
                bytes.extend_from_slice(&field);
            }
        }
        for index in 0..usize::from(self.index_words) {
            bytes.extend_from_slice(&(index as u32).to_le_bytes());
        }
        if let Some(events) = &self.reset {
            bytes.extend_from_slice(&sequence_block("RESET_SEQUENCE", events));
        }
        if let Some(events) = &self.damage {
            bytes.extend_from_slice(&sequence_block("DAMAGE_SEQUENCE", events));
        }
        for (name, events) in &self.sequences {
            bytes.extend_from_slice(&sequence_block(name, events));
        }
        bytes
    }
}

/// A 64-byte sequence info block (flags `0x303`, a nonzero pointer, `size`
/// events) followed by its events.
fn sequence_block(name: &str, events: &[u8]) -> Vec<u8> {
    let mut block = vec![0_u8; 64];
    block[..name.len()].copy_from_slice(name.as_bytes());
    block[32..36].copy_from_slice(&0x303_u32.to_le_bytes());
    block[56..60].copy_from_slice(&0x0436_3000_u32.to_le_bytes());
    block[60..64].copy_from_slice(&(events.len() as u32).to_le_bytes());
    block.extend_from_slice(events);
    block
}

/// A payload of `specs` records followed by `trailing` bytes, with the
/// declared count equal to the record count.
fn record_payload(specs: &[RecordSpec], trailing: &[u8]) -> Vec<u8> {
    let mut header = vec![0_u8; 68];
    header[10..12].copy_from_slice(&(specs.len() as u16).to_le_bytes());
    header[36..40].copy_from_slice(&(-9.8_f32).to_bits().to_le_bytes());
    header[40..44].copy_from_slice(&1_u32.to_le_bytes());
    header[60..64].copy_from_slice(&1_u32.to_le_bytes());
    let mut bytes = header;
    bytes.extend_from_slice(&[0_u8; 40]);
    for spec in specs {
        bytes.extend_from_slice(&spec.bytes());
    }
    bytes.extend_from_slice(trailing);
    bytes
}

/// Encodes one `.zrd` node in the measured tagged form.
fn zrd_node(value: &ZrdValue) -> Vec<u8> {
    let mut bytes = Vec::new();
    match value {
        ZrdValue::Int(word) => {
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        ZrdValue::Float(number) => {
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            bytes.extend_from_slice(&number.to_bits().to_le_bytes());
        }
        ZrdValue::Text(text) => {
            bytes.extend_from_slice(&3_u32.to_le_bytes());
            bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
        ZrdValue::List(children) => {
            bytes.extend_from_slice(&4_u32.to_le_bytes());
            // A list of `N` stores `N + 1`: the profiler writes one more than
            // its child count (measured by F09, read by the production decoder).
            bytes.extend_from_slice(&((children.len() + 1) as u32).to_le_bytes());
            for child in children {
                bytes.extend_from_slice(&zrd_node(child));
            }
        }
    }
    bytes
}

/// One text node.
fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

/// The measured shape of a paired animation record: one `ANIMATION_DEFINITIONS`
/// record whose body carries `GRAVITY`, `ANIMATION_PATH` and an
/// `ANIMATION_LIST` of `ANIMATION_DEFINITION_FILE` paths.
fn animation_definitions(roots: &[&str], files: &[&str]) -> Vec<u8> {
    let mut body = vec![
        text(GRAVITY_KEY),
        ZrdValue::List(vec![ZrdValue::Float(-9.8)]),
    ];
    if !roots.is_empty() {
        body.push(text(ANIMATION_PATH_KEY));
        body.push(ZrdValue::List(
            roots.iter().map(|root| text(root)).collect::<Vec<_>>(),
        ));
    }
    body.push(text(ANIMATION_LIST_KEY));
    let mut list = Vec::new();
    for file in files {
        list.push(text(ANIMATION_DEFINITION_FILE_KEY));
        list.push(ZrdValue::List(vec![text(file)]));
    }
    body.push(ZrdValue::List(list));
    zrd_node(&ZrdValue::List(vec![ZrdValue::List(vec![
        text(ANIMATION_DEFINITIONS_KEY),
        ZrdValue::List(body),
    ])]))
}

/// The measured shape of `startanims.zrd`: one record of startup keys, each
/// with a list of animation identities.
fn start_anims(groups: &[(&str, &[&str])]) -> Vec<u8> {
    let mut children = Vec::new();
    for (key, identities) in groups {
        children.push(text(key));
        children.push(ZrdValue::List(
            identities
                .iter()
                .map(|identity| ZrdValue::List(vec![text(identity)]))
                .collect::<Vec<_>>(),
        ));
    }
    zrd_node(&ZrdValue::List(vec![ZrdValue::List(children)]))
}

/// A reader archive over `members`, reusing this file's version-one writer.
fn reader_over(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let borrowed: Vec<(&str, &[u8])> = members
        .iter()
        .map(|(name, bytes)| (*name, bytes.as_slice()))
        .collect();
    reader_bytes(&borrowed)
}

/// Binds a synthetic mission scope: its carrier, its reader and its document.
fn bind_mission(
    members: &[Row],
    roots: &[&str],
    files: &[&str],
    startup: &[(&str, &[&str])],
) -> CarrierBinding {
    let carrier = anim_container_bytes(
        &[
            ("zbd\\c1c\\gamez.zbd", 0x39a7_7e80),
            ("zbd\\planes.zbd", 0x39a7_7ba5),
        ],
        members,
        &carrier_payload(573, "reserved_anim_0"),
    );
    let mut reader_members: Vec<(&str, Vec<u8>)> =
        vec![("mis_anim.zrd", animation_definitions(roots, files))];
    if !startup.is_empty() {
        reader_members.push((STARTUP_MEMBER, start_anims(startup)));
    }
    let reader = reader_over(&reader_members);
    bind_animation_carrier(
        &RelativePath::new("zbd/c1c/m01/mis_anim.zbd").expect("a relative spelling"),
        "zbd/c1c/m01/zrdr.zbd",
        CarrierKind::Mission,
        &carrier,
        SiblingReader::Bytes(&reader),
    )
}

/// The animation index of a synthetic carrier, read through production code.
fn read_synthetic_index<'a>(
    key: &'a str,
    path: &'a RelativePath,
    bytes: &'a [u8],
) -> Result<cs_formats::zbd::AnimationIndex<'a>, AnimationIndexError> {
    let mut context = ParseContext::with_defaults(key);
    let decision = dispatch(ZbdProbe::new(key, path, &bytes[..8])).map_err(|error| {
        AnimationIndexError::NotAnimationFamily {
            container: error.container().to_owned(),
            family: ZbdFamily::Reader,
        }
    })?;
    read_animation_index(&mut context, decision, bytes)
}

/// The same bytes through the **reader** entry point, which must refuse them:
/// the animation family has no version-one trailer, so
/// `read_version_one_index` has to say so.
fn reader_entry_point_refuses(bytes: &[u8]) -> bool {
    let key = "zbd/c1c/m01/mis_anim.zbd";
    let mut context = ParseContext::with_defaults(key);
    let path = synthetic_path();
    let Ok(decision) = dispatch(ZbdProbe::new(key, path, &bytes[..8])) else {
        return false;
    };
    cs_formats::zbd::read_version_one_index(&mut context, decision, bytes).is_err()
}

/// The same check at the reader's own declared path, where two-key dispatch
/// really does route the bytes to the reader family.
fn reader_entry_point_refuses_reader(bytes: &[u8]) -> bool {
    let key = "zbd/c1c/m01/zrdr.zbd";
    let mut context = ParseContext::with_defaults(key);
    let path = RelativePath::new(key).expect("a relative spelling");
    let Ok(decision) = dispatch(ZbdProbe::new(key, &path, bytes)) else {
        return false;
    };
    cs_formats::zbd::read_version_one_index(&mut context, decision, bytes).is_err()
}

/// The path the synthetic carriers are read under.
fn synthetic_path() -> &'static RelativePath {
    static PATH: std::sync::OnceLock<RelativePath> = std::sync::OnceLock::new();
    PATH.get_or_init(|| RelativePath::new("zbd/c1c/m01/mis_anim.zbd").expect("a relative spelling"))
}

/// The two families are told apart by their own readers, in both directions:
/// the animation container is not a version-one trailer archive, and the
/// reader archive is not an animation header. This is the acceptance
/// criterion's "these are NOT reader archives", tested rather than asserted in
/// a comment.
#[test]
fn accept_m01_lc_anim_carriers_an_animation_container_is_not_a_reader_archive() {
    assert!(
        indexed_by_animation_header(ZbdFamily::Animation),
        "the animation family is indexed by its own front header"
    );
    let carrier = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[("..\\data\\c1c\\m01\\zrdr\\mis_anim.zrd", 3)],
        &carrier_payload(2, "reserved_anim_0"),
    );
    let index = read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", synthetic_path(), &carrier)
        .expect("the animation index reads");
    assert_eq!(index.family(), ZbdFamily::Animation);
    assert_eq!(index.member_count(), 1);
    assert!(
        reader_entry_point_refuses(&carrier),
        "the version-one trailer reader refuses an animation container"
    );

    // And the other way round: a reader archive's own bytes, at the path the
    // reader role rule declares, through the animation reader.
    let reader = reader_bytes(&[("mis_anim.zrd", b"document")]);
    assert!(
        !reader_entry_point_refuses_reader(&reader),
        "a reader archive is a trailer-indexed archive, so that reader accepts it"
    );
    let key = "zbd/c1c/m01/zrdr.zbd";
    let mut context = ParseContext::with_defaults(key);
    let path = RelativePath::new(key).expect("a relative spelling");
    let decision = dispatch(ZbdProbe::new(key, &path, &reader[..8])).expect("a decision");
    match read_animation_index(&mut context, decision, &reader) {
        Err(AnimationIndexError::NotAnimationFamily { family, .. }) => {
            assert_eq!(
                family,
                ZbdFamily::Reader,
                "the other family's bytes are named"
            );
        }
        other => panic!("expected NotAnimationFamily for a reader archive, got {other:?}"),
    }
}

/// The index lists every declared row of both tables with its own span, stamp
/// and path, and the payload header behind them states the declared record
/// count and the gravity — the counts a consumer needs before any record is
/// decoded.
#[test]
fn accept_m01_lc_anim_carriers_the_index_lists_every_row_and_the_payload_header() {
    let carrier = anim_container_bytes(
        &[
            ("zbd\\c1c\\gamez.zbd", 0x39a7_7e80),
            ("zbd\\planes.zbd", 0x39a7_7ba5),
        ],
        &[
            ("..\\data\\c1c\\m01\\zrdr\\mis_anim.zrd", 10),
            ("..\\data\\c1c\\m01\\zrdr\\zeps\\climbladder.zan", 11),
            ("..\\data\\c1c\\m01\\zrdr\\zeps\\climbladder.zan", 12),
        ],
        &carrier_payload(573, "reserved_anim_0"),
    );
    let index = read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", synthetic_path(), &carrier)
        .expect("the index reads");
    assert_eq!(index.version(), 53, "the documented retail version word");
    assert_eq!(index.external_count(), 2);
    assert_eq!(
        index
            .externals()
            .iter()
            .map(|row| String::from_utf8_lossy(row.path()).into_owned())
            .collect::<Vec<_>>(),
        vec!["zbd\\c1c\\gamez.zbd", "zbd\\planes.zbd"],
        "both external rows, in declared order"
    );
    assert_eq!(index.member_count(), 3, "a repeated path stays three rows");
    assert_eq!(index.member(0).expect("row 0").stamp(), 10);
    assert_eq!(index.member(1).expect("row 1").stamp(), 11);
    assert_eq!(index.member(2).expect("row 2").stamp(), 12);
    assert_eq!(
        index.member(0).expect("row 0").record_span().offset,
        16 + 2 * 132,
        "the first member row sits behind the two external rows"
    );
    assert!(
        index.anomalous_rows().next().is_none(),
        "a zero-padded row is conforming"
    );

    let payload = index.payload().expect("the payload header reads");
    assert_eq!(payload.header().declared_record_count, 573);
    assert_eq!(payload.header().gravity, -9.8);
    assert!(payload.header().is_measured_shape());
    assert_eq!(
        payload.header().gravity_evidence(),
        cs_types::evidence::ClaimStatus::Documented
    );
    assert_eq!(
        payload.first_record_name(),
        b"reserved_anim_0",
        "the payload's first record name, verbatim"
    );
    assert_eq!(
        payload.record_table_offset(),
        index.payload_offset() + 68 + 40
    );
    assert_eq!(
        payload.records_not_decoded_reason(),
        RECORDS_NOT_DECODED_REASON,
        "the records this stage does not decode say so in the value itself"
    );
}

/// A byte after a row's terminating NUL is reported, not decoded: it is the
/// measured state of 1115 of the 2595 retail member rows, and a reader that
/// treated it as a second name would invent content.
#[test]
fn accept_m01_lc_anim_carriers_a_nonzero_byte_after_a_path_is_reported_not_decoded() {
    let mut carrier = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[("..\\data\\c1c\\m01\\zrdr\\mis_anim.zrd", 7)],
        &carrier_payload(1, "reserved_anim_0"),
    );
    // The first member row's 80-byte path field starts at 16 + 2 * 132.
    let field = 16 + 2 * 132;
    let text_len = "..\\data\\c1c\\m01\\zrdr\\mis_anim.zrd".len();
    carrier[field + text_len + 1] = b'X';
    let index = read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", synthetic_path(), &carrier)
        .expect("the index reads");
    let row = index.member(0).expect("row 0");
    assert!(
        row.has_anomaly(AnimationRowAnomaly::NonZeroPathPadding),
        "the byte after the NUL is an anomaly, not a second name"
    );
    assert!(
        !row.has_anomaly(AnimationRowAnomaly::UnterminatedPath),
        "the path itself is still terminated"
    );
    assert_eq!(
        row.path(),
        b"..\\data\\c1c\\m01\\zrdr\\mis_anim.zrd",
        "the path stops at its NUL"
    );
    assert_eq!(
        row.padding().first().copied(),
        Some(b'X'),
        "the padding is kept verbatim"
    );
    assert_eq!(index.anomalous_rows().count(), 1);

    // The binding keeps the reader's refusal: an anomaly a consumer of
    // `bind_animation_carrier` can still see, rather than one the lossy path
    // decode silently drops.
    let carrier = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[("..\\data\\c1c\\m01\\zrdr\\mis_anim.zrd", 7)],
        &carrier_payload(1, "reserved_anim_0"),
    );
    let binding = bind_animation_carrier(
        &RelativePath::new("zbd/c1c/m01/mis_anim.zbd").expect("a relative spelling"),
        "zbd/c1c/m01/zrdr.zbd",
        CarrierKind::Mission,
        &carrier,
        SiblingReader::Absent,
    );
    assert!(
        binding.members[0].anomalies.is_empty(),
        "a zero-padded row carries no anomaly through the binding"
    );
    let mut carrier = carrier;
    let field = 16 + 2 * 132;
    let text_len = "..\\data\\c1c\\m01\\zrdr\\mis_anim.zrd".len();
    carrier[field + text_len + 1] = b'X';
    let binding = bind_animation_carrier(
        &RelativePath::new("zbd/c1c/m01/mis_anim.zbd").expect("a relative spelling"),
        "zbd/c1c/m01/zrdr.zbd",
        CarrierKind::Mission,
        &carrier,
        SiblingReader::Absent,
    );
    assert_eq!(
        binding.members[0].anomalies,
        vec![AnimationRowAnomaly::NonZeroPathPadding],
        "the binding reports what the reader recorded, unchanged"
    );
    assert!(binding.members[0].has_anomaly(AnimationRowAnomaly::NonZeroPathPadding));
    assert_eq!(
        binding.members[0].path, "..\\data\\c1c\\m01\\zrdr\\mis_anim.zrd",
        "and the path it binds on is still the text before the NUL"
    );
}

/// The join itself: a full path binds to its row, a bare name resolves against
/// the record's root, and the bound row reports which reference named it.
#[test]
fn accept_m01_lc_anim_carriers_a_definition_file_binds_to_its_member_row() {
    let binding = bind_mission(
        &[
            ("..\\data\\c1c\\m01\\zrdr\\mis_anim.zrd", 1),
            ("..\\data\\c1c\\m01\\zrdr\\zeps\\placezeps.zrd", 2),
            ("..\\data\\common\\zrdr\\zeps\\wv_turrets.zrd", 3),
        ],
        &[r"..\data\c1c\m01\zrdr\zeps"],
        &["placezeps.zrd", r"..\data\common\zrdr\zeps\wv_turrets.zrd"],
        &[],
    );
    assert!(binding.blockers.is_empty(), "{:?}", binding.blockers);
    assert_eq!(binding.kind, CarrierKind::Mission);
    assert_eq!(binding.version, 53);
    assert_eq!(binding.members.len(), 3);
    assert_eq!(binding.references.len(), 2);
    assert_eq!(binding.bound_reference_count(), 2);
    assert!(binding.unresolved.is_empty());
    assert_eq!(binding.referenced_member_count(), 2);
    assert_eq!(
        binding.unreferenced_member_count(),
        1,
        "the row the document never names is counted, not dropped"
    );
    assert_eq!(binding.references[0].raw, "placezeps.zrd");
    assert_eq!(
        binding.references[0].member,
        Some(1),
        "the bare name resolved to row 1"
    );
    assert_eq!(
        binding.references[1].member,
        Some(2),
        "the full path matched row 2 verbatim"
    );
    assert_eq!(binding.members[1].references, vec![0]);
    assert_eq!(binding.members[2].references, vec![1]);
    assert!(binding.members[0].references.is_empty());
    assert_eq!(
        binding
            .members
            .iter()
            .find(|row| row.index == 1)
            .expect("row 1")
            .span
            .length,
        84,
        "each member row is the 84-byte record the index declares"
    );
    let document = binding
        .document
        .as_ref()
        .expect("the paired record decoded");
    assert_eq!(document.member, "mis_anim.zrd");
    assert_eq!(document.roots, vec![r"..\data\c1c\m01\zrdr\zeps"]);
    assert_eq!(document.gravity, Some(-9.8));
}

/// A `;`-joined `ANIMATION_PATH` is two roots, and the first one that names a
/// member wins — the measured shape of the eight world-group carriers.
#[test]
fn accept_m01_lc_anim_carriers_a_joined_animation_path_offers_every_root_in_order() {
    let binding = bind_mission(
        &[
            (r"..\data\c1c\zrdr\envmodels\speed_cue.zrd", 1),
            (r"..\data\common\zrdr\zeps\wv_turrets.zrd", 2),
        ],
        &[],
        &[r"..\data\common\zrdr\zeps\wv_turrets.zrd"],
        &[],
    );
    // No root is needed for a full path, and the record's absence of
    // `ANIMATION_PATH` is not a failure.
    assert!(binding.document.is_some());
    assert!(binding.document.as_ref().expect("decoded").roots.is_empty());
    assert_eq!(binding.bound_reference_count(), 1);

    let joined =
        format!(r"..\data\c1c\zrdr\envmodels{PATH_ROOT_SEPARATOR}..\data\common\zrdr\zeps");
    let binding = bind_mission(
        &[(r"..\data\common\zrdr\zeps\wv_turrets.zrd", 1)],
        &[joined.as_str()],
        &["wv_turrets.zrd"],
        &[],
    );
    let document = binding
        .document
        .as_ref()
        .expect("the paired record decoded");
    assert_eq!(
        document.roots,
        vec![
            r"..\data\c1c\zrdr\envmodels".to_owned(),
            r"..\data\common\zrdr\zeps".to_owned(),
        ],
        "one stored string, two roots, in order"
    );
    assert_eq!(
        binding.bound_reference_count(),
        1,
        "the second root answered"
    );
    assert_eq!(binding.references[0].member, Some(0));
}

/// The M01 case measured on the original installation: the document names
/// `..\data\common\zrdr\zeps\wv_tailhook.zrd` while the carrier stores the same
/// basename under the mission root. A basename match is **not** a binding, so
/// the reference is reported with the spelling it was compared against.
#[test]
fn accept_m01_lc_anim_carriers_a_reference_no_member_answers_is_reported_not_matched() {
    let binding = bind_mission(
        &[
            (r"..\data\c1c\m01\zrdr\zeps\placezeps.zrd", 1),
            (r"..\data\c1c\m01\zrdr\zeps\wv_tailhook.zrd", 2),
        ],
        &[r"..\data\c1c\m01\zrdr\zeps"],
        &[r"..\data\common\zrdr\zeps\wv_tailhook.zrd"],
        &[],
    );
    assert!(binding.blockers.is_empty(), "{:?}", binding.blockers);
    assert_eq!(
        binding.unresolved.len(),
        1,
        "the reference is reported, not bound"
    );
    let unresolved = &binding.unresolved[0];
    assert_eq!(unresolved.raw, r"..\data\common\zrdr\zeps\wv_tailhook.zrd");
    assert_eq!(
        unresolved.candidates,
        vec![r"..\data\common\zrdr\zeps\wv_tailhook.zrd".to_owned()],
        "a full path is compared as it stands — one candidate, unrewritten"
    );
    assert_eq!(unresolved.reason, UNRESOLVED_REASON_NO_MEMBER);
    assert_eq!(
        binding.referenced_member_count(),
        0,
        "the same-basename member under another root is not a reference"
    );
    assert_eq!(binding.references[0].member, None);
    assert!(
        !binding.is_bound(),
        "an unresolved reference is not a bound carrier"
    );
}

/// The startup identities are read and deliberately not bound: an identity
/// names a record inside the payload, and the payload's records are the open
/// item. The reason travels with the value.
#[test]
fn accept_m01_lc_anim_carriers_startup_identities_are_listed_and_carry_their_open_reason() {
    let binding = bind_mission(
        &[(r"..\data\c1c\m01\zrdr\zeps\placezeps.zrd", 1)],
        &[r"..\data\c1c\m01\zrdr\zeps"],
        &["placezeps.zrd"],
        &[
            ("NEW_GAME_START", &["generic_intro", "wv_hookup_state"]),
            ("LOAD_GAME_START", &["player_setup"]),
        ],
    );
    assert!(binding.blockers.is_empty(), "{:?}", binding.blockers);
    let startup = binding.startup.as_ref().expect("the startup record read");
    assert_eq!(startup.member, STARTUP_MEMBER);
    assert_eq!(
        startup
            .groups
            .iter()
            .map(|group| group.key.as_str())
            .collect::<Vec<_>>(),
        vec!["NEW_GAME_START", "LOAD_GAME_START"],
        "the measured two-key startup table, in order"
    );
    assert_eq!(
        startup.groups[0].identities,
        vec!["generic_intro".to_owned(), "wv_hookup_state".to_owned()]
    );
    assert_eq!(
        startup.groups[1].identities,
        vec!["player_setup".to_owned()]
    );
    assert_eq!(binding.startup_identity_count(), 3);
    assert_eq!(
        startup.reason, UNRESOLVED_REASON_NO_RECORD_NAMES,
        "why an identity is not bound is carried with it"
    );
    assert_eq!(
        binding.bound_reference_count(),
        1,
        "a definition file still binds; an identity is a different input"
    );

    // A scope with no startup record is content, not a blocker.
    let without = bind_mission(
        &[(r"..\data\c1c\m01\zrdr\zeps\placezeps.zrd", 1)],
        &[r"..\data\c1c\m01\zrdr\zeps"],
        &["placezeps.zrd"],
        &[],
    );
    assert!(without.startup.is_none());
    assert!(
        without.blockers.is_empty(),
        "an absent startanims.zrd is not a failure: {:?}",
        without.blockers
    );
}

/// Fail-closed: counts that do not fit, a payload with no room for its header,
/// and a reader without the paired record are each a named refusal, and the
/// row still exists.
#[test]
fn accept_m01_lc_anim_carriers_every_unreadable_input_is_a_named_refusal() {
    // A member count no file can hold.
    let mut carrier = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[("a.zrd", 1)],
        &carrier_payload(1, "reserved_anim_0"),
    );
    carrier[12..16].copy_from_slice(&0xffff_u32.to_le_bytes());
    match read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", synthetic_path(), &carrier) {
        Err(AnimationIndexError::IndexOutOfBounds {
            count, row_bytes, ..
        }) => {
            assert_eq!(count, 0xffff);
            assert_eq!(row_bytes, 84);
        }
        other => panic!("expected IndexOutOfBounds, got {other:?}"),
    }

    // A payload shorter than the fixed header.
    let short = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[("a.zrd", 1)],
        &[0_u8; 16],
    );
    let index = read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", synthetic_path(), &short)
        .expect("the index still reads");
    match index.payload() {
        Err(AnimationIndexError::PayloadTooSmall {
            needed, available, ..
        }) => {
            assert_eq!(needed, 68);
            assert_eq!(available, 16);
        }
        other => panic!("expected PayloadTooSmall, got {other:?}"),
    }

    // A payload that holds the header but stops inside the first record's name
    // field: named, never reported as an empty record name.
    let truncated = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[("a.zrd", 1)],
        &[0_u8; 100],
    );
    let index = read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", synthetic_path(), &truncated)
        .expect("the index still reads");
    match index.payload() {
        Err(AnimationIndexError::FirstRecordNameTruncated {
            needed, available, ..
        }) => {
            assert_eq!(needed, 32, "the name field is what it lacks");
            assert_eq!(
                available, 32,
                "100 bytes: 68 of header, 32 short of the field"
            );
        }
        other => panic!("expected FirstRecordNameTruncated, got {other:?}"),
    }
    // The header, the measured gap and the name field exactly: it reads.
    let exact = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[("a.zrd", 1)],
        &carrier_payload(1, "reserved_anim_0")[..140],
    );
    let index = read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", synthetic_path(), &exact)
        .expect("the index still reads");
    assert_eq!(
        index
            .payload()
            .expect("exactly the header, the gap and the name field")
            .first_record_name(),
        b"reserved_anim_0"
    );

    // A sibling reader with no paired record: the row is reported, not dropped.
    let carrier = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[("a.zrd", 1)],
        &carrier_payload(1, "reserved_anim_0"),
    );
    let reader = reader_over(&[("objectives.zrd", b"other".to_vec())]);
    let binding = bind_animation_carrier(
        &RelativePath::new("zbd/c1c/m01/mis_anim.zbd").expect("a relative spelling"),
        "zbd/c1c/m01/zrdr.zbd",
        CarrierKind::Mission,
        &carrier,
        SiblingReader::Bytes(&reader),
    );
    assert!(matches!(
        binding.blockers.as_slice(),
        [BindingBlocker::DocumentAbsent {
            member: "mis_anim.zrd",
            ..
        }]
    ));
    assert_eq!(
        binding.members.len(),
        1,
        "the carrier's own row still exists"
    );
    assert!(binding.document.is_none());
    assert!(binding.references.is_empty());

    // A scope with no sibling reader at all names the reader it expected.
    let binding = bind_animation_carrier(
        &RelativePath::new("zbd/c1c/m01/mis_anim.zbd").expect("a relative spelling"),
        "zbd/c1c/m01/zrdr.zbd",
        CarrierKind::Mission,
        &carrier,
        SiblingReader::Absent,
    );
    assert!(matches!(
        binding.blockers.as_slice(),
        [BindingBlocker::MissingReader { expected_key }] if expected_key == "zbd/c1c/m01/zrdr.zbd"
    ));
}

// --------------------------------------------------------------- retail ---

/// The retail inventory of the c1c group: the measured member counts, the
/// measured payload headers and the measured join between each carrier's own
/// index and the `ANIMATION_DEFINITION_FILE` references of its paired record.
///
/// This is task #633's `retail` half, re-measured from `$CS_GAME_DIR` on every
/// run — every number below is a count the production path computed, not a
/// value copied out of a document.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_m01_lc_anim_carriers_retail_c1c_lists_binds_and_reports_every_member() {
    let survey = survey_animation_bindings(&game_dir()).expect("the survey runs over the install");

    // The census: eight world-group camera carriers and 53 mission-scoped
    // carriers over the whole installation, all of them indexed.
    assert_eq!(survey.carriers.len(), 61);
    assert_eq!(survey.carriers_of(CarrierKind::Camera).count(), 8);
    assert_eq!(survey.carriers_of(CarrierKind::Mission).count(), 53);
    for carrier in &survey.carriers {
        assert_eq!(
            carrier.version, 53,
            "{}: the documented retail version word",
            carrier.container_key
        );
        assert_eq!(
            carrier.blockers.len(),
            0,
            "{}: {:?}",
            carrier.container_key,
            carrier.blockers
        );
        assert!(carrier.payload.is_some(), "{}", carrier.container_key);
        assert!(carrier.document.is_some(), "{}", carrier.container_key);
        assert_eq!(
            carrier.externals.len(),
            2,
            "{}: two sibling containers",
            carrier.container_key
        );
    }

    // c1c's five carriers: the measured member counts and joins.
    let camera = survey
        .carrier("zbd/c1c/cam_anim.zbd")
        .expect("the c1c camera carrier has a row");
    let m01 = survey
        .carrier("zbd/c1c/m01/mis_anim.zbd")
        .expect("the c1c M01 carrier has a row");
    for (carrier, members, references, bound, unreferenced) in [
        (camera, 134_usize, 2_usize, 2_usize, 132_usize),
        (m01, 91, 22, 21, 70),
        (
            survey
                .carrier("zbd/c1c/ia1/mis_anim.zbd")
                .expect("the c1c IA1 carrier has a row"),
            9,
            8,
            8,
            1,
        ),
        (
            survey
                .carrier("zbd/c1c/mp1/mis_anim.zbd")
                .expect("the c1c MP1 carrier has a row"),
            2,
            1,
            1,
            1,
        ),
        (
            survey
                .carrier("zbd/c1c/mp3/mis_anim.zbd")
                .expect("the c1c MP3 carrier has a row"),
            14,
            13,
            13,
            1,
        ),
    ] {
        assert_eq!(carrier.members.len(), members, "{}", carrier.container_key);
        assert_eq!(
            carrier.references.len(),
            references,
            "{}: definition files",
            carrier.container_key
        );
        assert_eq!(
            carrier.bound_reference_count(),
            bound,
            "{}: references that reached a member row",
            carrier.container_key
        );
        assert_eq!(
            carrier.referenced_member_count(),
            bound,
            "{}: member rows a reference named",
            carrier.container_key
        );
        assert_eq!(
            carrier.unreferenced_member_count(),
            unreferenced,
            "{}: member rows no reference names are counted, not dropped",
            carrier.container_key
        );
        let payload = carrier.payload.as_ref().expect("the payload header read");
        assert_eq!(payload.gravity, -9.8, "{}", carrier.container_key);
        assert_eq!(
            payload.first_record_name, b"reserved_anim_0",
            "{}: the payload's first record name",
            carrier.container_key
        );
        assert!(
            payload.declared_record_count as usize >= carrier.members.len(),
            "{}: the declared record count is never below the member count",
            carrier.container_key
        );
    }

    // c1c's camera carrier: two roots stored in one `ANIMATION_PATH` string,
    // both references bound, 132 member rows no reference names.
    assert_eq!(
        camera.document.as_ref().expect("the paired record").roots,
        vec![
            r"..\data\c1c\zrdr\envmodels".to_owned(),
            r"..\data\common\zrdr\zeps".to_owned(),
        ]
    );
    assert_eq!(camera.unreferenced_member_count(), 132);
    assert_eq!(
        camera
            .payload
            .as_ref()
            .expect("payload")
            .declared_record_count,
        307
    );
    assert_eq!(camera.size_bytes, 1_217_815, "zbd/c1c/cam_anim.zbd");

    // M01: 91 member rows, 22 references, 21 bound, and the one that is not,
    // named with both spellings.
    assert_eq!(m01.size_bytes, 2_019_493, "zbd/c1c/m01/mis_anim.zbd");
    assert_eq!(m01.unreferenced_member_count(), 70);
    assert_eq!(
        m01.payload.as_ref().expect("payload").declared_record_count,
        573
    );
    assert_eq!(m01.payload.as_ref().expect("payload").span.offset, 7924);
    assert_eq!(
        m01.payload.as_ref().expect("payload").span.length,
        2_019_493 - 7924
    );
    let unresolved: Vec<&str> = m01
        .unresolved
        .iter()
        .map(|entry| entry.raw.as_str())
        .collect();
    assert_eq!(
        unresolved,
        vec![r"..\data\common\zrdr\zeps\wv_tailhook.zrd"],
        "M01's one unresolved reference, verbatim"
    );
    assert_eq!(m01.unresolved[0].reason, UNRESOLVED_REASON_NO_MEMBER);
    // The container stores the same basename under the mission root; a
    // basename is not a binding, so that row stays unreferenced.
    assert!(
        m01.members
            .iter()
            .any(|row| row.path == r"..\data\c1c\m01\zrdr\zeps\wv_tailhook.zrd"),
        "the mission-scoped spelling is a member row"
    );
    // A bare definition-file name resolved against `ANIMATION_PATH`.
    assert!(
        m01.members
            .iter()
            .any(|row| row.path == r"..\data\c1c\m01\zrdr\zeps\placezeps.zrd"
                && row.references.len() == 1),
        "the bare name bound to the mission-rooted row"
    );

    // M01's startup table: two keys and seven identities, read and not bound.
    let startup = m01.startup.as_ref().expect("M01's startup table read");
    assert_eq!(startup.groups.len(), 2);
    assert_eq!(
        startup
            .groups
            .iter()
            .map(|group| group.key.as_str())
            .collect::<Vec<_>>(),
        vec!["NEW_GAME_START", "LOAD_GAME_START"]
    );
    assert_eq!(m01.startup_identity_count(), 7);
    assert_eq!(startup.reason, UNRESOLVED_REASON_NO_RECORD_NAMES);

    // The corpus totals: 2595 member rows over 61 carriers, 739 references,
    // 731 of which reach a row, and the 8 that do not — each named.
    let members: usize = survey.carriers.iter().map(|row| row.members.len()).sum();
    let references: usize = survey.carriers.iter().map(|row| row.references.len()).sum();
    let bound: usize = survey
        .carriers
        .iter()
        .map(CarrierBinding::bound_reference_count)
        .sum();
    assert_eq!(members, 2595, "every declared member row of every carrier");
    assert_eq!(
        references, 739,
        "every definition file every paired record names"
    );
    assert_eq!(bound, 731, "references that reached a member row");
    let unreferenced: usize = survey
        .carriers
        .iter()
        .map(|row| row.unreferenced_member_count())
        .sum();
    assert_eq!(unreferenced, 1865, "member rows no definition file names");
    // 731 bound references reach 730 distinct rows: exactly one carrier binds
    // two of its references to the same row, because a bare name resolved
    // against a later root and a full path name the same member. Measured, and
    // named here so the two totals are not read as a contradiction.
    let referenced_rows: usize = survey
        .carriers
        .iter()
        .map(|row| row.referenced_member_count())
        .sum();
    assert_eq!(
        referenced_rows, 730,
        "distinct member rows a reference names"
    );
    let shared: Vec<&str> = survey
        .carriers
        .iter()
        .filter(|row| {
            row.bound_reference_count() > 0
                && row.referenced_member_count() < row.bound_reference_count()
        })
        .map(|row| row.container_key.as_str())
        .collect();
    assert_eq!(shared, vec!["zbd/c1b/m03/mis_anim.zbd"]);
    let m03 = survey
        .carrier("zbd/c1b/m03/mis_anim.zbd")
        .expect("the c1b M03 carrier has a row");
    let row_18 = m03
        .members
        .iter()
        .find(|row| row.index == 18)
        .expect("member row 18");
    assert_eq!(row_18.references, vec![12, 22]);
    assert_eq!(
        m03.references[12].raw, "pzep_getcargo.zrd",
        "a bare name whose first root is not a member row"
    );
    assert_eq!(
        m03.references[22].raw, r"..\data\c1b\m03\zrdr\zeps\pzep_getcargo.zrd",
        "and the full path that reaches the same row"
    );

    // The index rows' own refusals, measured over the same corpus: 1115 member
    // rows carry a non-zero byte after their NUL (41 containers), and **no**
    // retail row is unterminated or non-ASCII — the two refusal arms the
    // synthetic tests exercise instead.
    let padded: usize = survey
        .carriers
        .iter()
        .flat_map(|row| &row.members)
        .filter(|row| row.has_anomaly(AnimationRowAnomaly::NonZeroPathPadding))
        .count();
    assert_eq!(
        padded, 1115,
        "member rows with a non-zero byte after the NUL"
    );
    let padded_carriers = survey
        .carriers
        .iter()
        .filter(|row| {
            row.members
                .iter()
                .any(|member| member.has_anomaly(AnimationRowAnomaly::NonZeroPathPadding))
        })
        .count();
    assert_eq!(padded_carriers, 41, "containers that hold one");
    for anomaly in [
        AnimationRowAnomaly::UnterminatedPath,
        AnimationRowAnomaly::NonAsciiPath,
    ] {
        let hits = survey
            .carriers
            .iter()
            .flat_map(|row| &row.members)
            .filter(|row| row.has_anomaly(anomaly))
            .count();
        assert_eq!(hits, 0, "{anomaly:?} does not occur in the original data");
    }
    // Every member row of every carrier is the scope's own paired document at
    // index 0, and no paired document names it — which is why row 0 is
    // unreferenced in all 61.
    for carrier in &survey.carriers {
        let first = carrier.members.first().expect("every carrier has member 0");
        assert_eq!(first.index, 0);
        assert!(
            first.references.is_empty(),
            "{}: the document never names its own row",
            carrier.container_key
        );
        let expected = match carrier.kind {
            CarrierKind::Mission => format!(
                r"..\data\{scope}\zrdr\mis_anim.zrd",
                scope = carrier
                    .container_key
                    .strip_prefix("zbd/")
                    .and_then(|rest| rest.strip_suffix("/mis_anim.zbd"))
                    .expect("a mission carrier key")
                    .replace('/', "\\")
            ),
            CarrierKind::Camera => format!(
                r"..\\data\\{group}\\zrdr\cam_anim.zrd",
                group = carrier
                    .container_key
                    .strip_prefix("zbd/")
                    .and_then(|rest| rest.strip_suffix("/cam_anim.zbd"))
                    .expect("a camera carrier key")
            ),
        };
        assert_eq!(first.path, expected, "{}: member 0", carrier.container_key);
    }
    let c1c: Vec<&CarrierBinding> = survey
        .carriers
        .iter()
        .filter(|row| row.container_key.starts_with(C1C))
        .collect();
    assert_eq!(c1c.len(), 5, "the c1c group and its four scopes");
    assert_eq!(c1c.iter().map(|row| row.members.len()).sum::<usize>(), 250);
    assert_eq!(
        c1c.iter()
            .map(|row| row.unreferenced_member_count())
            .sum::<usize>(),
        205
    );
    let unresolved: Vec<(&str, &str)> = survey
        .unresolved_references()
        .map(|(key, entry)| (key, entry.raw.as_str()))
        .collect();
    assert_eq!(unresolved.len(), 8, "{unresolved:?}");
    for (key, raw) in &unresolved {
        assert!(
            raw.contains(r"zeps\wv_tailhook.zrd")
                || raw.contains("hotelstart.zrd")
                || raw.contains("pzep_hangerlights.zrd")
                || raw.contains(r"zeps\no_rock.zrd")
                || raw.contains("manned_aa_gun.zrd")
                || raw.contains("generic_signs.zrd")
                || raw.contains("tarzan_huts.zrd"),
            "{key}: {raw} is one of the measured unresolved references"
        );
    }
    // Every campaign mission scope the layout declares is in the census.
    assert!(
        survey.carrier("zbd/c1c/m01/mis_anim.zbd").is_some(),
        "the c1c mission of the campaign has a row"
    );
    assert!(
        !survey.is_complete(),
        "the eight unresolved references keep the survey from claiming completeness"
    );
    assert!(
        C1C.starts_with("zbd/"),
        "the group key is an installation-relative spelling"
    );
}

// ------------------- task #650: the animation record walk ---------------------
//
// `AnimationPayload::records` derives every record's length from fields inside
// the record (a 272-byte fixed part, count x entry-size tables, and 64-byte
// sequence blocks that carry their own event length), so record *n* starts at
// the sum of the lengths before it. The synthetic tests build records from
// exactly that rule and check the production walk against it, plus every
// refusal; the retail test pins the measured counts of `zbd/c1c`.

use cs_app::animation::carrier::{
    StartupOutcome, UNBOUND_REASON_AMBIGUOUS, UNBOUND_REASON_NO_RECORD, bind_startup_identities,
};
use cs_formats::zbd::{
    AnimationRecordError, AnimationRecordSequenceKind, AnimationRecordTableKind,
    POINTERS_UNRESOLVED_REASON,
};

/// The payload of a synthetic container, walked through production code.
fn walk_synthetic(payload: Vec<u8>) -> Result<Vec<RecordSnapshot>, AnimationRecordError> {
    let carrier = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[(r"..\data\c1c\m01\zrdr\mis_anim.zrd", 10)],
        &payload,
    );
    let path = synthetic_path();
    let index = read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", path, &carrier)
        .expect("the synthetic index reads");
    let payload = index.payload().expect("the payload header reads");
    let walk = payload.records()?;
    Ok(walk
        .iter()
        .map(|record| RecordSnapshot {
            offset: record.payload_offset(),
            len: record.len(),
            name: String::from_utf8_lossy(record.anim_name()).into_owned(),
            sequences: record
                .sequences()
                .iter()
                .map(|sequence| {
                    (
                        sequence.kind(),
                        String::from_utf8_lossy(sequence.name()).into_owned(),
                        sequence.events().to_vec(),
                    )
                })
                .collect(),
            tables: record
                .tables()
                .iter()
                .map(|table| {
                    (
                        table.kind(),
                        table.count(),
                        table
                            .name(0)
                            .map(|name| String::from_utf8_lossy(name).into_owned()),
                    )
                })
                .collect(),
            trailing: walk.trailing().to_vec(),
        })
        .collect())
}

/// What a test needs of one walked record, owned so the walk can be dropped.
struct RecordSnapshot {
    offset: u64,
    len: usize,
    name: String,
    sequences: Vec<(AnimationRecordSequenceKind, String, Vec<u8>)>,
    tables: Vec<(AnimationRecordTableKind, usize, Option<String>)>,
    trailing: Vec<u8>,
}

/// The records every test below mixes: a bare one, one with every table, one
/// with a reset block, a damage block and two ordinary sequences, and one with
/// unknowns and an index list.
fn mixed_specs() -> Vec<RecordSpec> {
    let mut tables = RecordSpec::named("tables");
    tables.objects = 3;
    tables.nodes = 2;
    tables.lights = 1;
    tables.puffers = 2;
    tables.dynamic_sounds = 1;
    tables.static_sounds = 2;
    tables.prerequisites = 2;
    tables.animation_refs = 1;
    tables.sequences = vec![("only".to_owned(), vec![7; 20])];
    let mut blocks = RecordSpec::named("blocks");
    blocks.reset = Some(vec![1; 16]);
    blocks.damage = Some(vec![2; 8]);
    blocks.sequences = vec![
        ("first".to_owned(), vec![3; 12]),
        (String::new(), vec![4; 4]),
    ];
    let mut unknowns = RecordSpec::named("unknowns");
    unknowns.unknowns = 2;
    unknowns.index_words = 6;
    vec![
        RecordSpec::named("reserved_anim_0"),
        tables,
        blocks,
        unknowns,
        RecordSpec::named("last"),
    ]
}

/// The acceptance criterion's derivation, stated as arithmetic and checked: a
/// record is 272 bytes plus its tables plus 64 bytes and the declared events
/// per sequence block, and record *n + 1* starts where record *n* ends.
#[test]
fn accept_m01_lc_anim_records_a_records_length_is_derived_from_its_own_counts_and_event_sizes() {
    let specs = mixed_specs();
    let walked = walk_synthetic(record_payload(&specs, &[])).expect("the walk reads");
    assert_eq!(walked.len(), specs.len());

    let expected_lens = [
        272,
        // objects 3 x 92, nodes 2 x 44, lights 44, puffers 2 x 44, dynamic 44,
        // static 2 x 40, prerequisites 2 x 48, refs 72, one 64-byte block with
        // 20 event bytes.
        272 + 3 * 92 + 2 * 44 + 44 + 2 * 44 + 44 + 2 * 40 + 2 * 48 + 72 + 64 + 20,
        // reset 64 + 16, damage 64 + 8, then 64 + 12 and 64 + 4.
        272 + (64 + 16) + (64 + 8) + (64 + 12) + (64 + 4),
        // two 36-byte unknowns and six 4-byte index words.
        272 + 2 * 36 + 6 * 4,
        272,
    ];
    let mut start = 108_u64;
    for (record, expected) in walked.iter().zip(expected_lens) {
        assert_eq!(record.offset, start, "{}: start", record.name);
        assert_eq!(record.len, expected, "{}: derived length", record.name);
        start += expected as u64;
    }
    assert_eq!(
        walked
            .iter()
            .map(|record| record.name.as_str())
            .collect::<Vec<_>>(),
        vec!["reserved_anim_0", "tables", "blocks", "unknowns", "last"],
        "every record is addressed by its own index, in order"
    );
    assert!(walked[0].trailing.is_empty());

    // The tables, in on-disk order, each with its own first entry's name.
    let kinds: Vec<_> = walked[1]
        .tables
        .iter()
        .map(|(kind, count, _)| (kind.code(), *count))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("objects", 3),
            ("nodes", 2),
            ("lights", 1),
            ("puffers", 2),
            ("dynamic_sounds", 1),
            ("static_sounds", 2),
            ("activation_prerequisites", 2),
            ("animation_refs", 1),
        ]
    );
    assert_eq!(
        walked[1].tables[0].2.as_deref(),
        Some("t".to_owned() + "a2_0").as_deref(),
        "an object entry's name is at the start of the entry"
    );
    assert_eq!(
        walked[1].tables[1].2.as_deref(),
        Some("ta3_0"),
        "a node entry's name is four bytes in"
    );

    // Reset, damage, then the ordinary sequences, each with its own events.
    assert_eq!(
        walked[2].sequences,
        vec![
            (
                AnimationRecordSequenceKind::Reset,
                "RESET_SEQUENCE".to_owned(),
                vec![1; 16]
            ),
            (
                AnimationRecordSequenceKind::Damage,
                "DAMAGE_SEQUENCE".to_owned(),
                vec![2; 8]
            ),
            (
                AnimationRecordSequenceKind::Sequence,
                "first".to_owned(),
                vec![3; 12]
            ),
            (
                AnimationRecordSequenceKind::Sequence,
                String::new(),
                vec![4; 4]
            ),
        ]
    );
    assert_eq!(
        walked[3]
            .tables
            .iter()
            .map(|(kind, count, _)| (kind.code(), *count))
            .collect::<Vec<_>>(),
        vec![("unknowns", 2), ("index_words", 6)]
    );
}

/// Every refusal names the record and the part that did not fit, and an effect
/// table — for which no entry size is measured — is refused rather than
/// skipped with a guessed size.
#[test]
fn accept_m01_lc_anim_records_a_record_that_does_not_fit_is_a_named_refusal() {
    // Declared three records, only two present.
    let mut short = record_payload(&mixed_specs()[..2], &[]);
    short[10..12].copy_from_slice(&3_u16.to_le_bytes());
    match walk_synthetic(short) {
        Err(AnimationRecordError::Truncated {
            record,
            part,
            needed,
            ..
        }) => {
            assert_eq!((record, part, needed), (2, "fixed part", 272));
        }
        other => panic!("expected a truncated fixed part, got {:?}", other.err()),
    }

    // A table that runs past the end of the payload.
    let mut spec = RecordSpec::named("cut");
    spec.objects = 4;
    let mut cut = record_payload(&[spec], &[]);
    cut.truncate(cut.len() - 92);
    match walk_synthetic(cut) {
        Err(AnimationRecordError::Truncated { part, needed, .. }) => {
            assert_eq!((part, needed), ("objects", 4 * 92));
        }
        other => panic!("expected a truncated table, got {:?}", other.err()),
    }

    // A sequence whose events claim more bytes than remain.
    let mut spec = RecordSpec::named("events");
    spec.sequences = vec![("seq".to_owned(), vec![9; 10])];
    let mut events = record_payload(&[spec], &[]);
    events.truncate(events.len() - 1);
    match walk_synthetic(events) {
        Err(AnimationRecordError::Truncated { part, needed, .. }) => {
            assert_eq!((part, needed), ("sequence events", 10));
        }
        other => panic!("expected truncated events, got {:?}", other.err()),
    }

    // A sequence block cut inside its 64-byte info.
    let mut spec = RecordSpec::named("info");
    spec.sequences = vec![("seq".to_owned(), Vec::new())];
    let mut info = record_payload(&[spec], &[]);
    info.truncate(info.len() - 10);
    match walk_synthetic(info) {
        Err(AnimationRecordError::Truncated { part, needed, .. }) => {
            assert_eq!((part, needed), ("sequence info", 64));
        }
        other => panic!("expected a truncated info block, got {:?}", other.err()),
    }

    // An effect table has no measured entry size.
    let mut spec = RecordSpec::named("effects");
    spec.effects = 2;
    match walk_synthetic(record_payload(&[spec], &[])) {
        Err(
            error @ AnimationRecordError::UnmeasuredEffectTable {
                record: 0,
                count: 2,
                ..
            },
        ) => {
            assert_eq!(error.code(), "unmeasured_effect_table");
        }
        other => panic!("expected the effect-table refusal, got {:?}", other.err()),
    }
}

/// What follows the last record is reported, not walked.
#[test]
fn accept_m01_lc_anim_records_the_region_after_the_last_record_is_reported_not_walked() {
    let specs = mixed_specs();
    let expected_end: u64 = 108
        + specs
            .iter()
            .map(|spec| spec.bytes().len() as u64)
            .sum::<u64>();
    let trailing: Vec<u8> = (0..37).collect();
    let walked = walk_synthetic(record_payload(&specs, &trailing)).expect("the walk reads");
    assert_eq!(
        walked.len(),
        specs.len(),
        "the declared count bounds the walk"
    );
    assert_eq!(walked[0].trailing, trailing);

    let carrier = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[(r"..\data\c1c\m01\zrdr\mis_anim.zrd", 10)],
        &record_payload(&specs, &trailing),
    );
    let index = read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", synthetic_path(), &carrier)
        .expect("the index reads");
    let payload = index.payload().expect("the payload reads");
    let walk = payload.records().expect("the walk reads");
    assert_eq!(walk.trailing_offset(), expected_end);
    assert_eq!(walk.trailing().len(), 37);
    assert!(
        payload.records_not_decoded_reason().contains("not walked"),
        "the open state names the region it leaves"
    );
}

/// Pointer words are returned raw and never used: the reset and damage words
/// say whether a block is present, and nothing resolves one to a record.
#[test]
fn accept_m01_lc_anim_records_pointer_words_are_reported_raw_and_unresolved() {
    let specs = mixed_specs();
    let carrier = anim_container_bytes(
        &[("zbd\\c1c\\gamez.zbd", 1), ("zbd\\planes.zbd", 2)],
        &[(r"..\data\c1c\m01\zrdr\mis_anim.zrd", 10)],
        &record_payload(&specs, &[]),
    );
    let index = read_synthetic_index("zbd/c1c/m01/mis_anim.zbd", synthetic_path(), &carrier)
        .expect("the index reads");
    let payload = index.payload().expect("the payload reads");
    let walk = payload.records().expect("the walk reads");

    let blocks = walk.get(2).expect("record 2");
    let pointers = blocks.pointers();
    assert_eq!(pointers.reset_state, 0x0436_1000);
    assert_eq!(pointers.damage_sequence, 0x0436_2000);
    assert_eq!(
        blocks.pointers_unresolved_reason(),
        POINTERS_UNRESOLVED_REASON
    );
    let bare = walk.get(0).expect("record 0").pointers();
    assert_eq!((bare.reset_state, bare.damage_sequence), (0, 0));
    assert_eq!(
        blocks.field_evidence(),
        cs_types::evidence::ClaimStatus::ObservedTool,
        "every fixed-part field is this repository's own measurement"
    );
    // The pointer is a word, never an offset: a record whose pointer values
    // are far beyond the payload walks exactly the same.
    assert!(u64::from(pointers.reset_state) > payload.bytes().len() as u64);
    assert_eq!(blocks.sequences().len(), 4);
    assert_eq!(
        walk.by_anim_name(b"blocks")
            .map(|record| record.index())
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert!(walk.by_anim_name(b"nope").next().is_none());
}

/// A startup identity binds only to exactly one record across the mission
/// carrier and its camera carrier; absent and ambiguous identities stay open
/// with their own reason.
#[test]
fn accept_m01_lc_anim_records_a_startup_identity_binds_only_to_exactly_one_record() {
    fn carrier_over(names: &[&str]) -> Vec<u8> {
        let specs: Vec<RecordSpec> = std::iter::once(RecordSpec::named("reserved_anim_0"))
            .chain(names.iter().map(|name| RecordSpec::named(name)))
            .collect();
        anim_container_bytes(
            &[
                ("zbd\\c1c\\gamez.zbd", 0x39a7_7e80),
                ("zbd\\planes.zbd", 0x39a7_7ba5),
            ],
            &[(r"..\data\c1c\m01\zrdr\placezeps.zrd", 1)],
            &record_payload(&specs, &[]),
        )
    }
    let startup = start_anims(&[
        (
            "NEW_GAME_START",
            &["mission_only", "camera_only", "both", "neither"],
        ),
        ("LOAD_GAME_START", &["repeated_in_camera"]),
    ]);
    let mission_reader = reader_over(&[
        ("mis_anim.zrd", animation_definitions(&[], &[])),
        (STARTUP_MEMBER, startup),
    ]);
    let camera_reader = reader_over(&[("cam_anim.zrd", animation_definitions(&[], &[]))]);
    let mission_carrier = carrier_over(&["mission_only", "both"]);
    let camera_carrier = carrier_over(&[
        "camera_only",
        "both",
        "repeated_in_camera",
        "repeated_in_camera",
    ]);
    let mission = bind_animation_carrier(
        &RelativePath::new("zbd/c1c/m01/mis_anim.zbd").expect("a relative spelling"),
        "zbd/c1c/m01/zrdr.zbd",
        CarrierKind::Mission,
        &mission_carrier,
        SiblingReader::Bytes(&mission_reader),
    );
    let camera = bind_animation_carrier(
        &RelativePath::new("zbd/c1c/cam_anim.zbd").expect("a relative spelling"),
        "zbd/c1c/zrdr.zbd",
        CarrierKind::Camera,
        &camera_carrier,
        SiblingReader::Bytes(&camera_reader),
    );
    assert!(mission.blockers.is_empty(), "{:?}", mission.blockers);
    assert!(camera.blockers.is_empty(), "{:?}", camera.blockers);
    let facts = mission
        .payload
        .as_ref()
        .and_then(|payload| payload.records.as_ref())
        .expect("the mission carrier's records walked");
    assert_eq!(facts.count, 3);
    assert_eq!(facts.indices_named(b"both"), vec![2]);

    let rows = bind_startup_identities(&mission, Some(&camera));
    let outcomes: Vec<_> = rows
        .iter()
        .map(|row| (row.identity.as_str(), row.outcome.clone()))
        .collect();
    assert_eq!(
        outcomes,
        vec![
            (
                "mission_only",
                StartupOutcome::Bound {
                    carrier: CarrierKind::Mission,
                    record: 1
                }
            ),
            (
                "camera_only",
                StartupOutcome::Bound {
                    carrier: CarrierKind::Camera,
                    record: 1
                }
            ),
            (
                "both",
                StartupOutcome::Unbound {
                    reason: UNBOUND_REASON_AMBIGUOUS,
                    matches: vec![(CarrierKind::Mission, 2), (CarrierKind::Camera, 2)],
                }
            ),
            (
                "neither",
                StartupOutcome::Unbound {
                    reason: UNBOUND_REASON_NO_RECORD,
                    matches: Vec::new(),
                }
            ),
            (
                "repeated_in_camera",
                StartupOutcome::Unbound {
                    reason: UNBOUND_REASON_AMBIGUOUS,
                    matches: vec![(CarrierKind::Camera, 3), (CarrierKind::Camera, 4)],
                }
            ),
        ],
        "an identity binds only to exactly one record; the rest keep their reason"
    );
    assert_eq!(
        rows[0].key, "NEW_GAME_START",
        "the startup key travels with its identity"
    );
    assert_eq!(rows[4].key, "LOAD_GAME_START");

    // Without the camera carrier, "exactly one" cannot be established, so
    // nothing binds.
    let alone = bind_startup_identities(&mission, None);
    assert_eq!(alone.len(), 5);
    assert!(
        alone
            .iter()
            .all(|row| matches!(row.outcome, StartupOutcome::Unbound { .. })),
        "a missing camera carrier binds nothing"
    );
}

/// The retail half: the walk over every original carrier, with the measured
/// counts of `zbd/c1c/m01/mis_anim.zbd` and `zbd/c1c/cam_anim.zbd` pinned.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_m01_lc_anim_records_retail_walk_pins_counts_and_startup_census() {
    let root = game_dir();
    let survey = survey_animation_bindings(&root).expect("the survey runs over the install");
    assert_eq!(survey.carriers.len(), 61);

    let mut total_records = 0_usize;
    let mut zero_trailing = 0_usize;
    let mut trailing_min = u64::MAX;
    let mut trailing_max = 0_u64;
    for carrier in &survey.carriers {
        assert!(
            carrier.blockers.is_empty(),
            "{}: {:?}",
            carrier.container_key,
            carrier.blockers
        );
        let payload = carrier.payload.as_ref().expect("payload read");
        let records = payload.records.as_ref().expect("records walked");
        assert_eq!(
            records.count,
            usize::from(payload.declared_record_count),
            "{}: the walk reaches the declared count and stays inside the payload",
            carrier.container_key
        );
        assert_eq!(
            payload.first_record_name, b"reserved_anim_0",
            "{}: record 0",
            carrier.container_key
        );
        assert_eq!(records.anim_names[0], b"reserved_anim_0");
        total_records += records.count;
        if records.trailing_bytes == 0 {
            zero_trailing += 1;
        } else {
            trailing_min = trailing_min.min(records.trailing_bytes);
            trailing_max = trailing_max.max(records.trailing_bytes);
        }
    }
    assert_eq!(total_records, 15_024, "every declared record is walked");
    assert_eq!(
        zero_trailing, 30,
        "carriers whose records end at the payload's end"
    );
    assert_eq!((trailing_min, trailing_max), (29_690, 1_361_762));

    // The two carriers the task names.
    let m01 = survey
        .carrier("zbd/c1c/m01/mis_anim.zbd")
        .and_then(|carrier| carrier.payload.as_ref())
        .and_then(|payload| payload.records.as_ref())
        .expect("M01's records");
    assert_eq!(m01.count, 573);
    assert_eq!(m01.trailing_bytes, 543_041);
    assert_eq!(m01.trailing_offset, 1_468_528);
    let camera = survey
        .carrier("zbd/c1c/cam_anim.zbd")
        .and_then(|carrier| carrier.payload.as_ref())
        .and_then(|payload| payload.records.as_ref())
        .expect("the c1c camera records");
    assert_eq!(camera.count, 307);
    assert_eq!(camera.trailing_bytes, 405_135);

    // Identities, as measured: M01's anim names are unique, the camera
    // carrier repeats five.
    let unique = |names: &[Vec<u8>]| {
        names
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    };
    assert_eq!(unique(&m01.anim_names), 573);
    assert_eq!(unique(&camera.anim_names), 302);
    assert_eq!(m01.indices_named(b"pzep_engines_start"), vec![36]);
    assert_eq!(m01.indices_named(b"wvzep_engines_start"), vec![414]);
    assert_eq!(m01.indices_named(b"bszep_engines_start"), vec![230]);
    assert_eq!(m01.indices_named(b"wv_hookup_state"), vec![496]);
    assert_eq!(camera.indices_named(b"generic_intro"), vec![23]);
    assert_eq!(camera.indices_named(b"call_add_jack"), vec![60]);
    assert_eq!(camera.indices_named(b"player_setup"), vec![115]);

    // The startup census: all 53 scopes, 195 identities.
    let scopes = survey.startup_bindings();
    assert_eq!(scopes.len(), 53);
    let identities: usize = scopes.iter().map(|scope| scope.bindings.len()).sum();
    assert_eq!(identities, 195);
    let m01_scope = scopes
        .iter()
        .find(|scope| scope.mission_key == "zbd/c1c/m01/mis_anim.zbd")
        .expect("M01's scope");
    assert_eq!(m01_scope.camera_key, "zbd/c1c/cam_anim.zbd");
    let by_identity: std::collections::BTreeMap<_, _> = m01_scope
        .bindings
        .iter()
        .map(|row| (row.identity.as_str(), row.outcome.clone()))
        .collect();
    assert_eq!(
        by_identity["wv_hookup_state"],
        StartupOutcome::Bound {
            carrier: CarrierKind::Mission,
            record: 496
        }
    );
    assert_eq!(
        by_identity["player_setup"],
        StartupOutcome::Bound {
            carrier: CarrierKind::Camera,
            record: 115
        }
    );
    assert_eq!(
        m01_scope.bound_count(),
        7,
        "all of M01's seven identities bind"
    );
    let census = startup_census(&scopes);
    assert_eq!(census, STARTUP_CENSUS);
}

/// The retail half's field census: every claim the record reader's docs make
/// about the original files is counted here from the 15 024 walked records,
/// not copied out of a finding.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_m01_lc_anim_records_retail_fields_hold_over_every_record() {
    let root = game_dir();
    let found =
        cs_assets::install::discover(&root).expect("production discovery reads the install");
    let mut records = 0_usize;
    let mut nonzero = 0_usize;
    let mut blocks = 0_usize;
    let (mut pointer_min, mut pointer_max) = (u32::MAX, 0_u32);
    let mut largest_container = 0_usize;
    let mut anim_equals_root = 0_usize;
    let mut priorities = std::collections::BTreeSet::new();
    let mut activations = std::collections::BTreeSet::new();
    for file in &found.manifest.files {
        let key = file.relative_spelling.logical_key();
        if !(key.ends_with("/mis_anim.zbd") || key.ends_with("/cam_anim.zbd")) {
            continue;
        }
        let bytes =
            std::fs::read(root.join(file.relative_spelling.as_str())).expect("read carrier");
        largest_container = largest_container.max(bytes.len());
        let index = read_synthetic_index(&key, &file.relative_spelling, &bytes).expect("index");
        let payload = index.payload().expect("payload");
        let walk = payload
            .records()
            .expect("the walk reaches the declared count");
        assert_eq!(
            walk.len(),
            usize::from(payload.header().declared_record_count),
            "{key}"
        );
        // Record 0 is a bare fixed part with every count zero.
        let first = walk.get(0).expect("record 0");
        assert_eq!(
            first.len(),
            272,
            "{key}: record 0 is exactly the fixed part"
        );
        assert_eq!(first.anim_name(), b"reserved_anim_0");
        assert!(
            first.tables().is_empty() && first.sequences().is_empty(),
            "{key}"
        );
        // The records tile the payload: each starts where the last ended.
        let mut next = 108_u64;
        for record in walk.iter() {
            assert_eq!(
                record.payload_offset(),
                next,
                "{key}: record {}",
                record.index()
            );
            next += record.len() as u64;
            records += 1;
        }
        assert_eq!(walk.trailing_offset(), next, "{key}");
        for record in walk.iter().skip(1) {
            nonzero += 1;
            assert_eq!(record.status(), 0, "{key}");
            assert_eq!(record.two_word(), 2, "{key}");
            assert_eq!(record.counts().effects, 0, "{key}: no effect tables");
            priorities.insert(record.execution_priority());
            activations.insert(record.activation());
            let pointers = record.pointers();
            if pointers.anim == pointers.anim_root {
                anim_equals_root += 1;
            }
            for word in [
                pointers.unknowns,
                pointers.seq_defs,
                pointers.reset_state,
                pointers.damage_sequence,
                pointers.objects,
                pointers.nodes,
                pointers.lights,
                pointers.puffers,
                pointers.dynamic_sounds,
                pointers.static_sounds,
                pointers.effects,
                pointers.activation_prerequisites,
                pointers.animation_refs,
            ] {
                if word != 0 {
                    pointer_min = pointer_min.min(word);
                    pointer_max = pointer_max.max(word);
                }
            }
            // The reset and damage words say whether the block is present.
            let kinds: Vec<_> = record
                .sequences()
                .iter()
                .map(|block| block.kind())
                .collect();
            assert_eq!(
                kinds.contains(&AnimationRecordSequenceKind::Reset),
                pointers.reset_state != 0,
                "{key}"
            );
            assert_eq!(
                kinds.contains(&AnimationRecordSequenceKind::Damage),
                pointers.damage_sequence != 0,
                "{key}"
            );
            assert_eq!(
                kinds
                    .iter()
                    .filter(|kind| **kind == AnimationRecordSequenceKind::Sequence)
                    .count(),
                usize::from(record.counts().sequences),
                "{key}"
            );
            for block in record.sequences() {
                blocks += 1;
                assert!(matches!(block.flags(), 0 | 0x303), "{key}: block flags");
                assert_ne!(
                    block.pointer(),
                    0,
                    "{key}: a block carries its pointer word"
                );
                assert!(!block.events().is_empty(), "{key}: no empty event stream");
                assert!(
                    block.info()[36..56].iter().all(|byte| *byte == 0),
                    "{key}: info bytes 36..56 are zero"
                );
                match block.kind() {
                    AnimationRecordSequenceKind::Reset => {
                        assert_eq!(block.name(), b"RESET_SEQUENCE", "{key}")
                    }
                    AnimationRecordSequenceKind::Damage => {
                        assert_eq!(block.name(), b"DAMAGE_SEQUENCE", "{key}")
                    }
                    AnimationRecordSequenceKind::Sequence => {}
                }
            }
            // The zero entry of the object and node tables is unnamed.
            for kind in [
                AnimationRecordTableKind::Objects,
                AnimationRecordTableKind::Nodes,
            ] {
                if let Some(table) = record.table(kind) {
                    assert_eq!(
                        table.name(0),
                        Some(&b""[..]),
                        "{key}: {} entry 0",
                        kind.code()
                    );
                }
            }
        }
    }
    assert_eq!(records, 15_024);
    assert_eq!(nonzero, 14_963);
    assert_eq!(
        blocks, 56_994,
        "reset, damage and ordinary blocks over the installation"
    );
    assert_eq!(
        anim_equals_root, 12_051,
        "the two small id words agree in 12 051 records"
    );
    assert_eq!(
        priorities.iter().copied().collect::<Vec<_>>(),
        vec![1, 4, 5, 6],
        "the pinned MechWarrior source asserts 4; this family does not"
    );
    assert_eq!(
        activations.iter().copied().collect::<Vec<_>>(),
        vec![0, 2, 3, 4]
    );
    // No pointer word can be an offset into any container: every nonzero one
    // is far beyond the largest carrier.
    assert_eq!(largest_container, 2_019_493);
    assert!(
        pointer_min as usize > largest_container,
        "smallest nonzero pointer word {pointer_min:#x} is beyond every container"
    );
    assert_eq!((pointer_min, pointer_max), (0x01fa_dcf8, 0x04f7_fe60));
}

/// `(bound in mission, bound in camera, ambiguous, absent)` over all identities.
type StartupCensus = (usize, usize, usize, usize);

/// The measured census of the exactly-one rule over the retail installation:
/// 185 of the 195 identities bind (117 to a record of their own mission
/// carrier, 68 to one of their group's camera carrier), none is ambiguous, and
/// 10 match no record of either carrier — `pure_panic` x4, `deactivate_bmhookup_node`
/// x3, `fueltrlight1`, `black_chimneysmoke` and `dtzep_engines_start`. Three of
/// those names *are* record names in another scope's carrier (c1/m02, c1's
/// camera, c2/m02, c4/m04), which is unmeasured evidence about the original's
/// lookup and is not used.
const STARTUP_CENSUS: StartupCensus = (117, 68, 0, 10);

fn startup_census(scopes: &[cs_app::animation::carrier::ScopeStartupBinding]) -> StartupCensus {
    let mut census = (0, 0, 0, 0);
    for row in scopes.iter().flat_map(|scope| &scope.bindings) {
        match &row.outcome {
            StartupOutcome::Bound {
                carrier: CarrierKind::Mission,
                ..
            } => census.0 += 1,
            StartupOutcome::Bound {
                carrier: CarrierKind::Camera,
                ..
            } => census.1 += 1,
            StartupOutcome::Unbound { matches, .. } if matches.is_empty() => census.3 += 1,
            StartupOutcome::Unbound { .. } => census.2 += 1,
        }
    }
    census
}

// ------------------- task #633: the evidence-report harness ------------------
//
// Not part of the acceptance suite: its name does not carry the task's prefix,
// it fails loudly when its inputs are missing rather than passing vacuously,
// and the task's selection must never pick it up. Run from the workspace root
// after the acceptance suite, exactly as:
//
// 1. ```sh
//    cargo test --workspace --locked -- accept_m01_lc_anim_carriers_ --include-ignored \
//      2>&1 | tee private/evidence/M01-LC-ANIM-CARRIERS/cargo-test.log
//    ```
//    (record the pipeline's exit status — it is passed here as
//    `CS_EVIDENCE_EXIT_CODE`.)
// 2. ```sh
//    CS_EVIDENCE_DIR=private/evidence/M01-LC-ANIM-CARRIERS \
//    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_anim_carriers_ --include-ignored" \
//    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//    CS_GAME_DIR=<original installation> \
//      cargo test --locked -p cs_app --test accept_f20_d_validation -- \
//        --ignored --exact evidence_report_m01_lc_anim_carriers_writes_the_acceptance_report
//    ```
// 3. ```sh
//    python3 tools/validate_evidence.py \
//      private/evidence/M01-LC-ANIM-CARRIERS/acceptance.json \
//      --artifact-root private/evidence/M01-LC-ANIM-CARRIERS --require-pass
//    ```
// 4. Commit a copy of `acceptance.json` as
//    `docs/findings/evidence/M01-LC-ANIM-CARRIERS.json`.
//
// `unknowns` holds *this task's* blockers, which is empty because the
// acceptance run passed. The **product** state this task leaves open — the
// animation records behind the payload header are not decoded, so the
// `startanims.zrd` identities are unbound, and the payload header's own
// unmeasured words are unmeasured — is written into `review.method` below, so it
// travels in the machine-readable record and cannot be dropped to make a
// validator green. The same state is in
// `docs/findings/2026-10-04-m01-lc-anim-carriers.md`.

/// The acceptance tests whose `retail` capability this report declares.
const M01LC_CAPABILITY_TESTS: &[&str] =
    &["accept_m01_lc_anim_carriers_retail_c1c_lists_binds_and_reports_every_member"];

/// The synthetic half of the suite, which must be present beside the retail
/// one.
const M01LC_SYNTHETIC_TESTS: &[&str] = &[
    "accept_m01_lc_anim_carriers_an_animation_container_is_not_a_reader_archive",
    "accept_m01_lc_anim_carriers_the_index_lists_every_row_and_the_payload_header",
    "accept_m01_lc_anim_carriers_a_nonzero_byte_after_a_path_is_reported_not_decoded",
    "accept_m01_lc_anim_carriers_a_definition_file_binds_to_its_member_row",
    "accept_m01_lc_anim_carriers_a_joined_animation_path_offers_every_root_in_order",
    "accept_m01_lc_anim_carriers_a_reference_no_member_answers_is_reported_not_matched",
    "accept_m01_lc_anim_carriers_startup_identities_are_listed_and_carry_their_open_reason",
    "accept_m01_lc_anim_carriers_every_unreadable_input_is_a_named_refusal",
];

/// The task-test prefix this report is about.
const M01LC_PREFIX: &str = "accept_m01_lc_anim_carriers_";

/// The derived member/join census written beside the report and referenced by
/// digest: keys, member paths, stamps, spans, payload header words, references
/// and dispositions — never original content.
const M01LC_CENSUS_ARTIFACT: &str = "anim-carriers.json";

/// The product state this task leaves open, as it travels in this report.
const M01LC_OPEN_STATE: &str = "OPEN, and not this task's blocker: (1) the animation records behind \
     each carrier's payload header are NOT decoded (cs_formats::zbd::anim \
     RECORDS_NOT_DECODED_REASON) — no source documents their layout, their inline sub-table sizes \
     are underived, and their record-local pointers do not resolve inside the container, so a \
     bound member is a definition file and not its animation records; (2) the 195 startanims.zrd \
     identities over the 53 mission scopes are therefore read but unbound, and which carrier \
     (mission or camera) an identity lives in is unmeasured; (3) the payload header's +12 +14 \
     +16 +20 +32 +34 +40 words are measured values with no measured meaning, and the +10 word is \
     an inference (a declared record count nothing can index by); (4) 8 of 739 definition-file \
     references name no member, and whether the original resolved them — in particular M01's \
     ..\\data\\common\\zrdr\\zeps\\wv_tailhook.zrd against the carrier's mission-rooted spelling — \
     is UNMEASURED, since no original executable was run. Affected content: every mission-facing \
     animation claim (VS-M01-RUNTIME #359, F20-D's family validation, M01-B).";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m01_lc_anim_carriers_writes_the_acceptance_report() {
    let evidence_dir = m01lc_workspace_path(&m01lc_env("CS_EVIDENCE_DIR"));
    let candidate_tree = m01lc_env("CS_CANDIDATE_TREE");
    let argv: Vec<String> = m01lc_env("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = m01lc_env("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(m01lc_env("CS_GAME_DIR"));

    let head_tree = m01lc_git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; old \
         reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = std::fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = m01lc_parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `{M01LC_PREFIX}` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed.
    for required in M01LC_CAPABILITY_TESTS.iter().chain(M01LC_SYNTHETIC_TESTS) {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == required)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{required} did not run: this task needs `retail`, so step 1 must use \
                     --include-ignored with CS_GAME_DIR set"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }

    // The hashes describe the real installation, measured by production
    // discovery — the very pass this task's survey is built on.
    let found = cs_assets::install::discover(&game_dir)
        .expect("production discovery must read the original installation");
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();

    // The survey itself, re-run: the census artifact's numbers are this run's,
    // not a transcription of a test message.
    let survey = survey_animation_bindings(&game_dir).expect("the binding survey runs");
    let census_path = evidence_dir.join(M01LC_CENSUS_ARTIFACT);
    std::fs::write(&census_path, m01lc_census_json(&survey))
        .unwrap_or_else(|error| panic!("write {}: {error}", census_path.display()));

    let artifacts = vec![
        m01lc_artifact(&log_path, "log"),
        m01lc_artifact(&census_path, "json"),
    ];

    let method = format!(
        "acceptance suite run locally with the `retail` capability: `cargo test --workspace \
         --locked -- {M01LC_PREFIX} --include-ignored`, every task test passing (the \
         implementer's run is in the recorded log); this harness derives every field from that \
         log, from production discovery of $CS_GAME_DIR, and from the production binding survey \
         re-run over that installation (cs_app::animation::survey_animation_bindings over \
         cs_formats::zbd::anim::read_animation_index), validated with \
         tools/validate_evidence.py --require-pass. gpu and audio were available and UNUSED: \
         nothing was rendered or played. {}",
        M01LC_OPEN_STATE
    );
    let review = "implementer: bunny-alpha-1/bunny-alpha-1 (Rally task #633, session of \
         2026-10-04); reviewer: bunny-alpha-1/bunny-alpha-1 again, in a fresh session with no \
         memory of the implementation — a second pass over the code and an independent re-derivation \
         of every measured number, but the SAME agent identity, so it is not an independent \
         reviewer and not independent original-reference evidence (a follow-up task asks for one). \
         The review fixed a fail-open empty first-record name, an Unvalidated-header arm that read \
         the index anyway, the row anomalies the binding had dropped, and three measured-claim \
         slips in this repository's docs; the claim stays at level `implemented`, and no agent \
         review replaces the owner's human approval";

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M01-LC-ANIM-CARRIERS\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \
         \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        m01lc_json(&candidate_tree),
        m01lc_json(&m01lc_rustc_version()),
        m01lc_json(&m01lc_locked_version("bevy")),
        m01lc_json(&m01lc_locked_version("avian3d")),
        m01lc_json(&m01lc_iso_utc_now()),
        m01lc_str_array(&argv),
        m01lc_json(&m01lc_git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        m01lc_json(&install_sha256),
        m01lc_json(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        m01lc_assertion_array(&suite),
        m01lc_artifact_array(&artifacts),
        m01lc_json(review),
        m01lc_json(&method),
    );

    let out = evidence_dir.join("acceptance.json");
    std::fs::write(&out, &document)
        .unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    let written = std::fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M01-LC-ANIM-CARRIERS\"",
        "\"capabilities\": [\"retail\", \"synthetic\"]",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    for open in [
        "animation records behind each carrier",
        "195 startanims.zrd identities",
        "SAME agent identity",
    ] {
        assert!(
            written.contains(open),
            "the report must carry the open product state ({open:?}) in its own words"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written honestly \
         and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The derived census of every carrier the installation declares: keys, member
/// rows with their stamps and spans, the payload header's measured words, the
/// document's references and their dispositions, and the startup identities.
fn m01lc_census_json(survey: &cs_app::animation::AnimationBindingSurvey) -> String {
    let rows = survey
        .carriers
        .iter()
        .map(|carrier| {
            let members = carrier
                .members
                .iter()
                .map(|row| {
                    format!(
                        "{{\"index\":{},\"path\":{},\"stamp\":{},\"offset\":{},\"length\":{},\
                          \"referenced_by\":[{}]}}",
                        row.index,
                        m01lc_json(&row.path),
                        row.stamp,
                        row.span.offset,
                        row.span.length,
                        row.references
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let externals = carrier
                .externals
                .iter()
                .map(|row| format!("{{\"path\":{},\"stamp\":{}}}", m01lc_json(&row.path), row.stamp))
                .collect::<Vec<_>>()
                .join(",");
            let references = carrier
                .references
                .iter()
                .enumerate()
                .map(|(ordinal, entry)| {
                    format!(
                        "{{\"ordinal\":{ordinal},\"raw\":{},\"member\":{}}}",
                        m01lc_json(&entry.raw),
                        entry
                            .member
                            .map_or_else(|| "null".to_owned(), |member| member.to_string())
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let unresolved = carrier
                .unresolved
                .iter()
                .map(|entry| {
                    format!(
                        "{{\"raw\":{},\"candidates\":[{}]}}",
                        m01lc_json(&entry.raw),
                        entry
                            .candidates
                            .iter()
                            .map(|candidate| m01lc_json(candidate))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let payload = match &carrier.payload {
                Some(payload) => format!(
                    "{{\"offset\":{},\"length\":{},\"declared_record_count\":{},\"gravity\":{},\
                      \"record_table_offset\":{},\"first_record_name\":{},\
                      \"records_decoded\":false}}",
                    payload.span.offset,
                    payload.span.length,
                    payload.declared_record_count,
                    payload.gravity,
                    payload.record_table_offset,
                    m01lc_bytes(&payload.first_record_name),
                ),
                None => "null".to_owned(),
            };
            let startup = carrier
                .startup
                .as_ref()
                .map(|startup| {
                    let groups = startup
                        .groups
                        .iter()
                        .map(|group| {
                            format!(
                                "{{\"key\":{},\"identities\":[{}]}}",
                                m01lc_json(&group.key),
                                group
                                    .identities
                                    .iter()
                                    .map(|identity| m01lc_json(identity))
                                    .collect::<Vec<_>>()
                                    .join(",")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("{{\"member\":{},\"groups\":[{groups}]}}", m01lc_json(&startup.member))
                })
                .unwrap_or_else(|| "null".to_owned());
            let blockers = carrier
                .blockers
                .iter()
                .map(|blocker| format!("{{\"label\":{}}}", m01lc_json(blocker.label())))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"container\":{},\"kind\":{},\"version\":{},\"size_bytes\":{},\"bound\":{},\
                  \"externals\":[{externals}],\"members\":[{members}],\"payload\":{payload},\
                  \"references\":[{references}],\"unresolved\":[{unresolved}],\"startup\":{startup},\
                  \"blockers\":[{blockers}]}}",
                m01lc_json(&carrier.container_key),
                m01lc_json(carrier.kind.label()),
                carrier.version,
                carrier.size_bytes,
                carrier.is_bound(),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":\"cs-m01-lc-anim-carriers-census/1\",\"carriers\":{},\"complete\":{},\
          \"members\":{},\"references\":{},\"bound\":{},\"unresolved\":{},\"carrier_rows\":[{}]}}",
        survey.carriers.len(),
        survey.is_complete(),
        survey
            .carriers
            .iter()
            .map(|carrier| carrier.members.len())
            .sum::<usize>(),
        survey
            .carriers
            .iter()
            .map(|row| row.references.len())
            .sum::<usize>(),
        survey
            .carriers
            .iter()
            .map(|row| row.bound_reference_count())
            .sum::<usize>(),
        survey
            .carriers
            .iter()
            .map(|row| row.unresolved.len())
            .sum::<usize>(),
        rows,
    )
}

// ----------------------------------------------------------------- inputs ---

fn m01lc_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its section header \
             (crates/cs_app/tests/accept_f20_d_validation.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory at the *package* root,
/// so a workspace-relative evidence path is re-anchored here.
fn m01lc_workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&m01lc_git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn m01lc_git(args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn m01lc_rustc_version() -> String {
    let output = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The locked version of one `Cargo.lock` package: read, never asserted from
/// memory.
fn m01lc_locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");
    let lock = std::fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            wanted = false;
        } else if let Some(name) = line.strip_prefix("name = \"") {
            wanted = name.trim_end_matches('"') == package;
        } else if let Some(version) = line.strip_prefix("version = \"")
            && wanted
        {
            return version.trim_end_matches('"').to_owned();
        }
    }
    panic!("package {package:?} is not in {}", lock_path.display());
}

fn m01lc_iso_utc_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs();
    // Days since the epoch to a civil date (Howard Hinnant's algorithm), then
    // the time of day.
    let days = (now / 86_400) as i64;
    let seconds = now % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        seconds / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60
    )
}

// ------------------------------------------------------------- log parsing ---

/// What the recorded `cargo test` output says actually happened.
#[derive(Debug, Default)]
struct M01lcSuite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of the task's tests from a recorded
/// `cargo test` output. Only tests whose name carries the task prefix count, so
/// the rest of this binary's suite is never counted as this task's evidence.
fn m01lc_parse_suite(log: &str) -> M01lcSuite {
    let mut suite = M01lcSuite::default();
    let mut pending: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in m01lc_summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        if pending.front().is_some() {
            if trimmed == "ok" {
                let name = pending.pop_front().expect("pending test");
                m01lc_record(&mut suite, name, "pass");
                continue;
            }
            if trimmed == "FAILED" {
                let name = pending.pop_front().expect("pending test");
                m01lc_record(&mut suite, name, "fail");
                continue;
            }
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let full = &after[..separator];
            let tail = &after[separator + 5..];
            cursor = tail;
            if !full.contains(M01LC_PREFIX) || full.contains("evidence_report") {
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
            match tail.split_whitespace().next() {
                Some("ok") => m01lc_record(&mut suite, name, "pass"),
                Some("FAILED") => m01lc_record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// `(count, kind)` pairs of one `test result:` summary line.
fn m01lc_summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
        }
    }
    fields
}

fn m01lc_record(suite: &mut M01lcSuite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------------ json ---

/// One JSON string, escaped for a report that no other tool rewrites.
fn m01lc_json(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn m01lc_str_array(values: &[String]) -> String {
    values
        .iter()
        .map(|value| m01lc_json(value))
        .collect::<Vec<_>>()
        .join(",")
}

/// One byte string as a JSON array of numbers, so a name is never re-encoded.
fn m01lc_bytes(bytes: &[u8]) -> String {
    format!(
        "[{}]",
        bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn m01lc_assertion_array(suite: &M01lcSuite) -> String {
    suite
        .assertions
        .iter()
        .map(|(id, status)| {
            format!(
                "{{\"id\":{},\"status\":{},\"evidence\":[\"cargo-test.log\"]}}",
                m01lc_json(id),
                m01lc_json(status)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 of this
/// workspace (the validator re-hashes it independently).
fn m01lc_artifact(source: &Path, kind: &str) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let bytes = std::fs::read(source).expect("an artifact is readable");
    (
        name,
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
}

fn m01lc_artifact_array(artifacts: &[(String, String, String)]) -> String {
    artifacts
        .iter()
        .map(|(path, sha256, kind)| {
            format!(
                "{{\"path\":{},\"sha256\":{},\"kind\":{}}}",
                m01lc_json(path),
                m01lc_json(sha256),
                m01lc_json(kind)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}
