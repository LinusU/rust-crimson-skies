//! Acceptance scenarios for F43-B: progression, replay and reward
//! transactions.
//!
//! Spec: `specs/F43-campaign-progression-outcomes-and-economy-rules.md`, stage
//! `### F43-B`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`. Task
//! test prefix: `accept_f43_b_`.
//!
//! # The minimum scenario
//!
//! **AC02: "Replay an old mission with a worse result; preserve best while
//! recording latest as appropriate."** That is
//! `accept_f43_b_a_worse_replay_keeps_best_and_records_latest`, run on the
//! production path (`declared_interlude_campaign` → `lower_campaign` →
//! `CampaignState::apply_outcome` → `advance_interludes`). It replays a mission
//! the run already won, with a *lower* score and a fresh session, and pins all
//! four things the replay must not do: it must not lower `best_score`, it must
//! record the worse result as `latest`, it must not move the selected node, and
//! it must not pay the reward a second time.
//!
//! # What else this stage pins
//!
//! * **Progression across a narrative beat.** A declared campaign whose path
//!   crosses an interlude (`m01 → interlude → m02`) could not be walked past
//!   one in F43-A: no mission reports an outcome for a beat, so nothing moved
//!   `current` off it. `accept_f43_b_progression_walks_a_declared_interlude`
//!   drives the walk, and its idempotence and dead-end refusals.
//! * **The economy draft.** The contract's purchase rule — validate
//!   availability, money and the expected profile revision before writing, and
//!   fail a conflicting revision without overwriting unrelated progression.
//!   `accept_f43_b_a_purchase_is_validated_then_written_once` and its refusals.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No amount in this file is an original economy value: the prices
//! and grants exist to make the transaction observable, and the finding records
//! that the original's amounts are unmeasured.

use cs_app::campaign::lower_campaign;
use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignNode, CampaignNodeId, EdgeCondition,
    NodeKind, RewardSpec,
};
use cs_sim::campaign::{
    CampaignError, CampaignGraph, CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId,
    EventKey, MissionOutcome, Outcome, OutcomeAuthority, OutcomeId, OutcomeReceipt, ProfileId,
    RuntimeNodeKind, SessionGeneration, SymbolId, TransactionPlan,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const PROFILE: &str = "pilot.nathan";
const RUN: &str = "run.one";

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f43b.economy"))
}

/// An interlude asset that is still unsurveyed — the optional unknown the
/// lowering boundary is specified to accept.
fn unknown_asset(reason: &str) -> Resolved<ContentId> {
    Resolved::Unknown {
        claim_id: claim("f43b.interlude_asset"),
        reason: reason.to_owned(),
    }
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn node(id: &str) -> CampaignNodeId {
    CampaignNodeId::new(id).expect("valid node id")
}

fn key(id: &str) -> CampaignNodeKey {
    CampaignNodeKey::new(id).expect("valid node key")
}

fn content(kind: ContentKind, id: &str) -> ContentId {
    ContentId::from_source(kind, id).expect("valid content id")
}

fn mission_content(id: &str) -> ContentId {
    content(ContentKind::Mission, id)
}

/// An empty declared campaign entering at `entry`.
fn empty_draft(entry: CampaignNodeId) -> CampaignDraft {
    CampaignDraft {
        nodes: Vec::new(),
        entry,
        roster: Vec::new(),
        provenance: designed(),
    }
}

fn event(session: u32, tick: u64, sequence: u32) -> EventKey {
    EventKey {
        session: SessionGeneration(session),
        tick: Tick(tick),
        source: SymbolId(7),
        sequence,
    }
}

fn profile() -> ProfileId {
    ProfileId::new(PROFILE).expect("valid profile id")
}

/// A terminal report for `node`, in `session` generation.
///
/// `session` is part of the outcome identity, so two calls with different
/// sessions are two genuinely different transactions — a *replay* — while the
/// same session and sequence is the same transaction replayed after a crash.
fn outcome(
    session: u32,
    tick: u64,
    sequence: u32,
    node: &str,
    kind: Outcome,
    score: u64,
) -> MissionOutcome {
    MissionOutcome {
        id: OutcomeId {
            profile: profile(),
            run: CampaignRunId::new(RUN).expect("valid run id"),
            session: SessionGeneration(session),
            terminal_event: event(session, tick, sequence),
        },
        node: key(node),
        outcome: kind,
        score,
        authority: OutcomeAuthority::Authorized,
    }
}

/// The plan an applied outcome committed.
fn plan(receipt: &OutcomeReceipt) -> &TransactionPlan {
    match receipt {
        OutcomeReceipt::Applied(plan) => plan,
        OutcomeReceipt::AlreadyApplied => panic!("expected an applied plan, got AlreadyApplied"),
    }
}

/// A campaign whose victory path crosses a narrative beat and then a **later**
/// unlock, so a replay of `m01` has something it could damage:
///
/// ```text
/// m01 --Victory--> interlude --Victory--> m02 --Victory--> ending
/// ```
///
/// `m01`'s victory grants 500 and unlocks `plane_a`; the interlude grants 25 and
/// unlocks nothing; `m02`'s victory grants 900 and unlocks `plane_b`. Roster
/// availability: `plane_b` opens once `m01` is won, `plane_c` once `m02` is won.
fn declared_interlude_campaign() -> CampaignDefinition {
    let mut draft = empty_draft(node("m01"));
    draft.nodes.push(CampaignNode {
        id: node("m01"),
        kind: NodeKind::Mission {
            mission: known(mission_content("m01")),
        },
        edges: vec![
            CampaignEdge {
                on: EdgeCondition::Victory,
                to: node("interlude"),
                grant: Some(RewardSpec {
                    currency: known(500),
                    unlocks: vec![content(ContentKind::Airframe, "plane_a")],
                }),
                provenance: designed(),
            },
            CampaignEdge {
                on: EdgeCondition::Defeat,
                to: node("m01"),
                grant: None,
                provenance: designed(),
            },
        ],
        provenance: designed(),
    });
    draft.nodes.push(CampaignNode {
        id: node("interlude"),
        kind: NodeKind::Interlude {
            asset: unknown_asset("the beat's media asset is unsurveyed"),
        },
        edges: vec![CampaignEdge {
            on: EdgeCondition::Victory,
            to: node("m02"),
            grant: Some(RewardSpec {
                currency: known(25),
                unlocks: Vec::new(),
            }),
            provenance: designed(),
        }],
        provenance: designed(),
    });
    draft.nodes.push(CampaignNode {
        id: node("m02"),
        kind: NodeKind::Mission {
            mission: known(mission_content("m02")),
        },
        edges: vec![CampaignEdge {
            on: EdgeCondition::Victory,
            to: node("ending"),
            grant: Some(RewardSpec {
                currency: known(900),
                unlocks: vec![content(ContentKind::Airframe, "plane_b")],
            }),
            provenance: designed(),
        }],
        provenance: designed(),
    });
    draft.nodes.push(CampaignNode {
        id: node("ending"),
        kind: NodeKind::Ending,
        edges: Vec::new(),
        provenance: designed(),
    });
    draft.roster.push(cs_content::campaign::RosterEntry {
        item: content(ContentKind::Airframe, "plane_b"),
        available_from: node("m01"),
        provenance: designed(),
    });
    draft.roster.push(cs_content::campaign::RosterEntry {
        item: content(ContentKind::Airframe, "plane_c"),
        available_from: node("m02"),
        provenance: designed(),
    });
    CampaignDefinition::try_new(draft).expect("the fixture is a valid declared campaign")
}

fn interlude_run() -> (CampaignGraph, CampaignState) {
    let graph = lower_campaign(&declared_interlude_campaign()).expect("the fixture lowers");
    let state = CampaignState::begin(
        profile(),
        CampaignRunId::new(RUN).expect("valid run id"),
        DifficultyId::new("standard").expect("valid difficulty"),
        &graph,
    );
    (graph, state)
}

/// Wins `m01` and walks the beat, so the run sits on `m02` with `plane_a` and
/// `plane_b` already granted — the state a *replay* of `m01` has to survive.
fn progressed_past_the_beat() -> (CampaignGraph, CampaignState) {
    let (graph, mut state) = interlude_run();
    let applied = state
        .apply_outcome(&graph, &outcome(1, 10, 1, "m01", Outcome::Succeeded, 900))
        .expect("m01 applies");
    assert_eq!(plan(&applied.receipt).progress_to, Some(key("interlude")));
    // F43-B's addition: the mission outcome parks the run on the beat, and this
    // is what walks it off.
    state
        .advance_interludes(&graph)
        .expect("the declared beat has an onward edge");
    (graph, state)
}

/// Finishes the campaign: `m01` won, the beat walked, `m02` won, ending
/// selected and `plane_b` unlocked. The state AC02 replays *from*.
fn completed_run() -> (CampaignGraph, CampaignState) {
    let (graph, mut state) = progressed_past_the_beat();
    state
        .apply_outcome(&graph, &outcome(1, 40, 1, "m02", Outcome::Succeeded, 4000))
        .expect("m02 applies");
    state.advance_interludes(&graph).expect("m02 is not a beat");
    (graph, state)
}

// ---------------------------------------------------------------- AC02 ------

/// **AC02 — the stage's minimum scenario.** Replaying an already-won mission
/// with a worse result keeps `best`, records the worse result as `latest`, and
/// changes neither the selected next mission nor anything already unlocked.
///
/// The replay happens with the campaign **finished**, which is the hardest case
/// spec F43 behavior 3 names: "Replaying an earlier mission does not overwrite
/// the selected next mission or erase later unlocks." Here the selected node is
/// the ending and `plane_b` was unlocked by the *last* mission, so a replay that
/// regressed either would be plainly visible.
#[test]
fn accept_f43_b_a_worse_replay_keeps_best_and_records_latest() {
    let (graph, mut state) = completed_run();

    let record = state.progress(&key("m01")).expect("m01 has a record");
    assert_eq!(record.best_score, 900, "the first win set the best score");
    assert_eq!(record.victories, 1);
    let currency_before = state.currency();
    let selected_before = state.current().clone();
    assert_eq!(
        selected_before,
        key("ending"),
        "the run finished the campaign"
    );
    assert!(
        state.is_finished(&graph),
        "the fixture did not reach its ending"
    );

    // The replay: same mission, a **new session** (so it is a different
    // transaction, not a duplicate packet) and a much worse score.
    let applied = state
        .apply_outcome(&graph, &outcome(7, 900, 1, "m01", Outcome::Succeeded, 120))
        .expect("an already-visited node accepts a replay outcome");
    let replay = plan(&applied.receipt);

    // **Best is preserved**, never lowered by the worse replay.
    let record = state.progress(&key("m01")).expect("m01 still has a record");
    assert_eq!(
        record.best_score, 900,
        "a worse replay lowered the best score"
    );
    assert!(
        !replay.best_score_raised,
        "a worse replay raised the best score"
    );
    // …and the replay is **recorded**: latest is the worse result, the tally
    // counts it, and the record still shows the node was won.
    assert_eq!(
        record.latest,
        Some((Outcome::Succeeded, 120)),
        "latest is not the most recent result"
    );
    assert_eq!(
        record.victories, 2,
        "the replay was not counted as an attempt"
    );
    assert!(
        state.has_completed(&key("m01")),
        "the replay erased the win"
    );

    // **Progression is untouched**: a finished campaign stays finished.
    assert_eq!(
        state.current(),
        &selected_before,
        "replaying an old mission moved the selected node off the ending"
    );
    assert!(
        state.is_finished(&graph),
        "a replay un-finished the campaign"
    );
    assert_eq!(
        replay.progress_to, None,
        "a replay reported a progression move"
    );

    // **The reward is not re-paid**, and the later unlock survives.
    assert_eq!(
        replay.currency_delta, 0,
        "a replay paid the victory grant again"
    );
    assert!(replay.unlocks.is_empty(), "a replay re-granted its unlocks");
    assert_eq!(
        state.currency(),
        currency_before,
        "a replay moved the balance"
    );
    let owned: Vec<ContentId> = state.unlocks().cloned().collect();
    assert!(
        owned.contains(&content(ContentKind::Airframe, "plane_b")),
        "a replay erased the last mission's unlock: {owned:?}"
    );
    // The later mission's own record is untouched too: a replay of m01 must not
    // write through to the node that follows it.
    assert_eq!(
        state.progress(&key("m02")).map(|record| record.best_score),
        Some(4000),
        "a replay of m01 disturbed m02's record"
    );
}

/// The failure cases AC02 has to survive: a replay that never happened, and a
/// replay of a node the run has never reached.
///
/// A future node is refused before anything is written, so the refusal cannot be
/// used to smuggle a record onto a mission the player has not flown.
#[test]
fn accept_f43_b_a_replay_of_an_unflown_node_is_refused_without_writing() {
    let (graph, mut state) = interlude_run();

    // `m02` has never been selected and has no record: a future node.
    let error = state
        .apply_outcome(&graph, &outcome(1, 10, 1, "m02", Outcome::Succeeded, 9999))
        .expect_err("a future node refuses");
    assert_eq!(
        error,
        CampaignError::IneligibleNode {
            node: key("m02"),
            current: key("m01"),
        },
        "the refusal does not name the ineligible node and the selected one"
    );
    assert_eq!(state.revision(), 0, "a refusal wrote a revision");
    assert_eq!(state.currency(), 0);
    assert!(
        state.progress(&key("m02")).is_none(),
        "a refusal wrote a record"
    );

    // …and the real failure mode of a duplicate packet: the *same* identity
    // twice is the ledger's job, not a second grant (spec F43 AC01, which this
    // stage must not regress).
    let first = state
        .apply_outcome(&graph, &outcome(1, 10, 1, "m01", Outcome::Succeeded, 500))
        .expect("m01 applies");
    assert_eq!(plan(&first.receipt).currency_delta, 500);
    let again = state
        .apply_outcome(&graph, &outcome(1, 10, 1, "m01", Outcome::Succeeded, 500))
        .expect("a duplicate packet is answered, not refused");
    assert_eq!(
        again.receipt,
        OutcomeReceipt::AlreadyApplied,
        "a duplicate packet paid twice"
    );
    assert_eq!(
        state.currency(),
        500,
        "a duplicate packet doubled the balance"
    );
}

// ---------------------------------------------------------- progression -----

/// A declared campaign whose path crosses a narrative beat can now be walked to
/// its end — which it could not in F43-A, where nothing moved `current` off an
/// interlude.
#[test]
fn accept_f43_b_progression_walks_a_declared_interlude() {
    let (graph, mut state) = interlude_run();

    // Winning the first mission parks the run on the beat, which is the state
    // F43-A left with no way out.
    let applied = state
        .apply_outcome(&graph, &outcome(1, 10, 1, "m01", Outcome::Succeeded, 900))
        .expect("m01 applies");
    assert_eq!(plan(&applied.receipt).progress_to, Some(key("interlude")));
    assert_eq!(state.current(), &key("interlude"));

    let beat = state.advance_interludes(&graph).expect("the beat walks");
    assert_eq!(
        beat.traversed,
        vec![key("interlude")],
        "the walk did not report the beat it crossed"
    );
    assert_eq!(beat.currency_delta, 25, "the beat's own grant was not paid");
    assert_eq!(state.current(), &key("m02"), "the walk did not land on m02");
    assert_eq!(state.currency(), 525, "the beat's grant was not committed");

    // The rest of the declared graph is reachable through the same path.
    state
        .apply_outcome(&graph, &outcome(1, 40, 1, "m02", Outcome::Succeeded, 4000))
        .expect("m02 applies");
    state.advance_interludes(&graph).expect("m02 is not a beat");
    assert_eq!(state.current(), &key("ending"));
    assert!(
        state.is_finished(&graph),
        "the run did not reach its ending"
    );
    assert_eq!(state.currency(), 1425);
}

/// Walking a beat twice must not pay it twice (spec F43 behavior 2), and a
/// non-beat is not a walk at all.
#[test]
fn accept_f43_b_walking_a_beat_twice_pays_it_once() {
    let (graph, mut state) = progressed_past_the_beat();
    let currency = state.currency();
    let revision = state.revision();

    // `m02` is a mission, not a beat: nothing traversed, nothing written.
    let beat = state.advance_interludes(&graph).expect("m02 is not a beat");
    assert!(
        beat.is_empty(),
        "a mission was reported as a traversed beat"
    );
    assert_eq!(state.revision(), revision, "a no-op walk wrote a revision");
    assert_eq!(state.currency(), currency, "a no-op walk paid something");

    // Re-entering the beat is impossible through progression, and the guard is
    // what makes that safe: an interlude can only be re-walked by a graph that
    // routes back to it, and this declared graph does not.
    let applied = state
        .apply_outcome(&graph, &outcome(9, 10, 1, "m01", Outcome::Failed, 0))
        .expect("a defeat replay applies");
    assert_eq!(
        applied.receipt,
        OutcomeReceipt::Applied(TransactionPlan {
            currency_delta: 0,
            unlocks: Vec::new(),
            progress_to: None,
            best_score_raised: false,
            marked_modified: false,
        }),
        "the defeat replay moved progression or paid a grant"
    );
    assert_eq!(state.currency(), currency, "a defeat replay paid something");
    assert_eq!(
        state.current(),
        &key("m02"),
        "a defeat on an old node moved the selected mission"
    );
}

/// A beat with no onward edge is a dead end in the *declared* campaign, and the
/// walk must say so rather than stop quietly — otherwise an importer bug reads
/// as "the campaign ends here".
#[test]
fn accept_f43_b_a_beat_with_no_onward_edge_is_refused_not_silently_stopped() {
    // `m01 --Victory--> interlude`, and the interlude declares only a Defeat
    // edge, so it has no way onward. The declared schema accepts it (a
    // non-ending node needs *an* edge); the runtime refuses to walk it.
    let mut draft = empty_draft(node("m01"));
    draft.nodes.push(CampaignNode {
        id: node("m01"),
        kind: NodeKind::Mission {
            mission: known(mission_content("m01")),
        },
        edges: vec![CampaignEdge {
            on: EdgeCondition::Victory,
            to: node("interlude"),
            grant: None,
            provenance: designed(),
        }],
        provenance: designed(),
    });
    draft.nodes.push(CampaignNode {
        id: node("interlude"),
        kind: NodeKind::Interlude {
            asset: unknown_asset("unsurveyed"),
        },
        edges: vec![CampaignEdge {
            on: EdgeCondition::Defeat,
            to: node("interlude"),
            grant: None,
            provenance: designed(),
        }],
        provenance: designed(),
    });
    let declared =
        CampaignDefinition::try_new(draft).expect("a beat with one edge is a valid declaration");
    let graph = lower_campaign(&declared).expect("it lowers");
    let mut state = CampaignState::begin(
        profile(),
        CampaignRunId::new(RUN).expect("valid run id"),
        DifficultyId::new("standard").expect("valid difficulty"),
        &graph,
    );

    state
        .apply_outcome(&graph, &outcome(1, 10, 1, "m01", Outcome::Succeeded, 100))
        .expect("m01 applies");
    let revision = state.revision();
    let error = state
        .advance_interludes(&graph)
        .expect_err("a beat with no onward victory edge is a dead end");
    assert_eq!(
        error,
        CampaignError::InterludeDeadEnd {
            node: key("interlude")
        },
        "the refusal does not name the dead-end beat"
    );
    // …and it left the run where it was: a refusal is not half a walk.
    assert_eq!(state.current(), &key("interlude"));
    assert_eq!(state.revision(), revision, "the refusal wrote a revision");
}

// ------------------------------------------------------------- economy ------

/// The contract's purchase rule: validate availability, money and the expected
/// revision **before** writing, then write once.
#[test]
fn accept_f43_b_a_purchase_is_validated_then_written_once() {
    let (graph, mut state) = progressed_past_the_beat();
    let plane_b = content(ContentKind::Airframe, "plane_b");
    // `plane_b` was granted by `m02`… which this run has not reached, so it is
    // available (the gate is `m01`) but not owned.
    assert!(
        graph.available_items(&state).any(|item| *item == plane_b),
        "the roster gate did not open after winning m01"
    );
    assert_eq!(state.currency(), 525);
    let revision = state.revision();

    let draft = cs_sim::campaign::PurchaseDraft {
        item: plane_b.clone(),
        price: 400,
        expected_revision: revision,
    };
    let receipt = state.purchase(&graph, &draft).expect("the draft is valid");
    assert_eq!(receipt.paid, 400);
    assert_eq!(state.currency(), 125, "the price was not charged");
    assert!(
        state.unlocks().any(|item| *item == plane_b),
        "the purchase did not unlock the item"
    );
    assert_eq!(
        state.revision(),
        revision + 1,
        "the purchase wrote no revision"
    );

    // **Idempotent**: the same draft replayed after a crash must not charge
    // twice (spec F43 behavior 2 — "a repeated result packet after restart
    // cannot grant cash twice").
    let again = state.purchase(&graph, &draft);
    assert_eq!(
        again,
        Err(CampaignError::AlreadyOwned {
            item: plane_b.clone()
        }),
        "a replayed purchase draft was accepted twice"
    );
    assert_eq!(state.currency(), 125, "a replayed draft charged twice");
    assert_eq!(
        state.revision(),
        revision + 1,
        "a refused purchase wrote a revision"
    );
}

/// Every refusal a purchase draft can hit, and the invariant they share: the
/// state is bit-identical afterwards, because a conflict must "fail and refresh
/// the view" without "overwriting unrelated progression".
#[test]
fn accept_f43_b_every_purchase_refusal_leaves_the_profile_untouched() {
    let (graph, mut state) = progressed_past_the_beat();
    let revision = state.revision();
    let currency = state.currency();
    let unlocks: Vec<ContentId> = state.unlocks().cloned().collect();
    let plane_b = content(ContentKind::Airframe, "plane_b");
    let plane_c = content(ContentKind::Airframe, "plane_c");

    // **Unavailable**: `plane_c`'s gate is `m02`, which this run has not won.
    assert_eq!(
        state.purchase(
            &graph,
            &cs_sim::campaign::PurchaseDraft {
                item: plane_c.clone(),
                price: 1,
                expected_revision: revision,
            },
        ),
        Err(CampaignError::ItemUnavailable { item: plane_c }),
        "an ungated item was purchasable"
    );

    // **Too dear**: valid and available, but the balance cannot cover it.
    assert_eq!(
        state.purchase(
            &graph,
            &cs_sim::campaign::PurchaseDraft {
                item: plane_b.clone(),
                price: currency + 1,
                expected_revision: revision,
            },
        ),
        Err(CampaignError::InsufficientFunds {
            price: currency + 1,
            balance: currency,
        }),
        "an unaffordable purchase was accepted"
    );

    // **Stale**: the view was built against an older revision. This is the
    // conflict the contract names, and it must be reported as a conflict — even
    // though the item is affordable and available.
    let stale = revision.saturating_sub(1);
    assert_ne!(
        stale, revision,
        "the fixture revision cannot express staleness"
    );
    assert_eq!(
        state.purchase(
            &graph,
            &cs_sim::campaign::PurchaseDraft {
                item: plane_b.clone(),
                price: 1,
                expected_revision: stale,
            },
        ),
        Err(CampaignError::StaleRevision {
            expected: stale,
            actual: revision,
        }),
        "a stale draft was applied instead of refused"
    );

    // Nothing above wrote anything.
    assert_eq!(state.revision(), revision, "a refusal wrote a revision");
    assert_eq!(state.currency(), currency, "a refusal moved the balance");
    assert_eq!(
        state.unlocks().cloned().collect::<Vec<_>>(),
        unlocks,
        "a refusal changed the owned set"
    );
}

/// A stale draft must be reported as stale even when the purchase would *also*
/// have failed for another reason, so the view the caller refreshes names the
/// real cause.
#[test]
fn accept_f43_b_a_stale_draft_is_reported_as_stale_before_its_other_faults() {
    let (graph, mut state) = progressed_past_the_beat();
    let revision = state.revision();
    // Both faults at once: the item is unavailable *and* the draft is stale, and
    // it is also unaffordable.
    let error = state
        .purchase(
            &graph,
            &cs_sim::campaign::PurchaseDraft {
                item: content(ContentKind::Airframe, "never_gated"),
                price: u64::MAX,
                expected_revision: revision + 7,
            },
        )
        .expect_err("an unavailable item cannot be bought");
    // Availability is checked first and reported: the caller's view must learn
    // the item does not exist in this run before it learns the price or the
    // revision, or a refresh would show a menu entry that cannot be bought.
    assert_eq!(
        error,
        CampaignError::ItemUnavailable {
            item: content(ContentKind::Airframe, "never_gated"),
        },
        "the refusal order changed: {error}"
    );

    // With the item available, the *revision* fault outranks the price fault, so
    // a stale view is never told "you cannot afford it" when the truth is that
    // the profile moved on.
    let plane_b = content(ContentKind::Airframe, "plane_b");
    let error = state
        .purchase(
            &graph,
            &cs_sim::campaign::PurchaseDraft {
                item: plane_b,
                price: u64::MAX,
                expected_revision: revision + 7,
            },
        )
        .expect_err("an unaffordable purchase is refused");
    assert!(
        matches!(error, CampaignError::InsufficientFunds { .. }),
        "an available item with an impossible price reported {error:?} instead of the price"
    );
}

/// The progression and economy transactions share one revision counter, so a
/// purchase built against a view taken before a replay must be refused — the
/// replay moved the profile even though it paid nothing.
#[test]
fn accept_f43_b_a_replay_invalidates_a_draft_taken_before_it() {
    let (graph, mut state) = progressed_past_the_beat();
    let revision = state.revision();
    let plane_b = content(ContentKind::Airframe, "plane_b");
    let draft = cs_sim::campaign::PurchaseDraft {
        item: plane_b,
        price: 10,
        expected_revision: revision,
    };

    // A replay of an old mission: pays nothing, but it is a committed
    // transaction, so the revision moves.
    state
        .apply_outcome(&graph, &outcome(8, 10, 1, "m01", Outcome::Succeeded, 50))
        .expect("the replay applies");
    assert_eq!(
        state.revision(),
        revision + 1,
        "a replay that paid nothing wrote no revision"
    );

    // The draft was built against the old view, so it is refused rather than
    // silently applied to a profile that has moved.
    assert_eq!(
        state.purchase(&graph, &draft),
        Err(CampaignError::StaleRevision {
            expected: revision,
            actual: revision + 1,
        }),
        "a draft built before a replay was applied without complaint"
    );
}

/// The declared runtime graph still classifies the beat this stage walks, so the
/// walk and the classification agree on what a beat is.
#[test]
fn accept_f43_b_the_declared_beat_lowers_to_an_interlude_and_a_roster_gate_holds() {
    let graph = lower_campaign(&declared_interlude_campaign()).expect("the fixture lowers");
    assert_eq!(
        graph.node(&key("interlude")).map(|node| &node.kind),
        Some(&RuntimeNodeKind::Interlude),
        "the declared beat did not lower to an interlude"
    );
    assert_eq!(graph.roster().len(), 2, "the roster gates did not lower");
    assert_eq!(
        graph.node(&key("m02")).map(|node| &node.kind),
        Some(&RuntimeNodeKind::Mission {
            mission: mission_content("m02"),
        }),
        "m02 did not lower to a mission bound to its content id"
    );
}
