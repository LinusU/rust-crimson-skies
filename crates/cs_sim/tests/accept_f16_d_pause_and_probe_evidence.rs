//! Acceptance scenario F16-D (AC04): pause produces **zero** weapon cooldown
//! and objective timer advancement, measured by a fixed-tick behavioral probe
//! and compared against an attributed reference trace.
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! stage `### F16-D`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Every test here drives production code only: the `cs_sim::time`
//! [`GameplayTimeline`] (the clocked consumer), [`BehavioralProbe`] (the
//! measurement) and [`ProbeComparison`] (the comparison). Nothing in this file
//! reimplements a countdown, a pause or a frame split.
//!
//! What makes the scenario discriminating:
//!
//! * If the timers were advanced from the frame's wall delta instead of from
//!   committed ticks, 30 s of paused wall time would empty both of them — so
//!   the equality of the paused samples fails.
//! * If paused wall time were *banked*, the first frame after the resume
//!   would commit 30 s worth of ticks: the `after-resume` tick fails.
//! * If the pause were a no-op, the paused samples would still be equal to
//!   each other, so the **control** run — the same script with the pause
//!   removed — is what makes the paused equality mean something: there the
//!   same 30 s does advance both quantities.
//! * If the clock stepped once per render frame, the traces at 30, 60 and 144
//!   render FPS would disagree.
//!
//! Every tick count, period and frame rate here is a newly authored
//! development value. The reference traces are attributed to newly authored
//! fixture content, so a comparison that agrees claims `observed_tool` and
//! never `verified_original` — see
//! [`accept_f16_d_measured_trace_matches_an_attributed_reference`].

use cs_sim::time::{
    BehavioralProbe, GameplayTimeline, MAX_FRAMES_PER_STEP, ProbeComparison, ProbeError,
    ProbeReference, ProbeStep, TickRate,
};
use cs_types::Tick;
use cs_types::evidence::{
    ClaimStatus, EvidenceRecord, EvidenceSource, ObservationLocator, ObservationMethod,
};

/// Fixed simulation rate of the fixture. 64 Hz is a designed default shared
/// with the F16-C frame-rate fixture, not a measured original rate.
const TICK_HZ: u32 = 64;
/// Weapon cooldown period: 64 ticks is exactly one second at [`TICK_HZ`].
const COOLDOWN_TICKS: u64 = 64;
/// Objective deadline period: 600 ticks is 9.375 s at [`TICK_HZ`].
const OBJECTIVE_TICKS: u64 = 600;
/// Render frame rates the same script is delivered at, including one that
/// does not divide a second evenly (7 fps leaves a 6 ns remainder frame).
const RENDER_FPS: [u32; 4] = [7, 30, 60, 144];

const MS: u64 = 1_000_000;
const SECOND: u64 = 1_000_000_000;

/// The AC04 scenario: 500 ms of play, a pause covering 30 s of wall time
/// delivered both as one 30 s frame and as many short frames, a resume, a shot
/// that re-arms the cooldown, and 250 ms more play.
const PAUSE_SCRIPT: &[ProbeStep] = &[
    ProbeStep::Advance { nanos: 500 * MS },
    ProbeStep::Observe("before-pause"),
    ProbeStep::Pause(true),
    ProbeStep::OneFrame { nanos: 30 * SECOND },
    ProbeStep::Observe("during-pause-single-frame"),
    ProbeStep::Advance { nanos: 30 * SECOND },
    ProbeStep::Observe("during-pause-many-frames"),
    ProbeStep::Pause(false),
    ProbeStep::Fire,
    ProbeStep::Observe("after-fire"),
    ProbeStep::Advance { nanos: 250 * MS },
    ProbeStep::Observe("after-resume"),
];

/// The same script with the pause removed, as a control: it proves the paused
/// equality below is a property of pausing and not of the fixture.
const UNPAUSED_CONTROL: &[ProbeStep] = &[
    ProbeStep::Advance { nanos: 500 * MS },
    ProbeStep::Observe("before-pause"),
    ProbeStep::OneFrame { nanos: 30 * SECOND },
    ProbeStep::Observe("during-pause-single-frame"),
    ProbeStep::Advance { nanos: 30 * SECOND },
    ProbeStep::Observe("during-pause-many-frames"),
    ProbeStep::Fire,
    ProbeStep::Observe("after-fire"),
    ProbeStep::Advance { nanos: 250 * MS },
    ProbeStep::Observe("after-resume"),
];

/// The fixture's timeline: authoritative gameplay clock, one second of
/// weapon cooldown, 9.375 s of objective deadline.
fn timeline() -> Result<GameplayTimeline, cs_sim::time::TimeError> {
    GameplayTimeline::new(
        TickRate::new(TICK_HZ).expect("64 Hz is a valid rate"),
        COOLDOWN_TICKS,
        OBJECTIVE_TICKS,
    )
}

/// Evidence for a reference that was *not* observed in an original run.
///
/// `RuntimeObservation` is deliberately **not** used: nothing in this tree
/// observed the original program, and a record that claimed to would be
/// fabricated evidence. [`accept_f16_d_measured_trace_matches_an_attributed_reference`]
/// asserts that this record cannot verify the original whatever a comparison
/// concludes.
fn fixture_evidence() -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::SyntheticFixture,
        fingerprint: None,
        locator: Some(ObservationLocator {
            container: "crates/cs_sim/tests/accept_f16_d_pause_and_probe_evidence.rs".to_string(),
            span: None,
        }),
        method: ObservationMethod::Authored,
        limitations: vec![
            "newly authored fixture timeline; the original game was not run".to_string(),
            "the pause/cooldown/objective pairing is a designed default, not a \
             measured original policy"
                .to_string(),
        ],
    }
}

/// AC04's minimum scenario. 30 s of paused wall time — as one frame and as
/// many — advances the weapon cooldown and the objective timer by exactly zero
/// ticks, at every render frame rate, and the resume banks none of it.
#[test]
fn accept_f16_d_pause_produces_zero_weapon_cooldown_and_objective_advancement() {
    for render_fps in RENDER_FPS {
        let probe = BehavioralProbe::new("pause-zero-advancement", render_fps, PAUSE_SCRIPT)
            .expect("the fixture script is a valid probe");
        let trace = probe.run(timeline).expect("the probe runs");

        // 500 ms of play, 30 s paused twice, 250 ms of play: 32 + 16 ticks.
        assert_eq!(
            trace.committed_ticks(),
            48,
            "at {render_fps} fps the script commits only the 750 ms of unpaused play"
        );

        let before = trace.sample("before-pause").expect("observed");
        assert_eq!(before.tick(), Tick(32), "500 ms at 64 Hz is 32 ticks");
        assert_eq!(before.cooldown_remaining_ticks(), 32);
        assert_eq!(before.objective_remaining_ticks(), 568);
        assert!(!before.paused());

        for label in ["during-pause-single-frame", "during-pause-many-frames"] {
            let sample = trace.sample(label).expect("observed");
            assert!(sample.paused(), "{label} was observed while paused");
            assert_eq!(
                (sample.tick(), sample.cooldown_remaining_ticks()),
                (before.tick(), before.cooldown_remaining_ticks()),
                "{label}: a paused frame advances no tick and no weapon cooldown \
                 at {render_fps} fps"
            );
            assert_eq!(
                (
                    sample.objective_remaining_ticks(),
                    sample.cooldown_expirations()
                ),
                (
                    before.objective_remaining_ticks(),
                    before.cooldown_expirations()
                ),
                "{label}: a paused frame advances no objective timer at {render_fps} fps"
            );
        }

        // The shot re-arms the cooldown and does not touch the objective: the
        // resume then banks no paused time, so the tick moves by exactly the
        // 250 ms that followed it.
        let after_fire = trace.sample("after-fire").expect("observed");
        assert_eq!(after_fire.tick(), Tick(32), "firing costs no wall time");
        assert_eq!(
            after_fire.cooldown_remaining_ticks(),
            COOLDOWN_TICKS,
            "firing re-arms the cooldown to its full period"
        );
        assert_eq!(
            after_fire.objective_remaining_ticks(),
            568,
            "firing is not a pause: the objective timer keeps counting"
        );

        let after_resume = trace.sample("after-resume").expect("observed");
        assert_eq!(
            after_resume.tick(),
            Tick(48),
            "resume must bank none of the 30 s paused wall time"
        );
        assert_eq!(
            after_resume.cooldown_remaining_ticks(),
            48,
            "16 ticks of the re-armed cooldown elapsed, not 1936"
        );
        assert_eq!(after_resume.objective_remaining_ticks(), 552);
    }

    // The control: the same script without the pause *does* advance both
    // quantities, so the equality above is about pausing and not about the
    // fixture being inert.
    let control = BehavioralProbe::new("unpaused-control", 30, UNPAUSED_CONTROL)
        .expect("the control script is a valid probe")
        .run(timeline)
        .expect("the control runs");
    let before = control.sample("before-pause").expect("observed");
    let later = control
        .sample("during-pause-many-frames")
        .expect("observed");
    assert_eq!(before.cooldown_remaining_ticks(), 32);
    assert_eq!(
        later.cooldown_remaining_ticks(),
        0,
        "without a pause, 30.5 s of wall time empties the one-second cooldown"
    );
    assert_eq!(later.cooldown_expirations(), 1);
    assert_eq!(before.objective_remaining_ticks(), 568);
    assert!(
        later.objective_remaining_ticks() < 568,
        "without a pause, the objective timer counts down: got {}",
        later.objective_remaining_ticks()
    );
    assert_eq!(
        control.committed_ticks(),
        60_750 * 64 / 1_000,
        "60.75 s of unpaused play at 64 Hz is 3888 ticks"
    );
}

/// The gameplay quantities are frame-rate independent: the same script at 7,
/// 30, 60 and 144 render FPS commits the same ticks and reports the same
/// cooldown and objective values at every labelled sample. 7 fps is included
/// because it does not divide a second evenly, so the remainder frame is on
/// the path.
#[test]
fn accept_f16_d_probe_traces_agree_across_render_frame_rates() {
    let traces: Vec<_> = RENDER_FPS
        .into_iter()
        .map(|render_fps| {
            BehavioralProbe::new("pause-zero-advancement", render_fps, PAUSE_SCRIPT)
                .expect("the fixture script is a valid probe")
                .run(timeline)
                .expect("the probe runs")
        })
        .collect();

    let reference = &traces[0];
    // Each trace must report the rate it was actually delivered at, or the
    // comparison below would be vacuous: agreement between traces that all
    // reported the same rate would prove nothing about frame-rate
    // independence.
    let reported: Vec<u32> = traces.iter().map(|trace| trace.render_fps()).collect();
    assert_eq!(
        reported,
        RENDER_FPS.to_vec(),
        "each trace must report the render rate it was delivered at"
    );
    assert!(
        RENDER_FPS.windows(2).all(|pair| pair[0] != pair[1]),
        "the frame rates must be distinct for the agreement to mean anything"
    );
    for trace in &traces[1..] {
        assert_eq!(
            trace.committed_ticks(),
            reference.committed_ticks(),
            "{} fps and {} fps commit different ticks",
            trace.render_fps(),
            reference.render_fps()
        );
        assert_eq!(
            trace.samples(),
            reference.samples(),
            "{} fps and {} fps disagree on a labelled sample",
            trace.render_fps(),
            reference.render_fps()
        );
    }
}

/// The comparison half of the stage: a measured trace is compared against an
/// attributed reference, and the claim it reports is decided by the
/// reference's **evidence**, not by the comparison passing.
///
/// The reference trace is the designed scenario the spec prescribes, recorded
/// as newly authored fixture content. It agrees with the measurement, and the
/// result is `observed_tool`: the machinery works, and nothing here has
/// observed the original game.
#[test]
fn accept_f16_d_measured_trace_matches_an_attributed_reference() {
    let evidence = fixture_evidence();
    assert!(
        !evidence.verifies_original(),
        "fixture evidence must not be able to verify the original"
    );

    let measured = BehavioralProbe::new("pause-zero-advancement", 60, PAUSE_SCRIPT)
        .expect("the fixture script is a valid probe")
        .run(timeline)
        .expect("the probe runs");
    let reference = ProbeReference::new(
        "f16d-reference.pause-zero-advancement",
        measured.clone(),
        evidence,
        0,
    );
    assert_eq!(reference.tolerance_ticks(), 0);

    let comparison = ProbeComparison::new(&measured, &reference);
    assert!(comparison.agrees(), "{}", comparison.summary());
    assert_eq!(
        comparison.claim(),
        ClaimStatus::ObservedTool,
        "a designed reference can support an observed-tool claim and nothing more"
    );
    assert!(!comparison.verified_original());
    assert_eq!(comparison.divergences(), &[]);
    assert_eq!(comparison.measured().summary(), reference.trace().summary());
    assert!(
        comparison.summary().contains("agree"),
        "{}",
        comparison.summary()
    );

    // A reference that describes a different pause policy contradicts the
    // measurement, and a contradicting comparison never claims agreement. The
    // contradicting reference is a real measured trace of the *unpaused*
    // script, not a poked value.
    let unpaused = BehavioralProbe::new("unpaused-control", 60, UNPAUSED_CONTROL)
        .expect("the control script is a valid probe")
        .run(timeline)
        .expect("the control runs");
    let comparison = ProbeComparison::new(
        &measured,
        &ProbeReference::new(
            "reference.pause-does-not-freeze",
            unpaused,
            fixture_evidence(),
            0,
        ),
    );
    assert!(!comparison.agrees(), "{}", comparison.summary());
    assert_eq!(comparison.claim(), ClaimStatus::Contradicted);
    let paused_label = "during-pause-single-frame";
    assert!(
        comparison
            .divergences()
            .iter()
            .any(|d| d.label == paused_label
                && d.field == "cooldown_remaining_ticks"
                && d.measured == 32
                && d.expected == 0),
        "the reference claims pause emptied the cooldown; the measurement says \
         it did not: {}",
        comparison.summary()
    );
    assert!(
        comparison
            .divergences()
            .iter()
            .any(|d| d.label == paused_label && d.field == "objective_remaining_ticks"),
        "{}",
        comparison.summary()
    );
}

/// The failure cases the probe and the comparison must surface: an unusable
/// render rate, a script that observes nothing, a repeated label, an unbounded
/// frame count, and a refused timeline.
#[test]
fn accept_f16_d_probe_failure_cases_are_named_and_propagate() {
    static SILENT: &[ProbeStep] = &[ProbeStep::Pause(true)];
    static REPEATED: &[ProbeStep] = &[
        ProbeStep::Observe("twice"),
        ProbeStep::Observe("once"),
        ProbeStep::Observe("twice"),
    ];
    static HUGE: &[ProbeStep] = &[
        ProbeStep::Advance { nanos: u64::MAX },
        ProbeStep::Observe("never"),
    ];
    static ONE_SAMPLE: &[ProbeStep] = &[ProbeStep::Observe("only")];

    assert_eq!(
        BehavioralProbe::new("zero-rate", 0, ONE_SAMPLE),
        Err(ProbeError::UnusableRenderRate { render_fps: 0 })
    );
    assert_eq!(
        BehavioralProbe::new("silent", 60, SILENT),
        Err(ProbeError::NoObservations)
    );
    assert_eq!(
        BehavioralProbe::new("repeated", 60, REPEATED),
        Err(ProbeError::DuplicateObservationLabel { label: "twice" })
    );
    let huge = BehavioralProbe::new("huge", 144, HUGE).expect("the script itself is valid");
    assert!(
        matches!(
            huge.run(timeline),
            Err(ProbeError::FrameCountTooLarge { nanos, frames })
                if nanos == u64::MAX && frames > MAX_FRAMES_PER_STEP
        ),
        "an unbounded frame count must be refused, not attempted"
    );
    let probe = BehavioralProbe::new("refused", 60, ONE_SAMPLE).expect("valid probe");
    assert_eq!(
        probe.run(|| {
            GameplayTimeline::new(TickRate::new(TICK_HZ).expect("valid"), 0, OBJECTIVE_TICKS)
        }),
        Err(ProbeError::Time(cs_sim::time::TimeError::ZeroTimerPeriod)),
        "a refused timeline must not produce a trace"
    );
}
