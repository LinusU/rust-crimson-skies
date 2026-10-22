//! Acceptance for Rally #1881 `FLAK-B2-TURRET-UPDATE`: the turret update law
//! of `docs/findings/2026-10-21-flak-a-turret-update-law.md` over the FLAK-B1
//! entries. Synthetic legs run in CI; the retail leg reads the real `t_truck`
//! record and node positions of M01. Nothing is `verified_original`.

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_turret_update::{
    ALIGNED_COS, AttackState, MountPose, SightAnswer, TargetCandidate, TargetKind, TurretConfig,
    TurretFrame, TurretSystem, perturb, scope_turret_specs,
};
use cs_app::mission_turrets::{TurretEntries, TurretEntry, TurretMount, TurretSpec};
use cs_content::campaign_bindings::MissionLabel;

const HZ: u32 = 60;

fn config() -> TurretConfig {
    TurretConfig {
        team: 2,
        detection_range_m: 500.0,
        shoot_up_only: false,
        fire_rate_s: (0.5, 0.5),
        fire_limits_s: (0.0, 0.0),
        inaccuracy_rad: 0.0,
        attack_s: (1000.0, 1000.0),
        bored_s: (1000.0, 1000.0),
        pitch_rad: (0.0, 0.0),
        yaw_rad: (0.0, 0.0),
        effect_s: None,
        has_weapon: true,
        firepoints_m: vec![[0.0, 1.0, 0.0], [0.0, 2.0, 0.0]],
    }
}

fn entry(name: &str, live: bool) -> TurretEntry {
    TurretEntry {
        name: name.to_owned(),
        chain: vec![name.to_owned()],
        mount: TurretMount::World,
        record: 0,
        position_m: Some([0.0; 3]),
        team: 2,
        live,
    }
}

fn mount() -> MountPose {
    MountPose {
        position_m: [0.0; 3],
        yaw_rad: 0.0,
        active: true,
    }
}

fn target(id: u64, kind: TargetKind, team: u32, at: [f64; 3]) -> TargetCandidate {
    TargetCandidate {
        id,
        kind,
        team,
        position_m: at,
        is_player: false,
        vehicle_class4: false,
        accepts: true,
    }
}

fn player(at: [f64; 3]) -> TargetCandidate {
    TargetCandidate {
        is_player: true,
        ..target(1, TargetKind::Vehicle, 1, at)
    }
}

/// A system of one turret already attacking (timers pushed far out).
fn system(config: TurretConfig, live: bool) -> TurretSystem {
    TurretSystem::new(7, HZ, 0, vec![(entry("gun", live), config)])
}

fn frame<'a>(tick: u64, candidates: &'a [TargetCandidate]) -> TurretFrame<'a> {
    TurretFrame {
        tick,
        candidates,
        player_team: 1,
        include_structs_flag: false,
        player_flag: false,
        freeze_timers: false,
    }
}

fn clear(_: [f64; 3], _: [f64; 3]) -> SightAnswer {
    SightAnswer::Clear
}

/// Runs `ticks` frames and returns the shot ticks.
fn run(
    system: &mut TurretSystem,
    candidates: &[TargetCandidate],
    ticks: u64,
    sight: &mut dyn FnMut([f64; 3], [f64; 3]) -> SightAnswer,
) -> Vec<u64> {
    let mut shots = Vec::new();
    for tick in 1..=ticks {
        for intent in system.update(&frame(tick, candidates), &[mount()], sight) {
            shots.push(intent.tick);
        }
    }
    shots
}

fn attacking(cfg: TurretConfig) -> TurretSystem {
    let mut sys = system(cfg, true);
    sys.restore_attack_state(0, AttackState::Attacking, u64::MAX);
    sys
}

#[test]
fn accept_flak_b2_gates_latch_live_byte_and_weapon_stop_the_update() {
    let candidates = [player([0.0, 0.0, 100.0])];
    let mut asleep = {
        let mut cfg = config();
        cfg.attack_s = (0.0, 0.0);
        system(cfg, false)
    };
    assert!(run(&mut asleep, &candidates, 600, &mut clear).is_empty());
    assert_eq!(asleep.turrets()[0].target(), None);

    let mut weaponless = {
        let mut cfg = config();
        cfg.has_weapon = false;
        system(cfg, true)
    };
    assert!(run(&mut weaponless, &candidates, 600, &mut clear).is_empty());

    // A live turret fires; waking the asleep one makes it fire too.
    let mut woken = attacking(config());
    assert!(!run(&mut woken, &candidates, 600, &mut clear).is_empty());
    asleep.set_live(0, true);
    asleep.update(&frame(700, &[]), &[mount()], &mut clear);
    asleep.update(&frame(701, &[]), &[mount()], &mut clear);

    // A dropped latch releases the target and stops the shots.
    let mut sys = attacking(config());
    sys.update(&frame(10, &candidates), &[mount()], &mut clear);
    assert_eq!(sys.turrets()[0].target(), Some(1));
    let mut off = mount();
    off.active = false;
    sys.update(&frame(11, &candidates), &[off], &mut clear);
    assert_eq!(sys.turrets()[0].target(), None);

    // The two global skip flags.
    let mut flagged = attacking(config());
    let mut skip = frame(20, &candidates);
    skip.player_flag = true;
    assert!(flagged.update(&skip, &[mount()], &mut clear).is_empty());
    assert_eq!(flagged.turrets()[0].target(), None);
}

#[test]
fn accept_flak_b2_hostile_means_a_different_non_zero_team_in_range() {
    let mut sys = attacking(config());
    let near = |team| target(5, TargetKind::Vehicle, team, [0.0, 0.0, 100.0]);
    for (team, hostile) in [(1, true), (3, true), (2, false), (0, false)] {
        sys.update(&frame(10, &[near(team)]), &[mount()], &mut clear);
        assert_eq!(
            sys.turrets()[0].target().is_some(),
            hostile,
            "team {team} against team 2"
        );
    }
    // DETECTION_RANGE is a hard edge; SHOOT_UP_ONLY rejects lower targets.
    let far = target(6, TargetKind::Vehicle, 1, [0.0, 0.0, 500.5]);
    sys.update(&frame(11, &[far]), &[mount()], &mut clear);
    assert_eq!(sys.turrets()[0].target(), None);
    let mut up_only = config();
    up_only.shoot_up_only = true;
    let mut sys = attacking(up_only);
    let low = target(7, TargetKind::Vehicle, 1, [0.0, -5.0, 100.0]);
    sys.update(&frame(10, &[low]), &[mount()], &mut clear);
    assert_eq!(sys.turrets()[0].target(), None);
}

#[test]
fn accept_flak_b2_scorer_weights_player_penalises_turrets_and_prefers_earlier_lists() {
    let mut sys = attacking(config());
    // The player at 150 m (score 960+150) beats an ordinary vehicle at 100 m
    // (1200+100).
    let candidates = [
        target(10, TargetKind::Vehicle, 3, [0.0, 0.0, 100.0]),
        player([0.0, 0.0, 150.0]),
    ];
    sys.update(&frame(10, &candidates), &[mount()], &mut clear);
    assert_eq!(sys.turrets()[0].target(), Some(1));
    // A class-4 vehicle weighs 1.4: the plain vehicle further away wins.
    let mut heavy = target(11, TargetKind::Vehicle, 3, [0.0, 0.0, 50.0]);
    heavy.vehicle_class4 = true;
    let plain = target(12, TargetKind::Vehicle, 3, [0.0, 0.0, 300.0]);
    sys.update(&frame(11, &[heavy, plain]), &[mount()], &mut clear);
    assert_eq!(sys.turrets()[0].target(), Some(12));
    // A turret candidate (earlier list than vehicles) wins outright, even
    // with the 37.5 m penalty and a far better vehicle.
    let turret = target(13, TargetKind::Turret, 3, [0.0, 0.0, 400.0]);
    let vehicle = target(14, TargetKind::Vehicle, 3, [0.0, 0.0, 10.0]);
    sys.update(&frame(12, &[vehicle, turret]), &[mount()], &mut clear);
    assert_eq!(sys.turrets()[0].target(), Some(13));
    // Within one list the 37.5 m penalty orders two turrets.
    let a = target(20, TargetKind::Turret, 3, [0.0, 0.0, 100.0]);
    let b = target(21, TargetKind::Turret, 3, [0.0, 0.0, 120.0]);
    sys.update(&frame(13, &[b, a]), &[mount()], &mut clear);
    assert_eq!(sys.turrets()[0].target(), Some(20));
    // Structs are scanned only for another team's player or with the flag.
    let structure = target(30, TargetKind::Struct, 3, [0.0, 0.0, 80.0]);
    let mut same_team_as_player = frame(14, std::slice::from_ref(&structure));
    same_team_as_player.player_team = 2;
    sys.update(&same_team_as_player, &[mount()], &mut clear);
    assert_eq!(sys.turrets()[0].target(), None);
    sys.update(&frame(15, &[structure]), &[mount()], &mut clear);
    assert_eq!(sys.turrets()[0].target(), Some(30));
}

#[test]
fn accept_flak_b2_the_player_ray_gates_engagement_and_unavailable_is_not_clear() {
    let candidates = [player([0.0, 0.0, 100.0])];
    let mut blocked = attacking(config());
    let shots = run(&mut blocked, &candidates, 600, &mut |_, _| {
        SightAnswer::Blocked
    });
    assert!(shots.is_empty());
    assert!(!blocked.turrets()[0].engaged());

    let mut unknown = attacking(config());
    let shots = run(&mut unknown, &candidates, 600, &mut |_, _| {
        SightAnswer::Unavailable
    });
    assert!(shots.is_empty(), "an unknown ray never counts as clear");
    assert!(unknown.turrets()[0].sight_unavailable() > 0);

    // The ray runs at 1.0 + rand01 s intervals, to the player at y + 0.2.
    let mut sys = attacking(config());
    let mut asks = Vec::new();
    for tick in 10..10 + 6 * u64::from(HZ) {
        sys.update(&frame(tick, &candidates), &[mount()], &mut |from, to| {
            asks.push((tick, from, to));
            SightAnswer::Clear
        });
    }
    assert!(asks.len() >= 3 && asks.len() <= 6, "{}", asks.len());
    for pair in asks.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        assert!((u64::from(HZ)..=2 * u64::from(HZ)).contains(&gap), "{gap}");
    }
    assert!((asks[0].2[1] - 0.2).abs() < 1e-9);
    // Only the player is ray-tested.
    let mut other = attacking(config());
    let mut asked = false;
    other.update(
        &frame(10, &[target(9, TargetKind::Vehicle, 3, [0.0, 0.0, 50.0])]),
        &[mount()],
        &mut |_, _| {
            asked = true;
            SightAnswer::Clear
        },
    );
    assert!(!asked);
}

#[test]
fn accept_flak_b2_limits_apply_only_to_a_differing_pair_and_alignment_is_fifteen_degrees() {
    // Target 40 degrees above the horizon; free pitch aligns and fires.
    let high = [target(5, TargetKind::Vehicle, 3, [0.0, 83.9, 100.0])];
    let mut free = attacking(config());
    assert!(!run(&mut free, &high, 300, &mut clear).is_empty());
    // Equal limits (the default 0/0 pair) are free; a 0..20 degree pair clamps
    // and leaves the barrel 20 degrees off: not aligned, no shot.
    let mut limited_cfg = config();
    limited_cfg.pitch_rad = (0.0, 20f64.to_radians());
    let mut limited = attacking(limited_cfg);
    assert!(run(&mut limited, &high, 300, &mut clear).is_empty());
    assert!(!limited.turrets()[0].engaged());
    assert!((limited.turrets()[0].angles().1 - 20f64.to_radians()).abs() < 1e-9);
    // A 0..30 degree pair leaves 10 degrees: inside the 15-degree cone.
    let mut wide_cfg = config();
    wide_cfg.pitch_rad = (0.0, 30f64.to_radians());
    let mut wide = attacking(wide_cfg);
    assert!(!run(&mut wide, &high, 300, &mut clear).is_empty());
    assert!((ALIGNED_COS - 15f64.to_radians().cos()).abs() < 1e-6);
    // Yaw wraps: a target behind a ±90 degree arch is out of reach.
    let mut yaw_cfg = config();
    yaw_cfg.yaw_rad = ((-90f64).to_radians(), 90f64.to_radians());
    let mut yawed = attacking(yaw_cfg);
    let behind = [target(6, TargetKind::Vehicle, 3, [0.0, 0.0, -100.0])];
    assert!(run(&mut yawed, &behind, 300, &mut clear).is_empty());
}

#[test]
fn accept_flak_b2_attack_and_bored_timers_alternate_and_bored_never_fires() {
    let mut cfg = config();
    cfg.attack_s = (2.0, 2.0);
    cfg.bored_s = (3.0, 3.0);
    cfg.fire_rate_s = (0.1, 0.1);
    let mut sys = system(cfg, true);
    assert_eq!(sys.turrets()[0].state(), AttackState::Bored);
    let candidates = [target(5, TargetKind::Vehicle, 3, [0.0, 0.0, 100.0])];
    let mut shots = Vec::new();
    let mut states = Vec::new();
    for tick in 1..=20 * u64::from(HZ) {
        let before = sys.turrets()[0].state();
        for intent in sys.update(&frame(tick, &candidates), &[mount()], &mut clear) {
            shots.push(intent.tick);
            assert_eq!(sys.turrets()[0].state(), AttackState::Attacking);
        }
        let after = sys.turrets()[0].state();
        if before != after {
            states.push((tick, after));
        }
    }
    assert!(states.len() >= 4, "{states:?}");
    assert!(!shots.is_empty());
    // Attack windows are ~2 s, bored ~3 s.
    for pair in states.windows(2) {
        let gap = (pair[1].0 - pair[0].0) as f64 / f64::from(HZ);
        let want = if pair[0].1 == AttackState::Attacking {
            2.0
        } else {
            3.0
        };
        assert!((gap - want).abs() < 0.1, "{gap} vs {want}");
    }
    // The freeze flag stops the timer step only.
    let mut frozen = system(
        {
            let mut cfg = config();
            cfg.attack_s = (0.0, 0.0);
            cfg
        },
        true,
    );
    let mut f = frame(500, &[]);
    f.freeze_timers = true;
    frozen.update(&f, &[mount()], &mut clear);
    assert_eq!(frozen.turrets()[0].state(), AttackState::Bored);
}

#[test]
fn accept_flak_b2_fire_rate_limits_effect_windup_firepoints_and_inaccuracy() {
    let candidates = [target(5, TargetKind::Vehicle, 3, [0.0, 0.0, 100.0])];
    // FIRE_RATE spacing, round-robin firepoints from the mount.
    let mut sys = attacking(config());
    let mut all = Vec::new();
    for tick in 4..=4 + 6 * u64::from(HZ) {
        all.extend(sys.update(&frame(tick, &candidates), &[mount()], &mut clear));
    }
    assert!(all.len() >= 5, "{}", all.len());
    for pair in all.windows(2) {
        assert_eq!(pair[1].tick - pair[0].tick, u64::from(HZ) / 2);
        assert_ne!(pair[0].firepoint, pair[1].firepoint);
    }
    assert_eq!(all[0].muzzle_m[1], [1.0, 2.0][all[0].firepoint]);
    let d = all[0].direction;
    assert!(d[2] > 0.99 && (d.iter().map(|c| c * c).sum::<f64>() - 1.0).abs() < 1e-9);

    // FIRE_LIMITS (1 s burst, 2 s reload) with FIRE_RATE ignored.
    let mut cfg = config();
    cfg.fire_limits_s = (1.0, 2.0);
    let mut limited = attacking(cfg);
    let mut ticks = Vec::new();
    for tick in 4..=4 + 8 * u64::from(HZ) {
        for i in limited.update(&frame(tick, &candidates), &[mount()], &mut clear) {
            ticks.push(i.tick);
        }
    }
    let gaps: Vec<u64> = ticks.windows(2).map(|p| p[1] - p[0]).collect();
    assert!(gaps.iter().any(|g| *g >= 2 * u64::from(HZ)), "{gaps:?}");
    assert!(gaps.iter().filter(|g| **g == 1).count() > 20, "{gaps:?}");

    // EFFECT: the first due pass fires nothing and delays by its duration.
    let mut cfg = config();
    cfg.effect_s = Some(0.5);
    let mut windup = attacking(cfg);
    let mut shots = Vec::new();
    for tick in 4..=4 + 4 * u64::from(HZ) {
        shots.extend(windup.update(&frame(tick, &candidates), &[mount()], &mut clear));
    }
    assert!(!shots.is_empty());
    assert!(shots[0].tick >= 4 + u64::from(HZ) / 2);

    // INACCURACY stays inside its cone and differs shot to shot.
    let mut cfg = config();
    cfg.inaccuracy_rad = 0.1;
    let mut loose = attacking(cfg);
    let mut dirs = Vec::new();
    for tick in 4..=4 + 8 * u64::from(HZ) {
        dirs.extend(loose.update(&frame(tick, &candidates), &[mount()], &mut clear));
    }
    assert!(dirs.len() >= 5);
    assert!(dirs.windows(2).any(|p| p[0].direction != p[1].direction));
    for shot in &dirs {
        let to: [f64; 3] = [
            0.0 - shot.muzzle_m[0],
            -shot.muzzle_m[1],
            100.0 - shot.muzzle_m[2],
        ];
        let length = to.iter().map(|c| c * c).sum::<f64>().sqrt();
        let cos: f64 = (0..3).map(|i| shot.direction[i] * to[i] / length).sum();
        assert!(cos >= 0.1f64.cos() - 1e-9, "{cos}");
    }
    let tilt = perturb([0.0, 0.0, 1.0], 0.1, 0.25, 1.0);
    assert!((tilt[2] - 0.1f64.cos()).abs() < 1e-12);
}

#[test]
fn accept_flak_b2_the_same_seed_gives_the_same_shots() {
    let candidates = [player([0.0, 0.0, 100.0])];
    let shots = |seed: u64| {
        let mut cfg = config();
        cfg.attack_s = (1.0, 2.0);
        cfg.bored_s = (1.0, 2.0);
        cfg.fire_rate_s = (0.2, 0.9);
        cfg.inaccuracy_rad = 0.05;
        let mut sys = TurretSystem::new(seed, HZ, 0, vec![(entry("gun", true), cfg)]);
        let mut out = Vec::new();
        for tick in 1..=30 * u64::from(HZ) {
            out.extend(sys.update(&frame(tick, &candidates), &[mount()], &mut clear));
        }
        out
    };
    assert_eq!(shots(1), shots(1));
    assert_ne!(shots(1), shots(2));
    assert!(!shots(1).is_empty());
}

#[test]
fn accept_flak_b2_absent_fields_take_the_constructor_defaults_and_bad_shapes_are_refused() {
    let mut spec = TurretSpec {
        index: 0,
        nodes: vec![vec!["gun*".to_owned()]],
        activated: None,
        team: None,
        detection_range_m: None,
        shoot_up_only: None,
        fire_rate: None,
        fire_limits: None,
        inaccuracy: None,
        attack_interval: Some(vec![2.0]),
        bored_interval: None,
        pitch: None,
        yaw: None,
        effect: None,
        ammo: None,
        weapon: Some("wep_x".to_owned()),
        other_keys: Vec::new(),
    };
    let config = TurretConfig::from_spec(&spec, None, vec![[0.0; 3]]).unwrap();
    assert_eq!(config.fire_rate_s, (1.0, 1.0));
    assert_eq!(config.bored_s, (0.0, 0.0));
    assert_eq!(config.attack_s, (2.0, 2.0), "a single value duplicates");
    assert_eq!(config.team, 2);
    assert_eq!(config.detection_range_m, 200.0);
    spec.attack_interval = None;
    let config = TurretConfig::from_spec(&spec, None, Vec::new()).unwrap();
    assert_eq!(config.attack_s, (10.0, 10.0));
    spec.fire_rate = Some(vec![1.0, 2.0, 3.0]);
    spec.effect = Some("flash".to_owned());
    let refusals = TurretConfig::from_spec(&spec, None, vec![[0.0; 3]]).unwrap_err();
    let text: Vec<_> = refusals.iter().map(|r| r.0.as_str()).collect();
    assert!(text.iter().any(|t| t.starts_with("FIRE_RATE")), "{text:?}");
    assert!(text.iter().any(|t| t.starts_with("EFFECT")));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flak_b2_retail_the_t_truck_record_drives_the_update_at_its_real_position() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set for the retail leg"),
    );
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M01").expect("a mission label"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let specs = scope_turret_specs(&root, &plan.mission_dir, &plan.group_dir);
    let entries = TurretEntries::read(&root, &plan.group_dir, &specs, &[]);
    let truck = entries.entry("t_truck01").expect("t_truck01 is bound");
    let spec = specs
        .iter()
        .find(|spec| spec.index == truck.record)
        .expect("the record that built it");
    eprintln!("t_truck record: {spec:?}");
    let position = truck.position_m.expect("a composed position");
    // The record's own tuning; the wind-up duration and firepoint offsets are
    // not recovered, so the test supplies a single firepoint at the mount.
    let config = TurretConfig::from_spec(spec, spec.effect.as_ref().map(|_| 0.5), vec![[0.0; 3]])
        .unwrap_or_else(|refusals| panic!("the t_truck record is refused: {refusals:?}"));
    let mut woken = truck.clone();
    woken.live = true;
    let mut sys = TurretSystem::new(1, HZ, 0, vec![(woken, config.clone())]);
    let mount = MountPose {
        position_m: position,
        yaw_rad: 0.0,
        active: true,
    };
    // The player (team 1) sits inside DETECTION_RANGE, level with the truck;
    // free limits make the barrel follow, so it fires once attacking.
    let range = config.detection_range_m;
    let at = |d: f64| [position[0] + d, position[1], position[2]];
    let inside = [player(at(range * 0.5))];
    let outside = [player(at(range * 1.5))];
    let mut shots_in = Vec::new();
    let mut shots_out = Vec::new();
    let mut outside_sys = sys.clone();
    let horizon = 120 * u64::from(HZ);
    for tick in 1..=horizon {
        shots_in.extend(sys.update(&frame(tick, &inside), &[mount], &mut clear));
        shots_out.extend(outside_sys.update(&frame(tick, &outside), &[mount], &mut clear));
    }
    assert!(shots_out.is_empty(), "no shot beyond DETECTION_RANGE");
    assert_eq!(outside_sys.turrets()[0].target(), None);
    assert!(
        !shots_in.is_empty(),
        "the record's timers let it attack within {horizon} ticks"
    );
    for shot in &shots_in {
        assert_eq!(shot.name, "t_truck01");
        assert_eq!(shot.muzzle_m, position);
        assert!(shot.direction[0] > 0.9, "{:?}", shot.direction);
    }
}
