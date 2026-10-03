//! Acceptance scenario F14-D: the complete private baseline inventory and
//! its coverage denominator (`specs/F14-canonical-content-catalog-and-
//! dependency-closure.md`, `### F14-D`).
//!
//! The non-retail tests read a **synthetic installation tree** written into a
//! temporary directory: it is a directory of files like any other, and every
//! row it produces is traced to that tree's own fingerprints. They exercise
//! production code only — `cs_content::catalog::baseline::retail_baseline`
//! over `cs_assets::install::discover` and the shared campaign walk, plus the
//! production report renderer and the production
//! [`cs_types::content::account_catalog_rows`] — so removing or neutering the
//! inventory, the denominator declaration, the origin split, the coverage
//! accounting or the row-role classification makes them fail.
//!
//! Completeness of the inventory is **derived, not remembered**: every catalog
//! row falls into exactly one [`CatalogRowRole`] by its [`ContentKind`], and
//! the total of the non-launchable collections is what the kinds present add up
//! to. A collection that inserts rows is therefore accounted for without any
//! list naming it, which is the defect
//! `accept_f14_d_a_collection_named_by_no_list_is_still_accounted` pins: the
//! hand-maintained sum it replaced had to be extended by hand, and twice was
//! not.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation and pins the denominator against the frozen F50
//! campaign inventory. It must be run with `--include-ignored` by the
//! implementing and reviewing agents; without `CS_GAME_DIR` it fails loudly
//! rather than passing vacuously.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_assets::install as install_api;
use cs_content::campaign_bindings::CampaignInventory;
use cs_content::catalog::Catalog;
use cs_content::catalog::baseline::{
    Baseline, Coverage, ProgramDirRecord, baseline_report_json, install_file_key, retail_baseline,
};
use cs_content::catalog::reader_dirs::ReaderDirRole;
use cs_formats::zbd::{
    INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES, TRAILER_VERSION_ONE,
};
use cs_types::content::{
    CatalogElement, CatalogRowRole, ConsumerKind, ContentId, ContentKind, NormalizeState, Origin,
    Provenance, Readiness, RuntimeConsumer, account_catalog_rows,
};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::ParseState;

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture directory is created");
        Self(root)
    }

    /// Writes one file inside the tree, creating its directories.
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

/// The synthetic installation tree: eight regular files, one campaign
/// mission directory (`ZBD/C1C/M01`) and two reader-archive directories
/// (`ZBD` itself and the instant-action-shaped `ZBD/C1C/IA1`) whose archives
/// hold no readable member index, so F14-D.1 cannot classify them and they
/// stay named as unknowns.
fn tree(label: &str) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write("ZBD/C1C/M01/zrdr.zbd", b"mission program bytes");
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
    temp.write("ZBD/C1C/IA1/zrdr.zbd", b"unclassified program bytes");
    temp.write("ZBD/C1C/IA1/mis_anim.zbd", b"unclassified animation bytes");
    temp.write("ZBD/C1C/gamez.zbd", b"world mesh bytes");
    temp.write("ZBD/zrdr.zbd", b"world group reader bytes");
    temp.write("GOSDATA/ASSETS/BINARIES/langui.dll", b"string table bytes");
    temp.write("strings.dll", b"loose string table bytes");
    temp
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("test claim id is valid")
}

/// The complete inventory: every inventoried file is a row, every declared
/// campaign mission is a declared launchable row, and the two are linked
/// through the mission's reader archive with observed provenance.
#[test]
fn accept_f14_d_baseline_inventory_covers_every_inventoried_file_and_declared_mission() {
    let temp = tree("inventory");
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let catalog = &baseline.catalog;

    // Completeness of the inventory: one install-file row per inventoried
    // regular file, none filtered out, plus the mission and its program.
    let discovery = install_api::discover(&temp.0).expect("production discovery reads the tree");
    let install_rows = catalog
        .elements()
        .filter(|element| element.kind == ContentKind::InstallFile)
        .count();
    assert_eq!(
        install_rows,
        discovery.manifest.files.len(),
        "every inventoried file is a catalog row"
    );
    assert_eq!(
        catalog.len(),
        discovery.manifest.files.len() + 2,
        "the eight files plus one mission and one program row"
    );
    assert_eq!(discovery.manifest.files.len(), 8, "the fixture tree");

    // The denominator: the one campaign mission directory is declared
    // launchable and counted as unsupported, never filtered out.
    let mission = cid(ContentKind::Mission, "ch1-m01");
    let program = cid(ContentKind::Script, "c1c-m01-zrdr");
    assert_eq!(baseline.roots, vec![mission.clone()]);
    assert_eq!(catalog.launchable_count(), 1);
    assert_eq!(catalog.unsupported_count(), 1, "the mission is not ready");
    assert!(!catalog.is_fully_ready());
    assert!(
        !catalog.is_retail_ready(),
        "a launchable row that is not ready is not retail-ready"
    );

    // Every row is original installation data located by a checked span that
    // names the fingerprint of the bytes actually read.
    for element in catalog.elements() {
        assert!(
            element.origin.is_original(),
            "{} must be installation data, got {}",
            element.id,
            element.origin.label()
        );
        let span = element.origin.source().expect("an installation span");
        assert_eq!(
            span.install_sha256().to_hex(),
            baseline.install_sha256,
            "{} locates bytes of this installation",
            element.id
        );
        assert_eq!(span.offset(), 0, "a loose file starts at byte 0");
        assert!(span.length() > 0 || element.fingerprint.is_some());
    }
    assert_eq!(
        baseline.install_sha256,
        install_api::fingerprint(&discovery.manifest).to_hex(),
        "the reported installation fingerprint is production discovery's"
    );
    assert_eq!(
        baseline.content_sha256,
        install_api::content_fingerprint(&discovery.manifest).to_hex()
    );

    // The edges: mission -> program -> the inventory file that holds its
    // bytes, each with observed provenance over that span.
    let mission_row = catalog.get(&mission).expect("the mission row");
    assert_eq!(mission_row.dependencies.len(), 1);
    assert_eq!(mission_row.dependencies[0].target, program);
    assert_eq!(
        mission_row.dependencies[0].provenance.class,
        ClaimStatus::ObservedTool,
        "the layout observation is classed observed_tool, never verified_original"
    );
    let file_id = ContentId::from_source(
        ContentKind::InstallFile,
        &install_file_key("ZBD/C1C/M01/zrdr.zbd"),
    )
    .expect("the program's inventory id");
    let program_row = catalog.get(&program).expect("the program row");
    assert_eq!(program_row.dependencies[0].target, file_id);
    assert_eq!(
        program_row.fingerprint,
        catalog
            .get(&file_id)
            .expect("the inventory row")
            .fingerprint,
        "the program row and its inventory row describe the same bytes"
    );
    let file_row = catalog.get(&file_id).expect("the inventory row");
    assert_eq!(
        file_row.display_name.as_deref(),
        Some("ZBD/C1C/M01/zrdr.zbd"),
        "the original spelling stays outside identity"
    );
    assert_eq!(
        file_row.id.key(),
        "zbd_2f_c1c_2f_m01_2f_zrdr.zbd",
        "identity is the escaped, case-folded key"
    );

    // Coverage: the mission, its program and the file holding its bytes are
    // reached; every other inventory row stays accounted as unreachable
    // instead of disappearing from the report.
    assert_eq!(baseline.coverage.roots, 1);
    assert_eq!(baseline.coverage.reachable, 3);
    assert_eq!(baseline.coverage.unreachable, catalog.len() - 3);
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(baseline.coverage.ready, 0, "nothing is playable yet");
    assert_eq!(baseline.coverage.unavailable, 3);
    assert_eq!(
        baseline.coverage.unreachable_by_kind.get("install_file"),
        Some(&(catalog.len() - 3))
    );
    assert_eq!(
        baseline.coverage.unreachable_needing_classification,
        catalog.len() - 3,
        "every unreachable row is unknown and still needs a classification"
    );

    // Reader-archive directories no rule classifies stay visible: they are
    // neither counted in the denominator nor dropped. These two hold
    // unlistable fixture bytes, so the member evidence is missing rather than
    // negative: an archive that cannot be listed is unknown, never a guess
    // from its directory name.
    let unclassified: Vec<&str> = baseline
        .unrecognized_program_dirs
        .iter()
        .map(|record| record.path.as_str())
        .collect();
    assert_eq!(
        unclassified,
        vec!["ZBD", "ZBD/C1C/IA1"],
        "the two reader-archive directories whose archives cannot be listed"
    );
    assert!(
        baseline.classified_reader_dirs.is_empty(),
        "an archive with no member index classifies nothing"
    );
    assert_eq!(
        baseline.unrecognized_program_dirs[1].program,
        "ZBD/C1C/IA1/zrdr.zbd"
    );
    assert_eq!(
        baseline.unrecognized_program_dirs[1].program_sha256.len(),
        64,
        "the record carries the archive's digest, not its bytes"
    );

    // The report names its source, its fingerprints and the honest counts,
    // and the same installation produces the same bytes twice.
    let report = baseline_report_json(&baseline);
    assert_eq!(
        report,
        baseline_report_json(&retail_baseline(&temp.0).expect("re-read")),
        "the report is byte-stable for the same installation"
    );
    assert!(report.contains("\"schema\":\"cs-content-baseline/1\""));
    assert!(report.contains(&format!(
        "\"source\":\"{}\"",
        temp.0.display().to_string().replace('\\', "\\\\")
    )));
    assert!(report.contains("\"retail\":true"));
    assert!(report.contains(&format!(
        "\"install_sha256\":\"{}\"",
        baseline.install_sha256
    )));
    assert!(report.contains("\"launchable\":1"));
    assert!(report.contains("\"synthetic_launchable\":0"));
    assert!(report.contains("\"ready\":0"));
    assert!(report.contains("\"roots\":1"));
    assert!(report.contains("\"unresolved_references\":0"));
    assert!(report.contains("\"path\":\"ZBD/C1C/IA1\""));

    // Completeness is derived from the rows' kinds, not from a list of the
    // collections that existed when this test was written, so it is asserted
    // here too and runs in CI: the eight inventoried files, the one launchable
    // row, its one program and nothing else.
    let accounting = account_catalog_rows(catalog.elements());
    assert!(
        accounting.is_complete(),
        "every row has one role: {accounting}"
    );
    assert_eq!(accounting.total, catalog.len());
    assert_eq!(accounting.install_file, 8, "{accounting}");
    assert_eq!(accounting.launchable, 1, "{accounting}");
    assert_eq!(accounting.program, 1, "{accounting}");
    assert_eq!(
        accounting.source_derived, 0,
        "this fixture builds no source-derived collection: {accounting}"
    );
    assert!(
        accounting.collections().is_empty(),
        "and the completeness message names no collection: {accounting}"
    );
}

/// AC04, the F14-D minimum scenario: a synthetic launchable row is never
/// mistaken for a retail catalog entry.
///
/// The failure case is explicit: if the origin split were removed (a
/// launchable counted without asking where it came from), the retail
/// denominator would grow to two and the report would present the authored
/// row as retail content. Both readings are asserted here, and a fully ready
/// synthetic catalog is asserted to be *not* retail-ready, which is the
/// property that keeps the retail claim honest.
#[test]
fn accept_f14_d_synthetic_launchable_row_is_never_a_retail_catalog_entry() {
    let temp = tree("ac04");
    let mut baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let catalog = &mut baseline.catalog;
    assert_eq!(catalog.original_launchable_count(), 1);
    assert_eq!(catalog.synthetic_launchable_count(), 0);

    // An authored launchable mission joins the same catalog.
    let synthetic = cid(ContentKind::Mission, "m01-synthetic");
    let element = CatalogElement {
        kind: ContentKind::Mission,
        id: synthetic.clone(),
        display_name: Some("Synthetic Mission".to_owned()),
        origin: Origin::SyntheticFixture,
        dependencies: Vec::new(),
        parse_state: ParseState::Parsed,
        normalize_state: NormalizeState::Normalized,
        runtime_consumers: vec![RuntimeConsumer {
            kind: ConsumerKind::Gameplay,
            provenance: Provenance::designed(claim("f14.d.test.consumer")),
        }],
        readiness: Readiness::Ready,
        unsupported_reasons: Vec::new(),
        fingerprint: None,
    };
    catalog.insert(element.clone()).expect("the row validates");
    catalog
        .declare_launchable(&synthetic)
        .expect("a mission is launchable");

    // The retail denominator is unchanged: the authored row is counted
    // beside it, never inside it.
    assert_eq!(
        catalog.original_launchable_count(),
        1,
        "the synthetic row must not inflate the retail denominator"
    );
    assert_eq!(catalog.synthetic_launchable_count(), 1);
    assert_eq!(catalog.launchable_count(), 2);
    assert_eq!(
        catalog.unsupported_count(),
        1,
        "the ready synthetic row does not make the unsupported retail mission go away"
    );
    assert!(!catalog.is_fully_ready(), "the retail mission is unready");
    assert!(
        !catalog.is_retail_ready(),
        "a catalog holding a synthetic launchable row is never retail-ready"
    );

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"launchable\":2"));
    assert!(report.contains("\"original_launchable\":1"));
    assert!(report.contains("\"synthetic_launchable\":1"));
    assert!(
        report.contains("\"retail\":true"),
        "retail rows are present"
    );
    assert!(
        report.contains("\"origin\":\"synthetic_fixture\",\"source\":null"),
        "the authored row renders as synthetic and locates no original bytes"
    );
    assert_eq!(
        report.matches("\"origin\":\"installation\"").count(),
        10,
        "every row built from the tree keeps its installation origin"
    );

    // The other direction: a catalog whose launchables are all authored is
    // never retail, however ready it is.
    let mut synthetic_only = Catalog::new();
    synthetic_only
        .insert(CatalogElement {
            readiness: Readiness::Ready,
            ..element
        })
        .expect("the row validates");
    synthetic_only
        .declare_launchable(&synthetic)
        .expect("a mission is launchable");
    assert!(synthetic_only.is_fully_ready(), "every row is ready");
    assert_eq!(synthetic_only.original_launchable_count(), 0);
    assert!(!synthetic_only.is_retail_ready());
    let report = baseline_report_json(&Baseline {
        source: "synthetic-fixture".to_owned(),
        install_sha256: "0".repeat(64),
        content_sha256: "0".repeat(64),
        catalog: synthetic_only,
        roots: vec![synthetic],
        coverage: Coverage {
            roots: 1,
            reachable: 1,
            unreachable: 0,
            ready: 1,
            unavailable: 0,
            unresolved_references: 0,
            unreachable_by_kind: std::collections::BTreeMap::new(),
            unreachable_needing_classification: 0,
        },
        classified_reader_dirs: Vec::new(),
        unrecognized_program_dirs: Vec::<ProgramDirRecord>::new(),
        geometry_containers: Vec::new(),
        collection_status: Vec::new(),
    });
    assert!(report.contains("\"retail\":false"));
    assert!(report.contains("\"is_fully_ready\":true"));
    assert!(report.contains("\"is_retail_ready\":false"));
    assert!(report.contains("\"original_launchable\":0"));
    assert!(report.contains("\"synthetic_launchable\":1"));
}

/// One version-one reader archive whose member index lists `names` (each
/// member holds a few filler bytes), written exactly as the pinned reader
/// expects: member data, 148-byte index entries, then the trailer.
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

/// F14-D.1: reader-archive directories are classified from their own member
/// index. Scenario directories become launchable rows of the denominator,
/// the shared readers are classified as not launchable, and a directory whose
/// members do not corroborate its name stays unrecognized.
#[test]
fn accept_f14_d_1_reader_directories_are_classified_from_their_member_index() {
    const MISSION: [&str; 3] = ["map.zrd", "aiv.zrd", "objectives.zrd"];
    let temp = TempInstall::new("f14-d-1");
    let write_reader = |dir: &str, extra: &[&str], mission_members: bool| {
        let mut names: Vec<&str> = extra.to_vec();
        if mission_members {
            names.extend_from_slice(&MISSION);
        }
        temp.write(&format!("{dir}/zrdr.zbd"), &reader_archive(&names));
    };
    write_reader("ZBD/C1C/M01", &["net.zrd"], true);
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
    write_reader("ZBD/C1C/IA1", &["ia.zrd"], true);
    temp.write("ZBD/C1C/IA1/mis_anim.zbd", b"ia animation bytes");
    write_reader("ZBD/C1C/MP1", &["net.zrd"], true);
    temp.write("ZBD/C1C/MP1/mis_anim.zbd", b"mp animation bytes");
    // Named like a scenario, but its members say otherwise: unknown.
    write_reader("ZBD/C1C/MP2", &["templates.zrd"], true);
    temp.write("ZBD/C1C/MP2/mis_anim.zbd", b"mp animation bytes");
    write_reader("ZBD/C1C", &["templates.zrd", "cam_anim.zrd"], false);
    write_reader(
        "ZBD",
        &["instantaction.zrd", "multiplayer_setup.zrd"],
        false,
    );
    temp.write("ZBD/C1C/gamez.zbd", b"world mesh bytes");

    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let catalog = &baseline.catalog;

    let role = |path: &str| {
        baseline
            .classified_reader_dirs
            .iter()
            .find(|dir| dir.path == path)
            .map(|dir| dir.role)
    };
    assert_eq!(
        role("ZBD/C1C/IA1"),
        Some(ReaderDirRole::InstantActionScenario)
    );
    assert_eq!(
        role("ZBD/C1C/MP1"),
        Some(ReaderDirRole::MultiplayerScenario)
    );
    assert_eq!(role("ZBD/C1C"), Some(ReaderDirRole::WorldGroupReader));
    assert_eq!(role("ZBD"), Some(ReaderDirRole::SharedReader));
    assert_eq!(role("ZBD/C1C/MP2"), None, "the members do not corroborate");
    let unknown: Vec<&str> = baseline
        .unrecognized_program_dirs
        .iter()
        .map(|record| record.path.as_str())
        .collect();
    assert_eq!(unknown, vec!["ZBD/C1C/MP2"]);

    // The denominator: the mission plus the two scenario directories, every
    // row original and located by a span, none of the shared readers.
    let ia = cid(ContentKind::IaScenario, "c1c-ia1");
    let mp = cid(ContentKind::MultiplayerScenario, "c1c-mp1");
    assert_eq!(catalog.launchable_count(), 3);
    assert_eq!(catalog.original_launchable_count(), 3);
    assert_eq!(baseline.roots.len(), 3);
    assert!(baseline.roots.contains(&ia) && baseline.roots.contains(&mp));
    for id in [&ia, &mp] {
        let row = catalog.get(id).expect("the scenario row");
        assert!(row.origin.is_original());
        assert_eq!(row.dependencies.len(), 1);
        assert_eq!(
            row.dependencies[0].target,
            cid(ContentKind::Script, &format!("{}-zrdr", id.key()))
        );
        assert_eq!(
            row.dependencies[0].provenance.class,
            ClaimStatus::ObservedTool
        );
        assert!(!row.is_ready(), "nothing is playable yet");
    }
    assert_eq!(baseline.coverage.roots, 3);
    assert_eq!(baseline.coverage.reachable, 9);
    assert_eq!(baseline.coverage.unresolved_references, 0);

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"role\":\"instant_action_scenario\""));
    assert!(report.contains("\"role\":\"shared_reader\""));
    assert!(report.contains("\"launchable\":3"));
    assert_eq!(
        report,
        baseline_report_json(&retail_baseline(&temp.0).expect("re-read"))
    );
}

/// Failure cases: the baseline is refused rather than built over a hole.
#[test]
fn accept_f14_d_baseline_refuses_a_denominator_it_cannot_read() {
    // No campaign mission directory at all: there is nothing to declare.
    let empty = TempInstall::new("no-campaign");
    empty.write("strings.dll", b"loose string table bytes");
    let error = retail_baseline(&empty.0).expect_err("no campaign layout");
    assert!(
        error.to_string().contains("campaign"),
        "the refusal names the campaign layout: {error}"
    );

    // A declared mission whose reader archive is absent: the mission stays
    // in the denominator, so the whole baseline is refused. Filtering the
    // mission out (non-negotiable behavior 4) is exactly what must not
    // happen, and a fabricated origin for missing bytes is not an option.
    let missing = TempInstall::new("missing-program");
    missing.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
    missing.write("ZBD/C1C/gamez.zbd", b"world mesh bytes");
    let error = retail_baseline(&missing.0).expect_err("the program is missing");
    let message = error.to_string();
    assert!(message.contains("mission/ch1-m01"), "{message}");
    assert!(message.contains("ZBD/C1C/M01/zrdr.zbd"), "{message}");
    assert!(
        message.contains("shorter denominator"),
        "the refusal states that the denominator is never shortened: {message}"
    );

    // A reader archive that is present but was skipped by discovery (a
    // symbolic link) cannot back a fingerprinted row, so it is refused by
    // name instead of being inventoried twice or guessed.
    #[cfg(unix)]
    {
        let linked = TempInstall::new("linked-program");
        linked.write("payload.zbd", b"the bytes behind the link");
        linked.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
        let mission_dir = linked.0.join("ZBD/C1C/M01");
        std::os::unix::fs::symlink(linked.0.join("payload.zbd"), mission_dir.join("zrdr.zbd"))
            .expect("the fixture symlink is created");
        let error = retail_baseline(&linked.0).expect_err("the program is not inventoried");
        let message = error.to_string();
        assert!(message.contains("mission/ch1-m01"), "{message}");
        assert!(message.contains("not inventoried"), "{message}");
    }
}

/// One row of a collection this stage does not build, inserted the way a real
/// collection stage inserts its own: a validated [`CatalogElement`] of the
/// collection's kind, through the production catalog.
///
/// The origin is authored because the accounting is a property of the row's
/// *kind*, not of where its bytes came from; the retail half of the same
/// accounting runs over installation-origin rows.
fn collection_row(kind: ContentKind, key: &str) -> CatalogElement {
    CatalogElement {
        kind,
        id: cid(kind, key),
        display_name: Some(key.to_owned()),
        origin: Origin::SyntheticFixture,
        dependencies: Vec::new(),
        parse_state: ParseState::Parsed,
        normalize_state: NormalizeState::Normalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Ready,
        unsupported_reasons: Vec::new(),
        fingerprint: None,
    }
}

/// The sum this check used to assert, kept only so the demonstration below can
/// show it breaking: the collections named when the sum was written, added up
/// by hand. A collection that inserts rows and is absent from this list moves
/// the catalog but not this total, which is exactly how F14-D.6 and F14-D.7
/// each failed the check on a rebase of `main`.
fn hand_summed_collections(rows_by_kind: &BTreeMap<ContentKind, usize>) -> usize {
    [
        "world",
        "scene_node",
        "mesh",
        "airframe",
        "paint_mask",
        "faction",
        "sound",
        "multiplayer_rules",
    ]
    .iter()
    .filter_map(|label| ContentKind::from_label(label))
    .filter_map(|kind| rows_by_kind.get(&kind).copied())
    .sum()
}

/// The defect this file's completeness total had, demonstrated rather than
/// described: a collection that inserts rows **without appearing in any
/// hand-maintained list** cannot fail the completeness check any more.
///
/// The scenario is the real one. It starts from a production baseline over the
/// synthetic tree with the collections the old hand-sum named already in it, so
/// the old total is correct; then a new collection lands and inserts rows of
/// two kinds nothing in this stage uses (`music` and `image`); then both shapes
/// are evaluated against the same catalog. The hand-summed total no longer
/// matches — which is the failure that landed on `main` twice — while the
/// derived accounting is complete, names the new collections, and satisfies the
/// completeness identity the retail test asserts.
#[test]
fn accept_f14_d_a_collection_named_by_no_list_is_still_accounted() {
    let temp = tree("accounting");
    let mut baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let catalog = &mut baseline.catalog;

    // Two rows of collections the old hand-sum named, so that shape is complete
    // to begin with.
    for (kind, key) in [
        (ContentKind::World, "world-row"),
        (ContentKind::Sound, "sound-row"),
    ] {
        catalog
            .insert(collection_row(kind, key))
            .expect("the row validates");
    }
    let before = account_catalog_rows(catalog.elements());
    assert!(before.is_complete(), "{before}");
    assert_eq!(
        hand_summed_collections(&before.rows_by_kind),
        before.unaccounted(),
        "to begin with the hand-summed shape agrees with the catalog: {before}"
    );

    // A new collection lands. It uses two kinds no stage inserts today, so
    // neither appears in any list in this file.
    for (kind, key, rows) in [
        (ContentKind::Music, "music-row", 3),
        (ContentKind::Image, "image-row", 2),
    ] {
        for index in 0..rows {
            catalog
                .insert(collection_row(kind, &format!("{key}-{index}")))
                .expect("the row validates");
        }
    }
    let after = account_catalog_rows(catalog.elements());

    // The shape this replaced, on the same catalog: its list did not grow.
    assert_eq!(
        hand_summed_collections(&after.rows_by_kind),
        2,
        "the hand-maintained sum does not move when an unnamed collection lands"
    );
    assert_ne!(
        after.unaccounted(),
        hand_summed_collections(&after.rows_by_kind),
        "so asserting against it is the failure this task removes"
    );

    // The derived accounting, on the same catalog: complete, and it names the
    // collections nobody declared.
    assert!(after.is_complete(), "{after}");
    assert_eq!(after.total, catalog.len());
    assert_eq!(after.install_file, 8, "a collection adds no file row");
    assert_eq!(
        after.launchable, 1,
        "a source-derived collection is not launchable: {after}"
    );
    assert_eq!(after.program, 1, "a collection adds no program row");
    assert_eq!(
        after.unaccounted(),
        7,
        "the two named collections plus the five rows of the unnamed one: {after}"
    );
    assert_eq!(
        catalog.len(),
        after.install_file + 2 * after.launchable + after.unaccounted(),
        "the completeness identity the retail test asserts: {after}"
    );
    assert_eq!(
        after.collections(),
        vec![("world", 1), ("image", 2), ("sound", 1), ("music", 3)],
        "the breakdown names every collection present, in canonical kind order"
    );
    let rendered = after.to_string();
    assert!(rendered.contains("music 3"), "{rendered}");
    assert!(rendered.contains("image 2"), "{rendered}");
    assert!(rendered.contains("7 source-derived"), "{rendered}");

    // And the classification itself: the unnamed kinds are source-derived
    // collections, and a launchable kind is not one of them.
    for kind in [ContentKind::Music, ContentKind::Image, ContentKind::World] {
        assert_eq!(
            kind.baseline_row_role(),
            CatalogRowRole::SourceDerivedCollection,
            "{} is a collection row",
            kind.label()
        );
        assert!(!kind.is_launchable(), "{}", kind.label());
    }
    assert_eq!(
        ContentKind::Mission.baseline_row_role(),
        CatalogRowRole::Launchable
    );
    assert_eq!(
        ContentKind::Script.baseline_row_role(),
        CatalogRowRole::Program
    );
}

/// The retail half: the denominator covers every campaign mission the
/// installation declares, matches the frozen F50 inventory, and no row is
/// synthetic.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic() {
    let game_dir =
        PathBuf::from(std::env::var("CS_GAME_DIR").expect(
            "CS_GAME_DIR must name the original installation for the retail acceptance test",
        ));
    let baseline = retail_baseline(&game_dir).expect("the original installation reads");
    let catalog = &baseline.catalog;

    // The denominator equals two independent declarations of it: the frozen
    // F50 campaign inventory and the shared campaign walk.
    let inventory_path = workspace_root().join("missions/bindings/campaign-inventory.tsv");
    let inventory = CampaignInventory::load(&inventory_path)
        .unwrap_or_else(|error| panic!("{} reads: {error}", inventory_path.display()));
    assert_eq!(
        inventory.len(),
        24,
        "the frozen F50 denominator holds one work order per original campaign mission"
    );
    // F14-D.1: the reader-archive directories the campaign walk leaves over
    // are classified from their own member index. The scenario directories
    // join the denominator; the expected count is measured by a separate walk
    // of the directory tree, not by the production classifier.
    let (ia_dirs, mp_dirs, groups) = scenario_directories(&game_dir);
    assert_eq!(ia_dirs, 8, "one IA1 directory per world group");
    assert_eq!(mp_dirs, 21, "MP1 and MP3 in every world group, MP2 in five");
    let scenarios = ia_dirs + mp_dirs;
    let launchable = inventory.len() + scenarios;
    assert_eq!(
        scenarios, 29,
        "eight instant-action and 21 multiplayer scenario directories, measured 2026-10-03"
    );
    assert_eq!(
        launchable, 53,
        "the original installation declares 53 launchable roots: 24 campaign missions and 29 \
         scenario directories, measured 2026-10-03"
    );
    assert_eq!(
        catalog.launchable_count(),
        launchable,
        "every campaign mission and scenario directory is a declared launchable row"
    );
    assert_eq!(catalog.original_launchable_count(), launchable);
    assert_eq!(baseline.roots.len(), launchable);
    assert_eq!(baseline.coverage.roots, launchable);
    assert_eq!(
        catalog.unsupported_count(),
        launchable,
        "no launchable row is filtered out of the count, however unsupported it is"
    );
    for (kind, expected) in [
        (ContentKind::Mission, inventory.len()),
        (ContentKind::IaScenario, ia_dirs),
        (ContentKind::MultiplayerScenario, mp_dirs),
    ] {
        let rows = catalog
            .elements()
            .filter(|element| element.kind == kind)
            .count();
        assert_eq!(rows, expected, "{} rows", kind.label());
    }
    // Every reader directory is accounted for: the launchable scenario
    // directories, one world-group reader per world-group directory and the
    // install-wide reader are classified, and nothing is left unrecognized.
    assert_eq!(
        baseline.classified_reader_dirs.len(),
        scenarios + groups + 1
    );
    assert_eq!(
        groups, 8,
        "one world-group reader per world-group directory"
    );
    assert_eq!(
        baseline
            .classified_reader_dirs
            .iter()
            .filter(|dir| dir.role == ReaderDirRole::WorldGroupReader)
            .count(),
        groups
    );
    assert_eq!(
        baseline
            .classified_reader_dirs
            .iter()
            .filter(|dir| dir.role == ReaderDirRole::SharedReader)
            .count(),
        1
    );
    assert!(
        baseline.unrecognized_program_dirs.is_empty(),
        "unclassified: {:?}",
        baseline.unrecognized_program_dirs
    );
    assert_eq!(
        baseline
            .classified_reader_dirs
            .iter()
            .filter(|dir| dir.role.is_launchable())
            .count(),
        scenarios
    );
    assert!(
        !catalog.is_fully_ready(),
        "no mission program is decoded yet"
    );

    // Completeness of the inventory: one row per inventoried file.
    let discovery = install_api::discover(&game_dir)
        .expect("production discovery reads the original installation");
    let install_rows = catalog
        .elements()
        .filter(|element| element.kind == ContentKind::InstallFile)
        .count();
    assert_eq!(
        install_rows,
        discovery.manifest.files.len(),
        "every inventoried file of the original installation is a catalog row"
    );
    assert_eq!(
        install_rows, 228,
        "the original installation inventories 228 regular files, measured 2026-10-03"
    );
    // F14-D.2 added the first of the collections this stage leaves open: the
    // multiplayer modes the installation's string image names. F14-D.3 added
    // the world groups whose shared readers the classifier read, F14-D.4 the
    // scene nodes and mesh slots of every GameZ container the diagnosis names,
    // F14-D.5 the faction paint patterns and the verified paint masks, F14-D.6
    // the airframes the loading-script container declares, and F14-D.7 the audio
    // cues the sound family holds. None of them is launchable, so the denominator
    // asserted above is unchanged, but the row count is not, and this test states
    // the new totals instead of filtering the rows out.
    let rows_of = |kind: ContentKind| {
        catalog
            .elements()
            .filter(|element| element.kind == kind)
            .count()
    };
    let mode_rows = rows_of(ContentKind::MultiplayerRules);
    assert_eq!(
        mode_rows, 4,
        "the measured mode-name run of the installation's string image"
    );
    let world_rows = rows_of(ContentKind::World);
    assert_eq!(
        world_rows, groups,
        "one world row per world group the shared reader of that group names"
    );
    // F14-D.6's own retail test pins which airframes these are, against the
    // roster F11-D2 measured; this one counts the rows so the total below stays
    // a complete accounting rather than a filtered one.
    let airframe_rows = rows_of(ContentKind::Airframe);
    assert_eq!(
        airframe_rows, 11,
        "one airframe row per declared root of the loading-script container; the identities \
         themselves are pinned by accept_f14_d_6_retail_…"
    );
    // F14-D.5's faction and paint-mask collections, F14-D.4's scene-node and mesh
    // collections, F14-D.7's sound collection and F14-D.8's stunt and scrapbook
    // collections are counted by their own acceptance tests, which pin the
    // identities; this one does not restate a number another stage owns. Their
    // measured row counts are the floor `MEASURED_COLLECTIONS` below, which is
    // where their accounting lives now. None of them is launchable, so the
    // denominator asserted above is unchanged; the row count is not, and
    // nothing is filtered out to hide it.

    // Completeness is **derived from the kinds present**, which is what this
    // block used to get wrong. The equality it asserted compared the
    // non-launchable row count with a hand-written sum of the collections that
    // existed when the sum was written, so every later collection had to
    // remember to edit a list in another stage's test: F14-D.6 → F14-D.4,
    // F14-D.7 → F14-D.4 and F14-D.8 → F14-D.4 each broke it, and every time the
    // failure landed on a rebase of somebody else's branch instead of on the
    // collection that was forgotten. Every row now falls into exactly one
    // `CatalogRowRole`, the role is the kind's own classification
    // (`cs_types::content`, matched without a catch-all arm, so a new kind must
    // declare its role to compile), and the total below is what the roles add
    // up to.
    let accounting = account_catalog_rows(catalog.elements());
    assert!(
        accounting.is_complete(),
        "every catalog row falls into exactly one role: {accounting}"
    );
    assert_eq!(
        accounting.total,
        catalog.len(),
        "the accounting counts every row and no other: {accounting}"
    );
    assert_eq!(
        accounting.install_file, install_rows,
        "an install-file row is one inventoried file: {accounting}"
    );
    assert_eq!(
        accounting.launchable, launchable,
        "a launchable row is one of the three scenario kinds: {accounting}"
    );
    assert_eq!(
        accounting.program, launchable,
        "each launchable row is read from exactly one program archive and every \
         program archive belongs to one, so an orphan program row is a defect: \
         {accounting}"
    );
    // The non-launchable collection rows are whatever the source-derived kinds
    // present add up to — including a kind this test never names, which is the
    // point. What is recorded here is the measurement, as a floor rather than
    // an equality: a later collection raises these numbers without failing
    // here, and a collection that loses rows or disappears still does.
    const MEASURED_COLLECTIONS: [(&str, usize); 10] = [
        ("world", 8),
        ("scene_node", 56620),
        ("mesh", 17139),
        ("airframe", 11),
        ("paint_mask", 184),
        ("faction", 11),
        ("sound", 4951),
        ("multiplayer_rules", 4),
        // F14-D.8's two collections, merged while this branch was in review:
        // 45 `stunt_flying` fly-through targets and 461 `Mission_Spread_Item`
        // records. `accept_f14_d_8_retail_…` pins both exactly; here they are
        // the floor that says the collection did not lose rows. `CustomPlane`
        // contributes no row at all, so it has no floor.
        ("stunt", 45),
        ("scrapbook_item", 461),
    ];
    let collections = accounting.collections();
    for (kind, measured) in MEASURED_COLLECTIONS {
        let held = collections
            .iter()
            .find(|(label, _)| *label == kind)
            .map(|(_, rows)| *rows)
            .unwrap_or_else(|| {
                panic!("{kind} held {measured} rows on 2026-10-03 and is gone: {accounting}")
            });
        assert!(
            held >= measured,
            "{kind} held {measured} rows on 2026-10-03 and holds {held} now: {accounting}"
        );
    }
    let unaccounted = accounting.unaccounted();
    assert!(
        unaccounted >= 79434,
        "the non-launchable collection rows measured 79 434 on 2026-10-03 (228 inventoried \
         files, 53 launchable rows and their 53 programs, and the 506 F14-D.8 stunt and \
         scrapbook rows merged in while this branch was in review): {accounting}"
    );
    assert_eq!(
        catalog.len(),
        install_rows + 2 * launchable + unaccounted,
        "files plus one program row and one launchable row per mission and scenario, plus every \
         non-launchable collection row the catalog holds: {accounting}"
    );

    // Nothing authored reached the retail inventory, and every row is
    // located by the fingerprint production discovery measured.
    assert_eq!(catalog.synthetic_launchable_count(), 0);
    let install_sha = install_api::fingerprint(&discovery.manifest).to_hex();
    assert_eq!(baseline.install_sha256, install_sha);
    assert_eq!(
        baseline.content_sha256,
        install_api::content_fingerprint(&discovery.manifest).to_hex()
    );
    // The installation the published M01 binding cites is this one, so the
    // baseline's fingerprints and that binding's recorded fingerprint cannot
    // drift apart.
    let published = fs::read_to_string(workspace_root().join("missions/bindings/M01.json"))
        .expect("the published M01 binding reads");
    let cited = published
        .split("\"install_sha256\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("M01.json cites an installation fingerprint");
    assert_eq!(
        install_sha, cited,
        "the baseline fingerprints the installation the published binding cites"
    );
    for element in catalog.elements() {
        assert!(
            element.origin.is_original(),
            "a retail catalog row cannot be {}: {}",
            element.origin.label(),
            element.id
        );
        let span = element.origin.source().expect("a source span");
        assert_eq!(span.install_sha256().to_hex(), install_sha);
    }

    // Every mission id follows the published binding identity and every
    // mission reaches its program and its inventory row, with no orphaned
    // reference anywhere in the closure.
    for root in baseline
        .roots
        .iter()
        .filter(|root| root.kind() == ContentKind::Mission)
    {
        assert!(
            root.key().starts_with("ch") && root.key().contains("-m"),
            "the published mission identity is ch<chapter>-m<nn>, got {}",
            root.key()
        );
    }
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(
        baseline.coverage.reachable,
        3 * launchable,
        "each launchable reaches its program and the file holding its bytes"
    );
    assert_eq!(
        baseline.coverage.unreachable,
        catalog.len() - baseline.coverage.reachable
    );
    assert_eq!(baseline.coverage.ready, 0, "nothing is playable yet");
    assert_eq!(
        baseline.coverage.unreachable_needing_classification, baseline.coverage.unreachable,
        "every unreachable row is unknown and still needs a classification"
    );

    // AC04 on the retail installation: an authored launchable row added to
    // the retail catalog is counted beside the denominator, never inside it.
    let mut mixed = catalog.clone();
    let synthetic = cid(ContentKind::Mission, "m01-synthetic");
    mixed
        .insert(CatalogElement {
            kind: ContentKind::Mission,
            id: synthetic.clone(),
            display_name: None,
            origin: Origin::SyntheticFixture,
            dependencies: Vec::new(),
            parse_state: ParseState::Parsed,
            normalize_state: NormalizeState::Normalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Ready,
            unsupported_reasons: Vec::new(),
            fingerprint: None,
        })
        .expect("the row validates");
    mixed
        .declare_launchable(&synthetic)
        .expect("a mission is launchable");
    assert_eq!(
        mixed.original_launchable_count(),
        launchable,
        "the retail denominator is the installation's, not the catalog's"
    );
    assert_eq!(mixed.synthetic_launchable_count(), 1);
    assert!(!mixed.is_retail_ready());

    // The retail report names the installation and its fingerprints and
    // never renders a synthetic origin.
    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"retail\":true"));
    assert!(report.contains(&format!("\"install_sha256\":\"{install_sha}\"")));
    assert!(report.contains(&format!("\"launchable\":{launchable}")));
    assert!(report.contains("\"synthetic_launchable\":0"));
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "the retail report holds no authored row"
    );
}

/// Counts the world-group directories under `ZBD/` and the `IA<n>` and `MP<n>`
/// directories inside them by walking the tree directly, independent of the
/// production classifier.
fn scenario_directories(game_dir: &Path) -> (usize, usize, usize) {
    let (mut ia, mut mp, mut groups) = (0, 0, 0);
    for group in fs::read_dir(game_dir.join("ZBD"))
        .expect("ZBD reads")
        .flatten()
    {
        if !group.path().is_dir() {
            continue;
        }
        groups += 1;
        for leaf in fs::read_dir(group.path())
            .expect("a world group reads")
            .flatten()
        {
            let name = leaf.file_name().to_string_lossy().to_ascii_lowercase();
            if !leaf.path().is_dir() || !leaf.path().join("zrdr.zbd").is_file() {
                continue;
            }
            if name.starts_with("ia") {
                ia += 1;
            } else if name.starts_with("mp") {
                mp += 1;
            }
        }
    }
    (ia, mp, groups)
}

/// The workspace root, for reading the frozen denominator beside the tests.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("the workspace root")
        .to_path_buf()
}
