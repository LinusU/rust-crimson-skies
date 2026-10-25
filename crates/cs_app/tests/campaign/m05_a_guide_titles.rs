//! Owner ruling for the guide titles the installation spells differently
//! (Rally #470, `docs/findings/2026-10-25-owner-ruling-guide-titles.md`).
//!
//! The ruling joins seven work orders to their retail rows through
//! `missions/bindings/title-rulings.tsv`; the confirmation stays exact. The
//! retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the table tests run
//! in CI.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    CriticalDependency, DependencyState, RULED_TITLE_NOT_CARRIED_REFUSAL, SourceContext,
    TitleConfirmation, TitleRulings,
};
use cs_types::evidence::ClaimStatus;

use crate::campaign::common::{label, load_inventory, repo_path};

const RULED: [&str; 7] = ["M09", "M11", "M14", "M15", "M20", "M22", "M23"];

fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        let dir = PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
            panic!("CS_GAME_DIR is not set: run with `--include-ignored` and the installation")
        }));
        SourceContext::read(&dir).expect("the installation yields a source context")
    })
}

fn declared(work_order: &str) -> String {
    load_inventory()
        .iter()
        .find(|(l, _)| l.as_str() == work_order)
        .map(|(_, title)| title.clone())
        .expect("declared work order")
}

fn unresolved(binding: &cs_content::campaign_bindings::SourceBinding) -> usize {
    binding
        .dependencies
        .iter()
        .filter(|dependency| matches!(dependency.state, DependencyState::Unresolved { .. }))
        .count()
}

#[test]
fn accept_m05_a_guide_titles_table_covers_exactly_the_seven_and_matches_the_inventory() {
    let text = std::fs::read_to_string(repo_path("missions/bindings/title-rulings.tsv"))
        .expect("the ruling table is committed");
    let rulings = TitleRulings::parse(&text).expect("the table parses");
    let labels: Vec<&str> = rulings.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, RULED);
    for ruling in rulings.iter() {
        assert_eq!(ruling.declared_title, declared(ruling.label.as_str()));
        assert_ne!(ruling.declared_title, ruling.retail_short_name);
    }
    assert_eq!(TitleRulings::committed(), rulings);
}

#[test]
fn accept_m05_a_guide_titles_malformed_table_is_refused() {
    assert!(TitleRulings::parse("M09\tonly\tthree").is_err());
    assert!(TitleRulings::parse("M09\ta\tb\tx\t1").is_err());
    assert!(TitleRulings::parse("M09\ta\tb\t1\t2\nM09\ta\tb\t1\t2").is_err());
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_guide_titles_the_seven_bind_with_no_unresolved_critical_dependency() {
    for work_order in RULED {
        let binding = context()
            .bind(label(work_order), &declared(work_order))
            .expect("binds");
        assert_eq!(unresolved(&binding), 0, "{work_order}");
        assert!(binding.campaign_position.is_some(), "{work_order}");
        let title = binding
            .dependencies
            .iter()
            .find(|d| d.id == CriticalDependency::TitleString)
            .expect("title dependency");
        let DependencyState::Resolved { provenance } = &title.state else {
            panic!("{work_order} title unresolved");
        };
        assert_eq!(provenance.class, ClaimStatus::Inferred, "{work_order}");
        assert!(
            binding
                .unknowns
                .iter()
                .any(|u| u.starts_with("title ruling:")),
            "{work_order}"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_guide_titles_the_carried_orders_bind_exactly_as_before() {
    let without = context().clone().with_title_rulings(TitleRulings::none());
    for (work_order, title) in load_inventory().iter() {
        if RULED.contains(&work_order.as_str()) {
            continue;
        }
        let with = context().bind(work_order.clone(), title).expect("binds");
        let before = without.bind(work_order.clone(), title).expect("binds");
        assert_eq!(unresolved(&with), 0, "{work_order}");
        assert_eq!(with, before, "{work_order}");
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_guide_titles_a_wrong_spelling_refuses() {
    let text =
        std::fs::read_to_string(repo_path("missions/bindings/title-rulings.tsv")).expect("table");
    let wrong = text.replace("Peril for Blake", "Peril for Blak");
    let wrong_row = text.replace(
        "M09\tPerils for Blake\tPeril for Blake\t3488",
        "M09\tPerils for Blake\tPeril for Blake\t3489",
    );
    for table in [wrong, wrong_row] {
        let context = context()
            .clone()
            .with_title_rulings(TitleRulings::parse(&table).expect("parses"));
        let binding = context.bind(label("M09"), &declared("M09")).expect("binds");
        assert!(unresolved(&binding) > 0);
        assert!(binding.campaign_position.is_none());
        let reasons: Vec<_> = binding
            .dependencies
            .iter()
            .filter_map(|d| match &d.state {
                DependencyState::Unresolved { reason, .. } => Some(reason.as_str()),
                _ => None,
            })
            .collect();
        assert!(reasons.contains(&RULED_TITLE_NOT_CARRIED_REFUSAL));
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_guide_titles_without_the_table_the_seven_are_uncarried() {
    let without = context().clone().with_title_rulings(TitleRulings::none());
    for work_order in RULED {
        let title = declared(work_order);
        assert_eq!(without.confirm_title(&title), TitleConfirmation::Uncarried);
        let binding = without.bind(label(work_order), &title).expect("binds");
        assert!(unresolved(&binding) > 0, "{work_order}");
    }
}
