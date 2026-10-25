//! Acceptance stage M09-A: bind the ninth mission's original data and
//! branches (`missions/M09.md`, work order `M09-A`).
//!
//! The stage's minimum scenario is "Source-derived binding has no unresolved
//! critical dependencies". M09 is the first stage whose declared title the
//! installation does not carry in either display form: `Perils for Blake` is
//! joined to the retail mission whose rows read `Peril for Blake` and
//! `Northwest - Peril for Paladin Blake` only by the owner ruling of Rally #470
//! (`docs/findings/2026-10-25-owner-ruling-guide-titles.md`). So this suite
//! pins, besides the usual five resolved dependencies, that the title
//! dependency is recorded as `Inferred` (never observed), that the record says
//! so in its unknowns, and that without the ruling table the binding refuses.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which
//! has no original data) skips them; the synthetic test runs everywhere.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    BindingCategory, CampaignBindings, CellState, CriticalDependency, DependencyState,
    MissionLabel, NO_CONFIRMED_ROW_REFUSAL, SourceBinding, SourceContext, TitleConfirmation,
    TitleRulings,
};
use cs_types::content::ContentKind;
use cs_types::evidence::ClaimStatus;

use crate::campaign::common::{load_inventory, repo_path};

/// The one work order this stage binds.
const WORK_ORDER: &str = "M09";

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M09-A needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

fn label() -> MissionLabel {
    MissionLabel::new(WORK_ORDER).expect("M09 is a valid label")
}

/// The declared discovery title, read from the committed inventory.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == WORK_ORDER)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {WORK_ORDER} work order"))
}

fn binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        context()
            .bind(label(), &discovery_title())
            .expect("M09 binds to the original data")
    })
}

/// The comparable text of one retail row: a leading display tag such as
/// `[AB14I]` is a presentation instruction, not part of the title.
fn display_text(text: &str) -> &str {
    match text.strip_prefix('[').and_then(|rest| rest.find(']')) {
        Some(end) => &text[end + 2..],
        None => text,
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_a_source_derived_binding_has_no_unresolved_critical_dependencies() {
    let binding = binding();
    binding
        .validate()
        .expect("the derived record is internally consistent");
    assert_eq!(
        binding.unresolved_critical(),
        Vec::new(),
        "the source-derived binding left a critical dependency unresolved"
    );
    assert_eq!(
        binding
            .dependencies
            .iter()
            .map(|dependency| dependency.id)
            .collect::<Vec<_>>(),
        CriticalDependency::ALL.to_vec()
    );

    // The title is the one dependency the installation does not observe for
    // M09: it rests on the owner ruling, so it is Inferred.
    for dependency in &binding.dependencies {
        let DependencyState::Resolved { provenance } = &dependency.state else {
            panic!("{} is not resolved", dependency.id);
        };
        let expected = match dependency.id {
            CriticalDependency::MissionId
            | CriticalDependency::WorldGroupVariant
            | CriticalDependency::TitleString => ClaimStatus::Inferred,
            CriticalDependency::InstallHash | CriticalDependency::ProgramSourceMap => {
                ClaimStatus::ObservedTool
            }
        };
        assert_eq!(
            provenance.class, expected,
            "{} carries the wrong evidence class",
            dependency.id
        );
    }

    let found = cs_assets::install::discover(&game_dir()).expect("discovery reads the install");
    assert_eq!(
        binding.install_sha256,
        cs_assets::install::fingerprint(&found.manifest).to_hex()
    );

    assert_eq!(
        binding.catalog_id.as_ref().expect("mission id").kind(),
        ContentKind::Mission
    );
    assert_eq!(
        binding.world_id.as_ref().expect("world id").kind(),
        ContentKind::World
    );
    assert_eq!(
        binding.program_id.as_ref().expect("program id").kind(),
        ContentKind::Script
    );

    // M09 is the ninth mission: position 8, chapter 2's fourth mission.
    let campaign = context().campaign();
    let position = binding.campaign_position.expect("a position was resolved");
    assert_eq!(position, 8, "M09 is the ninth campaign position");
    assert_eq!(binding.campaign_size, campaign.len());
    let entry = &campaign[position];
    assert_eq!((entry.chapter, entry.mission_number), (2, 4));
    assert!(
        game_dir().join(&entry.program_asset).is_file(),
        "the program archive {} the record cites does not exist",
        entry.program_asset
    );

    // Every cited span points inside an existing asset and carries the digest
    // of those bytes, recomputed here.
    assert!(!binding.source_spans.is_empty());
    for span in &binding.source_spans {
        let bytes = fs::read(game_dir().join(&span.asset_id))
            .unwrap_or_else(|error| panic!("cannot re-read {}: {error}", span.asset_id));
        assert_eq!(span.sha256, cs_assets::install::sha256(&bytes).to_hex());
        assert!(span.length > 0 && span.offset + span.length <= bytes.len() as u64);
    }
    assert!(
        binding
            .source_spans
            .iter()
            .any(|span| span.asset_id == entry.program_asset)
    );

    // Source-derived is not verified.
    assert!(!binding.is_verified());
    for needle in [
        "objective graph",
        "difficulty branches",
        "script/native coverage",
        "closure_sha256",
    ] {
        assert!(
            binding.unknowns.iter().any(|entry| entry.contains(needle)),
            "the record no longer says that {needle:?} is unknown"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_a_the_committed_record_is_what_the_installation_derives() {
    let derived = binding().to_json();
    let path = repo_path("missions/bindings/M09.json");
    if std::env::var_os("CS_REGENERATE_M09_BINDING").is_some() {
        fs::write(&path, &derived).expect("write the regenerated record");
    }
    let committed = fs::read_to_string(&path).expect("missions/bindings/M09.json exists");
    assert_eq!(
        committed, derived,
        "the committed binding record is not what production code derives from $CS_GAME_DIR"
    );
    for key in [
        "\"work_order\": \"M09\"",
        "\"verified\": false",
        "\"catalog_id\": \"mission/",
        "\"world_id\": \"world/",
        "\"program_id\": \"script/",
        "\"closure_sha256\": null",
        "\"unknowns\": [",
        "title ruling:",
    ] {
        assert!(derived.contains(key), "the record is missing {key:?}");
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_a_the_declared_title_is_joined_by_the_ruling_and_by_nothing_else() {
    let context = context();
    let title = discovery_title();
    assert_eq!(title, "Perils for Blake");

    // The declared spelling is carried by no row; the ruled one exactly once.
    assert!(
        context
            .string_rows()
            .iter()
            .filter_map(|row| row.text.as_deref())
            .all(|text| !text.contains("Perils")),
        "a retail row carries the declared spelling, so the ruling is no longer needed"
    );
    let ruling = TitleRulings::committed();
    let ruling = ruling
        .iter()
        .find(|ruling| ruling.label.as_str() == WORK_ORDER)
        .expect("M09 has a ruling row");
    assert_eq!(ruling.retail_short_name, "Peril for Blake");
    let row = context
        .string_rows()
        .iter()
        .find(|row| row.id == ruling.short_row)
        .expect("the ruled short-name row exists");
    assert_eq!(
        row.text.as_deref().map(display_text),
        Some("Peril for Blake")
    );
    assert_eq!(binding().localized_title_id, Some(ruling.short_row));

    // Failure case: with the ruling table withdrawn the title is uncarried and
    // the binding refuses instead of matching by proximity or ordinal.
    let without = context.clone().with_title_rulings(TitleRulings::none());
    assert_eq!(without.confirm_title(&title), TitleConfirmation::Uncarried);
    let partial = without.bind(label(), &title).expect("a miss is recorded");
    partial
        .validate()
        .expect("the partial record is consistent");
    let unresolved = partial.unresolved_critical();
    for id in [
        CriticalDependency::TitleString,
        CriticalDependency::MissionId,
        CriticalDependency::WorldGroupVariant,
        CriticalDependency::ProgramSourceMap,
    ] {
        assert!(unresolved.contains(&id), "{id} resolved without the ruling");
    }
    assert!(!unresolved.contains(&CriticalDependency::InstallHash));
    assert_eq!(partial.catalog_id, None);
    assert!(partial.source_spans.is_empty());
    assert!(partial.dependencies.iter().any(|d| matches!(
        &d.state,
        DependencyState::Unresolved { reason, .. } if reason == NO_CONFIRMED_ROW_REFUSAL
    )));

    // A near-miss spelling of the ruled row is not confirmed either.
    let near = context
        .bind(label(), "Peril for Blak")
        .expect("a miss is recorded");
    assert!(
        near.unresolved_critical()
            .contains(&CriticalDependency::TitleString)
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_a_the_campaign_keeps_everything_else_unresolved_and_unready() {
    let mut campaign = CampaignBindings::from_inventory(&load_inventory())
        .expect("the declared inventory builds the campaign");
    campaign
        .bind(binding().to_mission_binding().expect("record is valid"))
        .expect("M09 is a declared mission");
    let coverage = campaign.coverage();
    assert_eq!(coverage.total_missions, 24);
    assert_eq!(coverage.cells, 24 * 7);
    assert_eq!(coverage.complete_cells, 1, "only M09's identity cell");
    assert_eq!(coverage.unknown_cells, 24 * 7 - 1);
    assert_eq!(coverage.missing_cells, 0);
    assert_eq!(coverage.subsystem_unresolved, 24 * 23);
    assert!(!coverage.is_ready());

    let m09 = campaign.get(&label()).expect("M09 is recorded");
    assert!(!m09.is_placeholder());
    for (category, state) in m09.cells() {
        let expected = if category == BindingCategory::MissionIdentity {
            CellState::Complete
        } else {
            CellState::Unknown
        };
        assert_eq!(state, expected, "category {category} has the wrong state");
    }
    assert!(!binding().is_verified());
}

#[test]
fn accept_m09_a_the_ruling_table_is_the_only_join_for_m09() {
    let text = fs::read_to_string(repo_path("missions/bindings/title-rulings.tsv"))
        .expect("the ruling table is committed");
    let rulings = TitleRulings::parse(&text).expect("the table parses");
    let ruling = rulings
        .iter()
        .find(|ruling| ruling.label.as_str() == WORK_ORDER)
        .expect("M09 has a ruling row");
    assert_eq!(ruling.declared_title, "Perils for Blake");
    assert_eq!(ruling.retail_short_name, "Peril for Blake");
    assert_eq!((ruling.short_row, ruling.long_row), (3488, 3458));

    // Failure case: a table without M09's row has no ruling for it.
    let stripped: String = text
        .lines()
        .filter(|line| !line.starts_with("M09\t"))
        .map(|line| format!("{line}\n"))
        .collect();
    let stripped = TitleRulings::parse(&stripped).expect("the stripped table parses");
    assert!(
        stripped
            .iter()
            .all(|ruling| ruling.label.as_str() != WORK_ORDER)
    );
}
