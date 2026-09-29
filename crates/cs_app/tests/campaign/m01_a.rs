//! Acceptance stage M01-A: bind the first mission's original data and
//! branches (`missions/M01.md`, work order `M01-A`).
//!
//! The stage's minimum scenario is "Source-derived binding has no unresolved
//! critical dependencies", and that is exactly what
//! [`accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies`]
//! asks production code: [`SourceContext::read`] fingerprints `$CS_GAME_DIR`
//! and reads its campaign layout and localized string table,
//! [`SourceContext::bind`] resolves the five critical dependencies of the
//! M01 data-binding checklist, and nothing else in the record is allowed to
//! read as finished.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which
//! has no original data) skips them; they are run with `--include-ignored`
//! by the implementing and reviewing agents. Every assertion below is made
//! against facts the test re-reads from the installation or from committed
//! records — never against a constant that repeats the implementation.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    BindingCategory, CampaignBindings, CategoryState, CellState, CriticalDependency,
    DependencyState, SourceBinding, SourceContext,
};
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::ClaimId;

use crate::common::{label, load_inventory, repo_path};

/// The one work order this stage binds.
const WORK_ORDER: &str = "M01";

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M01-A needs the retail capability; run this suite with \
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

/// The declared discovery title of `M01`, read from the committed inventory
/// rather than repeated here.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == WORK_ORDER)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {WORK_ORDER} work order"))
}

/// The M01 binding derived from the installation, built once.
fn binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        context()
            .bind(label(WORK_ORDER), &discovery_title())
            .expect("M01 binds to the original data")
    })
}

/// The declared campaign, with M01 bound the way the record describes.
fn campaign() -> CampaignBindings {
    let mut campaign = CampaignBindings::from_inventory(&load_inventory())
        .expect("the declared inventory builds the campaign");
    campaign
        .bind(binding().to_mission_binding().expect("record is valid"))
        .expect("M01 is a declared mission");
    campaign
}

// ---------------------------------------------------------------- retail ---

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies() {
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
    // was obtained: the installation hash, the localized title and the
    // program archive were observed through a tool run, while the join from
    // the work-order title to a retail mission directory is an inference.
    for dependency in &binding.dependencies {
        let DependencyState::Resolved { provenance } = &dependency.state else {
            panic!("{} is not resolved", dependency.id);
        };
        let expected = match dependency.id {
            CriticalDependency::MissionId | CriticalDependency::WorldGroupVariant => {
                cs_types::evidence::ClaimStatus::Inferred
            }
            CriticalDependency::InstallHash
            | CriticalDependency::TitleString
            | CriticalDependency::ProgramSourceMap => cs_types::evidence::ClaimStatus::ObservedTool,
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

    // The campaign position is real: the directory layout it selects exists
    // on disk, with the reader archive the program id names.
    let campaign = context().campaign();
    assert_eq!(
        campaign.len(),
        24,
        "the retail campaign declares 24 missions"
    );
    let position = binding.campaign_position.expect("a position was resolved");
    assert_eq!(position, 0, "M01 is the first mission of the campaign");
    assert_eq!(binding.campaign_size, campaign.len());
    let entry = &campaign[position];
    let program_path = game_dir().join(&entry.program_asset);
    assert!(
        program_path.is_file(),
        "the program archive {} the record cites does not exist",
        entry.program_asset
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
fn accept_m01_a_the_committed_record_is_what_the_installation_derives() {
    let committed = fs::read_to_string(repo_path("missions/bindings/M01.json"))
        .expect("missions/bindings/M01.json exists");
    let derived = binding().to_json();
    assert_eq!(
        committed, derived,
        "the committed binding record is not what production code derives from $CS_GAME_DIR"
    );

    // The record is the schema's shape, with the values that are still
    // unknown kept explicit instead of silently omitted.
    for key in [
        "\"schema_version\": 1",
        "\"work_order\": \"M01\"",
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
fn accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved() {
    // A title that is not in the local strings must never be accepted: the
    // dependency stays unresolved and the identity that depends on it stays
    // unresolved with it, while the installation hash — which does not
    // depend on the title — stays resolved.
    let missing = context()
        .bind(label(WORK_ORDER), "The Lost Treasur")
        .expect("a title miss is a recorded unknown, not a failure");
    missing
        .validate()
        .expect("the partial record is internally consistent");
    let unresolved = missing.unresolved_critical();
    assert!(
        unresolved.contains(&CriticalDependency::TitleString),
        "a title the local strings do not carry was reported as resolved: {unresolved:?}"
    );
    assert!(
        unresolved.contains(&CriticalDependency::MissionId),
        "a mission id derived from an unconfirmed title was reported as resolved"
    );
    assert!(
        !unresolved.contains(&CriticalDependency::InstallHash),
        "the installation hash does not depend on the title"
    );
    assert_eq!(missing.localized_title_id, None);
    assert_eq!(missing.catalog_id, None);
    assert!(!missing.is_verified());
    assert!(
        missing.source_spans.is_empty(),
        "an unresolved binding must not cite a source span it never resolved"
    );

    // The campaign record built from it does not read as bound either.
    let mission_binding = missing
        .to_mission_binding()
        .expect("an incomplete record still builds a valid mission record");
    assert_eq!(
        mission_binding.category(BindingCategory::MissionIdentity),
        Some(&CategoryState::Unresolved {
            claim_id: ClaimId::new("m01.source_binding").expect("claim id is valid"),
            reason: "the source-derived mission identity is incomplete".to_owned(),
        }),
        "an unresolved identity must not be recorded as a complete cell"
    );
    assert!(mission_binding.cells().any(|(category, state)| category
        == BindingCategory::MissionIdentity
        && state == CellState::Unknown));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_a_the_campaign_keeps_everything_else_unresolved_and_unready() {
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

    let m01 = campaign.get(&label(WORK_ORDER)).expect("M01 is recorded");
    assert!(
        !m01.is_placeholder(),
        "M01 was bound, not left a placeholder"
    );
    let states: Vec<(BindingCategory, CellState)> = m01.cells().collect();
    for (category, state) in states {
        let expected = if category == BindingCategory::MissionIdentity {
            CellState::Complete
        } else {
            CellState::Unknown
        };
        assert_eq!(state, expected, "category {category} has the wrong state");
    }
    assert!(
        m01.dependencies
            .iter()
            .all(|row| matches!(row.state, DependencyState::Unresolved { .. }))
    );

    let closure = campaign
        .closure(&label(WORK_ORDER), None)
        .expect("M01's closure computes without a catalog");
    assert_eq!(closure.reached, vec![label(WORK_ORDER)]);
    assert_eq!(closure.cell_count(), 7);
    assert_eq!(closure.complete_cells(), 1);
    assert_eq!(closure.unresolved_subsystems, 23);
    assert_eq!(closure.unknown_progression, 1);
    assert!(!closure.is_complete());
}

// -------------------------------------------------------------- synthetic ---

/// A source binding with every critical dependency resolved and nothing
/// unknown, built here from authored values. It exists only to prove the
/// *predicates*: a record shaped like this must read as verified, so neither
/// [`SourceBinding::is_verified`] nor [`SourceBinding::unresolved_critical`]
/// can be a constant.
fn authored_binding() -> SourceBinding {
    let claim = |suffix: &str| ClaimId::new(&format!("m01.a.synthetic.{suffix}")).expect("claim");
    let provenance = |suffix: &str| {
        Provenance::new(
            claim(suffix),
            cs_types::evidence::ClaimStatus::Designed,
            None,
        )
        .expect("designed provenance always validates")
    };
    SourceBinding {
        label: label(WORK_ORDER),
        discovery_title: "Authored Title".to_owned(),
        install_sha256: "0".repeat(64),
        campaign_position: Some(0),
        campaign_size: 1,
        catalog_id: Some(ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("id")),
        world_id: Some(ContentId::from_source(ContentKind::World, "c1c").expect("id")),
        program_id: Some(ContentId::from_source(ContentKind::Script, "c1c-m01-zrdr").expect("id")),
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
        closure_sha256: Some("f".repeat(64)),
        evidence_ids: vec![claim("evidence")],
        unknowns: Vec::new(),
    }
}

#[test]
fn accept_m01_a_verified_and_unresolved_are_two_distinct_states() {
    let mut complete = authored_binding();
    complete
        .validate()
        .expect("the authored record is internally consistent");
    assert_eq!(complete.unresolved_critical(), Vec::new());
    assert!(
        complete.is_verified(),
        "a record with every critical dependency resolved, no unknowns, a closure hash and \
         evidence must read as verified; otherwise the predicate is a constant"
    );

    // One unresolved critical dependency is enough to stop it — and the
    // record must report *which* one.
    let title = complete
        .dependencies
        .iter_mut()
        .find(|dependency| dependency.id == CriticalDependency::TitleString)
        .expect("the authored record carries a title dependency");
    title.state = DependencyState::unresolved(
        ClaimId::new("m01.a.synthetic.title_string").expect("claim"),
        "the authored title has no local string",
    )
    .expect("reason is non-empty");
    complete.localized_title_id = None;

    assert_eq!(
        complete.unresolved_critical(),
        vec![CriticalDependency::TitleString]
    );
    assert!(
        !complete.is_verified(),
        "a record with an unresolved critical dependency must not read as verified"
    );
    complete
        .validate()
        .expect("an explicitly unresolved dependency is still a consistent record");

    // Dropping the value without dropping the resolution is a contradiction,
    // and the record refuses it rather than agreeing with itself.
    let mut contradicted = authored_binding();
    contradicted.catalog_id = None;
    assert!(
        contradicted.validate().is_err(),
        "a resolved mission id with no mission id must be refused"
    );

    // Clearing the checklist's unknowns while a critical dependency is
    // unresolved must not be enough to read as verified either.
    let mut lies = authored_binding();
    lies.unknowns.clear();
    let install = lies
        .dependencies
        .iter_mut()
        .find(|dependency| dependency.id == CriticalDependency::InstallHash)
        .expect("the authored record carries an installation dependency");
    install.state = DependencyState::unresolved(
        ClaimId::new("m01.a.synthetic.installation_hash").expect("claim"),
        "no installation was read",
    )
    .expect("reason is non-empty");
    assert!(!lies.is_verified());
}

#[test]
fn accept_m01_a_the_critical_dependency_set_is_the_checklists_first_five() {
    // The set is data, not an adjective: it must stay exactly the five
    // entries the mission sheets list first, so a stage cannot quietly make
    // the acceptance scenario easier by dropping one.
    assert_eq!(
        CriticalDependency::ALL
            .iter()
            .map(|id| id.label())
            .collect::<Vec<_>>(),
        vec![
            "mission_id",
            "installation_hash",
            "title_string",
            "program_source_map",
            "world_group_variant",
        ]
    );
    let unique: BTreeSet<&str> = CriticalDependency::ALL
        .iter()
        .map(|id| id.label())
        .collect();
    assert_eq!(unique.len(), CriticalDependency::ALL.len());
}
