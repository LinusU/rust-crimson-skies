//! Acceptance scenario F14-D.6: the `airframe` collection of the retail baseline
//! inventory (follow-up task #489 of F14-D.2 / #389; stage `### F14-D` of
//! `specs/F14-canonical-content-catalog-and-dependency-closure.md`).
//!
//! `docs/contracts/IDENTITY-CONTENT.md` requires "airframes and exceptional
//! control laws" as a catalog collection. The baseline inventory had no
//! `ContentKind::Airframe` row at all, so its report's `collections` object had
//! no `airframe` entry, and every airframe id in the workspace was a synthetic
//! fixture.
//!
//! **What this stage adds.** One `ContentKind::Airframe` row per airframe the
//! installation's **loading-script container** (`ZBD/interp.zbd`) declares, read
//! by the *producing* stage's own discovery — F11-D2's production
//! `cs_content::scene::discover_airframe_roster` — over the decoded container.
//! Each row carries `Origin::Installation` over a **measured** byte extent: the
//! stored record of the line that *named* that airframe's root, looked up in the
//! same decoded container the discovery walked rather than taken from a number
//! written down here. The identity is the root the original bound
//! (`airframe/<root>`), never the model spelling it loaded, and one `Static`
//! edge with `observed_tool` provenance points at the inventory row of the
//! container holding those bytes.
//!
//! **What this stage refuses.** A container that does not read, or that does not
//! declare an airframe, yields *no* row: the reason is counted in the collection
//! record's `gaps` and named in its `diagnostic`. No airframe is invented from a
//! model name, a scene node or a UI message key, and a row the discovery could
//! not complete is never dropped — the discovery's own findings and unknowns are
//! on the record.
//!
//! The non-retail tests write **synthetic installation trees** into temporary
//! directories: a version-one reader archive for the campaign mission and the
//! world group (so the shared campaign walk and F14-D.1's classifier read
//! them), plus an authored INTERP container whose scripts mirror the measured
//! retail idiom in *structure only*. Every model spelling, root name and byte
//! below is authored for this file; nothing is derived from original data. They
//! exercise production code only — `retail_baseline` over `cs_assets::install::
//! discover`, the shared campaign walk, `reader_dirs::classify`,
//! `cs_formats::interp::decode_interp` and `discover_airframe_roster` — so
//! removing the collection fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation and pins the rows it really holds. Run it with
//! `--include-ignored`; without `CS_GAME_DIR` it fails loudly rather than
//! passing vacuously.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    AIRFRAME_SCRIPT_IMAGE, AIRFRAME_TUNING_CLAIM, baseline_report_json, install_file_key,
    retail_baseline,
};
use cs_formats::zbd::{
    INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES, TRAILER_VERSION_ONE,
};
use cs_types::content::{ContentId, ContentKind, NormalizeState, Readiness, UnsupportedReason};
use cs_types::evidence::ClaimStatus;
use cs_types::install::ParseState;

/// The claim the airframe rows and their edges are recorded under.
const CLAIM: &str = "f14.d.6.baseline.airframe_declaration";

/// The producing stage's own claim that roster availability is undiscovered
/// (`cs_content::scene::AVAILABILITY_DISCOVERY_CLAIM`).
const AVAILABILITY_CLAIM: &str = "f11d.roster-availability-undiscovered";

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

/// The authored loading-script container every non-retail test writes: three
/// scripts that mirror the measured retail idiom's *structure* — an init script
/// that binds the container directory, a declaring script that binds a model and
/// a root per airframe and includes a surgery script once per airframe, and the
/// surgery script whose `NewObject3D %planeOutput%` line creates the root — and
/// declares `roots.len()` airframes.
fn declaring_interp(roots: &[&[u8]], extra_lines: &[Vec<u8>]) -> (Vec<u8>, Vec<u64>) {
    let mut declaring = vec![
        line(&[b"source", b"support\\init.gw"]),
        line(&[b"set", b"ZBDFile", b"%ZBD_DIR%\\planes.zbd"]),
    ];
    for (index, root) in roots.iter().enumerate() {
        let mut model = b"common\\planes\\fixture\\".to_vec();
        model.extend_from_slice(format!("plane{index}.flt").as_bytes());
        declaring.push(line(&[b"set", b"planeInput", &model]));
        declaring.push(line(&[b"set", b"planeOutput", root]));
        declaring.push(line(&[b"source", b"support\\util\\surgery.gw"]));
    }
    declaring.extend_from_slice(extra_lines);
    declaring.push(line(&[b"GameZWriteZBDFile", b"%ZBDFile%"]));
    interp_container(&[
        (
            b"support\\init.gw",
            vec![line(&[b"set", b"ZBD_DIR", b"zbd"])],
        ),
        (b"support\\planes.gw", declaring),
        (
            b"support\\util\\surgery.gw",
            vec![
                line(&[b"LoadGameGen", b"%planeInput%", b"fixture0001"]),
                line(&[b"NewObject3D", b"%planeOutput%"]),
            ],
        ),
    ])
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

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

/// The collection record of the `airframe` collection.
fn airframe_status(
    baseline: &cs_content::catalog::baseline::Baseline,
) -> &cs_content::catalog::baseline::CollectionStatus {
    baseline
        .collection_status
        .iter()
        .find(|status| status.kind == ContentKind::Airframe)
        .expect("the airframe collection reports its status")
}

/// The byte extent `support\planes.gw` occupies inside the fixture container, as
/// the production decoder measures it, so a row's span is checked against the
/// script it claims to come from.
fn declaring_script_extent(bytes: &[u8]) -> (u64, u64) {
    let decoded = cs_formats::interp::decode_interp(
        &mut cs_formats::ParseContext::with_defaults("zbd/interp.zbd"),
        bytes,
    )
    .expect("the authored container reads");
    let mut matching = decoded
        .scripts()
        .iter()
        .filter(|script| script.name().eq_ignore_ascii_case(b"support\\planes.gw"));
    let script = matching
        .next()
        .expect("the fixture holds the declaring script");
    assert!(matching.next().is_none(), "one declaring script");
    (u64::from(script.entry().script_offset), script.end())
}

/// The claim ids of one airframe row's explicit unknowns, in the row's order.
///
/// A row carries its unknowns under the claims of the stage that measured them;
/// a row that lost one, or that replaced an unknown with a value, changes this
/// list.
fn unknown_claims(row: &cs_types::content::CatalogElement) -> Vec<&str> {
    row.unsupported_reasons
        .iter()
        .filter_map(|reason| match reason {
            UnsupportedReason::Unknown { claim_id, .. } => Some(claim_id.as_str()),
            _ => None,
        })
        .collect()
}

/// The mapping arm: every airframe the producing discovery declares in the
/// loading-script container becomes one row, located by the line that named it
/// and pointing at the container's own inventory row.
#[test]
fn accept_f14_d_6_a_declared_roster_becomes_one_row_per_airframe() {
    let temp = campaign_tree("rows");
    let (bytes, line_offsets) = declaring_interp(&[b"player_first", b"player_second"], &[]);
    temp.write(AIRFRAME_SCRIPT_IMAGE, &bytes);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let catalog = &baseline.catalog;

    let rows: Vec<&cs_types::content::CatalogElement> = catalog
        .elements()
        .filter(|element| element.kind == ContentKind::Airframe)
        .collect();
    assert_eq!(
        rows.iter()
            .map(|row| row.id.key().to_owned())
            .collect::<Vec<_>>(),
        vec!["player_first".to_owned(), "player_second".to_owned()],
        "one row per declared root, keyed by the root the original bound and in canonical order"
    );
    assert_eq!(
        rows.iter()
            .map(|row| row.display_name.clone())
            .collect::<Vec<_>>(),
        vec![None, None],
        "the installation states no display name for an airframe, and the model spelling it \
         loaded is provenance, never a name"
    );

    let (script_start, script_end) = declaring_script_extent(&bytes);
    let container_len = bytes.len() as u64;
    let mut seen_offsets = Vec::new();
    for row in &rows {
        // The row's own bytes: the record of the line that named this root,
        // inside the installation whose bytes were read.
        assert!(row.origin.is_original());
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), AIRFRAME_SCRIPT_IMAGE);
        assert_eq!(span.member_key(), None, "a loose file is its own container");
        assert_eq!(
            span.install_sha256().to_hex(),
            baseline.install_sha256,
            "the span names the installation whose bytes were read"
        );
        assert!(span.length() > 0, "{}: a real record", row.id);
        assert!(
            span.offset() >= script_start && span.offset() + span.length() <= script_end,
            "{}: its span lies inside the declaring script's own extent {script_start}..{script_end}",
            row.id
        );
        assert!(
            span.offset() + span.length() <= container_len,
            "{}: its span lies inside the container",
            row.id
        );
        // And the span really covers a stored line record, not padding.
        assert!(
            line_offsets.contains(&span.offset()),
            "{}: the span starts at a stored line, not between two",
            row.id
        );
        seen_offsets.push(span.offset());

        // The line was read and decoded; nothing was converted and nothing
        // consumes it yet.
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(!row.is_ready());
        assert!(row.runtime_consumers.is_empty());
        assert_eq!(
            row.unsupported_codes(),
            vec!["unknown", "unknown", "missing_runtime_consumer"],
            "{}: the two facts it does not know, plus the missing consumer",
            row.id
        );
        assert_eq!(
            unknown_claims(row),
            [AIRFRAME_TUNING_CLAIM, AVAILABILITY_CLAIM],
            "{}: the statistics claim first, then the producing stage's own availability claim",
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
        assert!(
            row.unsupported_reasons
                .iter()
                .any(|reason| reason.code() == "missing_runtime_consumer"),
            "{}: the row also says that nothing consumes it yet",
            row.id
        );

        // The edge: onto the inventory row of the container that declared it.
        assert_eq!(row.dependencies.len(), 1, "{}: one static edge", row.id);
        let edge = &row.dependencies[0];
        let container = cid(
            ContentKind::InstallFile,
            &install_file_key(AIRFRAME_SCRIPT_IMAGE),
        );
        assert_eq!(edge.target, container);
        assert_eq!(edge.kind.label(), "static");
        assert_eq!(edge.provenance.claim_id.as_str(), CLAIM);
        assert_eq!(
            edge.provenance.class,
            ClaimStatus::ObservedTool,
            "an agent-observed edge is never verified_original"
        );
        assert_eq!(edge.provenance.source.as_ref(), Some(span));
        assert_eq!(
            row.fingerprint,
            catalog
                .get(&container)
                .expect("the container's inventory row")
                .fingerprint,
            "the row and the container's inventory row describe the same bytes"
        );
    }
    assert_eq!(seen_offsets.len(), 2, "two rows");
    assert_ne!(
        seen_offsets[0], seen_offsets[1],
        "the two rows are told apart by the line that named each root"
    );

    // The collection's own record: the walk read every line, and it explicitly
    // does not know two facts.
    let status = airframe_status(&baseline);
    assert_eq!(status.source, AIRFRAME_SCRIPT_IMAGE);
    assert_eq!(
        status.language, None,
        "a script container has no language dimension"
    );
    assert_eq!(status.rows, 2);
    assert_eq!(status.gaps.get("roster_issue"), Some(&0), "every line read");
    assert_eq!(
        status.gaps.get("roster_unknown"),
        Some(&2),
        "availability and forced assignments are undiscovered"
    );
    assert_eq!(status.boundary_id, None);
    assert_eq!(status.diagnostic, None);

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"airframe\":2"), "{report}");
    assert!(
        report.contains(&format!(
            "\"kind\":\"airframe\",\"source\":\"{AIRFRAME_SCRIPT_IMAGE}\",\"language\":null,\"rows\":2"
        )),
        "{report}"
    );
    assert!(
        report.contains("\"id\":\"airframe/player_first\""),
        "{report}"
    );
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

/// A container the walk reads but that declares no airframe yields no row: the
/// finding is counted on the record and the rest of the inventory is intact.
#[test]
fn accept_f14_d_6_a_container_that_declares_no_airframe_is_named_not_invented() {
    let temp = campaign_tree("no-airframe");
    // The container holds the declaring script but never creates a root, so the
    // walk finds no airframe rather than an unreadable one.
    let (bytes, _) = interp_container(&[(
        b"support\\planes.gw",
        vec![
            line(&[b"set", b"ZBDFile", b"zbd\\planes.zbd"]),
            line(&[b"GameZWriteZBDFile", b"%ZBDFile%"]),
        ],
    )]);
    temp.write(AIRFRAME_SCRIPT_IMAGE, &bytes);

    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| element.kind != ContentKind::Airframe),
        "a container that creates no root declares no airframe"
    );
    let status = airframe_status(&baseline);
    assert_eq!(status.rows, 0);
    assert_eq!(
        status.gaps.get("roster_unknown"),
        Some(&1),
        "forced assignments only"
    );
    assert_eq!(
        status.gaps.get("no_airframes_declared"),
        Some(&1),
        "the finding is counted under its own stable label"
    );
    let diagnostic = status.diagnostic.as_deref().expect("a named gap");
    assert!(diagnostic.contains(AIRFRAME_SCRIPT_IMAGE), "{diagnostic}");

    // The rest of the inventory survives: a collection's failure never takes the
    // missions with it.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.catalog.launchable_count(), 1);
    let report = baseline_report_json(&baseline);
    assert!(!report.contains("\"airframe\":"), "{report}");
}

/// A container that reads but holds no declaring script names that absence
/// instead of borrowing another script's bytes: the idiom's own provenance span
/// is measured from the declaring script, so a corpus without it has none, and
/// the discovery reports both findings rather than an empty roster.
#[test]
fn accept_f14_d_6_a_container_without_the_declaring_script_is_named_not_invented() {
    let temp = campaign_tree("no-declaring-script");
    // The container reads as the interp container the roster is declared in, and
    // carries a script of its own, but not the declaring one.
    let (bytes, _) = interp_container(&[(
        b"support\\init.gw",
        vec![line(&[b"set", b"ZBD_DIR", b"zbd"])],
    )]);
    temp.write(AIRFRAME_SCRIPT_IMAGE, &bytes);

    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| element.kind != ContentKind::Airframe),
        "no airframe is declared by a script that does not exist"
    );
    let status = airframe_status(&baseline);
    assert_eq!(status.rows, 0);
    assert_eq!(
        status.gaps.get("declaring_script_absent"),
        Some(&1),
        "the declaring script's absence is counted under its own stable label"
    );
    assert_eq!(
        status.gaps.get("no_airframes_declared"),
        Some(&1),
        "and the declaration that therefore declared nothing"
    );
    assert_eq!(
        status.gaps.get("roster_unknown"),
        Some(&1),
        "forced assignments only: no row exists to be unavailable"
    );
    let diagnostic = status.diagnostic.as_deref().expect("a named gap");
    assert!(diagnostic.contains(AIRFRAME_SCRIPT_IMAGE), "{diagnostic}");
    assert!(
        diagnostic.contains("holds no script named"),
        "the diagnostic quotes the finding itself, not only its label: {diagnostic}"
    );
    // The rest of the inventory survives a collection that found nothing.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.catalog.launchable_count(), 1);
}

/// A line the walk cannot read is a finding on the record, never a silently lost
/// row: the container still yields the airframe it really declares, and the
/// unreadable line is counted.
#[test]
fn accept_f14_d_6_an_unreadable_line_is_a_finding_and_the_other_row_survives() {
    let temp = campaign_tree("unreadable-line");
    // A `set` line stored with four arguments is not the declared shape, so the
    // walk refuses to invent the argument boundary it is missing.
    let (bytes, _) = declaring_interp(
        &[b"player_first"],
        &[line(&[
            b"set",
            b"planeOutput",
            b"player_extra",
            b"trailing",
        ])],
    );
    temp.write(AIRFRAME_SCRIPT_IMAGE, &bytes);

    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    assert_eq!(
        baseline
            .catalog
            .elements()
            .filter(|element| element.kind == ContentKind::Airframe)
            .count(),
        1,
        "the airframe the container really declares is still a row"
    );
    let status = airframe_status(&baseline);
    assert_eq!(status.rows, 1);
    assert_eq!(status.gaps.get("roster_issue"), Some(&1));
    assert_eq!(status.gaps.get("line_unreadable"), Some(&1));
    assert_eq!(status.diagnostic, None, "the collection holds rows");
}

/// A container the decoder refuses, and a container the installation does not
/// have at all, are both reported gaps rather than empty readings.
#[test]
fn accept_f14_d_6_an_unreadable_or_absent_loading_container_is_a_named_gap() {
    // (a) present but not an INTERP container at all.
    let temp = campaign_tree("unreadable");
    temp.write(AIRFRAME_SCRIPT_IMAGE, b"not a loading-script container");
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| element.kind != ContentKind::Airframe)
    );
    let status = airframe_status(&baseline);
    assert_eq!(status.source, AIRFRAME_SCRIPT_IMAGE);
    assert_eq!(status.rows, 0);
    let diagnostic = status.diagnostic.as_deref().expect("a named gap");
    assert!(diagnostic.contains(AIRFRAME_SCRIPT_IMAGE), "{diagnostic}");
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);

    // (b) absent: an installation with no loading-script container at all.
    let absent = campaign_tree("absent");
    let baseline = retail_baseline(&absent.0).expect("the fixture installation reads");
    let status = airframe_status(&baseline);
    assert_eq!(status.rows, 0);
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("a missing container is named, not dropped");
    assert!(
        diagnostic.contains(&format!("inventories no {AIRFRAME_SCRIPT_IMAGE}")),
        "{diagnostic}"
    );
    let report = baseline_report_json(&baseline);
    assert!(!report.contains("\"airframe\":"), "{report}");
}

/// Airframes are not launchable content, so this collection adds no root and
/// cannot move the coverage denominator; its rows stay visible as unreachable
/// unknowns.
#[test]
fn accept_f14_d_6_airframes_are_not_launchable_and_the_denominator_does_not_move() {
    let temp = campaign_tree("roots");
    let (bytes, _) = declaring_interp(&[b"player_first", b"player_second"], &[]);
    temp.write(AIRFRAME_SCRIPT_IMAGE, &bytes);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let catalog = &baseline.catalog;

    assert!(
        !ContentKind::Airframe.is_launchable(),
        "an airframe is not a mission or a scenario directory, so the denominator cannot move when \
         the collection is populated"
    );
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(catalog.launchable_count(), 1);
    assert_eq!(catalog.original_launchable_count(), 1);
    assert_eq!(catalog.synthetic_launchable_count(), 0);
    assert_eq!(baseline.coverage.roots, 1);
    assert_eq!(
        baseline.coverage.reachable, 3,
        "the mission, its program and the file holding its bytes; nothing reaches an airframe yet"
    );
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(
        baseline
            .coverage
            .unreachable_by_kind
            .get("airframe")
            .copied(),
        Some(2),
        "no row points at an airframe yet, so the rows stay counted as unreachable unknowns"
    );
    assert!(baseline.coverage.unreachable_needing_classification >= 2);
    assert!(!catalog.is_fully_ready());
    assert!(!catalog.is_retail_ready());
}

// ------------------------------------------------------------- retail ----

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The eleven airframes `ZBD/interp.zbd` declares, as this project's own
/// normalized identities.
///
/// This is F11-D2's measured roster (`docs/findings/
/// 2026-10-02-f11-d-2-airframe-roster-discovery.md`), transcribed as catalog ids
/// — the roots the original container's `support\planes.gw` created — so the
/// inventory is pinned against that measured list and not against a count.
const RETAIL_DECLARED_ROOTS: [&str; 11] = [
    "player_pfighter",
    "player_bhawk",
    "player_fbrand",
    "player_brigand",
    "player_fury",
    "player_autogyro",
    "player_avenger",
    "player_kestrel",
    "player_peacemaker",
    "player_warhawk",
    "player_balmoral",
];

/// The retail half: every airframe the loading-script container declares becomes
/// one `airframe` row, located by the line that named it, pointing at the
/// container's inventory row, and the coverage denominator is unchanged.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_6_retail_the_installation_declares_its_shared_airframe_roster() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let catalog = &baseline.catalog;
    let discovery = cs_assets::install::discover(&dir).expect("production discovery reads it");
    let install_sha = cs_assets::install::fingerprint(&discovery.manifest).to_hex();

    let rows: Vec<&cs_types::content::CatalogElement> = catalog
        .elements()
        .filter(|element| element.kind == ContentKind::Airframe)
        .collect();
    let mut expected: Vec<String> = RETAIL_DECLARED_ROOTS
        .iter()
        .map(|root| (*root).to_owned())
        .collect();
    expected.sort();
    assert_eq!(
        rows.iter()
            .map(|row| row.id.key().to_owned())
            .collect::<Vec<_>>(),
        expected,
        "the roster is exactly F11-D2's measured one; the catalog renders it in canonical id \
         order, while the declaration order the container creates them in is pinned by F11-D2's \
         own acceptance test"
    );
    assert!(
        RETAIL_DECLARED_ROOTS.contains(&"player_autogyro"),
        "the exceptional autogyro configuration the same declaring script builds is one of the \
         declared roots (F25)"
    );

    // The declaring script's own extent, measured by the production decoder over
    // the container's real bytes, so each row's span is checked against the
    // script it claims to come from.
    let container_bytes =
        fs::read(dir.join(AIRFRAME_SCRIPT_IMAGE)).expect("the loading-script container reads");
    let record = discovery
        .manifest
        .files
        .iter()
        .find(|record| {
            record
                .relative_spelling
                .as_str()
                .eq_ignore_ascii_case(AIRFRAME_SCRIPT_IMAGE)
        })
        .expect("the installation inventories the loading-script container");
    let decoded = cs_formats::interp::decode_interp(
        &mut cs_formats::ParseContext::with_defaults(AIRFRAME_SCRIPT_IMAGE),
        &container_bytes,
    )
    .expect("the real loading-script container reads");
    let mut declaring = decoded
        .scripts()
        .iter()
        .filter(|script| script.name().eq_ignore_ascii_case(b"support\\planes.gw"));
    let script = declaring.next().expect("the container declares the roster");
    assert!(declaring.next().is_none(), "one declaring script");
    let (script_start, script_end) = (u64::from(script.entry().script_offset), script.end());

    let mut offsets = Vec::new();
    for row in &rows {
        assert!(row.origin.is_original(), "{}", row.id);
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), AIRFRAME_SCRIPT_IMAGE);
        assert_eq!(span.install_sha256().to_hex(), install_sha);
        assert!(span.length() > 0, "{}", row.id);
        assert!(
            span.offset() >= script_start && span.offset() + span.length() <= script_end,
            "{}: the span lies inside the declaring script's own extent",
            row.id
        );
        assert!(
            span.offset() + span.length() <= record.size_bytes,
            "{}: the span lies inside the container",
            row.id
        );
        // The span really is a stored line record of that container.
        let starts_a_line = decoded
            .scripts()
            .iter()
            .flat_map(cs_formats::interp::InterpScript::lines)
            .any(|line| line.offset() == span.offset());
        assert!(
            starts_a_line,
            "{}: the span starts at a stored line",
            row.id
        );
        offsets.push(span.offset());

        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(!row.is_ready());
        assert!(row.runtime_consumers.is_empty());
        assert_eq!(
            row.unsupported_codes(),
            vec!["unknown", "unknown", "missing_runtime_consumer"],
            "{}",
            row.id
        );
        assert_eq!(
            unknown_claims(row),
            [AIRFRAME_TUNING_CLAIM, AVAILABILITY_CLAIM],
            "{}",
            row.id
        );
        assert_eq!(row.display_name, None, "{}", row.id);

        assert_eq!(row.dependencies.len(), 1, "{}: one static edge", row.id);
        let edge = &row.dependencies[0];
        let container = cid(
            ContentKind::InstallFile,
            &install_file_key(AIRFRAME_SCRIPT_IMAGE),
        );
        assert_eq!(edge.target, container, "{}", row.id);
        assert_eq!(edge.kind.label(), "static");
        assert_eq!(edge.provenance.claim_id.as_str(), CLAIM, "{}", row.id);
        assert_eq!(
            edge.provenance.class,
            ClaimStatus::ObservedTool,
            "{}",
            row.id
        );
        assert_eq!(edge.provenance.source.as_ref(), Some(span), "{}", row.id);
        assert_eq!(
            row.fingerprint.as_ref().map(|digest| digest.sha256),
            Some(record.sha256),
            "{} fingerprints the bytes its naming line came from",
            row.id
        );
    }
    let distinct: BTreeMap<u64, &str> = offsets
        .iter()
        .zip(rows.iter())
        .map(|(offset, row)| (*offset, row.id.key()))
        .collect();
    assert_eq!(
        distinct.len(),
        rows.len(),
        "each row is located by its own naming line"
    );

    let status = airframe_status(&baseline);
    assert_eq!(status.source, AIRFRAME_SCRIPT_IMAGE);
    assert_eq!(status.language, None);
    assert_eq!(status.rows, 11);
    assert_eq!(
        status.gaps.get("roster_issue"),
        Some(&0),
        "the measured corpus reads completely: every declared line is read"
    );
    assert_eq!(
        status.gaps.get("roster_unknown"),
        Some(&2),
        "availability and forced mission assignments are undiscovered"
    );
    assert_eq!(status.diagnostic, None);

    // The denominator did not move: an airframe is not launchable content.
    assert!(!ContentKind::Airframe.is_launchable());
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Airframe),
        "no airframe row is a closure root"
    );
    assert_eq!(catalog.launchable_count(), baseline.roots.len());
    assert_eq!(baseline.coverage.roots, baseline.roots.len());
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(
        baseline
            .coverage
            .unreachable_by_kind
            .get("airframe")
            .copied(),
        Some(11),
        "no row points at an airframe yet, so the rows stay counted as unreachable unknowns"
    );
    assert!(!catalog.is_fully_ready());

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"airframe\":11"), "{report}");
    assert!(
        report.contains(&format!(
            "\"kind\":\"airframe\",\"source\":\"{AIRFRAME_SCRIPT_IMAGE}\",\"language\":null,\"rows\":11"
        )),
        "{report}"
    );
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );
}

/// The report's own accounting cannot disagree with the collection record: the
/// `collections` count and the record's `rows` come from the same walk, and a
/// row whose edge pointed at a file the inventory does not hold would be an
/// unresolved reference rather than a row.
#[test]
fn accept_f14_d_6_the_airframe_edges_resolve_and_the_counts_agree() {
    let temp = campaign_tree("edges");
    let (bytes, _) = declaring_interp(&[b"player_first"], &[]);
    temp.write(AIRFRAME_SCRIPT_IMAGE, &bytes);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let catalog = &baseline.catalog;
    let status = airframe_status(&baseline);

    let rows = catalog
        .elements()
        .filter(|element| element.kind == ContentKind::Airframe)
        .count();
    assert_eq!(status.rows, rows, "the record and the catalog agree");
    let report = baseline_report_json(&baseline);
    assert_eq!(
        report.matches("\"kind\":\"airframe\"").count(),
        2,
        "the report names the collection once and its single row once: {report}"
    );
    assert_eq!(
        report.matches("\"id\":\"airframe/").count(),
        1,
        "one row in the element array: {report}"
    );
    for element in catalog
        .elements()
        .filter(|element| element.kind == ContentKind::Airframe)
    {
        for dependency in &element.dependencies {
            assert!(
                catalog.get(&dependency.target).is_some(),
                "{}: its edge points at a row this inventory holds",
                element.id
            );
        }
    }
    assert!(
        !Path::new(AIRFRAME_SCRIPT_IMAGE).is_absolute(),
        "the collection names an installation-relative spelling"
    );
}
