//! Acceptance for Rally #1821 `FLAK-C-FLAK-ROUND-AND-EFFECT` — the split's
//! integration step: the `wep_27` flak rocket a woken turret fires (FLAK-C2),
//! the `flak_effect` its burst presents (FLAK-C3) and the `snd_enemyflak` /
//! `snd_missile_flak` cues it sounds (FLAK-C4), joined end to end through the
//! production turret step and shown in the owner-command smoke (FLAK-C5).
//!
//! Test prefix: `accept_flak_c_`. The synthetic tests run in CI and drive the
//! real `OrdnanceBurst` of a production `FlakRound` into both production
//! consumers. The ignored tests need `retail` (the headless chain on real M01)
//! and `retail,gpu,audio` (the owner's own windowed command) — nothing here is
//! `verified_original`: the wake and the placement are labelled test ones.

use std::path::PathBuf;
use std::time::Duration;

use avian3d::prelude::{LinearVelocity, Position};
use bevy::prelude::{App, Entity, FixedPostUpdate, GlobalTransform, Time, Vec3, With};
use bevy::time::TimeUpdateStrategy;
use cs_app::audio::{AudioSession, AudioSpatial};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::turret_fire::{MissionTurretFire, TurretWeaponRecord};
use cs_app::mission_session::{
    FlakBurstFxPlugin, FlakBurstFxReport, FlakEffectDefinition, FlakEffectResource, FlakFlash,
    FlakFlicker, FlakRecord, FlakRound, FlakSound, FlakSoundSequences, FlakStep, FlakVoice,
    FlashStep, FlickerStep, MissionHost, MissionPlayerBody, PendingFlakBursts,
    PendingOrdnanceBursts, SMOKE_REPORT_FILE, build_headless, play_flak_burst, play_flak_shot,
    stage_for, teardown,
};
use cs_app::mission_turrets::TurretMount;
use cs_app::physics::PhysicsTickLedger;
use cs_app::scene::SceneGeneration;
use cs_content::campaign_bindings::MissionLabel;
use cs_content::catalog::baseline::install_file_key;
use cs_content::stunts::ZrdValue;
use cs_sim::audio_events::{
    AudioAssetSpec, AudioBus, AudioRouter, Listener, PlaybackMode, SpatialPolicy,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::net::SessionId;

// ------------------------------------------------- the shared fixtures ---

/// The retail `wep_27` numbers as a `weapons.zrd` document body, so the
/// record's own reader produces the [`FlakRecord`] the tests fly.
fn weapons_document(extra: &[&str]) -> ZrdValue {
    let text = |s: &str| ZrdValue::Text(s.to_owned());
    let mut body = Vec::new();
    for (key, value) in [
        ("VELOCITY", 850.0),
        ("RANGE", 900.0),
        ("DETONATION_DISTANCE", 20.0),
        ("IMPACT_PROXIMITY", 22.0),
        ("ARMOR_DAMAGE", 10.0),
        ("HEALTH_DAMAGE", 10.0),
    ] {
        body.extend([text(key), ZrdValue::List(vec![ZrdValue::Float(value)])]);
    }
    for key in ["LOCK_ON", "ROCKET"].iter().chain(extra.iter()) {
        body.extend([text(key), ZrdValue::List(Vec::new())]);
    }
    ZrdValue::List(vec![ZrdValue::List(vec![
        text("wep_27"),
        ZrdValue::List(body),
    ])])
}

fn record(extra: &[&str]) -> FlakRecord {
    FlakRecord::from_document(&weapons_document(extra)).expect("the record reads")
}

/// The stored `flak_effect`'s shape with authored numbers.
fn effect() -> FlakEffectDefinition {
    FlakEffectDefinition {
        light_range: [4.0, 10.0],
        light_color: [0.95, 0.76, 0.24],
        flash_steps: vec![
            FlashStep {
                delta_range: [16.0, 40.0],
                run_time_s: 0.3,
            },
            FlashStep {
                delta_range: [-14.0, -34.0],
                run_time_s: 0.1,
            },
        ],
        player_range_m: 100.0,
        flicker_steps: vec![FlickerStep {
            from: [1.0, 1.0, 1.0, 0.3],
            to: [0.0, 0.0, 0.0, 0.4],
            run_time_s: 0.2,
        }],
    }
}

/// A composition holding the presentation plugin, its measured resource and
/// a player body at `player`.
fn fx_app(player: Vec3) -> App {
    let mut app = App::new();
    app.add_plugins(FlakBurstFxPlugin);
    app.insert_resource(PhysicsTickLedger {
        timestep_s: 1.0 / 120.0,
        ..Default::default()
    });
    app.insert_resource(FlakEffectResource(effect()));
    app.world_mut()
        .spawn((MissionPlayerBody, GlobalTransform::from_translation(player)));
    app
}

fn id(member: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::Sound,
        &install_file_key(&format!("ZBD/soundsh.zbd/{member}")),
    )
    .expect("a valid sound id")
}

/// The two flak voices `wep_27`'s rows name, as `FlakSound` carries them.
fn sound() -> FlakSound {
    let voice = |group: &str, member: &str| FlakVoice {
        group: group.to_owned(),
        member: member.to_owned(),
        range_m: Some([800.0, 2400.0]),
        content: id(member),
    };
    FlakSound {
        shot: voice("snd_enemyflak", "enemy_flak.wav"),
        burst: voice("snd_missile_flak", "missile_flak.wav"),
        container: "ZBD/soundsh.zbd".to_owned(),
        container_sha256: ContentHash::from_hex(&"0".repeat(64)).expect("a hash"),
        host: PathBuf::new(),
        bytes: 1,
    }
}

fn audio_session(delivered: &[&ContentId]) -> AudioSession {
    let specs: Vec<_> = delivered
        .iter()
        .map(|content| {
            AudioAssetSpec::try_new(
                (*content).clone(),
                AudioBus::Weapons,
                1.0,
                PlaybackMode::OneShot,
            )
            .expect("a valid one-shot spec")
        })
        .collect();
    AudioSession::new(
        AudioRouter::new(SessionId::new(7).expect("a session")),
        SceneGeneration::default(),
        specs,
    )
}

// -------------------------------------------------- the integration leg ---

/// **One `wep_27` round fired at a held target bursts within
/// `DETONATION_DISTANCE` of it, and that one burst feeds both consumers the
/// split built: the `flak_effect` presentation draws the flash, the smoke
/// ball and the close-range flicker, while `snd_missile_flak` plays at the
/// burst point — after `snd_enemyflak` played at the muzzle.**
#[test]
fn accept_flak_c_a_fired_rounds_burst_feeds_the_presentation_and_the_cues() {
    let muzzle = [0.0, 300.0, 0.0];
    let player = [300.0, 300.0, 0.0];
    let mut round =
        FlakRound::fire(&record(&[]), muzzle, [1.0, 0.0, 0.0]).expect("the round fires");

    // The muzzle shot's cue plays when the round leaves, through the same
    // session the burst cue will use.
    let sound = sound();
    let mut audio = audio_session(&[&sound.shot.content, &sound.burst.content]);
    let mut sequences = FlakSoundSequences::default();
    let spatial = AudioSpatial::new(
        SpatialPolicy::try_new(1.0, 10_000.0).expect("a valid policy"),
        Listener::try_new(player, [1.0, 0.0, 0.0]).expect("a valid listener"),
    );
    let shot = play_flak_shot(
        &sound,
        Tick(1),
        muzzle,
        &mut audio,
        &mut sequences,
        Some(&spatial),
    )
    .expect("snd_enemyflak plays at the muzzle");
    assert_eq!(shot.event.asset, sound.shot.content);
    assert_eq!(shot.origin_m, muzzle);

    // The round flies the production step until it bursts on the held
    // target: the same `OrdnanceBurst` `step_mission_turrets` publishes.
    let mut burst = None;
    for tick in 2..10_000 {
        if let FlakStep::Burst(found) = round.step(Tick(tick), 1.0 / 120.0, Some(player)) {
            burst = Some(found);
            break;
        }
    }
    let burst = burst.expect("the round bursts on the held target");
    let gap: f64 = burst
        .position_m
        .iter()
        .zip(player)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f64>()
        .sqrt();
    assert!(
        gap <= 20.0,
        "the burst is inside DETONATION_DISTANCE: {gap}"
    );

    // Consumer one: the `flak_effect` presentation.
    let mut app = fx_app(Vec3::from_array(player.map(|c| c as f32)));
    app.world_mut()
        .resource_mut::<PendingFlakBursts>()
        .0
        .push(burst);
    app.world_mut().run_schedule(FixedPostUpdate);
    let report = *app.world().resource::<FlakBurstFxReport>();
    assert_eq!(report.bursts_presented, 1);
    assert_eq!(report.flashes_spawned, 1, "the orange light flash");
    assert_eq!(report.smoke_balls_requested, 1, "the large_black_smokeball");
    assert_eq!(report.flickers_started, 1, "the burst stood inside 100 m");
    assert_eq!(
        report.trails_undrawn, 1,
        "the trails stay undrawn (unmeasured)"
    );
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<FlakFlash>>()
            .iter(app.world())
            .count(),
        1
    );
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<FlakFlicker>>()
            .iter(app.world())
            .count(),
        1
    );
    let puffs = &app.world().resource::<PendingOrdnanceBursts>().0;
    assert_eq!(puffs.len(), 1);
    assert_eq!(puffs[0].at, burst.at);

    // Consumer two: the burst's `snd_missile_flak` at the burst point.
    let cue = play_flak_burst(&sound, &burst, &mut audio, &mut sequences, Some(&spatial))
        .expect("snd_missile_flak plays at the burst");
    assert_eq!(cue.event.asset, sound.burst.content);
    assert_eq!(cue.event.id.tick, burst.at);
    assert_eq!(cue.origin_m, burst.position_m);
}

/// **A round whose record makes the range end silent (`LOCK_ON` + `EXPIRES`,
/// no `DETONATE_AT_RANGE`) never produces an `OrdnanceBurst`, so nothing is
/// published for the presentation or the audio path — an absent burst is
/// never faked.**
#[test]
fn accept_flak_c_a_silent_end_publishes_nothing() {
    let mut round =
        FlakRound::fire(&record(&["EXPIRES"]), [0.0; 3], [1.0, 0.0, 0.0]).expect("the round fires");
    let mut last = FlakStep::Flying;
    for tick in 0..10_000 {
        // No held target: the proximity test never applies, so the range-end
        // law alone decides.
        last = round.step(Tick(tick), 1.0 / 120.0, None);
        if last != FlakStep::Flying {
            break;
        }
    }
    assert_eq!(
        last,
        FlakStep::SilentEnd,
        "the record ends silently at RANGE"
    );
    assert!(round.is_finished());

    let mut app = fx_app(Vec3::ZERO);
    app.world_mut().run_schedule(FixedPostUpdate);
    let report = *app.world().resource::<FlakBurstFxReport>();
    assert_eq!(
        report,
        FlakBurstFxReport::default(),
        "no burst, nothing drawn"
    );
    assert!(app.world().resource::<PendingOrdnanceBursts>().0.is_empty());
}

// ------------------------------------------------------ the retail leg ---

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the retail install"))
}

fn pin_player(app: &mut App, at: [f64; 3]) {
    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    world.get_mut::<Position>(body).expect("a pose").0 =
        Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32);
    world.get_mut::<LinearVelocity>(body).expect("a velocity").0 = Vec3::ZERO;
}

/// **Retail M01, headless: the production turret chain end to end.** The
/// first world turret whose `WEAPON` reads as `wep_27` — `aagun99`, which no
/// M01 program wakes — stays silent while asleep with the player parked in
/// its field; after the test wake it fires the retail flak rocket, the round
/// bursts on the player, and the burst is presented (flash, smoke ball,
/// close-range flicker) and sounded (`snd_enemyflak` at the muzzle,
/// `snd_missile_flak` at the burst).
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_flak_c_retail_a_test_woken_aagun_fires_bursts_presents_and_sounds() {
    let root = game_dir();
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M01").expect("a mission label"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    assert!(
        stage.flak_effect.is_some(),
        "the stage measured flak_effect"
    );
    assert!(
        stage.flak_sound.is_some(),
        "the stage measured the flak sounds"
    );

    // The smoke's own selection: the first world entry whose `WEAPON` the
    // runtime classified as a rocket — `aagun99` in retail M01.
    let bound = &stage.host.turrets;
    let entry = bound
        .entries
        .iter()
        .find(|entry| {
            entry.mount == TurretMount::World
                && entry.position_m.is_some()
                && bound
                    .specs
                    .iter()
                    .find(|spec| spec.index == entry.record)
                    .and_then(|spec| spec.weapon.as_deref())
                    .and_then(|name| bound.weapons.get(name))
                    .is_some_and(|weapon| matches!(weapon, TurretWeaponRecord::Rocket(_)))
        })
        .expect("a world turret with a rocket record is bound")
        .clone();
    assert_eq!(entry.name, "aagun99", "the documented retail M01 flak gun");
    assert!(
        !entry.live,
        "aagun99 starts asleep: no M01 program wakes it"
    );
    let spec = bound
        .specs
        .iter()
        .find(|spec| spec.index == entry.record)
        .expect("its TURRET record is carried");
    let mount = entry.position_m.expect("a world entry has a position");

    // The smoke's stand-off: 0.6 of DETECTION_RANGE horizontally, 0.3 of that
    // vertically — inside detection, above the mount so `SHOOT_UP_ONLY` and
    // `PITCH` admit the approach. A test placement, not original behaviour.
    let horizontal = (spec.detection_range() * 0.6).clamp(80.0, 400.0);
    let at = [mount[0] + horizontal, mount[1] + horizontal * 0.3, mount[2]];

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let step = |app: &mut App| {
        pin_player(app, at);
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
    };

    // While the gun sleeps the whole chain stays silent: no `wep_27` shot,
    // no round, no burst, no presentation, no cue.
    for _ in 0..240 {
        step(&mut app);
    }
    let report = app.world().resource::<MissionTurretFire>().report().clone();
    assert_eq!(
        report.rockets_fired, 0,
        "asleep, it fires nothing: {report:?}"
    );
    assert!(
        report.shots.iter().all(|shot| shot.weapon != "wep_27"),
        "no wep_27 shot while asleep: {:?}",
        report.shots
    );
    assert!(report.rocket_bursts.is_empty());
    assert_eq!(
        app.world().resource::<FlakBurstFxReport>().bursts_presented,
        0
    );

    // The test wake — the ledger's own matcher, the same bookkeeping a
    // `WAKEUP_TURRETS` emission writes.
    let tick = app.world().resource::<PhysicsTickLedger>().ticks;
    app.world_mut()
        .resource_mut::<MissionHost>()
        .wake_turrets_mut()
        .wake_for_test("aagun99", Tick(tick));

    // The production law acquires the player, fires `wep_27`, the round
    // bursts on its held target and the burst is presented and sounded.
    let mut presented = false;
    for _ in 0..3600 {
        step(&mut app);
        if app.world().resource::<FlakBurstFxReport>().bursts_presented > 0 {
            presented = true;
            break;
        }
    }
    assert!(presented, "a burst was presented within the budget");
    let report = app.world().resource::<MissionTurretFire>().report().clone();
    assert!(report.rockets_fired >= 1, "the woken gun fired: {report:?}");
    let shot = report
        .shots
        .iter()
        .find(|shot| shot.weapon == "wep_27" && shot.name == "aagun99")
        .expect("aagun99's wep_27 shot is logged");
    assert_eq!(shot.weapon, "wep_27");
    let burst = report
        .rocket_bursts
        .first()
        .expect("a round burst on the held target");
    assert_eq!(burst.impact_row, 0, "the default IMPACT row (flak_effect)");
    let gap: f64 = burst
        .position_m
        .iter()
        .zip(at)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f64>()
        .sqrt();
    assert!(
        gap <= 21.0,
        "the burst stood within DETONATION_DISTANCE of the pinned player: {gap}"
    );

    let fx = app.world().resource::<FlakBurstFxReport>();
    assert!(fx.flashes_spawned >= 1, "the orange light flash spawned");
    assert!(
        fx.smoke_balls_requested >= 1,
        "the smoke ball was handed over"
    );
    assert!(
        fx.flickers_started >= 1,
        "the burst stood inside the stored 100 m gate"
    );
    assert!(
        fx.trails_thrown >= 1 && fx.trails_undrawn == 0,
        "the burst threw its call_flak_trails puff trails (FLAK-C3-TRAILS, #1925)"
    );
    assert!(
        report.flak_shot_cues >= 1 && report.flak_burst_cues >= 1,
        "snd_enemyflak and snd_missile_flak played: {report:?}"
    );
    assert!(
        report.flak_sound_refusals.is_empty(),
        "no flak cue was refused: {:?}",
        report.flak_sound_refusals
    );
    // Only the named gun woke: the rest of the island's `aagun*` stay asleep.
    let ledger = app.world().resource::<MissionHost>().wake_turrets().clone();
    for name in ["aagun01", "aagun02", "aagun30"] {
        assert!(
            !ledger.entry(name).expect("an entry").live,
            "{name} stays asleep: only aagun99 was test-woken"
        );
    }
    teardown(&mut app);
}

// ---------------------------------------------------- the windowed half ---

/// The windowed binary the ignored run drives: `CS_MISSION_BIN` when the
/// operator names one, else the release build, else this harness's own.
fn mission_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("CS_MISSION_BIN") {
        return PathBuf::from(bin);
    }
    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/release/cs");
    if release.is_file() {
        return release;
    }
    PathBuf::from(env!("CARGO_BIN_EXE_cs"))
}

/// The mean and peak 8-bit luminance of a captured framebuffer PNG — the
/// evidence that the burst/tail frames are the drawn scene and not black.
fn luminance(path: &std::path::Path) -> (u64, u8) {
    let image = image::ImageReader::open(path)
        .unwrap_or_else(|error| panic!("{} decodes: {error}", path.display()))
        .decode()
        .unwrap_or_else(|error| panic!("{} is a PNG: {error}", path.display()))
        .to_luma8();
    let mut sum = 0u64;
    let mut peak = 0u8;
    for pixel in image.pixels() {
        sum += u64::from(pixel.0[0]);
        peak = peak.max(pixel.0[0]);
    }
    let count = image.pixels().len().max(1) as u64;
    (sum / count, peak)
}

/// **The owner's own command shows it: the test-woken `aagun99` fires
/// `wep_27`, and the `flak_effect` burst and tail framebuffers arrive
/// non-black — flash, smoke ball and close-range flicker on screen, both
/// cues queued.**
///
/// This is the same 45 s smoke the FLAK-C5 phase drives; this task's copy is
/// the integration acceptance the parent still owes. The capture dir is
/// `CS_FLAK_C_SMOKE_OUT` when the evidence harness names it, else a temp dir.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_flak_c_the_windowed_smoke_shows_the_burst_puff_and_plays_the_cues() {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let Ok(capabilities) = std::env::var("CS_CAPABILITIES") else {
        panic!("CS_CAPABILITIES is unset: the mission launch needs gpu and audio");
    };
    for capability in ["gpu", "audio"] {
        assert!(
            capabilities
                .split(',')
                .any(|part| part.trim() == capability),
            "CS_CAPABILITIES must list {capability}: {capabilities}"
        );
    }
    let engine_image = std::env::var("CS_ENGINE_IMAGE")
        .expect("CS_ENGINE_IMAGE names the owner's decrypted executable");
    let dir = std::env::var("CS_FLAK_C_SMOKE_OUT")
        .map_or_else(|_| std::env::temp_dir().join("flak-c-smoke"), PathBuf::from);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let output = std::process::Command::new(mission_bin())
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .args(["--smoke-seconds", "45", "--capture-dir"])
        .arg(&dir)
        .env("CS_CAPABILITIES", capabilities)
        .env("CS_ENGINE_IMAGE", engine_image)
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.code().is_some(),
        "the run must exit by itself, never on a signal: {output:?}\n{stderr}"
    );

    let report = std::fs::read_to_string(dir.join(SMOKE_REPORT_FILE))
        .unwrap_or_else(|error| panic!("{} is written: {error}\n{stderr}", SMOKE_REPORT_FILE));
    for id in [
        "flak_turret_woken",
        "flak_turret_fired",
        "flak_burst_drawn",
        "flak_flicker_drawn",
        "flak_sounds_queued",
    ] {
        assert!(
            report.contains(&format!("\"id\":\"{id}\",\"applies\":true,\"passed\":true")),
            "the report judges {id} applied and passed: {report}"
        );
    }
    for field in [
        "\"flak\":{",
        "\"turret\":\"aagun",
        "\"test_wake\":true",
        "\"test_placement\":true",
        "\"rockets_fired\":",
        "\"bursts_presented\":",
    ] {
        assert!(
            report.contains(field),
            "the report's flak object carries {field}: {report}"
        );
    }
    for label in ["flak_burst", "flak_burst_tail"] {
        let pngs: Vec<_> = std::fs::read_dir(&dir)
            .expect("the capture directory lists")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(&format!("_{label}.png")))
            })
            .collect();
        assert_eq!(
            pngs.len(),
            1,
            "exactly one frame_*_{label}.png arrives: {pngs:?} in {}",
            dir.display()
        );
        let (mean, peak) = luminance(&pngs[0]);
        assert!(
            peak > 24,
            "{label} is a lit framebuffer, not black (mean {mean}, peak {peak})"
        );
    }
    println!("flak smoke artifacts in {}", dir.display());
}
