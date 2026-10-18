//! PARITY-M01-LC-MEASURE-CAM-FRAME (#1673): the frame of the M01 launch
//! cinematic's camera track positions
//! (finding `docs/findings/2026-10-18-parity-m01-lc-camera-frame-and-rotation.md`).
//!
//! The camera track is the node's own position (event flag bit 0 is clear), so
//! the frame is the parent's. These retail members read `$CS_GAME_DIR` (never
//! write it) and fail loudly without it: they decode `generic_intro`'s
//! `OBJECT_ADD_CHILD` / `OBJECT_DELETE_CHILD` events with the production event
//! walker and the C3 world's node array with the production GameZ reader. The
//! rotation half of the task is tested in `cs_formats`
//! (`tests/zbd/t1673.rs`).

use cs_app::animation::events::walk_event_stream;
use cs_formats::gamez::{NodeKind, read_gamez_nodes};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    AnimationRecordSequenceKind, AnimationRecordTableKind, ZbdFamily, ZbdProbe, dispatch,
    family_record, read_animation_index,
};
use cs_types::install::RelativePath;

const OP_ADD_CHILD: u8 = 15;
const OP_DELETE_CHILD: u8 = 16;

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    )
}

/// `(record, sequence, is_reset, opcode, parent, child, offset)` of every child
/// event in the `generic_intro` family of `ZBD/C3/cam_anim.zbd`.
type ChildEvent = (String, String, bool, u8, String, String, u64);

fn child_events() -> Vec<ChildEvent> {
    let bytes = std::fs::read(root().join("ZBD/C3/cam_anim.zbd")).expect("cam_anim.zbd reads");
    let path = RelativePath::new("zbd/c3/cam_anim.zbd").expect("a relative path");
    let key = path.logical_key();
    let mut context = ParseContext::with_defaults(&key);
    let needed = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .map_or(0, |rule| rule.required_bytes())
        .min(bytes.len());
    let decision = dispatch(ZbdProbe::new(&key, &path, &bytes[..needed])).expect("dispatches");
    let index = read_animation_index(&mut context, decision, &bytes).expect("index reads");
    let records = index
        .payload()
        .expect("payload")
        .records()
        .expect("records");
    let mut out = Vec::new();
    for record in records.iter() {
        let name = String::from_utf8_lossy(record.anim_name()).into_owned();
        if !(name.starts_with("gi_") || name == "generic_intro") {
            continue;
        }
        let table = record
            .table(AnimationRecordTableKind::Nodes)
            .expect("a Nodes table");
        let node = |index: i16| {
            usize::try_from(index)
                .ok()
                .and_then(|i| table.name(i))
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .expect("the event names a node of the record's table")
        };
        for sequence in record.sequences() {
            let events = sequence.events();
            for event in walk_event_stream(events).expect("the stream walks") {
                if event.opcode != OP_ADD_CHILD && event.opcode != OP_DELETE_CHILD {
                    continue;
                }
                let at = event.offset as usize;
                let half = |o: usize| i16::from_le_bytes([events[at + o], events[at + o + 1]]);
                out.push((
                    name.clone(),
                    String::from_utf8_lossy(sequence.name()).into_owned(),
                    sequence.kind() == AnimationRecordSequenceKind::Reset,
                    event.opcode,
                    node(half(0xc)),
                    node(half(0xe)),
                    event.offset,
                ));
            }
        }
    }
    out
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_measure_cam_frame_retail_the_intro_parents_the_camera_to_the_pandora() {
    let events = child_events();
    let camera: Vec<&ChildEvent> = events.iter().filter(|e| e.5 == "camera1").collect();
    // Exactly three child events name `camera1`, all in `generic_intro`: the
    // reset hands it back by deleting it from the Pandora; the start script
    // takes it out of the world and adds it to the Pandora.
    assert_eq!(camera.len(), 3, "{camera:?}");
    assert!(camera.iter().all(|e| e.0 == "generic_intro"));
    let reset: Vec<_> = camera.iter().filter(|e| e.2).collect();
    assert_eq!(reset.len(), 1);
    assert_eq!(
        (reset[0].3, reset[0].4.as_str()),
        (OP_DELETE_CHILD, "piratezep")
    );
    let start: Vec<_> = camera.iter().filter(|e| !e.2).collect();
    assert_eq!(start.len(), 2);
    assert!(start.iter().all(|e| e.1 == "start_script"));
    assert_eq!(
        (start[0].3, start[0].4.as_str()),
        (OP_DELETE_CHILD, "world1")
    );
    assert_eq!(
        (start[1].3, start[1].4.as_str()),
        (OP_ADD_CHILD, "piratezep")
    );
    assert!(start[0].6 < start[1].6, "the delete comes before the add");

    // No scene animation (`gi_scene1`, `gi_scene2`, `gi_1stperson`,
    // `gi_playerdrop`) re-parents anything but the pirate fighter.
    for event in events.iter().filter(|e| e.0 != "generic_intro") {
        assert_eq!(
            (event.0.as_str(), event.5.as_str()),
            ("gi_pfighter1", "piratefighter"),
            "{event:?}"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_measure_cam_frame_retail_the_world_graph_around_the_camera() {
    let bytes = std::fs::read(root().join("ZBD/C3/gamez.zbd")).expect("gamez.zbd reads");
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults("zbd/c3/gamez.zbd"), &bytes)
        .expect("the C3 node array reads");
    let named = |name: &str| {
        let found: Vec<_> = nodes.nodes.iter().filter(|n| n.name == name).collect();
        assert_eq!(found.len(), 1, "exactly one `{name}`");
        found[0]
    };
    // `camera1` is a camera node (type 1) with no parent in the file; it is
    // the node the motion executor sends to the camera setters.
    let camera = named("camera1");
    assert_eq!(camera.index, 3);
    assert!(matches!(camera.kind, NodeKind::Camera));
    assert_eq!(camera.parent, None);
    // The Pandora is a child of the world root, which has no parent: once the
    // camera is the Pandora's child, its frame is the Pandora's, whose own
    // frame is the world's.
    let pandora = named("piratezep");
    assert!(matches!(pandora.kind, NodeKind::Object3d(_)));
    let world = nodes
        .get(pandora.parent.expect("the Pandora has a parent"))
        .expect("the parent exists");
    assert_eq!((world.index, world.name.as_str()), (0, "world1"));
    assert_eq!(world.parent, None);
    assert!(world.children.contains(&pandora.index));
    assert!(
        !world.children.contains(&camera.index),
        "the stored world lists no camera child"
    );
}
