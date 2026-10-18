//! F49-D.01: the Instant Action scenario and preset denominator, measured from
//! the installation's own `ia.zrd` descriptors by
//! `cs_content::instant_action::survey_retail_instant_action`.
//!
//! The synthetic tests build version-one reader archives in a temporary tree
//! and drive the production survey over it; the retail test re-measures the
//! owner's installation and cross-checks the directory count against an
//! independent `std::fs` walk.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::instant_action::{
    IA_MISSION_TYPE_VOCABULARY, IaPlayerPlane, IaSurveyError, IaWingmen,
    survey_retail_instant_action,
};

pub(crate) struct TempInstall(pub(crate) PathBuf);

impl TempInstall {
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f49-d-01-{label}-{}-{}",
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

fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn zrd_float(value: f32) -> Vec<u8> {
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A list node; the stored count is one more than the child count.
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = 4_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::new();
    for (name, member) in members {
        entries.push((bytes.len() as u32, member.len() as u32, *name));
        bytes.extend_from_slice(member);
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name.as_bytes());
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

fn spawn_list(count: usize) -> Vec<u8> {
    zrd_list(
        (0..count)
            .map(|_| {
                zrd_list(vec![
                    zrd_float(1.0),
                    zrd_float(2.0),
                    zrd_float(3.0),
                    zrd_float(4.0),
                ])
            })
            .collect(),
    )
}

/// An `ia.zrd` root: spawn points per offered type, optional disallow list,
/// default mission type and an optional player plane. A document without a
/// player plane also leaves out `num_wingmen`, like `ZBD/c2b/IA1`.
pub(crate) fn ia_document(
    offered: &[(&str, usize)],
    disallow: Option<&[&str]>,
    default: &str,
    plane: Option<&str>,
) -> Vec<u8> {
    let mut children = vec![zrd_text("mission_type"), zrd_list(vec![zrd_text(default)])];
    if let Some(plane) = plane {
        children.push(zrd_text("player_plane"));
        children.push(zrd_list(vec![zrd_text(plane)]));
        children.push(zrd_text("num_wingmen"));
        children.push(zrd_list(vec![zrd_int(3)]));
    }
    let mut spawn = Vec::new();
    for (name, count) in offered {
        spawn.push(zrd_text(name));
        spawn.push(spawn_list(*count));
    }
    children.push(zrd_text("spawn_points"));
    children.push(zrd_list(spawn));
    if let Some(disallow) = disallow {
        children.push(zrd_text("disallow_missions"));
        children.push(zrd_list(disallow.iter().map(|n| zrd_text(n)).collect()));
    }
    zrd_list(children)
}

/// An `ia.zrd` root with no `spawn_points` at all.
fn ia_without_spawn_points() -> Vec<u8> {
    zrd_list(vec![
        zrd_text("mission_type"),
        zrd_list(vec![zrd_text("dogfight_ace")]),
    ])
}

pub(crate) fn add_directory(temp: &TempInstall, group: &str, leaf: &str, ia: Vec<u8>) {
    temp.write(
        &format!("ZBD/{group}/{leaf}/zrdr.zbd"),
        &reader_archive(&[("ia.zrd", ia), ("map.zrd", zrd_int(1))]),
    );
    temp.write(&format!("ZBD/{group}/{leaf}/mis_anim.zbd"), b"animation");
}

pub(crate) fn campaign_tree(label: &str) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write(
        "ZBD/C1/M01/zrdr.zbd",
        &reader_archive(&[("net.zrd", zrd_int(1)), ("map.zrd", zrd_int(1))]),
    );
    temp.write("ZBD/C1/M01/mis_anim.zbd", b"mission animation");
    temp
}

#[test]
fn accept_f49_d_01_synthetic_directory_is_read_with_offered_disallowed_and_roster() {
    let temp = campaign_tree("read");
    add_directory(
        &temp,
        "C1",
        "IA1",
        ia_document(
            &[("dogfight_ace", 2), ("stunt_flying", 3)],
            Some(&["ground_target", "zeppelin_run"]),
            "stunt_flying",
            Some("Kestrel"),
        ),
    );
    add_directory(
        &temp,
        "C1",
        "IA2",
        ia_document(&[("dogfight_ace", 1)], None, "dogfight_ace", None),
    );
    let survey = survey_retail_instant_action(&temp.0).expect("the survey reads");
    assert_eq!(survey.directories().len(), 2);
    let first = &survey.directories()[0];
    assert_eq!(first.world_group(), "c1");
    assert_eq!(first.leaf(), "ia1");
    assert_eq!(first.container(), "zbd/c1/ia1/zrdr.zbd");
    assert_eq!(first.container_sha256().len(), 64);
    assert!(first.member_span().1 > 0);
    let offered: Vec<_> = first
        .offered()
        .iter()
        .map(|o| (o.mission_type(), o.spawn_points()))
        .collect();
    assert_eq!(offered, [("dogfight_ace", 2), ("stunt_flying", 3)]);
    assert_eq!(first.disallowed(), ["ground_target", "zeppelin_run"]);
    assert_eq!(first.default_mission_type(), Some("stunt_flying"));
    assert_eq!(first.aircraft().wingmen(), Some(3));
    assert_eq!(
        first.player_plane(),
        IaPlayerPlane::Declared("Kestrel".into())
    );
    let second = &survey.directories()[1];
    assert_eq!(
        second.player_plane(),
        IaPlayerPlane::EngineDefault { index: 5 }
    );
    assert_eq!(second.wingmen(), IaWingmen::EngineDefault(0));
    assert_eq!(first.wingmen(), IaWingmen::Declared(3));
    let ids = survey.preset_ids().expect("ids are valid");
    let presets: BTreeSet<_> = ids.iter().map(|(p, _)| p.to_string()).collect();
    assert_eq!(presets.len(), 3, "2 + 1 offered types, all unique");
    assert!(presets.contains("ia_preset/c1-ia1-stunt_flying"));
    assert_eq!(ids[0].1.to_string(), "ia_scenario/c1-ia1");
}

#[test]
fn accept_f49_d_01_synthetic_missing_spawn_points_is_refused_not_zero_presets() {
    let temp = campaign_tree("nospawn");
    add_directory(&temp, "C1", "IA1", ia_without_spawn_points());
    let error = survey_retail_instant_action(&temp.0).expect_err("refused");
    assert!(
        matches!(error, IaSurveyError::MissingSpawnPoints { .. }),
        "{error:?}"
    );
    let text = error.to_string();
    assert!(
        text.contains("zbd/c1/ia1/zrdr.zbd") && text.contains("ia.zrd"),
        "{text}"
    );
}

#[test]
fn accept_f49_d_01_synthetic_unknown_disallowed_type_is_refused() {
    let temp = campaign_tree("unknown");
    add_directory(
        &temp,
        "C1",
        "IA1",
        ia_document(
            &[("dogfight_ace", 1)],
            Some(&["capture_the_flag"]),
            "dogfight_ace",
            None,
        ),
    );
    let error = survey_retail_instant_action(&temp.0).expect_err("refused");
    match error {
        IaSurveyError::UnknownMissionType {
            key, mission_type, ..
        } => {
            assert_eq!(key, "disallow_missions");
            assert_eq!(mission_type, "capture_the_flag");
            assert!(!IA_MISSION_TYPE_VOCABULARY.contains(&"capture_the_flag"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn accept_f49_d_01_synthetic_duplicate_ids_are_refused() {
    use cs_content::instant_action::read_ia_descriptor;
    let bytes = ia_document(&[("dogfight_ace", 1)], None, "dogfight_ace", None);
    let root = cs_content::stunts::decode_zrd(&bytes).expect("fixture decodes");
    let read = |container: &str| {
        read_ia_descriptor(
            &root,
            container,
            &"0".repeat(64),
            0,
            bytes.len() as u64,
            "c1",
            "ia1",
        )
        .expect("descriptor reads")
    };
    let error = cs_content::instant_action::IaDenominator::try_new(
        "f".repeat(64),
        vec![
            read("zbd/c1/ia1/zrdr.zbd"),
            read("other/zbd/c1/ia1/zrdr.zbd"),
        ],
    )
    .expect_err("duplicate refused");
    assert!(
        matches!(error, IaSurveyError::DuplicateId { .. }),
        "{error:?}"
    );
}

#[test]
fn accept_f49_d_01_synthetic_undecodable_member_is_refused_with_the_decode_reason() {
    let temp = campaign_tree("baddecode");
    add_directory(&temp, "C1", "IA1", vec![9, 0, 0, 0, 1, 2, 3, 4]);
    let error = survey_retail_instant_action(&temp.0).expect_err("refused");
    match &error {
        IaSurveyError::Decode {
            container, code, ..
        } => {
            assert_eq!(container, "zbd/c1/ia1/zrdr.zbd");
            assert_eq!(*code, "unknown_tag");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn accept_f49_d_01_synthetic_installation_without_directories_is_refused() {
    let temp = campaign_tree("none");
    let error = survey_retail_instant_action(&temp.0).expect_err("refused");
    assert_eq!(error, IaSurveyError::NoDirectories);
}

/// Every `ZBD/<group>/IA<n>/zrdr.zbd` under `root`, found with `std::fs` alone.
fn walk_ia_archives(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let lower = |p: &Path| {
        p.file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase()
    };
    let Some(zbd) = fs::read_dir(root)
        .unwrap()
        .flatten()
        .find(|e| lower(&e.path()) == "zbd")
    else {
        return found;
    };
    for group in fs::read_dir(zbd.path()).unwrap().flatten() {
        if !group.path().is_dir() {
            continue;
        }
        for leaf in fs::read_dir(group.path()).unwrap().flatten() {
            let name = lower(&leaf.path());
            let digits = name.strip_prefix("ia").unwrap_or("");
            if leaf.path().is_dir()
                && !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
            {
                for file in fs::read_dir(leaf.path()).unwrap().flatten() {
                    if lower(&file.path()) == "zrdr.zbd" {
                        found.insert(format!("zbd/{}/{name}/zrdr.zbd", lower(&group.path())));
                    }
                }
            }
        }
    }
    found
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_01_retail_denominator_is_remeasured_from_ia_zrd() {
    let root = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the installation"),
    );
    let survey = survey_retail_instant_action(&root).expect("the installation measures");

    let walked = walk_ia_archives(&root);
    let reported: BTreeSet<String> = survey
        .directories()
        .iter()
        .map(|d| d.container().to_owned())
        .collect();
    assert_eq!(
        reported, walked,
        "the reader finds exactly the walked archives"
    );
    assert!(!walked.is_empty());

    let vocabulary = survey.observed_vocabulary();
    let expected_vocabulary: BTreeSet<String> = IA_MISSION_TYPE_VOCABULARY
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(vocabulary, expected_vocabulary);

    let mut preset_total = 0;
    for dir in survey.directories() {
        let name = dir.container();
        assert!(survey.offered_is_vocabulary_minus_disallowed(dir), "{name}");
        assert!(
            !dir.offered()
                .iter()
                .any(|o| o.mission_type() == "ground_target"),
            "{name}"
        );
        assert!(
            dir.disallowed().iter().any(|d| d == "ground_target"),
            "{name}"
        );
        let expected_default = match dir.world_group() {
            "c1" | "c3" => "dogfight_squadron",
            "c1b" | "c2" | "c4" | "c5" => "stunt_flying",
            "c1c" | "c2b" => "zeppelin_run",
            other => panic!("unexpected world group {other}"),
        };
        assert_eq!(dir.default_mission_type(), Some(expected_default), "{name}");
        let stunt_disallowed = dir.disallowed().iter().any(|d| d == "stunt_flying");
        assert_eq!(
            stunt_disallowed,
            matches!(dir.world_group(), "c1c" | "c2b"),
            "{name}"
        );
        // The scoping note said all eight declare `num_wingmen = 3`; c2b
        // declares none, like its absent `player_plane`; both fall back to the
        // executable's record defaults (#1573).
        let expected_wingmen = (dir.world_group() != "c2b").then_some(3);
        assert_eq!(dir.aircraft().wingmen(), expected_wingmen, "{name}");
        assert_eq!(dir.aircraft().enemy_groups().len(), 4, "{name}");
        assert!(dir.aircraft().ace().is_declared(), "{name}");
        match (dir.world_group(), dir.player_plane()) {
            ("c2b", IaPlayerPlane::EngineDefault { index: 5 }) => {}
            ("c2b", other) => panic!("c2b must report the engine default plane, got {other:?}"),
            (_, IaPlayerPlane::Declared(_)) => {}
            (group, other) => panic!("{group} must declare a plane, got {other:?}"),
        }
        preset_total += dir.preset_ids().expect("ids").len();
    }
    let ids = survey.preset_ids().expect("ids are valid");
    assert_eq!(ids.len(), preset_total);
    let unique: BTreeSet<String> = ids.iter().map(|(p, _)| p.to_string()).collect();
    assert_eq!(unique.len(), ids.len(), "every preset id is unique");
    let scenarios: BTreeSet<String> = ids.iter().map(|(_, s)| s.to_string()).collect();
    assert_eq!(
        scenarios.len(),
        walked.len(),
        "one scenario id per directory"
    );
}
