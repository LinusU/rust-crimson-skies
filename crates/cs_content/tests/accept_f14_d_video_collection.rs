//! Acceptance scenario F14-D.VIDEO: the `video` collection of the retail
//! baseline inventory (task #573, stage `### F14-D` of
//! `specs/F14-canonical-content-catalog-and-dependency-closure.md`).
//!
//! `docs/contracts/IDENTITY-CONTENT.md` requires "sounds/music/dialogue/video"
//! as catalog collections and the baseline inventory had no
//! `ContentKind::Video` row at all. The installation holds **no video
//! container**: the only video files are loose `.mpg` files, so unlike the ZBD
//! sound family there is no member index that enumerates them.
//!
//! **What this stage adds.** One bounded source: the `fmv.zrd` member of the
//! shared reader archive `ZBD/zrdr.zbd`, whose `PLAYAVI` actions name the
//! videos in bytes (the decrypted engine's own strings corroborate the member
//! and the action tag). Each name is resolved against the inventory by its
//! file-name component, case-folded the way the id grammar folds case. Exactly
//! one match is one `ContentKind::Video` row whose identity is the resolved
//! file's own key, whose span is the file's own bytes with the file's digest,
//! and whose single static edge points at that file's `install_file` row.
//!
//! **What this stage refuses.** No row is ever minted from a file name: a
//! `PLAYAVI` name with no matching file, a name matching several files, a
//! repeat of a name already resolved and an `.mpg` file no name reaches are
//! each counted in `CollectionStatus::gaps`, and a `PLAYAVI` action whose
//! argument is not text counts under its own code. Nothing here decodes the
//! MPEG stream: every row is unparsed and carries an explicit unknown for
//! codec, duration, resolution and trigger. A video is not launchable, so the
//! collection cannot move the coverage denominator.
//!
//! The non-retail tests write **synthetic installation trees** into temporary
//! directories, whose reader archives are built the way the pinned format
//! readers expect, so the production `retail_baseline` really reads them.
//! Removing the video collection fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation and pins the rows it really holds. Run it with
//! `--include-ignored`; without `CS_GAME_DIR` it fails loudly.
//!
//! Every member name and every byte below is authored for this file. No
//! original content is committed.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    Baseline, CollectionStatus, FMV_ARCHIVE, FMV_MEMBER, baseline_report_json, install_file_key,
    retail_baseline,
};
use cs_types::content::{
    CatalogElement, ContentId, ContentKind, NormalizeState, Readiness, UnsupportedReason,
};
use cs_types::evidence::ClaimStatus;
use cs_types::install::ParseState;

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-video-{label}-{}-{}",
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

// ------------------------------------------------------------- .zrd -------

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
/// the children (the measured grammar: the writer stores one more than the
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

/// One authored `PLAYAVI` action: the tag and the file it names.
fn playavi(file: &str) -> Vec<u8> {
    zrd_list(vec![zrd_text("PLAYAVI"), zrd_text(file)])
}

/// An authored `fmv.zrd`: one `[sequence, actions]` pair per sequence, the
/// measured key/value shape.
fn fmv_document(sequences: &[(&str, Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut children = vec![zrd_text("IMAGE_PATH"), zrd_list(vec![zrd_text("video")])];
    for (name, actions) in sequences {
        children.push(zrd_text(name));
        children.push(zrd_list(actions.clone()));
    }
    zrd_list(children)
}

/// The per-mission members a scenario-shaped reader must list, so the campaign
/// walk finds the mission directory.
const MISSION_MEMBERS: [&str; 3] = ["map.zrd", "aiv.zrd", "objectives.zrd"];

/// A minimal installation: one campaign mission (so the shared campaign walk
/// finds a layout), the shared reader archive `FMV_ARCHIVE` holding `fmv.zrd`,
/// and any loose files the caller names.
fn tree(label: &str, fmv: Option<Vec<u8>>, loose: &[(&str, &[u8])]) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write(
        "ZBD/C1C/M01/zrdr.zbd",
        &reader_archive(&[
            ("net.zrd", zrd_int(1)),
            (MISSION_MEMBERS[0], zrd_int(1)),
            (MISSION_MEMBERS[1], zrd_int(1)),
            (MISSION_MEMBERS[2], zrd_int(1)),
        ]),
    );
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
    if let Some(document) = fmv {
        temp.write(FMV_ARCHIVE, &reader_archive(&[(FMV_MEMBER, document)]));
    }
    for (spelling, bytes) in loose {
        temp.write(spelling, bytes);
    }
    temp
}

// -------------------------------------------------------------- mapping ---

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

fn rows_of(baseline: &Baseline, kind: ContentKind) -> Vec<&CatalogElement> {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == kind)
        .collect()
}

/// Every reason a video row carries, as the claim ids or codes they report.
fn reasons(row: &CatalogElement) -> Vec<&'static str> {
    row.unsupported_reasons
        .iter()
        .map(UnsupportedReason::code)
        .collect()
}

/// The unknown reason's claim id, when the row carries one.
fn unknown_claim(row: &CatalogElement) -> Option<&str> {
    row.unsupported_reasons
        .iter()
        .find_map(|reason| match reason {
            UnsupportedReason::Unknown { claim_id, .. } => Some(claim_id.as_str()),
            _ => None,
        })
}

// ------------------------------------------------------------ the tests ---

/// A `PLAYAVI` name that resolves to exactly one inventoried file is one row,
/// located by that file's own bytes and edged onto its inventory row; an `.mpg`
/// no name reaches is a named gap and the denominator does not move.
#[test]
fn accept_f14_d_video_named_file_is_one_row() {
    let temp = tree(
        "named-file",
        Some(fmv_document(&[
            ("INTRO", vec![playavi("Intro.mpg")]),
            ("CHAP0", vec![playavi("chap0.mpg")]),
        ])),
        &[
            ("GOSDATA/ASSETS/GRAPHICS/MPG/Intro.mpg", b"intro video"),
            ("ZBD/chapters/chap0.mpg", b"chapter video"),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/crimflag.mpg", b"unnamed video"),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Video);
    assert_eq!(rows.len(), 2, "each resolved name is exactly one row");

    // The identity is the resolved file's own key, and the record's own
    // spelling stays outside identity as the display name.
    let intro = rows
        .iter()
        .find(|row| {
            row.id
                == cid(
                    ContentKind::Video,
                    &install_file_key("GOSDATA/ASSETS/GRAPHICS/MPG/Intro.mpg"),
                )
        })
        .expect("the INTRO name resolves to the Intro.mpg file");
    assert_eq!(intro.display_name.as_deref(), Some("Intro.mpg"));
    assert!(intro.origin.is_original());
    let span = intro
        .origin
        .source()
        .expect("a video row is located by a span");
    assert_eq!(
        span.container_path(),
        "GOSDATA/ASSETS/GRAPHICS/MPG/Intro.mpg"
    );
    assert_eq!(span.member_key(), None, "a loose file has no member");
    assert_eq!(span.offset(), 0);
    assert_eq!(span.length(), "intro video".len() as u64);
    assert!(
        span.member_sha256().is_some(),
        "the span carries the digest"
    );
    assert_eq!(intro.parse_state, ParseState::Unparsed);
    assert_eq!(intro.normalize_state, NormalizeState::NotNormalized);
    assert_eq!(intro.readiness, Readiness::Unavailable);
    assert!(!intro.is_ready());
    assert!(intro.runtime_consumers.is_empty());
    assert_eq!(
        reasons(intro),
        vec!["not_parsed", "unknown", "missing_runtime_consumer"]
    );
    assert_eq!(unknown_claim(intro), Some("f14.d.video.playback"));
    assert_eq!(intro.dependencies.len(), 1);
    assert_eq!(intro.dependencies[0].kind.label(), "static");
    assert_eq!(
        intro.dependencies[0].provenance.claim_id.as_str(),
        "f14.d.video.baseline.playavi"
    );
    assert_eq!(
        intro.dependencies[0].provenance.class,
        ClaimStatus::ObservedTool
    );
    // The single static edge points at the inventory row of the named file.
    assert_eq!(
        intro.dependencies[0].target,
        cid(
            ContentKind::InstallFile,
            &install_file_key("GOSDATA/ASSETS/GRAPHICS/MPG/Intro.mpg")
        )
    );

    // Resolution is by file-name component, so a name in another directory
    // still resolves — and its span is that file's own bytes.
    let chap0 = rows
        .iter()
        .find(|row| {
            row.id
                == cid(
                    ContentKind::Video,
                    &install_file_key("ZBD/chapters/chap0.mpg"),
                )
        })
        .expect("the CHAP0 name resolves to the ZBD/chapters/chap0.mpg file");
    assert_eq!(
        chap0.origin.source().expect("span").container_path(),
        "ZBD/chapters/chap0.mpg"
    );

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.source, FMV_ARCHIVE);
    assert_eq!(status.language, None);
    assert_eq!(status.rows, 2);
    assert_eq!(
        status.gaps,
        BTreeMap::from([
            ("playavi_name_not_text", 0),
            ("named_file_absent", 0),
            ("ambiguous_file_name", 0),
            ("duplicate_video_name", 0),
            // The `crimflag.mpg` file is real and stays accounted for, but
            // nothing in the original bytes names it.
            ("unnamed_video_file", 1),
        ]),
        "the only entry no name reaches is the one loose .mpg of the fixture"
    );
    assert_eq!(status.diagnostic, None);

    // The denominator did not move: a video is not launchable content.
    assert!(!ContentKind::Video.is_launchable());
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Video),
        "the video collection is not a closure root"
    );
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.coverage.roots, 1);
    assert_eq!(baseline.catalog.launchable_count(), 1);
    assert_eq!(baseline.catalog.synthetic_launchable_count(), 0);
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(
        baseline.coverage.unreachable_by_kind.get("video"),
        Some(&2),
        "both video rows are unreachable from the launchable root"
    );

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"video\":2"), "{report}");
    assert!(report.contains("\"unnamed_video_file\":1"), "{report}");
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );
}

/// Every way a name fails to become a row is counted under its own code, and
/// the name that does resolve still becomes a row beside them.
#[test]
fn accept_f14_d_video_unresolved_names_are_gaps() {
    let temp = tree(
        "unresolved",
        Some(fmv_document(&[(
            "INTRO",
            vec![
                // No inventoried file of this name.
                playavi("missing.mpg"),
                // Two inventoried files of this name, in two directories.
                playavi("both.mpg"),
                // Named twice: one row, the repeat counted.
                playavi("twice.mpg"),
                playavi("TWICE.mpg"),
                // A `PLAYAVI` action that names nothing text.
                zrd_list(vec![zrd_text("PLAYAVI"), zrd_int(7)]),
            ],
        )])),
        &[
            ("GOSDATA/ASSETS/GRAPHICS/MPG/both.mpg", b"first copy"),
            ("ZBD/chapters/both.mpg", b"second copy"),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/twice.mpg", b"one video"),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Video);
    assert_eq!(
        rows.len(),
        1,
        "only the unambiguous, once-named file becomes a row"
    );
    assert_eq!(
        rows[0].id,
        cid(
            ContentKind::Video,
            &install_file_key("GOSDATA/ASSETS/GRAPHICS/MPG/twice.mpg")
        )
    );

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 1);
    assert_eq!(
        status.gaps,
        BTreeMap::from([
            ("playavi_name_not_text", 1),
            ("named_file_absent", 1),
            ("ambiguous_file_name", 2),
            ("duplicate_video_name", 1),
            ("unnamed_video_file", 2),
        ]),
        "each failure keeps its own code, and both `both.mpg` files stay named \
         as ambiguous rather than being picked or dropped"
    );
    assert_eq!(status.diagnostic, None);

    // The two ambiguous files and the absent name built no row at all: no
    // identity may be minted where the bytes name several files or none.
    assert!(
        rows.iter()
            .all(|row| row.display_name.as_deref() == Some("twice.mpg")),
        "the first spelling of the resolved file is the display name kept; the \
         second spelling of the same file is a counted repeat"
    );
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.coverage.roots, 1);
}

/// A shared archive without the FMV member leaves the collection with a named
/// diagnostic and no rows — the collection is reported, never absent, and no
/// row is minted from the loose `.mpg` files the installation still holds.
#[test]
fn accept_f14_d_video_missing_member_is_diagnostic() {
    let temp = tree(
        "no-member",
        Some(reader_archive(&[("net.zrd", zrd_int(1))])),
        &[("GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg", b"video bytes")],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(rows_of(&baseline, ContentKind::Video).is_empty());
    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 0);
    assert_eq!(status.source, FMV_ARCHIVE);
    assert!(
        status.gaps.is_empty(),
        "nothing was read, so nothing was counted"
    );
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("the collection names why it holds no rows");
    assert!(
        diagnostic.contains(FMV_MEMBER) && diagnostic.contains(FMV_ARCHIVE),
        "{diagnostic}"
    );
    // The loose `.mpg` file is still inventoried as a file row: no video row
    // came from its name.
    assert!(
        rows_of(&baseline, ContentKind::InstallFile)
            .iter()
            .any(|row| row.display_name.as_deref() == Some("GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg")),
        "the file stays an inventory row"
    );
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
}

/// An FMV member that does not decode leaves the collection with a named
/// diagnostic rather than guessed rows.
#[test]
fn accept_f14_d_video_undecodable_member_is_diagnostic() {
    let temp = tree(
        "undecodable",
        // Tag `9` is outside the four measured kinds, so the member does not
        // decode as a `.zrd` document at all.
        Some(9_u32.to_le_bytes().to_vec()),
        &[("GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg", b"video bytes")],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(rows_of(&baseline, ContentKind::Video).is_empty());
    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 0);
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("the collection names why it holds no rows");
    assert!(diagnostic.contains("does not decode"), "{diagnostic}");
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
}

/// An FMV member with no `PLAYAVI` action names no video: the collection says
/// so instead of being minted from the loose files.
#[test]
fn accept_f14_d_video_no_action_is_diagnostic() {
    let temp = tree(
        "no-action",
        Some(fmv_document(&[(
            "INTRO",
            vec![zrd_list(vec![zrd_text("WAIT"), zrd_int(1)])],
        )])),
        &[("GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg", b"video bytes")],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(rows_of(&baseline, ContentKind::Video).is_empty());
    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 0);
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("the collection names why it holds no rows");
    assert!(diagnostic.contains("PLAYAVI"), "{diagnostic}");
}

// --------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The row counts the retail acceptance pins, measured on the owner's
/// installation and recorded in
/// `docs/findings/2026-10-12-f14-d-video-collection.md`.
///
/// `ZBD/zrdr.zbd`'s `fmv.zrd` member holds three `PLAYAVI` actions —
/// `MSopen1.mpg` and `zipper.mpg` of the `INTRO` sequence and `Chap0.mpg` of
/// the `CHAP0` sequence — and each resolves to exactly one of the ten loose
/// `.mpg` files under `GOSDATA/ASSETS/GRAPHICS/MPG/`. The other seven
/// (`chap1`…`chap5`, `crimflag`, `final`) are named nowhere in original bytes.
const RETAIL_VIDEO_ROWS: usize = 3;
const RETAIL_UNNAMED_MPG_FILES: usize = 7;

/// The retail half: the three files the FMV definitions name in bytes become
/// rows, the seven no byte names stay named gaps, and the coverage denominator
/// is unchanged.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_video_retail_videos_are_rows() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let catalog = &baseline.catalog;

    let rows = rows_of(&baseline, ContentKind::Video);
    assert_eq!(
        rows.len(),
        RETAIL_VIDEO_ROWS,
        "the three files fmv.zrd names are the three rows; a different count must be \
         re-measured and this test updated, never silently accepted"
    );

    let mut named = Vec::new();
    for row in &rows {
        assert!(
            row.origin.is_original(),
            "{} must be installation data",
            row.id
        );
        let span = row
            .origin
            .source()
            .expect("a video row is located by a span");
        assert_eq!(
            span.container_path().to_ascii_lowercase(),
            format!(
                "gosdata/assets/graphics/mpg/{}",
                row.display_name
                    .as_deref()
                    .expect("a video row carries the record's spelling")
                    .to_ascii_lowercase()
            ),
            "{}: the row's span is the named file's own bytes",
            row.id
        );
        assert_eq!(span.offset(), 0);
        assert!(span.length() > 0, "{}: the file has bytes", row.id);
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert_eq!(
            span.member_sha256().map(|digest| digest.to_hex()),
            Some(
                cs_assets::install::sha256(
                    &fs::read(dir.join(span.container_path())).expect("the named file reads")
                )
                .to_hex()
            ),
            "{}: the span's digest is the file's own bytes",
            row.id
        );
        assert_eq!(row.parse_state, ParseState::Unparsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(!row.is_ready());
        assert_eq!(
            reasons(row),
            vec!["not_parsed", "unknown", "missing_runtime_consumer"]
        );
        assert_eq!(
            unknown_claim(row),
            Some("f14.d.video.playback"),
            "{}: codec, duration, resolution and trigger stay unknown",
            row.id
        );
        assert!(row.runtime_consumers.is_empty());
        assert_eq!(row.dependencies.len(), 1);
        assert_eq!(row.dependencies[0].kind.label(), "static");
        assert_eq!(
            row.dependencies[0].provenance.claim_id.as_str(),
            "f14.d.video.baseline.playavi"
        );
        assert_eq!(
            row.dependencies[0].provenance.class,
            ClaimStatus::ObservedTool
        );
        assert_eq!(
            row.dependencies[0].target,
            cid(
                ContentKind::InstallFile,
                &install_file_key(span.container_path())
            ),
            "{}: one static edge onto the named file's inventory row",
            row.id
        );
        named.push(
            row.display_name
                .as_deref()
                .expect("a video row carries the record's spelling")
                .to_ascii_lowercase(),
        );
    }
    named.sort();
    assert_eq!(named, vec!["chap0.mpg", "msopen1.mpg", "zipper.mpg"]);

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.source, FMV_ARCHIVE);
    assert_eq!(status.language, None);
    assert_eq!(status.rows, RETAIL_VIDEO_ROWS);
    assert_eq!(
        status.gaps,
        BTreeMap::from([
            ("playavi_name_not_text", 0),
            ("named_file_absent", 0),
            ("ambiguous_file_name", 0),
            ("duplicate_video_name", 0),
            ("unnamed_video_file", RETAIL_UNNAMED_MPG_FILES),
        ]),
        "the seven .mpg files no PLAYAVI name reaches are the only unaccounted-for \
         entries; a new gap code here means the measured source changed and this \
         measurement has to be re-measured"
    );
    assert_eq!(status.diagnostic, None);

    // The denominator did not move: a video is not launchable content.
    assert!(!ContentKind::Video.is_launchable());
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Video),
        "the video collection is not a closure root"
    );
    assert_eq!(catalog.launchable_count(), baseline.roots.len());
    assert_eq!(catalog.synthetic_launchable_count(), 0);
    assert_eq!(baseline.coverage.roots, baseline.roots.len());
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(
        baseline.coverage.unreachable_by_kind.get("video"),
        Some(&RETAIL_VIDEO_ROWS),
        "every video row is unreachable from a launchable root"
    );

    let report = baseline_report_json(&baseline);
    assert!(
        report.contains(&format!("\"video\":{RETAIL_VIDEO_ROWS}")),
        "{report}"
    );
    assert!(
        report.contains(&format!(
            "\"unnamed_video_file\":{RETAIL_UNNAMED_MPG_FILES}"
        )),
        "{report}"
    );
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );
}
