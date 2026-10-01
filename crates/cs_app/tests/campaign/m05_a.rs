//! Acceptance stage M05-A: bind the fifth mission's original data and
//! branches (`missions/M05.md`, work order `M05-A`).
//!
//! The stage's minimum scenario is "Source-derived binding has no unresolved
//! critical dependencies", and
//! [`accept_m05_a_source_derived_binding_has_no_unresolved_critical_dependencies`]
//! asks production code exactly that: [`SourceContext::read`] fingerprints
//! `$CS_GAME_DIR` and reads its campaign layout and localized string table,
//! [`SourceContext::bind`] resolves the five critical dependencies of the M05
//! data-binding checklist, and nothing else in the record is allowed to read
//! as finished.
//!
//! M05 is the first work order whose declared discovery title the
//! installation does not spell the way the short-name rows do, so what this
//! stage pins is what is *different* at M05, on top of the join machinery
//! M02-A built and M03-A re-ran at a third position:
//!
//! * **The title is confirmed only through the long-name form.**
//!   `missions/README.md` declares `The Union Jack's Revenge`; the retail
//!   table carries that string *only* as the title part of the region-prefixed
//!   long name of campaign position 4, while the bare short name of the same
//!   position reads `Union Jack's Revenge`. Production code therefore confirms
//!   it through [`SourceContext::confirm_title`], and
//!   [`accept_m05_a_the_title_is_confirmed_only_through_the_long_name_form`]
//!   pins both halves: the confirmation, and the second spelling the record
//!   reports without reconciling it.
//! * **The verbatim form still wins where both carry the title.** Six of the
//!   declared titles sit in *both* display forms at the same campaign offset,
//!   and preferring the verbatim form must not quietly pick one identity over
//!   another.
//!   [`accept_m05_a_the_verbatim_form_wins_where_both_display_forms_carry_the_title`]
//!   measures that every such pair agrees on its campaign position first.
//! * **The world group is shared by three campaign missions.** `world/c1` is
//!   M02's, M04's *and* M05's, so the world identity alone does not single this
//!   mission out; the program archive inside it does.
//!   [`accept_m05_a_the_world_group_is_shared_and_the_program_singles_the_mission_out`]
//!   pins that without claiming which of the three is "the" c1 mission.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which has
//! no original data) skips them; they are run with `--include-ignored` by the
//! implementing and reviewing agents. Every assertion below is made against
//! facts the test re-reads from the installation or from committed records —
//! never against a constant that repeats the implementation. The synthetic
//! tests are left unignored so CI runs them, because the confirmation rule's
//! near-miss arms (a title that is merely *close* to a long name's tail) are
//! what makes the exact comparison meaningful, and the retail installation
//! produces only two of them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    AMBIGUOUS_TITLE_REFUSAL, BindingCategory, CONTRADICTED_JOIN_REFUSAL, CampaignBindings,
    CellState, CriticalDependency, DependencyState, GroupedTitleBlock, JoinAgreement,
    JoinCorroboration, MissionLabel, NO_CONFIRMED_ROW_REFUSAL, SHORT_ROW_BLOCK_REFUSAL,
    SourceBinding, SourceContext, TitleBlock, TitleConfirmation, TitleForm,
    UNCARRIED_TITLE_REFUSAL, campaign_position_for, classify_join, title_blocks, title_form,
};
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};

use crate::common::{label, load_inventory, repo_path};

/// The one work order this stage binds.
const WORK_ORDER: &str = "M05";

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M05-A needs the retail capability; run this suite with \
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

/// The declared discovery title of `M05`, read from the committed inventory
/// rather than repeated here.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == WORK_ORDER)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {WORK_ORDER} work order"))
}

/// The M05 binding derived from the installation, built once.
fn binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        context()
            .bind(
                MissionLabel::new(WORK_ORDER).expect("M05 is a valid label"),
                &discovery_title(),
            )
            .expect("M05 binds to the original data")
    })
}

/// The declared campaign, with M05 bound the way the record describes.
fn campaign() -> CampaignBindings {
    let mut campaign = CampaignBindings::from_inventory(&load_inventory())
        .expect("the declared inventory builds the campaign");
    campaign
        .bind(binding().to_mission_binding().expect("record is valid"))
        .expect("M05 is a declared mission");
    campaign
}

/// The comparable text of one retail row: a leading display tag such as
/// `[AB14I]` is a presentation instruction, not part of the title.
fn display_text(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('[') else {
        return text;
    };
    let Some(end) = rest.find(']') else {
        return text;
    };
    &text[end + 2..]
}

/// The display text of one retail row, or `None` when it decodes to nothing.
fn row_display(context: &SourceContext, id: u32) -> Option<&str> {
    let row = context.string_rows().iter().find(|row| row.id == id)?;
    row.text.as_deref().map(display_text)
}

/// The campaign-length row block `id` sits in.
fn block_containing(context: &SourceContext, id: u32) -> TitleBlock {
    context
        .campaign_title_blocks()
        .into_iter()
        .find(|block| block.contains(id))
        .unwrap_or_else(|| panic!("string id {id} sits in no campaign-length row block"))
}

// ---------------------------------------------------------------- retail ---

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_source_derived_binding_has_no_unresolved_critical_dependencies() {
    let binding = binding();
    binding
        .validate()
        .expect("the derived record is internally consistent");
    assert_eq!(
        binding.unresolved_critical(),
        Vec::new(),
        "the source-derived binding left a critical dependency unresolved: {:?}",
        binding.unresolved_critical()
    );
    assert_eq!(
        binding
            .dependencies
            .iter()
            .map(|dependency| dependency.id)
            .collect::<Vec<_>>(),
        CriticalDependency::ALL.to_vec(),
        "the record carries exactly the checklist's critical dependencies, in order"
    );

    // Each resolved dependency carries provenance, and the class says how it was
    // obtained: the installation hash, the localized title and the program
    // archive were observed through a tool run, while the join from the
    // work-order title to a retail mission directory is an inference.
    for dependency in &binding.dependencies {
        let DependencyState::Resolved { provenance } = &dependency.state else {
            panic!("{} is not resolved", dependency.id);
        };
        let expected = match dependency.id {
            CriticalDependency::MissionId | CriticalDependency::WorldGroupVariant => {
                ClaimStatus::Inferred
            }
            CriticalDependency::InstallHash
            | CriticalDependency::TitleString
            | CriticalDependency::ProgramSourceMap => ClaimStatus::ObservedTool,
        };
        assert_eq!(
            provenance.class, expected,
            "{} carries the wrong evidence class",
            dependency.id
        );
    }

    // The installation hash is re-measured here by the production discovery
    // path, independently of the binding.
    let found = cs_assets::install::discover(&game_dir()).expect("discovery reads the install");
    assert_eq!(
        binding.install_sha256,
        cs_assets::install::fingerprint(&found.manifest).to_hex(),
        "the recorded installation hash is not the one production discovery measures"
    );

    // The identities carry the kinds their roles mean.
    let mission = binding.catalog_id.as_ref().expect("mission id resolved");
    let world = binding.world_id.as_ref().expect("world id resolved");
    let program = binding.program_id.as_ref().expect("program id resolved");
    assert_eq!(mission.kind(), ContentKind::Mission);
    assert_eq!(world.kind(), ContentKind::World);
    assert_eq!(program.kind(), ContentKind::Script);

    // The campaign position is real: the directory layout it selects exists on
    // disk, with the reader archive the program id names.
    let campaign = context().campaign();
    assert_eq!(
        campaign.len(),
        24,
        "the retail campaign declares 24 missions"
    );
    let position = binding.campaign_position.expect("a position was resolved");
    assert_eq!(binding.campaign_size, campaign.len());
    let entry = &campaign[position];
    let program_path = game_dir().join(&entry.program_asset);
    assert!(
        program_path.is_file(),
        "the program archive {} the record cites does not exist",
        entry.program_asset
    );

    // M05 is the fifth mission: none of the positions M01-A, M02-A and M03-A
    // bound.
    assert_eq!(position, 4, "M05 is the fifth campaign position");
    for (earlier, other) in campaign.iter().enumerate().take(position) {
        assert_ne!(
            (entry.chapter, entry.mission_number),
            (other.chapter, other.mission_number),
            "M05 resolved campaign position {earlier}, which another work order already bound"
        );
    }

    // Every cited span points inside an existing asset, and the digest the
    // record carries is the digest of those bytes, recomputed here.
    assert!(
        !binding.source_spans.is_empty(),
        "no source span was recorded"
    );
    for span in &binding.source_spans {
        let path = game_dir().join(&span.asset_id);
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("cannot re-read {}: {error}", span.asset_id));
        assert_eq!(
            span.sha256,
            cs_assets::install::sha256(&bytes).to_hex(),
            "the recorded digest of {} is stale",
            span.asset_id
        );
        assert!(
            span.offset + span.length <= bytes.len() as u64,
            "the span of {} runs past the end of the asset",
            span.asset_id
        );
        assert!(span.length > 0, "an empty span cites nothing");
    }
    assert!(
        binding
            .source_spans
            .iter()
            .any(|span| span.asset_id == entry.program_asset),
        "the record cites no span of the program archive it names"
    );
    // The title span is the one the confirmed row itself reports, and it
    // points inside the localized string table rather than at the program.
    let confirmed_row = context()
        .string_rows()
        .iter()
        .find(|row| Some(row.id) == binding.localized_title_id)
        .expect("the confirmed title row is in the table");
    assert_eq!(
        binding.title_source.as_ref(),
        Some(&confirmed_row.span),
        "the recorded title span is not the confirmed row's own span"
    );
    assert_ne!(
        confirmed_row.span.container_path(),
        entry.program_asset,
        "the confirmed title row was read from the program archive"
    );

    // Source-derived is not verified: the unbound checklist entries are all
    // still there, and they say why.
    assert!(
        !binding.is_verified(),
        "the record claims verification while {} checklist entries are unknown",
        binding.unknowns.len()
    );
    assert!(
        !binding.unknowns.is_empty(),
        "the unbound checklist entries were dropped"
    );
    for needle in [
        "objective graph",
        "difficulty branches",
        "script/native coverage",
        "closure_sha256",
        "title spelling",
    ] {
        assert!(
            binding.unknowns.iter().any(|entry| entry.contains(needle)),
            "the record no longer says that {needle:?} is unknown"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_the_committed_record_is_what_the_installation_derives() {
    let committed = fs::read_to_string(repo_path("missions/bindings/M05.json"))
        .expect("missions/bindings/M05.json exists");
    let derived = binding().to_json();
    assert_eq!(
        committed, derived,
        "the committed binding record is not what production code derives from $CS_GAME_DIR"
    );

    // The record is the schema's shape, with the values that are still unknown
    // kept explicit instead of silently omitted.
    for key in [
        "\"schema_version\": 1",
        "\"work_order\": \"M05\"",
        "\"discovery_title\": ",
        "\"verified\": false",
        "\"install_sha256\": \"",
        "\"catalog_id\": \"mission/",
        "\"world_id\": \"world/",
        "\"program_id\": \"script/",
        "\"closure_sha256\": null",
        "\"source_spans\": [",
        "\"unknowns\": [",
        "\"evidence_ids\": []",
    ] {
        assert!(
            derived.contains(key),
            "the derived record is missing {key:?}:\n{derived}"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_the_title_is_confirmed_only_through_the_long_name_form() {
    // `M05-BIND`: "Confirm the title against local strings; do not key runtime
    // logic by this discovery label." This measures what that confirmation
    // actually is for M05, and the spelling difference that makes it a real
    // case rather than the M01–M04 one.
    //
    // The declared title is carried by *no* retail row verbatim. Exactly one
    // row carries it as the title part of a region-prefixed long name, and
    // that row's block is campaign-length, so the production confirmation and
    // the production join still select one campaign position. The bare short
    // name of the same position reads differently — it drops the leading
    // article — and the record reports that as measured instead of picking a
    // winner.
    let context = context();
    let binding = binding();
    let title = discovery_title();

    // Nothing in the table carries the title verbatim, and the installation
    // does carry the title's long-name form. This is the property that makes
    // M05 the first work order needing the second form, so it is measured
    // rather than assumed.
    let verbatim: Vec<u32> = context
        .string_rows()
        .iter()
        .filter_map(|row| {
            let display = display_text(row.text.as_deref()?);
            (title_form(display, &title) == Some(TitleForm::Verbatim)).then_some(row.id)
        })
        .collect();
    assert!(
        verbatim.is_empty(),
        "the declared title is now carried verbatim by rows {verbatim:?}, which would make the \
         long-name confirmation below unnecessary"
    );

    let confirmation = context.confirm_title(&title);
    let (row_id, form) = confirmation.confirmed().expect("the title is confirmed");
    assert_eq!(
        form,
        TitleForm::RegionPrefixedLongName,
        "M05 must be confirmed through the long-name form; the verbatim form carries nothing"
    );
    assert_eq!(
        binding.localized_title_id,
        Some(row_id),
        "the record did not confirm the title against the row the strings carry it in"
    );
    assert_eq!(confirmation.refusal(), None);

    // That row really does carry the declared title, exactly, after its
    // observed separator — the comparison the confirmation made, re-read here
    // from the installation.
    let display = row_display(context, row_id).expect("the confirmed row decodes");
    let (_, tail) = display
        .split_once(" - ")
        .expect("a long-name row carries the observed separator");
    assert_eq!(
        tail, title,
        "the confirmed row's title part is not the declared title"
    );
    assert_ne!(
        display, title,
        "the confirmed row carries the title verbatim, so this test no longer measures the \
         long-name form"
    );

    // The confirmed row sits in a campaign-length block, and it is the *only*
    // row in the whole table that carries the title in either form.
    let block = block_containing(context, row_id);
    assert_eq!(block.len(), context.campaign().len());
    let carrying: Vec<u32> = context
        .string_rows()
        .iter()
        .filter_map(|row| {
            let display = display_text(row.text.as_deref()?);
            title_form(display, &title).map(|_| row.id)
        })
        .collect();
    assert_eq!(
        carrying,
        vec![row_id],
        "more than one row carries the declared title, so the confirmation is not unique: \
         {carrying:?}"
    );

    // The join then selects exactly one campaign position, and it is M05's.
    let agreement = context.join_agreement();
    let position = campaign_position_for(Some(row_id), &agreement).expect("a position is derived");
    assert_eq!(binding.campaign_position, Some(position));
    assert_eq!(position, 4, "M05 is the fifth campaign position");

    // The second spelling is reported, not reconciled: the short-name row of
    // the same campaign position carries the title minus its leading article,
    // and the record says so with the row it read.
    let short_name = context
        .other_spellings(position, row_id)
        .into_iter()
        .find(|(id, _)| !display_of(context, *id).contains(" - "))
        .unwrap_or_else(|| {
            panic!("expected a bare short-name row for campaign position {position}")
        });
    assert_eq!(
        short_name.1, "Union Jack's Revenge",
        "the short-name spelling of this position changed; re-read the finding before updating \
         the expectation"
    );
    assert!(
        title.starts_with("The "),
        "this stage is about a title whose short-name spelling drops a leading word; the declared \
         title is now {title:?}"
    );
    assert!(
        title_form(&short_name.1, &title).is_none(),
        "the short name would confirm the title too, so M05 is not the long-name-only case"
    );

    let reported = binding
        .unknowns
        .iter()
        .find(|entry| entry.contains("title spelling"))
        .unwrap_or_else(|| {
            panic!(
                "the record does not report the spelling difference it read: {:?}",
                binding.unknowns
            )
        });
    assert!(
        reported.contains(&row_id.to_string()),
        "the reported difference does not name the confirmed row {row_id}: {reported}"
    );
    assert!(
        reported.contains(&short_name.0.to_string()),
        "the reported difference does not name the short-name row {}: {reported}",
        short_name.0
    );
    assert!(
        reported.contains("not reconciled"),
        "the reported difference must say it is not reconciled: {reported}"
    );
    assert!(
        !binding.is_verified(),
        "a record with an unreconciled spelling difference must not read as verified"
    );

    // The failure arms of the same confirmation. Neither of these is what M05
    // does, and both must stay unresolved identities:
    //
    // * a spelling no retail row carries, verbatim or as a long-name tail;
    // * a display text the table carries in two rows, which names no single
    //   row and therefore no single campaign position.
    //
    // Each arm is paired with the confirmation it must produce, so swapping
    // the two refusals cannot pass as "the reason names its own cause".
    for (miss, expected, refusal_text) in [
        (
            "The Union Jack's Revengee".to_owned(),
            TitleConfirmation::Uncarried,
            UNCARRIED_TITLE_REFUSAL,
        ),
        (
            duplicated_display_text(context),
            TitleConfirmation::Ambiguous,
            AMBIGUOUS_TITLE_REFUSAL,
        ),
    ] {
        assert_eq!(
            context.confirm_title(&miss),
            expected,
            "the confirmation for {miss:?} does not name its own cause"
        );
        let partial = context
            .bind(MissionLabel::new(WORK_ORDER).expect("valid label"), &miss)
            .expect("a title miss is a recorded unknown, not a failure");
        partial
            .validate()
            .expect("the partial record is internally consistent");
        let unresolved = partial.unresolved_critical();
        for id in [
            CriticalDependency::TitleString,
            CriticalDependency::MissionId,
            CriticalDependency::WorldGroupVariant,
            CriticalDependency::ProgramSourceMap,
        ] {
            assert!(
                unresolved.contains(&id),
                "{id} was resolved from the title {miss:?}, which names no single row: \
                 {unresolved:?}"
            );
        }
        assert!(
            !unresolved.contains(&CriticalDependency::InstallHash),
            "the installation hash does not depend on the title: {unresolved:?}"
        );
        let refusal = partial
            .dependency(CriticalDependency::TitleString)
            .and_then(|state| match state {
                DependencyState::Unresolved { reason, .. } => Some(reason.clone()),
                DependencyState::Resolved { .. } | DependencyState::Unsupported { .. } => None,
            })
            .expect("the title is unresolved, so it carries a reason");
        assert_eq!(
            refusal, refusal_text,
            "the record's reason is not the refusal this title must produce"
        );
        assert_eq!(partial.localized_title_id, None);
        assert_eq!(partial.catalog_id, None);
        assert_eq!(partial.campaign_position, None);
        assert!(
            partial.source_spans.is_empty(),
            "an unresolved binding must not cite a source span it never resolved"
        );
        assert!(
            !partial
                .unknowns
                .iter()
                .any(|entry| entry.contains("title spelling")),
            "an unconfirmed title must not report a spelling difference: {:?}",
            partial.unknowns
        );

        // And the campaign record built from it does not read as bound either.
        let mission_binding = partial
            .to_mission_binding()
            .expect("an incomplete record still builds a valid mission record");
        assert!(mission_binding.cells().any(|(category, state)| category
            == BindingCategory::MissionIdentity
            && state == CellState::Unknown));
    }
}

/// The display text of one retail row; panics when it decodes to nothing.
fn display_of(context: &SourceContext, id: u32) -> String {
    row_display(context, id)
        .unwrap_or_else(|| panic!("string row {id} does not decode"))
        .to_owned()
}

/// One display text the retail table carries in more than one row, read from
/// the installation rather than authored.
fn duplicated_display_text(context: &SourceContext) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for row in context.string_rows() {
        if let Some(text) = row.text.as_deref() {
            let display = display_text(text);
            if !display.is_empty() {
                *counts.entry(display).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .find(|(_, count)| *count > 1)
        .map(|(text, _)| text.to_owned())
        .expect("the retail table carries a display text more than once")
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_the_verbatim_form_wins_where_both_display_forms_carry_the_title() {
    // `SourceContext::confirm_title` prefers a verbatim row over a long name
    // naming the same title. That preference is only defensible if the two
    // forms never disagree about *which* mission they name — so this measures
    // it on the installation: every declared title carried by both forms
    // resolves to the same campaign offset in both, and every title carried
    // only by the long-name form is confirmed at the offset its row sits in.
    let context = context();
    let agreement = context.join_agreement();
    let blocks = context.campaign_title_blocks();

    let offsets = |id: u32| -> Option<usize> {
        blocks
            .iter()
            .find(|block| block.contains(id))
            .map(|block| (id - block.first_id()) as usize)
    };
    let rows_carrying = |title: &str| -> (Vec<u32>, Vec<u32>) {
        let mut verbatim = Vec::new();
        let mut long = Vec::new();
        for row in context.string_rows() {
            let Some(display) = row.text.as_deref().map(display_text) else {
                continue;
            };
            match title_form(display, title) {
                Some(TitleForm::Verbatim) => verbatim.push(row.id),
                Some(TitleForm::RegionPrefixedLongName) => long.push(row.id),
                None => {}
            }
        }
        (verbatim, long)
    };

    let mut both_forms = 0usize;
    let mut long_only = 0usize;
    for (work_order, title) in load_inventory().iter() {
        let (verbatim, long) = rows_carrying(title);
        match (verbatim.as_slice(), long.as_slice()) {
            ([only], [other]) => {
                both_forms += 1;
                let verbatim_offset = offsets(*only);
                let long_offset = offsets(*other);
                assert_eq!(
                    verbatim_offset, long_offset,
                    "{work_order}'s two display forms name different campaign positions \
                     ({verbatim_offset:?} vs {long_offset:?}), so preferring the verbatim row \
                     would pick one identity over the other"
                );
                assert_eq!(
                    context.confirm_title(title).confirmed(),
                    Some((*only, TitleForm::Verbatim)),
                    "{work_order} is carried verbatim, so the verbatim row must confirm it"
                );
                assert_eq!(
                    campaign_position_for(Some(*only), &agreement).ok(),
                    verbatim_offset,
                    "{work_order}'s verbatim row does not select the position its block offset \
                     names"
                );
            }
            ([], [only]) => {
                long_only += 1;
                assert_eq!(
                    context.confirm_title(title).confirmed(),
                    Some((*only, TitleForm::RegionPrefixedLongName)),
                    "{work_order} is carried only by a long name, so that row must confirm it"
                );
                assert_eq!(
                    campaign_position_for(Some(*only), &agreement).ok(),
                    offsets(*only),
                    "{work_order}'s long-name row does not select the position its block offset \
                     names"
                );
            }
            ([only], []) => {
                assert_eq!(
                    context.confirm_title(title).confirmed(),
                    Some((*only, TitleForm::Verbatim)),
                    "{work_order} is carried verbatim and must confirm on that row alone"
                );
            }
            // Nothing carries it: the declared title and the installation
            // disagree in a way this stage does not resolve. That state stays
            // unresolved on purpose and is another work order's stage to record.
            ([], []) => assert_eq!(
                context.confirm_title(title),
                TitleConfirmation::Uncarried,
                "{work_order}'s title is carried in neither form but the confirmation did not say \
                 so"
            ),
            _ => panic!(
                "{work_order}'s title is carried by several rows ({verbatim:?}, {long:?}), so no \
                 single row names it"
            ),
        }
    }
    // The installation really does exercise both branches, so this test is not
    // vacuously true on one form.
    assert!(
        both_forms > 0 && long_only > 0,
        "expected both forms to carry titles: {both_forms} verbatim+long, {long_only} long-only"
    );
    assert_eq!(
        long_only, 1,
        "M05 was the only work order needing the long-name form at this stage; re-read the \
         finding before changing the expectation"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_the_world_group_is_shared_and_the_program_singles_the_mission_out() {
    // M02-A already pinned that a world group can hold several campaign
    // missions. M05 shares that world group with two other missions, and this
    // time it is the *bound* record that must not read as if the world id
    // identified the mission: `world/c1` is M02's, M04's and M05's. What does
    // single M05 out is the program archive inside the shared group, and the
    // record's three identities are checked against each other here rather
    // than taken at their word.
    let context = context();
    let binding = binding();
    let position = binding.campaign_position.expect("a position was resolved");
    let entry = &context.campaign()[position];
    assert_eq!(entry.world_group, "c1", "M05 lives in the C1 world group");

    let members: Vec<_> = context
        .campaign()
        .iter()
        .filter(|other| other.world_group == entry.world_group)
        .collect();
    assert_eq!(
        members.len(),
        3,
        "the C1 world group should carry three campaign missions: {members:?}"
    );
    assert_eq!(members[0].mission_number, 2);
    assert_eq!(members[1].mission_number, 4);
    assert_eq!(members[2].mission_number, 5);

    // The world id is the layout's group, and it is shared — so it must not
    // equal the world of the neighbouring missions' *programs* either.
    let world = binding
        .world_id
        .as_ref()
        .expect("world id resolved")
        .as_str();
    assert!(
        world.ends_with(&entry.world_group),
        "the bound world id {world} is not the world group the layout declares"
    );

    // The program id is what names the mission inside the shared group: the
    // three missions of the group carry three distinct program identities,
    // three distinct archives on disk and three distinct digests.
    let programs: BTreeSet<String> = members
        .iter()
        .map(|member| member.program_asset.clone())
        .collect();
    assert_eq!(
        programs.len(),
        members.len(),
        "two missions of the shared world group cite one program archive: {programs:?}"
    );
    let digests: BTreeSet<String> = members
        .iter()
        .map(|member| {
            let path = game_dir().join(&member.program_asset);
            let bytes = fs::read(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", member.program_asset));
            assert!(
                !bytes.is_empty(),
                "the program archive {} is empty",
                member.program_asset
            );
            cs_assets::install::sha256(&bytes).to_hex()
        })
        .collect();
    assert_eq!(
        digests.len(),
        members.len(),
        "two program archives of the shared world group are byte-identical, so their digests cannot \
         tell the missions apart"
    );
    assert!(
        binding
            .source_spans
            .iter()
            .any(|span| span.asset_id == entry.program_asset),
        "the record cites no span of its own program archive"
    );

    // The world-level archives sit beside the mission directory, one level
    // above the program the record cites — evidence that the mission directory
    // is inside the world group the world id names.
    let program = game_dir().join(&entry.program_asset);
    let mission_dir = program
        .parent()
        .expect("program sits in a mission directory");
    let group_dir = mission_dir.parent().expect("mission sits in a world group");
    assert!(group_dir.join("gamez.zbd").is_file());
    assert!(group_dir.join("zrdr.zbd").is_file());
    assert!(
        binding.world_id.as_ref().map(|id| id.as_str())
            == Some(&format!("world/{}", entry.world_group)),
        "the world id does not name the directory the program archive is in"
    );

    // Subdirectories of the group that are not campaign missions exist (C1
    // carries the non-campaign `IA1`/`MP*` directories) and stay outside the
    // binding: their role is not established, so nothing is claimed about them
    // and nothing is cited from inside them.
    let campaign_dirs: BTreeSet<String> = members
        .iter()
        .map(|member| format!("M{:02}", member.mission_number))
        .collect();
    let extra: Vec<String> = fs::read_dir(group_dir)
        .expect("the world group directory reads")
        .filter_map(Result::ok)
        .filter(|item| item.path().is_dir())
        .map(|item| item.file_name().to_string_lossy().into_owned())
        .filter(|name| !campaign_dirs.contains(&name.to_ascii_uppercase()))
        .collect();
    assert!(
        !extra.is_empty(),
        "expected non-campaign subdirectories beside the missions in the C1 group"
    );
    for name in &extra {
        let sibling = group_dir.join(name);
        assert!(
            !binding
                .source_spans
                .iter()
                .any(|span| game_dir().join(&span.asset_id).starts_with(&sibling)),
            "the record cites a span inside the non-campaign directory {name}"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_a_the_campaign_keeps_everything_else_unresolved_and_unready() {
    let campaign = campaign();
    let coverage = campaign.coverage();
    assert_eq!(
        coverage.total_missions, 24,
        "the denominator is the campaign"
    );
    assert_eq!(coverage.declared_missions, 24);
    assert_eq!(coverage.cells, 24 * 7);
    assert_eq!(
        coverage.complete_cells, 1,
        "only the bound identity cell may be complete"
    );
    assert_eq!(
        coverage.unknown_cells,
        24 * 7 - 1,
        "every other cell stays explicitly unknown"
    );
    assert_eq!(coverage.missing_cells, 0);
    assert_eq!(coverage.subsystem_rows, 24 * 23);
    assert_eq!(
        coverage.subsystem_unresolved,
        24 * 23,
        "no subsystem is implemented yet, so no subsystem row may read as resolved"
    );
    assert_eq!(coverage.progression_unknown, 24);
    assert!(
        !coverage.is_ready(),
        "a campaign with one bound identity cell must not read as ready"
    );

    let m05 = campaign.get(&label(WORK_ORDER)).expect("M05 is recorded");
    assert!(
        !m05.is_placeholder(),
        "M05 was bound, not left a placeholder"
    );
    for (category, state) in m05.cells() {
        let expected = if category == BindingCategory::MissionIdentity {
            CellState::Complete
        } else {
            CellState::Unknown
        };
        assert_eq!(state, expected, "category {category} has the wrong state");
    }
    assert!(
        m05.dependencies
            .iter()
            .all(|row| matches!(row.state, DependencyState::Unresolved { .. }))
    );

    let closure = campaign
        .closure(&label(WORK_ORDER), None)
        .expect("M05's closure computes without a catalog");
    assert_eq!(closure.reached, vec![label(WORK_ORDER)]);
    assert_eq!(closure.cell_count(), 7);
    assert_eq!(closure.complete_cells(), 1);
    assert_eq!(closure.unresolved_subsystems, 23);
    assert_eq!(closure.unknown_progression, 1);
    assert!(!closure.is_complete());
}

// -------------------------------------------------------------- synthetic ---

#[test]
fn accept_m05_a_only_an_exact_title_or_an_exact_long_name_tail_confirms() {
    // The rule is a byte-for-byte comparison in both display forms, so the arms
    // that matter are the near misses: a tail that *almost* equals the title
    // must confirm nothing at all. The retail installation produces only the
    // two positive arms, which is why this is here.
    let title = "The Union Jack's Revenge";

    // Verbatim: the whole display text is the title.
    assert_eq!(
        title_form(title, title),
        Some(TitleForm::Verbatim),
        "a row carrying the title verbatim must confirm it"
    );
    assert_eq!(
        title_form("[AB14I]The Union Jack's Revenge", title),
        None,
        "`title_form` compares *display* text: the font tag is stripped by the reader that calls \
         it, so a tagged string is not what this function ever sees"
    );

    // Region-prefixed long name: the title is the part after the observed
    // separator, exactly.
    assert_eq!(
        title_form("Hawaii - The Union Jack's Revenge", title),
        Some(TitleForm::RegionPrefixedLongName)
    );
    assert_eq!(
        title_form("Northwest - Mercy's Errand", "Mercy's Errand"),
        Some(TitleForm::RegionPrefixedLongName),
        "a title of several words must confirm after the separator"
    );

    // Near misses: none of these may confirm in either form.
    for (display, why) in [
        (
            "Hawaii - Union Jack's Revenge",
            "the tail is missing its leading article",
        ),
        (
            "Hawaii - The Union Jack's Revenge.",
            "the tail has a trailing period",
        ),
        (
            "Hawaii - the union jack's revenge",
            "the tail differs in case",
        ),
        (
            "Hawaii - The Union Jack's",
            "the tail is a prefix of the title",
        ),
        (
            "The Union Jack's Revenge - Hawaii",
            "the title is the prefix, not the tail",
        ),
        (
            "Hawaii The Union Jack's Revenge",
            "the observed separator is missing",
        ),
        (" - The Union Jack's Revenge", "the region prefix is empty"),
        ("Hawaii - ", "the title part is empty"),
        ("Hawaii", "there is no separator at all"),
        ("", "an empty row carries nothing"),
    ] {
        assert_eq!(
            title_form(display, title),
            None,
            "{display:?} must not confirm the title: {why}"
        );
    }

    // A title that itself contains the separator still works, because the
    // verbatim comparison runs first and is not confused by a separator.
    assert_eq!(
        title_form("A - B", "A - B"),
        Some(TitleForm::Verbatim),
        "a title containing the separator must confirm verbatim"
    );
    assert_eq!(
        title_form("Region - A - B", "A - B"),
        Some(TitleForm::RegionPrefixedLongName),
        "a title containing the separator must also confirm as a tail"
    );
}

#[test]
fn accept_m05_a_a_contradicted_corroboration_establishes_no_position() {
    // M05-A's binding inherits M02-A's join guard, and the guard is only real
    // if a disagreement actually stops the join — which no retail installation
    // produces. So the contradiction arm is proved here on authored values, and
    // the record's own title-spelling reporting is proved against a binding
    // that carries every critical dependency resolved and an unknown entry.
    let layout = vec![5usize, 5, 5, 5, 4];
    let block = TitleBlock::new(1, 24).expect("a forward run is a block");
    let grouped = |groups: Vec<usize>| {
        if groups.is_empty() {
            Vec::new()
        } else {
            vec![GroupedTitleBlock { block, groups }]
        }
    };

    assert_eq!(
        TitleBlock::new(5, 4),
        None,
        "a run cannot end before it starts"
    );

    assert_eq!(
        classify_join(&layout, &grouped(layout.clone())),
        JoinCorroboration::Agreed
    );
    assert_eq!(
        classify_join(&layout, &grouped(vec![24])),
        JoinCorroboration::Disagreed,
        "the same rows grouped differently contradict the layout"
    );
    assert_eq!(
        classify_join(&layout, &grouped(Vec::new())),
        JoinCorroboration::Unavailable,
        "nothing to check against is not a contradiction"
    );

    // A confirmed row selects a position only when the table agrees; each
    // refusal names its own cause.
    let agreed = JoinAgreement {
        layout_chapters: layout.clone(),
        blocks: vec![TitleBlock::new(100, 123).expect("block")],
        grouped: grouped(layout.clone()),
        state: JoinCorroboration::Agreed,
    };
    assert_eq!(campaign_position_for(Some(113), &agreed), Ok(13));
    assert_eq!(
        campaign_position_for(None, &agreed),
        Err(NO_CONFIRMED_ROW_REFUSAL),
        "an unconfirmed row selects no position"
    );
    assert_eq!(
        campaign_position_for(Some(124), &agreed),
        Err(SHORT_ROW_BLOCK_REFUSAL),
        "a row outside every campaign-length block selects no position"
    );

    let contradicted = JoinAgreement {
        state: JoinCorroboration::Disagreed,
        ..agreed.clone()
    };
    assert_eq!(
        campaign_position_for(Some(113), &contradicted),
        Err(CONTRADICTED_JOIN_REFUSAL),
        "a contradicted table must select no position even for a confirmed row"
    );
    assert!(
        CONTRADICTED_JOIN_REFUSAL.contains("contradicts"),
        "the refusal must name the contradiction: {CONTRADICTED_JOIN_REFUSAL:?}"
    );

    // An installation with nothing to check against keeps the inference.
    let unchallenged = JoinAgreement {
        grouped: Vec::new(),
        state: JoinCorroboration::Unavailable,
        ..agreed
    };
    assert!(unchallenged.establishes());
    assert_eq!(campaign_position_for(Some(113), &unchallenged), Ok(13));

    // A row block that is not the campaign's length names no position, so the
    // block rule is only a length comparison: a four-row run is one run, and a
    // gap ends a run rather than widening it.
    let short: BTreeSet<u32> = (10..14).collect();
    let blocks = title_blocks(&short);
    assert_eq!(
        blocks.len(),
        1,
        "four consecutive rows are one run: {blocks:?}"
    );
    assert_eq!(blocks[0].len(), 4);
    assert_ne!(blocks[0].len(), 24, "four rows are not the campaign");
    let split: BTreeSet<u32> = (1..=3).chain(5..=7).collect();
    let blocks = title_blocks(&split);
    assert_eq!(blocks.len(), 2, "a gap must end a run: {blocks:?}");
    assert_eq!(blocks[1].first_id(), 5);

    // The confirmation's own refusals are distinct constants, so a record's
    // reason names its own cause.
    assert!(UNCARRIED_TITLE_REFUSAL.contains("no localized string carries"));
    assert!(AMBIGUOUS_TITLE_REFUSAL.contains("several localized strings carry"));
    assert_ne!(UNCARRIED_TITLE_REFUSAL, AMBIGUOUS_TITLE_REFUSAL);

    // A record whose critical dependencies are all resolved is still not
    // verified while its checklist entries — here including the reported
    // spelling difference — are unknown: the two states stay apart.
    let authored = authored_binding();
    authored
        .validate()
        .expect("the authored record is internally consistent");
    assert_eq!(authored.unresolved_critical(), Vec::new());
    assert!(
        !authored.is_verified(),
        "a record with {} unbound checklist entries must not read as verified",
        authored.unknowns.len()
    );
    for needle in ["title spelling", "closure_sha256", "objective graph"] {
        assert!(
            authored.unknowns.iter().any(|entry| entry.contains(needle)),
            "the authored record must still say that {needle:?} is unknown"
        );
    }
}

/// A source binding with every critical dependency resolved but the checklist
/// entries a source binding does not bind still unknown, built here from
/// authored values. It proves only the predicates of
/// [`SourceBinding::unresolved_critical`] and [`SourceBinding::is_verified`].
fn authored_binding() -> SourceBinding {
    let claim = |suffix: &str| ClaimId::new(&format!("m05.a.synthetic.{suffix}")).expect("claim");
    let provenance = |suffix: &str| {
        Provenance::new(claim(suffix), ClaimStatus::Designed, None)
            .expect("designed provenance always validates")
    };
    SourceBinding {
        label: MissionLabel::new(WORK_ORDER).expect("M05 is a valid label"),
        discovery_title: "The Union Jack's Revenge".to_owned(),
        install_sha256: "0".repeat(64),
        campaign_position: Some(4),
        campaign_size: 24,
        catalog_id: Some(ContentId::from_source(ContentKind::Mission, "ch1-m05").expect("id")),
        world_id: Some(ContentId::from_source(ContentKind::World, "c1").expect("id")),
        program_id: Some(ContentId::from_source(ContentKind::Script, "c1-m05-zrdr").expect("id")),
        localized_title_id: Some(1),
        localized_title_language: Some(1033),
        dependencies: CriticalDependency::ALL
            .iter()
            .map(|id| cs_content::campaign_bindings::SourceDependency {
                id: *id,
                state: DependencyState::resolved(provenance(id.label())),
            })
            .collect(),
        source_spans: Vec::new(),
        identity_source: None,
        title_source: None,
        closure_sha256: None,
        evidence_ids: Vec::new(),
        unknowns: vec![
            "title spelling: the declared title is carried only by the region-prefixed long-name \
             row 1, while the same campaign position is spelled differently elsewhere in the same \
             installation (row 25 reads \"Union Jack's Revenge\") — the difference is recorded, not \
             reconciled"
                .to_owned(),
            "objective graph: not bound from original data at this stage".to_owned(),
            "closure_sha256: the mission dependency closure hash is not measured".to_owned(),
        ],
    }
}
