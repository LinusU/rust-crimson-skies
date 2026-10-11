//! Acceptance scenario F14-D.9 (follow-up task #564 of F14-D.6 / #489): the
//! mission-only airframes the per-chapter load scripts declare.
//!
//! **Question.** The `airframe` collection holds the eleven shared airframes
//! `support\planes.gw` declares. A mission-only airframe would live in a
//! per-chapter `gamez.zbd`; do the per-chapter load scripts
//! (`LoadGameGen <model> <alias>`) declare one?
//!
//! **Measured answer: no.** Eight scripts store 408 `LoadGameGen` lines in one
//! command shape; 8 spell a model the shared roster already declares and 400
//! have no shared-roster equal. Nothing stored separates an airframe from a
//! world prop, and the aliases are bare load names, so a model name or an alias
//! is not an airframe (F11 non-negotiable behavior 3). The baseline inventory
//! therefore adds **no** row, and records what it read on the `airframe`
//! collection's `gaps` so "read and found nothing" differs from "not read". See
//! `docs/findings/2026-10-11-f14-d-9-mission-only-airframes.md`.
//!
//! The non-retail tests write synthetic installation trees authored for this
//! file. The retail test is `#[ignore = "requires CS_GAME_DIR"]`.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    AIRFRAME_SCRIPT_IMAGE, Baseline, CHAPTER_LOAD_SCRIPTS, CollectionStatus, retail_baseline,
};
use cs_formats::zbd::{
    INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES, TRAILER_VERSION_ONE,
};
use cs_types::content::ContentKind;

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-6-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture directory is created");
        Self(root)
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("a parent directory"))
            .expect("the fixture directory is created");
        fs::write(&path, bytes).expect("the fixture file is written");
    }
}

impl Drop for TempInstall {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One version-one reader archive whose member index lists `names` (each member
/// holds a few filler bytes), written exactly as the pinned reader expects.
fn reader_archive(names: &[&str]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for name in names {
        let start = u32::try_from(data.len()).expect("fits");
        entries.extend_from_slice(&start.to_le_bytes());
        entries.extend_from_slice(&4u32.to_le_bytes());
        let mut field = vec![0u8; INDEX_NAME_BYTES];
        field[..name.len()].copy_from_slice(name.as_bytes());
        entries.extend_from_slice(&field);
        entries.extend_from_slice(&[0xA5; INDEX_UNEXPLAINED_BYTES]);
        data.extend_from_slice(b"zrd\0");
    }
    assert_eq!(entries.len(), names.len() * INDEX_ENTRY_BYTES as usize);
    data.extend_from_slice(&entries);
    data.extend_from_slice(&TRAILER_VERSION_ONE.to_le_bytes());
    data.extend_from_slice(&u32::try_from(names.len()).expect("fits").to_le_bytes());
    data
}

/// One authored line: NUL-terminated arguments with their declared count,
/// which is the shape the INTERP decoder splits tokens on and rejoins.
fn line(tokens: &[&[u8]]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut count = 0u32;
    for token in tokens {
        data.extend_from_slice(token);
        data.push(0);
        count += 1;
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&data);
    bytes
}

/// An authored INTERP container built from `(script name, lines)`, recording the
/// absolute offset of every line so a row's span can be located in the bytes
/// rather than trusted.
fn interp_container(fixtures: &[(&[u8], Vec<Vec<u8>>)]) -> (Vec<u8>, Vec<u64>) {
    use cs_formats::interp::{INDEX_ENTRY_BYTES, INTERP_HEADER_BYTES, NAME_FIELD_BYTES};

    let body_start = (INTERP_HEADER_BYTES + fixtures.len() * INDEX_ENTRY_BYTES) as u64;
    let mut body = Vec::new();
    let mut line_offsets = Vec::new();
    let mut script_offsets = Vec::new();
    for (_, lines) in fixtures {
        script_offsets.push((body_start + body.len() as u64) as u32);
        for data in lines {
            line_offsets.push(body_start + body.len() as u64);
            body.extend_from_slice(data);
        }
        body.extend_from_slice(&0u32.to_le_bytes());
    }
    let mut bytes = Vec::new();
    for word in [0x0897_1119u32, 7, fixtures.len() as u32] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for ((name, _), offset) in fixtures.iter().zip(&script_offsets) {
        let mut field = [0u8; NAME_FIELD_BYTES];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&1_000u32.to_le_bytes());
        bytes.extend_from_slice(&offset.to_le_bytes());
    }
    bytes.extend_from_slice(&body);
    (bytes, line_offsets)
}

/// The authored campaign tree the F14-D.3 tests write: one campaign mission, its
/// mission animation archive, and the world group's shared reader.
fn campaign_tree(label: &str) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write(
        "ZBD/C1C/M01/zrdr.zbd",
        &reader_archive(&["net.zrd", "map.zrd", "aiv.zrd", "objectives.zrd"]),
    );
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
    temp.write(
        "ZBD/C1C/zrdr.zbd",
        &reader_archive(&["templates.zrd", "cam_anim.zrd", "landings.zrd"]),
    );
    temp
}

/// The fixture container: the shared declaring idiom for one root plus the given
/// per-chapter scripts.
fn container_with_chapters(chapters: &[(&[u8], Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut fixtures: Vec<(&[u8], Vec<Vec<u8>>)> = vec![
        (
            b"support\\init.gw",
            vec![line(&[b"set", b"ZBD_DIR", b"zbd"])],
        ),
        (
            b"support\\planes.gw",
            vec![
                line(&[b"source", b"support\\init.gw"]),
                line(&[b"set", b"ZBDFile", b"%ZBD_DIR%\\planes.zbd"]),
                line(&[
                    b"set",
                    b"planeInput",
                    b"common\\planes\\fixture\\plane0.flt",
                ]),
                line(&[b"set", b"planeOutput", b"player_first"]),
                line(&[b"source", b"support\\util\\surgery.gw"]),
                line(&[b"GameZWriteZBDFile", b"%ZBDFile%"]),
            ],
        ),
        (
            b"support\\util\\surgery.gw",
            vec![
                line(&[b"LoadGameGen", b"%planeInput%", b"fixture0001"]),
                line(&[b"NewObject3D", b"%planeOutput%"]),
            ],
        ),
    ];
    fixtures.extend(chapters.iter().map(|(name, lines)| (*name, lines.clone())));
    interp_container(&fixtures).0
}

fn airframe_status(baseline: &Baseline) -> &CollectionStatus {
    baseline
        .collection_status
        .iter()
        .find(|status| status.kind == ContentKind::Airframe)
        .expect("the airframe collection reports its status")
}

fn airframe_keys(baseline: &Baseline) -> Vec<String> {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == ContentKind::Airframe)
        .map(|row| row.id.key().to_owned())
        .collect()
}

fn gap(status: &CollectionStatus, label: &str) -> usize {
    status.gaps.get(label).copied().unwrap_or(0)
}

/// Chapter loads are counted and attributed, and add no airframe row: a model
/// the shared roster declares is another instance of it, and an alias that
/// names a scene root (`NewObject3D`) or a prop is not an airframe.
#[test]
fn accept_f14_d_9_chapter_loads_are_counted_and_add_no_airframe_row() {
    let temp = campaign_tree("chapter-loads");
    let shared_model: &[u8] = b"common\\planes\\fixture\\plane0.flt";
    let chapters: Vec<(&[u8], Vec<Vec<u8>>)> = vec![
        (
            b"support\\c1\\load.gw",
            vec![
                line(&[b"LoadGameGen", b"common\\objects\\zeppelin.flt", b"zep_1"]),
                line(&[b"LoadGameGen", shared_model, b"britfighter_1"]),
                line(&[b"NewObject3D", b"mission_root"]),
            ],
        ),
        (
            b"support\\c2b\\load.gw",
            vec![line(&[
                b"LoadGameGen",
                b"common\\objects\\flag.flt",
                b"flag_1",
            ])],
        ),
    ];
    temp.write(AIRFRAME_SCRIPT_IMAGE, &container_with_chapters(&chapters));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert_eq!(
        airframe_keys(&baseline),
        vec!["player_first".to_owned()],
        "only the shared declaration is a row; no chapter alias became an airframe"
    );
    let status = airframe_status(&baseline);
    assert_eq!(status.rows, 1);
    assert_eq!(gap(status, "chapter_load"), 3);
    assert_eq!(gap(status, "chapter_load_shared"), 1);
    assert_eq!(gap(status, "chapter_load_unclassified"), 2);
    assert_eq!(gap(status, "mission_only_airframe"), 0);
    assert_eq!(gap(status, "mission_only_unknown"), 1);
    assert_eq!(
        gap(status, "chapter_script_absent"),
        CHAPTER_LOAD_SCRIPTS.len() - 2,
        "the six chapter scripts this fixture lacks are named, not skipped"
    );
}

/// A chapter load line stored with an argument count the idiom does not spell is
/// a finding, not a silently joined value.
#[test]
fn accept_f14_d_9_an_unreadable_chapter_load_is_a_finding() {
    let temp = campaign_tree("chapter-unreadable");
    let chapters: Vec<(&[u8], Vec<Vec<u8>>)> = vec![(
        b"support\\c3\\load.gw",
        vec![
            line(&[b"LoadGameGen", b"lonely.flt"]),
            line(&[b"LoadGameGen", b"a.flt", b"a_1"]),
        ],
    )];
    temp.write(AIRFRAME_SCRIPT_IMAGE, &container_with_chapters(&chapters));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let status = airframe_status(&baseline);
    assert_eq!(gap(status, "chapter_line_unreadable"), 1);
    assert_eq!(gap(status, "chapter_load"), 1);
    assert_eq!(airframe_keys(&baseline), vec!["player_first".to_owned()]);
}

/// Airframes are not launchable: discovering chapter loads does not move the
/// coverage denominator.
#[test]
fn accept_f14_d_9_the_denominator_does_not_move() {
    let temp = campaign_tree("chapter-denominator");
    let with = vec![(
        b"support\\c1\\load.gw" as &[u8],
        vec![line(&[b"LoadGameGen", b"a.flt", b"a_1"])],
    )];
    temp.write(AIRFRAME_SCRIPT_IMAGE, &container_with_chapters(&with));
    let with_loads = retail_baseline(&temp.0).expect("reads");
    temp.write(AIRFRAME_SCRIPT_IMAGE, &container_with_chapters(&[]));
    let without = retail_baseline(&temp.0).expect("reads");
    assert_eq!(with_loads.roots, without.roots);
    assert_eq!(with_loads.coverage.roots, without.coverage.roots);
    assert_eq!(
        with_loads.catalog.launchable_count(),
        without.catalog.launchable_count()
    );
    assert!(!ContentKind::Airframe.is_launchable());
}

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The retail half: the eight per-chapter scripts store 408 loads, 8 of them
/// spell a shared-roster model, 400 are unclassified, and no mission-only
/// airframe row exists. The eleven shared rows are unchanged.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_9_retail_the_chapter_loads_declare_no_mission_only_airframe() {
    let baseline = retail_baseline(&game_dir()).expect("the original installation reads");
    let status = airframe_status(&baseline);
    assert_eq!(airframe_keys(&baseline).len(), 11, "only the shared roster");
    assert_eq!(status.rows, 11);
    assert_eq!(gap(status, "chapter_load"), 408);
    assert_eq!(gap(status, "chapter_load_shared"), 8);
    assert_eq!(gap(status, "chapter_load_unclassified"), 400);
    assert_eq!(gap(status, "mission_only_airframe"), 0);
    assert_eq!(gap(status, "mission_only_unknown"), 1);
    for label in [
        "chapter_script_absent",
        "chapter_script_ambiguous",
        "chapter_line_unreadable",
    ] {
        assert_eq!(gap(status, label), 0, "{label}: every chapter line reads");
    }
    assert_eq!(
        baseline
            .coverage
            .unreachable_by_kind
            .get("airframe")
            .copied(),
        Some(11)
    );
    assert!(!ContentKind::Airframe.is_launchable());
}
