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
