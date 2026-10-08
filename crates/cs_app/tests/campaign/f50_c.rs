//! Acceptance stage F50-C: the per-mission probe routes and the human
//! playtest route document (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
//! section `### F50-C`).
//!
//! F50-B bound the campaign's identities but planned no route: nothing said
//! how a mission is *re-entered* after it ends. This stage is the retry
//! contract of spec F50 acceptance test AC03 — "retry selected missions after
//! death, bailout, skip-media, save/restart and settings changes" — as
//! production vocabulary ([`probe_routes`], [`ProbeInterruption::ALL`]) and
//! as one route per declared work order of the real campaign, plus
//! `missions/bindings/playtest-routes.md`, the per-work-order document a
//! human playtest follows. The retail tests hold the plan to three things:
//!
//! * **one route per declared work order** — the denominator's frozen shape
//!   carried through, an identity the installation did not resolve a *refused*
//!   route that stays counted and named, never a route silently missing;
//! * **reentry after every AC03 interruption** — a fresh production pass over
//!   the installation stands for the restart after an interruption, and every
//!   ready route re-enters the same mission, world and program under the same
//!   fingerprint through every one of the five interruptions;
//! * **the human route document** — `missions/bindings/playtest-routes.md`
//!   lists exactly the planned routes, so the document a playtester follows
//!   cannot drift from what production derived.
//!
//! The six synthetic tests (unignored, so CI runs them) exercise the
//! planning rule and its refusals on authored values. The three retail tests
//! are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which has no original
//! data) skips them; the implementing and reviewing agents run them with
//! `--include-ignored`.
//!
//! What this stage does **not** claim, recorded here because a planned route
//! can read like an executed one: **no mission is played**. Executing a route
//! needs the mission launch path (`VS-M01-RUNTIME`) and the controlled runs
//! (`VS-M01-CONTROLLED-RUNS`); ordinary-play evidence for every mission and
//! ending is F50-D. The plan is the contract those consumers drive, verified
//! here against the installation it was derived from — not a runtime death,
//! bailout or retry that anyone has observed.

use std::sync::OnceLock;

use cs_assets::install::{discover, fingerprint};
use cs_content::campaign_bindings::{
    BoundCampaign, CampaignBindings, CampaignInventory, ProbeInterruption, ProbePlan,
    SourceBinding, SourceContext, assemble_campaign, probe_routes,
};

use crate::common::{label, load_inventory, repo_path};
use crate::f50_b::{SYNTHETIC_INSTALL, bound, game_dir, synthetic_inventory, synthetic_source};

/// The human playtest route document this stage pins to the planned routes.
const PLAYTEST_ROUTES_DOC: &str = "missions/bindings/playtest-routes.md";

/// The whole campaign's probe plan, built once from the campaign F50-B bound.
fn plan() -> &'static ProbePlan {
    static PLAN: OnceLock<ProbePlan> = OnceLock::new();
    PLAN.get_or_init(|| probe_routes(bound()).expect("the retail campaign plans its probe routes"))
}

// ---------------------------------------------------------- synthetic ---

/// A second synthetic fingerprint, also canonical hex: a record carrying it
/// beside [`SYNTHETIC_INSTALL`] is a campaign read under two installations.
const OTHER_SYNTHETIC_INSTALL: &str =
    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

/// The three-mission authored denominator the synthetic tests plan over, with
/// one work order whose identity the installation would not resolve.
fn synthetic_campaign() -> (CampaignInventory, Vec<SourceBinding>) {
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
    (inventory, sources)
}

/// [`synthetic_campaign`]'s identities, re-fingerprinted under
/// [`SYNTHETIC_INSTALL`]: the assembled denominator a test can put a broken
/// `sources` list beside without the bindings disagreeing for the wrong
/// reason.
fn synthetic_bindings(
    inventory: &CampaignInventory,
    sources: &[SourceBinding],
) -> CampaignBindings {
    let normalized: Vec<SourceBinding> = sources
        .iter()
        .map(|source| {
            let mut source = source.clone();
            source.install_sha256 = SYNTHETIC_INSTALL.to_owned();
            source
        })
        .collect();
    assemble_campaign(inventory, &normalized).expect("the identities themselves are assemblable")
}

/// The scenario F50-C plans for is exactly AC03's five interruptions, under
/// stable identities a document and a probe runner can both name. A sixth
/// interruption or a renamed one would silently change the scenario every
/// route is held to, so the set itself is pinned.
#[test]
fn accept_f50_c_the_minimum_scenario_is_exactly_the_five_ac03_interruptions() {
    let labels: Vec<&str> = ProbeInterruption::ALL
        .iter()
        .map(|interruption| interruption.label())
        .collect();
    assert_eq!(
        labels,
        vec![
            "DEATH",
            "BAILOUT",
            "SKIP_MEDIA",
            "SAVE_RESTART",
            "SETTINGS_CHANGE"
        ],
        "the minimum acceptance scenario of F50-C is death, bailout, skip-media, save/restart \
         and settings changes, in that order"
    );
    for interruption in ProbeInterruption::ALL {
        assert!(
            !interruption.scenario().is_empty(),
            "{interruption} must say what it does to a running mission"
        );
    }
}

/// A work order whose identity the installation would not resolve is a
/// *refused* route that stays in the plan — named, counted and positioned —
/// while the other work orders plan every interruption. The refusal is the
/// stage's whole failure mode made visible: a plan that dropped M02 instead
/// would leave two routes over a three-mission denominator and read as
/// complete (spec F50 non-negotiable behavior 5, on the probe path).
#[test]
fn accept_f50_c_an_unresolved_identity_is_a_refused_route_that_stays_in_the_plan() {
    let (inventory, sources) = synthetic_campaign();
    let campaign = BoundCampaign {
        bindings: assemble_campaign(&inventory, &sources)
            .expect("every declared work order was supplied a binding"),
        sources,
    };
    let plan = probe_routes(&campaign).expect("the authored campaign plans its routes");

    assert_eq!(
        plan.len(),
        3,
        "one route per declared work order, none dropped"
    );
    assert_eq!(plan.ready_count(), 2);
    assert_eq!(plan.refused_count(), 1);
    assert_eq!(
        plan.routes()
            .iter()
            .map(|route| route.label.as_str())
            .collect::<Vec<_>>(),
        vec!["M01", "M02", "M03"],
        "the plan keeps the declared denominator order"
    );

    let refused = plan
        .route(&label("M02"))
        .expect("the refused route is still in the plan");
    assert!(
        !refused.is_ready(),
        "a route without a mission identity is never ready"
    );
    assert!(
        refused.reentries.is_empty(),
        "a refused route plans no reentry: there is no identity to re-enter"
    );
    for interruption in ProbeInterruption::ALL {
        assert_eq!(
            refused.reentry(interruption),
            None,
            "a refused route answers no reentry for {interruption}"
        );
    }
    let refusal = refused.refusal.as_ref().expect("the refusal is named");
    assert!(
        refusal.contains("M02"),
        "the refusal names the work order it refuses: {refusal}"
    );
    assert!(
        refusal.contains("mission_id"),
        "the refusal says what is missing: {refusal}"
    );

    // The ready routes plan exactly the minimum scenario, each interruption
    // landing on that work order's own identity.
    for (work_order, mission) in [
        ("M01", "mission/synthetic-one"),
        ("M03", "mission/synthetic-three"),
    ] {
        let route = plan
            .route(&label(work_order))
            .expect("the route is planned");
        assert!(route.is_ready(), "{work_order} resolved its identity");
        assert_eq!(
            route.reentries.len(),
            ProbeInterruption::ALL.len(),
            "{work_order} plans one reentry per interruption"
        );
        for interruption in ProbeInterruption::ALL {
            let reentry = route
                .reentry(interruption)
                .unwrap_or_else(|| panic!("{work_order} plans {interruption}"));
            assert_eq!(reentry.mission.as_str(), mission);
            assert_eq!(
                reentry.install_sha256, SYNTHETIC_INSTALL,
                "the reentry is anchored to the fingerprint the route was read under"
            );
        }
    }
}

/// A campaign whose bindings were read under two installation fingerprints is
/// refused outright: "save, restart, re-enter" cannot name two different
/// games for one campaign. The refusal names both work orders so the conflict
/// is debuggable rather than a bare `false`.
#[test]
fn accept_f50_c_a_campaign_read_under_two_fingerprints_is_refused() {
    let (inventory, mut sources) = synthetic_campaign();
    sources[2].install_sha256 = OTHER_SYNTHETIC_INSTALL.to_owned();
    let campaign = BoundCampaign {
        bindings: synthetic_bindings(&inventory, &sources),
        sources,
    };

    let error = probe_routes(&campaign).expect_err("two installations cannot anchor one plan");
    let message = error.to_string();
    for work_order in ["M01", "M03"] {
        assert!(
            message.contains(work_order),
            "the refusal names both conflicting work orders ({work_order} missing): {message}"
        );
    }
}

/// A declared work order with no source binding is refused by name: its route
/// would otherwise silently vanish from the plan — the exact disappearance
/// F50 non-negotiable behavior 5 forbids.
#[test]
fn accept_f50_c_a_declared_work_order_with_no_source_is_refused() {
    let (inventory, sources) = synthetic_campaign();
    let campaign = BoundCampaign {
        bindings: CampaignBindings::from_inventory(&inventory)
            .expect("the three-mission denominator is declared"),
        sources: sources.into_iter().take(2).collect(),
    };

    let error = probe_routes(&campaign).expect_err("an unsupplied work order has no route");
    let message = error.to_string();
    assert!(
        message.contains("M03"),
        "the refusal names the declared work order with no source: {message}"
    );
    assert!(
        message.contains("no probe route"),
        "the refusal says what would have gone wrong: {message}"
    );
}

/// A source binding for a work order the denominator does not declare is
/// refused: planning its route would grow the denominator without a
/// `declare` call, the same rule `assemble_campaign` enforces one layer up.
#[test]
fn accept_f50_c_a_source_for_an_undeclared_work_order_is_refused() {
    let two = CampaignInventory::parse(
        "# a two-mission denominator\n\
         M01\tSynthetic One\n\
         M02\tSynthetic Two\n",
    )
    .expect("the authored inventory parses");
    let (_inventory, mut sources) = synthetic_campaign();
    let bindings = synthetic_bindings(&two, &sources[..2]);
    sources.truncate(2);
    sources.push(synthetic_source(
        "M04",
        "Synthetic Four",
        Some((
            "synthetic-four",
            "synthetic-four-world",
            "synthetic-four-program",
        )),
    ));
    let campaign = BoundCampaign { bindings, sources };

    let error = probe_routes(&campaign).expect_err("an undeclared work order has no route");
    let message = error.to_string();
    assert!(
        message.contains("M04"),
        "the refusal names the undeclared work order: {message}"
    );
}

/// An installation fingerprint that is not canonical lowercase hex is
/// refused: the save/restart reentry is anchored to that string, and an
/// unreadable anchor must fail loudly instead of comparing unequal forever.
#[test]
fn accept_f50_c_a_fingerprint_that_is_not_canonical_hex_is_refused() {
    let (inventory, mut sources) = synthetic_campaign();
    sources[0].install_sha256 = "NOT-HEX".to_owned();
    let campaign = BoundCampaign {
        bindings: synthetic_bindings(&inventory, &sources),
        sources,
    };

    let error = probe_routes(&campaign).expect_err("an unreadable anchor is refused");
    let message = error.to_string();
    assert!(
        message.contains("M01") && message.contains("NOT-HEX"),
        "the refusal names the work order and the unusable fingerprint: {message}"
    );
}

// ------------------------------------------------------------ retail ---

/// Every declared work order of the real campaign has exactly one route, in
/// declared order, under the one fingerprint production discovery reports.
/// The work orders whose identity the installation carries in neither display
/// form are *refused* routes — named, counted and pinned here so a change in
/// the installation or in the binding code is re-measured instead of silently
/// shrinking the plan (spec F50 non-negotiable behavior 5).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_c_the_whole_campaign_has_one_probe_route_per_declared_work_order() {
    let inventory = load_inventory();
    let plan = plan();

    assert_eq!(
        plan.len(),
        inventory.len(),
        "one route per declared work order, none dropped and none added"
    );
    let order: Vec<&str> = plan
        .routes()
        .iter()
        .map(|route| route.label.as_str())
        .collect();
    let declared: Vec<&str> = inventory.iter().map(|(label, _)| label.as_str()).collect();
    assert_eq!(
        order, declared,
        "the plan follows the declared denominator order"
    );

    let discovered = discover(&game_dir()).expect("production discovery reads the installation");
    assert_eq!(
        plan.install_sha256(),
        fingerprint(&discovered.manifest).to_hex(),
        "every route is anchored to the fingerprint discovery reports"
    );

    assert_eq!(
        plan.ready_count(),
        17,
        "17 of the 24 work orders resolve their identity on this installation; if this count \
         changes, re-measure the installation instead of adjusting the expectation"
    );
    let refused: Vec<&str> = plan.refused().map(|route| route.label.as_str()).collect();
    assert_eq!(
        refused,
        vec!["M09", "M11", "M14", "M15", "M20", "M22", "M23"],
        "the seven work orders whose declared title the installation carries in neither display \
         form stay refused routes, named rather than dropped; if this set changes, re-measure \
         the installation"
    );
    for route in plan.refused() {
        let refusal = route
            .refusal
            .as_ref()
            .expect("a refused route names its refusal");
        assert!(
            refusal.contains(route.label.as_str()),
            "the refusal names its work order: {refusal}"
        );
        assert!(
            route.mission_id().is_none(),
            "a refused route locates no mission"
        );
    }
    for route in plan.ready() {
        assert_eq!(
            route.reentries.len(),
            ProbeInterruption::ALL.len(),
            "{} plans one reentry per interruption",
            route.label
        );
        assert!(
            route.campaign_position.is_some(),
            "{} resolved a campaign position",
            route.label
        );
    }
}

/// The AC03 minimum scenario at the level today's production can honestly
/// check it: a **fresh** production pass over the installation — one new
/// `SourceContext::read`, one new `bind_campaign`, one new `probe_routes` —
/// stands for the restart after an interruption, and every ready route
/// re-enters the *same* mission, world and program under the *same*
/// fingerprint through every one of the five interruptions, and matches what
/// the original pass planned. A route that re-entered a different identity
/// after a death, or that lost its anchor across a restart, fails with the
/// work order and the interruption named. That a mission *runtime* can be
/// killed and retried is not claimed here: it is `VS-M01-CONTROLLED-RUNS`'
/// evidence, and this stage supplies the contract that run drives.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_c_every_ready_route_reenters_the_same_mission_after_each_ac03_interruption() {
    let restarted_context = SourceContext::read(&game_dir())
        .expect("a fresh pass reads the installation the way a restart would");
    let restarted = restarted_context
        .bind_campaign(&load_inventory())
        .expect("the campaign binds again from the installation");
    let restarted_plan = probe_routes(&restarted).expect("the routes plan again");

    let planned = plan();
    let mut compared = 0;
    for route in planned.ready() {
        let first = route
            .reentry(ProbeInterruption::Death)
            .expect("a ready route plans every interruption");
        for interruption in ProbeInterruption::ALL {
            let reentry = route
                .reentry(interruption)
                .unwrap_or_else(|| panic!("{} plans {interruption}", route.label));
            assert_eq!(
                reentry.mission, first.mission,
                "{} re-enters a different mission after {interruption}",
                route.label
            );
            assert_eq!(
                reentry.world, first.world,
                "{} re-enters a different world after {interruption}",
                route.label
            );
            assert_eq!(
                reentry.program, first.program,
                "{} re-enters a different program after {interruption}",
                route.label
            );
            assert_eq!(
                reentry.install_sha256, route.install_sha256,
                "{} loses its installation anchor at {interruption}",
                route.label
            );

            let after_restart = restarted_plan
                .route(&route.label)
                .and_then(|route| route.reentry(interruption))
                .unwrap_or_else(|| {
                    panic!(
                        "{} lost its {interruption} reentry across a restart",
                        route.label
                    )
                });
            assert_eq!(
                reentry, after_restart,
                "{} re-enters something else under a fresh pass at {interruption}",
                route.label
            );
            compared += 1;
        }
    }
    assert_eq!(
        compared,
        planned.ready_count() * ProbeInterruption::ALL.len(),
        "every ready route was compared through every interruption"
    );
    assert_eq!(
        restarted_plan.install_sha256(),
        planned.install_sha256(),
        "the restart re-entered under a different installation"
    );
}

/// The human playtest route document lists exactly the planned routes: one
/// table row per declared work order carrying its resolved identity, its
/// campaign position and its state (or its refusal, named verbatim below the
/// table), so the document a playtester follows cannot drift from what
/// production derived. A missing, extra or stale row fails with the work
/// order named.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_c_the_playtest_route_document_lists_every_work_order_as_planned() {
    let document = std::fs::read_to_string(repo_path(PLAYTEST_ROUTES_DOC))
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", PLAYTEST_ROUTES_DOC));

    let plan = plan();
    let mut rows = 0;
    for route in plan.routes() {
        let row = document
            .lines()
            .find(|line| line.starts_with(&format!("| {} |", route.label)))
            .unwrap_or_else(|| panic!("{PLAYTEST_ROUTES_DOC} has no row for {}", route.label));
        rows += 1;
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        match (route.mission_id(), route.refusal.as_ref()) {
            (Some(mission), None) => {
                let reentry = route
                    .reentry(ProbeInterruption::Death)
                    .expect("a ready route plans every interruption");
                assert_eq!(
                    cells.len(),
                    8,
                    "{}'s row must hold label, position, mission, world, program, state: {row}",
                    route.label
                );
                assert_eq!(
                    cells[2],
                    route
                        .campaign_position
                        .map(|position| position.to_string())
                        .unwrap_or_else(|| "—".to_owned()),
                    "{}'s row does not carry its campaign position: {row}",
                    route.label
                );
                for (cell, id) in [
                    (cells[3], mission),
                    (cells[4], &reentry.world),
                    (cells[5], &reentry.program),
                ] {
                    assert_eq!(
                        cell,
                        format!("`{}`", id.as_str()),
                        "{}'s row does not carry {}: {row}",
                        route.label,
                        id.as_str()
                    );
                }
                assert_eq!(
                    cells[6], "ready",
                    "{}'s row does not read ready: {row}",
                    route.label
                );
            }
            (None, Some(refusal)) => {
                assert_eq!(
                    cells[6], "refused",
                    "{}'s row does not read refused: {row}",
                    route.label
                );
                assert!(
                    cells[2..6].iter().all(|cell| *cell == "—"),
                    "{}'s refused row must carry no identity: {row}",
                    route.label
                );
                assert!(
                    document.lines().any(|line| {
                        line.starts_with(&format!("- **{}**", route.label))
                            && line.contains(refusal.as_str())
                    }),
                    "{}'s refusal in the document is not the refusal production plans: {refusal}",
                    route.label
                );
            }
            _ => panic!("{}'s route is in an impossible state", route.label),
        }
    }
    assert_eq!(
        rows,
        load_inventory().len(),
        "the document's route table covers the whole denominator and adds nothing"
    );
}
