//! Acceptance scenario F14-D.2: the `multiplayer_rules` collection of the
//! retail baseline inventory (`specs/F14-canonical-content-catalog-and-
//! dependency-closure.md`, stage `### F14-D`; follow-up task #389).
//!
//! The baseline inventory of the original installation (`cs_content::catalog:
//! :baseline::retail_baseline`) populated three of the collections
//! `docs/contracts/IDENTITY-CONTENT.md` requires: install files, campaign
//! missions and mission programs. This stage adds the first of the remaining
//! ones — the multiplayer modes the installation's string image names — read
//! by the **producing stage's own parser** (`cs_content::multiplayer::
//! discover_modes` over `cs_content::config::StringCatalog`), not by a reader
//! derived here.
//!
//! The non-retail tests write a **synthetic installation tree** into a
//! temporary directory whose `strings.dll` is an authored PE image holding the
//! mode-name run and the briefing blocks F56-A reads. They exercise production
//! code only, so removing the collection from the baseline fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! installation and pins the rows it really holds; run it with
//! `--include-ignored`. Without `CS_GAME_DIR` it fails loudly rather than
//! passing vacuously.
//!
//! Every string in the synthetic images below is authored for this file, like
//! the fixtures in `crates/cs_content/tests/accept_f56_a_catalog.rs`; no
//! original text is written to Git.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    Baseline, MODE_STRING_IMAGE, MODE_STRING_LANGUAGE, baseline_report_json, install_file_key,
    retail_baseline,
};
use cs_content::multiplayer::{
    BRIEFING_FIRST_ID, BRIEFING_STRIDE, MODE_NAME_IDS, RULE_LABELS, mode_name_id,
};
use cs_formats::{RT_STRING, STRING_UNITS_PER_BLOCK};
use cs_types::content::{
    ContentId, ContentKind, NormalizeState, Origin, Readiness, UnsupportedReason,
};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::ParseState;

/// The mode-name ids F56-A measured; the synthetic table stores exactly this
/// run so the production walk finds it.
fn name_ids() -> Vec<u32> {
    MODE_NAME_IDS.collect()
}

/// The synthetic mode table: four named modes, each with a briefing block of
/// the multiplayer family, and one block of another family that ends the
/// walk. The texts are authored here.
fn fixture_table() -> Vec<(u32, String)> {
    let mut rows = vec![
        (*MODE_NAME_IDS.start() - 1, "a column heading".to_owned()),
        (7011, "Free Fight".to_owned()),
        (7012, "Team Fight".to_owned()),
        (7013, "Flag Grab".to_owned()),
        (7014, "Blimp Duel".to_owned()),
        (7015, "TCP/IP".to_owned()),
    ];
    let blocks: [(u32, &str, &[&str], &[&str]); 4] = [
        (
            BRIEFING_FIRST_ID,
            "FREE FIGHT",
            &["shoot them", "do not crash"],
            &["2", "-2"],
        ),
        (
            BRIEFING_FIRST_ID + BRIEFING_STRIDE,
            "TEAM FIGHT",
            &["shoot them", "your team scores"],
            &["2", "-2"],
        ),
        (
            BRIEFING_FIRST_ID + 2 * BRIEFING_STRIDE,
            "FLAG GRAB",
            &["take it", "bring it back"],
            &["10", "2", "-2"],
        ),
        (
            BRIEFING_FIRST_ID + 3 * BRIEFING_STRIDE,
            "BLIMP DUEL",
            &["sink theirs", "protect yours"],
            &["10", "-2"],
        ),
    ];
    for (base, title, instructions, points) in blocks {
        rows.push((base, title.to_owned()));
        rows.push((base + 1, "a tagline".to_owned()));
        rows.push((base + 2, "POINTS".to_owned()));
        rows.push((base + 3, "INSTRUCTIONS".to_owned()));
        let mut next = base + 4;
        for text in instructions {
            rows.push((next, (*text).to_owned()));
            next += 1;
        }
        for text in points {
            rows.push((next, (*text).to_owned()));
            next += 1;
        }
    }
    // The first block of another family: the measured end of the table, which
    // is what makes the block count a measurement instead of an assumption.
    let other = BRIEFING_FIRST_ID + 4 * BRIEFING_STRIDE;
    rows.push((other, "INSTANT ACTION".to_owned()));
    rows.push((other + 1, "DOGFIGHT AN ACE.".to_owned()));
    rows.push((other + 2, "a different family".to_owned()));
    rows
}

/// The fixture table with the third mode's name spelled so that it pairs with
/// no briefing, while every briefing block is still there.
///
/// The producing stage's own gap: a named mode with no briefing and a briefing
/// with no name. (Deleting a briefing *block* is not a way to make this
/// fixture: the walk is self-terminating, so the first absent block ends it
/// and every later briefing would be skipped rather than reported.)
fn fixture_table_with_an_unpaired_name() -> Vec<(u32, String)> {
    fixture_table()
        .into_iter()
        .map(|(id, text)| {
            if id == 7013 {
                (id, "Flag Snatch".to_owned())
            } else {
                (id, text)
            }
        })
        .collect()
}

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-2-{label}-{}-{}",
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

/// One campaign mission directory, so the baseline has a launchable root and
/// the coverage accounting has something to walk.
fn write_campaign(temp: &TempInstall) {
    temp.write("ZBD/C1C/M01/zrdr.zbd", b"mission program bytes");
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
}

fn tree(label: &str, table: &[(u32, String)]) -> TempInstall {
    let temp = TempInstall::new(label);
    write_campaign(&temp);
    temp.write(MODE_STRING_IMAGE, &string_image(table));
    temp
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

fn rules_row(baseline: &Baseline, name_id: u32) -> &cs_types::content::CatalogElement {
    baseline
        .catalog
        .get(&cid(
            ContentKind::MultiplayerRules,
            &format!("mode.name-{name_id}"),
        ))
        .unwrap_or_else(|| panic!("the mode row of name {name_id}"))
}

fn unknown_claims(element: &cs_types::content::CatalogElement) -> Vec<String> {
    element
        .unsupported_reasons
        .iter()
        .filter_map(|reason| match reason {
            UnsupportedReason::Unknown { claim_id, .. } => Some(claim_id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

/// The complete collection: one row per named mode, each located by the span
/// of the string block its name was read from, each pointing at the inventory
/// row of the image that holds its bytes, and each carrying the rules F56-A
/// could not resolve as typed unknowns.
#[test]
fn accept_f14_d_2_a_synthetic_installation_yields_source_derived_mode_rows() {
    let table = fixture_table();
    let temp = tree("rows", &table);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    // Four rows, one per named mode, identified by F56-A's name-string id and
    // never by their position in a walk.
    let rules: Vec<ContentId> = baseline
        .catalog
        .elements()
        .filter(|element| element.kind == ContentKind::MultiplayerRules)
        .map(|element| element.id.clone())
        .collect();
    assert_eq!(
        rules,
        vec![
            cid(ContentKind::MultiplayerRules, "mode.name-7011"),
            cid(ContentKind::MultiplayerRules, "mode.name-7012"),
            cid(ContentKind::MultiplayerRules, "mode.name-7013"),
            cid(ContentKind::MultiplayerRules, "mode.name-7014"),
        ],
        "one row per named mode, in canonical id order"
    );

    let image_id = cid(
        ContentKind::InstallFile,
        &install_file_key(MODE_STRING_IMAGE),
    );
    let image_row = baseline
        .catalog
        .get(&image_id)
        .expect("the string image is inventoried like any other file");

    for (index, name_id) in name_ids().into_iter().enumerate() {
        let row = rules_row(&baseline, name_id);
        // The row's own bytes: the `RT_STRING` block its name was read from,
        // checked by the production PE reader, never the file as a whole.
        let span = row.origin.source().expect("an installation span");
        assert!(matches!(row.origin, Origin::Installation { .. }));
        assert_eq!(span.container_path(), MODE_STRING_IMAGE);
        assert_eq!(span.member_key(), None, "a loose file is its own container");
        assert!(
            span.offset() > 0,
            "a string block inside the image, not the image header"
        );
        assert!(span.length() > 0);
        assert_eq!(
            span.install_sha256().to_hex(),
            baseline.install_sha256,
            "the span names the installation whose bytes were read"
        );
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(row.runtime_consumers.is_empty());

        // The edge into the closure: the inventory row of the image.
        assert_eq!(row.dependencies.len(), 1, "one static edge per mode row");
        let edge = &row.dependencies[0];
        assert_eq!(edge.target, image_id);
        assert_eq!(edge.kind.label(), "static");
        assert_eq!(
            edge.provenance.class,
            ClaimStatus::ObservedTool,
            "an agent-observed edge is never verified_original"
        );
        assert_eq!(
            row.fingerprint, image_row.fingerprint,
            "the row and its inventory row describe the same bytes"
        );
        assert_eq!(
            row.display_name.as_deref(),
            Some(["Free Fight", "Team Fight", "Flag Grab", "Blimp Duel"][index])
        );
    }

    // What F56-A could not resolve stays on the row: every rule the mode
    // leaves unknown, with that stage's own claim id.
    let free_fight = rules_row(&baseline, 7011);
    assert_eq!(
        unknown_claims(free_fight),
        RULE_LABELS
            .iter()
            .map(|label| format!("f56.mode.name-7011.{label}"))
            .collect::<Vec<_>>(),
        "a mode whose team play the table does not state is unknown for every rule label"
    );
    let team_fight = rules_row(&baseline, 7012);
    let team_claims = unknown_claims(team_fight);
    assert_eq!(team_claims.len(), RULE_LABELS.len() - 1);
    assert!(
        !team_claims.contains(&"f56.mode.name-7012.teams".to_owned()),
        "the briefing states team play, so that rule is not unknown"
    );
    for row in [free_fight, team_fight] {
        for reason in &row.unsupported_reasons {
            if let UnsupportedReason::Unknown { reason, .. } = reason {
                assert!(
                    !reason.trim().is_empty(),
                    "a typed unknown must say what is not known"
                );
            }
        }
    }

    // The rules are not launchable, so the denominator is exactly the
    // campaign walk's and the coverage accounting keeps the rows visible as
    // unreachable unknowns.
    let mission = cid(ContentKind::Mission, "ch1-m01");
    assert_eq!(baseline.roots, vec![mission]);
    assert_eq!(baseline.catalog.launchable_count(), 1);
    assert_eq!(baseline.catalog.unsupported_count(), 1);
    assert!(!baseline.catalog.is_retail_ready());
    assert_eq!(baseline.coverage.roots, 1);
    assert_eq!(
        baseline.coverage.reachable, 3,
        "the mission, program and file"
    );
    assert_eq!(
        baseline
            .coverage
            .unreachable_by_kind
            .get("multiplayer_rules"),
        Some(&4)
    );
    assert!(
        baseline.coverage.unreachable_needing_classification >= 4,
        "the mode rows are unknown content that still needs a classification"
    );

    // The collection's own record and the report agree with the catalog.
    let status = baseline
        .collection_status
        .iter()
        .find(|status| status.kind == ContentKind::MultiplayerRules)
        .expect("the collection reports its status");
    assert_eq!(status.source, MODE_STRING_IMAGE);
    assert_eq!(status.language, Some(MODE_STRING_LANGUAGE));
    assert_eq!(status.rows, 4);
    assert_eq!(status.gaps.get("name_without_briefing"), Some(&0));
    assert_eq!(status.gaps.get("briefing_without_name"), Some(&0));
    assert_eq!(
        status.boundary_id,
        Some(BRIEFING_FIRST_ID + 4 * BRIEFING_STRIDE),
        "the walk ends at the first block of another family"
    );
    assert_eq!(status.diagnostic, None, "the parser read its source");

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"multiplayer_rules\":4"), "{report}");
    assert!(
        report.contains(&format!(
            "\"kind\":\"multiplayer_rules\",\"source\":\"{MODE_STRING_IMAGE}\",\"language\":{MODE_STRING_LANGUAGE},\"rows\":4"
        )),
        "{report}"
    );
    assert!(report.contains("\"launchable\":1"), "{report}");
    assert!(report.contains("\"diagnostic\":null"), "{report}");
    assert_eq!(
        report,
        baseline_report_json(&retail_baseline(&temp.0).expect("re-read")),
        "the report is byte-stable for the same installation"
    );
}

/// A named mode with no briefing stays an **inventory row with an explicit
/// unknown**: the three rows the parser did pair are present, the fourth name is
/// a row too (its identity and its bytes are both known), it says in one typed
/// unknown that no briefing of the family answers it, and the briefing that
/// pairs with no name — which has no identity to be given — is counted beside
/// it rather than becoming a row.
#[test]
fn accept_f14_d_2_a_named_mode_without_a_briefing_is_a_row_with_an_explicit_unknown() {
    let temp = tree("gap", &fixture_table_with_an_unpaired_name());
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rules = baseline
        .catalog
        .elements()
        .filter(|element| element.kind == ContentKind::MultiplayerRules)
        .map(|element| element.id.key().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        rules,
        [
            "mode.name-7011",
            "mode.name-7012",
            "mode.name-7013",
            "mode.name-7014"
        ],
        "a collection cannot exclude an entry it failed to complete"
    );

    // The unpaired name is a real row built from the name row's own bytes,
    // with the identity the producing stage derives and no borrowed briefing.
    let unpaired = rules_row(&baseline, 7013);
    assert_eq!(
        unpaired.id,
        mode_name_id(7013).expect("the producing stage's identity"),
        "the identity comes from the stage that owns it, so a paired and an unpaired name \
         cannot disagree about what a mode is called"
    );
    assert_eq!(unpaired.display_name.as_deref(), Some("Flag Snatch"));
    assert_eq!(unpaired.parse_state, ParseState::Parsed);
    assert_eq!(unpaired.readiness, Readiness::Unavailable);
    assert!(unpaired.runtime_consumers.is_empty());
    let span = unpaired.origin.source().expect("an installation span");
    assert_eq!(span.container_path(), MODE_STRING_IMAGE);
    assert!(
        span.offset() > 0 && span.length() > 0,
        "the name's own string block, not the image header"
    );
    assert_eq!(unpaired.dependencies.len(), 1);
    assert_eq!(
        unpaired.dependencies[0].target,
        cid(
            ContentKind::InstallFile,
            &install_file_key(MODE_STRING_IMAGE)
        )
    );
    // One unknown, and it says the whole mode is unresolved rather than
    // listing rules that were never read.
    assert_eq!(
        unknown_claims(unpaired),
        vec!["f14.d.2.baseline.mode_pairing".to_owned()],
        "the unresolved pairing is this baseline's own claim, not a borrowed rule list"
    );
    let reason = unpaired
        .unsupported_reasons
        .iter()
        .find_map(|reason| match reason {
            UnsupportedReason::Unknown { reason, .. } => Some(reason.as_str()),
            _ => None,
        })
        .expect("the unpaired name names what is not known");
    assert!(reason.contains("7013"), "{reason}");
    assert!(reason.contains("briefing"), "{reason}");

    // The paired rows are untouched by the gap.
    assert_eq!(
        unknown_claims(rules_row(&baseline, 7011)).len(),
        RULE_LABELS.len()
    );

    let status = &baseline.collection_status[0];
    assert_eq!(status.rows, 4, "every name the table carries is a row");
    assert_eq!(status.gaps.get("name_without_briefing"), Some(&1));
    assert_eq!(
        status.gaps.get("briefing_without_name"),
        Some(&1),
        "a briefing with no name has no identity, so it can only be counted"
    );
    assert_eq!(
        status.boundary_id,
        Some(BRIEFING_FIRST_ID + 4 * BRIEFING_STRIDE),
        "the walk still reaches the end of the family"
    );
    assert_eq!(status.diagnostic, None);
}

/// A string image the producing parser cannot read leaves the collection
/// empty **with a named diagnostic**, and the rest of the inventory is still
/// built: a reader of the report must be able to tell an installation with no
/// mode table from one this engine could not read.
#[test]
fn accept_f14_d_2_a_mode_table_the_parser_cannot_read_is_named_not_dropped() {
    let temp = TempInstall::new("unreadable");
    write_campaign(&temp);
    temp.write(MODE_STRING_IMAGE, b"not a PE image at all");
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| element.kind != ContentKind::MultiplayerRules),
        "no row is invented from a file name"
    );
    let status = &baseline.collection_status[0];
    assert_eq!(status.rows, 0);
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("an unreadable source is named, not dropped");
    assert!(diagnostic.contains(MODE_STRING_IMAGE), "{diagnostic}");

    // The inventory the F14-D stage built is unchanged.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(
        baseline.catalog.elements().count(),
        5,
        "three files, one program row, one mission row"
    );
    let report = baseline_report_json(&baseline);
    assert!(!report.contains("\"multiplayer_rules\":"), "{report}");
    assert!(
        report.contains(&format!("\"source\":\"{MODE_STRING_IMAGE}\"")),
        "{report}"
    );
}

/// An installation that ships no string image at all is a measured absence,
/// reported as one — never a row derived from a guess.
#[test]
fn accept_f14_d_2_a_missing_mode_table_is_named_not_invented() {
    let temp = TempInstall::new("absent");
    write_campaign(&temp);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let status = &baseline.collection_status[0];
    assert_eq!(status.kind, ContentKind::MultiplayerRules);
    assert_eq!(status.rows, 0);
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("an absent source is named, not dropped");
    assert!(diagnostic.contains(MODE_STRING_IMAGE), "{diagnostic}");
    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| element.kind != ContentKind::MultiplayerRules)
    );
}

// ------------------------------------------------------------- retail ----

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The retail half: the installation names four modes, each row located by the
/// span of the string block its name was read from, and the campaign
/// denominator is untouched.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_2_retail_the_installation_names_four_modes_with_checked_spans() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let discovery = cs_assets::install::discover(&dir).expect("production discovery reads");
    let install_sha = cs_assets::install::fingerprint(&discovery.manifest).to_hex();

    let names: Vec<String> = baseline
        .catalog
        .elements()
        .filter(|element| element.kind == ContentKind::MultiplayerRules)
        .map(|element| element.id.key().to_owned())
        .collect();
    assert_eq!(
        names,
        name_ids()
            .into_iter()
            .map(|id| format!("mode.name-{id}"))
            .collect::<Vec<_>>(),
        "the installation's mode-name run is one row per name, by string id"
    );

    // The image's inventory row and digest: production discovery measured both.
    let image = discovery
        .manifest
        .files
        .iter()
        .find(|record| {
            record
                .relative_spelling
                .as_str()
                .eq_ignore_ascii_case(MODE_STRING_IMAGE)
        })
        .expect("the installation inventories its string image");
    let image_id = cid(
        ContentKind::InstallFile,
        &install_file_key(image.relative_spelling.as_str()),
    );
    for name_id in name_ids() {
        let row = rules_row(&baseline, name_id);
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.install_sha256().to_hex(), install_sha);
        assert_eq!(span.container_path(), image.relative_spelling.as_str());
        assert!(
            span.offset() > 0 && span.length() > 0,
            "the mode's name is read from a string block inside the image"
        );
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.dependencies.len(), 1);
        assert_eq!(row.dependencies[0].target, image_id);
        assert_eq!(
            row.dependencies[0].provenance.class,
            ClaimStatus::ObservedTool
        );
        assert_eq!(
            row.fingerprint
                .as_ref()
                .map(|digest| digest.sha256.to_hex()),
            Some(image.sha256.to_hex()),
            "the row fingerprints the bytes it was read from"
        );
        assert!(!unknown_claims(row).is_empty(), "{name_id}");
        for unknown in unknown_claims(row) {
            // Every rule F56-A left unknown keeps that stage's claim id, so a
            // reader can join the row to the finding that recorded it.
            ClaimId::new(&unknown).expect("a valid claim id");
            assert!(unknown.starts_with("f56.mode.name-"), "{unknown}");
        }
    }

    let status = baseline
        .collection_status
        .iter()
        .find(|status| status.kind == ContentKind::MultiplayerRules)
        .expect("the collection reports its status");
    assert_eq!(status.rows, 4);
    assert_eq!(status.gaps.get("name_without_briefing"), Some(&0));
    assert_eq!(status.gaps.get("briefing_without_name"), Some(&0));
    // The measured end of the multiplayer briefing family (F56-A).
    assert_eq!(
        status.boundary_id,
        Some(BRIEFING_FIRST_ID + 4 * BRIEFING_STRIDE)
    );
    assert_eq!(status.diagnostic, None);

    // The denominator did not move: mode rules are not launchable content.
    assert_eq!(
        baseline.roots.len(),
        24,
        "the frozen F50 campaign denominator"
    );
    assert_eq!(baseline.coverage.roots, 24);
    assert_eq!(baseline.catalog.launchable_count(), 24);
    assert_eq!(
        baseline
            .coverage
            .unreachable_by_kind
            .get("multiplayer_rules")
            .copied(),
        Some(4),
        "nothing references a mode yet, so the rows stay unreachable unknowns"
    );
}

// -------------------------------------------------- synthetic PE image ---

/// The RVA every fixture image's `.rsrc` section sits at.
const RSRC_RVA: u32 = 0x2000;

/// Assembles a minimal single-section PE32 image whose `.rsrc` section is
/// `rsrc`.
///
/// Authored the way the F12-C string-catalog tests author theirs
/// (`crates/cs_content/src/config.rs`): the format's own field layout, no
/// original byte.
fn fixture_image(rsrc: &[u8]) -> Vec<u8> {
    let header_end = 0x80 + 4 + 20 + 224 + 40;
    let raw = (header_end + 0x1ff) & !0x1ff;
    let mut out = vec![0u8; header_end];
    out[0..2].copy_from_slice(b"MZ");
    out[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    out[0x80..0x84].copy_from_slice(b"PE\0\0");
    let coff = 0x84;
    out[coff..coff + 2].copy_from_slice(&0x014cu16.to_le_bytes());
    out[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
    out[coff + 16..coff + 18].copy_from_slice(&224u16.to_le_bytes());
    let optional = coff + 20;
    out[optional..optional + 2].copy_from_slice(&0x010bu16.to_le_bytes());
    out[optional + 92..optional + 96].copy_from_slice(&16u32.to_le_bytes());
    out[optional + 60..optional + 64].copy_from_slice(&(header_end as u32).to_le_bytes());
    out[optional + 112..optional + 116].copy_from_slice(&RSRC_RVA.to_le_bytes());
    out[optional + 116..optional + 120].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
    let section = optional + 224;
    out[section..section + 5].copy_from_slice(b".rsrc");
    out[section + 8..section + 12].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
    out[section + 12..section + 16].copy_from_slice(&RSRC_RVA.to_le_bytes());
    out[section + 16..section + 20].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
    out[section + 20..section + 24].copy_from_slice(&(raw as u32).to_le_bytes());
    out.resize(raw + rsrc.len(), 0);
    out[raw..raw + rsrc.len()].copy_from_slice(rsrc);
    out
}

fn rsrc_dir(bytes: &mut Vec<u8>, ids: usize) -> usize {
    let at = bytes.len();
    bytes.extend_from_slice(&[0u8; 16]);
    bytes.extend_from_slice(&vec![0u8; ids * 8]);
    bytes[at + 14..at + 16].copy_from_slice(&(ids as u16).to_le_bytes());
    at
}

fn rsrc_row(dir: usize, index: usize) -> usize {
    dir + 16 + index * 8
}

fn rsrc_id(bytes: &mut [u8], dir: usize, index: usize, id: u32) {
    let row = rsrc_row(dir, index);
    bytes[row..row + 4].copy_from_slice(&id.to_le_bytes());
}

fn rsrc_sub(bytes: &mut [u8], dir: usize, index: usize, child: usize) {
    let row = rsrc_row(dir, index);
    bytes[row + 4..row + 8].copy_from_slice(&(0x8000_0000u32 | child as u32).to_le_bytes());
}

fn rsrc_data(bytes: &mut [u8], dir: usize, index: usize, entry: usize) {
    let row = rsrc_row(dir, index);
    bytes[row + 4..row + 8].copy_from_slice(&(entry as u32).to_le_bytes());
}

/// Sixteen counted UTF-16LE units; the unused ones are empty strings, which is
/// what a real string block pads with.
fn string_payload(units: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for index in 0..STRING_UNITS_PER_BLOCK {
        let text = units.get(index).map(String::as_str).unwrap_or("");
        let encoded: Vec<u16> = text.encode_utf16().collect();
        out.extend_from_slice(&(encoded.len() as u16).to_le_bytes());
        for unit in encoded {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
    out
}

/// A PE image whose `RT_STRING` resources hold `rows`, addressed by the string
/// ids the production reader derives (`cs_formats::string_id`, one-based block
/// numbering) and read under [`MODE_STRING_LANGUAGE`].
fn string_image(rows: &[(u32, String)]) -> Vec<u8> {
    let mut blocks: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (id, text) in rows {
        let block = id / STRING_UNITS_PER_BLOCK as u32 + 1;
        let index = (*id % STRING_UNITS_PER_BLOCK as u32) as usize;
        let units = blocks
            .entry(block)
            .or_insert_with(|| vec![String::new(); STRING_UNITS_PER_BLOCK]);
        units[index] = text.clone();
    }
    let mut rsrc = Vec::new();
    let root = rsrc_dir(&mut rsrc, 1);
    rsrc_id(&mut rsrc, root, 0, RT_STRING);
    let strings = rsrc_dir(&mut rsrc, blocks.len());
    rsrc_sub(&mut rsrc, root, 0, strings);
    for (index, (block_id, units)) in blocks.into_iter().enumerate() {
        rsrc_id(&mut rsrc, strings, index, block_id);
        let languages = rsrc_dir(&mut rsrc, 1);
        rsrc_id(&mut rsrc, languages, 0, MODE_STRING_LANGUAGE);
        rsrc_sub(&mut rsrc, strings, index, languages);
        let payload = string_payload(&units);
        let rva = RSRC_RVA + rsrc.len() as u32;
        rsrc.extend_from_slice(&payload);
        let entry = rsrc.len();
        rsrc.extend_from_slice(&rva.to_le_bytes());
        rsrc.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        rsrc.extend_from_slice(&1252u32.to_le_bytes());
        rsrc.extend_from_slice(&0u32.to_le_bytes());
        rsrc_data(&mut rsrc, languages, 0, entry);
    }
    fixture_image(&rsrc)
}
