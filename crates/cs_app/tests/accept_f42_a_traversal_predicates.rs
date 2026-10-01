//! F42-A acceptance: traversal predicates and reward identity (synthetic).
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-A`. Task test prefix: `accept_f42_a_`.
//!
//! Every test drives the production path: `cs_content::stunts`'s declared
//! record → `cs_app::stunts::lower_stunt` → `cs_sim::stunts::StuntBook`.
//! There is no test-only gate, predicate or ledger here; the synthetic fixture
//! is the same function the catalog would call.
//!
//! The minimum scenario — **AC01: "Fly through, beside, backwards and
//! teleport across the same synthetic gate; only eligible traversals count"**
//! — is `accept_f42_a_through_counts_and_beside_backwards_and_teleport_do_not`,
//! which flies all four at the same gate and asserts exactly one completion.
//!
//! No original data and no `CS_GAME_DIR` access: the fixture is
//! `Origin::SyntheticFixture` with a `GateEvidence::Reconstructed` volume, so
//! nothing here is an original-fidelity claim.

use cs_app::stunts::{StuntLowerError, lower_mission_stunts, lower_stunt};
use cs_content::stunts::{
    Gate, GateEvidence, MissionScope, SYNTHETIC_GATE_HALF_HEIGHT_M, SYNTHETIC_GATE_HALF_WIDTH_M,
    SYNTHETIC_GATE_MIN_CLEARANCE_M, SYNTHETIC_GATE_MIN_FORWARD_COSINE, StuntDefinition, StuntDraft,
    StuntRepeat, StuntReward, declared_synthetic_gate_stunt, synthetic_gate_mission,
    synthetic_gate_world,
};
use cs_script::ir::ActorId;
use cs_sim::campaign::{ProfileId, SessionGeneration};
use cs_sim::stunts::{
    Admission, Gate as RuntimeGate, GateRefusal, PassRefusal, StuntAuthority, StuntBook,
    StuntLedger, StuntMovement, StuntObserveError, StuntRepeat as RuntimeRepeat, TraversalOutcome,
    TraversalRequest,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::WorldPosition;

// --------------------------------------------------------------- helpers ---

fn pos(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("the test position is finite")
}

const SUBJECT: ActorId = ActorId(1);
const CAMERA: ActorId = ActorId(2);
const SESSION: SessionGeneration = SessionGeneration(1);

fn profile(name: &str) -> ProfileId {
    ProfileId::new(name).expect("the test profile id is valid")
}

fn mission(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Mission, key).expect("the test mission id is valid")
}

fn designed() -> Provenance {
    Provenance::designed(ClaimId::new("f42a.acceptance").expect("valid claim id"))
}

/// A book over the lowered declared fixture, the production path every test
/// below uses.
fn book(rules: Vec<cs_sim::stunts::StuntRule>, ledger: StuntLedger) -> StuntBook {
    StuntBook::new(
        SESSION,
        profile("profile-1"),
        synthetic_gate_mission(),
        SUBJECT,
        rules,
        ledger,
    )
    .expect("the fixture book is valid")
}

fn fixture_book() -> StuntBook {
    book(
        vec![lower_stunt(&declared_synthetic_gate_stunt()).expect("the fixture lowers")],
        StuntLedger::new(SESSION, Vec::new()),
    )
}

/// A player-flight sample at `tick`.
fn swept(tick: u64, from_m: [f64; 3], to_m: [f64; 3]) -> TraversalRequest {
    TraversalRequest::player_flight(
        SESSION,
        SUBJECT,
        Tick(tick),
        StuntMovement::Swept {
            from_m: pos(from_m),
            to_m: pos(to_m),
        },
    )
}

/// The single outcome of the one-rule fixture book.
fn only(outcomes: &[TraversalOutcome]) -> &TraversalOutcome {
    assert_eq!(outcomes.len(), 1, "the fixture book holds one rule");
    &outcomes[0]
}

/// The refusal the fixture rule reports for a geometric failure.
fn geometry(reason: GateRefusal) -> PassRefusal {
    PassRefusal::Geometry {
        stunt: "stunt/synthetic.flyby-gate".to_owned(),
        reason,
    }
}

fn expect_refused(outcomes: &[TraversalOutcome]) -> &PassRefusal {
    match only(outcomes) {
        TraversalOutcome::Refused(reason) => reason,
        TraversalOutcome::Completed(done) => panic!(
            "the traversal completed {stunt:?} at {crossing:?}, but it had to be refused",
            stunt = done.stunt,
            crossing = done.crossing_m
        ),
    }
}

// ------------------------------------------------------------- the tests ---

/// **AC01: fly through, beside, backwards and teleport across the same
/// synthetic gate; only eligible traversals count.** Exactly the forward
/// pass completes; the other three are refused each with its own named
/// reason, and the book reports one completion.
#[test]
fn accept_f42_a_through_counts_and_beside_backwards_and_teleport_do_not() {
    let mut book = fixture_book();
    // Straight down the canonical forward axis through the aperture centre.
    let outcomes = book
        .observe(&swept(1, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]))
        .unwrap();
    let TraversalOutcome::Completed(done) = only(&outcomes) else {
        panic!("the axial pass must complete, got {:?}", only(&outcomes));
    };
    assert_eq!(done.stunt.as_str(), "stunt/synthetic.flyby-gate");
    assert_eq!(done.mission, synthetic_gate_mission());
    assert_eq!(done.actor, SUBJECT);
    assert_eq!(done.crossing_m, [0.0, 0.0, 0.0]);
    assert!((done.forward_cosine - 1.0).abs() < 1.0e-12);
    // The fixture's margin is 2 m; the axial pass keeps the whole half-width.
    assert!(
        (done.clearance_m - SYNTHETIC_GATE_HALF_WIDTH_M.min(SYNTHETIC_GATE_HALF_HEIGHT_M)).abs()
            < 1.0e-12
    );
    assert_eq!(book.completions(), 1);

    // Beside the gate: 200 m to the right of a 24 m wide hole, at the same
    // speed and direction.
    let beside = book
        .observe(&swept(2, [200.0, 0.0, 200.0], [200.0, 0.0, -200.0]))
        .unwrap();
    assert_eq!(expect_refused(&beside), &geometry(GateRefusal::MissedGate));

    // Backwards: the same line, reversed, so the travel cosine is -1 against a
    // rule of +0.5.
    let backwards = book
        .observe(&swept(3, [0.0, 0.0, -200.0], [0.0, 0.0, 200.0]))
        .unwrap();
    assert_eq!(
        expect_refused(&backwards),
        &geometry(GateRefusal::WrongDirection {
            forward_cosine: -1.0,
            required: SYNTHETIC_GATE_MIN_FORWARD_COSINE,
        })
    );

    // Teleport: from outside one side straight to outside the other. It
    // crosses the plane in space and earns nothing.
    let teleport = book
        .observe(&TraversalRequest::player_flight(
            SESSION,
            SUBJECT,
            Tick(4),
            StuntMovement::Teleport {
                to_m: pos([0.0, 0.0, -400.0]),
            },
        ))
        .unwrap();
    assert_eq!(
        expect_refused(&teleport),
        &PassRefusal::Discontinuous {
            stunt: "stunt/synthetic.flyby-gate".to_owned()
        }
    );

    assert_eq!(book.completions(), 1, "only the eligible traversal counted");
    assert_eq!(book.ledger().len(), 1);
}

/// **A rebase does not break a genuine continuous passage** (sheet behavior
/// 1). The passage is completed by two segments with the origin rebase
/// reported between them: world identity survives the frame change (F16
/// `OriginChange::Rebase`), so the gate is crossed exactly as it would have
/// been without the rebase. A teleport that jumps from the same place to the
/// same place earns nothing, which is what makes this a rebase test rather
/// than a "second segment" test.
#[test]
fn accept_f42_a_a_rebase_does_not_break_a_genuine_passage() {
    let mut book = fixture_book();
    // Tick 1 flies to 100 m short of the gate. Nothing is completed yet.
    let approach = book
        .observe(&swept(1, [0.0, 0.0, 300.0], [0.0, 0.0, 100.0]))
        .unwrap();
    assert_eq!(
        expect_refused(&approach),
        &geometry(GateRefusal::MissedGate),
        "stopping short of the gate completes nothing"
    );
    assert_eq!(book.completions(), 0);

    // Tick 2 carries the origin rebase: the same world-identity segment from
    // 100 m to -100 m, which crosses the gate.
    let rebased = book
        .observe(&TraversalRequest::player_flight(
            SESSION,
            SUBJECT,
            Tick(2),
            StuntMovement::Rebased {
                from_m: pos([0.0, 0.0, 100.0]),
                to_m: pos([0.0, 0.0, -100.0]),
            },
        ))
        .unwrap();
    let TraversalOutcome::Completed(done) = only(&rebased) else {
        panic!(
            "a rebase must not break the passage, got {:?}",
            only(&rebased)
        );
    };
    assert_eq!(done.crossing_m, [0.0, 0.0, 0.0]);
    assert!((done.forward_cosine - 1.0).abs() < 1.0e-12);
    assert_eq!(book.completions(), 1);

    // The same jump declared as a teleport, from the same place to the same
    // place, earns nothing — the distinction is the movement kind, not the
    // distance or the geometry.
    let mut teleported = fixture_book();
    let jumped = teleported
        .observe(&TraversalRequest::player_flight(
            SESSION,
            SUBJECT,
            Tick(1),
            StuntMovement::Teleport {
                to_m: pos([0.0, 0.0, -100.0]),
            },
        ))
        .unwrap();
    assert_eq!(
        expect_refused(&jumped),
        &PassRefusal::Discontinuous {
            stunt: "stunt/synthetic.flyby-gate".to_owned()
        }
    );
    assert_eq!(teleported.completions(), 0);
}

/// **AC02: complete a stunt twice and retry the mission; the reward
/// duplication policy is correct.** A one-time stunt pays once: the second
/// pass in the same session and a *new session* over a ledger seeded with the
/// persisted key both refuse, and a repeatable stunt pays each time.
#[test]
fn accept_f42_a_one_time_reward_pays_once_across_a_retry_but_a_repeatable_one_pays_again() {
    let record = declared_synthetic_gate_stunt();
    let once = lower_stunt(&record).expect("the fixture lowers");
    let key_first = cs_sim::stunts::StuntRewardKey::new(
        profile("profile-1"),
        synthetic_gate_mission(),
        record.id().clone(),
    );

    // First session: the pass pays and the identity is recorded.
    let mut first = book(vec![once.clone()], StuntLedger::new(SESSION, Vec::new()));
    let first_pass = first
        .observe(&swept(1, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]))
        .unwrap();
    let TraversalOutcome::Completed(done) = only(&first_pass) else {
        panic!("the first pass must pay");
    };
    assert_eq!(done.key, key_first);
    assert_eq!(
        done.reward.media.as_ref().map(ContentId::as_str),
        Some("scrapbook_item/synthetic.flyby-photo")
    );
    let paid = first.ledger().paid().cloned().collect::<Vec<_>>();
    assert_eq!(paid.as_slice(), std::slice::from_ref(&key_first));

    // Second pass, same session: the stunt is flown again but pays nothing,
    // and a completion is not counted twice.
    let repeat = first
        .observe(&swept(2, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]))
        .unwrap();
    assert_eq!(
        expect_refused(&repeat),
        &PassRefusal::AlreadyRewarded {
            stunt: "stunt/synthetic.flyby-gate".to_owned(),
            key: key_first.clone(),
        }
    );
    assert_eq!(first.completions(), 1);
    assert_eq!(first.ledger().len(), 1);

    // Retry: a new session generation over a ledger seeded from the
    // persisted key. Same identity, so the one-time photo cannot pay again.
    let retry_session = SessionGeneration(2);
    let mut retry = StuntBook::new(
        retry_session,
        profile("profile-1"),
        synthetic_gate_mission(),
        SUBJECT,
        vec![once],
        StuntLedger::new(retry_session, paid),
    )
    .unwrap();
    let replay = retry
        .observe(&TraversalRequest::player_flight(
            retry_session,
            SUBJECT,
            Tick(1),
            StuntMovement::Swept {
                from_m: pos([0.0, 0.0, 200.0]),
                to_m: pos([0.0, 0.0, -200.0]),
            },
        ))
        .unwrap();
    assert_eq!(
        expect_refused(&replay),
        &PassRefusal::AlreadyRewarded {
            stunt: "stunt/synthetic.flyby-gate".to_owned(),
            key: key_first.clone(),
        }
    );
    assert_eq!(retry.completions(), 0);
    assert_eq!(retry.ledger().len(), 1, "the retry added no identity");

    // A repeatable stunt, by contrast, pays on every pass. Same gate, same
    // session, fresh identity: the ledger records both passes and never
    // refuses one.
    let mut repeating = book(
        vec![lower_stunt(&repeatable_record()).expect("the repeatable fixture lowers")],
        StuntLedger::new(SESSION, Vec::new()),
    );
    for tick in 1..=2 {
        let outcomes = repeating
            .observe(&swept(tick, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]))
            .unwrap();
        let TraversalOutcome::Completed(done) = only(&outcomes) else {
            panic!("a repeatable stunt must pay on pass {tick}");
        };
        assert_eq!(done.stunt.as_str(), "stunt/synthetic.repeatable-gate");
    }
    assert_eq!(repeating.completions(), 2);
    assert_eq!(repeating.ledger().len(), 1, "one identity, two payments");
    // The repeating book holds its own identity, so the one-time gate stunt's
    // key is still unseen there and is granted once.
    let mut ledger = repeating.ledger().clone();
    assert_eq!(
        ledger.grant(SESSION, key_first.clone()),
        Ok(Admission::Admitted)
    );
    assert_eq!(
        ledger.grant(SESSION, key_first),
        Ok(Admission::AlreadyRewarded)
    );
}

/// **AC03: the same world in another mission has a different eligible set.**
/// The declared fixture is eligible in one synthetic mission, so a book for a
/// second mission over the same world refuses the stunt — and the mission
/// set the lowering boundary builds is empty for that mission.
#[test]
fn accept_f42_a_the_same_world_in_another_mission_has_a_different_eligible_set() {
    let record = declared_synthetic_gate_stunt();
    assert_eq!(
        record.world(),
        &synthetic_gate_world(),
        "the fixture gate lives in one world"
    );

    // The declared mission's set contains the stunt.
    let eligible = lower_mission_stunts(&synthetic_gate_mission(), [&record]).expect("lowers");
    assert_eq!(eligible.len(), 1);
    assert!(eligible[0].is_eligible_in(&synthetic_gate_mission()));

    // A different mission in the same world gets an empty set: the world's
    // stunt is not automatically available there.
    let other_mission = mission("synthetic.m02");
    let in_other = lower_mission_stunts(&other_mission, [&record]).expect("lowers");
    assert!(in_other.is_empty());

    // And if a caller hands that mission a book containing the stunt anyway,
    // the runtime re-checks the scope and refuses with the mission named.
    let mut book = StuntBook::new(
        SESSION,
        profile("profile-1"),
        other_mission,
        SUBJECT,
        vec![lower_stunt(&record).expect("the fixture lowers")],
        StuntLedger::new(SESSION, Vec::new()),
    )
    .unwrap();
    let refused = book
        .observe(&swept(1, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]))
        .unwrap();
    assert_eq!(
        expect_refused(&refused),
        &PassRefusal::MissionNotEligible {
            stunt: "stunt/synthetic.flyby-gate".to_owned(),
            mission: "mission/synthetic.m02".to_owned(),
        }
    );
    assert_eq!(book.completions(), 0);
}

/// A developer camera and a foreign actor cannot earn, and a pass that misses
/// the authored margin or the mid-plane is refused with its measurement
/// attached.
#[test]
fn accept_f42_a_only_the_player_aircraft_counts_and_geometry_failures_carry_measurements() {
    let mut book = fixture_book();

    // A developer camera on the same line, the same tick discipline.
    let mut camera = swept(1, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]);
    camera.authority = StuntAuthority::DeveloperCamera;
    assert_eq!(
        expect_refused(&book.observe(&camera).unwrap()),
        &PassRefusal::NotPlayerFlight {
            authority: StuntAuthority::DeveloperCamera
        }
    );

    // A spectator view and AI flight are likewise not the player's traversal.
    // Each gets its own tick: a book refuses a non-advancing tick outright.
    for (offset, authority) in [StuntAuthority::Spectator, StuntAuthority::AiFlight]
        .into_iter()
        .enumerate()
    {
        let mut sample = swept(2 + offset as u64, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]);
        sample.authority = authority;
        assert_eq!(
            expect_refused(&book.observe(&sample).unwrap()),
            &PassRefusal::NotPlayerFlight { authority }
        );
    }

    // Another actor's flight, even with player authority.
    let mut foreign = swept(4, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]);
    foreign.actor = CAMERA;
    assert_eq!(
        expect_refused(&book.observe(&foreign).unwrap()),
        &PassRefusal::ForeignActor {
            actor: CAMERA,
            subject: SUBJECT
        }
    );

    // Inside the hole but inside the authored 2 m rim margin: the crossing is
    // 11.5 m right, so only 0.5 m of clearance remains.
    let grazing = book
        .observe(&swept(5, [11.5, 0.0, 200.0], [11.5, 0.0, -200.0]))
        .unwrap();
    assert_eq!(
        expect_refused(&grazing),
        &geometry(GateRefusal::InsufficientClearance {
            clearance_m: 0.5,
            required_m: SYNTHETIC_GATE_MIN_CLEARANCE_M,
        })
    );

    // Just outside the authored margin the same line completes, which proves
    // the previous refusal was the margin and not the geometry.
    let clear = book
        .observe(&swept(6, [9.0, 0.0, 200.0], [9.0, 0.0, -200.0]))
        .unwrap();
    let TraversalOutcome::Completed(done) = only(&clear) else {
        panic!("a pass 3 m inside the rim must complete");
    };
    assert!((done.clearance_m - 3.0).abs() < 1.0e-12);
    assert_eq!(done.criticality, cs_sim::stunts::StuntCriticality::Optional);
    assert_eq!(book.completions(), 1);
}

/// A pass above the hole, a pass that grazes the slab without crossing its
/// mid-plane, and a stationary sample are all refused: only a real crossing
/// counts, and each refusal says which of the three it was.
#[test]
fn accept_f42_a_a_miss_a_rim_graze_and_a_stationary_sample_never_count() {
    let mut book = fixture_book();

    // Above the hole, on the gate's own mid-plane: a miss.
    let over = book
        .observe(&swept(1, [0.0, 30.0, 200.0], [0.0, 30.0, -200.0]))
        .unwrap();
    assert_eq!(expect_refused(&over), &geometry(GateRefusal::MissedGate));

    // A graze of the slab that stops short of the mid-plane: the segment never
    // reaches the plane, so it is a miss rather than a half traversal.
    let graze = book
        .observe(&swept(2, [0.0, 0.0, 200.0], [0.0, 0.0, 20.0]))
        .unwrap();
    assert_eq!(expect_refused(&graze), &geometry(GateRefusal::MissedGate));

    // A stationary sample moved nowhere and traversed nothing.
    let still = book
        .observe(&swept(3, [0.0, 0.0, 200.0], [0.0, 0.0, 200.0]))
        .unwrap();
    assert_eq!(expect_refused(&still), &geometry(GateRefusal::NoSweep));

    assert_eq!(book.completions(), 0);
    assert!(book.ledger().is_empty(), "no refusal granted an identity");
}

/// A stale session's delayed sample and a non-advancing tick are refused
/// before any rule is judged, so a retry can neither inherit the previous
/// attempt's completion nor be judged by an out-of-order sample.
#[test]
fn accept_f42_a_a_stale_session_and_a_replayed_tick_are_refused_before_any_rule_runs() {
    let mut book = fixture_book();
    book.observe(&swept(5, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]))
        .unwrap();
    assert_eq!(book.completions(), 1);

    // A delayed callback from the previous attempt's generation.
    let mut stale = swept(6, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0]);
    stale.session = SessionGeneration(0);
    assert_eq!(
        book.observe(&stale),
        Err(StuntObserveError::StaleSession {
            book: SESSION,
            given: SessionGeneration(0),
        })
    );

    // A replayed or out-of-order tick.
    assert_eq!(
        book.observe(&swept(5, [0.0, 0.0, 200.0], [0.0, 0.0, -200.0])),
        Err(StuntObserveError::NotAdvancing {
            last: Tick(5),
            given: Tick(5),
        })
    );
    assert_eq!(book.completions(), 1, "no refused sample changed the book");
    assert_eq!(book.ledger().len(), 1);
}

/// The lowering boundary refuses an unrecovered gate, an unmeasured rule and
/// an unmeasured payout by name, and never lowers a record whose runtime
/// validation would fail.
#[test]
fn accept_f42_a_lowering_refuses_unknowns_instead_of_defaulting_them() {
    let base = declared_synthetic_gate_stunt();

    let unknown_gate = with_draft(&base, |draft| {
        draft.gate = unknown("f42a.test-gate", "no gate volume was recovered");
    });
    assert!(matches!(
        lower_stunt(&unknown_gate),
        Err(StuntLowerError::UnknownGate { claim_id, .. })
            if claim_id == ClaimId::new("f42a.test-gate").expect("valid")
    ));

    let unknown_direction = with_draft(&base, |draft| {
        draft.rules.min_forward_cosine = unknown(
            "f42a.test-direction",
            "the original threshold was not measured",
        );
    });
    assert!(matches!(
        lower_stunt(&unknown_direction),
        Err(StuntLowerError::UnknownDirectionRule { claim_id, .. })
            if claim_id == ClaimId::new("f42a.test-direction").expect("valid")
    ));

    let unknown_clearance = with_draft(&base, |draft| {
        draft.rules.min_clearance_m = unknown("f42a.test-clearance", "no margin was measured");
    });
    assert!(matches!(
        lower_stunt(&unknown_clearance),
        Err(StuntLowerError::UnknownClearanceRule { .. })
    ));

    let unknown_fame = with_draft(&base, |draft| {
        draft.reward.fame = unknown("f42a.test-fame", "no payout was measured");
    });
    assert!(matches!(
        lower_stunt(&unknown_fame),
        Err(StuntLowerError::UnknownFame { .. })
    ));

    let unknown_media = with_draft(&base, |draft| {
        draft.reward.media = unknown("f42a.test-media", "the photo was not linked");
    });
    assert!(matches!(
        lower_stunt(&unknown_media),
        Err(StuntLowerError::UnknownMedia { .. })
    ));

    // A known but corrupt gate never reaches the boundary: the declared
    // constructor refuses it by name, and the runtime gate refuses the same
    // value, so the two halves agree on what usable geometry is.
    let mut corrupt = draft_of(&base);
    corrupt.gate = Resolved::Known(Known::new(
        Gate {
            center_m: [0.0; 3],
            normal: [0.0, 0.0, -1.0],
            right_half_extent_m: 12.0,
            up_half_extent_m: 8.0,
            // A negative depth is not a gate.
            half_depth_m: -3.0,
            evidence: GateEvidence::Reconstructed,
        },
        designed(),
    ));
    assert!(matches!(
        StuntDefinition::try_new(corrupt),
        Err(cs_content::stunts::StuntError::NegativeGateDepth { value: -3.0 })
    ));
    assert!(matches!(
        RuntimeGate::new([0.0; 3], [0.0, 0.0, -1.0], 12.0, 8.0, -3.0),
        Err(cs_sim::stunts::StuntError::BadGeometry {
            field: "half_depth_m"
        })
    ));

    // The intact fixture lowers, and its geometry stays marked as drawn.
    let lowered = lower_stunt(&base).expect("the intact fixture lowers");
    assert_eq!(lowered.id(), base.id());
    assert_eq!(lowered.world(), &synthetic_gate_world());
    assert_eq!(
        lowered.evidence(),
        cs_sim::stunts::GateEvidence::Reconstructed
    );
    assert!(!lowered.evidence().is_measured());
    assert_eq!(lowered.repeat(), RuntimeRepeat::Once);
    assert!(!lowered.criticality().affects_mission_success());
}

/// The runtime gate is a real geometric predicate, not a proximity test: an
/// oblique pass through the middle of the hole clears the authored direction
/// rule, and the same line at 80° off-axis does not.
#[test]
fn accept_f42_a_the_predicate_uses_direction_and_margin_rather_than_proximity() {
    let mut book = fixture_book();

    // 30° off the gate normal, still through the hole's centre: within the
    // authored 60° rule.
    let (from, to) = oblique(30.0);
    let shallow = book.observe(&swept(1, from, to)).unwrap();
    let TraversalOutcome::Completed(done) = only(&shallow) else {
        panic!("a 30° pass through the hole centre must complete");
    };
    assert!((done.forward_cosine - 30f64.to_radians().cos()).abs() < 1.0e-9);
    assert!(done.forward_cosine >= SYNTHETIC_GATE_MIN_FORWARD_COSINE);

    // 80° off-axis, the same line: geometrically through the hole, but far
    // outside the authored direction rule.
    let (from, to) = oblique(80.0);
    let refused = book.observe(&swept(2, from, to)).unwrap();
    let PassRefusal::Geometry {
        reason:
            GateRefusal::WrongDirection {
                forward_cosine,
                required,
            },
        ..
    } = expect_refused(&refused)
    else {
        panic!("an 80° pass must be refused for its direction, got {refused:?}");
    };
    assert!((forward_cosine - 80f64.to_radians().cos()).abs() < 1.0e-9);
    assert!(forward_cosine < required);
    assert_eq!(book.completions(), 1);
}

/// A two-rule book judges both rules and refuses a sample that a foreign
/// actor produced without needing a second traversal, while a duplicate
/// stunt id in one set is refused as ambiguous.
#[test]
fn accept_f42_a_a_duplicate_stunt_id_in_one_book_is_refused() {
    let once = lower_stunt(&declared_synthetic_gate_stunt()).expect("lowers");
    let error = StuntBook::new(
        SESSION,
        profile("profile-1"),
        synthetic_gate_mission(),
        SUBJECT,
        vec![once.clone(), once.clone()],
        StuntLedger::new(SESSION, Vec::new()),
    )
    .unwrap_err();
    assert_eq!(
        error,
        StuntObserveError::DuplicateStunt {
            id: "stunt/synthetic.flyby-gate".to_owned()
        }
    );
    // A non-mission book is refused by name too.
    assert!(matches!(
        StuntBook::new(
            SESSION,
            profile("profile-1"),
            ContentId::from_source(ContentKind::World, "synthetic.harbor").expect("valid id"),
            SUBJECT,
            vec![once],
            StuntLedger::new(SESSION, Vec::new()),
        ),
        Err(StuntObserveError::CorruptLedger { .. })
    ));
}

// -------------------------------------------------------------- fixtures ---

/// A declared stunt over the same gate that pays on every pass.
fn repeatable_record() -> StuntDefinition {
    let base = declared_synthetic_gate_stunt();
    with_draft(&base, |draft| {
        draft.id = ContentId::from_source(ContentKind::Stunt, "synthetic.repeatable-gate")
            .expect("valid id");
        draft.repeat = StuntRepeat::Repeatable;
    })
}

/// The editable draft of a declared record, for the tests that need to change
/// one field.
fn draft_of(record: &StuntDefinition) -> StuntDraft {
    StuntDraft {
        id: record.id().clone(),
        origin: record.origin().clone(),
        world: record.world().clone(),
        gate: record.gate().clone(),
        rules: record.rules().clone(),
        scope: record.scope().clone(),
        criticality: record.criticality(),
        repeat: record.repeat(),
        reward: record.reward().clone(),
        provenance: record.provenance().clone(),
    }
}

/// Rebuilds a declared record from the fixture with one field replaced. The
/// edited draft must still be valid — a test that wants an invalid record
/// asserts the refusal itself instead.
fn with_draft(record: &StuntDefinition, edit: impl FnOnce(&mut StuntDraft)) -> StuntDefinition {
    let mut draft = draft_of(record);
    edit(&mut draft);
    StuntDefinition::try_new(draft).expect("the edited draft is still valid")
}

/// An explicit unknown for any resolved field, carrying its claim and reason.
fn unknown<T>(claim: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(ClaimId::new(claim).expect("valid claim id"), reason)
        .expect("a reason is present")
}

/// A segment through the gate centre at `degrees` off the gate normal.
fn oblique(degrees: f64) -> ([f64; 3], [f64; 3]) {
    let radians = degrees.to_radians();
    // The gate faces -Z; travel is -Z rotated towards +X by `degrees`, with
    // the segment centred on the aperture.
    let (sin, cos) = (radians.sin(), radians.cos());
    let travel = [sin, 0.0, -cos];
    let start = [-travel[0] * 200.0, -travel[1] * 200.0, -travel[2] * 200.0];
    let end = [travel[0] * 200.0, travel[1] * 200.0, travel[2] * 200.0];
    (start, end)
}

// A direct use of the runtime gate keeps the acceptance file honest about
// the geometry it depends on.
#[test]
fn accept_f42_a_the_runtime_gate_classifies_a_segment_against_its_own_rules() {
    let gate = RuntimeGate::new([0.0; 3], [0.0, 0.0, -1.0], 12.0, 8.0, 4.0).expect("a usable gate");
    // The gate's own frame: -Z normal gives +X right and +Y up.
    assert_eq!(gate.gate_frame(pos([3.0, 4.0, -2.0])), [3.0, 4.0, 2.0]);
    let passage = gate
        .classify(
            pos([0.0, 0.0, 100.0]),
            pos([0.0, 0.0, -100.0]),
            SYNTHETIC_GATE_MIN_FORWARD_COSINE,
            SYNTHETIC_GATE_MIN_CLEARANCE_M,
        )
        .expect("the axial pass is a passage");
    assert!((passage.clearance_m - 8.0).abs() < 1.0e-12);
    // Beyond the half depth, the segment never reaches the mid-plane.
    assert_eq!(
        gate.classify(
            pos([0.0, 0.0, 100.0]),
            pos([0.0, 0.0, 50.0]),
            SYNTHETIC_GATE_MIN_FORWARD_COSINE,
            SYNTHETIC_GATE_MIN_CLEARANCE_M,
        ),
        Err(GateRefusal::MissedGate)
    );
    // A zero-length segment moved nowhere.
    assert_eq!(
        gate.classify(
            pos([0.0, 0.0, 100.0]),
            pos([0.0, 0.0, 100.0]),
            SYNTHETIC_GATE_MIN_FORWARD_COSINE,
            SYNTHETIC_GATE_MIN_CLEARANCE_M,
        ),
        Err(GateRefusal::NoSweep)
    );
    // A rule outside its documented range is a corrupt record, not a refusal
    // of the flight.
    assert_eq!(
        gate.classify(pos([0.0, 0.0, 100.0]), pos([0.0, 0.0, -100.0]), 2.0, 0.0),
        Err(GateRefusal::BadRule)
    );
}

// Keep the scope helper referenced: the acceptance file builds its own
// eligible set for a second mission above, and this proves the scope really
// is a list with no wildcard.
#[test]
fn accept_f42_a_a_scope_cannot_be_emptied_or_widened_to_every_mission() {
    let record = declared_synthetic_gate_stunt();
    assert_eq!(record.scope().len(), 1);
    assert!(!record.scope().is_empty());
    assert!(record.scope().contains(&synthetic_gate_mission()));
    assert!(!record.scope().contains(&mission("synthetic.m02")));
    // The declared set for the other mission is genuinely empty rather than
    // "all missions".
    assert!(record.scope().missions().is_empty() || record.scope().len() == 1);
    // An explicit unknown scope entry is refused; there is no wildcard id to
    // smuggle one through.
    assert!(
        MissionScope::try_new(Vec::new()).is_err(),
        "an empty scope is refused, so a world stunt is never mission-universal"
    );
}

// A declared reward that pays nothing is still a valid, lowerable record, and
// it is distinguishable from an unmeasured one.
#[test]
fn accept_f42_a_a_declared_empty_reward_lowers_and_is_distinct_from_an_unknown_one() {
    let base = declared_synthetic_gate_stunt();
    let empty = with_draft(&base, |draft| {
        draft.reward = StuntReward::nothing();
    });
    let lowered = lower_stunt(&empty).expect("an empty reward is content, not an error");
    assert!(lowered.reward().is_empty());
    assert_eq!(lowered.reward().fame, 0);
    assert!(lowered.reward().media.is_none());

    let unmeasured = with_draft(&base, |draft| {
        draft.rules.min_clearance_m = unknown("f42a.empty-test", "no margin measured");
    });
    assert!(matches!(
        lower_stunt(&unmeasured),
        Err(StuntLowerError::UnknownClearanceRule { .. })
    ));
}

// The declared rules vocabulary is carried, not reinterpreted: the fixture's
// thresholds reach the runtime rule unchanged.
#[test]
fn accept_f42_a_the_declared_rules_reach_the_runtime_rule_unchanged() {
    let record = declared_synthetic_gate_stunt();
    let rules = record.rules();
    assert_eq!(
        rules.min_forward_cosine.clone().known(),
        Some(SYNTHETIC_GATE_MIN_FORWARD_COSINE)
    );
    assert_eq!(
        rules.min_clearance_m.clone().known(),
        Some(SYNTHETIC_GATE_MIN_CLEARANCE_M)
    );
    let lowered = lower_stunt(&record).expect("the fixture lowers");
    assert_eq!(
        lowered.rule().min_forward_cosine(),
        SYNTHETIC_GATE_MIN_FORWARD_COSINE
    );
    assert_eq!(
        lowered.rule().min_clearance_m(),
        SYNTHETIC_GATE_MIN_CLEARANCE_M
    );
    assert_eq!(lowered.rule().gate().right_half_extent_m(), 12.0);
    assert_eq!(lowered.rule().gate().up_half_extent_m(), 8.0);
    assert_eq!(lowered.rule().gate().half_depth_m(), 4.0);
    // The gate's in-plane basis is derived, so the declared normal is all the
    // authoring needs.
    assert_eq!(lowered.rule().gate().right().to_array(), [1.0, 0.0, 0.0]);
    assert_eq!(lowered.rule().gate().up().to_array(), [0.0, 1.0, 0.0]);
}
