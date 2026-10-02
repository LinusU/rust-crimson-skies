//! Acceptance stage M16-A: bind the sixteenth mission's original data and
//! branches (`missions/M16.md`, work order `M16-A`).
//!
//! The stage's minimum scenario is "Source-derived binding has no unresolved
//! critical dependencies", and
//! [`accept_m16_a_source_derived_binding_has_no_unresolved_critical_dependencies`]
//! asks production code exactly that: [`SourceContext::read`] fingerprints
//! `$CS_GAME_DIR` and reads its campaign layout and localized string table,
//! [`SourceContext::bind`] resolves the five critical dependencies of the M16
//! data-binding checklist, and nothing else in the record is allowed to read
//! as finished.
//!
//! M16 is the sixteenth campaign position and the join machinery is M02-A's; what
//! this stage pins is what is *different* at M16:
//!
//! * **The position is the first mission of chapter 4.** The layout's chapter
//!   boundary and the localized long names' region boundary fall on the same
//!   row, as at M06, but here in the fourth chapter.
//!   [`accept_m16_a_the_position_is_the_first_row_of_the_fourth_chapter_and_region_group`]
//!   measures that campaign position 15 is the first mission of chapter 4
//!   and the first row of the localized long names' fourth region group.
//! * **The world group is the whole chapter, not the mission.** `world/c4`
//!   carries all five missions of chapter 4, unlike chapter 2 whose fifth
//!   mission lives in a separate `c2b` directory.
//!   [`accept_m16_a_the_world_group_is_the_whole_chapter_and_not_the_mission`]
//!   pins that the world row cannot identify M16.
//! * **The original spellings disagree, as at M03.** The short-name row and
//!   the region-prefixed long name are different strings;
//!   [`accept_m16_a_the_original_name_is_confirmed_against_the_local_strings`]
//!   pins what is actually confirmed.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which
//! has no original data) skips them; they are run with `--include-ignored`
//! by the implementing and reviewing agents. Every assertion below is made
//! against facts the test re-reads from the installation or from committed
//! records — never against a constant that repeats the implementation. The two
//! synthetic tests are left unignored so CI runs them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    BindingCategory, CONTRADICTED_JOIN_REFUSAL, CampaignBindings, CampaignMission, CellState,
    CriticalDependency, DependencyState, GroupedTitleBlock, JoinAgreement, JoinCorroboration,
    MissionLabel, NO_CONFIRMED_ROW_REFUSAL, SHORT_ROW_BLOCK_REFUSAL, SourceBinding, SourceContext,
    TitleBlock, campaign_position_for, classify_join, title_blocks,
};
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};

use crate::common::{label, load_inventory, repo_path};

/// The one work order this stage binds.
const WORK_ORDER: &str = "M16";

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M16-A needs the retail capability; run this suite with \
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

/// The declared discovery title of `M16`, read from the committed inventory
/// rather than repeated here.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == WORK_ORDER)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {WORK_ORDER} work order"))
}

/// The M16 binding derived from the installation, built once.
fn binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        context()
            .bind(
                MissionLabel::new(WORK_ORDER).expect("M16 is a valid label"),
                &discovery_title(),
            )
            .expect("M16 binds to the original data")
    })
}

/// The declared campaign, with M16 bound the way the record describes.
fn campaign() -> CampaignBindings {
    let mut campaign = CampaignBindings::from_inventory(&load_inventory())
        .expect("the declared inventory builds the campaign");
    campaign
        .bind(binding().to_mission_binding().expect("record is valid"))
        .expect("M16 is a declared mission");
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

// ---------------------------------------------------------------- retail ---

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_a_source_derived_binding_has_no_unresolved_critical_dependencies() {
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

    // M16 is the sixteenth mission: position 15. It is not any mission M01-A,
    // M02-A or M03-A bound, and its chapter is later than theirs.
    assert_eq!(position, 15, "M16 is the sixteenth campaign position");
    assert_ne!(
        (entry.chapter, entry.mission_number),
        (campaign[0].chapter, campaign[0].mission_number),
        "M16 resolved the campaign's first mission, which is M01's identity"
    );
    assert_ne!(
        (entry.chapter, entry.mission_number),
        (campaign[2].chapter, campaign[2].mission_number),
        "M16 resolved the campaign's third mission, which is M03's identity"
    );
    assert!(
        entry.chapter > campaign[2].chapter,
        "M16 resolved a mission inside M03's chapter, so it is not the later chapter this stage binds"
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
fn accept_m16_a_the_committed_record_is_what_the_installation_derives() {
    let committed = fs::read_to_string(repo_path("missions/bindings/M16.json"))
        .expect("missions/bindings/M16.json exists");
    let derived = binding().to_json();
    assert_eq!(
        committed, derived,
        "the committed binding record is not what production code derives from $CS_GAME_DIR"
    );

    // The record is the schema's shape, with the values that are still unknown
    // kept explicit instead of silently omitted.
    for key in [
        "\"schema_version\": 1",
        "\"work_order\": \"M16\"",
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
fn accept_m16_a_the_original_name_is_confirmed_against_the_local_strings() {
    // `M16-BIND`: "Find the corresponding original catalog/program/world
    // identities. Confirm the title against local strings; do not key runtime
    // logic by this discovery label." This measures what that confirmation
    // actually is: the declared title equals the display text of exactly one
    // retail row, and it is a *different* string from both the region-prefixed
    // long name the installation gives the same mission and from M03's.
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
    let block = context
        .campaign_title_blocks()
        .into_iter()
        .find(|block| block.contains(matching[0]))
        .expect("the confirmed title sits in a campaign-length row block");
    assert_eq!(block.len(), context.campaign().len());

    // The installation spells this mission differently in its long name, so
    // the confirmation is a fact about *a* localized string, not about "the"
    // name of the mission. The row at the same index of any *other*
    // campaign-length block must say something else.
    let position = binding.campaign_position.expect("a position was resolved");
    let own = block_containing(context, matching[0]);
    let siblings: Vec<(u32, String)> = context
        .campaign_title_blocks()
        .into_iter()
        .filter(|block| *block != own)
        .flat_map(|block| {
            let first = block.first_id();
            let last = block.last_id();
            (first..=last).map(move |id| (id, block))
        })
        .filter_map(|(id, block)| {
            let row = context.string_rows().iter().find(|row| row.id == id)?;
            let text = display_text(row.text.as_deref()?).to_owned();
            ((id - block.first_id()) as usize == position).then_some((id, text))
        })
        .collect();
    assert_eq!(
        siblings.len(),
        1,
        "expected exactly one second row at campaign position {position}, found {siblings:?}"
    );
    let (sibling_id, sibling) = &siblings[0];
    assert_ne!(
        sibling, &title,
        "the installation gives the same mission two identical spellings, so the two localized row \
         blocks are not independent"
    );
    assert!(
        sibling.contains(" - "),
        "the second spelling {sibling:?} at row {sibling_id} is not a region-prefixed long name"
    );

    // A title the strings do not carry is never confirmed. This is the failure
    // case of the confirmation itself: first a spelling no retail row carries,
    // then a real display text the retail table carries *twice* — which the
    // binding must refuse as firmly, because a title matching several rows
    // names no single row and therefore no single campaign position.
    for miss in [
        "The Red Menacee".to_owned(),
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

/// The campaign-length block `id` sits in.
fn block_containing(context: &SourceContext, id: u32) -> TitleBlock {
    context
        .campaign_title_blocks()
        .into_iter()
        .find(|block| block.contains(id))
        .unwrap_or_else(|| panic!("string id {id} sits in no campaign-length row block"))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_a_the_join_is_corroborated_by_the_long_name_rows() {
    // A campaign-length run of localized rows is weak evidence for the join:
    // any 24 unrelated strings would form one. The installation offers a
    // second structure — the region-prefixed long mission names — whose row
    // *grouping* is checkable without claiming what any region name means.
    // This measures that agreement on the retail installation, and checks that
    // M16's own row block is the one that carries no region prefix, so the
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

    // M16's own title block is one of the campaign-length blocks and is *not*
    // the grouped one, so the corroboration cannot be M16 agreeing with
    // itself.
    let title_id = binding()
        .localized_title_id
        .expect("M16's title string resolved");
    let own = block_containing(context, title_id);
    assert!(
        !agreement.grouped.iter().any(|entry| entry.block == own),
        "M16's own row block {} carries region prefixes, so it corroborates itself",
        own
    );

    // Every row of the corroborating block selects the same campaign position
    // its index names, through the production join, so the agreement is about
    // positions and not about a length.
    for entry in &agreement.grouped {
        for offset in 0..entry.block.len() {
            let id = entry.block.first_id() + u32::try_from(offset).expect("row fits in u32");
            let row = context
                .string_rows()
                .iter()
                .find(|row| row.id == id)
                .unwrap_or_else(|| {
                    panic!(
                        "the row block {block} has no row at {id}",
                        block = entry.block
                    )
                });
            let title = display_text(
                row.text
                    .as_deref()
                    .unwrap_or_else(|| panic!("row {id} does not decode")),
            );
            let bound = context
                .bind(MissionLabel::new(WORK_ORDER).expect("valid label"), title)
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
fn accept_m16_a_the_position_is_the_first_row_of_the_fourth_chapter_and_region_group() {
    // M16 is the first campaign position M01-A … M12-A did not cover, and it is
    // a genuine boundary: with the campaign ordered by `(chapter, mission
    // number)`, position 15 is the first mission of chapter 4, and the localized
    // long names' first region group ends on that same row. Both structures
    // put a boundary at row 5, so a join that divided either group one row off
    // would move the bound identity.
    let context = context();
    let binding = binding();
    let campaign = context.campaign();
    let position = binding.campaign_position.expect("position resolved");
    let entry = &campaign[position];

    assert_eq!(position, 15, "M16 is the sixteenth campaign position");
    assert_eq!(
        entry.mission_number, 1,
        "M16 is not its chapter's first mission: {entry:?}"
    );
    assert!(
        entry.chapter > 3,
        "M16 is not in a later chapter: {entry:?}"
    );
    assert_eq!(
        campaign
            .iter()
            .position(|other| other.chapter == entry.chapter),
        Some(position),
        "the bound position is not the first mission of chapter {}",
        entry.chapter
    );
    assert_ne!(
        campaign[position - 1].chapter,
        entry.chapter,
        "the previous position is in the same chapter, so {position} is not a chapter boundary"
    );

    // The campaign positions before M16's are exactly the earlier chapters: no
    // mission of M16's chapter precedes it, and no mission of an earlier
    // chapter follows it.
    let positions_before = campaign
        .iter()
        .filter(|other| other.chapter < entry.chapter)
        .count();
    assert_eq!(
        positions_before, position,
        "the campaign positions before M16 are not one full set of earlier chapters"
    );
    // How many *chapters* precede M16's: the index the two structures must
    // place their boundary at.
    let chapters_before = campaign
        .iter()
        .map(|other| other.chapter)
        .filter(|chapter| *chapter < entry.chapter)
        .collect::<BTreeSet<u32>>()
        .len();
    assert!(
        chapters_before > 0 && chapters_before < context.chapter_sizes().len(),
        "M16 is not an interior chapter boundary: {chapters_before} chapter(s) precede it"
    );

    let agreement = context.join_agreement();
    assert_eq!(
        agreement.state,
        JoinCorroboration::Agreed,
        "the localized table does not corroborate the layout, so no boundary can be compared"
    );
    assert_eq!(
        agreement.layout_chapters[..chapters_before]
            .iter()
            .sum::<usize>(),
        position,
        "the layout's chapter boundary is not at campaign position {position}"
    );
    assert!(
        !agreement.grouped.is_empty(),
        "no region-prefixed block exists to compare the boundary against"
    );
    for entry_block in &agreement.grouped {
        assert_eq!(
            entry_block.groups.len(),
            agreement.layout_chapters.len(),
            "the region groups and the chapters account for a different number of campaigns"
        );
        assert_eq!(
            entry_block.groups[..chapters_before].iter().sum::<usize>(),
            position,
            "the localized region boundary is not at campaign position {position}"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_a_the_world_group_is_the_whole_chapter_and_not_the_mission() {
    // M16's world group is not the mission: `world/c4` carries every campaign
    // mission of chapter 4, so the world row alone cannot identify M16 — only
    // the campaign position does. Unlike chapter 2, whose fifth mission lives
    // in a separate `c2b` directory, the whole of chapter 4 shares one world
    // group; what is bound is only the mission the position selected.
    let context = context();
    let binding = binding();
    let campaign = context.campaign();
    let position = binding.campaign_position.expect("position resolved");
    let entry = &campaign[position];
    assert_eq!(entry.world_group, "c4", "M16 lives in the C4 world group");

    let siblings: Vec<&CampaignMission> = campaign
        .iter()
        .filter(|other| other.world_group == entry.world_group)
        .collect();
    assert!(
        siblings.len() > 1,
        "the C4 world group carries only one campaign mission: {siblings:?}"
    );
    assert!(
        siblings
            .iter()
            .any(|other| other.mission_number == entry.mission_number),
        "the world group does not contain the bound mission: {siblings:?}"
    );
    assert!(
        siblings
            .iter()
            .any(|other| other.mission_number != entry.mission_number),
        "the world group contains only the bound mission, so it identifies it after all"
    );

    // The chapter is *not* spread across several world-group directories: the
    // world row is the chapter here, and the mission is told apart only by its
    // position and program archive.
    let chapter: Vec<&CampaignMission> = campaign
        .iter()
        .filter(|other| other.chapter == entry.chapter)
        .collect();
    assert_eq!(chapter.len(), 5, "chapter 4 declares five missions");
    let groups: BTreeSet<&str> = chapter
        .iter()
        .map(|other| other.world_group.as_str())
        .collect();
    assert_eq!(
        groups,
        BTreeSet::from(["c4"]),
        "chapter {} is spread over several world groups: {groups:?}",
        entry.chapter
    );
    assert_eq!(
        siblings.len(),
        chapter.len(),
        "the world group holds missions outside chapter {}",
        entry.chapter
    );

    // The world id is the layout's group for this mission, and it is not M03's
    // world group (the mission the previous stage bound).
    let world = binding
        .world_id
        .as_ref()
        .expect("world id resolved")
        .as_str();
    assert!(
        world.ends_with(&entry.world_group),
        "the bound world id {world} is not the world group the layout declares"
    );
    let m03 = context
        .bind(
            MissionLabel::new("M03").expect("valid label"),
            "The Secret Invasion",
        )
        .expect("M03 binds");
    assert_ne!(
        m03.world_id.as_ref().map(|id| id.as_str()),
        Some(world),
        "M16 resolved M03's world group"
    );

    // Every chapter member's archive exists, and the binding cites only the
    // archive of the mission its position selected, never a sibling's.
    for member in &chapter {
        let path = game_dir().join(&member.program_asset);
        assert!(
            path.is_file(),
            "the chapter member archive {} does not exist",
            member.program_asset
        );
    }
    let cited: Vec<&str> = binding
        .source_spans
        .iter()
        .map(|span| span.asset_id.as_str())
        .collect();
    assert!(
        cited.contains(&entry.program_asset.as_str()),
        "the record does not cite its own program archive {}",
        entry.program_asset
    );
    for sibling in chapter
        .iter()
        .filter(|other| other.program_asset != entry.program_asset)
    {
        assert!(
            !cited.contains(&sibling.program_asset.as_str()),
            "the record cites the sibling archive {}",
            sibling.program_asset
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_a_the_campaign_keeps_everything_else_unresolved_and_unready() {
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

    let m16 = campaign.get(&label(WORK_ORDER)).expect("M16 is recorded");
    assert!(
        !m16.is_placeholder(),
        "M16 was bound, not left a placeholder"
    );
    for (category, state) in m16.cells() {
        let expected = if category == BindingCategory::MissionIdentity {
            CellState::Complete
        } else {
            CellState::Unknown
        };
        assert_eq!(state, expected, "category {category} has the wrong state");
    }
    assert!(
        m16.dependencies
            .iter()
            .all(|row| matches!(row.state, DependencyState::Unresolved { .. }))
    );

    let closure = campaign
        .closure(&label(WORK_ORDER), None)
        .expect("M16's closure computes without a catalog");
    assert_eq!(closure.reached, vec![label(WORK_ORDER)]);
    assert_eq!(closure.cell_count(), 7);
    assert_eq!(closure.complete_cells(), 1);
    assert_eq!(closure.unresolved_subsystems, 23);
    assert_eq!(closure.unknown_progression, 1);
    assert!(!closure.is_complete());
}

// -------------------------------------------------------------- synthetic ---

#[test]
fn accept_m16_a_a_title_block_must_be_exactly_the_campaign_length() {
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
fn accept_m16_a_a_contradicted_corroboration_establishes_no_position() {
    // The guard is only real if a disagreement actually stops the join, and
    // the retail installation never disagrees — so the contradiction arm is
    // proved here on authored values, arm by arm. What those values measure
    // on the retail installation is
    // `accept_m16_a_the_join_is_corroborated_by_the_long_name_rows`.
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

    // Every arm of the decision a binding actually reads. The contradiction
    // arm is the one no retail installation produces, so it is proved here
    // rather than left untested: a confirmed row inside a campaign-length
    // block must select a position when the table agrees and none when it
    // contradicts the layout, and each refusal names its own cause.
    let agreed = JoinAgreement {
        layout_chapters: layout.clone(),
        blocks: vec![TitleBlock::new(100, 123).expect("block")],
        grouped: grouped(layout.clone()),
        state: JoinCorroboration::Agreed,
    };
    assert_eq!(
        campaign_position_for(Some(113), &agreed),
        Ok(13),
        "a confirmed row must select the position its index names"
    );
    assert_eq!(
        campaign_position_for(Some(113), &agreed),
        Ok(13),
        "the same row must select the same position twice"
    );
    assert_eq!(
        campaign_position_for(None, &agreed),
        Err(NO_CONFIRMED_ROW_REFUSAL),
        "no confirmed row can select a position"
    );
    assert_eq!(
        campaign_position_for(Some(124), &agreed),
        Err(SHORT_ROW_BLOCK_REFUSAL),
        "a row outside every campaign-length block selects no position"
    );
    assert_eq!(
        campaign_position_for(Some(99), &agreed),
        Err(SHORT_ROW_BLOCK_REFUSAL),
        "a row before every campaign-length block selects no position"
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
    assert_eq!(
        campaign_position_for(None, &contradicted),
        Err(NO_CONFIRMED_ROW_REFUSAL),
        "an unconfirmed row is refused before the table is consulted"
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
    assert_eq!(
        campaign_position_for(Some(113), &unchallenged),
        Ok(13),
        "an unchallenged table must not refuse a confirmed row"
    );

    // And a record whose critical dependencies are all resolved is still not
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
    let claim = |suffix: &str| ClaimId::new(&format!("m16.a.synthetic.{suffix}")).expect("claim");
    let provenance = |suffix: &str| {
        Provenance::new(claim(suffix), ClaimStatus::Designed, None)
            .expect("designed provenance always validates")
    };
    SourceBinding {
        label: MissionLabel::new(WORK_ORDER).expect("M16 is a valid label"),
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
