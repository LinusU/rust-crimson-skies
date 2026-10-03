//! Acceptance scenario F14-D.8: the `stunt` and `scrapbook_item` collections of
//! the retail baseline inventory, and the refused `custom_plane` collection
//! (follow-up task #491 of F14-D.2 / #389; stage `### F14-D` of
//! `specs/F14-canonical-content-catalog-and-dependency-closure.md`).
//!
//! `docs/contracts/IDENTITY-CONTENT.md` requires "stunts and scrapbook rewards"
//! and "legacy custom-plane resources" as catalog collections. The baseline
//! inventory had no `ContentKind::Stunt`, `ScrapbookItem` or `CustomPlane` row
//! at all, so its report's `collections` object had no such entries.
//!
//! **What this stage adds.** One `ContentKind::Stunt` row per fly-through
//! danger-zone target of an instant-action scenario the installation's own
//! `ia.zrd` marks `stunt_flying`, read by the producing stage's own readers
//! (`cs_content::stunts`) over the scenario reader archive's `targets.zrd`. One
//! `ContentKind::ScrapbookItem` row per `Mission_Spread_Item` record of the
//! shared archive's `ASSETS/SCRAPBOOK.CSV`, read through the production ROF and
//! keyed-list readers. Each row is `Origin::Installation` over a checked span,
//! with a stable semantic id and one `Static` edge, `observed_tool`, onto the
//! inventory row holding its bytes. The unknowns the source does not answer stay
//! explicit `UnsupportedReason::Unknown`s.
//!
//! **What this stage refuses.** `ContentKind::CustomPlane` gets **no** row and
//! no collection record: nothing about that format has been measured in either
//! direction — F64-A's `referenced_by: &[]` is empty because *that* stage was
//! written without the `retail` capability and opened no installation file, which
//! is the absence of a measurement rather than the result of one — so a row could
//! only be guessed from a file name. A scenario's fly-through target that is not a
//! stunt, a scrapbook entry the documented schema does not cover, and a repeated
//! identity in either collection stay as counted gaps in the collection record
//! instead of being dropped or turned into a duplicate identity.
//!
//! The non-retail tests write **synthetic installation trees** into temporary
//! directories: a version-one reader archive for the campaign mission, the world
//! group and the instant-action scenario (so the shared campaign walk and
//! F14-D.1's classifier read them), reader-archive members written with the
//! measured `.zrd` grammar, and a synthetic ROF container holding a keyed-list
//! `ASSETS/SCRAPBOOK.CSV`. Every spelling is authored for this file; nothing is
//! derived from original data. They exercise production code only —
//! `retail_baseline`, the shared campaign walk, `reader_dirs::classify`,
//! `script_raw::discover_container`, `stunts::{decode_zrd, scenario_mission_type,
//! scenario_fly_through_targets}`, `cs_assets::rof::mount_rof_into` and
//! `config::ConfigDocument` — so removing the collections fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation and pins the rows it really holds: 45 `stunt_flying`
//! targets and 461 scrapbook items, with 9 non-stunt fly-through targets left as
//! a counted gap. Run it with `--include-ignored`; without `CS_GAME_DIR` it
//! fails loudly rather than passing vacuously.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    SCRAPBOOK_CONTAINER, SCRAPBOOK_ITEM_CLAIM, SCRAPBOOK_MEMBER, STUNT_CLEARANCE_CLAIM,
    STUNT_DIRECTION_CLAIM, STUNT_GEOMETRY_CLAIM, STUNT_REPEAT_CLAIM, STUNT_REWARD_CLAIM,
    baseline_report_json, install_file_key, retail_baseline,
};
use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_DIRECTORY, RECORD_BYTES};
use cs_types::content::{ContentId, ContentKind, NormalizeState, Readiness, UnsupportedReason};
use cs_types::evidence::ClaimStatus;
use cs_types::install::{ParseState, RelativePath};

/// The claim the stunt rows and their edges are recorded under.
const CLAIM_STUNT: &str = "f14.d.8.baseline.stunt_target";

// ------------------------------------------------------------ temp tree ---

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-8-{label}-{}-{}",
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

// ------------------------------------------------------------- .zrd ---

/// A `.zrd` integer node: tag `1` and the value.
fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// A `.zrd` text node: tag `3`, the byte length and the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, then **`children.len() + 1`** as the count, then
/// the children (the measured grammar: the profiler stores one more than the
/// child count).
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

// ------------------------------------------------------ reader archives ---

/// A version-one reader archive holding `members` in order: the member data,
/// then one 148-byte index entry each (u32 start, u32 length, a 64-byte
/// NUL-padded name and 76 bytes), then the u32 version `1` and u32 count.
fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::with_capacity(members.len());
    for (name, member) in members {
        let start = bytes.len() as u32;
        bytes.extend_from_slice(member);
        entries.push((start, member.len() as u32, *name));
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let name = name.as_bytes();
        assert!(name.len() < 64, "a fixture member name fits its field");
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// An authored `ia.zrd` scenario descriptor: the measured flat shape
/// `["mission_type", ["<type>"], "dzones", [["dzpathN", "dzN"], …]]`.
fn ia_document(mission_type: &str, zones: &[(&str, &str)]) -> Vec<u8> {
    let mut children = vec![
        zrd_text("mission_type"),
        zrd_list(vec![zrd_text(mission_type)]),
    ];
    if !zones.is_empty() {
        children.push(zrd_text("dzones"));
        children.push(zrd_list(
            zones
                .iter()
                .map(|(world_node, label)| zrd_list(vec![zrd_text(world_node), zrd_text(label)]))
                .collect(),
        ));
    }
    zrd_list(children)
}

/// One authored `targets.zrd` fly-through danger-zone objective in the measured
/// pair shape.
fn fly_through_target(zone_label: &str, description: &str) -> Vec<u8> {
    zrd_list(vec![
        zrd_list(vec![zrd_text("description"), zrd_text(description)]),
        zrd_list(vec![zrd_text("category_label"), zrd_text("MSG_OBJ_DZ")]),
        zrd_list(vec![zrd_text("help_label"), zrd_text("MSG_OBJ_FLYTHROUGH")]),
        zrd_list(vec![
            zrd_text("nodes"),
            zrd_list(vec![zrd_text(zone_label)]),
        ]),
    ])
}

/// An authored `targets.zrd` objective that is **not** a fly-through
/// danger-zone: a rearm base, in the same pair shape.
fn other_target(description: &str) -> Vec<u8> {
    zrd_list(vec![zrd_list(vec![
        zrd_text("description"),
        zrd_text(description),
    ])])
}

/// An authored `targets.zrd` root: one child per objective.
fn targets_document(records: Vec<Vec<u8>>) -> Vec<u8> {
    zrd_list(records)
}

// ------------------------------------------------------------- fixtures ---

/// The authored campaign tree: one campaign mission under world group `C1`, its
/// mission animation archive, and the group's shared reader, so the campaign
/// walk declares `c1` and F14-D.1's classifier can classify the scenario.
fn campaign_tree(label: &str) -> TempInstall {
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
    temp
}

/// Adds one instant-action scenario directory under `C1` whose `ia.zrd` names
/// `mission_type` and whose `targets.zrd` holds `targets`.
fn add_ia_scenario(temp: &TempInstall, leaf: &str, mission_type: &str, targets: Vec<Vec<u8>>) {
    temp.write(
        &format!("ZBD/C1/{leaf}/zrdr.zbd"),
        &reader_archive(&[
            ("ia.zrd", ia_document(mission_type, &[("dzpath1", "dz1")])),
            ("targets.zrd", targets_document(targets)),
            ("map.zrd", zrd_int(1)),
            ("aiv.zrd", zrd_int(1)),
            ("objectives.zrd", zrd_int(1)),
        ]),
    );
    temp.write(
        &format!("ZBD/C1/{leaf}/mis_anim.zbd"),
        b"scenario animation bytes",
    );
}

// -------------------------------------------------------------- ROF ---

/// One authored file entry: its name and the bytes stored in the container.
struct Entry<'a> {
    name: &'a str,
    stored: Vec<u8>,
}

impl<'a> Entry<'a> {
    fn plain(name: &'a str, bytes: &[u8]) -> Self {
        Self {
            name,
            stored: bytes.to_vec(),
        }
    }
}

fn rof_names(names: &[&str]) -> Vec<u8> {
    let mut table = Vec::new();
    for name in names {
        table.extend_from_slice(name.as_bytes());
        table.push(0);
    }
    table
}

/// One directory block: header, 24-byte records, name table.
fn block(records: &[[u32; 6]], names: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for record in records {
        for word in record {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes.extend_from_slice(names);
    bytes
}

/// `[directory: entries…][payloads…]`, one directory deep: the shape of the
/// retail archive's `ASSETS/` directory.
fn rof_container(directory: &str, entries: &[Entry<'_>]) -> Vec<u8> {
    let root_names = rof_names(&[directory]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let entry_names: Vec<&str> = entries.iter().map(|entry| entry.name).collect();
    let sub_names = rof_names(&entry_names);
    let sub_len = DIRECTORY_HEADER_BYTES + entries.len() * RECORD_BYTES + sub_names.len();

    let root = block(
        &[[
            root_len as u32,
            0,
            0,
            FLAG_DIRECTORY,
            directory.len() as u32 + 1,
            1,
        ]],
        &root_names,
    );
    let mut cursor = (root_len + sub_len) as u32;
    let mut records = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let stored = entry.stored.len() as u32;
        records.push([
            cursor,
            stored,
            stored,
            0,
            entry.name.len() as u32 + 1,
            10 + index as u32,
        ]);
        cursor += stored;
    }
    let mut bytes = root;
    bytes.extend_from_slice(&block(&records, &sub_names));
    for entry in entries {
        bytes.extend_from_slice(&entry.stored);
    }
    assert_eq!(bytes.len(), cursor as usize, "every payload placed once");
    bytes
}

/// An authored `ASSETS/SCRAPBOOK.CSV`: a `[SCRAPBOOK]` section, a comment, two
/// `Mission_Spread_Item` records (sixteen fields, a numeric first field) and one
/// `B`-letter layout record the scrapbook schema does not cover.
fn scrapbook_member() -> Vec<u8> {
    let mut csv = Vec::new();
    csv.extend_from_slice(b"[SCRAPBOOK]\r\n");
    csv.extend_from_slice(b"; Mission_Spread_Item\r\n");
    for index in 0..2 {
        csv.extend_from_slice(
            format!(
                "SBITEM{index}={},IDS_IMG,thumb{index}.png,PNG,10,20,255,64,64,0,\
                 \"0,0,64,64\",1,1,1,TITLE{index},TEXT{index}\r\n",
                index + 1
            )
            .as_bytes(),
        );
    }
    csv.extend_from_slice(b"BTNSHAPE=B,art.png,1,2,3,0,IDS_LABEL,4,5,,,,,6,1\r\n");
    csv
}

/// Adds a synthetic `GOSDATA/ASSETS/crimson.rof` holding `member`.
fn add_scrapbook_archive(temp: &TempInstall, member: &[u8]) {
    temp.write(
        SCRAPBOOK_CONTAINER,
        &rof_container("ASSETS", &[Entry::plain("SCRAPBOOK.CSV", member)]),
    );
}

/// One authored `Mission_Spread_Item` record line: the sixteen fields the
/// documented schema covers, with `index` as the numeric first field and
/// `variant` in the title so two records under one key can be made to agree
/// (`variant` equal) or disagree (it differs).
fn scrapbook_record(index: u32, variant: &str) -> String {
    format!(
        "={},IDS_IMG,thumb.png,PNG,10,20,255,64,64,0,\
         \"0,0,64,64\",1,1,1,{variant},TEXT\r\n",
        index
    )
}

/// An authored scrapbook member holding exactly the `(key, variant)` records
/// named, in that order, under the documented `[SCRAPBOOK]` section.
///
/// The numeric first field is derived from the key itself rather than from the
/// record's position, so two records that carry the same key **and** the same
/// variant are byte-identical lines — which is what "the table declares this item
/// twice" means. A position-derived field would make every repeat look like a
/// different body and the test would prove nothing.
fn scrapbook_member_with(records: &[(&str, &str)]) -> Vec<u8> {
    let mut csv = Vec::new();
    csv.extend_from_slice(b"[SCRAPBOOK]\r\n");
    csv.extend_from_slice(b"; Mission_Spread_Item\r\n");
    for (key, variant) in records {
        let index = key.bytes().fold(0_u32, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(u32::from(byte)) % 900 + 1
        });
        csv.extend_from_slice(key.as_bytes());
        csv.extend_from_slice(scrapbook_record(index, variant).as_bytes());
    }
    csv
}

// --------------------------------------------------------------- helpers ---

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
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

/// The claim ids of one row's explicit unknowns, in the row's order.
fn unknown_claims(row: &cs_types::content::CatalogElement) -> Vec<&str> {
    row.unsupported_reasons
        .iter()
        .filter_map(|reason| match reason {
            UnsupportedReason::Unknown { claim_id, .. } => Some(claim_id.as_str()),
            _ => None,
        })
        .collect()
}

// ------------------------------------------------------------- stunts ---

/// The mapping arm: a `stunt_flying` scenario's fly-through targets become one
/// row each, keyed by the scenario and the target's own zone label, located by
/// the `targets.zrd` member's own span and pointing at the archive's inventory
/// row.
#[test]
fn accept_f14_d_8_a_stunt_scenario_becomes_one_row_per_fly_through_target() {
    let temp = campaign_tree("stunt-rows");
    add_ia_scenario(
        &temp,
        "IA1",
        "stunt_flying",
        vec![
            fly_through_target("dz1", "MSG_OBJ_DZ_ONE"),
            fly_through_target("dz2", "MSG_OBJ_DZ_TWO"),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let rows = rows_of(&baseline, ContentKind::Stunt);

    assert_eq!(
        rows.iter()
            .map(|row| row.id.key().to_owned())
            .collect::<Vec<_>>(),
        vec!["c1-ia1-dz1".to_owned(), "c1-ia1-dz2".to_owned()],
        "one row per fly-through target, keyed by the scenario and the target's own zone label"
    );
    assert_eq!(
        rows.iter()
            .map(|row| row.display_name.clone())
            .collect::<Vec<_>>(),
        vec![None, None],
        "the objective's description is a localized message key, not a display name"
    );

    let archive_id = cid(
        ContentKind::InstallFile,
        &install_file_key("ZBD/C1/IA1/zrdr.zbd"),
    );
    for row in &rows {
        assert!(row.origin.is_original());
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), "ZBD/C1/IA1/zrdr.zbd");
        assert_eq!(span.member_key(), Some("targets.zrd"));
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert!(span.length() > 0, "{}: the member has bytes", row.id);
        assert!(
            span.member_sha256().is_some(),
            "{}: the decoded member bytes are fingerprinted",
            row.id
        );

        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(!row.is_ready());
        assert!(row.runtime_consumers.is_empty());
        assert_eq!(
            row.unsupported_codes(),
            vec![
                "unknown",
                "unknown",
                "unknown",
                "unknown",
                "unknown",
                "missing_runtime_consumer",
            ],
            "{}: the five facts the scenario bytes do not state, plus the missing consumer",
            row.id
        );
        assert_eq!(
            unknown_claims(row),
            [
                STUNT_DIRECTION_CLAIM,
                STUNT_CLEARANCE_CLAIM,
                STUNT_REWARD_CLAIM,
                STUNT_REPEAT_CLAIM,
                STUNT_GEOMETRY_CLAIM,
            ],
            "{}",
            row.id
        );
        assert!(
            row.unsupported_reasons.iter().any(|reason| matches!(
                reason,
                UnsupportedReason::Unknown { reason, .. } if !reason.trim().is_empty()
            )),
            "{}: an unknown must say why",
            row.id
        );

        assert_eq!(row.dependencies.len(), 1, "{}: one static edge", row.id);
        let edge = &row.dependencies[0];
        assert_eq!(edge.target, archive_id);
        assert_eq!(edge.kind.label(), "static");
        assert_eq!(edge.provenance.claim_id.as_str(), CLAIM_STUNT);
        assert_eq!(
            edge.provenance.class,
            ClaimStatus::ObservedTool,
            "an agent-observed edge is never verified_original"
        );
        assert_eq!(edge.provenance.source.as_ref(), Some(span));
        assert_eq!(
            row.fingerprint.as_ref().map(|digest| digest.sha256),
            span.member_sha256(),
            "the row fingerprints exactly the member bytes its span locates"
        );
    }
    // The two targets are told apart by their zone labels, not by position: the
    // same archive member span backs both rows.
    let first = rows[0].origin.source().expect("a span");
    let second = rows[1].origin.source().expect("a span");
    assert_eq!(first, second, "both targets live in the same targets.zrd");

    let status = status_of(&baseline, ContentKind::Stunt);
    assert_eq!(
        status.source, "ZBD/<world group>/IA<n>/zrdr.zbd",
        "the collection has one source file per row, so it names the pattern"
    );
    assert_eq!(status.language, None);
    assert_eq!(status.rows, 2);
    assert_eq!(
        status.gaps.get("non_stunt_fly_through_targets"),
        None,
        "a stunt scenario contributes no non-stunt gap"
    );
    assert_eq!(status.diagnostic, None);

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"stunt\":2"), "{report}");
    assert!(report.contains("\"id\":\"stunt/c1-ia1-dz1\""), "{report}");
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );
    assert_eq!(
        report,
        baseline_report_json(&retail_baseline(&temp.0).expect("re-read")),
        "the report is byte-stable for the same installation"
    );
}

/// A fly-through target of a scenario the original does **not** mark
/// `stunt_flying` is a counted gap, never a stunt row.
#[test]
fn accept_f14_d_8_a_non_stunt_scenario_is_a_gap_not_a_row() {
    let temp = campaign_tree("dogfight");
    add_ia_scenario(
        &temp,
        "IA1",
        "dogfight_squadron",
        vec![fly_through_target("dz1", "MSG_OBJ_DZ_ONE")],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    assert!(
        rows_of(&baseline, ContentKind::Stunt).is_empty(),
        "a dogfight scenario declares no stunt"
    );
    let status = status_of(&baseline, ContentKind::Stunt);
    assert_eq!(status.rows, 0);
    assert_eq!(
        status.gaps.get("non_stunt_fly_through_targets"),
        Some(&1),
        "the target is accounted for under its own stable label"
    );
    assert!(
        status
            .diagnostic
            .as_deref()
            .is_some_and(|d| d.contains("stunt_flying")),
        "the empty collection names why: {:?}",
        status.diagnostic
    );
}

/// A `stunt_flying` scenario whose only target is not a danger-zone declares no
/// stunt: the objective is selected by its own measured labels, not by position.
#[test]
fn accept_f14_d_8_a_target_that_is_not_a_danger_zone_is_not_a_stunt() {
    let temp = campaign_tree("other-objective");
    add_ia_scenario(
        &temp,
        "IA1",
        "stunt_flying",
        vec![other_target("MSG_TRGT_REARM_BASE")],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    assert!(
        rows_of(&baseline, ContentKind::Stunt).is_empty(),
        "a rearm objective is not a fly-through danger-zone"
    );
    assert_eq!(status_of(&baseline, ContentKind::Stunt).rows, 0);
}

/// The zone label is the identity, not the position: two scenarios that name the
/// same label get two rows that cannot be confused.
#[test]
fn accept_f14_d_8_the_zone_label_is_the_identity_not_the_position() {
    let temp = campaign_tree("same-label");
    add_ia_scenario(
        &temp,
        "IA1",
        "stunt_flying",
        vec![fly_through_target("dz1", "MSG_OBJ_DZ_ONE")],
    );
    add_ia_scenario(
        &temp,
        "IA2",
        "stunt_flying",
        vec![fly_through_target("dz1", "MSG_OBJ_DZ_ONE")],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let keys: BTreeSet<String> = rows_of(&baseline, ContentKind::Stunt)
        .iter()
        .map(|row| row.id.key().to_owned())
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from(["c1-ia1-dz1".to_owned(), "c1-ia2-dz1".to_owned()]),
        "the scenario directory disambiguates the same zone label"
    );
}

/// One scenario naming one zone label twice is one gate named twice, not two
/// rows and not a reason to lose the whole inventory.
///
/// The identity is the zone label, so a second target under the same label would
/// be a second row with an identity the catalog refuses. Letting that refusal out
/// of the collection builder cost the installation **every** row it had — 6 009 on
/// the owner's data — over one repeated label, which is the defect this pins.
#[test]
fn accept_f14_d_8_a_zone_label_repeated_in_one_scenario_is_a_counted_repeat_not_a_second_row() {
    // (a) The same label twice with the same description: one gate, the repeat
    //     counted.
    let repeated = campaign_tree("zone-repeat");
    add_ia_scenario(
        &repeated,
        "IA1",
        "stunt_flying",
        vec![
            fly_through_target("dz1", "MSG_OBJ_DZ_ONE"),
            fly_through_target("dz1", "MSG_OBJ_DZ_ONE"),
            fly_through_target("dz2", "MSG_OBJ_DZ_TWO"),
        ],
    );
    let baseline = retail_baseline(&repeated.0).expect("the fixture installation reads");
    assert_eq!(
        rows_of(&baseline, ContentKind::Stunt)
            .iter()
            .map(|row| row.id.key().to_owned())
            .collect::<Vec<_>>(),
        vec!["c1-ia1-dz1".to_owned(), "c1-ia1-dz2".to_owned()],
        "the repeated label is one gate, so the identity is unique again"
    );
    let status = status_of(&baseline, ContentKind::Stunt);
    assert_eq!(status.rows, 2);
    assert_eq!(
        status.gaps.get("duplicate_zone_label"),
        Some(&1),
        "the repeat is accounted for under its own stable label"
    );
    assert_eq!(status.gaps.get("ambiguous_zone_label"), None);
    // The whole inventory still reads: the mission and the scenario are rows.
    assert_eq!(
        baseline.roots,
        vec![
            cid(ContentKind::Mission, "ch1-m01"),
            cid(ContentKind::IaScenario, "c1-ia1"),
        ]
    );

    // (b) The same label twice with two different descriptions: the scenario says
    //     nothing that tells the two apart, so neither is a row.
    let ambiguous = campaign_tree("zone-ambiguous");
    add_ia_scenario(
        &ambiguous,
        "IA1",
        "stunt_flying",
        vec![
            fly_through_target("dz1", "MSG_OBJ_DZ_ONE"),
            fly_through_target("dz1", "MSG_OBJ_DZ_OTHER"),
        ],
    );
    let baseline = retail_baseline(&ambiguous.0).expect("the fixture installation reads");
    assert!(
        rows_of(&baseline, ContentKind::Stunt).is_empty(),
        "a label the scenario gives two descriptions to is not keyed by guessing which one"
    );
    let status = status_of(&baseline, ContentKind::Stunt);
    assert_eq!(status.rows, 0);
    assert_eq!(
        status.gaps.get("ambiguous_zone_label"),
        Some(&2),
        "both declarations are accounted for"
    );
    assert_eq!(status.gaps.get("duplicate_zone_label"), None);
    assert_eq!(
        baseline.roots,
        vec![
            cid(ContentKind::Mission, "ch1-m01"),
            cid(ContentKind::IaScenario, "c1-ia1"),
        ],
        "the rest of the inventory still reads"
    );
}

// ---------------------------------------------------------- scrapbook ---

/// The mapping arm: every `Mission_Spread_Item` of the scrapbook member becomes
/// one row, keyed by the record's own entry key, located by the member's decoded
/// extent and pointing at the archive's inventory row.
#[test]
fn accept_f14_d_8_the_scrapbook_table_becomes_one_row_per_item() {
    let temp = campaign_tree("scrapbook-rows");
    let member = scrapbook_member();
    add_scrapbook_archive(&temp, &member);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let rows = rows_of(&baseline, ContentKind::ScrapbookItem);

    assert_eq!(
        rows.iter()
            .map(|row| row.id.key().to_owned())
            .collect::<Vec<_>>(),
        vec!["sbitem0".to_owned(), "sbitem1".to_owned()],
        "one row per Mission_Spread_Item, keyed by its own entry key"
    );
    assert_eq!(
        rows.iter()
            .map(|row| row.display_name.clone())
            .collect::<Vec<_>>(),
        vec![None, None],
        "the entry's field text is content, not a display name"
    );

    let archive_id = cid(
        ContentKind::InstallFile,
        &install_file_key(SCRAPBOOK_CONTAINER),
    );
    let member_sha = cs_assets::install::sha256(&member);
    for row in &rows {
        assert!(row.origin.is_original());
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), SCRAPBOOK_CONTAINER);
        assert_eq!(span.member_key(), Some(SCRAPBOOK_MEMBER));
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert_eq!(
            span.length(),
            member.len() as u64,
            "{}: the span is the member's decoded extent",
            row.id
        );
        assert_eq!(
            span.member_sha256(),
            Some(member_sha),
            "{}: the span fingerprints exactly the decoded member bytes",
            row.id
        );

        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(!row.is_ready());
        assert!(row.runtime_consumers.is_empty());
        assert_eq!(
            row.unsupported_codes(),
            vec!["not_normalized"],
            "{}: the record was parsed and nothing normalized its fields",
            row.id
        );

        assert_eq!(row.dependencies.len(), 1, "{}: one static edge", row.id);
        let edge = &row.dependencies[0];
        assert_eq!(edge.target, archive_id);
        assert_eq!(edge.kind.label(), "static");
        assert_eq!(edge.provenance.claim_id.as_str(), SCRAPBOOK_ITEM_CLAIM);
        assert_eq!(edge.provenance.class, ClaimStatus::ObservedTool);
        assert_eq!(edge.provenance.source.as_ref(), Some(span));
        assert_eq!(
            row.fingerprint.as_ref().map(|digest| digest.sha256),
            Some(member_sha)
        );
    }

    let status = status_of(&baseline, ContentKind::ScrapbookItem);
    assert_eq!(status.source, SCRAPBOOK_CONTAINER);
    assert_eq!(status.language, None);
    assert_eq!(status.rows, 2);
    assert_eq!(
        status.gaps.get("entry_not_a_scrapbook_item"),
        Some(&1),
        "the B-letter layout record is accounted for, not dropped"
    );
    assert_eq!(status.diagnostic, None);

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"scrapbook_item\":2"), "{report}");
    assert!(
        report.contains("\"id\":\"scrapbook_item/sbitem0\""),
        "{report}"
    );
    assert_eq!(
        report,
        baseline_report_json(&retail_baseline(&temp.0).expect("re-read")),
        "the report is byte-stable for the same installation"
    );
}

/// One entry key the table declares twice is one item declared twice, not two
/// rows and not a reason to lose the whole inventory.
///
/// Same defect as the repeated zone label, on the other collection: the entry key
/// is this row's identity, so a repeated key is a duplicate identity, and letting
/// [`Catalog::insert`]'s refusal out of the builder cost every catalog row the
/// installation has.
#[test]
fn accept_f14_d_8_a_repeated_entry_key_is_a_counted_repeat_not_a_second_row() {
    // (a) The same key twice with the same fields: one item, the repeat counted.
    let repeated = campaign_tree("key-repeat");
    add_scrapbook_archive(
        &repeated,
        &scrapbook_member_with(&[("SB0", "SAME"), ("SB1", "OTHER"), ("SB0", "SAME")]),
    );
    let baseline = retail_baseline(&repeated.0).expect("the fixture installation reads");
    assert_eq!(
        rows_of(&baseline, ContentKind::ScrapbookItem)
            .iter()
            .map(|row| row.id.key().to_owned())
            .collect::<Vec<_>>(),
        vec!["sb0".to_owned(), "sb1".to_owned()],
        "the repeated key is one item, so the identity is unique again"
    );
    let status = status_of(&baseline, ContentKind::ScrapbookItem);
    assert_eq!(status.rows, 2);
    assert_eq!(
        status.gaps.get("duplicate_entry_key"),
        Some(&1),
        "the repeat is accounted for under its own stable label"
    );
    assert_eq!(status.gaps.get("ambiguous_entry_key"), None);
    assert_eq!(
        baseline.roots,
        vec![cid(ContentKind::Mission, "ch1-m01")],
        "the rest of the inventory still reads"
    );

    // (b) The same key twice with different fields: the table says nothing that
    //     tells the two apart, so neither is a row.
    let ambiguous = campaign_tree("key-ambiguous");
    add_scrapbook_archive(
        &ambiguous,
        &scrapbook_member_with(&[("SB0", "ONE"), ("SB0", "TWO")]),
    );
    let baseline = retail_baseline(&ambiguous.0).expect("the fixture installation reads");
    assert!(
        rows_of(&baseline, ContentKind::ScrapbookItem).is_empty(),
        "a key the table gives two different bodies to is not keyed by guessing which one"
    );
    let status = status_of(&baseline, ContentKind::ScrapbookItem);
    assert_eq!(status.rows, 0);
    assert_eq!(
        status.gaps.get("ambiguous_entry_key"),
        Some(&2),
        "both declarations are accounted for"
    );
    assert_eq!(status.gaps.get("duplicate_entry_key"), None);
    assert_eq!(
        baseline.roots,
        vec![cid(ContentKind::Mission, "ch1-m01")],
        "the rest of the inventory still reads"
    );
}

/// A missing archive, an archive that does not mount, and a member that does not
/// read as the keyed-list table are all named gaps rather than empty readings.
#[test]
fn accept_f14_d_8_an_absent_or_unreadable_scrapbook_is_a_named_gap() {
    // (a) absent.
    let absent = campaign_tree("scrapbook-absent");
    let baseline = retail_baseline(&absent.0).expect("the fixture installation reads");
    assert!(rows_of(&baseline, ContentKind::ScrapbookItem).is_empty());
    let diagnostic = status_of(&baseline, ContentKind::ScrapbookItem)
        .diagnostic
        .as_deref()
        .expect("a missing archive is named, not dropped");
    assert!(
        diagnostic.contains(&format!("inventories no {SCRAPBOOK_CONTAINER}")),
        "{diagnostic}"
    );

    // (b) present but not a ROF container.
    let unreadable = campaign_tree("scrapbook-unreadable");
    unreadable.write(SCRAPBOOK_CONTAINER, b"not a rof container");
    let baseline = retail_baseline(&unreadable.0).expect("the fixture installation reads");
    assert!(rows_of(&baseline, ContentKind::ScrapbookItem).is_empty());
    let diagnostic = status_of(&baseline, ContentKind::ScrapbookItem)
        .diagnostic
        .as_deref()
        .expect("an unreadable archive is named");
    assert!(diagnostic.contains(SCRAPBOOK_CONTAINER), "{diagnostic}");
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
}

/// Neither a stunt nor a scrapbook item is launchable, so neither collection adds
/// a root or moves the coverage denominator.
#[test]
fn accept_f14_d_8_the_new_collections_are_not_launchable() {
    let temp = campaign_tree("roots");
    add_ia_scenario(
        &temp,
        "IA1",
        "stunt_flying",
        vec![fly_through_target("dz1", "MSG_OBJ_DZ_ONE")],
    );
    add_scrapbook_archive(&temp, &scrapbook_member());
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(!ContentKind::Stunt.is_launchable());
    assert!(!ContentKind::ScrapbookItem.is_launchable());
    assert!(!ContentKind::CustomPlane.is_launchable());
    assert_eq!(
        baseline.roots,
        vec![
            cid(ContentKind::Mission, "ch1-m01"),
            cid(ContentKind::IaScenario, "c1-ia1"),
        ],
        "only the campaign mission and the classified scenario are launchable"
    );
    assert_eq!(baseline.catalog.launchable_count(), 2);
    assert_eq!(baseline.catalog.original_launchable_count(), 2);
    assert_eq!(baseline.coverage.roots, 2);
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert!(
        baseline.coverage.unreachable_by_kind.get("stunt").copied() == Some(1),
        "no row reaches a stunt yet"
    );
    assert_eq!(
        baseline
            .coverage
            .unreachable_by_kind
            .get("scrapbook_item")
            .copied(),
        Some(2)
    );
    assert!(!baseline.catalog.is_fully_ready());
}

/// Legacy custom planes get no row and no collection record: no installation byte
/// names one, and an identity guessed from a file name is exactly what rule 4
/// rejects.
#[test]
fn accept_f14_d_8_custom_planes_have_no_row_and_no_collection_record() {
    let temp = campaign_tree("custom-plane");
    add_ia_scenario(
        &temp,
        "IA1",
        "stunt_flying",
        vec![fly_through_target("dz1", "MSG_OBJ_DZ_ONE")],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    assert!(
        rows_of(&baseline, ContentKind::CustomPlane).is_empty(),
        "no custom-plane row is fabricated"
    );
    assert!(
        baseline
            .collection_status
            .iter()
            .all(|status| status.kind != ContentKind::CustomPlane),
        "and no empty custom-plane collection record is fabricated either"
    );
    let report = baseline_report_json(&baseline);
    assert!(!report.contains("\"custom_plane\""), "{report}");
}

// ------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The stunt count a **second** derivation reaches: the fly-through danger-zone
/// targets of every scenario the installation's own `ia.zrd` marks
/// `stunt_flying`, read directly through the production container discovery and
/// `.zrd` decoders, without `stunt_rows` or `retail_baseline`.
fn independent_stunt_count(
    game_dir: &Path,
    classified: &[cs_content::catalog::reader_dirs::ClassifiedReaderDir],
) -> usize {
    use cs_content::stunts::{
        SCENARIO_MEMBER, SCENARIO_TARGETS_MEMBER, STUNT_MISSION_TYPE, decode_zrd,
        scenario_fly_through_targets, scenario_mission_type,
    };
    let mut total = 0;
    for dir in classified.iter().filter(|dir| {
        dir.role == cs_content::catalog::reader_dirs::ReaderDirRole::InstantActionScenario
    }) {
        let path = game_dir.join(&dir.program);
        let bytes = fs::read(&path).expect("a classified scenario archive reads");
        let spelling =
            RelativePath::new(&dir.program).expect("a classified program names a relative path");
        let discovery = cs_formats::script_raw::discover_container(&dir.program, &spelling, &bytes);
        let scenario = discovery
            .programs()
            .iter()
            .find(|program| program.locator().member() == Some(SCENARIO_MEMBER));
        let targets = discovery
            .programs()
            .iter()
            .find(|program| program.locator().member() == Some(SCENARIO_TARGETS_MEMBER));
        let (Some(scenario), Some(targets)) = (scenario, targets) else {
            continue;
        };
        let Ok(scenario_root) = decode_zrd(scenario.bytes()) else {
            continue;
        };
        if scenario_mission_type(&scenario_root) != Some(STUNT_MISSION_TYPE) {
            continue;
        }
        let Ok(targets_root) = decode_zrd(targets.bytes()) else {
            continue;
        };
        total += scenario_fly_through_targets(&targets_root).len();
    }
    total
}

/// The retail half: the installation's `stunt_flying` targets and scrapbook items
/// become rows, the non-stunt fly-through targets and non-scrapbook entries are
/// counted gaps, and the coverage denominator is unchanged.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_8_retail_the_installation_declares_its_stunt_and_scrapbook_collections() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let discovery = cs_assets::install::discover(&dir).expect("production discovery reads it");
    let install_sha = cs_assets::install::fingerprint(&discovery.manifest).to_hex();

    // --- stunts ---------------------------------------------------------
    let stunt_rows = rows_of(&baseline, ContentKind::Stunt);
    assert_eq!(
        stunt_rows.len(),
        45,
        "the 45 stunt_flying fly-through targets T463 measured"
    );
    let expected = independent_stunt_count(&dir, &baseline.classified_reader_dirs);
    assert_eq!(
        stunt_rows.len(),
        expected,
        "the baseline's rows equal an independent walk of the same scenarios"
    );
    let archive_ids: BTreeSet<String> = stunt_rows
        .iter()
        .map(|row| row.dependencies[0].target.key().to_owned())
        .collect();
    assert_eq!(
        archive_ids.len(),
        4,
        "the 45 targets come from the four stunt_flying scenarios T463 measured (c1b, c2, c4, c5)"
    );
    for row in &stunt_rows {
        assert!(row.origin.is_original(), "{}", row.id);
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.member_key(), Some("targets.zrd"), "{}", row.id);
        assert_eq!(span.install_sha256().to_hex(), install_sha, "{}", row.id);
        assert_eq!(
            row.unsupported_codes(),
            vec![
                "unknown",
                "unknown",
                "unknown",
                "unknown",
                "unknown",
                "missing_runtime_consumer",
            ],
            "{}",
            row.id
        );
        assert_eq!(
            unknown_claims(row),
            [
                STUNT_DIRECTION_CLAIM,
                STUNT_CLEARANCE_CLAIM,
                STUNT_REWARD_CLAIM,
                STUNT_REPEAT_CLAIM,
                STUNT_GEOMETRY_CLAIM,
            ],
            "{}",
            row.id
        );
        assert_eq!(row.dependencies[0].kind.label(), "static");
        assert_eq!(
            row.dependencies[0].provenance.claim_id.as_str(),
            CLAIM_STUNT
        );
        assert_eq!(
            row.dependencies[0].provenance.class,
            ClaimStatus::ObservedTool
        );
        assert_eq!(
            row.fingerprint.as_ref().map(|d| d.sha256),
            span.member_sha256()
        );
        assert_eq!(row.display_name, None, "{}", row.id);
    }
    let status = status_of(&baseline, ContentKind::Stunt);
    assert_eq!(status.rows, 45);
    assert_eq!(
        status.gaps.get("non_stunt_fly_through_targets"),
        Some(&9),
        "the five c1 and four c3 dogfight targets stay a counted gap"
    );
    assert_eq!(status.diagnostic, None);
    assert_eq!(
        status
            .gaps
            .get("duplicate_zone_label")
            .copied()
            .unwrap_or_default(),
        0,
        "no `stunt_flying` scenario names a zone label twice on this installation"
    );
    assert_eq!(
        status
            .gaps
            .get("ambiguous_zone_label")
            .copied()
            .unwrap_or_default(),
        0,
        "and none names one label twice with two different descriptions"
    );

    // --- scrapbook ------------------------------------------------------
    let scrapbook_rows = rows_of(&baseline, ContentKind::ScrapbookItem);
    assert_eq!(
        scrapbook_rows.len(),
        461,
        "the 461 Mission_Spread_Item records F12-D measured"
    );
    // F12-D measured the decoded member's digest; it is not the stored digest the
    // container's own listing carries.
    const SCRAPBOOK_DECODED_SHA: &str =
        "28b5144c54120f52c36717a3f1e094cb75845ecb1f854334a5686d5f6c6af5c1";
    let scrapbook_archive = cid(
        ContentKind::InstallFile,
        &install_file_key(SCRAPBOOK_CONTAINER),
    );
    for row in &scrapbook_rows {
        assert!(row.origin.is_original(), "{}", row.id);
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), SCRAPBOOK_CONTAINER, "{}", row.id);
        assert_eq!(span.member_key(), Some(SCRAPBOOK_MEMBER), "{}", row.id);
        assert_eq!(span.install_sha256().to_hex(), install_sha, "{}", row.id);
        assert_eq!(
            span.member_sha256().map(|sha| sha.to_hex()).as_deref(),
            Some(SCRAPBOOK_DECODED_SHA),
            "{}: the decoded member digest F12-D measured",
            row.id
        );
        assert_eq!(
            row.unsupported_codes(),
            vec!["not_normalized"],
            "{}",
            row.id
        );
        assert_eq!(row.dependencies.len(), 1, "{}", row.id);
        assert_eq!(row.dependencies[0].target, scrapbook_archive, "{}", row.id);
        assert_eq!(
            row.dependencies[0].provenance.claim_id.as_str(),
            SCRAPBOOK_ITEM_CLAIM,
            "{}",
            row.id
        );
        assert_eq!(row.display_name, None, "{}", row.id);
    }
    let status = status_of(&baseline, ContentKind::ScrapbookItem);
    assert_eq!(status.rows, 461);
    assert_eq!(
        status.gaps.get("entry_not_a_scrapbook_item"),
        None,
        "every scrapbook entry follows the documented Mission_Spread_Item shape, so no gap is \
         recorded"
    );
    assert_eq!(status.diagnostic, None);
    assert_eq!(
        status
            .gaps
            .get("duplicate_entry_key")
            .copied()
            .unwrap_or_default(),
        0,
        "the scrapbook table declares no entry key twice on this installation"
    );
    assert_eq!(
        status
            .gaps
            .get("ambiguous_entry_key")
            .copied()
            .unwrap_or_default(),
        0,
        "and no entry key is declared twice with two different bodies"
    );

    // --- legacy custom planes ------------------------------------------
    assert!(
        rows_of(&baseline, ContentKind::CustomPlane).is_empty(),
        "nothing about the legacy custom-plane format has been measured, so no row is guessed"
    );
    assert!(
        baseline
            .collection_status
            .iter()
            .all(|status| status.kind != ContentKind::CustomPlane)
    );

    // --- denominator unchanged ------------------------------------------
    assert_eq!(baseline.catalog.launchable_count(), baseline.roots.len());
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Stunt && id.kind() != ContentKind::ScrapbookItem)
    );

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"stunt\":45"), "{report}");
    assert!(report.contains("\"scrapbook_item\":461"), "{report}");
    assert!(!report.contains("\"custom_plane\""), "{report}");
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );
}
