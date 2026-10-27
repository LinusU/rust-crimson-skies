//! Acceptance scenario F32-FIRE-DISCIPLINE: the reported `fire_discipline_ticks`
//! has a session owner, so the probe's firing-tick count is a measurement and
//! not a control.
//!
//! Task: #571 (`F32-FIRE-DISCIPLINE`). Spec:
//! `specs/F32-ai-combat-formations-aces-and-difficulty.md`. Task test prefix:
//! `accept_f32_fd_`.
//!
//! Every test drives production code only: `cs_sim::ai::combat`'s
//! [`CombatRuntime::step`], the session-owned [`FireDiscipline`] it advances,
//! the [`FiringSolution::hold_until`] gate that reads it into every firing
//! solution, and the [`DifficultyProbeReport::firing_ticks`]-bearing
//! aggregates. An implementation that kept reporting the cadence without
//! applying it fails the hold, the readiness boundary, the per-actor
//! isolation and the probe-discrimination tests below; one that applied it
//! to the wrong actor, the wrong session or a step that fired nothing fails
//! the others.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_sim::ai::combat::{
    ArsenalSnapshot, CandidateView, CombatRole, CombatRuntime, CombatStep, CombatantRequest,
    DifficultyProbeSpec, DifficultyRoster, DifficultyTier, FireHoldReason, MountFireState,
    ProbeDifference, ProbeProvenance, RoleAssignment, SYNTHETIC_SESSION, SkillKnobs,
    synthetic_actor, synthetic_arsenal, synthetic_candidate, synthetic_combat_runtime,
    synthetic_escort_profile, synthetic_recovery_policies,
};
use cs_sim::damage::ActorId;
use cs_sim::targeting::Allegiance;
use cs_types::Tick;
use cs_types::space::WorldPosition;

/// One fighter decision against the candidates the caller supplies, at a
/// tick, on the runtime's own session.
fn fighter_step(
    runtime: &mut CombatRuntime,
    serial: u64,
    now: u64,
    candidates: &[CandidateView],
    arsenal: &ArsenalSnapshot,
) -> CombatStep {
    let assignment = RoleAssignment::new(synthetic_actor(serial), CombatRole::FighterAttack);
    runtime
        .step(&CombatantRequest {
            observer: synthetic_actor(serial),
            now: Tick(now),
            observer_position: WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
            assignment: &assignment,
            formation: None,
            protected_alive: None,
            candidates,
            arsenal: Some(arsenal),
            ace: None,
            tier: DifficultyTier::Standard,
        })
        .expect("the fighter decides")
}

/// One escort decision at a tick: the same step on the escort's own
/// assignment, so two roles can be compared on one runtime.
fn escort_step(
    runtime: &mut CombatRuntime,
    serial: u64,
    charge: u64,
    now: u64,
    candidates: &[CandidateView],
    arsenal: &ArsenalSnapshot,
) -> CombatStep {
    let assignment = RoleAssignment::protecting(
        synthetic_actor(serial),
        CombatRole::Escort,
        synthetic_actor(charge),
    )
    .expect("an escort never protects itself");
    runtime
        .step(&CombatantRequest {
            observer: synthetic_actor(serial),
            now: Tick(now),
            observer_position: WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
            assignment: &assignment,
            formation: None,
            protected_alive: Some(true),
            candidates,
            arsenal: Some(arsenal),
            ace: None,
            tier: DifficultyTier::Standard,
        })
        .expect("the escort decides")
}

fn hostile(serial: u64) -> Vec<CandidateView> {
    vec![synthetic_candidate(
        serial,
        [300.0, 0.0, 0.0],
        Some(Allegiance::Hostile),
        true,
    )]
}

fn key(name: &str) -> cs_sim::damage::DamageNodeKey {
    cs_sim::damage::DamageNodeKey::new(name).expect("the fixture mount key is valid")
}

/// The Standard tier's fighter cadence the fixtures declare.
const FIGHTER_CADENCE: u64 = 24;
/// The Standard tier's escort cadence the fixtures declare.
const ESCORT_CADENCE: u64 = 30;

/// The minimum scenario: an actor's own cadence is real state, owned by the
/// session's runtime. A shot that fires advances the actor's next allowed
/// shot by the profile's `fire_discipline_ticks`; a step inside that window
/// holds the very same mounts that would otherwise fire, named by the
/// consulted state; and the boundary tick fires again.
#[test]
fn accept_f32_fd_the_session_holds_an_actor_to_its_declared_cadence() {
    let mut runtime = synthetic_combat_runtime();
    let hostile = hostile(40);
    let arsenal = synthetic_arsenal();
    let actor = synthetic_actor(1);

    assert_eq!(
        runtime.fire_discipline().ready_at(actor),
        Tick(0),
        "an actor that never fired waits on nothing"
    );

    // First shot: the mounts the arsenal clears, fire.
    let first = fighter_step(&mut runtime, 1, 100, &hostile, &arsenal);
    let firing = first
        .trace()
        .firing
        .as_ref()
        .expect("a target was selected");
    assert_eq!(
        firing.fire_ready_at,
        Some(Tick(0)),
        "the session state the solution consulted: the actor had never fired"
    );
    assert_eq!(
        firing.state(&key("gun_mount_1")),
        Some(MountFireState::Firing)
    );
    assert_eq!(
        firing.state(&key("gun_mount_2")),
        Some(MountFireState::Firing)
    );
    assert!(firing.is_firing());
    assert_eq!(
        runtime.fire_discipline().ready_at(actor),
        Tick(100 + FIGHTER_CADENCE),
        "a shot that fired advanced the actor's next allowed shot by its cadence"
    );

    // Inside the window: the same step would fire, and the session's cadence
    // is the refusal the mounts report — by name, with the wait remaining.
    let held = fighter_step(&mut runtime, 1, 110, &hostile, &arsenal);
    let firing = held.trace().firing.as_ref().expect("a target was selected");
    assert_eq!(
        firing.fire_ready_at,
        Some(Tick(100 + FIGHTER_CADENCE)),
        "the trace records *which* session state produced the verdicts"
    );
    assert_eq!(
        firing.state(&key("gun_mount_1")),
        Some(MountFireState::DisciplineHold {
            remaining_ticks: 14
        }),
        "a ready mount the actor's cadence holds is named, not silently dropped"
    );
    assert_eq!(
        firing.state(&key("gun_mount_2")),
        Some(MountFireState::DisciplineHold {
            remaining_ticks: 14
        })
    );
    assert_eq!(
        firing.state(&key("ordnance_mount_1")),
        Some(MountFireState::WrongKind),
        "the cadence rewrites only mounts that would fire; availability stays its own verdict"
    );
    assert!(!firing.is_firing());
    assert_eq!(
        firing.hold(),
        Some(FireHoldReason::DisciplineHold {
            remaining_ticks: 14
        }),
        "the hold names the actor's cadence as the binding refusal"
    );
    assert_eq!(
        runtime.fire_discipline().ready_at(actor),
        Tick(100 + FIGHTER_CADENCE),
        "a held shot pays no cadence — the actor is still waiting out the first one"
    );

    // The boundary tick: the cadence has expired, so the same step fires.
    let again = fighter_step(&mut runtime, 1, 100 + FIGHTER_CADENCE, &hostile, &arsenal);
    let firing = again
        .trace()
        .firing
        .as_ref()
        .expect("a target was selected");
    assert_eq!(
        firing.state(&key("gun_mount_1")),
        Some(MountFireState::Firing),
        "at ready_at the mounts the arsenal clears fire again"
    );
    assert!(firing.is_firing());
    assert_eq!(
        runtime.fire_discipline().ready_at(actor),
        Tick(100 + 2 * FIGHTER_CADENCE),
        "the second shot advances the cadence from its own tick"
    );
}

/// The cadence is per *actor* and per *session generation*: one actor's paid
/// discipline never blocks another's, and a different session's runtime owes
/// the first actor nothing.
#[test]
fn accept_f32_fd_fire_discipline_is_per_actor_and_per_session() {
    let mut runtime = synthetic_combat_runtime();
    let hostile = hostile(40);
    let arsenal = synthetic_arsenal();

    fighter_step(&mut runtime, 1, 100, &hostile, &arsenal);
    assert_eq!(
        runtime.fire_discipline().ready_at(synthetic_actor(1)),
        Tick(100 + FIGHTER_CADENCE)
    );

    // A second actor in the same session at the same tick is not held by the
    // first actor's paid cadence.
    let second_actor = fighter_step(&mut runtime, 2, 110, &hostile, &arsenal);
    assert!(
        second_actor
            .trace()
            .firing
            .as_ref()
            .expect("a target was selected")
            .is_firing(),
        "actor 2 fires on its own schedule, not actor 1's"
    );
    assert_eq!(
        runtime.fire_discipline().ready_at(synthetic_actor(2)),
        Tick(110 + FIGHTER_CADENCE),
        "actor 2's cadence is its own"
    );
    // And the first actor's state is untouched by the second's shot.
    assert_eq!(
        runtime.fire_discipline().ready_at(synthetic_actor(1)),
        Tick(100 + FIGHTER_CADENCE)
    );

    // A different session generation is a different state entirely: the same
    // actor serial fires immediately on a fresh runtime.
    let mut other_generation = synthetic_combat_runtime();
    let fresh = fighter_step(&mut other_generation, 1, 110, &hostile, &arsenal);
    assert!(
        fresh
            .trace()
            .firing
            .as_ref()
            .expect("a target was selected")
            .is_firing(),
        "another session owes the actor nothing"
    );
}

/// A step that fires nothing advances nothing: a shot the weapons refuse
/// (cooling mounts), a request the runtime refuses (a foreign observer) and
/// a role with no guns all leave the actor's next allowed shot where it was,
/// so an actor is never made to pay a cadence for a shot it never took.
#[test]
fn accept_f32_fd_a_step_that_fires_nothing_advances_no_cadence() {
    use cs_sim::ai::combat::{MountAvailability, MountKind};
    use cs_types::net::SessionId;

    let mut runtime = synthetic_combat_runtime();
    let hostile = hostile(40);
    let actor = synthetic_actor(1);

    // The weapons' own schedule refuses the shot: the mounts are all still
    // cooling, so nothing fires and the actor's cadence owes nothing.
    let cooling = ArsenalSnapshot::try_new(vec![MountAvailability {
        mount: key("gun_mount_1"),
        kind: MountKind::Gun,
        rounds: 120,
        disabled: false,
        cooldown_ticks: 50,
    }])
    .expect("the cooling arsenal is valid");
    let held = fighter_step(&mut runtime, 1, 100, &hostile, &cooling);
    assert!(!held.trace().firing.as_ref().unwrap().is_firing());
    assert_eq!(
        held.trace().firing.as_ref().unwrap().hold(),
        Some(FireHoldReason::NoUsableMount {
            disabled: 0,
            empty: 0,
            cooling: 1,
            wrong_kind: 0,
        }),
        "the mount's own schedule is the refusal here"
    );
    assert_eq!(
        runtime.fire_discipline().ready_at(actor),
        Tick(0),
        "a shot the mounts refused paid no cadence"
    );

    // The very next step, with ready mounts, fires immediately: the actor
    // never owed a wait for the shot it never took.
    let ready = fighter_step(&mut runtime, 1, 101, &hostile, &synthetic_arsenal());
    assert!(
        ready
            .trace()
            .firing
            .as_ref()
            .expect("a target was selected")
            .is_firing(),
        "the first real shot fires at once — the refused step cost nothing"
    );

    // A refused *request* records no shot either: a foreign observer is
    // refused by name before anything can fire.
    let foreign = ActorId {
        session: SessionId::new(SYNTHETIC_SESSION + 1).expect("nonzero"),
        serial: 1,
    };
    let err = runtime
        .step(&CombatantRequest {
            observer: foreign,
            now: Tick(200),
            observer_position: WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
            assignment: &RoleAssignment::new(foreign, CombatRole::FighterAttack),
            formation: None,
            protected_alive: None,
            candidates: &hostile,
            arsenal: Some(&synthetic_arsenal()),
            ace: None,
            tier: DifficultyTier::Standard,
        })
        .expect_err("a foreign observer is refused");
    assert!(matches!(
        err,
        cs_sim::ai::combat::CombatError::ForeignSession { .. }
    ));
    assert!(
        runtime.fire_discipline().is_ready(actor, Tick(200)),
        "a refused step wrote no discipline state"
    );
}

/// Two actors on one session keep the cadences their own profiles declare:
/// the Standard-tier fighter waits 24 ticks between shots, the escort 30, and
/// neither waits on the other. The reported knob is the applied knob.
#[test]
fn accept_f32_fd_two_actors_with_different_profiles_keep_different_cadences() {
    let mut runtime = synthetic_combat_runtime();
    let hostile = hostile(40);
    let arsenal = synthetic_arsenal();

    // Both fire once at the same tick — each on its own first shot.
    let fighter_first = fighter_step(&mut runtime, 1, 100, &hostile, &arsenal);
    let escort_first = escort_step(&mut runtime, 2, 50, 100, &hostile, &arsenal);
    assert!(fighter_first.trace().firing.as_ref().unwrap().is_firing());
    assert!(escort_first.trace().firing.as_ref().unwrap().is_firing());

    assert_eq!(
        fighter_first
            .trace()
            .firing
            .as_ref()
            .unwrap()
            .fire_discipline_ticks,
        FIGHTER_CADENCE,
        "the fighter's reported cadence"
    );
    assert_eq!(
        escort_first
            .trace()
            .firing
            .as_ref()
            .unwrap()
            .fire_discipline_ticks,
        ESCORT_CADENCE,
        "the escort's reported cadence"
    );

    // At tick 124 the fighter's 24-tick wait is over and it fires again;
    // the escort's 30-tick wait still holds it for 6 more.
    let fighter_again = fighter_step(&mut runtime, 1, 100 + FIGHTER_CADENCE, &hostile, &arsenal);
    assert!(
        fighter_again.trace().firing.as_ref().unwrap().is_firing(),
        "the fighter's shorter cadence lets it shoot first"
    );
    let escort_held = escort_step(
        &mut runtime,
        2,
        50,
        100 + FIGHTER_CADENCE,
        &hostile,
        &arsenal,
    );
    assert_eq!(
        escort_held.trace().firing.as_ref().unwrap().hold(),
        Some(FireHoldReason::DisciplineHold {
            remaining_ticks: ESCORT_CADENCE - FIGHTER_CADENCE
        }),
        "the escort is still waiting out its own, longer cadence"
    );
    let escort_again = escort_step(
        &mut runtime,
        2,
        50,
        100 + ESCORT_CADENCE,
        &hostile,
        &arsenal,
    );
    assert!(
        escort_again.trace().firing.as_ref().unwrap().is_firing(),
        "and it fires the tick its own cadence expires"
    );
}

/// A profile that declares no wait between shots never waits: `0` is a real
/// cadence, not a missing one, and the session lets every tick fire.
#[test]
fn accept_f32_fd_a_zero_cadence_never_holds() {
    use cs_sim::ai::combat::synthetic_fighter_profile;

    let zeroed = synthetic_fighter_profile()
        .with_knobs(SkillKnobs {
            fire_discipline_ticks: 0,
            ..synthetic_fighter_profile().knobs()
        })
        .expect("a zero cadence is a valid cadence");
    let roster = DifficultyRoster::new()
        .with_tier(DifficultyTier::Standard, &[zeroed])
        .expect("the single-tier roster is valid");
    let mut runtime = CombatRuntime::new(SYNTHETIC_SESSION, &[zeroed], Vec::new(), roster)
        .expect("the runtime is valid");
    let hostile = hostile(40);
    let arsenal = synthetic_arsenal();

    for now in [100, 100, 101] {
        let step = fighter_step(&mut runtime, 1, now, &hostile, &arsenal);
        assert!(
            step.trace().firing.as_ref().unwrap().is_firing(),
            "a zero cadence holds nothing, even at the same tick"
        );
    }
}

/// The aim error is a *declared reported-only* field: the firing solution
/// reports the profile's half-angle, [`FiringSolution::aim_error_applied`]
/// pins that it is not applied here, and the two declarations cannot drift
/// — a change that starts dispersing the shot in this module must flip both
/// and the finding that records it.
#[test]
fn accept_f32_fd_aim_error_is_a_declared_report_only_field() {
    let mut runtime = synthetic_combat_runtime();
    let hostile = hostile(40);
    let arsenal = synthetic_arsenal();

    let step = fighter_step(&mut runtime, 1, 100, &hostile, &arsenal);
    let firing = step.trace().firing.as_ref().expect("a target was selected");
    assert!(
        !firing.aim_error_applied(),
        "the firing solution declares the aim error reported-only"
    );
    assert_eq!(
        firing.aim_error_rad,
        cs_sim::ai::combat::synthetic_fighter_profile()
            .knobs()
            .aim_error_rad,
        "the reported value is still the profile's own knob"
    );
    assert!(
        firing.aim_error_rad > 0.0,
        "reported-only is a statement about who applies it, not that it is zero"
    );
}

/// The load-bearing discrimination `f32.d.limit.fire_discipline` said was
/// missing: two tiers that differ **only** in `fire_discipline_ticks` — same
/// reaction, same aim, same priority policy, same arsenal — now measure
/// different firing-tick counts, because the session applies the cadence the
/// tier carries.
///
/// The roster below walks the cadence down the four designed tiers and
/// nothing else. Every other outcome is *expected* to be identical — that is
/// what "differs only in the cadence" means — and the firing-tick
/// comparisons carry the difference.
#[test]
fn accept_f32_fd_the_probe_discriminates_tiers_that_differ_only_in_fire_discipline() {
    // The cadence each tier carries: identical behavior elsewhere, so the
    // firing-tick count is the only thing that may move.
    let cadence_at = |tier: DifficultyTier| -> u64 {
        match tier {
            DifficultyTier::Relaxed => 120,
            DifficultyTier::Standard => 60,
            DifficultyTier::Hard => 30,
            DifficultyTier::Elite => 15,
        }
    };
    let base = synthetic_escort_profile();
    let mut roster = DifficultyRoster::new();
    for tier in DifficultyTier::ALL {
        let profile = base
            .with_knobs(SkillKnobs {
                fire_discipline_ticks: cadence_at(*tier),
                ..base.knobs()
            })
            .expect("the cadence-only tier profile is valid");
        roster = roster
            .with_tier(*tier, &[profile])
            .expect("the cadence roster declares every tier");
    }
    let runtime = CombatRuntime::new(SYNTHETIC_SESSION, &[base], Vec::new(), roster)
        .expect("the cadence runtime is valid");
    let spec = DifficultyProbeSpec::try_new(
        SYNTHETIC_SESSION,
        synthetic_difficulty_probe_spec_ticks(),
        8,
        4_242,
        synthetic_recovery_policies(),
        ProbeProvenance::DesignedFixture,
    )
    .expect("the probe spec is valid");

    let report = runtime
        .probe_difficulties(&spec)
        .expect("a cadence-only roster still replays");

    // The knob is still a per-tier profile — the probe did not collapse.
    assert_eq!(
        report.distinct_profile_count(),
        DifficultyTier::ALL.len(),
        "four cadences, four effective profiles"
    );

    // The world, the weapons and the clock are what they always were.
    assert!(report.geometry_is_tier_invariant());
    assert!(report.arsenal_is_tier_invariant());
    assert!(report.clock_is_tier_invariant());

    // Everything the cadence cannot touch measures identical: the selection,
    // the reaction gate's bookkeeping and the protected-actor answers are a
    // function of knobs this roster never varies.
    for comparison in &report.comparisons {
        assert_eq!(
            comparison.engagements,
            ProbeDifference::Same,
            "the cadence never changes what is selected"
        );
        assert_eq!(
            comparison.protected_answers,
            ProbeDifference::Same,
            "the cadence never changes the reaction gate's answers"
        );
        assert_eq!(
            comparison.deferred_threats,
            ProbeDifference::Same,
            "the cadence never changes what is noticed"
        );
        // And the one thing it does touch is measured: a shorter cadence
        // fires on strictly more ticks.
        assert_eq!(
            comparison.firing_ticks,
            ProbeDifference::Higher,
            "{} -> {} fires on more ticks under a shorter cadence",
            comparison.lower.label(),
            comparison.higher.label()
        );
    }
    assert!(
        report.outcomes_differ(),
        "the firing-tick count is what tells the tiers apart"
    );

    // The aggregates themselves, tier by tier: the order is the cadence's
    // order, and each count is nonzero — the probe saw the shots it counted.
    let counts: Vec<u64> = report.tiers.iter().map(|tier| tier.firing_ticks).collect();
    for pair in counts.windows(2) {
        assert!(
            pair[0] < pair[1],
            "firing ticks increase with the tier's shorter cadence: {counts:?}"
        );
    }
    assert!(
        counts.iter().all(|count| *count > 0),
        "every tier fired something to count: {counts:?}"
    );
}

/// The probe spec's tick count, so the cadence probe above runs the same
/// mission length as the synthetic fixture without reaching into the
/// fixture's private constants.
fn synthetic_difficulty_probe_spec_ticks() -> u64 {
    600
}

/// The aim error's owner is the weapon system: `weapons::guns` owns the
/// shot's trajectory, declares that it samples no spread, and the AI's
/// reported-only declaration agrees with it. A change that starts dispersing
/// shots by the knob must flip both declarations together, and a real
/// firing solution still reports a non-zero knob that nothing widens.
#[test]
fn accept_f32_aim_error_is_re_declared_report_only_at_the_weapon_boundary() {
    const { assert!(!cs_sim::weapons::guns::SPREAD_SAMPLED) };
    assert_eq!(
        cs_sim::ai::combat::AIM_ERROR_RAD_APPLIED,
        cs_sim::weapons::guns::SPREAD_SAMPLED,
        "the AI declaration and the weapon-boundary declaration cannot drift"
    );
    let mut runtime = synthetic_combat_runtime();
    let step = fighter_step(&mut runtime, 1, 100, &hostile(40), &synthetic_arsenal());
    let firing = step.trace().firing.as_ref().expect("a target was selected");
    assert!(firing.aim_error_rad > 0.0 && !firing.aim_error_applied());
}
