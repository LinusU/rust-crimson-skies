//! Acceptance for task #1662 (`PARITY-M01-LC-MEASURE-CAM`): where the M01
//! launch cinematic's camera pose comes from
//! (`docs/findings/2026-10-18-parity-m01-lc-cinematic-camera-pose-source.md`).
//!
//! It comes from the region after the last animation record of
//! `ZBD/<group>/cam_anim.zbd`, which task #650 left unwalked: compiled `.zan`
//! motion tracks, read by `cs_formats::zbd::anim_tracks`. The synthetic tests
//! author every byte they read. The retail tests read `$CS_GAME_DIR` (never
//! write it) and fail loudly without it.

use cs_formats::ParseContext;
use cs_formats::zbd::anim_tracks::{
    AnimationTracks, TRACK_DESCRIPTOR_BYTES, TRACK_PART_BYTES, TrackError, read_animation_tracks,
};
use cs_formats::zbd::{
    AnimationRecordTableKind, ZbdFamily, ZbdProbe, dispatch, family_record, read_animation_index,
};
use cs_types::install::RelativePath;

use super::t340::game_dir;

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// One authored part: `words[i] = base + i`, except the cubic of channel 0.
fn part(base: f32) -> Vec<u8> {
    let mut out = Vec::new();
    for index in 0..19 {
        put_f32(&mut out, base + index as f32);
    }
    out
}

fn segment(flags: u32, start: f32, end: f32) -> Vec<u8> {
    let mut out = Vec::new();
    put_u32(&mut out, flags);
    put_f32(&mut out, start);
    put_f32(&mut out, end);
    for (bit, base) in [(1, 100.0), (2, 200.0), (4, 300.0)] {
        if flags & bit != 0 {
            out.extend(part(base));
        }
    }
    out
}

/// A region of `blocks` = (path, object, mode, segments).
fn region(blocks: &[(&str, &str, u32, Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (_, _, mode, segments) in blocks {
        let data: usize = segments.iter().map(Vec::len).sum();
        for pointer in [0x045d_9210u32, 0x045d_a460] {
            put_u32(&mut out, pointer);
        }
        put_u32(&mut out, 0x82e);
        put_u32(&mut out, *mode);
        put_u32(&mut out, segments.len() as u32);
        put_u32(&mut out, data as u32);
        put_u32(&mut out, 0x03e1_f810);
    }
    for (path, object, _, segments) in blocks {
        out.extend_from_slice(path.as_bytes());
        out.push(0);
        out.extend_from_slice(object.as_bytes());
        out.push(0);
        for segment in segments {
            out.extend_from_slice(segment);
        }
    }
    out
}

fn two_blocks() -> Vec<u8> {
    region(&[
        (
            "..\\data\\common\\zrdr\\x\\a.zan",
            "camera1",
            1,
            vec![segment(3, 0.0, 0.5), segment(2, 0.5, 1.0)],
        ),
        (
            "..\\data\\common\\zrdr\\x\\b.zan",
            "player",
            0,
            vec![segment(7, 0.0, 2.0)],
        ),
    ])
}

#[test]
fn accept_parity_m01_lc_measure_cam_reads_authored_tracks() {
    let bytes = two_blocks();
    let tracks = read_animation_tracks(&bytes, 2).expect("the authored region tiles");
    assert_eq!(tracks.blocks().len(), 2);
    assert_eq!(tracks.segment_count(), 3);

    let first = tracks.block(0).expect("block 0");
    assert_eq!(first.index(), 0);
    assert_eq!(first.type_word(), 0x82e);
    assert_eq!(first.mode(), 1);
    assert_eq!(first.path(), b"..\\data\\common\\zrdr\\x\\a.zan");
    assert_eq!(first.object_name(), b"camera1");
    assert_eq!(first.duration(), 1.0);
    let [full, rotation_only] = first.segments() else {
        panic!("two segments")
    };
    assert_eq!((full.start(), full.end(), full.flags()), (0.0, 0.5, 3));
    assert!(full.position().is_some() && full.rotation().is_some() && full.scale().is_none());
    assert!(rotation_only.position().is_none() && rotation_only.rotation().is_some());

    let position = full.position().expect("position part");
    assert_eq!(position.start_vector(), [100.0, 101.0, 102.0]);
    assert_eq!(position.linear_rate(), [104.0, 105.0, 106.0]);
    assert_eq!(position.cubic(0), [107.0, 108.0, 109.0, 110.0]);
    assert_eq!(position.cubic(2), [115.0, 116.0, 117.0, 118.0]);
    // a0 + u (a1 + u (a2 + u a3)) at u = 2: 107 + 2 (108 + 2 (109 + 2 * 110)).
    assert_eq!(
        position.cubic_at(0, 2.0),
        107.0 + 2.0 * (108.0 + 2.0 * (109.0 + 2.0 * 110.0))
    );
    assert_eq!(
        full.rotation().expect("rotation part").start_quaternion(),
        [200.0, 201.0, 202.0, 203.0]
    );

    let second = tracks.block(1).expect("block 1");
    assert_eq!(second.mode(), 0);
    assert_eq!(second.object_name(), b"player");
    let all = &second.segments()[0];
    assert!(all.position().is_some() && all.rotation().is_some() && all.scale().is_some());
    assert_eq!(all.scale().expect("scale part").words()[0], 300.0);
}

fn refusal(bytes: &[u8], rows: usize) -> TrackError {
    read_animation_tracks(bytes, rows).expect_err("the region must be refused")
}

#[test]
fn accept_parity_m01_lc_measure_cam_refuses_regions_that_do_not_tile() {
    let good = two_blocks();

    // Fewer bytes than the descriptor table.
    assert_eq!(
        refusal(&good[..TRACK_DESCRIPTOR_BYTES], 2).code(),
        "track_descriptors_truncated"
    );
    // A region that stops inside a name.
    assert_eq!(
        refusal(&good[..2 * TRACK_DESCRIPTOR_BYTES + 5], 2).code(),
        "track_name_unterminated"
    );
    // A region that stops inside a block's data.
    assert_eq!(
        refusal(&good[..good.len() - 4], 2).code(),
        "track_data_truncated"
    );
    // Bytes after the last block.
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(refusal(&longer, 2).code(), "track_trailing_bytes");
    // A flag above the scale bit.
    let bad_flags = region(&[("p", "o", 1, vec![segment(8, 0.0, 1.0)])]);
    assert_eq!(refusal(&bad_flags, 1).code(), "track_unknown_flags");

    // A descriptor that declares more segments than the data holds.
    let mut lying = region(&[("p", "o", 1, vec![segment(1, 0.0, 1.0)])]);
    lying[16..20].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(refusal(&lying, 1).code(), "track_segments_disagree");

    // A segment whose part runs past the block's data.
    let mut short = region(&[("p", "o", 1, vec![segment(1, 0.0, 1.0)])]);
    short[20..24].copy_from_slice(&((12 + TRACK_PART_BYTES - 4) as u32).to_le_bytes());
    let cut = short.len() - 4;
    assert_eq!(refusal(&short[..cut], 1).code(), "track_segment_truncated");
}

fn normalize(path: &[u8]) -> String {
    String::from_utf8_lossy(path)
        .replace("\\\\", "\\")
        .to_ascii_lowercase()
}

/// An animation container read from the installation: the tracks, and every
/// member path.
struct Container {
    bytes: Vec<u8>,
}

impl Container {
    fn open(spelling: &str) -> Self {
        let path = game_dir().join(spelling);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{spelling}: cannot read the container: {error}"));
        Self { bytes }
    }

    fn with<R>(
        &self,
        spelling: &str,
        f: impl FnOnce(
            &cs_formats::zbd::AnimationIndex<'_>,
            &cs_formats::zbd::AnimationRecords<'_>,
        ) -> R,
    ) -> R {
        let relative = RelativePath::new(spelling).expect("a relative path");
        let key = relative.logical_key();
        let mut context = ParseContext::with_defaults(&key);
        let needed = family_record(ZbdFamily::Animation)
            .header_rule()
            .signature()
            .map_or(0, |rule| rule.required_bytes())
            .min(self.bytes.len());
        let decision = dispatch(ZbdProbe::new(&key, &relative, &self.bytes[..needed]))
            .unwrap_or_else(|error| panic!("{spelling}: dispatch refused: {error}"));
        let index = read_animation_index(&mut context, decision, &self.bytes)
            .unwrap_or_else(|error| panic!("{spelling}: index refused: {error}"));
        let payload = index
            .payload()
            .unwrap_or_else(|error| panic!("{spelling}: payload refused: {error}"));
        let records = payload
            .records()
            .unwrap_or_else(|error| panic!("{spelling}: records refused: {error}"));
        f(&index, &records)
    }
}

fn zan_rows(index: &cs_formats::zbd::AnimationIndex<'_>) -> Vec<String> {
    index
        .members()
        .iter()
        .filter(|row| row.path().to_ascii_lowercase().ends_with(b".zan"))
        .map(|row| normalize(row.path()))
        .collect()
}

fn tracks_of<'a>(
    spelling: &str,
    index: &cs_formats::zbd::AnimationIndex<'_>,
    records: &cs_formats::zbd::AnimationRecords<'a>,
) -> Option<AnimationTracks<'a>> {
    let region = records.trailing();
    if region.is_empty() {
        assert!(
            zan_rows(index).is_empty(),
            "{spelling}: .zan rows but no tail"
        );
        return None;
    }
    Some(
        read_animation_tracks(region, zan_rows(index).len())
            .unwrap_or_else(|error| panic!("{spelling}: tracks refused: {error}")),
    )
}

fn retail_carriers() -> Vec<String> {
    let root = game_dir().join("ZBD");
    let mut out = Vec::new();
    let mut groups: Vec<_> = std::fs::read_dir(&root)
        .expect("ZBD directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.is_dir())
        .collect();
    groups.sort();
    for group in groups {
        let name = group
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        if group.join("cam_anim.zbd").is_file() {
            out.push(format!("ZBD/{name}/cam_anim.zbd"));
        }
        let mut missions: Vec<_> = std::fs::read_dir(&group)
            .expect("group directory")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| path.join("mis_anim.zbd").is_file())
            .collect();
        missions.sort();
        for mission in missions {
            let mission_name = mission
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            out.push(format!("ZBD/{name}/{mission_name}/mis_anim.zbd"));
        }
    }
    out
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_measure_cam_retail_every_tail_is_tracks() {
    let carriers = retail_carriers();
    assert_eq!(
        carriers.len(),
        61,
        "the installation's animation containers"
    );
    let mut with_tail = 0usize;
    let (mut blocks, mut segments, mut linear) = (0usize, 0usize, 0usize);
    let mut worst_unit_error = 0.0f32;
    for spelling in &carriers {
        let container = Container::open(spelling);
        container.with(spelling, |index, records| {
            let Some(tracks) = tracks_of(spelling, index, records) else {
                return;
            };
            with_tail += 1;
            let rows = zan_rows(index);
            assert_eq!(
                tracks.blocks().len(),
                rows.len(),
                "{spelling}: one block per .zan row"
            );
            for (block, row) in tracks.blocks().iter().zip(&rows) {
                assert_eq!(
                    &normalize(block.path()),
                    row,
                    "{spelling}: block {} repeats its member row's path",
                    block.index()
                );
                blocks += 1;
                linear += usize::from(block.mode() == 0);
                for segment in block.segments() {
                    segments += 1;
                    if let Some(rotation) = segment.rotation() {
                        let [w, x, y, z] = rotation.start_quaternion();
                        worst_unit_error = worst_unit_error
                            .max(((w * w + x * x + y * y + z * z).sqrt() - 1.0).abs());
                    }
                }
            }
        });
    }
    assert_eq!(with_tail, 31, "containers with a tail");
    assert_eq!(blocks, 1090, "track blocks");
    assert_eq!(segments, 76_845, "segments");
    assert_eq!(linear, 15, "blocks whose mode word is zero");
    assert!(
        worst_unit_error < 1e-6,
        "rotation quaternions are unit length: {worst_unit_error}"
    );
}

const GENERIC_INTRO: [(&str, &str, usize, f32); 14] = [
    ("gi_cam1.zan", "camera1", 48, 15.933_334),
    ("gi_cam1b.zan", "camera1", 24, 15.933_334),
    ("gi_cam1c.zan", "camera1", 24, 15.933_334),
    ("gi_cam1d.zan", "camera1", 24, 15.933_334),
    ("gi_cam2.zan", "camera1", 20, 12.4375),
    ("gi_cam2b.zan", "camera1", 20, 12.4375),
    ("gi_cam2c.zan", "camera1", 20, 12.4375),
    ("gi_pfighter1.zan", "piratefighter", 20, 12.4375),
    ("gi_player1.zan", "player", 67, 9.95),
    ("gi_cam3.zan", "camera1", 40, 9.95),
    ("gi_player2.zan", "player", 12, 1.966_666_8),
    ("gi_cam4.zan", "camera1", 12, 1.966_666_8),
    ("gi_cam4b.zan", "camera1", 6, 1.966_666_8),
    ("gi_pfighter2.zan", "piratefighter", 12, 1.966_666_8),
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_measure_cam_retail_c3_generic_intro_tracks() {
    let spelling = "ZBD/C3/cam_anim.zbd";
    let container = Container::open(spelling);
    container.with(spelling, |index, records| {
        let tracks = tracks_of(spelling, index, records).expect("C3 has a tail");
        assert_eq!(records.trailing().len(), 405_135);
        for (k, (file, object, count, duration)) in GENERIC_INTRO.iter().enumerate() {
            let block = tracks.block(k).expect("block");
            let path = normalize(block.path());
            assert!(
                path.ends_with(&format!("\\generic_intro\\{file}")),
                "block {k} is {path}"
            );
            assert_eq!(block.object_name(), object.as_bytes(), "{file}");
            assert_eq!(block.segments().len(), *count, "{file}: segment count");
            assert!(
                (block.duration() - duration).abs() < 1e-5,
                "{file}: {}",
                block.duration()
            );
            assert_eq!(block.mode(), 1, "{file}: cubic mode");
            // Contiguous in time.
            for pair in block.segments().windows(2) {
                assert_eq!(pair[0].end(), pair[1].start(), "{file}");
            }
        }

        // The camera path of the first variant: the cubic of each segment,
        // evaluated at the segment's length, is the next segment's start.
        let cam1 = tracks.block(0).expect("gi_cam1");
        let first = cam1.segments()[0].position().expect("position");
        assert_eq!(first.start_vector(), [-155.705_87, -62.025_87, -768.973_2]);
        let mut worst = 0.0f32;
        for pair in cam1.segments().windows(2) {
            let length = pair[0].end() - pair[0].start();
            let from = pair[0].position().expect("position");
            let to = pair[1].position().expect("position").start_vector();
            for (channel, expected) in to.iter().enumerate() {
                worst = worst.max((from.cubic_at(channel, length) - expected).abs());
            }
        }
        assert!(worst < 1e-3, "cubic continuity: {worst}");

        // The fourteen intro tracks are the same in all eight world groups.
        for group in ["C1", "C1B", "C1C", "C2", "C2B", "C4", "C5"] {
            let other_spelling = format!("ZBD/{group}/cam_anim.zbd");
            Container::open(&other_spelling).with(&other_spelling, |other_index, other_records| {
                let other = tracks_of(&other_spelling, other_index, other_records)
                    .expect("every camera carrier has a tail");
                for k in 0..GENERIC_INTRO.len() {
                    let (here, there) =
                        (tracks.block(k).expect("C3"), other.block(k).expect("other"));
                    // Word 3 of a position part is padding the engine does
                    // not read; it differs between carriers, nothing else does.
                    let essential = |block: &cs_formats::zbd::anim_tracks::TrackBlock<'_>| {
                        block
                            .segments()
                            .iter()
                            .map(|segment| {
                                let position = segment.position().map(|part| {
                                    let mut words = *part.words();
                                    words[3] = 0.0;
                                    words
                                });
                                (
                                    segment.flags(),
                                    segment.start(),
                                    segment.end(),
                                    position,
                                    segment.rotation().map(|part| *part.words()),
                                    segment.scale().map(|part| *part.words()),
                                )
                            })
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(
                        essential(here),
                        essential(there),
                        "{other_spelling}: block {k}"
                    );
                    assert_eq!(here.object_name(), there.object_name());
                }
            });
        }

        // The join: a scene's INDEX_WORDS table names the tracks its
        // OBJECT_MOTION_SI_SCRIPT events play.
        let scenes: [(&str, &[u32]); 4] = [
            ("gi_scene1", &[0, 1, 2, 3]),
            ("gi_scene2", &[4, 5, 6]),
            ("gi_1stperson", &[8, 9]),
            ("gi_playerdrop", &[10, 11, 12]),
        ];
        for (scene, expected) in scenes {
            let record = records
                .by_anim_name(scene.as_bytes())
                .next()
                .unwrap_or_else(|| panic!("record {scene}"));
            let table = record
                .table(AnimationRecordTableKind::IndexWords)
                .expect("index words");
            let words: Vec<u32> = (0..table.count())
                .map(|k| {
                    u32::from_le_bytes(table.entry(k).expect("entry")[..4].try_into().unwrap())
                })
                .collect();
            // Entry 0 of the table is the empty sentinel for some tables; the
            // measured tables here start at the first index.
            assert_eq!(words, expected, "{scene}: index words");
            for word in &words {
                let block = tracks.block(*word as usize).expect("a track block");
                let named = normalize(block.path());
                assert!(named.contains("generic_intro"), "{scene}: {named}");
            }
        }
    });
}
