//! Acceptance scenario F14-D.5: the `faction` and `paint_mask` collections of
//! the retail baseline inventory (task #488, stage `### F14-D` of
//! `specs/F14-canonical-content-catalog-and-dependency-closure.md`).
//!
//! `docs/contracts/IDENTITY-CONTENT.md` requires "blueprints and faction paint
//! masks" and "pilot/voice/faction relations" as catalog collections. The
//! baseline inventory had no `ContentKind::Faction` or `ContentKind::PaintMask`
//! row at all.
//!
//! **What this stage adds.**
//!
//! * One `ContentKind::Faction` row per paint pattern the paint records of
//!   `ZBD/zrdr.zbd`'s `vehicle.zrd` member **name in bytes** (F09-PALETTE's
//!   `FactionPaletteCatalog`). The identity is that byte-named `paint_pattern`,
//!   never a file or directory name, and the row is located by the pattern
//!   field's own checked span; its single static edge points at the inventory
//!   row of the archive holding the member.
//! * One `ContentKind::PaintMask` row per `.bm` member of
//!   `GOSDATA/ASSETS/crimson.rof` that the producing stage's verifier
//!   (`StockLiveryCatalog`) read and verified. The identity is the escaped
//!   member spelling and the span is the member's stored extent (container path
//!   plus member key).
//!
//! **What this stage refuses.** The faction **directory** a `.bm` member sits in
//! is not byte-backed content (the 2026-10-03 F09-PAINTSHOP finding records that
//! the directory-to-pattern binding is engine-internal), so it is never an
//! identity and no member-to-faction edge is minted. A paint record that names a
//! pattern without a complete palette is a named gap, not a guessed faction row.
//! A `.bm` member that fails verification is a named gap, not a row.
//!
//! The non-retail tests write **synthetic installation trees** into temporary
//! directories, whose reader archives and ROF container are built the way the
//! pinned format readers expect, so the production `retail_baseline` really
//! reads them. Removing the two collections fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation and pins the rows it really holds. Run it with
//! `--include-ignored`; without `CS_GAME_DIR` it fails loudly.
//!
//! Every member name and every byte below is authored for this file. No
//! original content is committed.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    Baseline, CollectionStatus, PAINT_MASK_CONTAINER, baseline_report_json, install_file_key,
    retail_baseline,
};
use cs_content::livery::{
    PALETTE_CONTAINER as FACTION_PALETTE_CONTAINER, PALETTE_MEMBER as FACTION_PALETTE_MEMBER,
};
use cs_formats::zbd::{
    INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES, TRAILER_VERSION_ONE,
};
use cs_types::content::{
    ContentId, ContentKind, NormalizeState, Origin, Readiness, UnsupportedReason,
};
use cs_types::evidence::ClaimStatus;
use cs_types::install::ParseState;

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-5-{label}-{}-{}",
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

/// One version-one reader archive holding `members`, written as the pinned
/// reader expects: member data, 148-byte index entries, trailer.
fn reader_archive(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for (name, bytes) in members {
        entries.extend_from_slice(&u32::try_from(data.len()).expect("fits").to_le_bytes());
        entries.extend_from_slice(&u32::try_from(bytes.len()).expect("fits").to_le_bytes());
        let mut field = vec![0u8; INDEX_NAME_BYTES];
        field[..name.len()].copy_from_slice(name.as_bytes());
        entries.extend_from_slice(&field);
        entries.extend_from_slice(&[0u8; INDEX_UNEXPLAINED_BYTES]);
        data.extend_from_slice(bytes);
    }
    assert_eq!(entries.len(), members.len() * INDEX_ENTRY_BYTES as usize);
    data.extend_from_slice(&entries);
    data.extend_from_slice(&TRAILER_VERSION_ONE.to_le_bytes());
    data.extend_from_slice(&u32::try_from(members.len()).expect("fits").to_le_bytes());
    data
}

// ------------------------------------------------------------ the .zrd ---

const ZRD_TAG_INT: u32 = 1;
const ZRD_TAG_TEXT: u32 = 3;
const ZRD_TAG_LIST: u32 = 4;

fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = ZRD_TAG_INT.to_le_bytes().to_vec();
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = ZRD_TAG_TEXT.to_le_bytes().to_vec();
    bytes.extend_from_slice(&u32::try_from(text.len()).expect("fits").to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// One `T4` list node; its count word is `len + 1`.
fn zrd_list(children: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = ZRD_TAG_LIST.to_le_bytes().to_vec();
    bytes.extend_from_slice(
        &u32::try_from(children.len() + 1)
            .expect("fits")
            .to_le_bytes(),
    );
    for child in children {
        bytes.extend_from_slice(child);
    }
    bytes
}

/// One paint record carrying a pattern, a color triple and a decal triple.
fn zrd_paint_record(pattern: &str, colors: [[u8; 3]; 3], decals: [u32; 3]) -> Vec<u8> {
    let mut children = vec![zrd_text("paint_pattern"), zrd_list(&[zrd_text(pattern)])];
    for (slot, color) in colors.iter().enumerate() {
        children.push(zrd_text(&format!("paint_color{}", slot + 1)));
        children.push(zrd_list(&[
            zrd_int(u32::from(color[0])),
            zrd_int(u32::from(color[1])),
            zrd_int(u32::from(color[2])),
        ]));
    }
    for (slot, decal) in decals.iter().enumerate() {
        children.push(zrd_text(&format!("paint_decal{}", slot + 1)));
        children.push(zrd_list(&[zrd_int(*decal)]));
    }
    zrd_list(&children)
}

/// One paint record carrying a pattern but no colors or decals.
fn zrd_pattern_only_record(pattern: &str) -> Vec<u8> {
    zrd_list(&[zrd_text("paint_pattern"), zrd_list(&[zrd_text(pattern)])])
}

/// A whole `vehicle.zrd`: a root list whose only child alternates record-name
/// text and record list.
fn vehicle_zrd(records: &[(&str, &[u8])]) -> Vec<u8> {
    let mut top = Vec::new();
    for (name, record) in records {
        top.push(zrd_text(name));
        top.push(record.to_vec());
    }
    zrd_list(&[zrd_list(&top)])
}

// --------------------------------------------------------------- the ROF ---

/// One ROF directory block: header, 24-byte records and the NUL-separated name
/// table, in record order.
fn rof_block(records: &[[u32; 6]], names: &[u8]) -> Vec<u8> {
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

fn rof_names(names: &[&str]) -> Vec<u8> {
    let mut table = Vec::new();
    for name in names {
        table.extend_from_slice(name.as_bytes());
        table.push(0);
    }
    table
}

const ROF_RECORD_BYTES: usize = 24;
const ROF_HEADER_BYTES: usize = 8;

/// One faction directory of an airframe-library fixture: its directory name and
/// its `.bm` members as `(name, bytes)`.
type FixtureFaction<'a> = (&'a str, &'a [(&'a str, &'a [u8])]);

/// A ROF container whose root holds `GRAPHICS`, which holds one directory per
/// faction, each holding its `.bm` files. The layout is
/// `GRAPHICS/<FACTION>/<PREFIX>_<PART>.BM`, the observed stock-livery spelling.
fn airframe_library(factions: &[FixtureFaction<'_>]) -> Vec<u8> {
    let root_names = rof_names(&["GRAPHICS"]);
    let root_len = ROF_HEADER_BYTES + ROF_RECORD_BYTES + root_names.len();

    let faction_names: Vec<&str> = factions.iter().map(|(faction, _)| *faction).collect();
    let graphics_names = rof_names(&faction_names);
    let graphics_len = ROF_HEADER_BYTES + ROF_RECORD_BYTES * factions.len() + graphics_names.len();

    let mut block_lengths = Vec::new();
    let mut block_names = Vec::new();
    for (_, files) in factions {
        let names: Vec<&str> = files.iter().map(|(name, _)| *name).collect();
        let table = rof_names(&names);
        block_lengths.push(ROF_HEADER_BYTES + ROF_RECORD_BYTES * files.len() + table.len());
        block_names.push(table);
    }

    let mut offset = root_len + graphics_len;
    let mut block_offset = Vec::new();
    for length in &block_lengths {
        block_offset.push(offset);
        offset += length;
    }
    let mut cursor = offset;

    let mut container = Vec::new();
    container.extend_from_slice(&rof_block(
        &[[root_len as u32, 0, 0, 1, "GRAPHICS".len() as u32 + 1, 1]],
        &root_names,
    ));
    let mut graphics_records = Vec::new();
    for (index, (faction, _)) in factions.iter().enumerate() {
        graphics_records.push([
            block_offset[index] as u32,
            0,
            0,
            1,
            faction.len() as u32 + 1,
            2 + index as u32,
        ]);
    }
    container.extend_from_slice(&rof_block(&graphics_records, &graphics_names));

    for (index, (_, files)) in factions.iter().enumerate() {
        let mut records = Vec::new();
        for (slot, (name, bytes)) in files.iter().enumerate() {
            let length = u32::try_from(bytes.len()).expect("a fixture payload fits");
            records.push([
                cursor as u32,
                length,
                length,
                0,
                name.len() as u32 + 1,
                100 + index as u32 * 10 + slot as u32,
            ]);
            cursor += bytes.len();
        }
        container.extend_from_slice(&rof_block(&records, &block_names[index]));
    }
    for (_, files) in factions {
        for (_, bytes) in files.iter() {
            container.extend_from_slice(bytes);
        }
    }
    assert_eq!(container.len(), cursor, "every payload was placed once");
    container
}

/// One valid 1x1 BM: RGB base, three one-byte masks, RGBA overlay.
fn bm_1x1() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes()); // height
    bytes.extend_from_slice(&1u16.to_le_bytes()); // width
    bytes.extend_from_slice(&[9, 8, 7]); // base
    for mask in [[10u8], [20u8], [30u8]] {
        bytes.extend_from_slice(&mask);
    }
    bytes.extend_from_slice(&[1, 2, 3, 200]); // overlay
    bytes
}

// ------------------------------------------------------------ the tree ---

/// The per-mission members a scenario-shaped reader must list, so the campaign
/// walk finds the mission directory.
const MISSION_MEMBERS: [&str; 3] = ["map.zrd", "aiv.zrd", "objectives.zrd"];

/// A minimal installation: one campaign mission (so the shared campaign walk
/// finds a layout) plus an optional palette archive and airframe library.
fn tree(label: &str, palette: Option<&[u8]>, library: Option<&[u8]>) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write(
        "ZBD/C1C/M01/zrdr.zbd",
        &reader_archive(&[
            ("net.zrd", b"shared"),
            (MISSION_MEMBERS[0], b"map"),
            (MISSION_MEMBERS[1], b"aiv"),
            (MISSION_MEMBERS[2], b"objectives"),
        ]),
    );
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
    if let Some(palette) = palette {
        temp.write(FACTION_PALETTE_CONTAINER, palette);
    }
    if let Some(library) = library {
        temp.write(PAINT_MASK_CONTAINER, library);
    }
    temp
}

/// A palette archive naming two complete factions and one pattern-only record.
fn two_faction_palette() -> Vec<u8> {
    let medusas = zrd_paint_record(
        "medusas",
        [[95, 125, 143], [41, 14, 21], [141, 137, 93]],
        [21, 14, 14],
    );
    let british = zrd_paint_record(
        "british",
        [[177, 130, 66], [48, 47, 39], [255, 255, 255]],
        [21, 4, 4],
    );
    let fortune = zrd_pattern_only_record("player_fortune");
    let zrd = vehicle_zrd(&[
        ("medkestrel", &medusas),
        ("britpeace", &british),
        ("devastator", &fortune),
    ]);
    reader_archive(&[(FACTION_PALETTE_MEMBER, &zrd)])
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

fn status_of(baseline: &Baseline, kind: ContentKind) -> &CollectionStatus {
    baseline
        .collection_status
        .iter()
        .find(|status| status.kind == kind)
        .unwrap_or_else(|| panic!("the {kind} collection reports its status"))
}

fn rows_of(baseline: &Baseline, kind: ContentKind) -> Vec<&cs_types::content::CatalogElement> {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == kind)
        .collect()
}

// -------------------------------------------------------------- mapping ---

/// Every verified `.bm` member becomes a paint-mask row, keyed by the escaped
/// member spelling, located by the member's stored extent and pointing at the
/// airframe library's inventory row.
#[test]
fn accept_f14_d_5_a_verified_member_yields_a_paint_mask_row() {
    let library = airframe_library(&[
        (
            "BLACKHAT",
            &[
                ("AGYRO_FUSALAGE1.BM", &bm_1x1()),
                ("AGYRO_WING1.BM", &bm_1x1()),
            ],
        ),
        ("MEDUSAS", &[("AGYRO_FUSALAGE1.BM", &bm_1x1())]),
    ]);
    let temp = tree("messages", Some(&two_faction_palette()), Some(&library));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::PaintMask);
    assert_eq!(rows.len(), 3, "one row per verified BM member");
    let expected: Vec<ContentId> = [
        "GRAPHICS/BLACKHAT/AGYRO_FUSALAGE1.BM",
        "GRAPHICS/BLACKHAT/AGYRO_WING1.BM",
        "GRAPHICS/MEDUSAS/AGYRO_FUSALAGE1.BM",
    ]
    .iter()
    .map(|spelling| cid(ContentKind::PaintMask, &install_file_key(spelling)))
    .collect();
    assert_eq!(
        rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
        expected,
        "identity is the escaped member spelling, in canonical id order"
    );

    let library_row = cid(
        ContentKind::InstallFile,
        &install_file_key(PAINT_MASK_CONTAINER),
    );
    let blob = fs::read(temp.0.join(PAINT_MASK_CONTAINER)).expect("the library bytes read");
    for (row, spelling) in rows.iter().zip([
        "GRAPHICS/BLACKHAT/AGYRO_FUSALAGE1.BM",
        "GRAPHICS/BLACKHAT/AGYRO_WING1.BM",
        "GRAPHICS/MEDUSAS/AGYRO_FUSALAGE1.BM",
    ]) {
        // The row's own bytes: the member's stored extent inside the library.
        assert!(matches!(row.origin, Origin::Installation { .. }));
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), PAINT_MASK_CONTAINER);
        assert_eq!(span.member_key(), Some(spelling));
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert!(span.length() > 0, "{}: a BM member has bytes", row.id);
        let offset = usize::try_from(span.offset()).expect("fits");
        let length = usize::try_from(span.length()).expect("fits");
        assert_eq!(
            &blob[offset..offset + length],
            &bm_1x1()[..length],
            "{} names the member's stored bytes",
            row.id
        );
        assert_eq!(
            row.fingerprint.as_ref().map(|digest| digest.sha256),
            Some(cs_assets::install::sha256(&blob[offset..offset + length])),
            "{} fingerprints the member's stored extent",
            row.id
        );

        // The bytes are verified, so the row is parsed but not normalized.
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert_eq!(
            row.unsupported_reasons,
            vec![UnsupportedReason::NotNormalized]
        );
        assert!(row.runtime_consumers.is_empty());

        // The one static edge onto the library's inventory row.
        assert_eq!(row.dependencies.len(), 1);
        assert_eq!(row.dependencies[0].target, library_row);
        assert_eq!(row.dependencies[0].kind.label(), "static");
        assert_eq!(
            row.dependencies[0].provenance.claim_id.as_str(),
            "f14.d.5.baseline.paint_mask_member"
        );
        assert_eq!(
            row.dependencies[0].provenance.class,
            ClaimStatus::ObservedTool,
            "an agent-observed edge is never verified_original"
        );

        // The faction directory is not identity and no member-to-faction edge
        // is minted.
        assert!(
            row.dependencies
                .iter()
                .all(|edge| edge.target.kind() != ContentKind::Faction),
            "{} must not carry a filename-derived faction edge",
            row.id
        );
    }

    let status = status_of(&baseline, ContentKind::PaintMask);
    assert_eq!(status.source, PAINT_MASK_CONTAINER);
    assert_eq!(status.language, None);
    assert_eq!(status.rows, 3);
    assert!(status.gaps.is_empty(), "every member verified");
    assert_eq!(status.diagnostic, None);

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"paint_mask\":3"), "{report}");
    assert!(
        report.contains(&format!(
            "\"kind\":\"paint_mask\",\"source\":\"{PAINT_MASK_CONTAINER}\",\"language\":null,\"rows\":3"
        )),
        "{report}"
    );
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "the retail consumer report holds no authored row"
    );
    assert_eq!(
        report,
        baseline_report_json(&retail_baseline(&temp.0).expect("re-read")),
        "the report is byte-stable for the same installation"
    );
}

/// A faction row's identity is a pattern the paint records name in bytes, never
/// a filename or a directory name, and its edge points at the archive holding
/// those bytes.
#[test]
fn accept_f14_d_5_a_byte_named_pattern_yields_a_faction_row() {
    let library = airframe_library(&[("ZED", &[("AAA_B.BM", &bm_1x1())])]);
    let temp = tree("factions", Some(&two_faction_palette()), Some(&library));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Faction);
    assert_eq!(
        rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
        vec![
            cid(ContentKind::Faction, "british"),
            cid(ContentKind::Faction, "medusas"),
        ],
        "one row per complete paint pattern, in canonical order"
    );
    assert!(
        baseline
            .catalog
            .get(&cid(ContentKind::Faction, "zed"))
            .is_none(),
        "a member directory name is not a faction"
    );

    let archive = cid(
        ContentKind::InstallFile,
        &install_file_key(FACTION_PALETTE_CONTAINER),
    );
    for row in &rows {
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), FACTION_PALETTE_CONTAINER);
        assert_eq!(
            span.member_key(),
            Some(FACTION_PALETTE_MEMBER),
            "the row is located inside the archive's own member"
        );
        assert!(span.length() > 0);
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert_eq!(
            row.display_name.as_deref(),
            Some(row.id.key()),
            "the byte-named pattern is also the display name"
        );

        assert!(matches!(row.origin, Origin::Installation { .. }));
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert_eq!(
            row.unsupported_reasons,
            vec![UnsupportedReason::NotNormalized]
        );
        assert_eq!(row.dependencies.len(), 1);
        assert_eq!(row.dependencies[0].target, archive);
        assert_eq!(row.dependencies[0].kind.label(), "static");
        assert_eq!(
            row.dependencies[0].provenance.claim_id.as_str(),
            "f14.d.5.baseline.faction_pattern"
        );
        assert_eq!(
            row.dependencies[0].provenance.class,
            ClaimStatus::ObservedTool
        );
    }

    // The pattern-only record is a named gap, not a guessed faction row.
    let status = status_of(&baseline, ContentKind::Faction);
    assert_eq!(status.source, FACTION_PALETTE_CONTAINER);
    assert_eq!(status.language, None);
    assert_eq!(status.rows, 2);
    assert_eq!(status.gaps.get("pattern_without_colors"), Some(&1));
    assert_eq!(status.diagnostic, None);

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"faction\":2"), "{report}");
    assert!(report.contains("\"pattern_without_colors\":1"), "{report}");
}

/// A `.bm` member the verifier cannot read or parse is a named gap, not a row.
#[test]
fn accept_f14_d_5_a_member_that_fails_verification_is_a_gap_not_a_row() {
    let library = airframe_library(&[(
        "BLACKHAT",
        &[
            ("AGYRO_FUSALAGE1.BM", &bm_1x1()),
            // Not `GRAPHICS/<FACTION>/<PREFIX>_<PART>.BM`: a finding.
            ("NOPREFIX.BM", &bm_1x1()),
            // The livery layout but not a parseable BM.
            ("AGYRO_FUSALAGE2.BM", b"not a bm at all"),
        ],
    )]);
    let temp = tree("gaps", None, Some(&library));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert_eq!(
        rows_of(&baseline, ContentKind::PaintMask).len(),
        1,
        "only the verified member is a row"
    );
    let status = status_of(&baseline, ContentKind::PaintMask);
    assert_eq!(status.rows, 1);
    assert_eq!(status.gaps.get("not_a_stock_livery"), Some(&1));
    assert_eq!(
        status.gaps.get("unexpected_eof").copied().unwrap_or(0)
            + status.gaps.get("empty_image").copied().unwrap_or(0),
        1,
        "the unparseable BM is counted under the parser's own code: {:?}",
        status.gaps
    );
    assert_eq!(status.diagnostic, None);
}

/// A missing or unreadable source is a named gap in the collection record, never
/// a silently empty collection and never a guessed row.
#[test]
fn accept_f14_d_5_a_missing_source_is_a_named_gap_not_an_empty_reading() {
    let temp = tree("missing", None, None);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    for kind in [ContentKind::Faction, ContentKind::PaintMask] {
        assert!(rows_of(&baseline, kind).is_empty());
        let status = status_of(&baseline, kind);
        assert_eq!(status.rows, 0);
        let diagnostic = status
            .diagnostic
            .as_deref()
            .expect("a missing source is named, not dropped");
        assert!(diagnostic.contains("inventories no"), "{diagnostic}");
    }
    assert_eq!(
        status_of(&baseline, ContentKind::Faction).source,
        FACTION_PALETTE_CONTAINER
    );
    assert_eq!(
        status_of(&baseline, ContentKind::PaintMask).source,
        PAINT_MASK_CONTAINER
    );

    let report = baseline_report_json(&baseline);
    assert!(!report.contains("\"faction\":"), "{report}");
    assert!(!report.contains("\"paint_mask\":"), "{report}");
    assert!(report.contains("\"kind\":\"faction\""), "{report}");
    assert!(report.contains("\"kind\":\"paint_mask\""), "{report}");
}

/// A source that is present but does not read as the observed layout is a
/// diagnostic, and it does not take the rest of the inventory with it.
#[test]
fn accept_f14_d_5_a_refused_source_is_a_diagnostic_and_does_not_take_the_missions() {
    let temp = tree("refused", Some(b"not a reader archive"), Some(b"not a rof"));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(rows_of(&baseline, ContentKind::Faction).is_empty());
    assert!(rows_of(&baseline, ContentKind::PaintMask).is_empty());
    let faction = status_of(&baseline, ContentKind::Faction);
    assert!(
        faction
            .diagnostic
            .as_deref()
            .is_some_and(|text| text.contains("do not read")),
        "{:?}",
        faction.diagnostic
    );
    let paint_mask = status_of(&baseline, ContentKind::PaintMask);
    assert!(
        paint_mask
            .diagnostic
            .as_deref()
            .is_some_and(|text| text.contains("does not mount")),
        "{:?}",
        paint_mask.diagnostic
    );

    // The rest of the inventory is unchanged: the collection's failure does not
    // take the campaign mission with it.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.catalog.launchable_count(), 1);
}

/// Neither kind is launchable, so this stage adds no root and cannot move the
/// coverage denominator; its rows stay visible as unreachable unknowns.
#[test]
fn accept_f14_d_5_neither_collection_is_launchable_and_the_denominator_does_not_move() {
    let library = airframe_library(&[("BLACKHAT", &[("AAA_B.BM", &bm_1x1())])]);
    let temp = tree("roots", Some(&two_faction_palette()), Some(&library));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(!ContentKind::Faction.is_launchable());
    assert!(!ContentKind::PaintMask.is_launchable());
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.catalog.launchable_count(), 1);
    assert_eq!(baseline.catalog.synthetic_launchable_count(), 0);
    assert_eq!(baseline.coverage.roots, 1);
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Faction && id.kind() != ContentKind::PaintMask),
        "neither collection declares a closure root"
    );
    assert_eq!(
        baseline
            .coverage
            .unreachable_by_kind
            .get("paint_mask")
            .copied(),
        Some(1)
    );
    assert_eq!(
        baseline
            .coverage
            .unreachable_by_kind
            .get("faction")
            .copied(),
        Some(2)
    );
}

// ------------------------------------------------------------- retail ----

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The 11 paint patterns the original `vehicle.zrd` names with a complete
/// palette, pinned by the F09-PALETTE retail test.
const RETAIL_PALETTE_NAMES: [&str; 11] = [
    "blackhat", "blake", "blckswan", "british", "cccp", "german", "hollywd", "hughes", "medusas",
    "sactrust", "studio",
];

/// The retail half: the airframe library's verified `.bm` members and the
/// original faction paint patterns become rows, each located by its own bytes,
/// and the coverage denominator is unchanged.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_5_retail_paint_masks_and_factions_are_rows() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let catalog = &baseline.catalog;

    // Paint masks: one row per verified BM member (F09-D measured 184).
    let masks = rows_of(&baseline, ContentKind::PaintMask);
    assert_eq!(
        masks.len(),
        184,
        "the airframe library holds 184 BM members"
    );
    for row in &masks {
        assert!(
            row.origin.is_original(),
            "{} must be installation data",
            row.id
        );
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), PAINT_MASK_CONTAINER);
        assert!(
            span.member_key()
                .is_some_and(|member| member.to_ascii_lowercase().ends_with(".bm")),
            "{}: a paint mask names its .bm member",
            row.id
        );
        assert!(span.length() > 0);
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(!row.is_ready());
        assert_eq!(row.dependencies.len(), 1);
        assert_eq!(
            row.dependencies[0].provenance.claim_id.as_str(),
            "f14.d.5.baseline.paint_mask_member"
        );
        assert!(
            row.dependencies
                .iter()
                .all(|edge| edge.target.kind() != ContentKind::Faction),
            "{} must not carry a filename-derived faction edge",
            row.id
        );
    }
    // The library's own inventory row is the edge target of every mask.
    let library_row = cid(
        ContentKind::InstallFile,
        &install_file_key(PAINT_MASK_CONTAINER),
    );
    assert_eq!(
        masks
            .iter()
            .filter(|row| row.dependencies[0].target == library_row)
            .count(),
        masks.len()
    );
    let mask_status = status_of(&baseline, ContentKind::PaintMask);
    assert_eq!(mask_status.source, PAINT_MASK_CONTAINER);
    assert_eq!(mask_status.rows, 184);
    assert!(
        mask_status.gaps.is_empty(),
        "every original member verifies: {:?}",
        mask_status.gaps
    );
    assert_eq!(mask_status.diagnostic, None);

    // Factions: the 11 byte-named paint patterns, and no filename-derived row.
    let factions = rows_of(&baseline, ContentKind::Faction);
    assert_eq!(
        factions
            .iter()
            .map(|row| row.id.key().to_owned())
            .collect::<Vec<_>>(),
        RETAIL_PALETTE_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>(),
        "the 11 original faction paint patterns, in canonical order"
    );
    for row in &factions {
        assert!(row.origin.is_original());
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), FACTION_PALETTE_CONTAINER);
        assert_eq!(span.member_key(), Some(FACTION_PALETTE_MEMBER));
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert_eq!(row.dependencies.len(), 1);
    }
    let faction_status = status_of(&baseline, ContentKind::Faction);
    assert_eq!(faction_status.source, FACTION_PALETTE_CONTAINER);
    assert_eq!(faction_status.rows, 11);
    assert_eq!(
        faction_status.gaps.get("pattern_without_colors"),
        Some(&2),
        "the two player_fortune records name a pattern but store no palette"
    );
    assert_eq!(faction_status.diagnostic, None);

    // The denominator did not move: neither kind is launchable content.
    assert!(!ContentKind::Faction.is_launchable());
    assert!(!ContentKind::PaintMask.is_launchable());
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Faction && id.kind() != ContentKind::PaintMask),
        "neither collection is a closure root"
    );
    assert_eq!(catalog.launchable_count(), baseline.roots.len());
    assert_eq!(catalog.synthetic_launchable_count(), 0);
    assert!(!catalog.is_fully_ready() || baseline.roots.is_empty());

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"paint_mask\":184"), "{report}");
    assert!(report.contains("\"faction\":11"), "{report}");
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );
}
