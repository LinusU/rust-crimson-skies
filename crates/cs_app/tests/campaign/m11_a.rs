//! Acceptance stage M11-A: bind the eleventh mission's original data and
//! branches (`missions/M11.md`, work order `M11-A`).
//!
//! **This stage does not meet its minimum acceptance scenario, and its tests
//! say so instead of hiding it.** The scenario is *"Source-derived binding has
//! no unresolved critical dependencies"*; for M11 the declared discovery title
//! `The Stolen Scarlet` is carried by **no** localized row in the owner's
//! installation — the installation spells this mission `The Stolen Starlet` —
//! so [`CriticalDependency::TitleString`] stays unresolved, and with it the
//! campaign position the mission, world and program identities are read from.
//! That is the honest state of the original data, not a defect of this code,
//! and the retail tests below pin it:
//!
//! * [`accept_m11_a_the_declared_title_is_carried_by_no_retail_row`] measures
//!   that no row of *any* campaign-length block carries M11's title in either
//!   observed display form — and that the scan is not vacuous, because the same
//!   scan does find a declared title the installation does carry.
//! * [`accept_m11_a_the_source_derived_binding_keeps_m11_unresolved_and_says_why`]
//!   pins the unresolved set, the refusal each dependency carries, and the rule
//!   M11-A adds: [`SourceBinding::unknowns`] names every unresolved critical
//!   dependency, because [`SourceBinding::to_json`] writes no dependency states
//!   and a record with three `null` ids would otherwise carry no reason at all.
//! * [`accept_m11_a_the_committed_record_is_what_the_installation_derives`]
//!   pins `missions/bindings/M11.json` to what production code derives.
//! * [`accept_m11_a_the_declared_order_agrees_with_the_retail_campaign_order_where_it_is_carried`]
//!   is evidence for the *owner decision*, measured here because nothing else
//!   measures it: for all 24 declared work orders, every one the localized table
//!   carries exactly selects its own index of the declared inventory, and
//!   exactly seven are carried by no row. Nothing binds M11 from that
//!   measurement; the test asserts that too.
//! * [`accept_m11_a_the_position_the_declared_order_would_select_is_read_from_the_installation`]
//!   reads what the original data says at that index — both blocks' rows, the
//!   exact word-level difference, the chapter and region-group boundary that
//!   falls there, the program archive that exists — and pins that the binding
//!   still cites none of it.
//! * [`accept_m11_a_the_campaign_keeps_everything_else_unresolved_and_unready`]
//!   pins the campaign totals: with M11 recorded and nothing bound, no cell is
//!   complete and coverage is not ready.
//!
//! Which retail string names M11 — or a correction to the declared title — is
//! Rally #470 `M05-A-GUIDE-TITLES`, an owner decision recorded as `blocked`.
//! Nothing here approximates it: no fuzzy comparison was added, `TitleForm`
//! stays exact, and the two synthetic tests below prove the exactness rules the
//! refusal rests on, including the arm M11 is itself.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which has
//! no original data) skips them; they are run with `--include-ignored` by the
//! implementing and reviewing agents. Every assertion is made against facts the
//! test re-reads from the installation or from committed records — never against
//! a constant that repeats the implementation. The two synthetic tests are left
//! unignored so CI runs them.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    BindingCategory, CONTRADICTED_JOIN_REFUSAL, CampaignBindings, CampaignInventory,
    CampaignMission, CategoryState, CellState, CriticalDependency, DependencyState,
    GroupedTitleBlock, JoinAgreement, JoinCorroboration, MissionLabel, NO_CONFIRMED_ROW_REFUSAL,
    SourceBinding, SourceContext, SourceDependency, TitleBlock, TitleConfirmation, TitleForm,
    UNCARRIED_TITLE_REFUSAL, campaign_position_for, classify_join, title_blocks, title_form,
    unresolved_critical_entries,
};
use cs_types::content::{ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};

use crate::common::{label, load_inventory, repo_path};

/// The one work order this stage binds.
const WORK_ORDER: &str = "M11";

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M11-A needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The source context, read once for the whole suite (fingerprinting the
/// installation walks every file, so it happens exactly once).
fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

/// The declared campaign inventory, read from the committed denominator.
fn inventory() -> &'static CampaignInventory {
    static INVENTORY: OnceLock<CampaignInventory> = OnceLock::new();
    INVENTORY.get_or_init(load_inventory)
}

/// The declared discovery title of [`WORK_ORDER`], read from the committed
/// inventory rather than repeated here.
fn discovery_title() -> &'static str {
    inventory()
        .iter()
        .find(|(declared, _)| declared.as_str() == WORK_ORDER)
        .map(|(_, title)| title.as_str())
        .unwrap_or_else(|| panic!("the declared inventory has no {WORK_ORDER} work order"))
}

/// [`WORK_ORDER`]'s index in the declared inventory.
///
/// This is a property of the committed work-order list, not of the
/// installation: nothing binds from it. The retail tests that use it treat it as
/// a *measured* fact about the declared order and assert what the installation
/// says at that offset, never the other way round.
fn declared_index() -> usize {
    inventory()
        .iter()
        .position(|(declared, _)| declared.as_str() == WORK_ORDER)
        .expect("the declared inventory has the work order")
}

/// The M11 binding derived from the installation, built once.
fn binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        context()
            .bind(
                MissionLabel::new(WORK_ORDER).expect("M11 is a valid label"),
                discovery_title(),
            )
            .expect("M11 binds to the original data it does or does not name")
    })
}

/// The declared campaign, with M11 recorded the way the record describes.
fn campaign() -> CampaignBindings {
    let mut campaign =
        CampaignBindings::from_inventory(inventory()).expect("the declared inventory builds");
    campaign
        .bind(binding().to_mission_binding().expect("record is valid"))
        .expect("M11 is a declared mission");
    campaign
}

/// The comparable text of one retail row: a leading display tag such as
/// `[AB14I]` is a presentation instruction, not part of the title. This is the
/// same rule `cs_content::campaign_bindings` applies before it compares a row
/// with a title, so a row scan here and a confirmation there see the same text.
fn display_text(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('[') else {
        return text;
    };
    let Some(end) = rest.find(']') else {
        return text;
    };
    &text[end + 2..]
}

/// The display text of one localized row, read from the installation rather
/// than authored.
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

/// The display text of every row of every campaign-length block, as the
/// production confirmation sees it.
fn campaign_row_displays(context: &SourceContext) -> Vec<(u32, String)> {
    let mut rows = Vec::new();
    for block in context.campaign_title_blocks() {
        for id in block.first_id()..=block.last_id() {
            rows.push((id, display_of(context, id)));
        }
    }
    rows
}

/// Why one critical dependency of `binding` was left unresolved.
fn unresolved_reason(binding: &SourceBinding, id: CriticalDependency) -> String {
    let DependencyState::Unresolved { reason, .. } = binding
        .dependency(id)
        .unwrap_or_else(|| panic!("{id} has no recorded state"))
    else {
        panic!("{id} is resolved, so it has no unresolved reason");
    };
    reason.clone()
}

// ---------------------------------------------------------------- retail ---

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_the_declared_title_is_carried_by_no_retail_row() {
    // M11-BIND: "Confirm the title against local strings." That confirmation
    // fails, and this is the measurement of the failure: the installation
    // spells this mission `The Stolen Starlet`, and `The Stolen Scarlet` — the
    // title `missions/README.md` declares — is in no row of any campaign-length
    // block in either observed display form. No row carries it, so no row names
    // a campaign position, so the three identities read from a position stay
    // unresolved.
    let context = context();
    let title = discovery_title();

    assert_eq!(
        context.confirm_title(title),
        TitleConfirmation::Uncarried,
        "the declared title is now carried by a retail row, so this stage's refusal is stale"
    );

    let blocks = context.campaign_title_blocks();
    assert!(
        blocks.len() >= 2,
        "the installation offers only {} campaign-length row block(s), so 'no row carries it' \
         would be a statement about one structure",
        blocks.len()
    );
    let rows = campaign_row_displays(context);
    assert_eq!(
        rows.len(),
        blocks.iter().map(TitleBlock::len).sum::<usize>(),
        "the row scan did not visit every row of every block"
    );
    for (id, display) in &rows {
        assert_ne!(
            display, title,
            "row {id} is the declared title itself, so the table does carry it"
        );
        assert_eq!(
            title_form(display, title),
            None,
            "row {id} carries the declared title as {display:?} after all"
        );
    }

    // The scan is not vacuous: the same rule over the same rows finds some
    // declared titles the installation carries once, and some it carries in
    // *both* display forms. A scan that matched nothing at all would otherwise
    // let this test pass.
    let carrier_counts: Vec<usize> = inventory()
        .iter()
        .map(|(_, title)| {
            rows.iter()
                .filter(|(_, display)| title_form(display, title).is_some())
                .count()
        })
        .collect();
    assert_eq!(carrier_counts.len(), inventory().len());
    assert!(
        carrier_counts.iter().any(|count| *count == 1),
        "no declared title is carried by exactly one row, so the scan above proves nothing: \
         {carrier_counts:?}"
    );
    assert!(
        carrier_counts.iter().any(|count| *count >= 2),
        "no declared title is carried by more than one row, so the scan cannot tell one carrier \
         from several: {carrier_counts:?}"
    );
    assert!(
        carrier_counts.iter().filter(|count| **count > 0).count() >= inventory().len() / 2,
        "only {} of {} declared titles are carried by any row",
        carrier_counts.iter().filter(|count| **count > 0).count(),
        inventory().len()
    );
    assert_eq!(
        carrier_counts[declared_index()],
        0,
        "M11's declared title is carried after all"
    );

    // The refusal is the *title's*, not the table's: the localized rows still
    // agree with the campaign directory layout, exactly as M02-A's guard
    // requires. A binding that cannot name M11 must not be able to blame a
    // contradicting table for it instead.
    let agreement = context.join_agreement();
    assert_eq!(
        agreement.state,
        JoinCorroboration::Agreed,
        "the localized table contradicts the layout, so M11's refusal would have another cause"
    );
    assert!(agreement.establishes());
    assert!(
        !agreement.grouped.is_empty(),
        "no campaign-length block groups into region groups, so the agreement rests on the \
         layout alone"
    );
    assert_eq!(
        campaign_position_for(None, &agreement),
        Err(NO_CONFIRMED_ROW_REFUSAL),
        "no confirmed row must select a position, whatever the table says"
    );

    // The record carries the refusal instead of an empty identity: no title
    // row, no campaign position, no cited span, nothing verified.
    let binding = binding();
    assert_eq!(binding.localized_title_id, None);
    assert_eq!(binding.localized_title_language, None);
    assert_eq!(binding.campaign_position, None);
    assert!(
        binding.source_spans.is_empty(),
        "an unresolved binding must not cite a source span it never resolved: {:?}",
        binding.source_spans
    );
    assert!(binding.title_source.is_none());
    assert!(binding.identity_source.is_none());
    assert!(!binding.is_verified());
    assert_eq!(
        unresolved_reason(binding, CriticalDependency::TitleString),
        UNCARRIED_TITLE_REFUSAL,
        "M11's title dependency must be refused as uncarried, with its own reason"
    );
    // The three identities that need a position are refused by the join's own
    // sentence, because *that* is what is missing: no confirmed row, therefore
    // no position.
    for id in [
        CriticalDependency::MissionId,
        CriticalDependency::ProgramSourceMap,
        CriticalDependency::WorldGroupVariant,
    ] {
        assert_eq!(
            unresolved_reason(binding, id),
            NO_CONFIRMED_ROW_REFUSAL,
            "{id} must name the missing confirmed row as its cause"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_the_source_derived_binding_keeps_m11_unresolved_and_says_why() {
    // The stage's minimum scenario is "Source-derived binding has no
    // unresolved critical dependencies". This asserts the measured state
    // instead of the scenario: four of the five are unresolved and the one that
    // is not depends on no mission name at all, because it is measured from the
    // installation as a whole.
    let binding = binding();
    binding
        .validate()
        .expect("the derived record is internally consistent");
    assert_eq!(
        binding
            .dependencies
            .iter()
            .map(|dependency| dependency.id)
            .collect::<Vec<_>>(),
        CriticalDependency::ALL.to_vec(),
        "the record carries exactly the checklist's critical dependencies, in order"
    );
    assert_eq!(
        binding.unresolved_critical(),
        vec![
            CriticalDependency::MissionId,
            CriticalDependency::TitleString,
            CriticalDependency::ProgramSourceMap,
            CriticalDependency::WorldGroupVariant,
        ],
        "the derived record left a different critical dependency unresolved"
    );

    // The one resolved dependency does not depend on the title, and its value
    // is re-measured here through production discovery, independently of `bind`.
    let DependencyState::Resolved { provenance } = binding
        .dependency(CriticalDependency::InstallHash)
        .expect("hash resolved")
    else {
        panic!("the installation hash must be resolved");
    };
    assert_eq!(provenance.class, ClaimStatus::ObservedTool);
    let found = cs_assets::install::discover(&game_dir()).expect("discovery reads the install");
    assert_eq!(
        binding.install_sha256,
        cs_assets::install::fingerprint(&found.manifest).to_hex(),
        "the recorded installation hash is not the one production discovery measures"
    );

    // The three identities are absent, not defaulted.
    assert_eq!(binding.catalog_id, None);
    assert_eq!(binding.world_id, None);
    assert_eq!(binding.program_id, None);
    assert_eq!(binding.campaign_size, 24);

    // The rule this stage adds: a record whose dependencies are unresolved
    // names them *in the record*. `to_json` writes no dependency states, so
    // without this the committed `M11.json` would hold three `null` ids and no
    // reason, and a reader could not tell a mission the installation does not
    // name from a stage that did not reach it.
    assert_eq!(
        unresolved_critical_entries(&binding.dependencies),
        binding
            .unknowns
            .iter()
            .filter(|entry| entry.contains(": unresolved — "))
            .cloned()
            .collect::<Vec<_>>(),
        "the record's own unknown entries are not the unresolved critical dependencies"
    );
    for id in binding.unresolved_critical() {
        let reason = unresolved_reason(binding, id);
        assert!(
            binding
                .unknowns
                .contains(&format!("{}: unresolved — {reason}", id.label())),
            "the record does not name {id} as unresolved with the reason {reason:?}: {:?}",
            binding.unknowns
        );
    }
    // A resolved dependency contributes no such entry, which is why the
    // committed records of the stages that resolve all five carry none.
    assert!(
        !binding
            .unknowns
            .iter()
            .any(|entry| entry.starts_with(CriticalDependency::InstallHash.label())),
        "the resolved installation hash must not be recorded as unresolved"
    );
    // The checklist entries this stage never binds are still named, so the
    // record cannot be read as complete by counting only the dependencies.
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
    assert!(!binding.is_verified());
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_the_committed_record_is_what_the_installation_derives() {
    let committed = fs::read_to_string(repo_path("missions/bindings/M11.json"))
        .expect("missions/bindings/M11.json exists");
    let derived = binding().to_json();
    assert_eq!(
        committed, derived,
        "the committed binding record is not what production code derives from $CS_GAME_DIR"
    );

    // The record is the schema's shape, with the identities explicitly null
    // and the refusals that kept them null written out.
    for key in [
        "\"schema_version\": 1",
        "\"work_order\": \"M11\"",
        "\"discovery_title\": \"The Stolen Scarlet\"",
        "\"verified\": false",
        "\"install_sha256\": \"",
        "\"catalog_id\": null",
        "\"world_id\": null",
        "\"program_id\": null",
        "\"closure_sha256\": null",
        "\"source_spans\": []",
        "\"unknowns\": [",
        "\"evidence_ids\": []",
    ] {
        assert!(
            derived.contains(key),
            "the derived record is missing {key:?}:\n{derived}"
        );
    }
    assert!(
        derived.contains(&format!(
            "{}: unresolved — {UNCARRIED_TITLE_REFUSAL}",
            CriticalDependency::TitleString.label()
        )),
        "the committed record does not name the title string as unresolved with its reason:\n\
         {derived}"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_the_declared_order_agrees_with_the_retail_campaign_order_where_it_is_carried() {
    // The fact the owner decision (Rally #470) rests on, measured here because
    // nothing else measures it. For all 24 declared work orders: every title the
    // localized table carries *exactly* selects its own index of the declared
    // inventory, and exactly the seven the M05-A finding lists are carried by no
    // row at all. This binds nothing; the assertions at the end prove M11 is
    // still unbound after it.
    let context = context();
    let agreement = context.join_agreement();
    assert_eq!(
        agreement.state,
        JoinCorroboration::Agreed,
        "the localized table does not corroborate the layout, so no position can be measured"
    );

    let mut carried = Vec::new();
    let mut uncarried = Vec::new();
    for (index, (declared, title)) in inventory().iter().enumerate() {
        let confirmation = context.confirm_title(title);
        assert_eq!(
            confirmation.refusal().is_some(),
            !matches!(confirmation, TitleConfirmation::Confirmed { .. }),
            "{declared}: a confirmation and a refusal cannot both hold"
        );
        match campaign_position_for(confirmation.confirmed().map(|(row, _)| row), &agreement) {
            Ok(position) => carried.push((index, declared.as_str().to_owned(), position)),
            Err(NO_CONFIRMED_ROW_REFUSAL) => uncarried.push(declared.as_str().to_owned()),
            Err(other) => panic!("{declared} was refused for an unexpected reason: {other:?}"),
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
        carried.len() >= inventory().len() / 2,
        "only {} of {} declared titles are carried, so the agreement rests on too few",
        carried.len(),
        inventory().len()
    );

    // The uncarried set is measured, then compared with the seven #470 covers.
    let expected: BTreeSet<&str> = ["M09", "M11", "M14", "M15", "M20", "M22", "M23"]
        .into_iter()
        .collect();
    let found: BTreeSet<&str> = uncarried.iter().map(String::as_str).collect();
    assert_eq!(
        found, expected,
        "the set of uncarried declared titles changed; the owner decision #470 covers {expected:?}"
    );
    assert!(
        uncarried.iter().any(|declared| declared == WORK_ORDER),
        "M11 is no longer among the uncarried titles, so this stage's refusal is stale"
    );

    // Whatever the order agreement says, this stage binds nothing from it.
    let binding = binding();
    assert_eq!(binding.campaign_position, None);
    assert_eq!(binding.catalog_id, None);
    assert!(!binding.unresolved_critical().is_empty());

    // And the join is per-title, never per-index: the spelling the
    // installation *does* carry resolves the five critical dependencies through
    // the normal exact comparison, which is precisely why M11's declared
    // spelling does not. That record is the owner decision #470 has to make, so
    // it is measured here and bound by nothing.
    let borrowed = context
        .bind(
            MissionLabel::new(WORK_ORDER).expect("M11 is a valid label"),
            "The Stolen Starlet",
        )
        .expect("a spelling the strings carry binds without I/O failure");
    borrowed
        .validate()
        .expect("the borrowed record is internally consistent");
    assert_eq!(
        borrowed.discovery_title, "The Stolen Starlet",
        "the borrowed record does not say which title it was bound from"
    );
    assert_eq!(
        borrowed.unresolved_critical(),
        Vec::new(),
        "a spelling the strings carry exactly must resolve all five critical dependencies, so the \
         exact comparison is what keeps M11 unbound"
    );
    assert_eq!(
        borrowed.campaign_position,
        Some(declared_index()),
        "the carried spelling does not select M11's declared index of the inventory"
    );
    assert_eq!(
        borrowed.localized_title_id.is_some(),
        true,
        "the borrowed record cites no localized row"
    );
    // The two records differ only in what they were asked, and the asked-for
    // one stays empty.
    assert_ne!(borrowed.catalog_id, binding.catalog_id);
    assert_ne!(borrowed.localized_title_id, binding.localized_title_id);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_the_position_the_declared_order_would_select_is_read_from_the_installation() {
    // What the original data actually says at M11's declared index, read from
    // the installation and never asserted from this file. **Nothing here is a
    // binding**: the last assertions prove the record still cites neither row
    // and derives no identity, and choosing which retail string names M11 is
    // Rally #470, an owner decision.
    let context = context();
    let position = declared_index();
    let campaign = context.campaign();
    let entry: &CampaignMission = &campaign[position];

    // Every campaign-length block has a row at this offset. The blocks are not
    // classified by which of them comes first in the table — they are classified
    // by the observed separator, so a change of block order cannot relabel them.
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

    // The installation agrees with itself about the spelling: the region-prefixed
    // long name *ends with* exactly the string the bare short name gives, so the
    // two structures of the original describe one mission under one spelling.
    // This is an exact suffix test, not a similarity: `long` either ends with
    // `short` or it does not.
    assert!(
        long.ends_with(short),
        "row {long_id} reads {long:?}, which does not end with row {short_id}'s {short:?}"
    );
    assert_ne!(short, discovery_title());

    // Measured exactly, not matched approximately: the declared title and the
    // installation's spelling are the same words except one, and that word
    // differs by exactly one character. This is a *count*, not a comparison the
    // engine performs — nothing in production treats the two strings as equal.
    let declared_words: Vec<&str> = discovery_title().split(' ').collect();
    let short_words: Vec<&str> = short.split(' ').collect();
    assert_eq!(
        declared_words.len(),
        short_words.len(),
        "the declared title and row {short_id} do not have the same number of words"
    );
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
    assert_eq!(
        declared_word.chars().count(),
        retail_word.chars().count(),
        "the differing words {declared_word:?} and {retail_word:?} have different lengths"
    );
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

    // And it is a genuine boundary, which is the second piece of evidence for
    // the owner decision: the offset is the first row of its chapter, and the
    // localized long names' region groups — which fall into the layout's chapter
    // sizes — put their boundary on the same row.
    let missions_before = campaign
        .iter()
        .filter(|other| other.chapter < entry.chapter)
        .count();
    assert_eq!(
        missions_before, position,
        "offset {position} is not the first row of its chapter, so it is not a boundary"
    );
    assert_eq!(
        entry.mission_number, 1,
        "offset {position} is not a chapter's first mission: {entry:?}"
    );
    assert_ne!(
        campaign[position - 1].chapter,
        entry.chapter,
        "the previous offset is in the same chapter, so {position} is not a boundary"
    );
    // How many *chapters* precede this one: the index the localized region
    // groups must put their boundary at, as opposed to how many *missions* do.
    let chapters_before: BTreeSet<u32> = campaign
        .iter()
        .map(|other| other.chapter)
        .filter(|chapter| *chapter < entry.chapter)
        .collect();
    assert!(
        !chapters_before.is_empty(),
        "no chapter precedes offset {position}, so its boundary is the campaign's own first row"
    );
    let chapters_before = chapters_before.len();
    let agreement = context.join_agreement();
    assert_eq!(agreement.state, JoinCorroboration::Agreed);
    assert!(!agreement.grouped.is_empty());
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

    // Finally: the binding cites none of it. No id, no position, no span, and
    // in particular none of the rows read above.
    let binding = binding();
    assert_eq!(binding.campaign_position, None);
    assert_eq!(binding.catalog_id, None);
    assert_eq!(binding.world_id, None);
    assert_eq!(binding.program_id, None);
    assert!(binding.source_spans.is_empty());
    for id in [short_id, long_id] {
        assert!(
            !binding
                .unknowns
                .iter()
                .any(|entry| entry.contains("source span") && entry.contains(&id.to_string())),
            "the record's unknowns name row {id} it never resolved"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m11_a_the_campaign_keeps_everything_else_unresolved_and_unready() {
    // M11 is *recorded* now — the placeholder is replaced — but nothing about it
    // is bound, so the denominator, the cell counts and readiness read exactly
    // as they did before, with one difference a reader must be able to see: M11
    // is no longer a placeholder, and its identity cell says *why* it is empty.
    let campaign = campaign();
    let coverage = campaign.coverage();
    assert_eq!(
        coverage.total_missions, 24,
        "the denominator is the campaign"
    );
    assert_eq!(coverage.declared_missions, 24);
    assert_eq!(coverage.cells, 24 * 7);
    assert_eq!(
        coverage.complete_cells, 0,
        "no cell of any mission may read as complete while M11 is unbound"
    );
    assert_eq!(coverage.unknown_cells, 24 * 7);
    assert_eq!(coverage.missing_cells, 0);
    assert_eq!(coverage.unsupported_cells, 0);
    assert_eq!(coverage.subsystem_rows, 24 * 23);
    assert_eq!(
        coverage.subsystem_unresolved,
        24 * 23,
        "no subsystem is implemented yet, so no subsystem row may read as resolved"
    );
    assert_eq!(coverage.progression_unknown, 24);
    assert!(
        !coverage.is_ready(),
        "a campaign whose only recorded mission is entirely unknown must not read as ready"
    );

    let m11 = campaign.get(&label(WORK_ORDER)).expect("M11 is recorded");
    assert!(
        !m11.is_placeholder(),
        "M11 is still the unbound placeholder the inventory declared"
    );
    assert_eq!(m11.catalog_identity(), None);
    assert!(
        m11.cells().all(|(_, state)| state == CellState::Unknown),
        "an unbound mission must leave every required category explicitly unknown"
    );
    assert!(
        m11.dependencies
            .iter()
            .all(|row| matches!(row.state, DependencyState::Unresolved { .. }))
    );
    // The identity cell is unknown for a recorded reason, not a blank one.
    let identity = m11
        .category(BindingCategory::MissionIdentity)
        .expect("the identity category is recorded");
    let CategoryState::Unresolved { reason, .. } = identity else {
        panic!("M11's identity must be unresolved while its title is uncarried");
    };
    assert!(
        reason.contains("incomplete") || reason.contains("identity"),
        "the identity cell's reason does not say the identity is incomplete: {reason:?}"
    );

    let closure = campaign
        .closure(&label(WORK_ORDER), None)
        .expect("M11's closure computes without a catalog");
    assert_eq!(closure.reached, vec![label(WORK_ORDER)]);
    assert_eq!(closure.cell_count(), 7);
    assert_eq!(closure.complete_cells(), 0);
    assert_eq!(closure.unresolved_subsystems, 23);
    assert_eq!(closure.unknown_progression, 1);
    assert!(!closure.is_complete());
}

// -------------------------------------------------------------- synthetic ---

#[test]
fn accept_m11_a_only_an_exact_title_or_an_exact_long_name_tail_confirms() {
    // The rule M11 is refused by, proved without an installation so CI runs it.
    // A title is carried by a row only when the row's display text *is* the
    // title, or is the title as the exact tail of a region-prefixed long name.
    // M11's own failure is the arm that matters — a difference of one letter in
    // one word — and it is exactly the shape a proximity rule would wave
    // through, so it is asserted here rather than left implicit.
    let retail = "The Stolen Starlet";
    let declared = "The Stolen Scarlet";

    // The two forms, on strings the installation actually carries.
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
            "Rocky Mountains - The Fight for the FIGAROA",
            "The Fight for the FIGAROA"
        ),
        Some(TitleForm::RegionPrefixedLongName),
        "a multi-word region prefix is dropped whole, never compared"
    );
    // The prefix is dropped without being interpreted, so any words may
    // precede the tail — and the *separator* is the observed spelling, not
    // arbitrary whitespace.
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
    assert_eq!(
        title_form(
            "Hollywood - Nathan Zachary & The Red Menace",
            "The Red Menace"
        ),
        None,
        "a long name whose tail carries extra words does not carry the title"
    );

    // Every way a row can fail to carry a title. M11's own difference is the
    // first arm; the others are the failures a weaker comparison would let
    // through.
    for (display, title, why) in [
        (retail, declared, "one letter of one word differs"),
        (
            "Hawaii - The Stolen Starlet",
            declared,
            "the long-name tail differs too",
        ),
        (
            "Hollywood - Nathan Zachary & The Stolen Starlet",
            declared,
            "the tail differs",
        ),
        (
            "The Stolen Scarlet",
            "The Stolen Scarlet ",
            "a trailing space differs",
        ),
        ("The Stolen Scarlet ", declared, "a trailing space differs"),
        ("The Stolen", declared, "a word is missing"),
        ("The Stolen Scarlet of the Sea", declared, "a word is added"),
        ("the stolen starlet", declared, "case differs"),
        ("THE STOLEN STARLET", declared, "case differs"),
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
fn accept_m11_a_an_unresolved_critical_dependency_is_named_in_the_records_unknowns() {
    // The rule M11-A adds, proved without an installation: a record whose
    // critical dependencies are unresolved names every one of them, with the
    // refusal that caused it, in `unknowns` — and a record that resolves all
    // five names none of them. `to_json` writes no dependency states, so this is
    // the only place the reason survives into the committed record, and the
    // fully resolved arm is what keeps the earlier stages' committed records
    // unchanged.
    let unresolved = SourceBinding {
        label: MissionLabel::new(WORK_ORDER).expect("M11 is a valid label"),
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
        closure_sha256: None,
        evidence_ids: Vec::new(),
        unknowns: Vec::new(),
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
    assert!(
        !unresolved_critical_entries(&unresolved.dependencies)
            .iter()
            .any(|entry| entry.starts_with(CriticalDependency::InstallHash.label())),
        "the resolved installation hash must not be recorded as unresolved"
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
    assert!(
        !unresolved.is_verified(),
        "a record with four unresolved dependencies must not read as verified"
    );

    // The resolved counterpart: the same dependency list with all five resolved
    // contributes nothing, which is why the committed records of M01-A … M06-A
    // carry no such entry and this production change did not alter them.
    let resolved = SourceBinding {
        campaign_position: Some(10),
        catalog_id: Some(
            cs_types::content::ContentId::from_source(ContentKind::Mission, "ch3-m01").expect("id"),
        ),
        world_id: Some(
            cs_types::content::ContentId::from_source(ContentKind::World, "c3").expect("id"),
        ),
        program_id: Some(
            cs_types::content::ContentId::from_source(ContentKind::Script, "c3-m01-zrdr")
                .expect("id"),
        ),
        localized_title_id: Some(1),
        localized_title_language: Some(1033),
        dependencies: CriticalDependency::ALL
            .iter()
            .map(|id| SourceDependency {
                id: *id,
                state: DependencyState::resolved(provenance(id.label())),
            })
            .collect(),
        // One checklist entry this stage never binds, so the record is still
        // internally consistent and still not verified — which is the state
        // every committed binding of M01-A … M06-A is in.
        unknowns: vec![
            "closure_sha256: the mission dependency closure hash is not measured".to_owned(),
        ],
        ..unresolved
    };
    assert!(
        unresolved_critical_entries(&resolved.dependencies).is_empty(),
        "a fully resolved dependency list must contribute no unknown entry"
    );
    assert!(
        !resolved
            .unknowns
            .iter()
            .any(|entry| entry.contains(": unresolved — ")),
        "a fully resolved record must not record an unresolved dependency: {:?}",
        resolved.unknowns
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

    // The last predicate the retail tests lean on and CI cannot see: a
    // localized table that groups differently from the campaign layout selects
    // no position, even for a row it does carry. Derived through production
    // `classify_join`, not authored.
    let blocks = title_blocks(&(0u32..24).collect());
    assert_eq!(blocks.len(), 1, "0..24 is one campaign-length run");
    let grouped = vec![GroupedTitleBlock {
        block: blocks[0],
        groups: vec![6, 6, 6, 6],
    }];
    let layout = vec![5usize, 5, 5, 5, 4];
    assert_eq!(
        classify_join(&layout, &grouped),
        JoinCorroboration::Disagreed
    );
    let contradicted = JoinAgreement {
        layout_chapters: layout.clone(),
        blocks,
        grouped,
        state: classify_join(&layout, &[]),
    };
    assert!(contradicted.establishes());
    assert_eq!(
        campaign_position_for(None, &contradicted),
        Err(NO_CONFIRMED_ROW_REFUSAL),
        "an unconfirmed row is refused before the table is consulted"
    );
    let contradicted = JoinAgreement {
        state: JoinCorroboration::Disagreed,
        ..contradicted
    };
    assert!(!contradicted.establishes());
    assert_eq!(
        campaign_position_for(Some(10), &contradicted),
        Err(CONTRADICTED_JOIN_REFUSAL),
        "a confirmed row must still select no position while the table contradicts the layout"
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
