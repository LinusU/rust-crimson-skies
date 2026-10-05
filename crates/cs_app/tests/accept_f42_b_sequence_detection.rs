//! F42-B acceptance: swept and sequence stunt detection (synthetic).
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-B`. Task test prefix: `accept_f42_b_`.
//!
//! Every test drives the production path: the declared
//! `cs_content::stunts` record → `cs_app::stunts::lower_stunt` →
//! `cs_sim::stunts::StuntBook`, fed by `StuntSampler` where a test is about
//! per-tick poses. The minimum scenario — **complete a stunt twice and retry
//! the mission; the reward duplication policy is correct** — is
//! `accept_f42_b_a_sequence_stunt_completed_twice_and_retried_pays_by_policy`.
//!
//! The synthetic sequence is a designed fixture (`Origin::SyntheticFixture`,
//! reconstructed geometry): how the original spells an ordered sequence of
//! gates is not measured, so nothing here is an original-fidelity claim.

use cs_app::stunts::lower_stunt;
use cs_content::stunts::{
    Gate, GateEvidence, StuntDefinition, StuntDraft, StuntError, StuntRepeat,
    declared_synthetic_gate_stunt, synthetic_gate_mission,
};
use cs_script::ir::ActorId;
use cs_sim::campaign::{ProfileId, SessionGeneration};
use cs_sim::stunts::{
    GateRefusal, PassRefusal, PoseChange, StuntBook, StuntLedger, StuntRule, StuntSampler,
    TraversalOutcome, TraversalRequest,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::space::WorldPosition;

const SUBJECT: ActorId = ActorId(1);
const SESSION: SessionGeneration = SessionGeneration(1);

fn pos(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("the test position is finite")
}

fn second_gate() -> Gate {
    Gate {
        center_m: [0.0, 0.0, -400.0],
        normal: [0.0, 0.0, -1.0],
        right_half_extent_m: 12.0,
        up_half_extent_m: 8.0,
        half_depth_m: 4.0,
        evidence: GateEvidence::Reconstructed,
    }
}

fn draft_of(record: &StuntDefinition) -> StuntDraft {
    StuntDraft {
        id: record.id().clone(),
        origin: record.origin().clone(),
        world: record.world().clone(),
        gate: record.gate().clone(),
        follow_on_gates: record.follow_on_gates().to_vec(),
        rules: record.rules().clone(),
        scope: record.scope().clone(),
        criticality: record.criticality(),
        repeat: record.repeat(),
        reward: record.reward().clone(),
        provenance: record.provenance().clone(),
    }
}

/// The synthetic gate followed by a second gate 400 m further down -Z.
fn sequence_record(repeat: StuntRepeat) -> StuntDefinition {
    let mut draft = draft_of(&declared_synthetic_gate_stunt());
    draft.id = ContentId::from_source(ContentKind::Stunt, "synthetic.slalom").expect("valid id");
    draft.follow_on_gates = vec![second_gate()];
    draft.repeat = repeat;
    StuntDefinition::try_new(draft).expect("the sequence record is valid")
}

fn sequence_rule(repeat: StuntRepeat) -> StuntRule {
    lower_stunt(&sequence_record(repeat)).expect("the sequence record lowers")
}

fn book_in(
    session: SessionGeneration,
    rule: StuntRule,
    paid: Vec<cs_sim::stunts::StuntRewardKey>,
) -> StuntBook {
    StuntBook::new(
        session,
        ProfileId::new("profile-1").expect("valid profile"),
        synthetic_gate_mission(),
        SUBJECT,
        vec![rule],
        StuntLedger::new(session, paid),
    )
    .expect("the book is valid")
}

fn swept(tick: u64, from_z: f64, to_z: f64) -> TraversalRequest {
    TraversalRequest::player_flight(
        SESSION,
        SUBJECT,
        Tick(tick),
        cs_sim::stunts::StuntMovement::Swept {
            from_m: pos([0.0, 0.0, from_z]),
            to_m: pos([0.0, 0.0, to_z]),
        },
    )
}

fn only(outcomes: &[TraversalOutcome]) -> &TraversalOutcome {
    assert_eq!(outcomes.len(), 1);
    &outcomes[0]
}

fn assert_advanced(outcomes: &[TraversalOutcome], expected: usize) {
    let TraversalOutcome::Advanced { passed, of, .. } = only(outcomes) else {
        panic!("expected an advance, got {:?}", only(outcomes));
    };
    assert_eq!((*passed, *of), (expected, 2));
}

fn assert_completed(outcomes: &[TraversalOutcome]) {
    assert!(
        matches!(only(outcomes), TraversalOutcome::Completed(_)),
        "expected a completion, got {:?}",
        only(outcomes)
    );
}

/// Gate one on `tick`, gate two on `tick + 1`: the whole slalom in order.
fn fly_both(book: &mut StuntBook, tick: u64) -> Vec<TraversalOutcome> {
    assert_advanced(&book.observe(&swept(tick, 200.0, -200.0)).unwrap(), 1);
    book.observe(&swept(tick + 1, -200.0, -600.0)).unwrap()
}

#[test]
fn accept_f42_b_a_sequence_pays_only_after_every_gate_in_order() {
    let mut book = book_in(SESSION, sequence_rule(StuntRepeat::Once), Vec::new());
    assert_advanced(&book.observe(&swept(1, 200.0, -200.0)).unwrap(), 1);
    assert_eq!(book.completions(), 0, "the first gate pays nothing");
    assert!(book.ledger().is_empty());

    let TraversalOutcome::Completed(done) = &book.observe(&swept(2, -200.0, -600.0)).unwrap()[0]
    else {
        panic!("the last gate must complete the stunt");
    };
    assert_eq!(
        done.crossing_m,
        [0.0, 0.0, -400.0],
        "measured at the last gate"
    );
    assert_eq!(book.completions(), 1);
    assert_eq!(book.ledger().len(), 1);
}

#[test]
fn accept_f42_b_the_second_gate_alone_or_a_teleport_between_gates_never_counts() {
    // Out of order: the second gate without the first.
    let mut book = book_in(SESSION, sequence_rule(StuntRepeat::Once), Vec::new());
    let outcomes = book.observe(&swept(1, -200.0, -600.0)).unwrap();
    assert!(matches!(
        only(&outcomes),
        TraversalOutcome::Refused(PassRefusal::Geometry {
            reason: GateRefusal::MissedGate,
            ..
        })
    ));
    assert_eq!(book.completions(), 0);

    // A teleport between the gates forgets the first one.
    let mut sampler = StuntSampler::new();
    let mut book = book_in(SESSION, sequence_rule(StuntRepeat::Once), Vec::new());
    let mut tick = 0;
    let mut feed = |book: &mut StuntBook, z: f64, change: PoseChange| {
        tick += 1;
        let movement = sampler.advance(pos([0.0, 0.0, z]), change);
        book.observe(&TraversalRequest::player_flight(
            SESSION,
            SUBJECT,
            Tick(tick),
            movement,
        ))
        .unwrap()
    };
    feed(&mut book, 200.0, PoseChange::Flight);
    assert_advanced(&feed(&mut book, -200.0, PoseChange::Flight), 1);
    // Respawn just before the second gate, then fly through it.
    feed(&mut book, -300.0, PoseChange::Teleport);
    let outcomes = feed(&mut book, -600.0, PoseChange::Flight);
    assert!(
        matches!(only(&outcomes), TraversalOutcome::Refused(_)),
        "the sequence must have restarted, got {:?}",
        only(&outcomes)
    );
    assert_eq!(book.completions(), 0);
}

#[test]
fn accept_f42_b_looping_back_through_the_first_gate_restarts_the_sequence() {
    let mut book = book_in(SESSION, sequence_rule(StuntRepeat::Once), Vec::new());
    assert_advanced(&book.observe(&swept(1, 200.0, -200.0)).unwrap(), 1);
    // The pilot misses gate two and comes back round through gate one.
    assert_advanced(&book.observe(&swept(2, 200.0, -200.0)).unwrap(), 1);
    assert_completed(&book.observe(&swept(3, -200.0, -600.0)).unwrap());
}

#[test]
fn accept_f42_b_a_rebased_pose_stream_completes_a_sequence_and_the_first_sample_cannot() {
    let mut sampler = StuntSampler::new();
    let mut book = book_in(SESSION, sequence_rule(StuntRepeat::Once), Vec::new());
    let stream = [
        (200.0, PoseChange::Flight),
        (-200.0, PoseChange::Flight),
        // The origin rebases while the aircraft is between the gates.
        (-300.0, PoseChange::Rebase),
        (-600.0, PoseChange::Flight),
    ];
    let mut last = Vec::new();
    for (tick, (z, change)) in (1..).zip(stream) {
        let movement = sampler.advance(pos([0.0, 0.0, z]), change);
        last = book
            .observe(&TraversalRequest::player_flight(
                SESSION,
                SUBJECT,
                Tick(tick),
                movement,
            ))
            .unwrap();
    }
    assert_completed(&last);

    // A sampler with no history cannot invent a path through a gate.
    let mut fresh = StuntSampler::new();
    assert!(
        !fresh
            .advance(pos([0.0, 0.0, -200.0]), PoseChange::Flight)
            .is_continuous()
    );
    assert!(
        fresh
            .advance(pos([0.0, 0.0, -300.0]), PoseChange::Flight)
            .is_continuous()
    );
    fresh.reset();
    assert!(
        !fresh
            .advance(pos([0.0, 0.0, -400.0]), PoseChange::Flight)
            .is_continuous()
    );
}

#[test]
fn accept_f42_b_a_sequence_stunt_completed_twice_and_retried_pays_by_policy() {
    let once = sequence_rule(StuntRepeat::Once);
    let mut first = book_in(SESSION, once.clone(), Vec::new());
    assert_completed(&fly_both(&mut first, 1));
    let paid: Vec<_> = first.ledger().paid().cloned().collect();
    assert_eq!(paid.len(), 1);

    // Same session, second lap: the whole sequence is flown again and the
    // one-time reward is refused rather than paid or counted.
    let second = fly_both(&mut first, 3);
    assert!(matches!(
        only(&second),
        TraversalOutcome::Refused(PassRefusal::AlreadyRewarded { .. })
    ));
    assert_eq!(first.completions(), 1);

    // Retry the mission: new session, ledger seeded from the profile.
    let retry = SessionGeneration(2);
    let mut book = book_in(retry, once, paid);
    let request = |tick, from_z, to_z| {
        TraversalRequest::player_flight(
            retry,
            SUBJECT,
            Tick(tick),
            cs_sim::stunts::StuntMovement::Swept {
                from_m: pos([0.0, 0.0, from_z]),
                to_m: pos([0.0, 0.0, to_z]),
            },
        )
    };
    assert_advanced(&book.observe(&request(1, 200.0, -200.0)).unwrap(), 1);
    let replay = book.observe(&request(2, -200.0, -600.0)).unwrap();
    assert!(matches!(
        only(&replay),
        TraversalOutcome::Refused(PassRefusal::AlreadyRewarded { .. })
    ));
    assert_eq!(book.completions(), 0);
    assert_eq!(book.ledger().len(), 1);

    // A repeatable sequence pays on every lap.
    let mut repeating = book_in(SESSION, sequence_rule(StuntRepeat::Repeatable), Vec::new());
    assert_completed(&fly_both(&mut repeating, 1));
    assert_completed(&fly_both(&mut repeating, 3));
    assert_eq!(repeating.completions(), 2);
    assert_eq!(repeating.ledger().len(), 1);
}

#[test]
fn accept_f42_b_the_declared_sequence_is_validated_and_lowered_in_order() {
    let rule = sequence_rule(StuntRepeat::Once);
    assert_eq!(rule.gate_count(), 2);
    assert!(rule.gate_rule(2).is_none());
    assert_eq!(
        rule.gate_rule(1)
            .expect("second gate")
            .gate()
            .center_m()
            .z(),
        -400.0
    );
    // The single-gate fixture is unchanged.
    let single = lower_stunt(&declared_synthetic_gate_stunt()).unwrap();
    assert_eq!(single.gate_count(), 1);

    let mut broken = draft_of(&declared_synthetic_gate_stunt());
    let mut gate = second_gate();
    gate.normal = [0.0, 0.0, 0.0];
    broken.follow_on_gates = vec![gate];
    assert_eq!(
        StuntDefinition::try_new(broken).unwrap_err(),
        StuntError::ZeroGateNormal
    );

    let mut narrow = draft_of(&declared_synthetic_gate_stunt());
    let mut gate = second_gate();
    gate.up_half_extent_m = 1.0; // narrower than the 2 m margin
    narrow.follow_on_gates = vec![gate];
    assert!(matches!(
        StuntDefinition::try_new(narrow).unwrap_err(),
        StuntError::UnsatisfiableClearance { .. }
    ));
}
