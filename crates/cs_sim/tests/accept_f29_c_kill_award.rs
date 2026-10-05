//! F29-C.3 acceptance: `DamageEventKind::KillAwarded` reaches the session's
//! score ledger exactly once.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C` (scoring), task F29-C.3. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md` ("Results, previous targets and
//! delayed callbacks are always generation-qualified"). Task test prefix:
//! `accept_f29_c_kill_`.
//!
//! These tests drive production code on both sides of the wiring: the real
//! [`DamageResolver`] produces the award (the same AC01 fixture F29-A pins),
//! and [`NetStateLedger::apply_kill_awards`] / [`NetStateLedger::
//! record_kill_award`] are the consumer that scores it. Deleting the consumer,
//! scoring on any lifecycle transition, or skipping the session check makes
//! one of them fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data; the score *value* is a designed one (see the test at the
//! bottom), because the original's per-kill scoring numbers are unmeasured.

use cs_sim::damage::{
    AttributionRule, DamageChannel, DamageEvent, DamageEventKind, DamagePolicy, DamageResolver,
    HitEvent, HitEventId, LifecycleKind, TickResolution, synthetic_airframe_graph,
};
use cs_sim::net_state::{
    KillAward, KillScoreError, KillScoreValue, NetActorState, NetLifecycle, NetStateError,
    NetStateLedger,
};
use cs_types::Tick;
use cs_types::net::{ActorAllocator, ActorId, EventId, SessionId};
use cs_types::space::{Quaternion, WorldPosition};

const SESSION: SessionId = match SessionId::new(7) {
    Some(id) => id,
    None => unreachable!(),
};

/// A session generation this ledger does not own: the "restart" a previous
/// generation's award must never be applied to.
const FOREIGN: SessionId = match SessionId::new(8) {
    Some(id) => id,
    None => unreachable!(),
};

const RESOLVER_PRODUCER: u32 = 1;
const RESOLVED_TICK: u64 = 5;

/// A ledger with a live victim and a live attacker, ids from the real
/// allocator so both are session-qualified like the production path's.
fn score_ledger() -> (NetStateLedger, ActorId, ActorId) {
    let mut ledger = NetStateLedger::new(SESSION);
    let mut allocator = ActorAllocator::new(SESSION);
    let victim = allocator.allocate().expect("serial space");
    let attacker = allocator.allocate().expect("serial space");
    for actor in [victim, attacker] {
        let state = NetActorState::spawn(
            actor,
            WorldPosition::try_new([0.0, 100.0, 0.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        );
        ledger.spawn(state).expect("the spawn succeeds");
    }
    (ledger, victim, attacker)
}

fn key(name: &str) -> cs_sim::damage::DamageNodeKey {
    cs_sim::damage::DamageNodeKey::new(name).expect("test node keys are valid")
}

#[allow(clippy::too_many_arguments)]
fn hit(
    producer: u32,
    sequence: u32,
    attacker: ActorId,
    target: ActorId,
    node: &str,
    tick: u64,
) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: SESSION,
            tick: Tick(tick),
            producer,
            sequence,
        },
        Some(attacker),
        target,
        key(node),
        DamageChannel::Internal,
        50.0,
    )
    .expect("test hit is valid")
}

/// Two individually lethal hits on the same tick — the F29-A/AC01 fixture —
/// resolved by the real resolver, which emits exactly one `KillAwarded`.
fn lethal_resolution(victim: ActorId, attacker: ActorId) -> TickResolution {
    let mut resolver = DamageResolver::new(SESSION, RESOLVER_PRODUCER);
    resolver
        .register_actor(
            victim,
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the synthetic actor registers");
    // Fed out of id order on purpose: the declared ordering rule, not the
    // input order, decides the blow.
    let late = hit(9, 0, attacker, victim, "hull", RESOLVED_TICK);
    let early = hit(3, 0, attacker, victim, "hull", RESOLVED_TICK);
    resolver
        .resolve(Tick(RESOLVED_TICK), &[late, early])
        .expect("valid batch")
}

fn kill_awards(events: &[DamageEvent]) -> Vec<&DamageEvent> {
    events
        .iter()
        .filter(|event| matches!(event.kind, DamageEventKind::KillAwarded { .. }))
        .collect()
}

/// A hand-built kill award for `victim`, stamped `session` — the shape a
/// replayed, crossed or foreign batch would hand the consumer.
fn kill_award_event(session: SessionId, victim: ActorId, credited: Option<ActorId>) -> DamageEvent {
    DamageEvent {
        id: EventId {
            session,
            tick: Tick(RESOLVED_TICK),
            producer: RESOLVER_PRODUCER + 1,
            sequence: 1,
        },
        kind: DamageEventKind::KillAwarded {
            victim,
            credited,
            rule: AttributionRule::FirstLethalHit,
            blow: HitEventId {
                session,
                tick: Tick(RESOLVED_TICK),
                producer: RESOLVER_PRODUCER + 1,
                sequence: 0,
            },
        },
    }
}

/// Minimum scenario: the resolver's `KillAwarded` is delivered to the score
/// exactly once. The second delivery of the same batch — a replayed tick, a
/// retransmitted report — is absorbed, so neither a restart of the consumer
/// nor a repeated batch can pay the same kill twice.
#[test]
fn accept_f29_c_kill_one_kill_awards_exactly_once() {
    let (mut ledger, victim, attacker) = score_ledger();
    let resolution = lethal_resolution(victim, attacker);
    let awards = kill_awards(&resolution.events);
    assert_eq!(
        awards.len(),
        1,
        "two same-tick lethal hits produce exactly one award (AC01)"
    );

    let report = ledger.apply_kill_awards(&resolution.events);
    assert_eq!(report.refusals(), &[], "nothing here is refused");
    assert_eq!(report.awarded(), 1, "the kill scores once");
    assert_eq!(report.already_awarded(), 0);
    let value = ledger.kill_score().clone();
    assert_eq!(report.points(), value.points());
    assert_eq!(
        report.outcomes()[0],
        KillAward::Awarded {
            victim,
            credited: Some(attacker),
            tick: Tick(RESOLVED_TICK),
            points: value.points(),
        },
        "the declared attribution rule's credit reaches the ledger unchanged"
    );
    assert_eq!(ledger.score(), value.points());
    assert_eq!(ledger.scored_kills(), 1);
    assert_eq!(
        ledger.score_for(attacker),
        value.points(),
        "the score is keyed by the session-qualified attacker id"
    );
    assert_eq!(
        ledger.state(victim).expect("known").lifecycle,
        NetLifecycle::Destroyed,
        "scoring a kill also records the destruction it implies"
    );

    // The identical batch handed over again: absorbed, nothing moves.
    let again = ledger.apply_kill_awards(&resolution.events);
    assert_eq!(again.awarded(), 0, "a replayed batch must not score again");
    assert_eq!(again.already_awarded(), 1);
    assert_eq!(again.points(), 0);
    assert_eq!(ledger.score(), value.points());
    assert_eq!(ledger.scored_kills(), 1);
    assert_eq!(ledger.score_for(attacker), value.points());
}

/// Two same-tick lethal hits award one kill, and a second award for that same
/// victim on that same tick — a duplicate emission or a retransmission — is
/// absorbed rather than paid.
#[test]
fn accept_f29_c_kill_a_same_tick_double_kill_does_not_double_award() {
    let (mut ledger, victim, attacker) = score_ledger();
    let resolution = lethal_resolution(victim, attacker);
    let awards = kill_awards(&resolution.events);
    assert_eq!(awards.len(), 1, "one tick, one victim: one award");

    let first = ledger.apply_kill_awards(&resolution.events);
    assert_eq!(first.awarded(), 1);
    let points = ledger.kill_score().points();
    assert_eq!(ledger.score(), points);

    // The same victim, the same tick, another producer's sequence: a second
    // `KillAwarded` nobody should ever be able to spend twice.
    let duplicate = kill_award_event(SESSION, victim, Some(attacker));
    assert_eq!(duplicate.id.tick, Tick(RESOLVED_TICK));
    let second = ledger.apply_kill_awards(&[duplicate]);
    assert_eq!(second.awarded(), 0, "the duplicate must not score");
    assert_eq!(second.already_awarded(), 1);
    assert_eq!(second.points(), 0);
    assert_eq!(
        second.outcomes()[0],
        KillAward::AlreadyAwarded {
            victim,
            first: Tick(RESOLVED_TICK)
        },
        "the absorbed report names the tick the score was awarded on"
    );
    assert_eq!(ledger.score(), points, "the score does not move");
    assert_eq!(ledger.scored_kills(), 1);

    // And the original batch still cannot score a third time.
    let third = ledger.apply_kill_awards(&resolution.events);
    assert_eq!(third.awarded(), 0);
    assert_eq!(third.already_awarded(), 1);
    assert_eq!(ledger.score(), points);
}

/// An award from a previous session generation is refused, not applied: the
/// event's own session stamp and the actor ids it names are both checked, and
/// the refusal leaves the score and the victim's record untouched.
#[test]
fn accept_f29_c_kill_a_foreign_session_generation_is_refused() {
    let (mut ledger, victim, attacker) = score_ledger();

    // (a) an award stamped by another generation's event id.
    let crossed = kill_award_event(FOREIGN, victim, Some(attacker));
    let report = ledger.apply_kill_awards(&[crossed]);
    assert_eq!(report.awarded(), 0, "a foreign award must not score");
    assert_eq!(
        report.refusals().len(),
        1,
        "the refusal is reported by name"
    );
    let (refused, fault) = report.refusals()[0];
    assert_eq!(refused, victim);
    assert!(
        matches!(
            fault,
            NetStateError::ForeignSession { expected, found }
                if expected == SESSION && found == FOREIGN
        ),
        "expected a foreign-session refusal, got {fault:?}"
    );
    assert_eq!(ledger.score(), 0, "a refused award changes nothing");
    assert_eq!(ledger.scored_kills(), 0);
    assert!(
        !ledger.destruction_recorded(victim),
        "a refused award does not even record the destruction"
    );

    // (b) an award naming an actor of another generation, however it was
    // stamped — the id itself carries the session.
    let foreign_actor = ActorId {
        session: FOREIGN,
        serial: victim.serial,
    };
    assert_eq!(
        ledger.record_kill_award(foreign_actor, None, Tick(RESOLVED_TICK)),
        Err(NetStateError::ForeignSession {
            expected: SESSION,
            found: FOREIGN,
        })
    );
    assert_eq!(
        ledger.record_kill_award(victim, Some(foreign_actor), Tick(RESOLVED_TICK)),
        Err(NetStateError::ForeignSession {
            expected: SESSION,
            found: FOREIGN,
        }),
        "a foreign *credited* attacker is refused too: its id can never hold this session's score"
    );
    assert_eq!(ledger.score(), 0);

    // The ledger is not poisoned: this generation's own award still scores.
    let own = ledger.apply_kill_awards(&resolution_events(victim, attacker));
    assert_eq!(own.awarded(), 1);
    assert_eq!(ledger.score(), ledger.kill_score().points());
}

/// The same fixture as above, factored out so the refusal test can finish by
/// proving the ledger still works.
fn resolution_events(victim: ActorId, attacker: ActorId) -> Vec<DamageEvent> {
    lethal_resolution(victim, attacker).events
}

/// A pilot who bails out did not die: the bailout transition is not a score
/// event at all, and a kill award arriving for an actor that already ended
/// without one is refused by name. Nothing scores, ever.
#[test]
fn accept_f29_c_kill_a_bailout_awards_nothing() {
    let (mut ledger, victim, attacker) = score_ledger();
    let generation = ledger.generation(victim).expect("the ledger knows it");
    ledger
        .end_lifecycle(victim, generation, NetLifecycle::BailedOut)
        .expect("the pilot bails out");

    // The damage domain records a bailout as its own lifecycle transition,
    // never as a kill (F29 non-negotiable behavior 3). Such a batch carries
    // no award, and a consumer that scored any lifecycle would pay it here.
    let bailout = DamageEvent {
        id: EventId {
            session: SESSION,
            tick: Tick(RESOLVED_TICK),
            producer: RESOLVER_PRODUCER,
            sequence: 1,
        },
        kind: DamageEventKind::Lifecycle {
            actor: victim,
            kind: LifecycleKind::PilotBailout,
        },
    };
    let report = ledger.apply_kill_awards(&[bailout]);
    assert!(
        report.outcomes().is_empty(),
        "a bailout is not a kill award"
    );
    assert!(
        report.refusals().is_empty(),
        "it is not a refused kill either — it is not a score event at all"
    );
    assert_eq!(ledger.score(), 0);
    assert_eq!(ledger.scored_kills(), 0);

    // A `KillAwarded` that arrives anyway for the bailed-out victim is
    // refused by name, and the refusal is what the report carries.
    let late = kill_award_event(SESSION, victim, Some(attacker));
    let refused = ledger.apply_kill_awards(&[late]);
    assert_eq!(refused.awarded(), 0, "a bailout never becomes a kill");
    assert_eq!(refused.points(), 0);
    assert_eq!(refused.refusals().len(), 1, "the refusal is reported");
    let (subject, fault) = refused.refusals()[0];
    assert_eq!(subject, victim);
    assert!(
        matches!(
            fault,
            NetStateError::AlreadyTerminal {
                lifecycle: NetLifecycle::BailedOut,
                ..
            }
        ),
        "expected the already-ended refusal, got {fault:?}"
    );
    assert_eq!(ledger.score(), 0);
    assert_eq!(ledger.scored_kills(), 0);
    assert!(
        !ledger.destruction_recorded(victim),
        "a bailout is not a destruction"
    );
}

/// The value the ledger awards is provenance-carrying and caller-supplied:
/// the default states the design it came from, a measured value can replace
/// it, and a value that would subtract score is refused rather than applied.
#[test]
fn accept_f29_c_kill_value_carries_its_provenance() {
    let (mut ledger, victim, attacker) = score_ledger();

    let designed = ledger.kill_score().clone();
    assert_eq!(
        designed.claim().as_str(),
        KillScoreValue::DESIGNED_CLAIM,
        "the default value names the design it was authored under"
    );
    assert_eq!(designed.points(), KillScoreValue::DESIGNED_POINTS);
    assert_eq!(
        KillScoreValue::try_new(-1, "f29-c3.bad"),
        Err(KillScoreError::NegativePoints { points: -1 }),
        "a score that subtracts is refused at the value's own boundary"
    );

    let supplied = KillScoreValue::try_new(25, "f29-c3.supplied-kill-value")
        .expect("a non-negative value under a well-formed claim id");
    let previous = ledger.set_kill_score(supplied.clone());
    assert_eq!(previous, designed, "the setter hands the old value back");
    assert_eq!(ledger.kill_score(), &supplied);

    let report = ledger.apply_kill_awards(&resolution_events(victim, attacker));
    assert_eq!(report.awarded(), 1);
    assert_eq!(
        report.points(),
        25,
        "the awarded points are the ledger's own provenance-carrying value"
    );
    assert_eq!(ledger.score(), 25);
    assert_eq!(ledger.score_for(attacker), 25);
}
