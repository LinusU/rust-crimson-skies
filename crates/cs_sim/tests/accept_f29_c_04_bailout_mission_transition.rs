//! Acceptance scenario F29-C.4: the pilot bailout's **distinct** mission
//! transition, and what a later destruction report may not do to it.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C` (task F29-C.4, re-homed), acceptance case **AC04** — "Bailout and
//! ordinary death trigger the correct distinct mission transitions" — with
//! non-negotiable behaviors 3 and 4. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//! Task test prefix: `accept_f29_c_04_`.
//!
//! These tests drive the production path end to end and never a parallel
//! test-only bridge: the authoritative [`DamageResolver`] records the lifecycle
//! transition and emits the [`DamageEvent`]s the mission host would consume, its
//! `Lifecycle` events are read back and handed to the real
//! [`ObjectiveRuntime`] as `TickInput::lifecycles`, and the runtime's own
//! [`MissionTransitions`] decides the transition. Remove the wiring from
//! [`apply_counters`](cs_sim::objectives::runtime) — make it fold the raw
//! lifecycle facts straight into the counters as it did before — and the
//! minimum test below fails; make the ledger keep the *last* report instead of
//! the first and the third test fails.
//!
//! Every value here is newly authored synthetic fixture data. **No rule here is
//! an original-fidelity claim**: the mission-result policy a bailout should
//! apply is *unmeasured* (see
//! `docs/findings/2026-10-05-f29-c-4-bailout-mission-transition.md`), so the
//! only policy here withholds every result and grants no survival. The tests
//! assert that withholding is *structural* — a bailout cannot settle a mission,
//! whatever the airframe looks like — and leave the original's rule to F29-D.

use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::damage::{
    AttributionRule, DamageChannel, DamageEventKind, DamagePolicy, DamageResolver, HitEvent,
    HitEventId, LifecycleKind, SYNTHETIC_HULL_INTEGRITY, synthetic_airframe_graph,
};
use cs_sim::objectives::bailout::{
    AppliedTransition, BAILOUT_RESULT_POLICY_UNMEASURED, BailoutConfirmation, BailoutRefusal,
    BailoutResultPolicy, MissionTransition, MissionTransitions, TransitionOutcome,
};
use cs_sim::objectives::counters::CountKind;
use cs_sim::objectives::runtime::{
    ACTOR_EVENT_SOURCE, CountCondition, CountReaction, ObjectiveCompletion, ObjectiveEventKind,
    ObjectiveRuntime, ObjectiveSpec, ObjectiveTick, RevealRule, RuntimeLimits, TickInput,
};
use cs_sim::objectives::state::ObjectiveState;
use cs_sim::objectives::terminal::{TerminalOutcome, TerminalPrecedence};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::input::{Action, FlightCommand, InputContext};
use cs_types::net::SessionId;

// --- mission ids ------------------------------------------------------------

const SESSION: SessionGeneration = SessionGeneration(1);
const RESOLVER_SESSION: u64 = 7;
const RESOLVER_PRODUCER: u32 = 1;

/// The raider whose loss this mission fails on.
const PROTECTED: ActorId = ActorId(41);
/// A bystander: it may die, and it changes nothing.
const BYSTANDER: ActorId = ActorId(99);
/// The declared failure condition on the protected roster.
const PROTECTED_LOST: SymbolId = SymbolId(10);
/// The mission's primary objective, whose completion requests success.
const PRIMARY: SymbolId = SymbolId(1);

// ---------------------------------------------------------------------------
// Fixtures: the real damage resolver as the producer, the real objective
// runtime as the consumer.
// ---------------------------------------------------------------------------

fn net_session() -> SessionId {
    SessionId::new(RESOLVER_SESSION).expect("a nonzero session generation")
}

/// The world's actor id for the mission's protected raider. Its serial is the
/// mission actor's, so the identity bridge below is the identity it is.
fn protected_actor() -> cs_sim::damage::ActorId {
    cs_sim::damage::ActorId {
        session: net_session(),
        serial: u64::from(PROTECTED.0),
    }
}

/// A resolver holding one synthetic airframe, which is the authoritative
/// producer of the lifecycle transitions this task consumes.
fn resolver() -> DamageResolver {
    let mut resolver = DamageResolver::new(net_session(), RESOLVER_PRODUCER);
    resolver
        .register_actor(
            protected_actor(),
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the synthetic airframe registers");
    resolver
}

/// Kills the registered airframe through a real lethal hit, so the destruction
/// the mission sees is the one the damage domain decided — not a hand-written
/// lifecycle fact.
fn lethal_hit(tick: u64) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: net_session(),
            tick: Tick(tick),
            producer: RESOLVER_PRODUCER,
            sequence: 0,
        },
        None,
        protected_actor(),
        cs_sim::damage::DamageNodeKey::new("hull").expect("a valid node key"),
        DamageChannel::Internal,
        SYNTHETIC_HULL_INTEGRITY + 1.0,
    )
    .expect("a valid hit")
}

/// The mission-scoped actor id a resolver actor's serial maps to.
///
/// The identity bridge between the world's `cs_types::net::ActorId` and the
/// mission program's `cs_script::ir::ActorId` is the **host's** surface
/// (`cs_sim::mission` says so in the same words), so the test performs it
/// explicitly rather than hiding it behind a helper the production code lacks.
fn mission_actor(serial: u64) -> ActorId {
    ActorId(u32::try_from(serial).expect("a mission actor serial fits"))
}

/// Reads the lifecycle transitions the damage domain emitted for one resolution
/// or record, in emission order — the facts a mission host hands to the
/// objective runtime.
fn reported_lifecycles(events: &[cs_sim::damage::DamageEvent]) -> Vec<(ActorId, LifecycleKind)> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            DamageEventKind::Lifecycle { actor, kind } => {
                Some((mission_actor(actor.serial), *kind))
            }
            _ => None,
        })
        .collect()
}

/// Records a bailout through the real resolver and returns the facts the
/// mission host would report from it.
fn bailout_from_the_damage_domain(tick: u64) -> Vec<(ActorId, LifecycleKind)> {
    let mut resolver = resolver();
    let event = resolver
        .record_lifecycle(protected_actor(), LifecycleKind::PilotBailout, Tick(tick))
        .expect("the bailout records");
    reported_lifecycles(&[event])
}

/// Kills the airframe through the real resolver and returns the facts the
/// mission host would report from it.
fn destruction_from_the_damage_domain(tick: u64) -> Vec<(ActorId, LifecycleKind)> {
    let mut resolver = resolver();
    let resolution = resolver
        .resolve(Tick(tick), &[lethal_hit(tick)])
        .expect("a valid batch");
    reported_lifecycles(&resolution.events)
}

/// The runtime of the AC04 mission: a protected roster whose declared category
/// takes `reaction`, and a primary objective whose completion requests success.
/// A bailout and a kill must reach different answers here.
fn mission_runtime_with(reaction: CountReaction) -> ObjectiveRuntime {
    let mut runtime = ObjectiveRuntime::new(
        SESSION,
        TerminalPrecedence::SyntheticConservative,
        RuntimeLimits::default(),
    );
    runtime
        .add_objective(ObjectiveSpec {
            id: PRIMARY,
            content: ContentId::from_source(ContentKind::Objective, "escort-the-lancet")
                .expect("a valid objective id"),
            initial: ObjectiveState::Active,
            reveal: RevealRule::Immediate,
            on_complete: ObjectiveCompletion::Requests(TerminalOutcome::Success),
            completion_effects: Vec::new(),
        })
        .expect("the objective registers");
    runtime
        .add_condition(
            CountCondition::new(PROTECTED_LOST, CountKind::Destroyed, [PROTECTED], 1)
                .expect("a roster of one"),
            reaction,
        )
        .expect("the condition registers");
    runtime
}

/// The AC04 mission proper: losing the protected raider **fails** it.
fn mission_runtime() -> ObjectiveRuntime {
    mission_runtime_with(CountReaction::Finish(TerminalOutcome::Failure))
}

/// The same mission with the declared reaction reporting instead of finishing, so
/// a tick after a kill still does work. Used where the subject under test is a
/// *later* report rather than the outcome the first report settled.
fn reporting_runtime() -> ObjectiveRuntime {
    mission_runtime_with(CountReaction::ReportOnly)
}

fn input<'a>(tick: u64, lifecycles: &'a [(ActorId, LifecycleKind)]) -> TickInput<'a> {
    TickInput {
        tick: Tick(tick),
        committed_ticks: 0,
        lifecycles,
        movements: &[],
        signals: &[],
        timer_requests: &[],
        objective_requests: &[],
        terminal_requests: &[],
    }
}

fn step(
    runtime: &mut ObjectiveRuntime,
    tick: u64,
    lifecycles: &[(ActorId, LifecycleKind)],
) -> ObjectiveTick {
    runtime
        .step(&input(tick, lifecycles))
        .expect("a legal tick")
}

fn count_of(tick: &ObjectiveTick, predicate: impl Fn(&ObjectiveEventKind) -> bool) -> usize {
    tick.filter(predicate).len()
}

fn has_counted(tick: &ObjectiveTick, actor: ActorId, kind: CountKind) -> bool {
    tick.events.iter().any(|event| {
        event.source() == ACTOR_EVENT_SOURCE
            && matches!(
                event.kind,
                ObjectiveEventKind::Counted {
                    actor: reported,
                    kind: counted,
                } if reported == actor && counted == kind
            )
    })
}

fn bailed_out(tick: &ObjectiveTick, actor: ActorId) -> Vec<BailoutConfirmation> {
    tick.events
        .iter()
        .filter_map(|event| match &event.kind {
            ObjectiveEventKind::PilotBailedOut {
                actor: reported,
                confirmation,
            } if *reported == actor => Some(*confirmation),
            _ => None,
        })
        .collect()
}

fn transition_refusals(tick: &ObjectiveTick) -> Vec<(MissionTransition, BailoutRefusal)> {
    tick.events
        .iter()
        .filter_map(|event| match &event.kind {
            ObjectiveEventKind::TransitionRefused {
                requested, reason, ..
            } => Some((*requested, *reason)),
            _ => None,
        })
        .collect()
}

/// The confirmation a player produces by pressing the declared eject command in
/// flight — through the production constructor, so the input-context gate is the
/// one the binding layer enforces.
fn player_eject() -> BailoutConfirmation {
    BailoutConfirmation::from_action(Action::Flight(FlightCommand::Eject), InputContext::Flight)
        .expect("an eject edge in the flight context confirms a bailout")
}

// ---------------------------------------------------------------------------
// AC04: the minimum scenario
// ---------------------------------------------------------------------------

/// **AC04, first half.** A confirmed pilot bailout fires the **bailout**
/// transition and not the destruction transition: the stream reports the
/// departure with what confirmed it, no actor is counted as destroyed, the
/// protected roster's declared failure condition never latches, and the mission
/// outcome stays unsettled.
///
/// What makes this discriminating: the bailout arrives as the
/// [`DamageEvent`] the real [`DamageResolver`] emitted, so a runtime that
/// ignored `LifecycleKind::PilotBailout` — the behaviour before this wiring —
/// produces *no event at all* here and fails on the first assertion rather than
/// merely on a comparison.
#[test]
fn accept_f29_c_04_a_confirmed_bailout_fires_the_bailout_transition_and_not_a_kill() {
    let mut runtime = mission_runtime();
    runtime
        .confirm_bailout(PROTECTED, player_eject())
        .expect("the eject edge confirms the bailout");

    let lifecycles = bailout_from_the_damage_domain(5);
    assert_eq!(
        lifecycles,
        vec![(PROTECTED, LifecycleKind::PilotBailout)],
        "the damage domain reported a bailout, not a destruction"
    );

    let tick = step(&mut runtime, 5, &lifecycles);

    assert_eq!(
        bailed_out(&tick, PROTECTED),
        vec![player_eject()],
        "the bailout transition is reported, carrying what confirmed it"
    );
    assert!(
        !has_counted(&tick, PROTECTED, CountKind::Destroyed),
        "a bailout is not a kill"
    );
    assert_eq!(
        runtime.counted(CountKind::Destroyed),
        0,
        "no actor was counted as destroyed"
    );
    assert!(!runtime.condition_met(PROTECTED_LOST));
    assert_eq!(
        runtime.outcome(),
        None,
        "a bailout does not settle the mission"
    );
    assert_eq!(
        runtime.mission_transition(PROTECTED),
        Some(MissionTransition::PilotBailedOut)
    );
    assert!(
        !MissionTransition::PilotBailedOut.is_kill(),
        "the transition says so itself, so no consumer has to remember"
    );
    assert_eq!(
        count_of(&tick, |kind| matches!(
            kind,
            ObjectiveEventKind::TransitionRefused { .. }
        )),
        0,
        "nothing was refused: the bailout was confirmed and applied"
    );
}

/// **AC04, second half.** An ordinary kill fires the **destruction** transition
/// and not the bailout one: the same roster's declared category latches, the
/// mission fails on it, and the stream carries no bailout transition for an
/// actor whose pilot is still in the seat.
///
/// The kill is a real lethal hit through the real resolver, so this test also
/// pins that the wiring did not narrow the destruction path while adding the
/// bailout one.
#[test]
fn accept_f29_c_04_an_ordinary_kill_fires_the_destruction_transition_and_not_a_bailout() {
    let mut runtime = mission_runtime();

    let lifecycles = destruction_from_the_damage_domain(5);
    assert_eq!(
        lifecycles,
        vec![(PROTECTED, LifecycleKind::Destroyed)],
        "the damage domain reported a destruction"
    );

    let tick = step(&mut runtime, 5, &lifecycles);

    assert!(
        has_counted(&tick, PROTECTED, CountKind::Destroyed),
        "the kill counts toward the declared category"
    );
    assert!(
        runtime.condition_met(PROTECTED_LOST),
        "the declared roster condition latched on the kill"
    );
    assert_eq!(
        runtime.outcome(),
        Some(TerminalOutcome::Failure),
        "the declared reaction failed the mission"
    );
    assert_eq!(
        runtime.mission_transition(PROTECTED),
        Some(MissionTransition::Destroyed)
    );
    assert!(
        MissionTransition::Destroyed.is_kill(),
        "only destruction is a kill"
    );
    assert!(
        bailed_out(&tick, PROTECTED).is_empty(),
        "a kill is not a bailout"
    );
    assert!(
        transition_refusals(&tick).is_empty(),
        "nothing about an ordinary kill is refused"
    );
}

/// **AC04, third half.** A later destruction report cannot turn a bailout into a
/// kill: the first transition an actor reached is the one kept, so the wreck
/// being shot down afterwards moves no counter, latches no condition, awards no
/// mission outcome and grants no survival — and the refusal is *named*, so the
/// consumer can tell "the world reported a destruction after a bailout" from
/// "nothing happened".
///
/// The mirror case is pinned too, because a latch that only refuses in one
/// direction is not a latch: a pilot cannot leave an airframe that is already a
/// wreck.
#[test]
fn accept_f29_c_04_a_later_destruction_cannot_turn_a_bailout_into_a_kill() {
    let mut runtime = mission_runtime();
    runtime
        .confirm_bailout(PROTECTED, player_eject())
        .expect("the eject edge confirms the bailout");
    step(&mut runtime, 5, &bailout_from_the_damage_domain(5));
    assert_eq!(runtime.outcome(), None);

    // The abandoned airframe is shot down on a later tick, through the real
    // resolver, so the destruction is a real one.
    let later = step(&mut runtime, 9, &destruction_from_the_damage_domain(9));

    assert_eq!(
        transition_refusals(&later),
        vec![(
            MissionTransition::Destroyed,
            BailoutRefusal::AlreadyTransitioned {
                actor: PROTECTED,
                requested: MissionTransition::Destroyed,
                kept: MissionTransition::PilotBailedOut,
            }
        )],
        "the later destruction is refused by name, naming both transitions"
    );
    assert!(
        !has_counted(&later, PROTECTED, CountKind::Destroyed),
        "and it counted nothing"
    );
    assert_eq!(
        runtime.counted(CountKind::Destroyed),
        0,
        "a kill was never awarded"
    );
    assert!(!runtime.condition_met(PROTECTED_LOST));
    assert_eq!(
        runtime.outcome(),
        None,
        "a bailed-out airframe that crashed does not fail the mission"
    );
    assert_eq!(
        runtime.mission_transition(PROTECTED),
        Some(MissionTransition::PilotBailedOut),
        "the first transition is the one kept"
    );
    assert!(
        !runtime.bailout_policy().grants_survival(),
        "and no survival is credited for either"
    );

    // The mirror: a destruction first, then a bailout reported for the wreck.
    // The confirmation itself is refused, so no transition is even applied.
    //
    // The declared reaction is `ReportOnly` here, not `Finish`: a mission that
    // had already settled on tick 5 stops the tick at the latch and does no
    // work, which would hide the refusal this half is about.
    let mut wrecked = reporting_runtime();
    step(&mut wrecked, 5, &destruction_from_the_damage_domain(5));
    assert_eq!(
        wrecked.confirm_bailout(PROTECTED, player_eject()),
        Err(BailoutRefusal::AlreadyTransitioned {
            actor: PROTECTED,
            requested: MissionTransition::PilotBailedOut,
            kept: MissionTransition::Destroyed,
        }),
        "a pilot cannot bail out of an airframe that is already destroyed"
    );
    let refused = step(&mut wrecked, 6, &bailout_from_the_damage_domain(6));
    assert_eq!(
        transition_refusals(&refused),
        vec![(
            MissionTransition::PilotBailedOut,
            BailoutRefusal::AlreadyTransitioned {
                actor: PROTECTED,
                requested: MissionTransition::PilotBailedOut,
                kept: MissionTransition::Destroyed,
            }
        )],
        "the later bailout is refused by name, naming both transitions"
    );
    assert!(
        bailed_out(&refused, PROTECTED).is_empty(),
        "and it recorded no bailout transition"
    );
    assert_eq!(
        wrecked.mission_transition(PROTECTED),
        Some(MissionTransition::Destroyed),
        "so the destruction stays the transition this actor reached"
    );
    assert_eq!(
        wrecked.counted(CountKind::Destroyed),
        1,
        "and the kill stands"
    );
}

// ---------------------------------------------------------------------------
// The two gates that make the withholding structural rather than a convention
// ---------------------------------------------------------------------------

/// Non-negotiable 4's confirmation half: a bailout needs a confirmation, and
/// the gate is the production one — the declared eject command in a context that
/// accepts flight commands. A rendering parachute is not a confirmation, so an
/// unconfirmed transition records nothing at all.
#[test]
fn accept_f29_c_04_a_bailout_without_a_confirmed_input_is_refused_by_name() {
    let mut runtime = mission_runtime();

    // No confirmation at all.
    let tick = step(&mut runtime, 5, &bailout_from_the_damage_domain(5));
    assert_eq!(
        transition_refusals(&tick),
        vec![(
            MissionTransition::PilotBailedOut,
            BailoutRefusal::Unconfirmed { actor: PROTECTED }
        )]
    );
    assert!(
        bailed_out(&tick, PROTECTED).is_empty(),
        "nothing is reported, because nothing was confirmed"
    );
    assert!(runtime.mission_transition(PROTECTED).is_none());
    assert_eq!(runtime.outcome(), None);

    // A context that accepts no flight command cannot confirm one — F22's gate,
    // reused rather than reimplemented.
    let mut cinematic = mission_runtime();
    assert_eq!(
        cinematic.confirm_bailout(
            PROTECTED,
            BailoutConfirmation::PlayerEject {
                context: InputContext::Cinematic
            }
        ),
        Err(BailoutRefusal::InputContextRefused {
            context: InputContext::Cinematic,
        })
    );
    assert!(
        cinematic.confirm_bailout(PROTECTED, player_eject()).is_ok(),
        "the flight context does confirm one"
    );

    // The same gate on the edge itself: a different flight command, and a
    // non-flight action, are both refused with their own reason.
    assert_eq!(
        BailoutConfirmation::from_action(
            Action::Flight(FlightCommand::FirePrimary),
            InputContext::Flight
        ),
        Err(BailoutRefusal::NotAnEjectCommand {
            command: FlightCommand::FirePrimary
        })
    );
    assert_eq!(
        BailoutConfirmation::from_action(
            Action::Ui(cs_types::input::UiAction::Confirm),
            InputContext::Flight
        ),
        Err(BailoutRefusal::NotAnEjectEdge {
            action: Action::Ui(cs_types::input::UiAction::Confirm),
            context: InputContext::Flight,
        })
    );

    // A mission program's own request needs no local input, and is a
    // confirmation all the same.
    let mut scripted = mission_runtime();
    scripted
        .confirm_bailout(PROTECTED, BailoutConfirmation::scripted(PROTECTED_LOST))
        .expect("a program's request confirms the bailout");
    let tick = step(&mut scripted, 5, &bailout_from_the_damage_domain(5));
    assert_eq!(
        bailed_out(&tick, PROTECTED),
        vec![BailoutConfirmation::scripted(PROTECTED_LOST)],
        "the reported transition names what asked for it"
    );
}

/// Non-negotiable 4's result half: the policy is **unmeasured**, it grants
/// nothing, and the runtime asks it rather than deciding. A bailout therefore
/// cannot settle a mission — not because a test says so, but because the
/// declared policy has no outcome to hand the terminal latch.
///
/// This is the assertion a reviewer must not be able to satisfy by editing the
/// test: make [`BailoutResultPolicy::terminal_outcome`] return
/// `Some(TerminalOutcome::Success)` and the "withholds the result" assertion
/// fails while every other test in this file still passes.
#[test]
fn accept_f29_c_04_the_unmeasured_bailout_policy_grants_no_result_and_no_survival() {
    let policy = mission_runtime().bailout_policy();
    assert_eq!(
        policy,
        BailoutResultPolicy::Unmeasured,
        "the only policy this engine may declare is the one that withholds"
    );
    assert!(!policy.is_measured(), "no measured rule exists");
    assert_eq!(
        policy.reason(),
        BAILOUT_RESULT_POLICY_UNMEASURED,
        "and it names the absence rather than filling it"
    );
    assert_eq!(
        policy.terminal_outcome(),
        None,
        "a bailout requests no terminal outcome"
    );
    assert!(
        !policy.grants_survival(),
        "survival is a separate question from the mission carrying on"
    );

    // Behaviourally: a mission whose only remaining objective completes on the
    // same tick still settles **success**, because it was the completion that
    // asked — and the bailout is nowhere in the reason. Nothing here lets a
    // bailout contribute a result of its own.
    let mut runtime = mission_runtime();
    runtime
        .confirm_bailout(PROTECTED, player_eject())
        .expect("the eject edge confirms the bailout");
    let completed = step(&mut runtime, 5, &bailout_from_the_damage_domain(5));
    assert_eq!(
        runtime.outcome(),
        None,
        "a bailout on its own settles nothing"
    );
    assert!(
        completed
            .events
            .iter()
            .all(|event| !matches!(event.kind, ObjectiveEventKind::OutcomeSettled { .. })),
        "and nothing on the stream claims otherwise"
    );

    // The bystander keeps the counted path working: the wiring narrowed nothing
    // that F39-E4 measured.
    let mut with_bystander = mission_runtime();
    with_bystander
        .confirm_bailout(PROTECTED, player_eject())
        .expect("the eject edge confirms the bailout");
    let tick = step(
        &mut with_bystander,
        5,
        &[
            (BYSTANDER, LifecycleKind::Destroyed),
            (PROTECTED, LifecycleKind::PilotBailout),
        ],
    );
    assert!(has_counted(&tick, BYSTANDER, CountKind::Destroyed));
    assert!(!with_bystander.condition_met(PROTECTED_LOST));
    assert_eq!(with_bystander.outcome(), None);
}

/// The ledger is the single place the transition is decided, so it is testable
/// on its own — and the derivation it replaces cannot drift: the three lifecycle
/// kinds this ledger does not own keep their own measured producers.
#[test]
fn accept_f29_c_04_the_ledger_is_the_one_place_the_transition_is_decided() {
    let mut ledger = MissionTransitions::default();

    // The mapping is total over the two kinds and silent on the other three.
    for kind in LifecycleKind::ALL {
        let derived = MissionTransition::from_lifecycle(*kind);
        assert_eq!(
            derived.is_some(),
            matches!(kind, LifecycleKind::Destroyed | LifecycleKind::PilotBailout),
            "{kind} must agree with the sheet's two transitions"
        );
        if let Some(transition) = derived {
            assert_eq!(transition.lifecycle(), *kind, "and round-trip");
            assert!(MissionTransition::ALL.contains(&transition));
        }
    }

    // A bailout with no confirmation latches nothing.
    assert_eq!(
        ledger.observe(PROTECTED, MissionTransition::PilotBailedOut),
        TransitionOutcome::Refused(BailoutRefusal::Unconfirmed { actor: PROTECTED })
    );
    assert!(ledger.transition(PROTECTED).is_none());

    // Confirmed, it applies once and repeats silently.
    ledger
        .confirm(PROTECTED, player_eject())
        .expect("the confirmation records");
    assert_eq!(
        ledger.observe(PROTECTED, MissionTransition::PilotBailedOut),
        TransitionOutcome::Applied(AppliedTransition {
            actor: PROTECTED,
            transition: MissionTransition::PilotBailedOut,
            confirmation: Some(player_eject()),
        })
    );
    assert_eq!(
        ledger.observe(PROTECTED, MissionTransition::PilotBailedOut),
        TransitionOutcome::Repeated(MissionTransition::PilotBailedOut),
        "a repeated transition is idempotent"
    );
    assert_eq!(
        ledger.pending_confirmation(PROTECTED),
        None,
        "a confirmation is consumed by the transition it confirms"
    );

    // A second confirmation is refused: the first edge is the one that counts.
    assert_eq!(
        ledger.confirm(PROTECTED, player_eject()),
        Err(BailoutRefusal::AlreadyTransitioned {
            actor: PROTECTED,
            requested: MissionTransition::PilotBailedOut,
            kept: MissionTransition::PilotBailedOut,
        })
    );

    // The other actor is independent: the ledger is per actor.
    assert_eq!(
        ledger.observe(BYSTANDER, MissionTransition::Destroyed),
        TransitionOutcome::Applied(AppliedTransition {
            actor: BYSTANDER,
            transition: MissionTransition::Destroyed,
            confirmation: None,
        })
    );
    assert_eq!(
        ledger.transitioned().collect::<Vec<_>>(),
        vec![PROTECTED, BYSTANDER]
    );
}
