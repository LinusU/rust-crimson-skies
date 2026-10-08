//! Acceptance stage F50-B: bind the discovered campaign identities and run
//! the prerequisite closures over the whole declared campaign
//! (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
//! section `### F50-B`).
//!
//! F50-A defined the records but bound nothing: every identity was
//! `Resolved::Unknown`, every cell was a placeholder and no closure walked a
//! bound campaign. This stage is the production path that fills them from the
//! installation — [`SourceContext::bind_campaign`] reads `$CS_GAME_DIR` once,
//! binds every work order the frozen denominator declares, and
//! [`assemble_campaign`] refuses every way a declared work order could
//! silently leave the record. The retail tests then hold the assembled
//! campaign to the two things the stage is judged on:
//!
//! * **discovered campaign identities** — every declared work order is
//!   recorded under one installation fingerprint, an identity the
//!   installation resolves is a complete cell, and an identity it does not
//!   resolve stays an explicitly unknown cell that names its refusal. No
//!   mission is dropped and none is guessed at;
//! * **prerequisite closures** — one closure per declared mission, each
//!   accounting for its seven required cells and all twenty-three
//!   [`REQUIRED_SUBSYSTEMS`] rows, together reaching every declared mission.
//!
//! The five synthetic tests (unignored, so CI runs them) exercise the
//! assembly rule on authored values, including its refusals. The four retail
//! tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which has no
//! original data) skips them; the implementing and reviewing agents run them
//! with `--include-ignored`.
//!
//! What this stage does **not** claim, recorded here because a walk that ends
//! at M24 can read like more than it is: the *campaign progression* — which
//! mission succeeds which — lives in original scripts nobody has measured, so
//! every assembled mission keeps `Progression::Unknown` and
//! `CoverageReport::is_ready` stays false. The walk below is the declared
//! work-order order under one profile; playing those missions in progression
//! order is F50-C's probes and F50-D's ordinary-play evidence.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_assets::install::{discover, fingerprint};
use cs_content::campaign_bindings::{
    BindingCategory, BoundCampaign, CampaignBindings, CampaignInventory, CellState,
    CriticalDependency, DependencyState, MissionLabel, REQUIRED_SUBSYSTEMS, SourceBinding,
    SourceBindingError, SourceContext, SourceDependency, assemble_campaign, cell_state,
};
use cs_types::content::{ContentId, ContentKind, Provenance};

use crate::common::{claim, label, load_inventory};

/// The synthetic installation fingerprint the authored records below carry:
/// sixty-four lowercase hex digits, so a record built here is not refused for
/// a fingerprint it was never given.
const SYNTHETIC_INSTALL: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: F50-B needs the retail capability; run this suite with \
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

/// The whole campaign bound from the installation, built once.
fn bound() -> &'static BoundCampaign {
    static BOUND: OnceLock<BoundCampaign> = OnceLock::new();
    BOUND.get_or_init(|| {
        context()
            .bind_campaign(&load_inventory())
            .expect("the declared campaign binds to the original data")
    })
}

// ---------------------------------------------------------- synthetic ---

/// One authored source binding for the three-mission denominator below.
///
/// `identities` is `(mission, world, program)` when the installation would
/// have located all three, and [`None`] when it would have located none —
/// exactly the two states a real work order lands in.
fn synthetic_source(
    work_order: &str,
    title: &str,
    identities: Option<(&str, &str, &str)>,
) -> SourceBinding {
    let dependencies = CriticalDependency::ALL
        .iter()
        .map(|id| SourceDependency {
            id: *id,
            state: DependencyState::resolved(Provenance::designed(claim(&format!(
                "f50.b.synthetic.{work_order}.{}",
                id.label()
            )))),
        })
        .collect();
    let (mission, world, program) = match identities {
        Some((mission, world, program)) => (
            Some(
                ContentId::from_source(ContentKind::Mission, mission)
                    .expect("synthetic mission id is valid"),
            ),
            Some(
                ContentId::from_source(ContentKind::World, world)
                    .expect("synthetic world id is valid"),
            ),
            Some(
                ContentId::from_source(ContentKind::Script, program)
                    .expect("synthetic program id is valid"),
            ),
        ),
        None => (None, None, None),
    };
    SourceBinding {
        label: label(work_order),
        discovery_title: title.to_owned(),
        install_sha256: SYNTHETIC_INSTALL.to_owned(),
        campaign_position: identities.map(|_| 0),
        campaign_size: 3,
        catalog_id: mission,
        world_id: world,
        program_id: program,
        localized_title_id: identities.map(|_| 3450),
        localized_title_language: identities.map(|_| 1033),
        dependencies,
        source_spans: Vec::new(),
        identity_source: None,
        title_source: None,
        title_enclosure: None,
        closure_sha256: None,
        evidence_ids: Vec::new(),
        unknowns: if identities.is_none() {
            vec!["the authored identity is deliberately absent".to_owned()]
        } else {
            Vec::new()
        },
    }
}

/// The three-mission authored denominator the synthetic tests assemble over.
fn synthetic_inventory() -> CampaignInventory {
    CampaignInventory::parse(
        "# a synthetic denominator, authored by this file\n\
         M01\tSynthetic One\n\
         M02\tSynthetic Two\n\
         M03\tSynthetic Three\n",
    )
    .expect("the authored inventory parses")
}

/// Every declared work order is recorded exactly once, a work order whose
/// identity the installation would not resolve is recorded with an
/// explicitly unknown identity instead of being dropped, and the totals still
/// account for every `(mission, category)` cell. A missing work order or a
/// cell that vanished from the totals fails the assertions below — the
/// "missing stays red, no filtering to the working subset" rule of spec F50
/// non-negotiable behavior 5, on the assembly path rather than on the schema.
#[test]
fn accept_f50_b_a_declared_campaign_assembles_one_record_per_work_order() {
    let inventory = synthetic_inventory();
    let sources = vec![
        synthetic_source(
            "M01",
            "Synthetic One",
            Some((
                "synthetic-one",
                "synthetic-one-world",
                "synthetic-one-program",
            )),
        ),
        synthetic_source("M02", "Synthetic Two", None),
        synthetic_source(
            "M03",
            "Synthetic Three",
            Some((
                "synthetic-three",
                "synthetic-three-world",
                "synthetic-three-program",
            )),
        ),
    ];

    let campaign = assemble_campaign(&inventory, &sources)
        .expect("every declared work order was supplied a binding");

    assert_eq!(campaign.len(), 3, "one record per declared work order");
    assert_eq!(campaign.declared_count(), 3, "the denominator is intact");
    for work_order in inventory.labels() {
        let recorded = campaign
            .get(work_order)
            .unwrap_or_else(|| panic!("{work_order} is missing from the assembled campaign"));
        assert!(
            !recorded.is_placeholder(),
            "{work_order} was declared but never bound"
        );
    }

    let coverage = campaign.coverage();
    assert_eq!(coverage.declared_missions, 3);
    assert_eq!(coverage.total_missions, 3);
    assert_eq!(
        coverage.discovered_extra(),
        0,
        "nothing crept in beside the denominator"
    );
    assert_eq!(coverage.cells, 3 * BindingCategory::ALL.len());
    assert_eq!(
        coverage.complete_cells, 2,
        "only the two work orders with a complete identity have a complete cell"
    );
    assert_eq!(
        coverage.missing_cells, 0,
        "no cell may disappear from the totals: a category nobody recorded is missing, never unused"
    );
    assert_eq!(
        coverage.unknown_cells,
        coverage.cells - coverage.complete_cells,
        "every non-complete cell is an explicit unknown"
    );
    assert_eq!(coverage.subsystem_rows, 3 * REQUIRED_SUBSYSTEMS.len());
    assert_eq!(coverage.subsystem_unresolved, coverage.subsystem_rows);
    assert_eq!(
        coverage.progression_unknown, 3,
        "progression is not measured here"
    );
    assert!(
        !coverage.is_ready(),
        "a campaign whose identities are partly unknown and whose subsystems are all \
         unresolved is never ready"
    );

    // The work order whose identity is absent is unknown with a reason, not
    // missing and not complete.
    let unbound = campaign.get(&label("M02")).expect("M02 is recorded");
    assert_eq!(
        cell_state(
            BindingCategory::MissionIdentity,
            unbound.category(BindingCategory::MissionIdentity)
        ),
        CellState::Unknown,
        "an identity the installation does not resolve stays an explicit unknown cell"
    );
    assert_eq!(
        unbound.catalog_identity(),
        None,
        "a discovery label never stands in for a retail identity"
    );
    assert_eq!(
        cell_state(
            BindingCategory::MissionIdentity,
            campaign
                .get(&label("M01"))
                .expect("M01 is recorded")
                .category(BindingCategory::MissionIdentity)
        ),
        CellState::Complete,
        "an identity that did resolve is a complete cell"
    );
}

/// The assembled campaign is ordered by the frozen denominator, not by the
/// order bindings arrive in: reversing the input changes nothing a reader can
/// observe. A record that kept arrival order would reorder every report built
/// on top of it.
#[test]
fn accept_f50_b_the_assembled_campaign_is_ordered_by_the_denominator_not_by_arrival() {
    let inventory = synthetic_inventory();
    let sources = vec![
        synthetic_source(
            "M01",
            "Synthetic One",
            Some((
                "synthetic-one",
                "synthetic-one-world",
                "synthetic-one-program",
            )),
        ),
        synthetic_source(
            "M02",
            "Synthetic Two",
            Some((
                "synthetic-two",
                "synthetic-two-world",
                "synthetic-two-program",
            )),
        ),
        synthetic_source(
            "M03",
            "Synthetic Three",
            Some((
                "synthetic-three",
                "synthetic-three-world",
                "synthetic-three-program",
            )),
        ),
    ];
    let reversed: Vec<SourceBinding> = sources.iter().rev().cloned().collect();

    let in_order = assemble_campaign(&inventory, &sources).expect("the forward assembly succeeds");
    let backwards = assemble_campaign(&inventory, &reversed)
        .expect("the reversed assembly succeeds just as well");

    let ordered = |campaign: &CampaignBindings| {
        campaign
            .missions()
            .map(|mission| mission.label.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    let roots = |campaign: &CampaignBindings| {
        campaign
            .closures(None)
            .expect("the synthetic campaign has no progression edges to fail on")
            .into_iter()
            .map(|report| report.root.as_str().to_owned())
            .collect::<Vec<_>>()
    };

    assert_eq!(ordered(&in_order), vec!["M01", "M02", "M03"]);
    assert_eq!(
        ordered(&backwards),
        ordered(&in_order),
        "arrival order must not leak into the recorded order"
    );
    assert_eq!(
        roots(&backwards),
        roots(&in_order),
        "closure roots follow the denominator, not the input"
    );
}

/// A binding for a work order the denominator does not declare is refused
/// with the offending label, instead of growing the denominator without a
/// `declare` call. Silent growth would let a campaign report totals nobody
/// froze.
#[test]
fn accept_f50_b_a_work_order_the_inventory_does_not_declare_is_refused() {
    let inventory = synthetic_inventory();
    let sources = vec![
        synthetic_source(
            "M01",
            "Synthetic One",
            Some((
                "synthetic-one",
                "synthetic-one-world",
                "synthetic-one-program",
            )),
        ),
        synthetic_source(
            "M02",
            "Synthetic Two",
            Some((
                "synthetic-two",
                "synthetic-two-world",
                "synthetic-two-program",
            )),
        ),
        synthetic_source(
            "M03",
            "Synthetic Three",
            Some((
                "synthetic-three",
                "synthetic-three-world",
                "synthetic-three-program",
            )),
        ),
        synthetic_source(
            "M99",
            "Undeclared",
            Some(("undeclared", "undeclared-world", "undeclared-program")),
        ),
    ];

    let error = assemble_campaign(&inventory, &sources)
        .expect_err("a work order outside the denominator must be refused");
    match error {
        SourceBindingError::Inconsistent { reason } => {
            assert!(
                reason.contains("M99"),
                "the refusal must name the offending work order, got: {reason}"
            );
            assert!(
                reason.contains("denominator"),
                "the refusal must say what would have gone wrong, got: {reason}"
            );
        }
        other => panic!("expected an inconsistency refusal, got {other:?}"),
    }
}

/// Two bindings for one work order never merge: the placeholder is bound
/// exactly once, and a second record with the same identity is refused rather
/// than silently winning.
#[test]
fn accept_f50_b_a_repeated_work_order_is_refused() {
    let inventory = synthetic_inventory();
    let one = synthetic_source(
        "M01",
        "Synthetic One",
        Some((
            "synthetic-one",
            "synthetic-one-world",
            "synthetic-one-program",
        )),
    );
    let sources = vec![
        one.clone(),
        synthetic_source(
            "M02",
            "Synthetic Two",
            Some((
                "synthetic-two",
                "synthetic-two-world",
                "synthetic-two-program",
            )),
        ),
        synthetic_source(
            "M03",
            "Synthetic Three",
            Some((
                "synthetic-three",
                "synthetic-three-world",
                "synthetic-three-program",
            )),
        ),
        one,
    ];

    let error = assemble_campaign(&inventory, &sources)
        .expect_err("the second binding for M01 must be refused");
    match error {
        SourceBindingError::Inconsistent { reason } => {
            assert!(
                reason.contains("M01") && reason.contains("more than one"),
                "the refusal must name the repeated work order, got: {reason}"
            );
        }
        other => panic!("expected an inconsistency refusal, got {other:?}"),
    }
}

/// A declared work order supplied no binding is refused. If it were not, the
/// placeholder would survive assembly and read as an ordinary unresolved
/// mission — indistinguishable from a mission whose identity the installation
/// merely does not carry, and therefore not a missing input but a silent one.
#[test]
fn accept_f50_b_a_declared_work_order_with_no_binding_is_refused() {
    let inventory = synthetic_inventory();
    let sources = vec![
        synthetic_source(
            "M01",
            "Synthetic One",
            Some((
                "synthetic-one",
                "synthetic-one-world",
                "synthetic-one-program",
            )),
        ),
        synthetic_source(
            "M03",
            "Synthetic Three",
            Some((
                "synthetic-three",
                "synthetic-three-world",
                "synthetic-three-program",
            )),
        ),
    ];

    let error = assemble_campaign(&inventory, &sources)
        .expect_err("the declared M02 must not be left a placeholder");
    match error {
        SourceBindingError::Inconsistent { reason } => {
            assert!(
                reason.contains("M02") && reason.contains("no source binding"),
                "the refusal must name the work order that was left out, got: {reason}"
            );
        }
        other => panic!("expected an inconsistency refusal, got {other:?}"),
    }
}

// ---------------------------------------------------------------- retail ---

/// Every work order the denominator declares is bound from the installation
/// by one production call, under one installation fingerprint, and the
/// identities that call located are exactly the identities the campaign
/// records as complete.
///
/// The equivalence asserted per work order is the stage's core claim:
/// `SourceBinding::unresolved_critical().is_empty()` (the installation
/// located the mission id, the world group and the program archive) if and
/// only if the recorded `mission_identity` cell is [`CellState::Complete`].
/// A record that read a cell complete while its own critical dependencies are
/// open — or that dropped a work order it could not resolve — fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_b_the_whole_campaign_binds_from_one_installation() {
    let inventory = load_inventory();
    let campaign = bound();

    assert_eq!(
        campaign.sources.len(),
        inventory.len(),
        "one source binding per declared work order"
    );
    assert_eq!(
        campaign.bindings.len(),
        inventory.len(),
        "one recorded mission per declared work order"
    );
    assert_eq!(
        campaign.bindings.declared_count(),
        inventory.len(),
        "the frozen denominator survived assembly intact"
    );

    let expected_install = fingerprint(
        &discover(&game_dir())
            .expect("production discovery")
            .manifest,
    )
    .to_hex();
    assert_eq!(
        context().install_sha256(),
        expected_install,
        "the context the campaign was bound under is production discovery of this installation"
    );

    let mut resolved = BTreeSet::new();
    let mut unresolved = BTreeSet::new();
    for (work_order, source) in inventory.iter().zip(&campaign.sources) {
        assert_eq!(
            source.label, work_order.0,
            "sources must follow the declared inventory order so a caller can pair them"
        );
        assert_eq!(
            source.install_sha256, expected_install,
            "{} was bound under a different installation fingerprint than the rest",
            source.label
        );
        source
            .validate()
            .expect("every derived record is internally consistent");

        let recorded = campaign
            .bindings
            .get(&source.label)
            .unwrap_or_else(|| panic!("{} is missing from the campaign", source.label));
        let identity_cell = cell_state(
            BindingCategory::MissionIdentity,
            recorded.category(BindingCategory::MissionIdentity),
        );
        if source.unresolved_critical().is_empty() {
            resolved.insert(source.label.clone());
            assert_eq!(
                identity_cell,
                CellState::Complete,
                "{} located every identity, so its identity cell must be complete",
                source.label
            );
            assert_eq!(
                recorded.catalog_identity().map(ContentId::as_str),
                source.catalog_id.as_ref().map(ContentId::as_str),
                "{}'s recorded mission id is the one the source binding located",
                source.label
            );
        } else {
            unresolved.insert(source.label.clone());
            assert_eq!(
                identity_cell,
                CellState::Unknown,
                "{} left {:?} unresolved, so its identity cell must stay explicitly unknown",
                source.label,
                source.unresolved_critical()
            );
            assert_eq!(
                recorded.catalog_identity(),
                None,
                "{} located no mission id, so none may appear in the record",
                source.label
            );
        }
    }

    assert!(
        !resolved.is_empty(),
        "no work order resolved an identity: this installation would have to be re-measured \
         before this stage means anything"
    );
    assert_eq!(
        resolved.len() + unresolved.len(),
        inventory.len(),
        "every declared work order is either bound or explicitly unbound"
    );
}

/// The prerequisite closures of the bound campaign account for everything:
/// one closure per declared mission, each holding its seven required cells
/// and all twenty-three prerequisite rows, together reaching every declared
/// mission exactly once as roots and once in the union of what is reached.
///
/// This is AC01's "run all mission dependency closures and assert none are
/// silently omitted", measured on records the installation produced rather
/// than on authored placeholders.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_b_the_prerequisite_closures_of_the_bound_campaign_omit_nothing() {
    let inventory = load_inventory();
    let campaign = &bound().bindings;

    let reports = campaign
        .closures(None)
        .expect("the bound campaign records no progression edges to fail on");
    assert_eq!(
        reports.len(),
        inventory.len(),
        "every declared mission gets its own closure"
    );

    let mut roots = BTreeSet::new();
    let mut reached = BTreeSet::new();
    let mut cells = 0;
    let mut subsystem_rows = 0;
    for report in &reports {
        roots.insert(report.root.clone());
        reached.extend(report.reached.iter().cloned());
        cells += report.cell_count();
        subsystem_rows += report.subsystem_rows;

        assert_eq!(
            report.reached.len(),
            1,
            "no progression is bound, so closure {} reaches itself only",
            report.root
        );
        assert_eq!(
            report.cell_count(),
            BindingCategory::ALL.len(),
            "closure {} accounts for every required category",
            report.root
        );
        for category in BindingCategory::ALL {
            let state = report
                .cells
                .iter()
                .find(|(mission, cell_category, _)| {
                    mission == &report.root && cell_category == category
                })
                .map(|(_, _, state)| *state)
                .unwrap_or_else(|| {
                    panic!(
                        "closure of {} omits its {} cell",
                        report.root,
                        category.label()
                    )
                });
            let recorded = campaign
                .get(&report.root)
                .expect("the root is recorded")
                .category(*category);
            assert_eq!(
                state,
                cell_state(*category, recorded),
                "closure of {} reports a {} cell its record does not hold",
                report.root,
                category.label()
            );
        }
        assert_eq!(
            report.subsystem_rows,
            REQUIRED_SUBSYSTEMS.len(),
            "every prerequisite subsystem row is in the closure of {}",
            report.root
        );
        assert_eq!(
            report.unresolved_subsystems, report.subsystem_rows,
            "no prerequisite subsystem is implemented by this stage"
        );
        assert_eq!(report.unsupported_subsystems, 0);
        assert_eq!(report.unknown_progression, 1);
        assert!(
            !report.is_complete(),
            "a closure whose identities are partly unknown is never complete"
        );
    }

    assert_eq!(roots.len(), inventory.len(), "no root was served twice");
    assert_eq!(
        reached.len(),
        inventory.len(),
        "every declared mission is reached by some closure"
    );
    for work_order in inventory.labels() {
        assert!(
            reached.contains(work_order),
            "{work_order} is missing from every closure"
        );
    }
    assert_eq!(
        cells,
        inventory.len() * BindingCategory::ALL.len(),
        "the closures together account for every cell of the campaign"
    );
    assert_eq!(
        subsystem_rows,
        inventory.len() * REQUIRED_SUBSYSTEMS.len(),
        "the closures together account for every prerequisite row of the campaign"
    );
}

/// The identities the installation did not resolve stay unresolved, and the
/// campaign as a whole stays unready: nothing was filtered to the subset that
/// worked, no cell went missing, and readiness was not awarded by this stage.
///
/// The set of unresolved work orders is pinned rather than counted, because
/// "seven missions are unbound" would keep passing if a *different* seven
/// were unbound — the owner's denominator and this installation decide which
/// ones, and a change in either must fail here for a re-measurement.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_b_unresolved_identities_stay_unresolved_and_the_campaign_stays_unready() {
    let inventory = load_inventory();
    let campaign = &bound().bindings;
    let coverage = campaign.coverage();

    assert_eq!(coverage.declared_missions, inventory.len());
    assert_eq!(coverage.total_missions, inventory.len());
    assert_eq!(coverage.discovered_extra(), 0);
    assert_eq!(coverage.cells, inventory.len() * BindingCategory::ALL.len());
    assert_eq!(
        coverage.missing_cells, 0,
        "no required cell may be absent from the totals"
    );
    assert_eq!(
        coverage.complete_cells + coverage.unknown_cells,
        coverage.cells,
        "every cell is either complete or an explicit unknown"
    );
    assert_eq!(
        coverage.subsystem_rows,
        inventory.len() * REQUIRED_SUBSYSTEMS.len()
    );
    assert_eq!(
        coverage.subsystem_unresolved, coverage.subsystem_rows,
        "every prerequisite subsystem row is still unresolved"
    );
    assert_eq!(coverage.subsystem_resolved, 0);
    assert_eq!(
        coverage.progression_unknown,
        inventory.len(),
        "the campaign progression is unmeasured, so no mission may record a successor"
    );
    assert_eq!(coverage.progression_known, 0);
    assert!(
        !coverage.is_ready(),
        "this stage binds identities; it must never report the campaign ready"
    );

    let unresolved: Vec<&str> = bound()
        .sources
        .iter()
        .filter(|source| !source.unresolved_critical().is_empty())
        .map(|source| source.label.as_str())
        .collect();
    assert_eq!(
        unresolved,
        vec!["M09", "M11", "M14", "M15", "M20", "M22", "M23"],
        "the declared titles the installation carries in neither display form changed; \
         re-measure this installation before adjusting the expectation"
    );
    for work_order in &unresolved {
        let recorded = campaign.get(&label(work_order)).expect("it is recorded");
        assert!(
            !recorded.is_placeholder(),
            "{work_order} must be a bound record whose identity is unknown, not a placeholder"
        );
        assert_eq!(
            cell_state(
                BindingCategory::MissionIdentity,
                recorded.category(BindingCategory::MissionIdentity)
            ),
            CellState::Unknown
        );
        let source = bound()
            .sources
            .iter()
            .find(|source| source.label.as_str() == *work_order)
            .expect("the source binding is recorded");
        assert!(
            !source.unknowns.is_empty(),
            "{work_order} must still carry its unbound checklist entries rather than reading \
             as finished"
        );
    }
}

/// The declared campaign walked from its first work order to its last under
/// one profile, ending on the retail campaign's own final mission.
///
/// This is the binding-level shape of F50-B's minimum scenario ("play M01
/// through M24 in progression order with profile continuity and final
/// ending"): the walk covers every work order in declared order under the one
/// installation fingerprint the context was read with, and the last work order
/// binds the last position of the campaign the installation declares — so the
/// campaign the record describes has an end rather than trailing off. What it
/// does *not* claim is a played run or a measured successor relation; both
/// stay with F50-C and F50-D.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_b_the_campaign_is_walked_from_m01_to_m24_under_one_profile_and_ends_at_the_last_retail_position()
 {
    let inventory = load_inventory();
    let campaign = bound();

    let walk: Vec<MissionLabel> = inventory.labels().cloned().collect();
    assert_eq!(walk.len(), 24, "the campaign is twenty-four work orders");
    assert_eq!(walk.first().map(MissionLabel::as_str), Some("M01"));
    assert_eq!(walk.last().map(MissionLabel::as_str), Some("M24"));

    let mut fingerprints = BTreeSet::new();
    for work_order in &walk {
        let recorded = campaign.bindings.get(work_order).unwrap_or_else(|| {
            panic!("the walk reaches {work_order}, which the campaign does not record")
        });
        assert!(
            !recorded.is_placeholder(),
            "the walk reaches {work_order}, which assembly left unbound"
        );
        let source = campaign
            .sources
            .iter()
            .find(|source| &source.label == work_order)
            .unwrap_or_else(|| panic!("the walk reaches {work_order}, with no source binding"));
        fingerprints.insert(source.install_sha256.as_str());
    }
    assert_eq!(
        fingerprints.len(),
        1,
        "profile continuity: every work order of the walk carries one installation fingerprint"
    );

    let retail_campaign = context().campaign();
    assert_eq!(
        retail_campaign.len(),
        24,
        "the installation declares twenty-four campaign missions"
    );
    let last = campaign
        .sources
        .iter()
        .find(|source| source.label.as_str() == "M24")
        .expect("M24 is bound");
    assert_eq!(
        last.campaign_position,
        Some(retail_campaign.len() - 1),
        "the last work order binds the installation's last campaign position, so the campaign \
         the record describes ends there"
    );
    assert!(
        last.unresolved_critical().is_empty(),
        "the campaign's final position must be identified: {:?}",
        last.unresolved_critical()
    );
}
