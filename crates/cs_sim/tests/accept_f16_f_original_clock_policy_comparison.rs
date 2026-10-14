//! Acceptance scenario F16-F: the **measured original clock policy**,
//! declared in code from static analysis of the owner-supplied executable and
//! compared with the project's `ClockPolicy`/`PausePolicy` under a tolerance
//! selected before the comparison.
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! follow-up stage of `### F16-D`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`. The measured answers and their
//! addresses live in
//! `docs/findings/2026-10-06-f16-f-original-clock-pause-and-speed-up-policy.md`
//! ([`ORIGINAL_POLICY_FINDINGS`]), which the second test below pins against
//! the declaration.
//!
//! What makes this scenario discriminating:
//!
//! * The declaration is data the comparison reads. Flipping a declared
//!   constant (the 125 ms cap, the 2× factor, the network gate, "banking is
//!   false", "the timers share a dt source") changes a finding's relation, so
//!   the pinned relation set fails.
//! * The claim is decided by the evidence and then **capped**: static code
//!   evidence can never reach `verified_original`, even when the record
//!   carries an original fingerprint and a locator, and even if someone
//!   hands the constructor a record that would otherwise verify the original.
//! * The tolerance is an input declared before the comparison, not a value
//!   fitted afterwards: the same policy at 4 Hz diverges under the declared
//!   0 ns slack and agrees under a slack declared up front.
//! * The comparison alone would be a table, so
//!   [`accept_f16_f_project_clock_behaves_where_the_policies_agree`] also
//!   drives the real `SimClock` and `GameplayTimeline` to show the project
//!   actually behaves where the comparison claims agreement: pause commits
//!   nothing, resume banks nothing, multiplayer refuses local speed-up.

use std::time::Duration;

use cs_sim::time::{
    ClockPolicy, GameplayTimeline, MAX_FRAME_DT, ORIGINAL_CLOCK_POLICY_STATUS,
    ORIGINAL_IMAGE_SHA256, ORIGINAL_POLICY_FINDINGS, ORIGINAL_POLICY_TOLERANCE,
    OriginalClockPolicy, OriginalDtSource, OriginalSubsystem, PausePolicy, PolicyRelation,
    PolicyTolerance, SimClock, SpeedUpPolicy, TickRate,
};
use cs_types::evidence::ClaimStatus;

/// The project's designed fixed rate: the "project side" of every comparison
/// below. 64 Hz is a designed development value, not a measured original
/// rate.
const TICK_HZ: u32 = 64;
/// The original's measured per-frame `game_dt` cap (owner notes, `0x63b158`),
/// which F16-F-CAP (#721) adopted as [`MAX_FRAME_DT`].
const ORIGINAL_MAX_FRAME_DT_NANOS: u128 = 125_000_000;

fn rate() -> TickRate {
    TickRate::new(TICK_HZ).expect("64 Hz is a valid rate")
}

/// The 14 findings, as sorted field names, so the split below is pinned
/// exactly rather than by membership.
fn fields(
    comparison: &cs_sim::time::OriginalPolicyComparison,
    relation: PolicyRelation,
) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = comparison
        .findings_with(relation)
        .iter()
        .map(|finding| finding.field)
        .collect();
    names.sort_unstable();
    names
}

fn relation_of(
    comparison: &cs_sim::time::OriginalPolicyComparison,
    field: &str,
) -> Option<PolicyRelation> {
    comparison
        .findings()
        .iter()
        .find(|finding| finding.field == field)
        .map(|finding| finding.relation)
}

/// The measured constants from the owner's static analysis are pinned here,
/// not merely described in a comment: this is the declaration the comparison
/// consumes.
#[test]
fn accept_f16_f_measured_original_policy_pins_the_static_analysis_constants() {
    let policy = OriginalClockPolicy::measured_original();
    assert_eq!(
        policy.name(),
        "crimson-skies-2000-original.static-code-analysis"
    );

    // One variable game_dt per rendered frame, clamped to [0, 125 ms].
    assert_eq!(policy.dt_source(), OriginalDtSource::VariableFrameDelta);
    assert_eq!(policy.dt_source().label(), "variable-frame-dt");
    assert_eq!(
        policy.max_frame_dt().as_nanos(),
        ORIGINAL_MAX_FRAME_DT_NANOS
    );
    assert_eq!(policy.max_frame_dt(), Duration::from_millis(125));
    assert_eq!(policy.min_frame_dt(), Duration::ZERO);
    assert!(policy.frame_dt_clamped(), "the clamp is on by default");

    // Paused time is never banked.
    assert!(
        !policy.banks_paused_time(),
        "the original keeps ticking during a pause and banks nothing"
    );

    // One dt source, separate accumulators for the two gameplay timers.
    assert!(policy.gameplay_timers_share_dt_source());
    assert!(
        policy.gameplay_timers_have_separate_accumulators(),
        "the vehicle clock and the mission timers are separate counters"
    );

    // Speed-up: 2x dt, still capped, refused in network games, and a player
    // cannot reach it without an external binding.
    let speed_up = policy.speed_up();
    assert!(
        (speed_up.factor() - 2.0).abs() < f64::EPSILON,
        "the measured speed-up factor is 2x, got {}",
        speed_up.factor()
    );
    assert!(speed_up.capped_by_max_frame_dt());
    assert!(speed_up.network_gated());
    assert!(!speed_up.default_binding());

    // Which subsystems freeze and which keep running: five and five, one
    // explicit pause policy each, and nothing left undeclared.
    let frozen = [
        OriginalSubsystem::WorldSimulation,
        OriginalSubsystem::VehicleClock,
        OriginalSubsystem::MissionObjectives,
        OriginalSubsystem::CameraHudInput,
        OriginalSubsystem::SoundPlayback,
    ];
    let running = [
        OriginalSubsystem::FrameClock,
        OriginalSubsystem::SoundSystemUpdate,
        OriginalSubsystem::Network,
        OriginalSubsystem::ForceFeedbackEffects,
        OriginalSubsystem::CountdownWallClock,
    ];
    assert_eq!(policy.frozen_subsystems().len(), frozen.len());
    assert_eq!(policy.keeps_running_subsystems().len(), running.len());
    assert_eq!(
        policy.frozen_subsystems().len() + policy.keeps_running_subsystems().len(),
        10,
        "every subsystem the owner notes name is declared exactly once"
    );
    for subsystem in frozen {
        assert_eq!(
            policy.pause_policy_for(subsystem),
            Some(PausePolicy::Freeze),
            "{} must freeze while paused",
            subsystem.label()
        );
    }
    for subsystem in running {
        assert_eq!(
            policy.pause_policy_for(subsystem),
            Some(PausePolicy::KeepRunning),
            "{} must keep running while paused",
            subsystem.label()
        );
    }
    for subsystem in frozen {
        assert!(
            !policy.keeps_running_subsystems().contains(&subsystem),
            "{:?} cannot be both frozen and running",
            subsystem
        );
    }
}

/// The evidence behind the declaration is code-derived and non-runtime, so it
/// can never verify the original — and neither can a record that would. The
/// findings entry itself is pinned too: the addresses and the image sha256
/// the acceptance criteria require are really written down.
#[test]
fn accept_f16_f_code_derived_evidence_never_verifies_the_original() {
    let policy = OriginalClockPolicy::measured_original();
    let evidence = policy.evidence();

    assert_eq!(
        evidence.method,
        cs_types::evidence::ObservationMethod::Inference
    );
    assert_ne!(
        evidence.method,
        cs_types::evidence::ObservationMethod::RuntimeObservation,
        "static code analysis is never presented as a runtime observation"
    );
    assert!(
        !evidence.verifies_original(),
        "code-derived evidence must not verify the original"
    );
    let source = evidence.source.clone();
    assert_eq!(
        source,
        cs_types::evidence::EvidenceSource::Document(ORIGINAL_POLICY_FINDINGS.to_string())
    );
    let fingerprint = evidence
        .fingerprint
        .as_ref()
        .expect("the analysed image is named");
    assert_eq!(
        fingerprint.sha256.to_hex(),
        ORIGINAL_IMAGE_SHA256,
        "the declared fingerprint is the owner's image sha256"
    );
    assert_eq!(
        fingerprint.kind,
        cs_types::evidence::FingerprintKind::Installation
    );
    assert!(evidence.locator.is_some(), "the analysis is located");
    assert!(
        evidence
            .limitations
            .iter()
            .any(|l| l.contains("static code analysis")),
        "the limitations must say the evidence is static analysis"
    );
    assert!(
        evidence
            .limitations
            .iter()
            .any(|l| l.contains("unmeasured")),
        "what only a run could give stays recorded as unmeasured"
    );

    // The claim the evidence supports, and the structural ceiling.
    assert_eq!(policy.claim_status(), ClaimStatus::Inferred);
    assert_eq!(policy.claim_status(), ORIGINAL_CLOCK_POLICY_STATUS);
    assert_ne!(policy.claim_status(), ClaimStatus::VerifiedOriginal);

    // A record that *would* verify the original still claims `inferred`: a
    // policy declared in source is compiled from static analysis, so the cap
    // is a property of the type, not of the record it is handed.
    let would_verify = cs_types::evidence::EvidenceRecord {
        source: cs_types::evidence::EvidenceSource::OriginalInstallation,
        fingerprint: evidence.fingerprint,
        locator: evidence.locator.clone(),
        method: cs_types::evidence::ObservationMethod::RuntimeObservation,
        limitations: vec!["a record that claims a run nobody supplied".to_string()],
    };
    assert!(would_verify.verifies_original());
    let raised = OriginalClockPolicy::measured_original().with_evidence(would_verify);
    assert!(raised.evidence().verifies_original());
    assert_eq!(
        raised.claim_status(),
        ORIGINAL_CLOCK_POLICY_STATUS,
        "static code evidence never yields verified_original, whatever record it carries"
    );

    // Synthetic fixture evidence claims nothing at all.
    let synthetic = OriginalClockPolicy::measured_original().with_evidence(
        cs_types::evidence::EvidenceRecord {
            source: cs_types::evidence::EvidenceSource::SyntheticFixture,
            fingerprint: None,
            locator: None,
            method: cs_types::evidence::ObservationMethod::Authored,
            limitations: vec![],
        },
    );
    assert_eq!(synthetic.claim_status(), ClaimStatus::Unknown);

    // The findings entry the locator names exists, and carries the sha256 and
    // the addresses that settle the four questions.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(ORIGINAL_POLICY_FINDINGS);
    let findings = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "the findings entry {} must exist: {e}",
            ORIGINAL_POLICY_FINDINGS
        )
    });
    assert!(
        findings.contains(ORIGINAL_IMAGE_SHA256),
        "the findings entry must record the image sha256"
    );
    for address in ["0x9ad744", "0x71c470", "0x59c0c0", "0x63b158", "0x59c1f0"] {
        assert!(
            findings.contains(address),
            "the findings entry must record {address}"
        );
    }
    assert!(
        findings.contains("#721") && findings.contains("#722"),
        "the divergences must be filed, and the entry must name the tasks"
    );
}

/// The comparison itself: what agrees, what diverges and what the project's
/// clock policies do not model — and the claim, which the evidence decides
/// before the comparison ever runs.
#[test]
fn accept_f16_f_comparison_pins_the_measured_policies_against_the_project_clocks() {
    let comparison = OriginalClockPolicy::measured_original()
        .compare_project_clocks(rate(), ORIGINAL_POLICY_TOLERANCE);

    assert_eq!(
        comparison.policy(),
        "crimson-skies-2000-original.static-code-analysis"
    );
    assert_eq!(comparison.project_rate(), rate());
    assert_eq!(comparison.tolerance(), ORIGINAL_POLICY_TOLERANCE);
    assert_eq!(
        comparison.tolerance().dt_slack_nanos,
        0,
        "the declared tolerance is exact equality: no slack"
    );
    assert_eq!(comparison.findings().len(), 14);

    assert_eq!(
        fields(&comparison, PolicyRelation::Agrees),
        vec![
            "clock.frame-dt-bound",
            "clock.frame-dt-cap",
            "pause.bank",
            "pause.gameplay",
            "pause.presentation",
            "speed-up.network-gate",
            "timers.shared-dt-source",
        ],
        "the original freezes the flight world, keeps presentation running, \
         banks nothing, shares one dt source between the two timers, gates \
         the speed-up off in network games and clamps each frame's game_dt — \
         all of which the project's policies do, the last since F16-F-CAP \
         adopted the measured bound as MAX_FRAME_DT"
    );
    assert_eq!(
        fields(&comparison, PolicyRelation::Diverges),
        vec![
            "clock.tick-source",
            "speed-up.mechanism",
            "speed-up.reaches-gameplay-timers",
        ],
        "the fixed tick, the fixed-tick speed-up mechanism and the gameplay \
         clock's missing speed-up authority are the divergences that remain \
         after #721 adopted the frame-dt cap"
    );
    assert_eq!(
        fields(&comparison, PolicyRelation::NotModeled),
        vec![
            "pause.network",
            "pause.sound-playback",
            "speed-up.default-binding",
            "timers.player-down-gate",
        ],
        "subsystems outside these clock policies must be named, not silently \
         counted as agreement"
    );

    for finding in comparison.findings() {
        assert!(
            !finding.original.is_empty(),
            "{} must state what the original does",
            finding.field
        );
        assert!(
            !finding.project.is_empty(),
            "{} must state what the project declares",
            finding.field
        );
        assert!(
            finding.relation.label() == "agrees"
                || finding.relation.label() == "diverges"
                || finding.relation.label() == "not-modeled",
            "every relation has a stable label"
        );
    }

    // The claim is the evidence's, decided before the comparison: static code
    // evidence is `inferred` and never `verified_original`, and the designed
    // divergences do not downgrade or upgrade it.
    assert_eq!(comparison.claim(), ClaimStatus::Inferred);
    assert_eq!(comparison.claim(), ORIGINAL_CLOCK_POLICY_STATUS);
    assert_ne!(comparison.claim(), ClaimStatus::VerifiedOriginal);
    assert!(!comparison.verified_original());
    assert!(
        comparison.summary().contains("claim inferred"),
        "{}",
        comparison.summary()
    );
    assert!(
        comparison.summary().contains("3 diverge"),
        "{}",
        comparison.summary()
    );
}

/// The tolerance is selected **before** the comparison runs: the same policy
/// and the same project rate give different answers under different declared
/// tolerances, and the comparison reports the tolerance it was handed rather
/// than widening it to fit.
#[test]
fn accept_f16_f_tolerance_is_selected_before_the_comparison() {
    let policy = OriginalClockPolicy::measured_original();
    assert_eq!(
        ORIGINAL_POLICY_TOLERANCE.dt_slack_nanos, 0,
        "F16-F declares exact equality before comparing anything"
    );

    // At 64 Hz one tick is 15 625 000 ns: inside the original's measured
    // [0, 125 000 000 ns] window, so the project's dt agrees with it.
    let at_64 = policy.compare_project_clocks(rate(), ORIGINAL_POLICY_TOLERANCE);
    assert_eq!(
        relation_of(&at_64, "clock.frame-dt-bound"),
        Some(PolicyRelation::Agrees),
        "{}",
        at_64.summary()
    );

    // At 4 Hz one tick is 250 000 000 ns: outside the window, and the
    // declared 0 ns slack does not widen it.
    let slow = TickRate::new(4).expect("4 Hz is a valid rate");
    let at_4 = policy.compare_project_clocks(slow, ORIGINAL_POLICY_TOLERANCE);
    assert_eq!(
        relation_of(&at_4, "clock.frame-dt-bound"),
        Some(PolicyRelation::Diverges),
        "{}",
        at_4.summary()
    );
    assert_eq!(at_4.tolerance(), ORIGINAL_POLICY_TOLERANCE);

    // The same 4 Hz run under a *larger declared* tolerance agrees: 125 ms of
    // declared slack puts 250 000 000 ns exactly on the 125 000 000 + slack
    // boundary, which is inside. The slack is an input chosen before the
    // comparison, not a fitted fudge.
    let generous = PolicyTolerance {
        dt_slack_nanos: 125_000_000,
    };
    let at_4_wide = policy.compare_project_clocks(slow, generous);
    assert_eq!(at_4_wide.tolerance(), generous);
    assert_eq!(
        relation_of(&at_4_wide, "clock.frame-dt-bound"),
        Some(PolicyRelation::Agrees),
        "{}",
        at_4_wide.summary()
    );
    // The slack only ever moves the fact it governs: the variable-dt
    // divergence is untouched by it.
    assert_eq!(
        relation_of(&at_4_wide, "clock.tick-source"),
        Some(PolicyRelation::Diverges)
    );

    // The debug -freq variant is a fixed dt too, so it can agree with the
    // project's fixed dt — exactly, at the declared tolerance, and only when
    // the two rates match.
    let fixed_64 = policy
        .clone()
        .with_dt_source(OriginalDtSource::FixedDebugFrequency {
            period: Duration::from_nanos(15_625_000),
        });
    let same = fixed_64.compare_project_clocks(rate(), ORIGINAL_POLICY_TOLERANCE);
    assert_eq!(
        relation_of(&same, "clock.tick-source"),
        Some(PolicyRelation::Agrees),
        "{}",
        same.summary()
    );
    let fixed_30 = policy
        .clone()
        .with_dt_source(OriginalDtSource::FixedDebugFrequency {
            period: Duration::from_nanos(33_333_333),
        });
    let different = fixed_30.compare_project_clocks(rate(), ORIGINAL_POLICY_TOLERANCE);
    assert_eq!(
        relation_of(&different, "clock.tick-source"),
        Some(PolicyRelation::Diverges),
        "{}",
        different.summary()
    );
    let admitted = fixed_30.compare_project_clocks(
        rate(),
        PolicyTolerance {
            dt_slack_nanos: 33_333_333 - 15_625_000,
        },
    );
    assert_eq!(
        relation_of(&admitted, "clock.tick-source"),
        Some(PolicyRelation::Agrees),
        "{}",
        admitted.summary()
    );
    assert_eq!(
        admitted.tolerance().dt_slack_nanos,
        33_333_333 - 15_625_000,
        "the comparison reports the tolerance it was given"
    );
}

/// A table of agreements is worth nothing if the project does not actually
/// behave that way, so the three behaviours the comparison marks "agrees" are
/// driven here through the production clock and timeline: pause commits zero
/// ticks, resume banks nothing, and local speed-up is granted only where the
/// original's network gate would grant it.
#[test]
fn accept_f16_f_project_clock_behaves_where_the_policies_agree() {
    let policy = OriginalClockPolicy::measured_original();
    let comparison = policy.compare_project_clocks(rate(), ORIGINAL_POLICY_TOLERANCE);
    assert_eq!(
        relation_of(&comparison, "pause.gameplay"),
        Some(PolicyRelation::Agrees)
    );
    assert_eq!(
        relation_of(&comparison, "pause.bank"),
        Some(PolicyRelation::Agrees)
    );
    assert_eq!(
        relation_of(&comparison, "speed-up.network-gate"),
        Some(PolicyRelation::Agrees)
    );
    assert_eq!(
        relation_of(&comparison, "clock.frame-dt-cap"),
        Some(PolicyRelation::Agrees),
        "F16-F-CAP adopted the measured 125 ms bound as MAX_FRAME_DT"
    );

    // `pause.bank`: paused wall time is dropped, so resume cannot release a
    // burst the original never banks either. The unpaused one-second frames
    // exceed the cap, so each commits the cap's 8 ticks — the same thing the
    // original's clamp does to a stalled frame.
    let mut clock = SimClock::new(ClockPolicy::single_player_simulation(), rate());
    assert_eq!(clock.advance(Duration::from_secs(1)), Ok(8));
    clock.set_paused(true);
    assert_eq!(
        clock.advance(Duration::from_secs(30)),
        Ok(0),
        "a paused frame commits nothing"
    );
    clock.set_paused(false);
    assert_eq!(
        clock.advance(Duration::from_secs(1)),
        Ok(8),
        "a stalled frame after resume commits the cap, not thirty-one seconds"
    );
    assert_eq!(
        clock.tick().0,
        16,
        "8 ticks before the pause and 8 after it, nothing from the pause"
    );

    // `pause.gameplay`: both gameplay timers freeze together, because they
    // are fed only ticks the clock committed.
    let mut timeline = GameplayTimeline::new(rate(), 120, 600).expect("periods are positive");
    assert_eq!(
        timeline.advance_frame(Duration::from_millis(500)),
        Ok(8),
        "a 500 ms frame is over the cap and commits its 8 ticks"
    );
    let before = (
        timeline.cooldown().remaining_ticks(),
        timeline.objective_timer().remaining_ticks(),
    );
    assert_eq!(before, (112, 592));
    timeline.set_paused(true);
    assert_eq!(timeline.advance_frame(Duration::from_secs(30)), Ok(0));
    assert_eq!(
        (
            timeline.cooldown().remaining_ticks(),
            timeline.objective_timer().remaining_ticks()
        ),
        before,
        "pause advances neither the cooldown nor the objective timer"
    );
    timeline.set_paused(false);
    assert_eq!(timeline.advance_frame(Duration::from_millis(500)), Ok(8));
    assert_eq!(
        (
            timeline.cooldown().remaining_ticks(),
            timeline.objective_timer().remaining_ticks()
        ),
        (104, 584),
        "resume banks none of the paused time"
    );

    // `speed-up.network-gate`: the single-player policy accepts whole ticks,
    // the multiplayer policy refuses them by name.
    let mut single_player = SimClock::new(ClockPolicy::single_player_simulation(), rate());
    assert_eq!(single_player.advance_fixed_ticks(2), Ok(()));
    assert_eq!(single_player.tick().0, 2);
    let mut multiplayer = SimClock::new(ClockPolicy::multiplayer_simulation(), rate());
    assert_eq!(
        multiplayer.advance_fixed_ticks(2),
        Err(cs_sim::time::TimeError::NoSpeedUpAuthority),
        "the network gate the original enforces is the multiplayer refusal here"
    );
    assert_eq!(multiplayer.tick().0, 0);

    // The speed-up's declared cap is the original's: the project's fixed dt
    // fits inside it, so one tick can never advance more game time than one
    // original frame may.
    let dt_nanos = 1_000_000_000u128 / u128::from(rate().ticks_per_second());
    assert!(dt_nanos <= policy.max_frame_dt().as_nanos());
    assert!(policy.speed_up().capped_by_max_frame_dt());
    assert!(
        matches!(
            ClockPolicy::multiplayer_simulation().speed_up(),
            SpeedUpPolicy::NoLocalAuthority
        ),
        "the multiplayer side of the declared gate"
    );
}

/// F16-F-CAP (#721), the decided divergence: the original frame clock
/// (`0x59c0c0`) clamps every frame's `game_dt` to `[0, 125 ms]` after scaling
/// (clamp flag `0x63b160` default 1, max `0x63b158`, min `0x63b15c`), and the
/// project adopted the same bound — [`MAX_FRAME_DT`], enforced inside
/// `SimClock::advance`. A frame longer than the cap advances exactly the cap.
#[test]
fn accept_f16_f_cap_a_stalled_frame_advances_exactly_the_cap() {
    // The enforced bound is the measured original's, declared once — not a
    // value fitted to this test.
    assert_eq!(MAX_FRAME_DT, Duration::from_millis(125));
    assert_eq!(
        OriginalClockPolicy::measured_original().max_frame_dt(),
        MAX_FRAME_DT,
        "the declared policy and the enforced cap share one bound"
    );

    // Every domain's clock is the frame-clock accumulator, so the cap is
    // uniform: a stalled frame commits the cap's 8 ticks at 64 Hz, whatever
    // the policy, and however far past the bound the frame ran.
    for policy in [
        ClockPolicy::single_player_simulation(),
        ClockPolicy::multiplayer_simulation(),
        ClockPolicy::authoritative_gameplay(),
        ClockPolicy::ui_wall(),
        ClockPolicy::media_unscaled(),
    ] {
        let mut clock = SimClock::new(policy, rate());
        for stalled in [
            Duration::from_millis(126),
            Duration::from_secs(5),
            Duration::from_secs(30),
        ] {
            assert_eq!(
                clock.advance(stalled).expect("the clock advances"),
                8,
                "{}: {stalled:?} must commit the cap's 8 ticks at {TICK_HZ} Hz",
                policy.domain().label()
            );
        }
        assert_eq!(clock.tick().0, 24);
    }

    // The excess is dropped, never banked: after a 5 s stall the clock holds
    // no debt, so the next ordinary frame commits only its own ticks.
    let mut clock = SimClock::new(ClockPolicy::single_player_simulation(), rate());
    assert_eq!(clock.advance(Duration::from_secs(5)), Ok(8));
    assert_eq!(
        clock.advance(Duration::from_nanos(15_625_000)),
        Ok(1),
        "the frame after the stall is one ordinary 1/64 s frame, not the banked remainder"
    );
    assert_eq!(clock.tick().0, 9);

    // The gameplay quantities see the same bound: a stalled frame feeds the
    // timers the cap's ticks and nothing more.
    let mut timeline = GameplayTimeline::new(rate(), 120, 600).expect("periods are positive");
    assert_eq!(timeline.advance_frame(Duration::from_secs(5)), Ok(8));
    assert_eq!(timeline.cooldown().remaining_ticks(), 112);
    assert_eq!(timeline.objective_timer().remaining_ticks(), 592);
}

/// The other side of the pin: a frame at or under [`MAX_FRAME_DT`] is
/// untouched, and the sub-tick carry still accumulates exactly across
/// consecutive short frames.
#[test]
fn accept_f16_f_cap_a_frame_within_the_cap_is_untouched() {
    let mut clock = SimClock::new(ClockPolicy::single_player_simulation(), rate());

    // The boundary itself is not clamped: exactly 125 ms commits its ticks.
    assert_eq!(clock.advance(MAX_FRAME_DT), Ok(8));

    // A frame one millisecond under the cap commits its own ticks; the
    // sub-tick remainder carries, so a 1 ms frame after it commits the tick
    // the pair was owed — inside the cap nothing is dropped or invented.
    assert_eq!(clock.advance(Duration::from_millis(124)), Ok(7));
    assert_eq!(
        clock.advance(Duration::from_millis(1)),
        Ok(1),
        "the carry still delivers the tick the two frames sum to"
    );
    assert_eq!(clock.tick().0, 16);

    // Five 100 ms frames of an ordinary delivery: 500 ms untouched, 32 ticks.
    let mut clock = SimClock::new(ClockPolicy::authoritative_gameplay(), rate());
    for _ in 0..5 {
        clock.advance(Duration::from_millis(100)).expect("runs");
    }
    assert_eq!(clock.tick().0, 32, "frames inside the cap are untouched");
}
