//! Acceptance for Rally #1887 `FLAK-C5-SMOKE`: the owner-command smoke's flak
//! phase — a **test-woken** world turret (`aagun99` in M01, a `WEAPON wep_27`
//! record no M01 program wakes) firing the retail flak rocket at the player,
//! the round's burst reaching the `flak_effect` presentation and the two flak
//! cues sounding (findings `docs/findings/2026-10-22-flak-c1-burst-presentation-and-descriptor-flag.md`,
//! `docs/findings/2026-10-22-flak-c4-flak-sounds.md`).
//!
//! The synthetic tests run in CI: they exercise the production wake ledger's
//! `wake_for_test` — the same matcher a `WAKEUP_TURRETS` emission uses — and
//! the smoke's check judge on the phase's own measurements. The windowed test
//! at the bottom drives the owner's own command and needs `retail`, `gpu` and
//! `audio` (`--include-ignored`). `retail` is read access to the installation;
//! nothing here is `verified_original`.

use std::path::PathBuf;

use cs_app::mission_session::{
    Check, FlakMetrics, SMOKE_REPORT_FILE, SmokeMeasurements, evaluate_mission_smoke,
};
use cs_app::mission_turrets::{TurretEntries, TurretWakeLedger, read_turret_specs};
use cs_content::stunts::ZrdValue;
use cs_types::Tick;

fn check<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("the smoke reports a {id} check: {checks:?}"))
}

/// The phase's measurements after a run that woke `aagun99`, placed the
/// player, fired `wep_27` and drew one burst with its flicker and cues.
fn drawn() -> SmokeMeasurements {
    SmokeMeasurements {
        flak: FlakMetrics {
            turret: Some("aagun99".to_owned()),
            woken: true,
            placed: true,
            detection_range_m: 200.0,
            sound_measured: true,
            rockets_fired: 3,
            shots_logged: 3,
            bursts: 2,
            bursts_presented: 2,
            flashes_spawned: 2,
            smoke_balls: 2,
            flickers_started: 1,
            flickers_skipped: 1,
            trails_undrawn: 2,
            shot_cues: 3,
            burst_cues: 2,
            sound_refusals: vec![],
            refusals: vec![],
        },
        ..SmokeMeasurements::default()
    }
}

// ----------------------------------------------------------- the ledger ---

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn list(values: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(values)
}

fn field(key: &str, values: Vec<ZrdValue>) -> [ZrdValue; 2] {
    [text(key), list(values)]
}

/// A synthetic `ai.zrd`: one `wep_27` gun that starts asleep (the M01
/// `aagun*` case) and one that starts live.
fn synthetic_ai() -> ZrdValue {
    let mut asleep = Vec::new();
    asleep.extend(field("NODES", vec![text("aagun**")]));
    asleep.extend(field("ACTIVATED", vec![ZrdValue::Int(0)]));
    asleep.extend(field(
        "WEAPON",
        vec![text("NAME"), list(vec![text("wep_27")])],
    ));
    let mut live = Vec::new();
    live.extend(field("NODES", vec![text("flak**")]));
    list(vec![list(vec![
        text("TURRET"),
        list(vec![list(asleep), list(live)]),
    ])])
}

/// **#1966: `stand_down_for_test` undoes a test wake, so the smoke's flak
/// turret stops bursting round the player once its phase is over.**
#[test]
fn accept_dmg_frame_grade_stand_down_clears_the_live_byte() {
    let specs = read_turret_specs(&synthetic_ai());
    let nodes = ["aagun01", "aagun02"]
        .iter()
        .enumerate()
        .map(|(index, name)| ((*name).to_owned(), [index as f64, 0.0, 0.0]))
        .collect::<Vec<(String, [f64; 3])>>();
    let entries = TurretEntries::bind(&specs, Ok(nodes), &[]);
    let mut ledger = TurretWakeLedger::new(&entries);
    ledger.wake_for_test("aagun01", Tick(9));
    assert!(ledger.entry("aagun01").unwrap().live);
    assert!(ledger.stand_down_for_test("aagun01"));
    assert!(!ledger.entry("aagun01").unwrap().live);
    assert!(
        !ledger.stand_down_for_test("aagun01"),
        "a sleeping entry reports it was not live"
    );
}

/// **`wake_for_test` walks the ledger's own matcher: the sleeping entry's
/// live byte sets, the `TurretWake` records which names woke, and an
/// already-live entry is counted — the same bookkeeping a `WAKEUP_TURRETS`
/// emission produces.**
#[test]
fn accept_flak_c5_wake_for_test_wakes_through_the_ledgers_own_matcher() {
    let specs = read_turret_specs(&synthetic_ai());
    let nodes = ["aagun01", "aagun02", "flak07"]
        .iter()
        .enumerate()
        .map(|(index, name)| ((*name).to_owned(), [index as f64, 0.0, 0.0]))
        .collect::<Vec<(String, [f64; 3])>>();
    let entries = TurretEntries::bind(&specs, Ok(nodes), &[]);
    assert!(!entries.entry("aagun01").unwrap().live);
    assert!(!entries.entry("aagun02").unwrap().live);
    assert!(entries.entry("flak07").unwrap().live);

    let mut ledger = TurretWakeLedger::new(&entries);
    ledger.wake_for_test("aagun01", Tick(9));
    assert!(
        ledger.entry("aagun01").unwrap().live,
        "the test wake sets the live byte the turret step mirrors"
    );
    assert!(
        !ledger.entry("aagun02").unwrap().live,
        "the match is the entry's own name, not its record's pattern"
    );
    assert_eq!(ledger.wakes().len(), 1);
    assert_eq!(ledger.wakes()[0].name, "aagun01");
    assert_eq!(ledger.wakes()[0].woken, ["aagun01"]);
    assert_eq!(ledger.wakes()[0].already_live, 0);
    assert_eq!(ledger.wakes()[0].at, Tick(9));

    // A second wake of the now-live entry counts it as already live, the
    // same record a repeated `WAKEUP_TURRETS` writes.
    ledger.wake_for_test("aagun**", Tick(10));
    assert_eq!(ledger.wakes().len(), 2);
    assert_eq!(ledger.wakes()[1].woken, ["aagun02"]);
    assert_eq!(ledger.wakes()[1].already_live, 1);

    // A miss is a named refusal, never silent.
    ledger.wake_for_test("no-such-gun", Tick(11));
    assert_eq!(ledger.refused().len(), 1);
    assert_eq!(ledger.refused()[0].name, "no-such-gun");
}

// ------------------------------------------------------------ the judge ---

/// **A phase that woke the turret, fired `wep_27` and drew the burst, the
/// flicker and both cues passes every flak check.**
#[test]
fn accept_flak_c5_the_judge_passes_a_woken_turret_that_fires_and_draws_the_burst() {
    let checks = evaluate_mission_smoke(&drawn());
    for id in [
        "flak_turret_woken",
        "flak_turret_fired",
        "flak_burst_drawn",
        "flak_flicker_drawn",
        "flak_sounds_queued",
    ] {
        let verdict = check(&checks, id);
        assert!(verdict.applies, "{id} applies: {}", verdict.detail);
        assert!(verdict.passed, "{id}: {}", verdict.detail);
    }
}

/// **A phase that found no turret it could wake fails `flak_turret_woken`,
/// and the downstream checks record themselves not applicable — a turret
/// that never went live can neither fire nor draw.**
#[test]
fn accept_flak_c5_the_judge_fails_when_nothing_woke() {
    let mut measurements = drawn();
    measurements.flak = FlakMetrics {
        woken: false,
        ..measurements.flak
    };
    let checks = evaluate_mission_smoke(&measurements);
    assert!(
        !check(&checks, "flak_turret_woken").passed,
        "nothing woke: {}",
        check(&checks, "flak_turret_woken").detail
    );
    for id in [
        "flak_turret_fired",
        "flak_burst_drawn",
        "flak_flicker_drawn",
    ] {
        assert!(
            !check(&checks, id).applies,
            "{id} cannot apply to a turret that never went live"
        );
    }
}

/// **A turret that woke but fired nothing — the player stood outside its
/// field or the runtime refused — fails `flak_turret_fired`, and a round
/// that fired but never burst on screen fails `flak_burst_drawn`.**
#[test]
fn accept_flak_c5_the_judge_fails_the_shot_and_the_drawing_each() {
    let mut silent = drawn();
    silent.flak.rockets_fired = 0;
    silent.flak.shots_logged = 0;
    let checks = evaluate_mission_smoke(&silent);
    assert!(
        !check(&checks, "flak_turret_fired").passed,
        "no wep_27 left the muzzle: {}",
        check(&checks, "flak_turret_fired").detail
    );

    let mut undrawn = drawn();
    undrawn.flak.bursts_presented = 0;
    let checks = evaluate_mission_smoke(&undrawn);
    assert!(
        !check(&checks, "flak_burst_drawn").passed,
        "no burst reached the flak_effect presentation: {}",
        check(&checks, "flak_burst_drawn").detail
    );

    let mut no_flicker = drawn();
    no_flicker.flak.flickers_started = 0;
    let checks = evaluate_mission_smoke(&no_flicker);
    assert!(
        !check(&checks, "flak_flicker_drawn").passed,
        "no close-range flicker started: {}",
        check(&checks, "flak_flicker_drawn").detail
    );
}

/// **When the stage measured the flak sounds, a cue the audio session never
/// played fails `flak_sounds_queued`; a world that measured no flak sounds
/// records the check not applicable instead of failing.**
#[test]
fn accept_flak_c5_the_judge_judges_the_cues_only_when_measured() {
    let mut muted = drawn();
    muted.flak.shot_cues = 0;
    let checks = evaluate_mission_smoke(&muted);
    assert!(
        !check(&checks, "flak_sounds_queued").passed,
        "a measured shot cue that never played fails: {}",
        check(&checks, "flak_sounds_queued").detail
    );

    let mut unmeasured = drawn();
    unmeasured.flak.sound_measured = false;
    unmeasured.flak.shot_cues = 0;
    unmeasured.flak.burst_cues = 0;
    let checks = evaluate_mission_smoke(&unmeasured);
    let verdict = check(&checks, "flak_sounds_queued");
    assert!(
        !verdict.applies,
        "no measured sounds: the check records not applicable — {}",
        verdict.detail
    );
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

/// **The windowed run: the owner's own command test-wakes the flak turret,
/// fires `wep_27`, and the burst and tail framebuffers arrive non-black.**
///
/// The run writes the same `report.json` the mission smoke always writes;
/// this test asserts the flak checks were judged, the `flak` object is in
/// the report, and the two flak PNGs carry a lit framebuffer (a burst's
/// flash and smoke over the sea is many luminance levels above black).
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_flak_c5_the_windowed_launch_wakes_fires_and_draws_wep27() {
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
    let dir = std::env::var("CS_FLAK_C5_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("flak-c5-smoke"),
        PathBuf::from,
    );
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
    // The run must not merely judge the flak checks — on a retail,gpu,audio
    // host every one applies and passes (the sounds are measured, the cues
    // play). A check that failed or never applied fails this test, so a
    // regression in the wake, the fire, the presentation or the cue wiring
    // cannot slip through a run that still wrote lit framebuffers.
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
