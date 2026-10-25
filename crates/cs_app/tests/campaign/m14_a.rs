//! Acceptance stage M14-A: bind the fourteenth mission's original data and
//! branches (`missions/M14.md`, work order `M14-A`).
//!
//! The stage's minimum scenario is "Source-derived binding has no unresolved
//! critical dependencies". M14's declared title, `Clash of the Dreadnaughts`,
//! is carried by no retail row in either display form, so the join is the
//! owner's per-work-order ruling (Rally #470, note 22711,
//! `docs/findings/2026-10-25-owner-ruling-guide-titles.md`) in
//! `missions/bindings/title-rulings.tsv`: the ruled retail short name
//! `Clash of Dreadnaughts` is confirmed *exactly* at row 3493, and the long
//! name at row 3463 must sit at the same campaign position. The TitleString
//! dependency is therefore `Inferred`, never `ObservedTool`, and the record
//! carries a `title ruling:` unknown naming the ruling.
//!
//! What this stage pins, beyond the shared join machinery (M02-A's):
//!
//! * the record resolves to the campaign position the declared order selects
//!   (M14 is the fourteenth inventory entry) and to the identities the retail
//!   layout builds for that position — the order is corroboration, never the
//!   derivation;
//! * without the ruling table the same installation leaves M14 unresolved, and
//!   a ruling whose spelling or row is wrong refuses: nothing is fuzzy;
//! * the campaign stays unready.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI skips
//! them; the implementing and reviewing agents run them with
//! `--include-ignored`. The synthetic test is not ignored.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    BindingCategory, CampaignBindings, CellState, CriticalDependency, DependencyState,
    MissionLabel, RULED_TITLE_NOT_CARRIED_REFUSAL, SourceBinding, SourceContext, TitleRulings,
};
use cs_types::content::ContentKind;
use cs_types::evidence::ClaimStatus;

use crate::campaign::common::{label, load_inventory, repo_path};

/// The one work order this stage binds.
const WORK_ORDER: &str = "M14";

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M14-A needs the retail capability; run this suite with \
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

/// The declared discovery title of `M14`, read from the committed inventory.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == WORK_ORDER)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {WORK_ORDER} work order"))
}

fn bind_with(context: &SourceContext) -> SourceBinding {
    context
        .bind(
            MissionLabel::new(WORK_ORDER).expect("M14 is a valid label"),
            &discovery_title(),
        )
        .expect("M14 binds")
}

fn binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| bind_with(context()))
}

fn campaign() -> CampaignBindings {
    let mut campaign = CampaignBindings::from_inventory(&load_inventory())
        .expect("the declared inventory builds the campaign");
    campaign
        .bind(binding().to_mission_binding().expect("record is valid"))
        .expect("M14 is a declared mission");
    campaign
}

fn unresolved_reasons(binding: &SourceBinding) -> Vec<String> {
    binding
        .dependencies
        .iter()
        .filter_map(|d| match &d.state {
            DependencyState::Unresolved { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

/// The ruling row for M14, parsed from the committed table.
fn ruling_text() -> String {
    fs::read_to_string(repo_path("missions/bindings/title-rulings.tsv"))
        .expect("the ruling table is committed")
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_a_source_derived_binding_has_no_unresolved_critical_dependencies() {
    let binding = binding();
    binding
        .validate()
        .expect("the derived record is consistent");
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
    // The title join is the owner's ruling: Inferred, like the position joins.
    for dependency in &binding.dependencies {
        let DependencyState::Resolved { provenance } = &dependency.state else {
            panic!("{} is not resolved", dependency.id);
        };
        let expected = match dependency.id {
            CriticalDependency::TitleString
            | CriticalDependency::MissionId
            | CriticalDependency::WorldGroupVariant => ClaimStatus::Inferred,
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

    let mission = binding.catalog_id.as_ref().expect("mission id resolved");
    let world = binding.world_id.as_ref().expect("world id resolved");
    let program = binding.program_id.as_ref().expect("program id resolved");
    assert_eq!(mission.kind(), ContentKind::Mission);
    assert_eq!(world.kind(), ContentKind::World);
    assert_eq!(program.kind(), ContentKind::Script);

    // The identities name the layout entry the position selected.
    let layout = context().campaign();
    let position = binding.campaign_position.expect("a position was resolved");
    let entry = &layout[position];
    assert_eq!(
        mission.as_str(),
        format!("mission/ch{}-m{:02}", entry.chapter, entry.mission_number)
    );
    assert_eq!(world.as_str(), format!("world/{}", entry.world_group));
    assert_eq!(
        program.as_str(),
        format!(
            "script/{}-m{:02}-zrdr",
            entry.world_group, entry.mission_number
        )
    );
    assert!(game_dir().join(&entry.program_asset).is_file());

    // The ruling is corroborated, not derived, by the declared order: M14 is
    // the fourteenth inventory entry and the join selected that position.
    let index = load_inventory()
        .iter()
        .position(|(label, _)| label.as_str() == WORK_ORDER)
        .expect("M14 is declared");
    assert_eq!(
        position, index,
        "the ruled position disagrees with the declared order"
    );
    assert_eq!(binding.campaign_size, layout.len());

    // Every cited span points at real bytes whose digest is recomputed here.
    assert!(!binding.source_spans.is_empty());
    for span in &binding.source_spans {
        let bytes = fs::read(game_dir().join(&span.asset_id)).expect("span asset re-read");
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
        "title ruling:",
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
fn accept_m14_a_the_committed_record_is_what_the_installation_derives() {
    let committed = fs::read_to_string(repo_path("missions/bindings/M14.json"))
        .expect("missions/bindings/M14.json exists");
    assert_eq!(
        committed,
        binding().to_json(),
        "the committed binding record is not what production code derives from $CS_GAME_DIR"
    );
    for key in [
        "\"work_order\": \"M14\"",
        "\"verified\": false",
        "\"catalog_id\": \"mission/",
        "\"world_id\": \"world/",
        "\"program_id\": \"script/",
        "\"closure_sha256\": null",
        "\"evidence_ids\": []",
    ] {
        assert!(committed.contains(key), "the record is missing {key:?}");
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_a_the_ruled_spelling_is_carried_exactly_and_the_declared_one_is_not() {
    // Re-read the rows the ruling names straight from the retail table.
    let rows = context().string_rows();
    let text = |id: u32| {
        rows.iter()
            .find(|row| row.id == id)
            .and_then(|row| row.text.as_deref())
            .unwrap_or_else(|| panic!("string id {id} is not in the retail table"))
            .to_owned()
    };
    assert!(
        text(3493).ends_with("Clash of Dreadnaughts"),
        "row 3493 is {:?}",
        text(3493)
    );
    assert!(
        text(3463).ends_with("Nathan Zachary & The Clash of Dreadnaughts"),
        "row 3463 is {:?}",
        text(3463)
    );
    // The declared spelling is in no row: the ruling, not the title, is the join.
    let declared = discovery_title();
    assert!(
        rows.iter()
            .all(|row| row.text.as_deref().is_none_or(|t| !t.contains(&declared))),
        "a retail row carries the declared title, so no ruling would be needed"
    );
    // The committed ruling row names exactly those rows.
    let rulings = TitleRulings::committed();
    let ruling = rulings.for_label(&label(WORK_ORDER)).expect("M14 is ruled");
    assert_eq!(ruling.declared_title, declared);
    assert_eq!(ruling.retail_short_name, "Clash of Dreadnaughts");
    assert!(binding().campaign_position.is_some());
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_a_without_the_ruling_m14_stays_unresolved() {
    let without = context().clone().with_title_rulings(TitleRulings::none());
    let binding = bind_with(&without);
    assert!(!binding.unresolved_critical().is_empty());
    assert!(binding.campaign_position.is_none());
    assert!(binding.catalog_id.is_none() && binding.world_id.is_none());
    assert!(binding.program_id.is_none());
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_a_a_ruling_with_the_wrong_spelling_or_row_refuses() {
    let text = ruling_text();
    let wrong_spelling = text.replace("Clash of Dreadnaughts", "Clash of Dreadnaoughts");
    let wrong_row = text.replace("Dreadnaughts\t3493", "Dreadnaughts\t3492");
    let wrong_long = text.replace("3493\t3463", "3493\t3462");
    for table in [wrong_spelling, wrong_row, wrong_long] {
        assert_ne!(table, text, "the mutation changed nothing");
        let context = context()
            .clone()
            .with_title_rulings(TitleRulings::parse(&table).expect("the mutated table parses"));
        let binding = bind_with(&context);
        assert!(binding.campaign_position.is_none());
        assert!(binding.catalog_id.is_none());
        assert!(
            unresolved_reasons(&binding)
                .iter()
                .any(|reason| reason == RULED_TITLE_NOT_CARRIED_REFUSAL),
            "a ruling that does not hold must refuse with the ruled-title refusal"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_a_the_campaign_keeps_everything_else_unresolved_and_unready() {
    let campaign = campaign();
    let coverage = campaign.coverage();
    assert_eq!(coverage.total_missions, 24);
    assert_eq!(
        coverage.complete_cells, 1,
        "only the identity cell is complete"
    );
    assert_eq!(coverage.unknown_cells, 24 * 7 - 1);
    assert_eq!(coverage.missing_cells, 0);
    assert_eq!(coverage.subsystem_unresolved, 24 * 23);
    assert!(!coverage.is_ready());

    let m14 = campaign.get(&label(WORK_ORDER)).expect("M14 is recorded");
    assert!(!m14.is_placeholder());
    for (category, state) in m14.cells() {
        let expected = if category == BindingCategory::MissionIdentity {
            CellState::Complete
        } else {
            CellState::Unknown
        };
        assert_eq!(state, expected, "category {category} has the wrong state");
    }
    assert_eq!(
        m14.catalog_identity().expect("identity bound").as_str(),
        binding().catalog_id.as_ref().expect("catalog id").as_str()
    );
}

#[test]
fn accept_m14_a_the_committed_ruling_names_m14_with_rows_the_record_cites() {
    // Runs in CI: the committed table and the inventory agree about M14, and
    // the committed record (if present) is a resolved one.
    let rulings = TitleRulings::committed();
    let ruling = rulings.for_label(&label(WORK_ORDER)).expect("M14 is ruled");
    assert_eq!(ruling.declared_title, discovery_title());
    assert_ne!(ruling.retail_short_name, ruling.declared_title);
    let record = fs::read_to_string(repo_path("missions/bindings/M14.json"))
        .expect("missions/bindings/M14.json is committed");
    assert!(record.contains("\"work_order\": \"M14\""));
    assert!(record.contains("\"catalog_id\": \"mission/"));
    assert!(!record.contains("\"catalog_id\": null"));
    assert!(record.contains("title ruling:"));
}
