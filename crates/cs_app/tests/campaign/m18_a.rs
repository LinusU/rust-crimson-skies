//! Acceptance stage M18-A: bind the eighteenth mission's original data and
//! branches (`missions/M18.md`, work order `M18-A`).
//!
//! The stage's minimum scenario is "Source-derived binding has no unresolved
//! critical dependencies", and
//! [`accept_m18_a_source_derived_binding_has_no_unresolved_critical_dependencies`]
//! asks production code exactly that: [`SourceContext::read`] fingerprints
//! `$CS_GAME_DIR` and reads its campaign layout and localized string table,
//! [`SourceContext::bind`] resolves the five critical dependencies of the M18
//! data-binding checklist, and nothing else in the record is allowed to read
//! as finished.
//!
//! M18 is the eighteenth campaign position and the join machinery is M02-A's; what
//! this stage pins is what is *different* at M18:
//!
//! * **The position is the middle mission of chapter 4.** It is the third of
//!   the chapter's five, so neither the layout's chapter boundary nor the
//!   localized long names' region boundary falls on it or next to it.
//!   [`accept_m18_a_the_position_is_interior_to_the_fourth_chapter_and_its_region_group`]
//!   measures that campaign position 17 lies strictly inside the layout's
//!   chapter and the long names' region group, and that both structures give
//!   the same boundaries.
//! * **The world group is the whole chapter, and the mission number is not an
//!   identity either.** `world/c4` carries all five missions of chapter 4, and
//!   the number `3` names a mission directory in *every* chapter
//!   (`ZBD/C1B/M03` … `ZBD/C5/M03`), with five different program archives.
//!   [`accept_m18_a_the_world_group_is_the_whole_chapter_and_neither_it_nor_the_mission_number_identifies_the_mission`]
//!   pins both axes: only the campaign position singles M18 out.
//! * **A confirmed row can still select no position — and M18's own region
//!   prefix is such a row.** The installation carries `Rocky Mountains` as a
//!   standalone localized string, so the title-shaped string a consumer would
//!   most plausibly reach for *confirms* and then resolves nothing: the row
//!   sits outside every campaign-length block. No earlier binding stage
//!   exercised that arm on real data.
//!   [`accept_m18_a_m18s_own_region_prefix_is_a_confirmed_row_that_selects_no_position`]
//!   measures it, and
//!   [`accept_m18_a_a_confirmed_row_outside_every_campaign_block_selects_no_position`]
//!   proves the same production predicate on authored values, so CI runs it.
//! * **The comparison is exact, and the installation offers no near miss that
//!   would prove it.** A fuzzy match would let an unrelated row confirm a
//!   mission's title.
//!   [`accept_m18_a_a_near_miss_title_is_never_confirmed`] proves
//!   [`title_form`]'s exactness arm by arm on authored values.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which
//! has no original data) skips them; they are run with `--include-ignored`
//! by the implementing and reviewing agents. Every assertion below is made
//! against facts the test re-reads from the installation or from committed
//! records — never against a constant that repeats the implementation. The
//! four synthetic tests are left unignored so CI runs them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    BindingCategory, CONTRADICTED_JOIN_REFUSAL, CampaignBindings, CampaignMission, CellState,
    CriticalDependency, DependencyState, GroupedTitleBlock, JoinAgreement, JoinCorroboration,
    MissionLabel, NO_CONFIRMED_ROW_REFUSAL, SHORT_ROW_BLOCK_REFUSAL, SourceBinding, SourceContext,
    TitleBlock, TitleForm, campaign_layout, campaign_position_for, classify_join, title_blocks,
    title_form,
};
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};

use crate::common::{label, load_inventory, repo_path};

/// The one work order this stage binds.
const WORK_ORDER: &str = "M18";

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M18-A needs the retail capability; run this suite with \
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

/// The declared discovery title of `M18`, read from the committed inventory
/// rather than repeated here.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == WORK_ORDER)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {WORK_ORDER} work order"))
}

/// The M18 binding derived from the installation, built once.
fn binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        context()
            .bind(
                MissionLabel::new(WORK_ORDER).expect("M18 is a valid label"),
                &discovery_title(),
            )
            .expect("M18 binds to the original data")
    })
}

/// The declared campaign, with M18 bound the way the record describes.
fn campaign() -> CampaignBindings {
    let mut campaign = CampaignBindings::from_inventory(&load_inventory())
        .expect("the declared inventory builds the campaign");
    campaign
        .bind(binding().to_mission_binding().expect("record is valid"))
        .expect("M18 is a declared mission");
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

/// The display text of one localized row of the installation, by id.
fn row_display(context: &SourceContext, id: u32) -> String {
    let row = context
        .string_rows()
        .iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("the localized table has no row {id}"));
    display_text(
        row.text
            .as_deref()
            .unwrap_or_else(|| panic!("row {id} does not decode")),
    )
    .to_owned()
}

/// The campaign-length block `id` sits in.
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
fn accept_m18_a_source_derived_binding_has_no_unresolved_critical_dependencies() {
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

    // Each resolved dependency carries provenance, and the class says how it
    // was obtained: the installation hash, the localized title and the program
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

    // M18 is the eighteenth mission: position 17. It is not any mission an
    // earlier binding stage bound. Positions 0, 2, 10 and 15 are the missions
    // M01-A, M03-A, M12-A and M16-A bound; the first three are in earlier
    // chapters and M16 shares M18's chapter 4, so the check is on the whole
    // identity, not on the chapter alone.
    assert_eq!(position, 17, "M18 is the eighteenth campaign position");
    for (earlier, work_order) in [(0usize, "M01"), (2, "M03"), (10, "M12"), (15, "M16")] {
        assert!(
            earlier < position,
            "the campaign is not ordered by position ({earlier} is not before {position})"
        );
        assert_ne!(
            (entry.chapter, entry.mission_number),
            (campaign[earlier].chapter, campaign[earlier].mission_number),
            "M18 resolved the mission at campaign position {earlier}, which {work_order}-A bound"
        );
    }
    // And it is genuinely later in the campaign than every one of them.
    assert_eq!(
        entry.chapter, campaign[15].chapter,
        "M16 and M18 are expected to share chapter 4: {:?} vs {:?}",
        entry, campaign[15]
    );
    assert!(
        entry.mission_number > campaign[15].mission_number,
        "M18 is not a later mission of chapter 4 than M16: {entry:?}"
    );

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
    ] {
        assert!(
            binding.unknowns.iter().any(|entry| entry.contains(needle)),
            "the record no longer says that {needle:?} is unknown"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_a_the_committed_record_is_what_the_installation_derives() {
    let committed = fs::read_to_string(repo_path("missions/bindings/M18.json"))
        .expect("missions/bindings/M18.json exists");
    let derived = binding().to_json();
    assert_eq!(
        committed, derived,
        "the committed binding record is not what production code derives from $CS_GAME_DIR"
    );

    // The record is the schema's shape, with the values that are still unknown
    // kept explicit instead of silently omitted.
    for key in [
        "\"schema_version\": 1",
        "\"work_order\": \"M18\"",
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
fn accept_m18_a_the_original_name_is_confirmed_against_the_local_strings() {
    // `M18-BIND`: "Find the corresponding original catalog/program/world
    // identities. Confirm the title against local strings; do not key runtime
    // logic by this discovery label." This measures what that confirmation
    // actually is: the declared title equals the display text of exactly one
    // retail row, and the *other* display form of the same campaign position
    // is the same title behind an observed region prefix.
    let context = context();
    let binding = binding();
    let title = discovery_title();

    let matching: Vec<u32> = context
        .string_rows()
        .iter()
        .filter_map(|row| {
            let text = row.text.as_deref()?;
            display_text(text).eq(&title).then_some(row.id)
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "the declared title must be carried by exactly one retail row, found {matching:?}"
    );
    assert_eq!(
        binding.localized_title_id,
        Some(matching[0]),
        "the record did not confirm the title against the row the strings carry it in"
    );

    // The block this row sits in is exactly the campaign's length.
    let block = block_containing(context, matching[0]);
    assert_eq!(block.len(), context.campaign().len());

    // The installation gives this mission a second display form, a
    // region-prefixed long name. Unlike a stage where the two forms *disagree*
    // about the mission's name, M18's two forms carry the same title text, so
    // the record needs no spelling-disagreement note — and it must not invent
    // one.
    let position = binding.campaign_position.expect("a position was resolved");
    let other = context.other_spellings(position, matching[0]);
    assert_eq!(
        other.len(),
        1,
        "expected exactly one second spelling at campaign position {position}, found {other:?}"
    );
    let (other_id, other_text) = &other[0];
    assert_ne!(
        other_text, &title,
        "the installation gives the same mission two identical spellings, so the two localized row \
         blocks are not independent"
    );
    assert!(
        other_text.contains(" - "),
        "the second spelling {other_text:?} at row {other_id} is not a region-prefixed long name"
    );
    let prefix = other_text
        .strip_suffix(&title)
        .and_then(|rest| rest.strip_suffix(" - "))
        .unwrap_or_else(|| {
            panic!("the second spelling {other_text:?} does not end with the declared title")
        });
    assert!(
        !prefix.is_empty(),
        "the long name {other_text:?} has an empty region prefix"
    );
    assert!(
        !binding
            .unknowns
            .iter()
            .any(|entry| entry.contains("title spelling")),
        "the record claims a title-spelling disagreement the installation does not have: {:?}",
        binding.unknowns
    );

    // A title the strings do not carry is never confirmed. This is the failure
    // case of the confirmation itself: first a spelling no retail row carries,
    // then the region prefix paired with a mission the strings do spell (the
    // exact long name a consumer would compose), then a real display text the
    // retail table carries *twice* — which the binding must refuse as firmly,
    // because a title matching several rows names no single row and therefore
    // no single campaign position.
    for miss in [
        "The Red Menacee".to_owned(),
        format!("{prefix} - The Red Menacee"),
        duplicated_display_text(context),
    ] {
        let partial = context
            .bind(MissionLabel::new(WORK_ORDER).expect("valid label"), &miss)
            .expect("a title miss is a recorded unknown, not a failure");
        partial
            .validate()
            .expect("the partial record is internally consistent");
        let unresolved = partial.unresolved_critical();
        assert!(
            unresolved.contains(&CriticalDependency::TitleString),
            "a title the local strings do not carry exactly once was reported as resolved \
             ({miss:?}): {unresolved:?}"
        );
        for id in [
            CriticalDependency::MissionId,
            CriticalDependency::WorldGroupVariant,
            CriticalDependency::ProgramSourceMap,
        ] {
            assert!(
                unresolved.contains(&id),
                "{id} was resolved from the title {miss:?}, which names no row: {unresolved:?}"
            );
        }
        assert!(
            !unresolved.contains(&CriticalDependency::InstallHash),
            "the installation hash does not depend on the title: {unresolved:?}"
        );
        assert_eq!(partial.localized_title_id, None);
        assert_eq!(partial.catalog_id, None);
        assert!(
            partial.source_spans.is_empty(),
            "an unresolved binding must not cite a source span it never resolved"
        );
        assert!(!partial.is_verified());

        // And the campaign record built from it does not read as bound either.
        let mission_binding = partial
            .to_mission_binding()
            .expect("an incomplete record still builds a valid mission record");
        assert!(mission_binding.cells().any(|(category, state)| category
            == BindingCategory::MissionIdentity
            && state == CellState::Unknown));
    }
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
fn accept_m18_a_the_join_is_corroborated_by_the_long_name_rows() {
    // A campaign-length run of localized rows is weak evidence for the join:
    // any 24 unrelated strings would form one. The installation offers a
    // second structure — the region-prefixed long mission names — whose row
    // *grouping* is checkable without claiming what any region name means.
    // This measures that agreement on the retail installation, and checks that
    // M18's own row block is the one that carries no region prefix, so the
    // corroboration genuinely comes from elsewhere.
    let context = context();
    let agreement = context.join_agreement();

    assert_eq!(
        agreement.state,
        JoinCorroboration::Agreed,
        "the localized row blocks and the campaign directory layout disagree about the campaign"
    );
    assert!(
        agreement.establishes(),
        "an agreed corroboration must establish the join"
    );
    assert!(
        agreement.blocks.len() >= 2,
        "the installation offers only {} campaign-length row block(s), so the join rests on one \
         structure alone",
        agreement.blocks.len()
    );
    // Every block listed is exactly as long as the campaign. The corroborated
    // block is *also* the one carrying region prefixes, so without this the
    // test would still pass on a table that listed every row run it finds.
    let campaign_len = context.campaign().len();
    assert!(
        agreement
            .blocks
            .iter()
            .all(|block| block.len() == campaign_len),
        "a listed row block is not as long as the campaign: {:?}",
        agreement.blocks
    );
    assert!(
        !agreement.grouped.is_empty(),
        "no campaign-length row block carries a region prefix, so nothing corroborates the join"
    );
    let sizes = agreement.layout_chapters.iter().copied().sum::<usize>();
    assert_eq!(
        sizes,
        context.campaign().len(),
        "the chapter sizes do not add up to the campaign"
    );
    assert_eq!(
        agreement.grouped[0].groups, agreement.layout_chapters,
        "the long-name rows do not group into the layout's chapter sizes"
    );
    for entry in &agreement.grouped {
        assert_eq!(
            entry.groups.iter().copied().sum::<usize>(),
            entry.block.len(),
            "the region groups of {} do not cover every row",
            entry.block
        );
    }

    // M18's own title block is one of the campaign-length blocks and is *not*
    // the grouped one, so the corroboration cannot be M18 agreeing with
    // itself.
    let title_id = binding()
        .localized_title_id
        .expect("M18's title string resolved");
    let own = block_containing(context, title_id);
    assert!(
        !agreement.grouped.iter().any(|entry| entry.block == own),
        "M18's own row block {} carries region prefixes, so it corroborates itself",
        own
    );

    // Every row of the corroborating block selects the same campaign position
    // its index names, through the production join, so the agreement is about
    // positions and not about a length.
    for entry in &agreement.grouped {
        for offset in 0..entry.block.len() {
            let id = entry.block.first_id() + u32::try_from(offset).expect("row fits in u32");
            let title = row_display(context, id);
            let bound = context
                .bind(MissionLabel::new(WORK_ORDER).expect("valid label"), &title)
                .expect("a retail long name binds without I/O failure");
            bound
                .validate()
                .expect("the derived record is internally consistent");
            assert_eq!(
                bound.unresolved_critical(),
                Vec::new(),
                "the long name at row {id} left a critical dependency unresolved"
            );
            assert_eq!(
                bound.campaign_position,
                Some(offset),
                "the long name at row {id} did not select campaign position {offset}"
            );
            assert_eq!(
                bound.catalog_id.as_ref().map(|id| id.as_str()),
                context
                    .campaign()
                    .get(offset)
                    .map(|entry| format!(
                        "mission/ch{}-m{:02}",
                        entry.chapter, entry.mission_number
                    ))
                    .as_deref(),
                "the long name at row {id} selected a different mission identity"
            );
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_a_the_position_is_interior_to_the_fourth_chapter_and_its_region_group() {
    // M18 is a genuine interior: with the campaign ordered by `(chapter,
    // mission number)`, position 17 is the third mission of chapter 4, and the
    // localized long names' *fourth* region group spans the same five rows.
    // Both structures put the boundary at row 15, two rows above M18, so a
    // join that divided either group one row off would still have to agree on
    // where the group starts and ends.
    let context = context();
    let binding = binding();
    let campaign = context.campaign();
    let position = binding.campaign_position.expect("position resolved");
    let entry = &campaign[position];

    assert_eq!(position, 17, "M18 is the eighteenth campaign position");
    assert_eq!(
        entry.mission_number, 3,
        "M18 is not its chapter's third mission: {entry:?}"
    );
    assert_eq!(
        entry.chapter, 4,
        "M18 is not in the fourth chapter: {entry:?}"
    );
    let first = campaign
        .iter()
        .position(|other| other.chapter == entry.chapter)
        .expect("the chapter has a first mission");
    let end = first
        + campaign
            .iter()
            .filter(|other| other.chapter == entry.chapter)
            .count();
    assert_eq!(
        (first, end),
        (15, 20),
        "chapter 4 does not span campaign positions 15..20"
    );
    assert!(
        first + 1 < position && position + 1 < end,
        "position {position} is not strictly inside chapter {} ({first}..{end})",
        entry.chapter
    );
    assert_eq!(
        campaign[position - 1].chapter,
        entry.chapter,
        "the previous position is in another chapter"
    );
    assert_eq!(
        campaign[position + 1].chapter,
        entry.chapter,
        "the next position is in another chapter"
    );

    // Both structures place the chapter's first and one-past-last rows at the
    // same campaign positions.
    let chapters_before = campaign
        .iter()
        .map(|other| other.chapter)
        .filter(|chapter| *chapter < entry.chapter)
        .collect::<BTreeSet<u32>>()
        .len();
    assert_eq!(
        chapters_before, 3,
        "M18 is not preceded by exactly three chapters"
    );
    let agreement = context.join_agreement();
    assert_eq!(
        agreement.state,
        JoinCorroboration::Agreed,
        "the localized table does not corroborate the layout, so no boundary can be compared"
    );
    let layout_bounds = |sizes: &[usize]| {
        (
            sizes[..chapters_before].iter().sum::<usize>(),
            sizes[..=chapters_before].iter().sum::<usize>(),
        )
    };
    assert_eq!(
        layout_bounds(&agreement.layout_chapters),
        (first, end),
        "the layout's chapter group does not span the campaign positions of chapter {}",
        entry.chapter
    );
    assert!(
        !agreement.grouped.is_empty(),
        "no region-prefixed block exists to compare the boundaries against"
    );
    for entry_block in &agreement.grouped {
        assert_eq!(
            entry_block.groups.len(),
            agreement.layout_chapters.len(),
            "the region groups and the chapters account for a different number of campaigns"
        );
        assert_eq!(
            layout_bounds(&entry_block.groups),
            (first, end),
            "the localized region group does not span campaign positions {first}..{end}"
        );
    }

    // M18's chapter is the *last* full one: the campaign's chapter sizes end
    // with a shorter group, so M18's own chapter boundary is the only place
    // where a "one row off" regrouping of the long names would still agree
    // with the layout. That is what makes the boundary above a measurement
    // rather than an assumption.
    let first_size = agreement
        .layout_chapters
        .first()
        .copied()
        .expect("a first chapter");
    let last_size = agreement
        .layout_chapters
        .last()
        .copied()
        .expect("a last chapter");
    assert!(
        last_size < first_size,
        "the layout's last chapter is not shorter than its first, so no chapter boundary is a real \
         boundary: {:?}",
        agreement.layout_chapters
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_a_the_world_group_is_the_whole_chapter_and_neither_it_nor_the_mission_number_identifies_the_mission()
 {
    // Two identities a consumer might wrongly key on coincide at M18.
    // `world/c4` holds all five campaign missions of chapter 4, and the number
    // `3` names a mission directory in *every* chapter
    // (`ZBD/C1B/M03` … `ZBD/C5/M03`) — with five different program archives, so
    // even the archive bytes do not collapse the two axes. Only the campaign
    // position singles out the one mission the record binds; the test pins both
    // axes and that the record binds exactly the mission its position selects.
    let context = context();
    let binding = binding();
    let campaign = context.campaign();
    let position = binding.campaign_position.expect("position resolved");
    let entry = &campaign[position];
    assert_eq!(
        position, 17,
        "M18 must resolve the eighteenth campaign position"
    );
    assert_eq!(entry.world_group, "c4", "M18 lives in the C4 world group");

    // --- the world group is the whole chapter, so the world row is not an
    // identity.
    let world_members: Vec<&CampaignMission> = campaign
        .iter()
        .filter(|other| other.world_group == entry.world_group)
        .collect();
    assert_eq!(
        world_members.len(),
        5,
        "the C4 world group does not hold all five campaign missions: {world_members:?}"
    );
    let chapter: Vec<&CampaignMission> = campaign
        .iter()
        .filter(|other| other.chapter == entry.chapter)
        .collect();
    assert_eq!(
        world_members.len(),
        chapter.len(),
        "the world group holds missions outside chapter {}",
        entry.chapter
    );
    let world = binding
        .world_id
        .as_ref()
        .expect("world id resolved")
        .as_str();
    assert!(
        world.ends_with(&entry.world_group),
        "the bound world id {world} is not the world group the layout declares"
    );
    let bound = binding.catalog_id.as_ref().map(|id| id.as_str());
    for member in &world_members {
        assert!(
            game_dir().join(&member.program_asset).is_file(),
            "world-group sibling {} does not exist",
            member.program_asset
        );
        let key = format!("mission/ch{}-m{:02}", member.chapter, member.mission_number);
        if (member.chapter, member.mission_number) == (entry.chapter, entry.mission_number) {
            assert_eq!(
                bound,
                Some(key.as_str()),
                "the record does not bind the mission the position selects"
            );
        } else {
            assert_ne!(
                bound,
                Some(key.as_str()),
                "the record binds {key}, which is not the mission its position selects"
            );
        }
    }
    let distinct_programs: BTreeSet<String> = world_members
        .iter()
        .map(|member| {
            format!(
                "script/{}-m{:02}-zrdr",
                member.world_group, member.mission_number
            )
        })
        .collect();
    assert_eq!(
        distinct_programs.len(),
        world_members.len(),
        "two missions of one world group claim the same program identity"
    );

    // --- the mission number is reused by one mission in every chapter, so the
    // mission number is not an identity either, and the record must not cite a
    // same-number sibling's archive.
    let same_number: Vec<&CampaignMission> = campaign
        .iter()
        .filter(|other| other.mission_number == entry.mission_number)
        .collect();
    assert_eq!(
        same_number.len(),
        5,
        "the mission number {} does not name one mission in every chapter: {same_number:?}",
        entry.mission_number
    );
    for sibling in &same_number {
        assert!(
            game_dir().join(&sibling.program_asset).is_file(),
            "same-number sibling {} is missing",
            sibling.program_asset
        );
        let key = format!(
            "mission/ch{}-m{:02}",
            sibling.chapter, sibling.mission_number
        );
        if sibling.chapter == entry.chapter {
            assert_eq!(
                bound,
                Some(key.as_str()),
                "the record must bind the chapter-{} mission numbered {}",
                entry.chapter,
                entry.mission_number
            );
        } else {
            assert_ne!(
                bound,
                Some(key.as_str()),
                "the record binds {key}, a different chapter's mission number, not its own"
            );
        }
    }
    // The two axes are genuinely different sets of missions: the world group is
    // chapter 4, while the mission number spans every chapter, and exactly one
    // mission sits at their intersection.
    assert_eq!(
        same_number
            .iter()
            .filter(|sibling| sibling.world_group == entry.world_group)
            .count(),
        1,
        "more or fewer than one mission is at the intersection of M18's world group and its \
         mission number"
    );

    // The same-number siblings are five *different* archives, measured through
    // the production layout read with digests, so no byte-level tie collapses
    // the reuse and the campaign position is the only thing that separates
    // them.
    let layout = campaign_layout(&game_dir()).expect("the production layout reads");
    let digests: BTreeMap<&str, &str> = layout
        .iter()
        .filter_map(|row| {
            Some((
                row.mission.program_asset.as_str(),
                row.program_sha256.as_deref()?,
            ))
        })
        .collect();
    assert_eq!(
        digests.len(),
        layout.len(),
        "the layout holds a program archive without a digest"
    );
    let sibling_digests: BTreeSet<&str> = same_number
        .iter()
        .map(|sibling| {
            *digests
                .get(sibling.program_asset.as_str())
                .unwrap_or_else(|| panic!("the layout has no digest for {}", sibling.program_asset))
        })
        .collect();
    assert_eq!(
        sibling_digests.len(),
        same_number.len(),
        "two missions numbered {} share program archive bytes: {sibling_digests:?}",
        entry.mission_number
    );
    let cited: BTreeSet<&str> = binding
        .source_spans
        .iter()
        .map(|span| span.asset_id.as_str())
        .collect();
    assert!(
        cited.contains(&entry.program_asset.as_str()),
        "the record does not cite its own program archive {}",
        entry.program_asset
    );
    for sibling in &same_number {
        if sibling.program_asset != entry.program_asset {
            assert!(
                !cited.contains(&sibling.program_asset.as_str()),
                "the record cites the same-number sibling {}",
                sibling.program_asset
            );
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_a_m18s_own_region_prefix_is_a_confirmed_row_that_selects_no_position() {
    // The refusal arm no earlier binding stage exercised on real data: a title
    // the localized table *does* carry, exactly once, so
    // `SourceContext::confirm_title` confirms it — and the row still names no
    // campaign position, because it sits outside every campaign-length block.
    // M18 supplies the natural instance: the region prefix of its own long
    // name is a standalone localized row in the same table.
    //
    // The region prefix is read out of M18's own second spelling, not written
    // down here, so the test follows the installation rather than a constant.
    let context = context();
    let binding = binding();
    let title = discovery_title();
    let position = binding.campaign_position.expect("position resolved");
    let confirmed = binding
        .localized_title_id
        .expect("M18's title string resolved");
    let long_name = context
        .other_spellings(position, confirmed)
        .into_iter()
        .next()
        .map(|(_, text)| text)
        .expect("M18 has a second display form");
    let prefix = long_name
        .strip_suffix(&title)
        .and_then(|rest| rest.strip_suffix(" - "))
        .map(str::to_owned)
        .expect("M18's second display form ends with the declared title");

    // The prefix is a title-shaped string the table carries on its own, and the
    // production confirmation accepts it.
    let rows_carrying: Vec<u32> = context
        .string_rows()
        .iter()
        .filter_map(|row| {
            let text = row.text.as_deref()?;
            display_text(text).eq(&prefix).then_some(row.id)
        })
        .collect();
    assert_eq!(
        rows_carrying.len(),
        1,
        "M18's region prefix {prefix:?} is not carried by exactly one retail row: {rows_carrying:?}"
    );
    let prefix_row = rows_carrying[0];
    assert_eq!(
        context
            .confirm_title(&prefix)
            .confirmed()
            .map(|(row_id, _)| row_id),
        Some(prefix_row),
        "production code did not confirm the row the strings carry {prefix:?} in"
    );
    // It is not a title row of the mission, and nothing claims it is: it sits
    // in no campaign-length row block, while M18's own confirmed row does.
    assert!(
        !context
            .campaign_title_blocks()
            .iter()
            .any(|block| block.contains(prefix_row)),
        "M18's region prefix row {prefix_row} now sits in a campaign-length block, so the refusal \
         this test pins is no longer produced by the installation"
    );
    assert!(
        block_containing(context, confirmed).contains(confirmed),
        "M18's own title row is not in a campaign-length block"
    );

    // Binding the prefix yields a record that confirms the title and resolves
    // no position at all: the title is real, the campaign is not.
    let partial = context
        .bind(MissionLabel::new(WORK_ORDER).expect("valid label"), &prefix)
        .expect("a title that selects no position is a recorded unknown, not a failure");
    partial
        .validate()
        .expect("the partial record is internally consistent");
    assert_eq!(
        partial.localized_title_id,
        Some(prefix_row),
        "the record does not name the row its title was confirmed in"
    );
    assert_eq!(partial.campaign_position, None);
    assert_eq!(partial.catalog_id, None);
    assert_eq!(partial.world_id, None);
    assert_eq!(partial.program_id, None);
    assert!(!partial.is_verified());

    // The order is the checklist's own, and the three position-dependent
    // entries are exactly the ones a row without a position cannot settle.
    let unresolved = partial.unresolved_critical();
    assert_eq!(
        unresolved,
        vec![
            CriticalDependency::MissionId,
            CriticalDependency::ProgramSourceMap,
            CriticalDependency::WorldGroupVariant
        ],
        "a confirmed row outside every campaign-length block must leave exactly the three \
         position-dependent dependencies unresolved: {unresolved:?}"
    );
    // The refusal names its own cause, and it is the row-block cause rather
    // than the missing-title one.
    for id in &unresolved {
        let dependency = partial
            .dependencies
            .iter()
            .find(|dependency| dependency.id == *id)
            .expect("the record carries the dependency");
        let DependencyState::Unresolved { reason, .. } = &dependency.state else {
            panic!("{id} is not unresolved");
        };
        assert_eq!(
            reason, SHORT_ROW_BLOCK_REFUSAL,
            "{id} refuses with the wrong reason: {reason:?}"
        );
    }
    // The title dependency *is* resolved: the installation carries the string.
    let title_dependency = partial
        .dependencies
        .iter()
        .find(|dependency| dependency.id == CriticalDependency::TitleString)
        .expect("the record carries the title dependency");
    assert!(
        matches!(title_dependency.state, DependencyState::Resolved { .. }),
        "the title the strings carry was reported as unresolved"
    );
    // The record cites the string row it confirmed and no program archive,
    // because it never located one: a confirmed title alone is not an
    // identity, and the cited spans must not imply otherwise.
    assert_eq!(
        partial.source_spans.len(),
        1,
        "the record cites more than the one string row it confirmed"
    );
    let cited_row = &partial.source_spans[0];
    assert!(
        game_dir().join(&cited_row.asset_id).is_file(),
        "the record cites {} which is not in the installation",
        cited_row.asset_id
    );
    assert_eq!(
        row_display(context, prefix_row),
        prefix,
        "the cited row {prefix_row} does not carry the confirmed title"
    );
    assert!(
        partial
            .source_spans
            .iter()
            .all(|span| !span.asset_id.starts_with("ZBD/")),
        "the record cites a mission program archive it never resolved: {:?}",
        partial
            .source_spans
            .iter()
            .map(|span| span.asset_id.as_str())
            .collect::<Vec<_>>()
    );

    // The campaign record built from it does not read as bound either.
    let mission_binding = partial
        .to_mission_binding()
        .expect("an incomplete record still builds a valid mission record");
    assert!(mission_binding.cells().any(|(category, state)| category
        == BindingCategory::MissionIdentity
        && state == CellState::Unknown));
    assert!(!mission_binding.is_placeholder());
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_a_the_campaign_keeps_everything_else_unresolved_and_unready() {
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

    let m18 = campaign.get(&label(WORK_ORDER)).expect("M18 is recorded");
    assert!(
        !m18.is_placeholder(),
        "M18 was bound, not left a placeholder"
    );
    for (category, state) in m18.cells() {
        let expected = if category == BindingCategory::MissionIdentity {
            CellState::Complete
        } else {
            CellState::Unknown
        };
        assert_eq!(state, expected, "category {category} has the wrong state");
    }
    assert!(
        m18.dependencies
            .iter()
            .all(|row| matches!(row.state, DependencyState::Unresolved { .. }))
    );

    let closure = campaign
        .closure(&label(WORK_ORDER), None)
        .expect("M18's closure computes without a catalog");
    assert_eq!(closure.reached, vec![label(WORK_ORDER)]);
    assert_eq!(closure.cell_count(), 7);
    assert_eq!(closure.complete_cells(), 1);
    assert_eq!(closure.unresolved_subsystems, 23);
    assert_eq!(closure.unknown_progression, 1);
    assert!(!closure.is_complete());
}

// -------------------------------------------------------------- synthetic ---

#[test]
fn accept_m18_a_a_title_block_must_be_exactly_the_campaign_length() {
    // The run rule decides which rows may name a campaign position, so it is
    // exercised here without an installation: a gap ends a run, a run one row
    // short or one row long is not a campaign, and two runs of the campaign
    // length are both listed rather than only the first.
    let campaign = 4usize;
    let ids = |range: std::ops::RangeInclusive<u32>| range.collect::<BTreeSet<u32>>();

    assert_eq!(title_blocks(&BTreeSet::new()), Vec::new(), "no ids, no run");
    assert_eq!(
        title_blocks(&ids(7..=7)).len(),
        1,
        "a single row is a run of one"
    );

    // A gap ends a run: 1..=3 and 5..=7 are two runs, not one.
    let split = title_blocks(&ids(1..=3).union(&ids(5..=7)).copied().collect());
    assert_eq!(split.len(), 2, "a gap must end a run: {split:?}");
    assert_eq!(split[0].len(), 3);
    assert_eq!(split[0].first_id(), 1);
    assert_eq!(split[0].last_id(), 3);
    assert!(split[0].contains(2) && !split[0].contains(4));

    let exact = title_blocks(&ids(10..=13));
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].len(), campaign);

    // One row short and one row long are not the campaign.
    for (rows, why) in [(3usize, "one row short"), (5, "one row long")] {
        let set: BTreeSet<u32> = (0..rows as u32).collect();
        let blocks = title_blocks(&set);
        assert_eq!(blocks.len(), 1);
        assert_ne!(
            blocks[0].len(),
            campaign,
            "a block {why} must not pass as the campaign"
        );
    }

    // Two runs of exactly the campaign length are both reported, ascending.
    let two: BTreeSet<u32> = (0..campaign as u32)
        .chain(100..(100 + campaign) as u32)
        .collect();
    let blocks = title_blocks(&two);
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    assert!(blocks[0].last_id() < blocks[1].first_id());
    assert!(blocks.iter().all(|block| block.len() == campaign));
    assert!(!blocks.iter().any(TitleBlock::is_empty));
    assert_eq!(blocks[0].to_string(), "0..3");
}

#[test]
fn accept_m18_a_a_confirmed_row_outside_every_campaign_block_selects_no_position() {
    // The predicate the M18-A refusal test relies on, proved arm by arm on
    // authored values, because the installation only ever produces one of these
    // arms: a *confirmed* row that names no campaign position. Confirmation and
    // position are separate rules, and a table where a real string is
    // confirmed verbatim outside every campaign-length block must still yield
    // no identity — the short-block refusal, not the missing-title one.
    let layout = vec![5usize, 5, 5, 5, 4];
    let grouped = |groups: Vec<usize>| {
        if groups.is_empty() {
            Vec::new()
        } else {
            vec![GroupedTitleBlock {
                block: TitleBlock::new(100, 123).expect("block"),
                groups,
            }]
        }
    };
    let agreed = JoinAgreement {
        layout_chapters: layout.clone(),
        blocks: vec![TitleBlock::new(100, 123).expect("block")],
        grouped: grouped(layout.clone()),
        state: JoinCorroboration::Agreed,
    };

    // A row inside the campaign-length block selects its index.
    assert_eq!(
        campaign_position_for(Some(117), &agreed),
        Ok(17),
        "a confirmed row must select the position its index names"
    );
    assert_eq!(
        campaign_position_for(Some(117), &agreed),
        Ok(17),
        "the same row must select the same position twice"
    );

    // A confirmed row *outside* every campaign-length block names no position,
    // even though the table agrees: confirmation never implies a position.
    for outside in [0u32, 99, 124, 500] {
        assert_eq!(
            campaign_position_for(Some(outside), &agreed),
            Err(SHORT_ROW_BLOCK_REFUSAL),
            "a confirmed row outside every campaign-length block selected a position"
        );
    }
    // No confirmed row is refused before the table is consulted.
    assert_eq!(
        campaign_position_for(None, &agreed),
        Err(NO_CONFIRMED_ROW_REFUSAL),
        "no confirmed row can select a position"
    );
    assert!(
        NO_CONFIRMED_ROW_REFUSAL.contains("no single localized row"),
        "the refusal must name the missing confirmation: {NO_CONFIRMED_ROW_REFUSAL:?}"
    );
    assert!(
        SHORT_ROW_BLOCK_REFUSAL.contains("row block"),
        "the refusal must name the short row block: {SHORT_ROW_BLOCK_REFUSAL:?}"
    );

    // A contradicting table refuses a confirmed in-block row too, so the two
    // refusals cannot be confused.
    let contradicted = JoinAgreement {
        state: JoinCorroboration::Disagreed,
        ..agreed.clone()
    };
    assert_eq!(
        campaign_position_for(Some(117), &contradicted),
        Err(CONTRADICTED_JOIN_REFUSAL),
        "a contradicted table must select no position even for a confirmed row"
    );
    assert_eq!(
        campaign_position_for(Some(99), &contradicted),
        Err(CONTRADICTED_JOIN_REFUSAL),
        "the contradiction is decided before the row block is consulted"
    );
    // An unchallenged table — nothing to compare against — must not refuse an
    // in-block row, or the corroboration would be decorative.
    let unchallenged = JoinAgreement {
        grouped: Vec::new(),
        state: JoinCorroboration::Unavailable,
        ..agreed
    };
    assert_eq!(
        campaign_position_for(Some(117), &unchallenged),
        Ok(17),
        "an unchallenged table must not refuse a confirmed row"
    );
    assert_eq!(
        campaign_position_for(Some(99), &unchallenged),
        Err(SHORT_ROW_BLOCK_REFUSAL),
        "an unchallenged table still refuses a row outside every campaign-length block"
    );
}

#[test]
fn accept_m18_a_a_near_miss_title_is_never_confirmed() {
    // `SourceContext::confirm_title` compares display text byte for byte, and
    // the retail installation offers no near miss that would expose a fuzzy
    // comparison — so the exactness is proved here on authored values, arm by
    // arm. A prefix, a suffix, a case change, a whitespace change and an
    // empty tail all name no row, and a title that merely *contains* the
    // long-name separator must not be treated as a region-prefixed long name
    // either. Without this, a comparison that accepted a partial match would
    // let an unrelated row confirm a mission's title.
    let title = "Deceit at Devil's Horn";

    // The two forms that do confirm, and nothing else.
    assert_eq!(
        title_form(title, title),
        Some(TitleForm::Verbatim),
        "a row whose display text is the title must confirm verbatim"
    );
    assert_eq!(
        title_form(&format!("Rocky Mountains - {title}"), title),
        Some(TitleForm::RegionPrefixedLongName),
        "a region-prefixed long name must confirm through its tail"
    );
    // A row that is the long name *with* the title is not verbatim: it carries
    // the title in the weaker form, which is the only form available to it.
    assert_eq!(
        title_form(&format!("Rocky Mountains - {title}"), title),
        Some(TitleForm::RegionPrefixedLongName),
        "the long-name form must not be reported as verbatim"
    );

    // Every near miss: no row, so no confirmation.
    for miss in [
        String::new(),
        " ".to_owned(),
        "deceit at devil's horn".to_owned(),
        "Deceit at Devil's Horn ".to_owned(),
        " Deceit at Devil's Horn".to_owned(),
        "Deceit at Devil's Horn.".to_owned(),
        "Deceit at Devils Horn".to_owned(),
        "Deceit at Devil's Horns".to_owned(),
        "The Deceit at Devil's Horn".to_owned(),
    ] {
        assert_eq!(
            title_form(&miss, title),
            None,
            "the near miss {miss:?} was treated as a confirmed row"
        );
    }
    // A title that is a *prefix* of a long name's tail names no row either:
    // the tail must equal the title, not merely contain it. The same shorter
    // string still confirms a long name whose tail is exactly it, so this is
    // exactness, not a stricter rule for prefixes.
    for (short, decoy) in [
        ("Deceit", "Deceit at Devil's Horn II"),
        ("Deceit at", "Deceit at Angel Island"),
        ("Deceit at Devil's", "Deceit at Devil's Peak"),
    ] {
        let longer = format!("Rocky Mountains - {decoy}");
        assert!(
            decoy.starts_with(short) && decoy.len() > short.len(),
            "the decoy {decoy:?} must be a strict extension of {short:?}"
        );
        assert_eq!(
            title_form(&longer, decoy),
            Some(TitleForm::RegionPrefixedLongName),
            "the decoy long name must confirm its own exact tail"
        );
        assert_eq!(
            title_form(&longer, short),
            None,
            "a long name whose tail merely starts with {short:?} confirmed it"
        );
        assert_eq!(
            title_form(&longer, title),
            None,
            "a long name whose tail merely extends {short:?} confirmed the full title"
        );
        assert_eq!(
            title_form(&format!("Rocky Mountains - {short}"), short),
            Some(TitleForm::RegionPrefixedLongName),
            "a long name whose tail *is* {short:?} must still confirm that shorter title"
        );
    }
    // The separator is a display convention of the long-name rows, not a
    // wildcard: an empty prefix or an empty tail is not a long name.
    for degenerate in [
        format!(" - {title}"),
        "Rocky Mountains - ".to_owned(),
        format!("Rocky Mountains -{title}"),
        format!("Rocky Mountains{title}"),
    ] {
        assert_eq!(
            title_form(&degenerate, title),
            None,
            "the degenerate long name {degenerate:?} confirmed the title"
        );
    }
    // A title that happens to contain the separator is only ever matched
    // verbatim; it never manufactures a long-name row.
    let embedded = "Rocky Mountains - Deceit at Devil's Horn";
    assert_eq!(
        title_form(embedded, embedded),
        Some(TitleForm::Verbatim),
        "a title containing the separator must confirm verbatim when a row is its whole text"
    );
    assert_eq!(
        title_form(
            "Deceit at Devil's Horn - Rocky Mountains",
            "Rocky Mountains"
        ),
        Some(TitleForm::RegionPrefixedLongName),
        "the separator splits on its first occurrence, in either orientation"
    );
    // `title_form` is the pure rule the confirmation reads, so what it refuses
    // is what `confirm_title` has to report as `Uncarried`. This is why the
    // retail failure arms in
    // `accept_m18_a_the_original_name_is_confirmed_against_the_local_strings`
    // can assert an unresolved title dependency rather than a wrong identity.
    assert!(
        title_form("The Red Menacee", "The Red Menacee") == Some(TitleForm::Verbatim)
            && title_form("The Red Menacee", "The Red Menace").is_none(),
        "the two comparison rules no longer distinguish a title from its prefix"
    );
}

#[test]
fn accept_m18_a_a_contradicted_corroboration_establishes_no_position() {
    // The guard is only real if a disagreement actually stops the join, and
    // the retail installation never disagrees — so the contradiction arm is
    // proved here on authored values, arm by arm. What those values measure
    // on the retail installation is
    // `accept_m18_a_the_join_is_corroborated_by_the_long_name_rows`.
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

    // Agreed: the long names fall into the layout's chapter sizes.
    let agreed = grouped(layout.clone());
    assert_eq!(classify_join(&layout, &agreed), JoinCorroboration::Agreed);

    // Disagreed: the same rows grouped differently.
    let disagreed = grouped(vec![24]);
    assert_eq!(
        classify_join(&layout, &disagreed),
        JoinCorroboration::Disagreed
    );
    // One agreeing block cannot outvote one that contradicts.
    let mixed = [GroupedTitleBlock {
        block,
        groups: layout.clone(),
    }]
    .into_iter()
    .chain(disagreed.clone())
    .collect::<Vec<_>>();
    assert_eq!(
        classify_join(&layout, &mixed),
        JoinCorroboration::Disagreed,
        "a single contradicting block must decide the join"
    );
    // A different shape with the same total is still a contradiction.
    let regrouped = grouped(vec![6, 6, 6, 6]);
    assert_eq!(
        classify_join(&layout, &regrouped),
        JoinCorroboration::Disagreed
    );

    // Unavailable: nothing to check against is not a disagreement.
    let unavailable = classify_join(&layout, &grouped(Vec::new()));
    assert_eq!(unavailable, JoinCorroboration::Unavailable);

    // The guard a binding reads: only a contradiction stops it.
    for (state, expected) in [
        (JoinCorroboration::Unavailable, true),
        (JoinCorroboration::Agreed, true),
        (JoinCorroboration::Disagreed, false),
    ] {
        let report = JoinAgreement {
            layout_chapters: layout.clone(),
            blocks: vec![block],
            grouped: match state {
                JoinCorroboration::Unavailable => Vec::new(),
                JoinCorroboration::Agreed => grouped(layout.clone()),
                JoinCorroboration::Disagreed => grouped(vec![24]),
            },
            state,
        };
        assert_eq!(
            report.establishes(),
            expected,
            "state {state:?} must {} a campaign position",
            if expected { "establish" } else { "refuse" }
        );
    }

    // A record whose critical dependencies are all resolved is still not
    // verified while its checklist entries are unknown: the two states stay
    // apart.
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
    assert!(
        authored
            .unknowns
            .iter()
            .any(|entry| entry.contains("closure_sha256")),
        "the closure hash is one of the checklist entries still unbound"
    );
}

/// A source binding with every critical dependency resolved but the checklist
/// entries a source binding does not bind still unknown, built here from
/// authored values. It proves only the predicates of
/// [`SourceBinding::unresolved_critical`] and [`SourceBinding::is_verified`].
fn authored_binding() -> SourceBinding {
    let claim = |suffix: &str| ClaimId::new(&format!("m18.a.synthetic.{suffix}")).expect("claim");
    let provenance = |suffix: &str| {
        Provenance::new(claim(suffix), ClaimStatus::Designed, None)
            .expect("designed provenance always validates")
    };
    SourceBinding {
        label: MissionLabel::new(WORK_ORDER).expect("M18 is a valid label"),
        discovery_title: "Authored Title".to_owned(),
        install_sha256: "0".repeat(64),
        campaign_position: Some(5),
        campaign_size: 24,
        catalog_id: Some(ContentId::from_source(ContentKind::Mission, "ch2-m01").expect("id")),
        world_id: Some(ContentId::from_source(ContentKind::World, "c2").expect("id")),
        program_id: Some(ContentId::from_source(ContentKind::Script, "c2-m01-zrdr").expect("id")),
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
            "objective graph: not bound from original data at this stage".to_owned(),
            "closure_sha256: the mission dependency closure hash is not measured".to_owned(),
        ],
    }
}
