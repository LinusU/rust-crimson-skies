//! PARITY-M01-LC-MEASURE-CAM-TIMELINE (#1674): how the M01 launch cinematic's
//! scenes are sequenced and which camera variant can play
//! (finding `docs/findings/2026-10-18-parity-m01-lc-cinematic-timeline.md`).
//!
//! The synthetic members author every event byte and run in CI. The retail
//! members read `$CS_GAME_DIR` (never write it) and fail loudly without it;
//! the image members read `$CS_ENGINE_IMAGE` and pin the instructions the
//! finding read, so a different image cannot pass them.

use cs_app::animation::events::walk_event_stream;
use cs_app::cinematics::intro_timeline::{
    RecordView, TimelineError, measure_intro_timeline, variant_chains,
};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    AnimationRecords, ZbdFamily, ZbdProbe, dispatch, family_record, read_animation_index,
};
use cs_types::install::RelativePath;

// ---------------------------------------------------------------- authoring

fn event(
    tag_op: u8,
    group: u8,
    length: usize,
    words: &[(usize, u32)],
    name: Option<&str>,
) -> Vec<u8> {
    let mut out = vec![0_u8; length];
    out[0..4].copy_from_slice(&(u32::from(tag_op) | (u32::from(group) << 8)).to_le_bytes());
    out[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    for (at, value) in words {
        out[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
    }
    if let Some(name) = name {
        out[0xc..0xc + name.len()].copy_from_slice(name.as_bytes());
    }
    out
}

fn start(seconds: f32) -> (usize, u32) {
    (8, seconds.to_bits())
}

fn call_animation(group: u8, at: f32, name: &str) -> Vec<u8> {
    event(24, group, 0x50, &[start(at)], Some(name))
}

fn stop_animation(group: u8, at: f32, name: &str) -> Vec<u8> {
    event(25, group, 0x30, &[start(at)], Some(name))
}

fn call_sequence(name: &str) -> Vec<u8> {
    event(22, 1, 0x30, &[start(0.0)], Some(name))
}

fn if_node_active(node: u32) -> Vec<u8> {
    event(
        31,
        1,
        0x1c,
        &[start(0.0), (0xc, 0x2000), (0x10, node)],
        None,
    )
}

fn if_random(threshold: f32) -> Vec<u8> {
    event(
        31,
        1,
        0x1c,
        &[start(0.0), (0xc, 1), (0x14, threshold.to_bits())],
        None,
    )
}

fn motion(index_word: u32) -> Vec<u8> {
    event(12, 1, 0x4c, &[start(0.0), (0x14, index_word)], None)
}

fn else_() -> Vec<u8> {
    event(32, 1, 0xc, &[], None)
}

fn endif() -> Vec<u8> {
    event(34, 1, 0xc, &[], None)
}

fn stream(events: &[Vec<u8>]) -> Vec<u8> {
    events.concat()
}

/// A miniature of `generic_intro`: A for 5 s, then B for 7 s, then a branch on
/// node 0 that either drops at once or shows C for 3 s first.
fn authored_intro() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "start_script".into(),
            stream(&[
                call_animation(1, 0.0, "a"),
                stop_animation(1, 5.0, "a"),
                call_animation(1, 0.0, "b"),
                stop_animation(3, 7.0, "b"),
                call_sequence("check"),
            ]),
        ),
        (
            "check".into(),
            stream(&[
                if_node_active(0),
                call_sequence("drop"),
                else_(),
                call_animation(1, 0.0, "c"),
                stop_animation(3, 3.0, "c"),
                call_sequence("drop"),
                endif(),
            ]),
        ),
        (
            "drop".into(),
            stream(&[call_animation(1, 0.0, "d"), stop_animation(3, 2.0, "d")]),
        ),
    ]
}

fn view<'a>(sequences: &'a [(String, Vec<u8>)]) -> RecordView<'a> {
    RecordView {
        sequences: sequences
            .iter()
            .map(|(n, b)| (n.clone(), b.as_slice()))
            .collect(),
        nodes: vec!["player_x".into(), "other".into()],
        index_words: vec![10, 11, 12, 13],
    }
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

// ---------------------------------------------------------------- synthetic

#[test]
fn accept_parity_m01_lc_measure_cam_timeline_waits_follow_the_tag_group() {
    let sequences = authored_intro();
    // The branch not taken: the extra scene C plays between B and the drop.
    let long = measure_intro_timeline(&view(&sequences), &[]).expect("replays");
    let spans: Vec<(&str, f64, Option<f64>)> = long
        .scenes
        .iter()
        .map(|s| (s.animation.as_str(), s.started_at, s.stopped_at))
        .collect();
    assert_eq!(spans.len(), 4);
    assert_eq!(spans[0].0, "a");
    assert!(approx(spans[0].1, 0.0) && approx(spans[0].2.unwrap(), 5.0));
    // B starts when A stops (group-1 wait on the absolute clock) …
    assert!(approx(spans[1].1, 5.0));
    // … and stops 7 s after it started (group-3 wait: since the previous event).
    assert!(approx(spans[1].2.unwrap(), 12.0));
    assert_eq!(spans[2].0, "c");
    assert!(approx(spans[2].1, 12.0) && approx(spans[2].2.unwrap(), 15.0));
    assert!(approx(spans[3].1, 15.0) && approx(spans[3].2.unwrap(), 17.0));
    assert!(approx(long.length, 17.0));
    assert_eq!(long.sequences, ["start_script", "check", "drop"]);

    // The branch taken (node 0 active): C never plays.
    let short = measure_intro_timeline(&view(&sequences), &["player_x"]).expect("replays");
    assert!(short.scene("c").is_none());
    assert!(approx(short.scene("d").unwrap().started_at, 12.0));
    assert!(approx(short.length, 14.0));
    // A node the table does not list counts as not active.
    let unlisted = measure_intro_timeline(&view(&sequences), &["other"]).expect("replays");
    assert!(approx(unlisted.length, 17.0));
}

#[test]
fn accept_parity_m01_lc_measure_cam_timeline_refuses_what_it_has_not_measured() {
    let refuse = |sequences: Vec<(String, Vec<u8>)>| -> TimelineError {
        measure_intro_timeline(&view(&sequences), &[]).expect_err("must refuse")
    };
    let only = |events: &[Vec<u8>]| vec![("start_script".to_owned(), stream(events))];
    // A group-2 (sequence clock) wait.
    let error = refuse(only(&[call_animation(2, 1.0, "a")]));
    assert!(
        matches!(error, TimelineError::Unmeasured(ref m) if m.contains("group-2")),
        "{error}"
    );
    // A group-3 wait on the first event never fires in the engine.
    let error = refuse(only(&[call_animation(3, 1.0, "a")]));
    assert!(
        matches!(error, TimelineError::Unmeasured(ref m) if m.contains("first event")),
        "{error}"
    );
    // An event after CALL_SEQUENCE: the callee runs concurrently.
    let mut sequences = only(&[call_sequence("x"), call_animation(1, 0.0, "a")]);
    sequences.push(("x".into(), stream(&[call_animation(1, 0.0, "b")])));
    let error = refuse(sequences);
    assert!(
        matches!(error, TimelineError::Unmeasured(ref m) if m.contains("CALL_SEQUENCE")),
        "{error}"
    );
    // A random IF is not a node test.
    let error = refuse(only(&[if_random(0.5), endif()]));
    assert!(
        matches!(error, TimelineError::Unmeasured(ref m) if m.contains("condition")),
        "{error}"
    );
    // A call to a sequence that is not there.
    let error = refuse(only(&[call_sequence("nowhere")]));
    assert_eq!(error, TimelineError::NoSequence("nowhere".into()));
    // A stop of something never started.
    let error = refuse(only(&[stop_animation(1, 0.0, "ghost")]));
    assert!(
        matches!(error, TimelineError::Unmeasured(ref m) if m.contains("ghost")),
        "{error}"
    );
}

#[test]
fn accept_parity_m01_lc_measure_cam_timeline_variant_chain_probabilities() {
    let sequences = vec![
        (
            "head".to_owned(),
            stream(&[
                if_random(0.25),
                motion(0),
                else_(),
                call_sequence("second"),
                endif(),
            ]),
        ),
        (
            "second".to_owned(),
            stream(&[
                if_random(0.5),
                motion(1),
                else_(),
                call_sequence("third"),
                endif(),
            ]),
        ),
        ("third".to_owned(), stream(&[motion(2)])),
    ];
    let chains = variant_chains("scene", &view(&sequences)).expect("reads");
    assert_eq!(chains.len(), 1);
    let chain = &chains[0];
    assert_eq!(chain.thresholds, [0.25, 0.5]);
    assert_eq!(chain.track_blocks, [10, 11, 12]);
    let p = chain.probabilities();
    assert!(approx(p[0], 0.25) && approx(p[1], 0.375) && approx(p[2], 0.375));
    assert!(approx(p.iter().sum::<f64>(), 1.0));

    // Two links in one sequence (the shape of `gi_playerdrop`).
    let flat = vec![(
        String::new(),
        stream(&[if_random(0.3), motion(1), else_(), motion(2), endif()]),
    )];
    let chains = variant_chains("scene", &view(&flat)).expect("reads");
    assert_eq!(chains[0].track_blocks, [11, 12]);

    // An IF on anything but the random bit is refused, not skipped.
    let bad = vec![(
        "head".to_owned(),
        stream(&[if_random(0.3), motion(1), else_(), endif()]),
    )];
    assert!(variant_chains("scene", &view(&bad)).is_err());
}

// ------------------------------------------------------------------- retail

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    )
}

fn with_records<R>(f: impl FnOnce(&AnimationRecords<'_>) -> R) -> R {
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
    f(&records)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_measure_cam_timeline_retail_generic_intro_sequence() {
    with_records(|records| {
        let record = records
            .by_anim_name(b"generic_intro")
            .next()
            .expect("generic_intro");
        let intro = RecordView::from_record(record).expect("view");
        let nodes: Vec<&str> = intro.nodes.iter().map(String::as_str).collect();
        assert_eq!(nodes[5], "player_balmoral");
        assert_eq!(nodes[6], "player_warhawk");

        // The groups the finding's scheduler reading explains: the first
        // scene is stopped on the absolute clock, the others on the time
        // since the previous event.
        let (_, start_script) = intro
            .sequences
            .iter()
            .find(|(n, _)| n == "start_script")
            .expect("start_script");
        let tags: Vec<(u8, u8)> = walk_event_stream(start_script)
            .expect("walks")
            .iter()
            .map(|e| (e.opcode, e.group))
            .collect();
        assert!(
            tags.contains(&(25, 1)) && tags.contains(&(25, 3)),
            "{tags:?}"
        );

        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;

        // A player whose root is neither of the two: the long cinematic.
        let long = measure_intro_timeline(&intro, &[]).expect("replays");
        let span = |name: &str| long.scene(name).unwrap_or_else(|| panic!("{name}")).clone();
        assert!(near(span("gi_scene1").started_at, 0.0));
        assert!(near(span("gi_scene1").stopped_at.unwrap(), 15.9));
        assert!(near(span("gi_scene2").started_at, 15.9));
        assert!(near(span("gi_scene2").stopped_at.unwrap(), 28.3));
        assert!(near(span("gi_1stperson").started_at, 28.3));
        assert!(near(span("gi_1stperson").stopped_at.unwrap(), 38.2));
        assert!(near(span("gi_playerdrop").started_at, 38.2));
        assert!(near(span("gi_playerdrop").stopped_at.unwrap(), 40.15));
        assert!(near(long.length, 40.15));
        // The two ambient calls start with the cinematic and are never stopped
        // by it.
        for ambient in ["apzep_engines_start", "letterbox"] {
            assert!(near(span(ambient).started_at, 0.0));
            assert_eq!(span(ambient).stopped_at, None);
        }
        assert_eq!(
            long.sequences,
            [
                "start_script",
                "check_balmoral",
                "check_warhawk",
                "drop_planes"
            ]
        );

        // Balmoral and Warhawk skip the first-person shot.
        for root in ["player_balmoral", "player_warhawk"] {
            let short = measure_intro_timeline(&intro, &[root]).expect("replays");
            assert!(short.scene("gi_1stperson").is_none(), "{root}");
            assert!(near(short.scene("gi_playerdrop").unwrap().started_at, 28.3));
            assert!(near(short.length, 30.25), "{root}: {}", short.length);
        }
        // Any other airframe root is not listed in the record, so the
        // result is the long one.
        let other = measure_intro_timeline(&intro, &["player_pfighter"]).expect("replays");
        assert!(near(other.length, 40.15));
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_measure_cam_timeline_retail_variant_chains() {
    with_records(|records| {
        let chains_of = |name: &str| {
            let record = records
                .by_anim_name(name.as_bytes())
                .next()
                .unwrap_or_else(|| panic!("{name}"));
            variant_chains(name, &RecordView::from_record(record).expect("view"))
                .unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        let scene1 = chains_of("gi_scene1");
        assert_eq!(scene1.len(), 1);
        assert_eq!(scene1[0].thresholds, [0.2, 0.2, 0.2]);
        assert_eq!(scene1[0].track_blocks, [0, 1, 2, 3]);
        let p = scene1[0].probabilities();
        for (got, want) in p.iter().zip([0.2, 0.16, 0.128, 0.512]) {
            assert!((got - want).abs() < 1e-6, "{p:?}");
        }
        let scene2 = chains_of("gi_scene2");
        assert_eq!(scene2.len(), 1);
        assert_eq!(scene2[0].thresholds, [0.3, 0.3]);
        assert_eq!(scene2[0].track_blocks, [4, 5, 6]);
        let drop = chains_of("gi_playerdrop");
        assert_eq!(drop.len(), 1);
        assert_eq!(drop[0].thresholds, [0.3]);
        assert_eq!(drop[0].track_blocks, [11, 12]);
        // The first-person scene has no draw: `gi_cam3` always plays.
        assert!(chains_of("gi_1stperson").is_empty());
    });
}

// -------------------------------------------------------------------- image

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity_m01_lc_measure_cam_timeline_image_instructions_are_the_ones_the_finding_read() {
    let image = cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names it");
    assert_eq!(
        image.digest,
        cs_content::coordinates::original_image_digest()
    );
    let bytes = image.bytes.as_slice();
    let at = |va: usize| va - 0x40_0000;
    let is = |va: usize, expected: &[u8]| {
        assert_eq!(
            &bytes[at(va)..at(va) + expected.len()],
            expected,
            "bytes at {va:#x}"
        );
    };

    // The step function reads the event tag's second byte and compares the
    // event's START_TIME (`+8`) with a different clock for each value.
    is(
        0x4ecc04,
        &[0x8b, 0x46, 0x34, 0x33, 0xc9, 0x8a, 0x48, 0x01, 0x49],
    );
    is(
        0x4ecc3f,
        &[0xd9, 0x87, 0xb0, 0x00, 0x00, 0x00, 0xd8, 0x58, 0x08],
    ); // group 1: instance clock
    is(0x4ecc2c, &[0xd9, 0x46, 0x24, 0xd8, 0x58, 0x08]); // group 2: sequence clock
    is(0x4ecc19, &[0xd9, 0x46, 0x28, 0xd8, 0x58, 0x08]); // group 3: since the previous event
    // The instance update adds game_dt to the clock before it steps the
    // sequences, and a sequence in state 0 or 1 is stepped.
    is(0x4ecec0, &[0xd8, 0x86, 0xb0, 0x00, 0x00, 0x00]);
    is(
        0x4ecee8,
        &[0x8a, 0x47, 0x20, 0x84, 0xc0, 0x74, 0x04, 0x3c, 0x01, 0x75],
    );
    // game_dt is in seconds: the frame clock multiplies milliseconds by
    // 0.001 (`0x6075dc`) and is clamped to 0.125 s (`0x63b158`).
    is(0x6075dc, &0.001_f32.to_le_bytes());
    is(0x63b158, &0.125_f32.to_le_bytes());
    // The executors: CALL_SEQUENCE (22), CALL_ANIMATION (24), STOP_ANIMATION
    // (25) and IF (31) are installed in the slots the step function indexes
    // (`0x727de0 + 4 * opcode`).
    is(
        0x4ee279,
        &[0xc7, 0x05, 0x38, 0x7e, 0x72, 0x00, 0x70, 0xb5, 0x4e, 0x00],
    );
    is(
        0x4ee28d,
        &[0xc7, 0x05, 0x40, 0x7e, 0x72, 0x00, 0xb0, 0xb6, 0x4e, 0x00],
    );
    is(
        0x4ee297,
        &[0xc7, 0x05, 0x44, 0x7e, 0x72, 0x00, 0x80, 0xbc, 0x4e, 0x00],
    );
    is(
        0x4ee2ce,
        &[0xc7, 0x05, 0x5c, 0x7e, 0x72, 0x00, 0x80, 0xc0, 0x4e, 0x00],
    );
    // CALL_SEQUENCE turns a dormant (state 3) sequence into a running one.
    is(
        0x4eb5fa,
        &[
            0x8a, 0x4c, 0x10, 0x20, 0x03, 0xc2, 0x80, 0xf9, 0x03, 0x75, 0x04, 0xc6, 0x40, 0x20,
            0x00,
        ],
    );
    // CALL_ANIMATION waits only when event word `+0x2e` bit 0x10 is set.
    is(0x4eba0e, &[0xf6, 0x45, 0x2e, 0x10]);
    // IF condition 0x2000 reads the node's flag bit 0x4, which
    // OBJECT_ACTIVE_STATE sets (state 1) and clears (state 0).
    is(0x4ec1c8, &[0xf6, 0xc7, 0x20]);
    is(0x4ec1e5, &[0xf6, 0x40, 0x24, 0x04]);
    is(0x4ccaba, &[0x83, 0x4e, 0x24, 0x04]);
    is(0x4ccac7, &[0x83, 0x66, 0x24, 0xfb]);

    // The random table: filled from the CRT `rand` (the import slot the
    // fill loop calls through `ebx`), scaled by 1/32768, 200 entries, read
    // through one cursor that nothing resets.
    is(0x4ee393, &[0x8b, 0x1d, 0x50, 0x03, 0xa2, 0x00]);
    is(
        0x4ee410,
        &[0xc7, 0x05, 0x58, 0x83, 0x72, 0x00, 0x00, 0x01, 0x00, 0x38],
    );
    is(0x4ee41a, &[0xbe, 0x20, 0xce, 0x9f, 0x00, 0xff, 0xd3]);
    is(0x4ec0a1, &[0xa1, 0x6c, 0x83, 0x72, 0x00]);
    // The start-up seeds with `srand(time(NULL))`, unless `-seed N` follows,
    // and then fills the table (the call to the animation reset `0x5239b0`).
    is(
        0x4a742a,
        &[
            0x6a, 0x00, 0xff, 0x15, 0xb0, 0x01, 0xa2, 0x00, 0x50, 0xff, 0x15, 0xd8, 0x01, 0xa2,
            0x00,
        ],
    );
    is(0x629904, b"-seed\0");
    is(0x4a7662, &[0xff, 0x15, 0x48, 0x02, 0xa2, 0x00]); // atoi
    is(0x4a7669, &[0xff, 0x15, 0xd8, 0x01, 0xa2, 0x00]); // srand(atoi(..))
    is(0x4a7f2e, &[0xe8, 0x7d, 0xba, 0x07, 0x00]); // call 0x5239b0
    // The constant re-seed of `0x4df1d0` is followed by a re-seed with the time.
    is(0x4df1d7, &[0x68, 0x36, 0x18, 0xa9, 0x8e]);
    is(
        0x4df1e8,
        &[
            0x6a, 0x00, 0xff, 0x15, 0xb0, 0x01, 0xa2, 0x00, 0x50, 0xff, 0xd6,
        ],
    );
}
