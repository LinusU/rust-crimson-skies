//! Acceptance scenarios for F44-B: the shared validator and the transactional
//! construction economy. Task test prefix: `accept_f44_b_`.
//!
//! Spec: `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-B` and AC01-AC03; contract `docs/contracts/STATE-TRANSACTIONS.md`
//! ("Outcome and economy transaction").
//!
//! The stage's minimum scenario is AC02,
//! `accept_f44_b_cancelling_an_edited_draft_leaves_inventory_and_currency_unchanged`.
//! Every test drives `cs_app::construction::ConstructionSession`, which calls
//! the shared `ConstructionRules::validate` and `cs_sim::economy::commit`.
//!
//! Every value is newly authored synthetic data; no price, mass or limit is an
//! original value.

use std::collections::BTreeSet;

use cs_app::campaign::lower_campaign;
use cs_app::construction::{ConstructionContext, ConstructionError, ConstructionSession};
use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignNode, CampaignNodeId, EdgeCondition,
    NodeKind, RewardSpec, RosterEntry,
};
use cs_content::construction::{
    AircraftBlueprint, ConstraintViolation, ConstructionPolicy, ConstructionRules, GunFitment,
    LimitBreach, PriceBook, SYNTHETIC_HEAVY_PLATE_KEY, SYNTHETIC_MISSILE_KEY, ValidationRefusal,
    declared_synthetic_blueprint, declared_synthetic_price_book, synthetic_boundary_rules,
    synthetic_gun_fitments, synthetic_policy,
};
use cs_sim::campaign::{
    CampaignError, CampaignGraph, CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId,
    EventKey, MissionOutcome, Outcome, OutcomeAuthority, OutcomeId, ProfileId, SessionGeneration,
    SymbolId,
};
use cs_sim::economy::EconomyError;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// The synthetic budget of one win, far above the synthetic prices.
const GRANT: u64 = 1_000_000;
/// The price of every distinct component of the boundary blueprint:
/// 12000 airframe + 3000 engine + 1000 plate + 1500 gun + 4250 missile + 2000 radio.
const DISTINCT_PRICE: u64 = 23_750;

fn designed() -> Provenance {
    Provenance::designed(ClaimId::new("f44b.test").expect("valid claim id"))
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn node(key: &str) -> CampaignNodeId {
    CampaignNodeId::new(key).expect("valid node id")
}

/// `m01 --Victory(+grant)--> ending`, with every synthetic component rostered
/// from `m01`.
fn graph(grant: u64) -> CampaignGraph {
    let policy = synthetic_policy();
    let mut draft = CampaignDraft {
        nodes: vec![
            CampaignNode {
                id: node("m01"),
                kind: NodeKind::Mission {
                    mission: known(id(ContentKind::Mission, "m01")),
                },
                edges: vec![CampaignEdge {
                    on: EdgeCondition::Victory,
                    to: node("ending"),
                    grant: Some(RewardSpec {
                        currency: known(grant),
                        unlocks: Vec::new(),
                    }),
                    provenance: designed(),
                }],
                provenance: designed(),
            },
            CampaignNode {
                id: node("ending"),
                kind: NodeKind::Ending,
                edges: Vec::new(),
                provenance: designed(),
            },
        ],
        entry: node("m01"),
        roster: Vec::new(),
        provenance: designed(),
    };
    for item in policy.available() {
        draft.roster.push(RosterEntry {
            item: item.clone(),
            available_from: node("m01"),
            provenance: designed(),
        });
    }
    let definition = CampaignDefinition::try_new(draft).expect("the fixture campaign is valid");
    lower_campaign(&definition).expect("the fixture campaign lowers")
}

fn profile() -> ProfileId {
    ProfileId::new("pilot.test").expect("valid profile id")
}

/// A state that has won `m01` and so holds `GRANT` with every component open.
fn funded(graph: &CampaignGraph) -> CampaignState {
    won(graph, "run.one")
}

/// A state of `run` that has won `m01` under `graph`.
fn won(graph: &CampaignGraph, run: &str) -> CampaignState {
    let run = CampaignRunId::new(run).expect("valid run id");
    let mut state = CampaignState::begin(
        profile(),
        run.clone(),
        DifficultyId::new("standard").expect("valid difficulty"),
        graph,
    );
    let outcome = MissionOutcome {
        id: OutcomeId {
            profile: profile(),
            run,
            session: SessionGeneration(1),
            terminal_event: EventKey {
                session: SessionGeneration(1),
                tick: Tick(10),
                source: SymbolId(7),
                sequence: 1,
            },
        },
        node: CampaignNodeKey::new("m01").expect("valid node key"),
        outcome: Outcome::Succeeded,
        score: 1,
        authority: OutcomeAuthority::Authorized,
    };
    state.apply_outcome(graph, &outcome).expect("m01 is won");
    state
}

struct Fixture {
    graph: CampaignGraph,
    rules: ConstructionRules,
    policy: ConstructionPolicy,
    book: PriceBook,
}

impl Fixture {
    fn new() -> Self {
        Self {
            graph: graph(GRANT),
            rules: synthetic_boundary_rules(),
            policy: synthetic_policy(),
            book: declared_synthetic_price_book(),
        }
    }

    fn ctx(&self) -> ConstructionContext<'_> {
        ConstructionContext {
            rules: &self.rules,
            policy: &self.policy,
            book: &self.book,
            graph: &self.graph,
        }
    }
}

fn plate_heavy() -> ContentId {
    id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY)
}

/// The boundary blueprint with its armor replaced by one heavy plate on the
/// first zone: exactly one weight unit over the ceiling.
fn one_unit_overweight() -> AircraftBlueprint {
    let mut armor = cs_content::construction::synthetic_armor_fitments();
    armor[0] =
        cs_content::construction::ArmorFitment::try_new(armor[0].zone().clone(), plate_heavy())
            .expect("valid armor");
    declared_synthetic_blueprint()
        .with_armor(armor)
        .expect("valid blueprint")
}

/// The boundary blueprint with its first gun mated into a pair (two positions)
/// and the second gun dropped, so the rack still uses four positions: one pair
/// plus two singles.
fn paired_blueprint() -> AircraftBlueprint {
    let mut guns = synthetic_gun_fitments();
    let first = guns.remove(0).with_positions(known(2)).expect("valid");
    guns.remove(0); // four positions: one pair plus two singles
    guns.insert(0, first);
    declared_synthetic_blueprint()
        .with_guns(guns)
        .expect("valid blueprint")
}

#[test]
fn accept_f44_b_cancelling_an_edited_draft_leaves_inventory_and_currency_unchanged() {
    let fx = Fixture::new();
    let mut state = funded(&fx.graph);
    let before = state.snapshot();

    let mut session = ConstructionSession::begin(&state, declared_synthetic_blueprint(), vec![]);
    // Edit: a different, still-valid blueprint (a pair in place of two singles),
    // and look at the live verdict, as the editor's preview does.
    session.edit(paired_blueprint());
    session.queue_sale(plate_heavy());
    let verdict = session
        .verdict(&fx.ctx())
        .expect("the edited blueprint is measurable");
    assert!(verdict.is_valid(), "the edited draft is a legal loadout");
    session.cancel();
    assert_eq!(state.snapshot(), before, "cancel changed the profile");
    assert_eq!(state.currency(), GRANT);
    assert_eq!(state.unlocks().count(), 0);

    // The contrast: the same edit committed *does* change the profile, so the
    // equality above is a property of cancel and not of an inert economy.
    let mut session = ConstructionSession::begin(&state, declared_synthetic_blueprint(), vec![]);
    session.edit(declared_synthetic_blueprint());
    let receipt = session
        .commit(&fx.ctx(), &mut state)
        .expect("the boundary blueprint commits");
    assert_ne!(state.snapshot(), before);
    assert_eq!(receipt.charged, DISTINCT_PRICE);
    assert_eq!(state.currency(), GRANT - DISTINCT_PRICE);
}

#[test]
fn accept_f44_b_a_commit_charges_once_and_writes_one_revision() {
    let fx = Fixture::new();
    let mut state = funded(&fx.graph);
    let revision = state.revision();

    let session = ConstructionSession::begin(&state, declared_synthetic_blueprint(), vec![]);
    let stale = session.clone();
    let receipt = session
        .commit(&fx.ctx(), &mut state)
        .expect("the boundary blueprint commits");
    assert_eq!(receipt.revision, revision + 1, "one revision for six items");
    assert_eq!(state.revision(), revision + 1);
    assert_eq!(state.unlocks().count(), 6);
    assert_eq!(state.currency(), GRANT - DISTINCT_PRICE);

    // The conflicting editor opened before the commit is refused and the
    // profile is not touched.
    let after = state.snapshot();
    let error = stale
        .commit(&fx.ctx(), &mut state)
        .expect_err("a stale draft must not apply");
    assert!(
        matches!(
            error,
            ConstructionError::Economy(EconomyError::Campaign(CampaignError::StaleRevision { .. }))
        ),
        "got {error:?}"
    );
    assert_eq!(state.snapshot(), after);
}

#[test]
fn accept_f44_b_an_unaffordable_or_unavailable_draft_changes_nothing() {
    let fx = Fixture::new();

    // Unavailable: nothing is rostered until m01 is won.
    let mut fresh = CampaignState::begin(
        profile(),
        CampaignRunId::new("run.two").expect("valid run id"),
        DifficultyId::new("standard").expect("valid difficulty"),
        &fx.graph,
    );
    let before = fresh.snapshot();
    let error = ConstructionSession::begin(&fresh, declared_synthetic_blueprint(), vec![])
        .commit(&fx.ctx(), &mut fresh)
        .expect_err("an unwon campaign offers nothing to buy");
    assert!(
        matches!(
            error,
            ConstructionError::Economy(EconomyError::Campaign(
                CampaignError::ItemUnavailable { .. }
            ))
        ),
        "got {error:?}"
    );
    assert_eq!(fresh.snapshot(), before);

    // Unaffordable: one minor unit short of the distinct components' price.
    let poor_graph = graph(DISTINCT_PRICE - 1);
    let poor_fx = Fixture {
        graph: poor_graph,
        ..Fixture::new()
    };
    let mut poor = won(&poor_fx.graph, "run.three");
    let before = poor.snapshot();
    let error = ConstructionSession::begin(&poor, declared_synthetic_blueprint(), vec![])
        .commit(&poor_fx.ctx(), &mut poor)
        .expect_err("one unit short must not commit");
    assert!(
        matches!(
            error,
            ConstructionError::Economy(EconomyError::Campaign(
                CampaignError::InsufficientFunds { .. }
            ))
        ),
        "got {error:?}"
    );
    assert_eq!(poor.snapshot(), before, "no partial purchase");

    // Exactly the price is enough.
    let exact_fx = Fixture {
        graph: graph(DISTINCT_PRICE),
        ..Fixture::new()
    };
    let mut exact = won(&exact_fx.graph, "run.four");
    ConstructionSession::begin(&exact, declared_synthetic_blueprint(), vec![])
        .commit(&exact_fx.ctx(), &mut exact)
        .expect("exactly the price commits");
    assert_eq!(exact.currency(), 0);
}

#[test]
fn accept_f44_b_the_validator_accepts_the_limit_and_rejects_one_unit_over() {
    let fx = Fixture::new();
    let mut state = funded(&fx.graph);
    let before = state.snapshot();

    let over = one_unit_overweight();
    let session = ConstructionSession::begin(&state, over, vec![]);
    let verdict = session.verdict(&fx.ctx()).expect("measurable");
    assert!(!verdict.is_valid());
    assert!(matches!(
        verdict.assessment().first_breach(),
        Some(LimitBreach::Mass { .. })
    ));
    let error = session
        .commit(&fx.ctx(), &mut state)
        .expect_err("one unit over the ceiling must not commit");
    assert!(
        matches!(error, ConstructionError::Invalid(_)),
        "got {error:?}"
    );
    assert_eq!(state.snapshot(), before);

    let at_limit = ConstructionSession::begin(&state, declared_synthetic_blueprint(), vec![]);
    assert!(at_limit.verdict(&fx.ctx()).expect("measurable").is_valid());
    at_limit
        .commit(&fx.ctx(), &mut state)
        .expect("exactly at the limit commits");
}

#[test]
fn accept_f44_b_an_imported_blueprint_cannot_bypass_pairing_or_banned_rules() {
    let fx = Fixture::new();
    let state = funded(&fx.graph);

    // A pair of a gun the pairing rule does not allow.
    let no_pairs = ConstructionPolicy::new(
        known(BTreeSet::new()),
        BTreeSet::new(),
        fx.policy.available().clone(),
    );
    let verdict = fx
        .rules
        .validate(&no_pairs, &paired_blueprint(), &fx.book)
        .expect("measurable");
    assert!(!verdict.is_valid());
    assert!(
        verdict
            .violations()
            .iter()
            .any(|v| matches!(v, ConstraintViolation::GunNotPairable { .. }))
    );
    // The same blueprint under the permissive pairing rule is valid, so the
    // refusal above comes from the rule and not from the blueprint.
    assert!(
        fx.rules
            .validate(&fx.policy, &paired_blueprint(), &fx.book)
            .expect("measurable")
            .is_valid()
    );

    // A host ban on a component the blueprint names.
    let banned = fx
        .policy
        .clone()
        .with_banned(id(ContentKind::Weapon, SYNTHETIC_MISSILE_KEY));
    let mut state = state;
    let before = state.snapshot();
    let fx_banned = ConstructionContext {
        policy: &banned,
        ..fx.ctx()
    };
    let error = ConstructionSession::begin(&state, declared_synthetic_blueprint(), vec![])
        .commit(&fx_banned, &mut state)
        .expect_err("a banned component must not commit");
    let ConstructionError::Invalid(verdict) = error else {
        panic!("expected Invalid, got {error:?}");
    };
    assert_eq!(
        cs_app::construction::banned_components(&verdict),
        vec![&id(ContentKind::Weapon, SYNTHETIC_MISSILE_KEY); 4]
    );
    assert_eq!(state.snapshot(), before);

    // Three positions on one mount is neither single nor pair.
    let mut guns = synthetic_gun_fitments();
    guns[0] = guns[0].clone().with_positions(known(3)).expect("valid");
    guns.truncate(2); // 3 + 1 = four positions: inside the rack
    let triple = declared_synthetic_blueprint()
        .with_guns(guns)
        .expect("valid blueprint");
    let verdict = fx
        .rules
        .validate(&fx.policy, &triple, &fx.book)
        .expect("measurable");
    assert!(matches!(
        verdict.violations(),
        [ConstraintViolation::UnsupportedGunSelection { positions: 3, .. }]
    ));

    // An unavailable component is a violation, not a free pass.
    let nothing = ConstructionPolicy::new(known(BTreeSet::new()), BTreeSet::new(), BTreeSet::new());
    let verdict = fx
        .rules
        .validate(&nothing, &declared_synthetic_blueprint(), &fx.book)
        .expect("measurable");
    assert!(
        verdict
            .violations()
            .iter()
            .all(|v| matches!(v, ConstraintViolation::Unavailable { .. }))
            && !verdict.violations().is_empty()
    );

    // An unmeasured pairing rule refuses a pair instead of guessing.
    let unmeasured = ConstructionPolicy::new(
        Resolved::unknown(
            ClaimId::new("f44b.test.pairing").expect("valid claim id"),
            "the original pairing rule is unmeasured",
        )
        .expect("a reason is present"),
        BTreeSet::new(),
        fx.policy.available().clone(),
    );
    assert!(matches!(
        fx.rules
            .validate(&unmeasured, &paired_blueprint(), &fx.book),
        Err(ValidationRefusal::UnknownPairingRule { .. })
    ));
}

#[test]
fn accept_f44_b_asymmetric_loadouts_are_valid() {
    let fx = Fixture::new();
    // Guns only on the left-hand mounts, rockets on one side: valid.
    let guns: Vec<GunFitment> = synthetic_gun_fitments().into_iter().take(1).collect();
    let blueprint = declared_synthetic_blueprint()
        .with_guns(guns)
        .expect("valid blueprint");
    assert!(
        fx.rules
            .validate(&fx.policy, &blueprint, &fx.book)
            .expect("measurable")
            .is_valid()
    );
}

#[test]
fn accept_f44_b_selling_cannot_leave_an_active_blueprint_referencing_it() {
    let fx = Fixture::new();
    let mut state = funded(&fx.graph);
    ConstructionSession::begin(&state, declared_synthetic_blueprint(), vec![])
        .commit(&fx.ctx(), &mut state)
        .expect("buys the components");
    let owned = state.snapshot();

    // The edited blueprint still uses the plate: selling it is refused and
    // nothing moves.
    let plate = id(
        ContentKind::Armor,
        cs_content::construction::SYNTHETIC_PLATE_KEY,
    );
    let mut session = ConstructionSession::begin(&state, declared_synthetic_blueprint(), vec![]);
    session.queue_sale(plate.clone());
    let error = session
        .commit(&fx.ctx(), &mut state)
        .expect_err("selling an in-use component must be refused");
    assert!(
        matches!(
            error,
            ConstructionError::Economy(EconomyError::ActiveReference { .. })
        ),
        "got {error:?}"
    );
    assert_eq!(state.snapshot(), owned);

    // Another active blueprint using the plate also blocks it, even when the
    // edited blueprint does not use it.
    let no_armor = declared_synthetic_blueprint()
        .with_armor(Vec::new())
        .expect("valid blueprint");
    let mut session = ConstructionSession::begin(
        &state,
        no_armor.clone(),
        vec![declared_synthetic_blueprint()],
    );
    session.queue_sale(plate.clone());
    assert!(matches!(
        session.commit(&fx.ctx(), &mut state),
        Err(ConstructionError::Economy(
            EconomyError::ActiveReference { .. }
        ))
    ));
    assert_eq!(state.snapshot(), owned);

    // With no active reference left the sale refunds the price paid.
    let mut session = ConstructionSession::begin(&state, no_armor, vec![]);
    session.queue_sale(plate);
    let receipt = session
        .commit(&fx.ctx(), &mut state)
        .expect("an unreferenced component sells");
    assert_eq!(receipt.refunded, 1_000);
    assert_eq!(state.currency(), GRANT - DISTINCT_PRICE + 1_000);
}
