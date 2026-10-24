//! Rally #1926: the owner-command smoke's woken-trucks stage
//! (`--smoke-trucks`). Task test prefix: `accept_trucks_`.
//!
//! The judging is pure, so these tests drive the same `evaluate_mission_smoke`
//! the windowed run ends with. The windowed half is the retail run recorded in
//! `docs/findings/2026-10-24-trucks-owner-command-smoke.md`.

use cs_app::cli::{self, CliRequest};
use cs_app::mission_session::{
    Check, MissionSmokeMode, SmokeMeasurements, TrucksMetrics, evaluate_mission_smoke,
};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn check<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("no check named {id}: {checks:#?}"))
}

fn full() -> TrucksMetrics {
    TrucksMetrics {
        route_frame: Some(500),
        stops: 12,
        woke_frame: Some(900),
        live: ["t_truck01", "t_truck02", "t_truck03"]
            .map(str::to_owned)
            .to_vec(),
        wake_emissions: 1,
        wake_names: vec!["t_truck*".to_owned()],
        truck_m: Some([100.0, 5.0, 200.0]),
        hold_frame: Some(900),
        cannon_orders: 14,
        shots: 14,
        shooters: vec!["t_truck01".to_owned()],
        start_hull: Some(100.0),
        min_hull: Some(80.0),
        fire_frame: Some(1_100),
        frames_delivered: true,
        awake_permille: 900,
        firing_permille: 900,
    }
}

fn measured(trucks: TrucksMetrics) -> SmokeMeasurements {
    SmokeMeasurements {
        trucks: Some(trucks),
        ..SmokeMeasurements::default()
    }
}

#[test]
fn accept_trucks_flag_parses_to_the_mode() {
    let line = |extra: &[&str]| {
        let mut line = args(&[
            "--cs-path",
            "/retail",
            "--mission",
            "M01",
            "--smoke-seconds",
            "60",
            "--smoke-trucks",
        ]);
        line.extend(extra.iter().map(|arg| (*arg).to_owned()));
        line
    };
    match cli::parse(line(&["--capture-dir", "/out"])) {
        CliRequest::Mission(mission) => {
            assert!(mission.smoke_trucks);
            assert!(mission.smoke.is_some());
        }
        other => panic!("{other:?}"),
    }
    let _ = MissionSmokeMode::Trucks;
    for other in [
        "--smoke-no-teleport",
        "--smoke-engagement",
        "--smoke-failure",
        "--smoke-crash",
        "--smoke-start-flow",
    ] {
        assert!(
            matches!(cli::parse(line(&[other])), CliRequest::Invalid { .. }),
            "--smoke-trucks combines with no other mode, not {other}"
        );
    }
}

#[test]
fn accept_trucks_empty_stage_fails_every_check() {
    let checks = evaluate_mission_smoke(&measured(TrucksMetrics::default()));
    for id in [
        "trucks_woken_by_script",
        "trucks_fire_orders",
        "trucks_hull_drops",
        "trucks_frames_drawn",
    ] {
        assert!(
            !check(&checks, id).passed,
            "{id} must fail on the empty stage"
        );
    }
}

#[test]
fn accept_trucks_whole_sequence_passes() {
    let checks = evaluate_mission_smoke(&measured(full()));
    for check in &checks {
        assert!(check.passed, "{} failed: {}", check.id, check.detail);
    }
}

#[test]
fn accept_trucks_each_missing_piece_fails_its_own_check() {
    type Mutate = fn(&mut TrucksMetrics);
    let cases: [(&str, Mutate); 6] = [
        ("trucks_woken_by_script", |t| t.wake_emissions = 0),
        ("trucks_woken_by_script", |t| {
            t.live.pop().map(drop).unwrap_or(())
        }),
        ("trucks_fire_orders", |t| t.cannon_orders = 0),
        ("trucks_fire_orders", |t| {
            t.shooters = vec!["aagun01".to_owned()]
        }),
        ("trucks_hull_drops", |t| t.min_hull = t.start_hull),
        ("trucks_frames_drawn", |t| t.firing_permille = 10),
    ];
    for (id, mutate) in cases {
        let mut trucks = full();
        mutate(&mut trucks);
        let checks = evaluate_mission_smoke(&measured(trucks));
        assert!(!check(&checks, id).passed, "{id} must fail");
    }
}
