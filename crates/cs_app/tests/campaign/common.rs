//! Shared fixtures for the F50-A acceptance tests.
//!
//! Everything here is authored in this file: synthetic content ids,
//! designed provenance and the new-engine work-order inventory committed at
//! `missions/bindings/campaign-inventory.tsv`. No original game data, no
//! `CS_GAME_DIR` access — these tests prove the schema and its validation,
//! never the campaign (F50 owner ruling, 2026-09-28).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cs_content::campaign_bindings::{
    BindingCategory, BindingRow, CampaignBindings, CampaignInventory, CategoryState,
    DependencyState, MISSION_ROLE, MissionBinding, MissionLabel, PROGRAM_ROLE, Progression,
    REQUIRED_SUBSYSTEMS, SubsystemDependency, SubsystemId, WORLD_ROLE,
};
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::ClaimId;

/// A claim id for a synthetic assertion.
pub(crate) fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("test claim id is valid")
}

/// Designed provenance for a value this file invents.
pub(crate) fn designed(id: &str) -> Provenance {
    Provenance::designed(claim(id))
}

/// A content id of `kind` for a synthetic key.
pub(crate) fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

/// A work-order label.
pub(crate) fn label(text: &str) -> MissionLabel {
    MissionLabel::new(text).expect("test label is valid")
}

/// A path inside this checkout, resolved from the test crate's manifest.
pub(crate) fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The declared campaign inventory committed with this stage.
pub(crate) fn load_inventory() -> CampaignInventory {
    CampaignInventory::load(&repo_path("missions/bindings/campaign-inventory.tsv"))
        .expect("the declared campaign inventory loads")
}

/// One known content row.
pub(crate) fn row(role: &str, kind: ContentKind, key: &str) -> BindingRow {
    BindingRow::content(role, cid(kind, key), designed("f50.a.synthetic.row"))
        .expect("synthetic row role is valid")
}

/// One dependency row per required subsystem, all satisfied.
pub(crate) fn resolved_dependencies() -> Vec<SubsystemDependency> {
    REQUIRED_SUBSYSTEMS
        .iter()
        .map(|id| SubsystemDependency {
            subsystem: SubsystemId::new(id).expect("required subsystem id is valid"),
            state: DependencyState::resolved(designed("f50.a.synthetic.subsystem")),
        })
        .collect()
}

/// A fully bound synthetic mission: every required category present with
/// known rows, every required subsystem satisfied, progression recorded.
pub(crate) fn ready_binding(text: &str, title: &str, progression: Progression) -> MissionBinding {
    let slug = text.to_ascii_lowercase();
    let mut categories: BTreeMap<BindingCategory, CategoryState> = BTreeMap::new();

    categories.insert(
        BindingCategory::MissionIdentity,
        CategoryState::rows(vec![
            BindingRow::content(
                MISSION_ROLE,
                cid(ContentKind::Mission, &format!("{slug}-mission")),
                designed("f50.a.synthetic.identity"),
            )
            .expect("identity role is valid"),
            BindingRow::content(
                WORLD_ROLE,
                cid(ContentKind::World, &format!("{slug}-world")),
                designed("f50.a.synthetic.identity"),
            )
            .expect("identity role is valid"),
            BindingRow::content(
                PROGRAM_ROLE,
                cid(ContentKind::Script, &format!("{slug}-program")),
                designed("f50.a.synthetic.identity"),
            )
            .expect("identity role is valid"),
        ])
        .expect("identity rows are present"),
    );
    categories.insert(
        BindingCategory::Actors,
        CategoryState::rows(vec![
            row("actor", ContentKind::Pilot, &format!("{slug}-pilot")),
            row(
                "forced_airframe",
                ContentKind::Airframe,
                &format!("{slug}-airframe"),
            ),
        ])
        .expect("actor rows are present"),
    );
    categories.insert(
        BindingCategory::AssetsMedia,
        CategoryState::rows(vec![row(
            "media",
            ContentKind::Video,
            &format!("{slug}-briefing"),
        )])
        .expect("media rows are present"),
    );
    categories.insert(
        BindingCategory::Objectives,
        CategoryState::rows(vec![
            row(
                "objective",
                ContentKind::Objective,
                &format!("{slug}-objective"),
            ),
            row("branch", ContentKind::Objective, &format!("{slug}-branch")),
            row(
                "failure_cause",
                ContentKind::Trigger,
                &format!("{slug}-failure"),
            ),
        ])
        .expect("objective rows are present"),
    );
    categories.insert(
        BindingCategory::Interactions,
        CategoryState::rows(vec![row(
            "interaction",
            ContentKind::Route,
            &format!("{slug}-pickup"),
        )])
        .expect("interaction rows are present"),
    );
    categories.insert(
        BindingCategory::RewardsProgression,
        CategoryState::rows(vec![
            row(
                "reward",
                ContentKind::ScrapbookItem,
                &format!("{slug}-reward"),
            ),
            row(
                "progression",
                ContentKind::Mission,
                &format!("{slug}-next-step"),
            ),
        ])
        .expect("reward rows are present"),
    );
    categories.insert(
        BindingCategory::ReferenceEvidence,
        CategoryState::rows(vec![
            BindingRow::evidence(
                "evidence",
                claim("f50.a.synthetic.evidence"),
                designed("f50.a.synthetic.evidence"),
            )
            .expect("evidence row is valid"),
        ])
        .expect("evidence rows are present"),
    );

    MissionBinding {
        label: label(text),
        discovery_title: Some(title.to_owned()),
        categories,
        dependencies: resolved_dependencies(),
        progression,
        placeholder: false,
    }
}

/// Records every binding and puts every one of them in the denominator.
pub(crate) fn declared(bindings: Vec<MissionBinding>) -> CampaignBindings {
    let mut campaign = CampaignBindings::new();
    for binding in bindings {
        let mission = binding.label.clone();
        campaign
            .insert(binding)
            .expect("synthetic binding is valid");
        campaign
            .declare(&mission)
            .expect("synthetic binding joins the denominator");
    }
    campaign
}

/// A three-mission synthetic campaign in which every cell is complete:
/// `M01 -> M02 -> M03`, the last mission ending the chain.
pub(crate) fn ready_campaign() -> CampaignBindings {
    declared(vec![
        ready_binding(
            "M01",
            "Synthetic One",
            Progression::known(vec![label("M02")]),
        ),
        ready_binding(
            "M02",
            "Synthetic Two",
            Progression::known(vec![label("M03")]),
        ),
        ready_binding("M03", "Synthetic Three", Progression::known(Vec::new())),
    ])
}
