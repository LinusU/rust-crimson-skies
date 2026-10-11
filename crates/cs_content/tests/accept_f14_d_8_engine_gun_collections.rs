//! Acceptance scenario F14-D.8 (follow-up task #563): the `engine` and `gun`
//! collections of the retail baseline inventory (stage `### F14-D` of
//! `specs/F14-canonical-content-catalog-and-dependency-closure.md`).
//!
//! `docs/contracts/IDENTITY-CONTENT.md` requires "engines/armor/guns/ammo/
//! hardpoint equipment" as catalog collections. The reader archive
//! `ZBD/zrdr.zbd` states two of those families as data: `engines.zrd` holds one
//! `(id "name" factor)` row per engine and `weapons.zrd`'s `BALLISTICS` section
//! holds one `wep_NN` record per weapon, a cannon being a record that sets the
//! file's own `CANNON` key. Each becomes one row, located by its own bytes
//! inside the member and pointing at the archive's inventory row. No stored
//! number is copied and no unit is assumed; every row says so as an explicit
//! unknown. Armor, ammunition and hardpoint equipment have no entry list in
//! this archive, so they have no row (see the findings note).
//!
//! The non-retail tests write **synthetic installation trees**: a reader
//! archive authored for this file in the measured grammar, with names and
//! numbers that are not original data. The retail test is
//! `#[ignore = "requires CS_GAME_DIR"]`; run it with `--include-ignored`.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    ENGINE_GUN_ARCHIVE, ENGINE_UNKNOWN_CLAIM, GUN_UNKNOWN_CLAIM, baseline_report_json,
    install_file_key, retail_baseline,
};
use cs_content::original_airframe::OriginalDocuments;
use cs_types::content::{ContentId, ContentKind, NormalizeState, Readiness, UnsupportedReason};
use cs_types::evidence::ClaimStatus;

const CLAIM_ENGINE: &str = "f14.d.8.baseline.engine_row";
const CLAIM_GUN: &str = "f14.d.8.baseline.gun_record";

struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-8-eg-{label}-{}-{}",
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
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    bytes
}

fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

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

fn engine_row(id: u32, name: &str, factor: f32) -> Vec<u8> {
    zrd_list(vec![zrd_int(id), zrd_text(name), zrd_float(factor)])
}

/// A `key (value)` pair of a weapon record, as the measured flat shape stores it.
fn pair(key: &str, value: Vec<u8>) -> Vec<Vec<u8>> {
    vec![zrd_text(key), zrd_list(vec![value])]
}

fn weapon(key: &str, name: &str, cannon: bool) -> Vec<Vec<u8>> {
    let mut fields = pair("NAME", zrd_text(name));
    fields.extend(pair("AMMO_LIMIT", zrd_int(7)));
    if cannon {
        fields.push(zrd_text("CANNON"));
        fields.push(zrd_list(Vec::new()));
    } else {
        fields.push(zrd_text("ROCKET"));
        fields.push(zrd_list(Vec::new()));
    }
    vec![zrd_text(key), zrd_list(fields)]
}

fn weapons_document(records: Vec<Vec<Vec<u8>>>) -> Vec<u8> {
    let mut section = Vec::new();
    for record in records {
        section.extend(record);
    }
    let mut top = pair("VERSION", zrd_int(2));
    top.push(zrd_text("BALLISTICS"));
    top.push(zrd_list(section));
    zrd_list(vec![zrd_list(top)])
}

fn campaign_tree(label: &str, engines: Option<Vec<u8>>, weapons: Option<Vec<u8>>) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write(
        "ZBD/C1/M01/zrdr.zbd",
        &reader_archive(&[
            ("net.zrd", zrd_int(1)),
            ("map.zrd", zrd_int(1)),
            ("aiv.zrd", zrd_int(1)),
            ("objectives.zrd", zrd_int(1)),
        ]),
    );
    temp.write("ZBD/C1/M01/mis_anim.zbd", b"mission animation bytes");
    temp.write(
        "ZBD/C1/zrdr.zbd",
        &reader_archive(&[("templates.zrd", zrd_int(1)), ("cam_anim.zrd", zrd_int(1))]),
    );
    let mut members = Vec::new();
    if let Some(engines) = engines {
        members.push(("engines.zrd", engines));
    }
    if let Some(weapons) = weapons {
        members.push(("weapons.zrd", weapons));
    }
    if !members.is_empty() {
        temp.write(ENGINE_GUN_ARCHIVE, &reader_archive(&members));
    }
    temp
}

fn rows_of(
    baseline: &cs_content::catalog::baseline::Baseline,
    kind: ContentKind,
) -> Vec<&cs_types::content::CatalogElement> {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == kind)
        .collect()
}

fn status_of(
    baseline: &cs_content::catalog::baseline::Baseline,
    kind: ContentKind,
) -> &cs_content::catalog::baseline::CollectionStatus {
    baseline
        .collection_status
        .iter()
        .find(|status| status.kind == kind)
        .unwrap_or_else(|| panic!("the {kind} collection reports its status"))
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

fn unknown_claims(row: &cs_types::content::CatalogElement) -> Vec<&str> {
    row.unsupported_reasons
        .iter()
        .filter_map(|reason| match reason {
            UnsupportedReason::Unknown { claim_id, .. } => Some(claim_id.as_str()),
            _ => None,
        })
        .collect()
}

/// The common row contract: a span inside `archive_bytes` that starts at a list
/// node, one static edge onto the archive's inventory row, and no value.
fn check_row(
    baseline: &cs_content::catalog::baseline::Baseline,
    row: &cs_types::content::CatalogElement,
    archive_bytes: &[u8],
    claim: &str,
    unknown: &str,
) {
    assert!(row.origin.is_original());
    let span = row.origin.source().expect("an installation span");
    assert_eq!(span.container_path(), ENGINE_GUN_ARCHIVE);
    assert!(matches!(
        span.member_key(),
        Some("engines.zrd" | "weapons.zrd")
    ));
    assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
    let start = span.offset() as usize;
    let end = start + span.length() as usize;
    assert!(end <= archive_bytes.len(), "{}: inside the archive", row.id);
    assert_eq!(
        archive_bytes[start..start + 4],
        4_u32.to_le_bytes(),
        "{}: the span starts on a list node",
        row.id
    );
    assert_eq!(row.dependencies.len(), 1);
    let edge = &row.dependencies[0];
    assert_eq!(
        edge.target,
        cid(
            ContentKind::InstallFile,
            &install_file_key(ENGINE_GUN_ARCHIVE)
        )
    );
    assert_eq!(edge.provenance.claim_id.as_str(), claim);
    assert_eq!(edge.provenance.class, ClaimStatus::ObservedTool);
    assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
    assert_eq!(row.readiness, Readiness::Unavailable);
    assert_eq!(unknown_claims(row), vec![unknown]);
    assert!(
        row.unsupported_reasons
            .contains(&UnsupportedReason::MissingRuntimeConsumer)
    );
}

#[test]
fn accept_f14_d_8_engine_gun_rows_are_read_from_their_members() {
    let engines = zrd_list(vec![
        engine_row(3, "generic", 0.5),
        engine_row(11, "Alpha Lvl-1", 0.75),
        engine_row(3, "generic again", 0.9),
    ]);
    let weapons = weapons_document(vec![
        weapon("wep_00", "30slug", true),
        weapon("wep_04", "rocket", false),
        weapon("wep_30", "30 DD", true),
    ]);
    let temp = campaign_tree("rows", Some(engines), Some(weapons));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let archive = fs::read(temp.0.join(ENGINE_GUN_ARCHIVE)).expect("archive");

    let engines = rows_of(&baseline, ContentKind::Engine);
    assert_eq!(
        engines.iter().map(|row| row.id.key()).collect::<Vec<_>>(),
        vec!["engine-11", "engine-3"]
    );
    assert_eq!(engines[1].display_name.as_deref(), Some("generic"));
    for row in &engines {
        check_row(&baseline, row, &archive, CLAIM_ENGINE, ENGINE_UNKNOWN_CLAIM);
    }
    let status = status_of(&baseline, ContentKind::Engine);
    assert_eq!(status.rows, 2);
    assert_eq!(status.source, ENGINE_GUN_ARCHIVE);
    assert_eq!(status.gaps.get("duplicate_engine_id"), Some(&1));

    let guns = rows_of(&baseline, ContentKind::Gun);
    assert_eq!(
        guns.iter().map(|row| row.id.key()).collect::<Vec<_>>(),
        vec!["wep_00", "wep_30"],
        "the rocket record sets no CANNON key and is not a gun"
    );
    assert_eq!(guns[1].display_name.as_deref(), Some("30 DD"));
    for row in &guns {
        check_row(&baseline, row, &archive, CLAIM_GUN, GUN_UNKNOWN_CLAIM);
    }
    let status = status_of(&baseline, ContentKind::Gun);
    assert_eq!(status.rows, 2);
    assert_eq!(status.gaps.get("weapon_record_without_cannon"), Some(&1));
    assert!(rows_of(&baseline, ContentKind::Armor).is_empty());
    assert!(rows_of(&baseline, ContentKind::Ammo).is_empty());
    assert!(rows_of(&baseline, ContentKind::HardpointEquipment).is_empty());
    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"engine\"") && report.contains("\"gun\""));
}

#[test]
fn accept_f14_d_8_engine_gun_absent_members_are_named_gaps_not_rows() {
    let temp = campaign_tree("absent", None, None);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    for kind in [ContentKind::Engine, ContentKind::Gun] {
        assert!(rows_of(&baseline, kind).is_empty());
        let status = status_of(&baseline, kind);
        assert_eq!(status.rows, 0);
        assert!(
            status
                .diagnostic
                .as_deref()
                .is_some_and(|d| d.contains(ENGINE_GUN_ARCHIVE))
        );
    }

    let temp = campaign_tree(
        "one-member",
        Some(zrd_list(vec![engine_row(1, "e", 1.0)])),
        None,
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    assert_eq!(rows_of(&baseline, ContentKind::Engine).len(), 1);
    assert!(rows_of(&baseline, ContentKind::Gun).is_empty());
    assert!(
        status_of(&baseline, ContentKind::Gun)
            .diagnostic
            .as_deref()
            .is_some_and(|d| d.contains("weapons.zrd"))
    );
}

#[test]
fn accept_f14_d_8_engine_gun_rows_are_not_launchable_and_the_denominator_does_not_move() {
    let without = campaign_tree("plain", None, None);
    let with = campaign_tree(
        "with",
        Some(zrd_list(vec![engine_row(1, "e", 1.0)])),
        Some(weapons_document(vec![weapon("wep_00", "g", true)])),
    );
    let a = retail_baseline(&without.0).expect("reads");
    let b = retail_baseline(&with.0).expect("reads");
    assert!(!ContentKind::Engine.is_launchable() && !ContentKind::Gun.is_launchable());
    assert_eq!(a.roots, b.roots);
    assert_eq!(a.coverage.roots, b.coverage.roots);
    assert_eq!(a.coverage.reachable, b.coverage.reachable);
    assert_eq!(b.coverage.unresolved_references, 0);
    assert_eq!(b.coverage.unreachable_by_kind.get("engine"), Some(&1));
    assert_eq!(b.coverage.unreachable_by_kind.get("gun"), Some(&1));
}

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_8_retail_the_installation_states_its_engines_and_guns() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let archive = fs::read(dir.join(ENGINE_GUN_ARCHIVE)).expect("archive");

    let documents = OriginalDocuments::read(&dir).expect("production reader");
    let mut ids: Vec<u32> = documents
        .engines()
        .root()
        .as_list()
        .expect("a list")
        .iter()
        .map(|row| row.as_list().expect("row")[0].as_scalar_u32().expect("id"))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let engines = rows_of(&baseline, ContentKind::Engine);
    assert_eq!(
        engines.len(),
        ids.len(),
        "one row per distinct stored engine id"
    );
    for id in [10_u32, 42] {
        assert!(
            engines
                .iter()
                .any(|row| row.id.key() == format!("engine-{id}"))
        );
    }
    assert!(engines.iter().any(|row| {
        row.display_name
            .as_deref()
            .is_some_and(|n| n.contains("Lvl-1"))
    }));
    for row in &engines {
        check_row(&baseline, row, &archive, CLAIM_ENGINE, ENGINE_UNKNOWN_CLAIM);
    }

    let guns = rows_of(&baseline, ContentKind::Gun);
    let keys: Vec<&str> = guns.iter().map(|row| row.id.key()).collect();
    assert!(keys.contains(&"wep_00") && keys.contains(&"wep_73"));
    assert!(!keys.contains(&"wep_04"), "a rocket record is not a gun");
    assert!(
        !keys.contains(&"wep_25"),
        "a record with no CANNON key is not a gun"
    );
    for row in &guns {
        check_row(&baseline, row, &archive, CLAIM_GUN, GUN_UNKNOWN_CLAIM);
    }
    let status = status_of(&baseline, ContentKind::Gun);
    assert_eq!(status.rows, guns.len());
    assert!(
        status
            .gaps
            .get("weapon_record_without_cannon")
            .copied()
            .unwrap_or(0)
            > 0
    );
    assert_eq!(baseline.coverage.unresolved_references, 0);
}
