//! Acceptance stage M11-A: bind the eleventh mission's original data and
//! branches (`missions/M11.md`, work order `M11-A`).
//!
//! The stage's minimum scenario is "Source-derived binding has no unresolved
//! critical dependencies". M11's declared title `The Stolen Scarlet` is
//! carried by no localized row of the installation — the original spells the
//! mission `The Stolen Starlet` at short-name row 3490 and
//! `Hollywood - Nathan Zachary & The Stolen Starlet` at long-name row 3460 —
//! so the join rests on the owner ruling of Rally #470
//! (`docs/findings/2026-10-25-owner-ruling-guide-titles.md`), applied through
//! the committed `missions/bindings/title-rulings.tsv` as an inferred claim.
//! It is never `verified_original`.
//!
//! This suite therefore both measures the discrepancy and pins the join:
//!
//! * [`accept_m11_a_the_declared_title_is_joined_by_the_ruling_and_by_nothing_else`]
//!   asserts no row carries the declared spelling, that the ruled spelling is
//!   carried exactly at the ruled rows, that withdrawing the ruling table
//!   leaves title, mission, world and program unresolved, and that a near-miss
//!   spelling is still refused.
//! * [`accept_m11_a_the_declared_order_agrees_with_the_retail_campaign_order_where_it_is_carried`]
//!   re-measures the evidence the ruling rests on: over all 24 declared work
//!   orders every title the installation carries exactly selects its own index
//!   of the declared inventory, and exactly the seven the ruling names are
//!   carried by no row. Under the ruling all 24 bind.
//! * [`accept_m11_a_the_position_the_declared_order_would_select_is_read_from_the_installation`]
//!   reads what the original data says at that offset — the two rows, the
//!   exact word-level difference, the chapter and region-group boundary, the
//!   program archive — and pins that the record cites exactly those ruled rows
//!   and that position, not the declared ordinal.
//! * [`accept_m11_a_source_derived_binding_has_no_unresolved_critical_dependencies`]
//!   pins the resolved dependencies, their evidence classes and every cited
//!   span.
//! * [`accept_m11_a_the_committed_record_is_what_the_installation_derives`]
//!   pins `missions/bindings/M11.json` to what production code derives.
//! * [`accept_m11_a_the_campaign_keeps_everything_else_unresolved_and_unready`]
//!   pins the campaign totals: only M11's identity cell is complete.
//!
//! Nothing here approximates the title: `TitleForm` stays exact and the three
//! synthetic tests below prove the exactness rules the join rests on, the
//! record's self-explaining `unknowns` rule, and the ruling row itself.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which has
//! no original data) skips them; they are run with `--include-ignored` by the
//! implementing and reviewing agents.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    BindingCategory, CampaignBindings, CellState, CriticalDependency, DependencyState,
    MissionLabel, NO_CONFIRMED_ROW_REFUSAL, SourceBinding, SourceContext, SourceDependency,
    TitleConfirmation, TitleForm, TitleRulings, UNCARRIED_TITLE_REFUSAL, title_form,
    unresolved_critical_entries,
};
use cs_types::content::{ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};

use crate::campaign::common::{label, load_inventory, repo_path};

/// The one work order this stage binds.
const WORK_ORDER: &str = "M11";

/// The owner-ruled retail spelling this work order names.
const RULED_SHORT_NAME: &str = "The Stolen Starlet";

/// Its bare short-name row and region-prefixed long-name row.
const RULED_SHORT_ROW: u32 = 3490;
const RULED_LONG_ROW: u32 = 3460;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M11-A needs the retail capability; run this suite with \
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

fn label_of_work_order() -> MissionLabel {
    MissionLabel::new(WORK_ORDER).expect("M11 is a valid label")
}

/// The declared discovery title, read from the committed inventory.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(declared, _)| declared.as_str() == WORK_ORDER)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {WORK_ORDER} work order"))
}

/// [`WORK_ORDER`]'s index in the declared inventory.
///
/// A property of the committed work-order list, not of the installation.
fn declared_index() -> usize {
    load_inventory()
        .iter()
        .position(|(declared, _)| declared.as_str() == WORK_ORDER)
        .expect("the declared inventory has the work order")
}

fn binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        context()
            .bind(label_of_work_order(), &discovery_title())
            .expect("M11 binds to the original data")
    })
}

/// The comparable text of one retail row: a leading display tag such as
/// `[AB14I]` is a presentation instruction, not part of the title, and
/// production confirmation strips it before comparing.
fn display_text(text: &str) -> &str {
    match text.strip_prefix('[').and_then(|rest| rest.find(']')) {
        Some(end) => &text[end + 2..],
        None => text,
    }
}

/// The display text of one localized row, read from the installation.
fn display_of(context: &SourceContext, id: u32) -> String {
    let row = context
        .string_rows()
        .iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("the localized table carries no row {id}"));
    display_text(
        row.text
            .as_deref()
            .unwrap_or_else(|| panic!("row {id} does not decode")),
    )
    .to_owned()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_source_derived_binding_has_no_unresolved_critical_dependencies() {
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
    // M11: it rests on the owner ruling, so the title — and the identity read
    // through the position it selects — is Inferred, never observed.
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

    // M11 is the eleventh declared work order: position 10, and chapter 3's
    // first mission.
    let campaign = context().campaign();
    let position = binding.campaign_position.expect("a position was resolved");
    assert_eq!(position, declared_index(), "M11 is the eleventh position");
    assert_eq!(binding.campaign_size, campaign.len());
    let entry = &campaign[position];
    assert_eq!((entry.chapter, entry.mission_number), (3, 1));
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
fn accept_m11_a_the_committed_record_is_what_the_installation_derives() {
    let derived = binding().to_json();
    let path = repo_path("missions/bindings/M11.json");
    if std::env::var_os("CS_REGENERATE_M11_BINDING").is_some() {
        fs::write(&path, &derived).expect("write the regenerated record");
    }
    let committed = fs::read_to_string(&path).expect("missions/bindings/M11.json exists");
    assert_eq!(
        committed, derived,
        "the committed binding record is not what production code derives from $CS_GAME_DIR"
    );
    for key in [
        "\"work_order\": \"M11\"",
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
fn accept_m11_a_the_declared_title_is_joined_by_the_ruling_and_by_nothing_else() {
    let context = context();
    let title = discovery_title();
    assert_eq!(title, "The Stolen Scarlet");

    // The declared spelling is carried by no localized row, in either display
    // form, while the ruled spelling is carried exactly once. The scan uses
    // production `title_form`, so it sees what confirmation sees.
    let displays: Vec<(u32, String)> = context
        .string_rows()
        .iter()
        .filter_map(|row| row.text.as_deref().map(|text| (row.id, display_text(text))))
        .map(|(id, text)| (id, text.to_owned()))
        .collect();
    assert!(!displays.is_empty(), "the localized table is empty");
    for (id, display) in &displays {
        assert_eq!(
            title_form(display, &title),
            None,
            "row {id} carries the declared title as {display:?}, so the ruling is no longer needed"
        );
    }
    // The scan is not vacuous: the same rule finds declared titles the
    // installation does carry.
    let carried = load_inventory()
        .iter()
        .filter(|(_, declared)| {
            displays
                .iter()
                .any(|(_, display)| title_form(display, declared).is_some())
        })
        .count();
    assert!(
        carried >= load_inventory().len() / 2,
        "only {carried} of {} declared titles are carried, so the scan above proves nothing",
        load_inventory().len()
    );

    let ruling = TitleRulings::committed();
    let ruling = ruling
        .iter()
        .find(|ruling| ruling.label.as_str() == WORK_ORDER)
        .expect("M11 has a ruling row")
        .clone();
    assert_eq!(ruling.declared_title, title);
    assert_eq!(ruling.retail_short_name, RULED_SHORT_NAME);
    assert_eq!(
        (ruling.short_row, ruling.long_row),
        (RULED_SHORT_ROW, RULED_LONG_ROW)
    );
    assert_eq!(
        display_of(context, ruling.short_row),
        RULED_SHORT_NAME,
        "the ruled short-name row does not read as the ruling says"
    );
    assert_eq!(binding().localized_title_id, Some(ruling.short_row));

    // Failure case: with the ruling table withdrawn the title is uncarried and
    // the binding refuses instead of matching by proximity or ordinal.
    let without = context.clone().with_title_rulings(TitleRulings::none());
    assert_eq!(without.confirm_title(&title), TitleConfirmation::Uncarried);
    let partial = without
        .bind(label_of_work_order(), &title)
        .expect("a miss is recorded");
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
    assert!(
        matches!(
            partial.dependency(CriticalDependency::TitleString),
            Some(DependencyState::Unresolved { reason, .. }) if reason == UNCARRIED_TITLE_REFUSAL
        ),
        "the withdrawn ruling must refuse the title as uncarried: {:?}",
        partial.dependency(CriticalDependency::TitleString)
    );

    // A near-miss spelling of the ruled row is not confirmed either: the join
    // is exact, so one character of one word is enough to lose it.
    let near = context
        .bind(label_of_work_order(), "The Stolen Starle")
        .expect("a miss is recorded");
    assert!(
        near.unresolved_critical()
            .contains(&CriticalDependency::TitleString)
    );
    assert_eq!(near.catalog_id, None);
    assert_eq!(
        near.dependency(CriticalDependency::ProgramSourceMap)
            .map(|state| matches!(state, DependencyState::Unresolved { .. })),
        Some(true)
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_the_declared_order_agrees_with_the_retail_campaign_order_where_it_is_carried() {
    // The fact the owner ruling (Rally #470) rests on, re-measured here because
    // nothing else measures it: for all 24 declared work orders, every title
    // the installation carries *exactly* selects its own index of the declared
    // inventory, and exactly the ruled seven are carried by no row at all.
    let context = context();
    let agreement = context.join_agreement();
    assert_eq!(
        agreement.state,
        cs_content::campaign_bindings::JoinCorroboration::Agreed,
        "the localized table does not corroborate the layout, so no position can be measured"
    );

    let mut carried = Vec::new();
    let mut uncarried = Vec::new();
    for (index, (declared, title)) in load_inventory().iter().enumerate() {
        match context.confirm_title(title).confirmed() {
            None => uncarried.push(declared.as_str().to_owned()),
            Some((row, _)) => {
                let position =
                    cs_content::campaign_bindings::campaign_position_for(Some(row), &agreement)
                        .expect("a confirmed row selects a position in an agreeing table");
                carried.push((index, declared.as_str().to_owned(), position));
            }
        }
    }

    // Every carried work order agrees with the declared order: no reordering,
    // no gap, no tie. One disagreement would mean the declared list is not the
    // retail campaign order and this measurement would say nothing about M11.
    for (index, declared, position) in &carried {
        assert_eq!(
            index, position,
            "{declared} selects campaign position {position} but is declared at {index}: the \
             declared order and the retail campaign order disagree here"
        );
    }
    assert!(
        carried.len() >= load_inventory().len() / 2,
        "only {} of {} declared titles are carried, so the agreement rests on too few",
        carried.len(),
        load_inventory().len()
    );

    // The uncarried set is measured, then compared with the seven #470 covers.
    // The ruling changes what binds, not what the installation spells.
    let expected: BTreeSet<&str> = ["M09", "M11", "M14", "M15", "M20", "M22", "M23"]
        .into_iter()
        .collect();
    let found: BTreeSet<&str> = uncarried.iter().map(String::as_str).collect();
    assert_eq!(
        found, expected,
        "the set of uncarried declared titles changed; the owner ruling covers {expected:?}"
    );

    // Under the ruling every declared title does bind, M11 among them, and only
    // through the exact ruled spelling — never through the declared ordinal.
    let bound = context
        .clone()
        .bind_campaign(&load_inventory())
        .expect("the declared inventory binds under the ruling");
    let unresolved: Vec<String> = bound
        .sources
        .iter()
        .filter(|source| !source.unresolved_critical().is_empty())
        .map(|source| source.label.as_str().to_owned())
        .collect();
    assert_eq!(
        unresolved,
        Vec::<String>::new(),
        "every declared title resolves under the committed ruling"
    );
    assert_eq!(
        binding().campaign_position,
        Some(declared_index()),
        "M11 does not select its declared index under the ruling"
    );

    // And with the table withdrawn the same installation refuses exactly the
    // seven, at their own positions — nothing is inferred by adjacency.
    let withdrawn = context
        .clone()
        .with_title_rulings(TitleRulings::none())
        .bind_campaign(&load_inventory())
        .expect("the campaign still binds without the ruling");
    let refused: BTreeSet<&str> = withdrawn
        .sources
        .iter()
        .filter(|source| !source.unresolved_critical().is_empty())
        .map(|source| source.label.as_str())
        .collect();
    assert_eq!(refused, expected);
    for source in &withdrawn.sources {
        if source.label.as_str() == WORK_ORDER {
            assert_eq!(source.campaign_position, None);
            assert_eq!(source.catalog_id, None);
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_the_position_the_declared_order_would_select_is_read_from_the_installation() {
    // What the original data actually says at M11's declared index, read from
    // the installation and never asserted from this file. The last assertions
    // pin that the record cites exactly the ruled rows and that position — and
    // that the join is the ruling's exact spelling, not the declared ordinal.
    let context = context();
    let position = declared_index();
    let campaign = context.campaign();
    let entry = &campaign[position];

    let blocks = context.campaign_title_blocks();
    assert!(
        blocks.len() >= 2,
        "the installation offers only {} campaign-length row block(s)",
        blocks.len()
    );
    let rows: Vec<(u32, String)> = blocks
        .iter()
        .map(|block| {
            let id = block.first_id() + u32::try_from(position).expect("offset fits in u32");
            assert!(
                block.contains(id),
                "block {block} has no row at offset {position}"
            );
            (id, display_of(context, id))
        })
        .collect();
    let (bare, prefixed): (Vec<_>, Vec<_>) = rows
        .iter()
        .partition(|(_, display)| !display.contains(" - "));
    assert_eq!(
        (bare.len(), prefixed.len()),
        (1, 1),
        "expected one bare short name and one region-prefixed long name at offset {position}, \
         found {rows:?}"
    );
    let (short_id, short) = (bare[0].0, bare[0].1.as_str());
    let (long_id, long) = (prefixed[0].0, prefixed[0].1.as_str());
    assert_eq!((short_id, long_id), (RULED_SHORT_ROW, RULED_LONG_ROW));
    assert_eq!(short, RULED_SHORT_NAME);

    // The installation agrees with itself about the spelling: the
    // region-prefixed long name *ends with* exactly the string the bare short
    // name gives. This is an exact suffix test, not a similarity.
    assert!(
        long.ends_with(short),
        "row {long_id} reads {long:?}, which does not end with row {short_id}'s {short:?}"
    );
    assert_ne!(short, discovery_title());

    // Measured exactly, not matched approximately: the declared title and the
    // installation's spelling are the same words except one, and that word
    // differs by exactly one character. This is a *count*, not a comparison
    // the engine performs.
    let declared_title = discovery_title();
    let declared_words: Vec<&str> = declared_title.split(' ').collect();
    let short_words: Vec<&str> = short.split(' ').collect();
    assert_eq!(declared_words.len(), short_words.len());
    let differing: Vec<(usize, &str, &str)> = declared_words
        .iter()
        .zip(&short_words)
        .enumerate()
        .filter(|(_, (declared, retail))| declared != retail)
        .map(|(index, (declared, retail))| (index, *declared, *retail))
        .collect();
    assert_eq!(
        differing.len(),
        1,
        "the declared title differs from row {short_id} in {differing:?}, not in one word"
    );
    let (_, declared_word, retail_word) = differing[0];
    assert_eq!(declared_word.chars().count(), retail_word.chars().count());
    assert_eq!(
        declared_word
            .chars()
            .zip(retail_word.chars())
            .filter(|(declared, retail)| declared != retail)
            .count(),
        1,
        "the differing words differ in more than one character: {declared_word:?} / {retail_word:?}"
    );

    // The offset is a real directory entry with a reader archive beside it.
    let program_path = game_dir().join(&entry.program_asset);
    assert!(
        program_path.is_file(),
        "the program archive {} at offset {position} does not exist",
        entry.program_asset
    );
    assert!(entry.program_present);

    // And it is a genuine boundary: the offset is the first row of its chapter
    // and the localized long names' region groups put their boundary on the
    // same row.
    let missions_before = campaign
        .iter()
        .filter(|other| other.chapter < entry.chapter)
        .count();
    assert_eq!(missions_before, position);
    assert_eq!(entry.mission_number, 1);
    assert_ne!(campaign[position - 1].chapter, entry.chapter);
    let chapters_before: BTreeSet<u32> = campaign
        .iter()
        .map(|other| other.chapter)
        .filter(|chapter| *chapter < entry.chapter)
        .collect();
    let chapters_before = chapters_before.len();
    assert!(chapters_before > 0);
    let agreement = context.join_agreement();
    assert_eq!(
        agreement.state,
        cs_content::campaign_bindings::JoinCorroboration::Agreed
    );
    for grouped in &agreement.grouped {
        assert_eq!(
            grouped.groups.len(),
            agreement.layout_chapters.len(),
            "the region groups and the chapters account for a different number of campaigns"
        );
        assert_eq!(
            grouped.groups[..chapters_before].iter().sum::<usize>(),
            position,
            "the localized region boundary is not at offset {position}"
        );
    }

    // Finally: the record cites exactly these rows and this position, and it
    // says the join is the ruling's, not an observation.
    let binding = binding();
    assert_eq!(binding.campaign_position, Some(position));
    assert_eq!(
        binding.localized_title_id,
        Some(short_id),
        "the record does not cite the ruled short-name row"
    );
    assert!(binding.catalog_id.is_some());
    assert!(binding.world_id.is_some());
    assert!(binding.program_id.is_some());
    // The title span the record cites is the row it confirmed — inside the
    // block that encloses it, never the block itself when the row is
    // recoverable — and that span is one of the cited source spans.
    let title_source = binding
        .title_source
        .as_ref()
        .expect("a confirmed title cites its span");
    let enclosure = binding
        .title_enclosure
        .as_ref()
        .expect("a decoded title row is enclosed by its RT_STRING block");
    assert!(
        enclosure.offset() <= title_source.offset()
            && title_source.offset() + title_source.length()
                <= enclosure.offset() + enclosure.length(),
        "the cited title span is not inside the block it was decoded from"
    );
    assert!(
        binding.source_spans.iter().any(|span| {
            span.offset == title_source.offset() && span.length == title_source.length()
        }),
        "the record cites no source span for the title row it confirmed"
    );
    assert!(
        binding
            .unknowns
            .iter()
            .any(|entry| entry.starts_with("title ruling:") && entry.contains(RULED_SHORT_NAME)),
        "the record does not say its identity rests on the owner ruling: {:?}",
        binding.unknowns
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_the_campaign_keeps_everything_else_unresolved_and_unready() {
    let mut campaign = CampaignBindings::from_inventory(&load_inventory())
        .expect("the declared inventory builds the campaign");
    campaign
        .bind(binding().to_mission_binding().expect("record is valid"))
        .expect("M11 is a declared mission");
    let coverage = campaign.coverage();
    assert_eq!(coverage.total_missions, 24);
    assert_eq!(coverage.cells, 24 * 7);
    assert_eq!(coverage.complete_cells, 1, "only M11's identity cell");
    assert_eq!(coverage.unknown_cells, 24 * 7 - 1);
    assert_eq!(coverage.missing_cells, 0);
    assert_eq!(coverage.subsystem_unresolved, 24 * 23);
    assert!(!coverage.is_ready());

    let m11 = campaign.get(&label(WORK_ORDER)).expect("M11 is recorded");
    assert!(!m11.is_placeholder());
    for (category, state) in m11.cells() {
        let expected = if category == BindingCategory::MissionIdentity {
            CellState::Complete
        } else {
            CellState::Unknown
        };
        assert_eq!(state, expected, "category {category} has the wrong state");
    }
    assert!(!binding().is_verified());
}

// -------------------------------------------------------------- synthetic ---

#[test]
fn accept_m11_a_only_an_exact_title_or_an_exact_long_name_tail_confirms() {
    // The rule the join rests on, proved without an installation so CI runs it.
    // A title is carried by a row only when the row's display text *is* the
    // title, or is the title as the exact tail of a region-prefixed long name.
    // M11's own discrepancy — one letter of one word — is exactly the shape a
    // proximity rule would wave through, so it is asserted here.
    let retail = "The Stolen Starlet";
    let declared = "The Stolen Scarlet";

    assert_eq!(
        title_form(retail, retail),
        Some(TitleForm::Verbatim),
        "a row that is the title carries it verbatim"
    );
    assert_eq!(
        title_form(
            "Hawaii - The Union Jack's Revenge",
            "The Union Jack's Revenge"
        ),
        Some(TitleForm::RegionPrefixedLongName),
        "the exact tail of a region-prefixed long name carries the title"
    );
    assert_eq!(
        title_form(
            "Hollywood - Nathan Zachary & The Stolen Starlet",
            "The Stolen Starlet"
        ),
        None,
        "M11's own long name carries its pilot name after the region prefix, so its tail is not \
         the title: the ruling joins that row by position, not by this form"
    );
    assert_eq!(
        title_form(
            "Hawaii -The Union Jack's Revenge",
            "The Union Jack's Revenge"
        ),
        None,
        "the separator is an observed spelling, not the absence of a space"
    );
    assert_eq!(
        title_form(" - The Union Jack's Revenge", "The Union Jack's Revenge"),
        None,
        "an empty region prefix is not a region prefix"
    );

    for (display, title, why) in [
        (retail, declared, "one letter of one word differs"),
        (
            "Hollywood - Nathan Zachary & The Stolen Starlet",
            declared,
            "the long-name tail differs too",
        ),
        (
            "The Stolen Scarlet",
            "The Stolen Scarlet ",
            "a trailing space differs",
        ),
        ("The Stolen", declared, "a word is missing"),
        ("The Stolen Scarlet of the Sea", declared, "a word is added"),
        (
            "the stolen starlet",
            "The Stolen Starlet",
            "case alone differs",
        ),
        (
            "Hawaii - THE STOLEN STARLET",
            "The Stolen Starlet",
            "case alone differs in the tail",
        ),
        ("Stolen Starlet", declared, "a leading word is missing"),
        ("", declared, "an empty row carries nothing"),
        (declared, "", "an empty title is carried by nothing"),
        (
            "The Stolen Scarlet",
            "The Stolen Scarlet and More",
            "the title is longer",
        ),
    ] {
        assert_eq!(
            title_form(display, title),
            None,
            "{display:?} carries {title:?} even though {why}"
        );
    }
}

#[test]
fn accept_m11_a_a_ruled_title_is_named_in_the_ruling_table_and_nowhere_else() {
    let text = fs::read_to_string(repo_path("missions/bindings/title-rulings.tsv"))
        .expect("the ruling table is committed");
    let rulings = TitleRulings::parse(&text).expect("the table parses");
    // The table covers exactly the seven work orders the owner ruling names and
    // no other: a row for an order the owner did not rule on would invent a
    // join, and a missing row would silently drop one.
    let labels: BTreeSet<&str> = rulings.iter().map(|ruling| ruling.label.as_str()).collect();
    assert_eq!(
        labels,
        ["M09", "M11", "M14", "M15", "M20", "M22", "M23"]
            .into_iter()
            .collect()
    );
    let ruling = rulings
        .iter()
        .find(|ruling| ruling.label.as_str() == WORK_ORDER)
        .expect("M11 has a ruling row");
    assert_eq!(ruling.declared_title, discovery_title());
    assert_eq!(ruling.retail_short_name, RULED_SHORT_NAME);
    assert_eq!(
        (ruling.short_row, ruling.long_row),
        (RULED_SHORT_ROW, RULED_LONG_ROW)
    );
    // The row exists because the two spellings differ, and the join is exact:
    // the ruled name is carried verbatim, the declared one by nothing.
    assert_ne!(ruling.declared_title, ruling.retail_short_name);
    assert_eq!(
        title_form(&ruling.retail_short_name, &ruling.retail_short_name),
        Some(TitleForm::Verbatim)
    );
    assert_eq!(
        title_form(&ruling.retail_short_name, &ruling.declared_title),
        None,
        "the ruled row must not carry the declared spelling, or a row would relax the comparison"
    );

    // Failure case: a table without M11's row has no ruling for it.
    let stripped: String = text
        .lines()
        .filter(|line| !line.starts_with("M11\t"))
        .map(|line| format!("{line}\n"))
        .collect();
    let stripped = TitleRulings::parse(&stripped).expect("the stripped table parses");
    assert!(
        stripped
            .iter()
            .all(|ruling| ruling.label.as_str() != WORK_ORDER)
    );
    assert_eq!(stripped.iter().count(), 6);
}

#[test]
fn accept_m11_a_an_unresolved_critical_dependency_is_named_in_the_records_unknowns() {
    // The rule this stage adds, proved without an installation: a record whose
    // critical dependencies are unresolved names every one of them, with the
    // refusal that caused it, in `unknowns` — and a record that resolves all
    // five names none of them. `to_json` writes no dependency states, so this is
    // the only place the reason survives into the committed record.
    let unresolved = SourceBinding {
        label: label_of_work_order(),
        discovery_title: "Authored Title".to_owned(),
        install_sha256: "0".repeat(64),
        campaign_position: None,
        campaign_size: 24,
        catalog_id: None,
        world_id: None,
        program_id: None,
        localized_title_id: None,
        localized_title_language: None,
        dependencies: vec![
            SourceDependency {
                id: CriticalDependency::MissionId,
                state: DependencyState::unresolved(claim("mission_id"), UNCARRIED_TITLE_REFUSAL)
                    .expect("the refusal is a valid reason"),
            },
            SourceDependency {
                id: CriticalDependency::InstallHash,
                state: DependencyState::resolved(provenance("installation_hash")),
            },
            SourceDependency {
                id: CriticalDependency::TitleString,
                state: DependencyState::unresolved(claim("title_string"), UNCARRIED_TITLE_REFUSAL)
                    .expect("the refusal is a valid reason"),
            },
            SourceDependency {
                id: CriticalDependency::ProgramSourceMap,
                state: DependencyState::unresolved(
                    claim("program_source_map"),
                    NO_CONFIRMED_ROW_REFUSAL,
                )
                .expect("the refusal is a valid reason"),
            },
            SourceDependency {
                id: CriticalDependency::WorldGroupVariant,
                state: DependencyState::unresolved(claim("world_group_variant"), "no world group")
                    .expect("the refusal is a valid reason"),
            },
        ],
        source_spans: Vec::new(),
        identity_source: None,
        title_source: None,
        title_enclosure: None,
        closure_sha256: None,
        evidence_ids: Vec::new(),
        unknowns: vec![
            "closure_sha256: the mission dependency closure hash is not measured".to_owned(),
        ],
    };
    unresolved
        .validate()
        .expect("the authored record is internally consistent");
    assert_eq!(
        unresolved_critical_entries(&unresolved.dependencies),
        vec![
            format!(
                "{}: unresolved — {UNCARRIED_TITLE_REFUSAL}",
                CriticalDependency::MissionId.label()
            ),
            format!(
                "{}: unresolved — {UNCARRIED_TITLE_REFUSAL}",
                CriticalDependency::TitleString.label()
            ),
            format!(
                "{}: unresolved — {NO_CONFIRMED_ROW_REFUSAL}",
                CriticalDependency::ProgramSourceMap.label()
            ),
            format!(
                "{}: unresolved — no world group",
                CriticalDependency::WorldGroupVariant.label()
            ),
        ],
        "each unresolved critical dependency must be named once, with its own reason, in \
         checklist order"
    );
    assert_eq!(
        unresolved.unresolved_critical(),
        vec![
            CriticalDependency::MissionId,
            CriticalDependency::TitleString,
            CriticalDependency::ProgramSourceMap,
            CriticalDependency::WorldGroupVariant,
        ]
    );

    // The resolved counterpart contributes nothing, which is why the committed
    // records of the stages that resolve all five carry no such entry.
    let resolved = SourceBinding {
        campaign_position: Some(10),
        catalog_id: Some(
            cs_types::content::ContentId::from_source(ContentKind::Mission, "ch3-m01").expect("id"),
        ),
        world_id: Some(
            cs_types::content::ContentId::from_source(ContentKind::World, "c1").expect("id"),
        ),
        program_id: Some(
            cs_types::content::ContentId::from_source(ContentKind::Script, "c1-m01-zrdr")
                .expect("id"),
        ),
        localized_title_id: Some(RULED_SHORT_ROW),
        localized_title_language: Some(1033),
        dependencies: CriticalDependency::ALL
            .iter()
            .map(|id| SourceDependency {
                id: *id,
                state: DependencyState::resolved(provenance(id.label())),
            })
            .collect(),
        ..unresolved
    };
    assert!(
        unresolved_critical_entries(&resolved.dependencies).is_empty(),
        "a fully resolved dependency list must contribute no unknown entry"
    );
    assert!(resolved.unresolved_critical().is_empty());
    resolved
        .validate()
        .expect("the fully resolved record is internally consistent");
    assert!(
        !resolved.is_verified(),
        "resolving the five dependencies alone must not read as verified: the checklist entries \
         are still unknown"
    );
}

/// A claim id for a synthetic assertion.
fn claim(suffix: &str) -> ClaimId {
    ClaimId::new(&format!("m11.a.synthetic.{suffix}")).expect("test claim id is valid")
}

/// Designed provenance for a value this file invents.
fn provenance(suffix: &str) -> Provenance {
    Provenance::new(claim(suffix), ClaimStatus::Designed, None)
        .expect("designed provenance always validates")
}
