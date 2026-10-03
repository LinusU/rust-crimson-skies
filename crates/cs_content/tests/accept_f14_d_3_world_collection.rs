//! Acceptance scenario F14-D.3: the `world` collection of the retail baseline
//! inventory (follow-up task #486 of F14-D.2 / #389; stage `### F14-D` of
//! `specs/F14-canonical-content-catalog-and-dependency-closure.md`).
//!
//! `docs/contracts/IDENTITY-CONTENT.md` requires "world groups and variants" as
//! a catalog collection. The baseline inventory had no `ContentKind::World` row
//! at all, so its report's `collections` object had no `world` entry.
//!
//! **What this stage adds.** One `ContentKind::World` row per world group that
//! the *producing* stage's own parser can name from original bytes: the
//! world-group reader (`ZBD/<group>/zrdr.zbd`) whose **own member index**
//! F14-D.1's `reader_dirs::classify` read (F14-D.1's measured rule: a declared
//! group, `templates.zrd` + `cam_anim.zrd`, no per-mission member, no
//! `mis_anim.zbd` beside it). Each row carries `Origin::Installation` over a
//! checked span of that archive, the identity `world/<group>` — the group
//! directory, lowercased, which is the same derivation
//! `cs_content::campaign_bindings` uses for a mission binding's `world` row —
//! and one `Static` edge onto the **inventory row of the reader archive whose
//! member index named it**, with `observed_tool` provenance.
//!
//! **What this stage refuses.** A world-group reader whose member index cannot
//! be read classifies nothing (F14-D.1), so its group yields *no* row: the group
//! is counted in the collection record's `declared_group_without_reader` gap and
//! stays named in `Baseline::unrecognized_program_dirs`. A world row is never
//! minted from a directory name alone.
//!
//! The non-retail tests write **synthetic installation trees** into temporary
//! directories, whose reader archives are valid version-one archives built the
//! way the pinned format reader expects (member data, 148-byte index entries,
//! trailer) so the production classifier really reads them. They exercise
//! production code only — `cs_content::catalog::baseline::retail_baseline` over
//! `cs_assets::install::discover`, the shared campaign walk and
//! `reader_dirs::classify` — so removing the collection fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation and pins the rows it really holds. Run it with
//! `--include-ignored`; without `CS_GAME_DIR` it fails loudly rather than
//! passing vacuously.
//!
//! Every member name and every byte below is authored for this file. No
//! original content is committed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    WORLD_READER_PATTERN, baseline_report_json, install_file_key, retail_baseline,
};
use cs_content::catalog::reader_dirs::ReaderDirRole;
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
            "cs-f14-d-3-{label}-{}-{}",
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

/// The per-mission members a scenario-shaped reader must list.
const MISSION_MEMBERS: [&str; 3] = ["map.zrd", "aiv.zrd", "objectives.zrd"];

/// The members a world-group shared reader lists, and no per-mission one.
const WORLD_MEMBERS: [&str; 3] = ["templates.zrd", "cam_anim.zrd", "landings.zrd"];

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

/// The collection record of the `world` collection.
fn world_status(
    baseline: &cs_content::catalog::baseline::Baseline,
) -> &cs_content::catalog::baseline::CollectionStatus {
    baseline
        .collection_status
        .iter()
        .find(|status| status.kind == ContentKind::World)
        .expect("the world collection reports its status")
}

/// A tree with two world groups, each with its own campaign mission, each with a
/// listable shared reader, plus an instant-action scenario and the install-wide
/// reader. Written as the F14-D.1 acceptance test writes its archives.
fn two_group_tree(label: &str) -> TempInstall {
    let temp = TempInstall::new(label);
    for (group, mission) in [("C1C", "M01"), ("C1B", "M02")] {
        temp.write(
            &format!("ZBD/{group}/{mission}/zrdr.zbd"),
            &reader_archive(&[
                "net.zrd",
                MISSION_MEMBERS[0],
                MISSION_MEMBERS[1],
                MISSION_MEMBERS[2],
            ]),
        );
        temp.write(
            &format!("ZBD/{group}/{mission}/mis_anim.zbd"),
            b"mission animation bytes",
        );
        temp.write(
            &format!("ZBD/{group}/zrdr.zbd"),
            &reader_archive(&WORLD_MEMBERS),
        );
    }
    temp.write(
        "ZBD/C1C/IA1/zrdr.zbd",
        &reader_archive(&[
            "ia.zrd",
            MISSION_MEMBERS[0],
            MISSION_MEMBERS[1],
            MISSION_MEMBERS[2],
        ]),
    );
    temp.write("ZBD/C1C/IA1/mis_anim.zbd", b"ia animation bytes");
    temp.write(
        "ZBD/zrdr.zbd",
        &reader_archive(&["instantaction.zrd", "multiplayer_setup.zrd", "ai.zrd"]),
    );
    temp
}

/// The mapping arm: every world group whose shared reader the producing
/// classifier read is a row, located by that archive's span and pointing at its
/// inventory row.
#[test]
fn accept_f14_d_3_a_classified_world_group_reader_yields_a_world_row() {
    let temp = two_group_tree("rows");
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let catalog = &baseline.catalog;

    // Both groups the classifier read, and nothing else. The install-wide
    // reader is classified too, but it names no group.
    let worlds: Vec<ContentId> = catalog
        .elements()
        .filter(|element| element.kind == ContentKind::World)
        .map(|element| element.id.clone())
        .collect();
    assert_eq!(
        worlds,
        vec![
            cid(ContentKind::World, "c1b"),
            cid(ContentKind::World, "c1c")
        ],
        "one row per world group, keyed by the group directory and in canonical id order"
    );
    assert_eq!(
        catalog
            .elements()
            .filter(|element| element.kind == ContentKind::World)
            .map(|element| element.display_name.clone())
            .collect::<Vec<_>>(),
        vec![Some("ZBD/C1B".to_owned()), Some("ZBD/C1C".to_owned())],
        "the group's own spelling stays outside identity, as display_name"
    );

    for (group, spelling) in [("c1b", "ZBD/C1B/zrdr.zbd"), ("c1c", "ZBD/C1C/zrdr.zbd")] {
        let row = catalog
            .get(&cid(ContentKind::World, group))
            .unwrap_or_else(|| panic!("the world row of {group}"));
        // The row's own bytes: the shared reader's member index, over a span
        // that names the installation whose bytes were read.
        assert!(matches!(row.origin, Origin::Installation { .. }));
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), spelling);
        assert_eq!(
            span.member_key(),
            None,
            "a loose archive is its own container"
        );
        assert_eq!(span.offset(), 0, "the archive as a whole, not one member");
        assert!(span.length() > 0);
        assert_eq!(
            span.install_sha256().to_hex(),
            baseline.install_sha256,
            "the span names the installation whose bytes were read"
        );

        // Nothing is decoded: the row is honest about that instead of claiming a
        // world this engine cannot load.
        assert_eq!(row.parse_state, ParseState::Unparsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert_eq!(
            row.unsupported_reasons,
            vec![UnsupportedReason::NotParsed],
            "no member of the shared reader is decoded at this stage"
        );
        assert!(row.runtime_consumers.is_empty());

        // The edge: onto the inventory row of the archive whose member index
        // named the group, classed as an agent observation.
        assert_eq!(row.dependencies.len(), 1, "one static edge per world row");
        let edge = &row.dependencies[0];
        let archive = cid(ContentKind::InstallFile, &install_file_key(spelling));
        assert_eq!(edge.target, archive);
        assert_eq!(edge.kind.label(), "static");
        assert_eq!(
            edge.provenance.claim_id.as_str(),
            "f14.d.3.baseline.world_reader"
        );
        assert_eq!(
            edge.provenance.class,
            ClaimStatus::ObservedTool,
            "an agent-observed edge is never verified_original"
        );
        assert_eq!(
            row.fingerprint,
            catalog
                .get(&archive)
                .expect("the archive's inventory row")
                .fingerprint,
            "the world row and its archive's inventory row describe the same bytes"
        );
    }

    // The collection's own record: rows with no diagnostic.
    let status = world_status(&baseline);
    assert_eq!(status.source, WORLD_READER_PATTERN);
    assert_eq!(
        status.language, None,
        "a reader archive has no language dimension"
    );
    assert_eq!(status.rows, 2);
    assert_eq!(status.gaps.get("declared_group_without_reader"), Some(&0));
    assert_eq!(status.boundary_id, None);
    assert_eq!(
        status.diagnostic, None,
        "the classifier read every group reader"
    );

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"world\":2"), "{report}");
    assert!(
        report.contains(&format!(
            "\"kind\":\"world\",\"source\":\"{WORLD_READER_PATTERN}\",\"language\":null,\"rows\":2"
        )),
        "{report}"
    );
    assert!(report.contains("\"id\":\"world/c1c\""), "{report}");
    assert_eq!(
        report,
        baseline_report_json(&retail_baseline(&temp.0).expect("re-read")),
        "the report is byte-stable for the same installation"
    );
}

/// A world group is a row only because its **own member index** says so. A
/// shared reader that cannot be listed yields no row: the group is counted in the
/// collection's gap record and stays named as an unrecognized reader directory.
#[test]
fn accept_f14_d_3_a_world_group_reader_that_cannot_be_listed_is_named_not_invented() {
    let temp = two_group_tree("unlistable");
    // A third group the layout declares (a third mission in chapter 1), whose
    // shared reader holds bytes with no member index at all.
    temp.write("ZBD/C2C/M03/zrdr.zbd", b"mission program bytes");
    temp.write("ZBD/C2C/M03/mis_anim.zbd", b"mission animation bytes");
    temp.write("ZBD/C2C/zrdr.zbd", b"not a reader archive at all");

    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(
        baseline
            .catalog
            .get(&cid(ContentKind::World, "c2c"))
            .is_none(),
        "an archive whose member index cannot be read names no world"
    );
    assert_eq!(
        baseline
            .catalog
            .elements()
            .filter(|element| element.kind == ContentKind::World)
            .count(),
        2,
        "the two listable groups keep their rows"
    );

    // The evidence is missing rather than negative, so the directory stays named
    // instead of being counted or dropped.
    let unrecognized: Vec<&str> = baseline
        .unrecognized_program_dirs
        .iter()
        .map(|record| record.path.as_str())
        .collect();
    assert_eq!(unrecognized, vec!["ZBD/C2C"]);
    assert!(
        baseline
            .classified_reader_dirs
            .iter()
            .all(|dir| dir.role != ReaderDirRole::WorldGroupReader || dir.path != "ZBD/C2C"),
        "an unlistable archive classifies nothing"
    );

    // The gap is in the collection record, so a reader of the report can tell an
    // installation with no such group from one whose reader this engine could
    // not read.
    let status = world_status(&baseline);
    assert_eq!(status.rows, 2);
    assert_eq!(status.gaps.get("declared_group_without_reader"), Some(&1));
    assert_eq!(status.diagnostic, None, "the collection holds rows");

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"world\":2"), "{report}");
    assert!(
        report.contains("\"declared_group_without_reader\":1"),
        "{report}"
    );
}

/// An installation whose every world-group reader is unreadable holds no world
/// row at all, and says so in the collection record instead of reading like an
/// installation that has no world groups.
#[test]
fn accept_f14_d_3_no_readable_world_group_reader_is_a_named_gap_not_an_empty_reading() {
    let temp = TempInstall::new("no-reader");
    temp.write("ZBD/C1C/M01/zrdr.zbd", b"mission program bytes");
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
    temp.write("ZBD/C1C/zrdr.zbd", b"not a reader archive at all");

    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| element.kind != ContentKind::World),
        "no row is invented from a directory name"
    );
    let status = world_status(&baseline);
    assert_eq!(status.source, WORLD_READER_PATTERN);
    assert_eq!(status.rows, 0);
    assert_eq!(status.gaps.get("declared_group_without_reader"), Some(&1));
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("an unreadable source is named, not dropped");
    assert!(diagnostic.contains(WORLD_READER_PATTERN), "{diagnostic}");
    assert!(diagnostic.contains("c1c"), "{diagnostic}");

    // The rest of the inventory is unchanged: the collection's failure does not
    // take the missions with it.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.catalog.launchable_count(), 1);
    let report = baseline_report_json(&baseline);
    assert!(!report.contains("\"world\":"), "{report}");
}

/// Worlds are not launchable content, so this collection adds no root and cannot
/// move the coverage denominator; its rows stay visible as unreachable unknowns.
#[test]
fn accept_f14_d_3_world_rows_are_not_launchable_and_the_denominator_does_not_move() {
    let temp = two_group_tree("roots");
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let catalog = &baseline.catalog;

    assert!(
        !ContentKind::World.is_launchable(),
        "a world is not a mission or a scenario directory, so the denominator cannot move when \
         the collection is populated"
    );
    assert_eq!(
        baseline.roots,
        vec![
            cid(ContentKind::Mission, "ch1-m01"),
            cid(ContentKind::Mission, "ch1-m02"),
            cid(ContentKind::IaScenario, "c1c-ia1"),
        ],
        "the roots are the two campaign missions and the one scenario directory, unchanged"
    );
    assert_eq!(catalog.launchable_count(), 3);
    assert_eq!(catalog.original_launchable_count(), 3);
    assert_eq!(baseline.coverage.roots, 3);
    assert_eq!(
        baseline.coverage.reachable, 9,
        "each launchable reaches its program and the file holding its bytes, and nothing reaches \
         a world yet"
    );
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(
        baseline.coverage.unreachable_by_kind.get("world").copied(),
        Some(2),
        "no row points at a world yet, so the world rows stay counted as unreachable"
    );
    assert!(baseline.coverage.unreachable_needing_classification >= 2);
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::World),
        "no world row is a closure root"
    );
}

// ------------------------------------------------------------- retail ----

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The world-group directory names under `ZBD/`, as the installation spells
/// them, sorted case-insensitively — a walk of the tree, independent of the
/// production classifier and of the campaign layout.
fn world_group_dirs(game_dir: &Path) -> Vec<String> {
    let mut groups: Vec<String> = fs::read_dir(game_dir.join("ZBD"))
        .expect("the installation's reader container reads")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    groups.sort_by_key(|name| name.to_ascii_lowercase());
    groups
}

/// The retail half: the installation's eight world-group readers each become one
/// `world` row, located by that reader's own bytes and pointing at its inventory
/// row, and the coverage denominator is unchanged.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_3_retail_every_world_group_reader_the_installation_names_is_a_row() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let catalog = &baseline.catalog;
    let discovery =
        cs_assets::install::discover(&dir).expect("production discovery reads the installation");
    let install_sha = cs_assets::install::fingerprint(&discovery.manifest).to_hex();

    // The expected groups come from a walk of the tree, not from the classifier.
    let groups = world_group_dirs(&dir);
    assert_eq!(groups.len(), 8, "eight world-group directories under ZBD/");

    let rows: Vec<&cs_types::content::CatalogElement> = catalog
        .elements()
        .filter(|element| element.kind == ContentKind::World)
        .collect();
    assert_eq!(
        rows.len(),
        groups.len(),
        "one world row per world-group reader the classifier read"
    );
    assert_eq!(
        rows.iter()
            .map(|row| row.id.key().to_owned())
            .collect::<Vec<_>>(),
        groups
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<Vec<_>>(),
        "the identity is the group directory, lowercased, in canonical order"
    );

    for (row, name) in rows.iter().zip(&groups) {
        // The bytes that named the group: its own shared reader, over a checked
        // span, with the digest production discovery measured.
        assert!(
            row.origin.is_original(),
            "{} must be installation data, got {}",
            row.id,
            row.origin.label()
        );
        let span = row.origin.source().expect("an installation span");
        let spelling = format!("ZBD/{name}/zrdr.zbd");
        assert_eq!(span.container_path(), spelling);
        assert_eq!(span.install_sha256().to_hex(), install_sha);
        assert_eq!(span.offset(), 0, "the archive as a whole");
        let record = discovery
            .manifest
            .files
            .iter()
            .find(|record| {
                record
                    .relative_spelling
                    .as_str()
                    .eq_ignore_ascii_case(&spelling)
            })
            .unwrap_or_else(|| panic!("the installation inventories {spelling}"));
        assert_eq!(span.length(), record.size_bytes);
        assert_eq!(
            row.fingerprint.as_ref().map(|digest| digest.sha256),
            Some(record.sha256),
            "{} fingerprints the bytes its member index came from",
            row.id
        );

        // The edge onto that archive's inventory row, with observed provenance.
        assert_eq!(row.dependencies.len(), 1, "{}: one static edge", row.id);
        let edge = &row.dependencies[0];
        assert_eq!(
            edge.target,
            cid(ContentKind::InstallFile, &install_file_key(&spelling)),
            "{} points at the reader archive whose member index named it",
            row.id
        );
        assert_eq!(edge.kind.label(), "static");
        assert_eq!(
            edge.provenance.class,
            ClaimStatus::ObservedTool,
            "an agent observation is never verified_original"
        );
        assert_eq!(
            edge.provenance.claim_id.as_str(),
            "f14.d.3.baseline.world_reader"
        );

        // Nothing is decoded, so nothing claims to be loadable.
        assert_eq!(row.parse_state, ParseState::Unparsed);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(!row.is_ready());
        assert!(row.runtime_consumers.is_empty());
        assert_eq!(
            row.unsupported_reasons,
            vec![UnsupportedReason::NotParsed],
            "{}: no member of a shared reader is decoded at this stage",
            row.id
        );
        assert_eq!(
            row.display_name.as_deref(),
            Some(format!("ZBD/{name}").as_str()),
            "the group's own spelling stays outside identity"
        );
    }

    let status = world_status(&baseline);
    assert_eq!(status.source, WORLD_READER_PATTERN);
    assert_eq!(status.language, None);
    assert_eq!(status.rows, 8);
    assert_eq!(
        status.gaps.get("declared_group_without_reader"),
        Some(&0),
        "every declared world group of the installation has a listable shared reader"
    );
    assert_eq!(status.diagnostic, None);

    // The denominator did not move: a world is not launchable content.
    assert!(!ContentKind::World.is_launchable());
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::World),
        "no world row is a closure root"
    );
    assert_eq!(catalog.launchable_count(), baseline.roots.len());
    assert_eq!(catalog.synthetic_launchable_count(), 0);
    assert_eq!(
        baseline.coverage.unreachable_by_kind.get("world").copied(),
        Some(8),
        "no row points at a world yet, so the rows stay counted as unreachable unknowns"
    );
    assert!(!catalog.is_fully_ready());

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"world\":8"), "{report}");
    assert!(
        report.contains("\"declared_group_without_reader\":0"),
        "{report}"
    );
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );
}
