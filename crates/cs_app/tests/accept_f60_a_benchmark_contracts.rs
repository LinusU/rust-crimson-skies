//! Acceptance scenarios F60-A: benchmark scenarios, hardware-specific budgets
//! and the 60-minute soak with memory-trend bounds.
//! Task test prefix: `accept_f60_a_`.

use cs_app::diagnostics::scenario::{FRAME_BUDGET_US_60FPS, SIM_TICK_BUDGET_US_120HZ};
use cs_app::diagnostics::{
    BudgetLimit, HardwareProfile, LeakCounter, MemoryTrendBound, Platform, Quality,
    RunMeasurements, ScenarioError, ScenarioKind, SoakError, SoakPhase, SoakPlan, SystemKind,
    Verdict, evaluate_soak, percentiles, scenario_for, synthetic_soak_samples,
};

const MIB: u64 = 1024 * 1024;

fn run_for(profile: &HardwareProfile, frame_p95: u64) -> RunMeasurements {
    let p = |v| percentiles(&[v]).expect("one sample");
    RunMeasurements {
        profile_id: profile.id.clone(),
        resolution: profile.resolution,
        quality: profile.quality,
        enabled_systems: SystemKind::ALWAYS.to_vec(),
        frame_us: Some(p(frame_p95)),
        sim_tick_us: Some(p(SIM_TICK_BUDGET_US_120HZ)),
        load_ms: None,
        peak_memory_bytes: None,
        cache_bytes: None,
    }
}

#[test]
fn accept_f60_a_designed_soak_is_sixty_minutes_and_passes_when_flat() {
    let plan = SoakPlan::designed();
    assert_eq!(plan.total_ticks(), 60 * 60 * 120);
    assert!(
        plan.cycle
            .iter()
            .any(|(p, _)| *p == SoakPhase::InstantAction)
    );
    // Flat after the one-off warm-up fill: the warm-up must not read as growth.
    let samples = synthetic_soak_samples(&plan, 0, 0);
    let report = evaluate_soak(&plan, MemoryTrendBound::DESIGNED, &samples).expect("judged");
    assert!(report.passed(), "{report:?}");
    assert_eq!(report.growth_bytes_per_hour, 0);
}

/// AC01 says "mission/IA/menu". `IA` is this project's abbreviation for
/// Instant Action (F49), not an AI phase: the designed cycle is campaign
/// mission, then an Instant Action mission, then the menu. This fails if the
/// middle phase regresses to an AI-engagement reading.
#[test]
fn accept_f60_a_soak_cycle_is_mission_instant_action_menu() {
    let plan = SoakPlan::designed();
    let phases: Vec<SoakPhase> = plan.cycle.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        phases,
        vec![
            SoakPhase::MissionPlay,
            SoakPhase::InstantAction,
            SoakPhase::MenuReturn
        ]
    );
    assert_eq!(
        plan.cycle
            .iter()
            .find(|(p, _)| *p == SoakPhase::InstantAction)
            .map(|(_, t)| *t),
        Some(2 * 60 * 120)
    );
}

#[test]
fn accept_f60_a_memory_trend_over_the_bound_fails() {
    let plan = SoakPlan::designed();
    // 10 MiB per 6-minute cycle = 100 MiB/h, over the 16 MiB/h bound.
    let samples = synthetic_soak_samples(&plan, 10 * MIB, 0);
    let report = evaluate_soak(&plan, MemoryTrendBound::DESIGNED, &samples).expect("judged");
    assert!(!report.trend_within_bound && !report.passed());
    assert_eq!(report.growth_bytes_per_hour, (100 * MIB) as i128);
    // A bound just above the slope passes, just below fails: exact arithmetic.
    let at = MemoryTrendBound {
        max_growth_bytes_per_hour: 100 * MIB,
    };
    assert!(
        evaluate_soak(&plan, at, &samples)
            .unwrap()
            .trend_within_bound
    );
    let below = MemoryTrendBound {
        max_growth_bytes_per_hour: 100 * MIB - 1,
    };
    assert!(
        !evaluate_soak(&plan, below, &samples)
            .unwrap()
            .trend_within_bound
    );
}

#[test]
fn accept_f60_a_leaked_entities_are_named_per_cycle() {
    let plan = SoakPlan::designed();
    let samples = synthetic_soak_samples(&plan, 0, 5);
    let report = evaluate_soak(&plan, MemoryTrendBound::DESIGNED, &samples).expect("judged");
    assert!(!report.passed());
    assert!(
        report.trend_within_bound,
        "memory is flat; only entities leak"
    );
    assert_eq!(
        report.leaks.len(),
        9,
        "cycles 1..=9 against the warm-up baseline"
    );
    assert!(
        report
            .leaks
            .iter()
            .all(|l| l.counter == LeakCounter::Entities)
    );
    assert_eq!(
        (
            report.leaks[0].cycle,
            report.leaks[0].got - report.leaks[0].baseline
        ),
        (1, 5)
    );

    for (counter, edit) in [
        (LeakCounter::AssetHandles, 0usize),
        (LeakCounter::AudioLoops, 1),
        (LeakCounter::Tasks, 2),
    ] {
        let mut s = synthetic_soak_samples(&plan, 0, 0);
        match edit {
            0 => s[7].asset_handles += 1,
            1 => s[7].audio_loops += 1,
            _ => s[7].tasks += 1,
        }
        let r = evaluate_soak(&plan, MemoryTrendBound::DESIGNED, &s).unwrap();
        assert_eq!(r.leaks.len(), 1);
        assert_eq!((r.leaks[0].cycle, r.leaks[0].counter), (7, counter));
    }
}

#[test]
fn accept_f60_a_soak_without_evidence_is_an_error_not_a_pass() {
    let plan = SoakPlan::designed();
    let bound = MemoryTrendBound::DESIGNED;
    let mut short = plan.clone();
    short.cycles = 9;
    assert!(matches!(
        evaluate_soak(&short, bound, &synthetic_soak_samples(&short, 0, 0)),
        Err(SoakError::PlanTooShort { .. })
    ));
    let mut no_menu = plan.clone();
    no_menu.cycle.retain(|(p, _)| *p != SoakPhase::MenuReturn);
    assert_eq!(
        evaluate_soak(&no_menu, bound, &[]),
        Err(SoakError::PlanMissingPhase)
    );
    let samples = synthetic_soak_samples(&plan, 0, 0);
    assert_eq!(
        evaluate_soak(&plan, bound, &samples[..9]),
        Err(SoakError::SampleCount {
            expected: 10,
            got: 9
        })
    );
    let mut late = samples.clone();
    late[4].tick += 1;
    assert!(matches!(
        evaluate_soak(&plan, bound, &late),
        Err(SoakError::SampleTick { cycle: 4, .. })
    ));
}

#[test]
fn accept_f60_a_budgets_are_hardware_specific_and_unset_until_measured() {
    let mac = HardwareProfile::designed_1080p(Platform::MacosAppleSilicon);
    let scenario = scenario_for(ScenarioKind::ManyProjectiles, mac.clone());
    assert_eq!(
        scenario.budget.frame_p95_us,
        BudgetLimit::Designed(FRAME_BUDGET_US_60FPS)
    );
    assert_eq!(scenario.budget.peak_memory_bytes, BudgetLimit::Unset);

    // Within the frame budget, but unset limits keep the run from passing.
    let report = scenario
        .evaluate(&run_for(&mac, FRAME_BUDGET_US_60FPS))
        .unwrap();
    assert_eq!(report.frame_p95, Verdict::Pass);
    assert_eq!(report.sim_tick_p99, Verdict::Pass);
    assert_eq!(report.peak_memory, Verdict::Unevaluated);
    assert!(!report.has_failure() && !report.is_complete_pass());

    let slow = scenario
        .evaluate(&run_for(&mac, FRAME_BUDGET_US_60FPS + 1))
        .unwrap();
    assert_eq!(
        slow.frame_p95,
        Verdict::Fail {
            limit: FRAME_BUDGET_US_60FPS,
            measured: FRAME_BUDGET_US_60FPS + 1
        }
    );
    assert!(slow.has_failure());

    // Loads are not frame-timed; a measured limit is judged.
    let cold = scenario_for(ScenarioKind::ColdLoad, mac.clone());
    assert_eq!(cold.budget.frame_p95_us, BudgetLimit::Unset);
}

#[test]
fn accept_f60_a_runs_on_other_hardware_or_without_required_systems_are_refused() {
    let mac = HardwareProfile::designed_1080p(Platform::MacosAppleSilicon);
    let win = HardwareProfile::designed_1080p(Platform::WindowsX86_64);
    let scenario = scenario_for(ScenarioKind::WorstCaseCampaignBattle, mac.clone());
    assert!(matches!(
        scenario.evaluate(&run_for(&win, 1)),
        Err(ScenarioError::ProfileMismatch { .. })
    ));
    let mut reduced = run_for(&mac, 1);
    reduced.quality = Quality::Reduced;
    assert!(matches!(
        scenario.evaluate(&reduced),
        Err(ScenarioError::ProfileMismatch { .. })
    ));
    // Dropping collision, AI, audio or mission content to pass is refused.
    for off in SystemKind::ALWAYS {
        let mut run = run_for(&mac, 1);
        run.enabled_systems.retain(|s| *s != off);
        assert_eq!(
            scenario.evaluate(&run),
            Err(ScenarioError::SystemDisabled(off))
        );
    }
    // Multiplayer load additionally needs networking.
    let mp = scenario_for(ScenarioKind::FullMultiplayerLoad, mac.clone());
    assert_eq!(
        mp.evaluate(&run_for(&mac, 1)),
        Err(ScenarioError::SystemDisabled(SystemKind::Networking))
    );
}

#[test]
fn accept_f60_a_percentiles_use_nearest_rank() {
    assert_eq!(percentiles(&[]), None);
    let samples: Vec<u64> = (1..=100).rev().collect();
    let p = percentiles(&samples).unwrap();
    assert_eq!((p.p50, p.p95, p.p99), (50, 95, 99));
    assert_eq!(percentiles(&[7]).unwrap().p99, 7);
}
