//! Evidence-report harness for task #1726 `PARITY2-M01-HUD-ART`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//!
//! Not named `accept_parity2_m01_hud_art_*`: it is not part of the
//! acceptance suite and it fails loudly when its inputs are missing, so the
//! task's own test selection can never pick it up as an acceptance test. Run
//! it from the workspace root, after the acceptance run, exactly as:
//!
//! 1. The owner-command smoke on the reviewed commit — the run the task
//!    names, which writes the framebuffer PNGs and `report.json` the report
//!    hashes:
//!    ```sh
//!    cargo build --release --locked -p cs_app --bin cs
//!    CS_CAPABILITIES=retail,gpu,audio \
//!      target/release/cs --cs-path "$CS_GAME_DIR" --mission M01 \
//!      --smoke-seconds 45 --capture-dir private/evidence/PARITY2-M01-HUD-ART
//!    ```
//! 2. The no-teleport smoke, which flies the measured start pose without a
//!    launch cinematic — the path the `FlightCheckPhase` hold covers — and
//!    writes the in-flight frames the cluster is read off:
//!    ```sh
//!    CS_CAPABILITIES=retail,gpu,audio \
//!      target/release/cs --cs-path "$CS_GAME_DIR" --mission M01 \
//!      --smoke-seconds 45 --smoke-no-teleport \
//!      --capture-dir private/evidence/PARITY2-M01-HUD-ART/no-teleport
//!    ```
//! 3. The acceptance run (the same command AGENTS.md names for a task's
//!    selection). Its log is recorded **outside** the evidence directory and
//!    moved in afterwards:
//!    ```sh
//!    mkdir -p private/evidence/PARITY2-M01-HUD-ART
//!    cargo test --locked -p cs_app -- accept_parity2_m01_hud_art_ --include-ignored \
//!      2>&1 | tee private/evidence/PARITY2-M01-HUD-ART-cargo-test.log
//!    mv private/evidence/PARITY2-M01-HUD-ART-cargo-test.log \
//!      private/evidence/PARITY2-M01-HUD-ART/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — `${PIPESTATUS[0]}` under bash or
//!    `set -o pipefail` first; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 4. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/PARITY2-M01-HUD-ART \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app -- accept_parity2_m01_hud_art_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 3> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app --test cs_app_tests_a \
//!        'evidence_report_parity2_m01_hud_art::' -- --ignored
//!    ```
//! 5. ```sh
//!    python3 tools/validate_evidence.py \
//!      private/evidence/PARITY2-M01-HUD-ART/acceptance.json \
//!      --artifact-root private/evidence/PARITY2-M01-HUD-ART
//!    ```
//!    (`--require-pass` reports `Unresolved issues`: the report keeps its
//!    recorded follow-up unknowns — the owner directive forbids emptying
//!    them to pass the validator. The structural check and artifact hashes
//!    still validate, exit 0.)
//! 6. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/PARITY2-M01-HUD-ART.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the smoke runs' own
//! `report.json`s and framebuffer PNGs, and `rustc --version` plus
//! `Cargo.lock`. Nothing is typed in by hand.
//!
//! The claim is `implemented`, never `checked` or `verified_original`: the
//! windowed runs are this machine's own frames and no original executable
//! ran.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_parity2_m01_hud_art_";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_parity2_m01_hud_art_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );

    // The scripted smoke's own report: the checks, their verdicts and the
    // tick count this run measured. The task's acceptance is the smoke
    // passing, so the report must record the run's own `passed` flag true.
    let smoke_path = evidence_dir.join("report.json");
    let smoke = fs::read_to_string(&smoke_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", smoke_path.display()));
    assert!(
        smoke.contains("\"kind\":\"mission-smoke-report\""),
        "{} is the smoke's own report",
        smoke_path.display()
    );
    assert!(
        smoke.contains("\"human_play\":false"),
        "a scripted run is never human_play"
    );
    assert!(
        smoke.contains("\"passed\":true"),
        "the owner-command smoke must pass on the reviewed commit"
    );
    let ticks_end = json_u64(&smoke, "ticks").unwrap_or(0);

    // The no-teleport run's own report: the flight-check-hold path, and the
    // in-flight frames the cluster is read off.
    let no_teleport_path = evidence_dir.join("no-teleport").join("report.json");
    let no_teleport = fs::read_to_string(&no_teleport_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", no_teleport_path.display()));
    assert!(
        no_teleport.contains("\"mode\":\"no_teleport\""),
        "{} is the no-teleport run's own report",
        no_teleport_path.display()
    );
    assert!(
        no_teleport.contains("\"player_teleported\":false"),
        "the parity frames are of a flight that never teleported"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // Every artifact this run left in the evidence directory — the log, the
    // smokes' reports and their framebuffer PNGs — hashed as it sits.
    let mut artifacts = collect_artifacts(&evidence_dir, &evidence_dir);
    artifacts.sort_by(|left, right| left.0.cmp(&right.0));
    assert!(
        artifacts
            .iter()
            .any(|(name, _, kind)| name == "cargo-test.log" && kind == "log"),
        "the acceptance log must be one of the artifacts"
    );
    assert!(
        artifacts
            .iter()
            .any(|(name, _, kind)| name == "report.json" && kind == "json"),
        "the smoke's report must be one of the artifacts"
    );
    assert!(
        artifacts
            .iter()
            .any(|(name, _, kind)| name == "no-teleport/report.json" && kind == "json"),
        "the no-teleport run's report must be one of the artifacts"
    );
    assert!(
        artifacts.iter().any(|(_, _, kind)| kind == "png"),
        "the GPU captures must be among the artifacts: this task claims `gpu`"
    );
    // The frames the task's acceptance names: the scripted run's `hud` and
    // Flight Check frames (their tick numbers are the run's own), and the
    // no-teleport run's in-flight frames at the fixed capture ticks.
    for (pattern, present) in [
        (
            "the scripted run's frame_*_hud.png",
            artifacts.iter().any(|(name, _, kind)| {
                name.starts_with("frame_") && name.ends_with("_hud.png") && kind == "png"
            }),
        ),
        (
            "the scripted run's frame_fc*_flight_check.png",
            artifacts.iter().any(|(name, _, kind)| {
                name.starts_with("frame_fc") && name.ends_with("_flight_check.png") && kind == "png"
            }),
        ),
        (
            "no-teleport/frame_01500_flight_25s.png",
            artifacts.iter().any(|(name, _, kind)| {
                name == "no-teleport/frame_01500_flight_25s.png" && kind == "png"
            }),
        ),
        (
            "no-teleport/frame_04200_flight_70s.png",
            artifacts.iter().any(|(name, _, kind)| {
                name == "no-teleport/frame_04200_flight_70s.png" && kind == "png"
            }),
        ),
    ] {
        assert!(present, "a cited framebuffer frame is missing: {pattern}");
    }

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"PARITY2-M01-HUD-ART\",\n \"candidate_tree\": {},\n \
         \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \
         \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \
         \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {ticks_end}}},\n \"overrides\": {},\n \
         \"capabilities\": [\"retail\", \"gpu\", \"audio\"],\n \"tests\": {{\"discovered\": {}, \
         \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": \
         [{}],\n \"artifacts\": [{}],\n \"unknowns\": {},\n \"review\": {{\"identity\": {}, \
         \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        str_array(&[
            "the mission smoke's manual 1/60 s clock".to_owned(),
            "the mission smoke's test placements of the production player body (each one listed \
             in report.json under `placements`); the no-teleport run that produces the flight \
             frames uses none"
                .to_owned(),
        ]),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        str_array(&[
            "the altimeter needle rates, the compass tape's period and scroll direction, the pip \
             fill rule and the damage state→colour mapping (PartState::Damaged → yellow, \
             Destroyed → red; the orangehilite set is never selected) are designed fits — the \
             original's exact values are unmeasured, recorded in \
             docs/findings/2026-10-20-m01-hud-art-instruments.md"
                .to_owned(),
            "the lowalt_on/stallwarning_on flags, the mgarrow/ggarrow ordnance arrows and the \
             nitro_boost/nitro_charge needles have no measured driver: they keep their authored \
             state (the flags hidden) and are reported in MissionHudArt.unknowns"
                .to_owned(),
            "the drawn dial radius (0.9 HUD units per measured face radius) and the 4:3 camera \
             fit are designed capture fits to the owner's reference frames, not stored rows"
                .to_owned(),
            "the gun gauge's pip fill has no declared capacity in the weapon authority: it \
             compares remaining rounds against remaining rounds, so the four ggindicator pips \
             are lit until the bank runs dry — a designed all-or-nothing rule"
                .to_owned(),
            "the scripted smoke never damages the player, so the green → yellow/red zone \
             material swap is composed and driven but only the intact green state is \
             photographed"
                .to_owned(),
        ]),
        jstr(&reviewer),
        jstr(&review_method()),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// How this run was reviewed.
///
/// The prose names the windowed runs, what they measured and what they could
/// not reach, so a regenerated report cannot describe more than this one
/// observed.
fn review_method() -> String {
    "Acceptance suite run locally with `retail,gpu,audio` (CS_GAME_DIR, CS_ENGINE_IMAGE): the \
     owner's own command `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 \
     --capture-dir private/evidence/PARITY2-M01-HUD-ART` drove the same windowed composition \
     the owner plays on this machine's GPU — through the launch cinematic — and wrote real \
     framebuffer PNGs plus report.json with its scripted checks passing (`passed:true`, \
     `human_play:false`). A second, no-teleport run (`--smoke-no-teleport`) flew the measured \
     start pose continuously — never moving the player (`player_teleported:false`, \
     `placements:[]`) — and wrote the in-flight frames at the fixed capture ticks \
     (`frame_01500_flight_25s.png`, `frame_04200_flight_70s.png`), which is also the path the \
     Flight Check hold covers: the mesh cluster's camera stays off while the check screen is \
     pending and draws after Fly Mission. The cited HUD frame (`frame_*_hud.png`) shows the \
     original gauge cluster drawn from the measured `instruments.zrd` placements and the \
     `player > player_pfighter > cockpit1 > gauges` subtree of `zbd/planes.zbd` — dial faces, \
     needles, the damage silhouette's four zones, missile/gun pips and glyph quads, the nitro \
     gauge and the compass tape — plus the decoded `rcircle` reticle from `zbd/rimage.zbd`, \
     where the text/CSS stand-ins stood down. The retail test composes M01 headless and asserts \
     the cluster at once: the seven measured placements, one HUD root plus one dedicated \
     overlay camera on the HUD render layer, ~30 mesh parts layered, and every measured live \
     element carrying its marker (3 needles, the compass tape, 4 damage zones, 12 pips, 20 \
     glyph cells, 2 flags, 2 arrows, 2 nitro needles). The runs are automated, never \
     `human_play`, and no original executable ran. This harness derives every field from the \
     recorded log, production discovery of $CS_GAME_DIR, the smokes' reports and the hashes of \
     those artifacts. LIMITS: the altimeter rates, tape period, pip fill and damage-colour \
     mappings are designed (unmeasured); the lowalt/stall flags, ordnance arrows and nitro \
     needles have no measured driver and keep their authored state; the gun pips have no \
     declared capacity so they light all-or-nothing; and the scripted smoke never damages the \
     player, so only the zones' intact green is photographed. Validated with \
     tools/validate_evidence.py (structure and artifact hashes, exit 0); --require-pass \
     reports the recorded follow-up unknowns as unresolved issues, which stand as this task's \
     honest limitations rather than being emptied to pass the gate."
        .to_owned()
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_parity2_m01_hud_art.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/PARITY2-M01-HUD-ART` written
/// relative to the workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The locked version of one `Cargo.lock` package: read, never asserted from
/// memory.
fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            wanted = false;
        } else if let Some(name) = line.strip_prefix("name = \"") {
            wanted = name.trim_end_matches('"') == package;
        } else if let Some(version) = line.strip_prefix("version = \"")
            && wanted
        {
            return version.trim_end_matches('"').to_owned();
        }
    }
    panic!("package {package:?} is not in {}", lock_path.display());
}

/// One integer out of the smoke's `report.json`, by key: the run's own
/// numbers, never a remembered one.
fn json_u64(report: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\":");
    let index = report.find(&needle)?;
    let rest = &report[index + needle.len()..];
    let digits: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn iso_utc_now() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs();
    let days = (since / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        (since % 86_400) / 3600,
        (since % 3600) / 60,
        since % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (y, m, d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

// ---------------------------------------------------------- log parsing ---

/// What the recorded `cargo test` output says actually happened.
#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail" | "unknown")`, in log order.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of the `accept_parity2_m01_hud_art_`
/// tests from a recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, never from the
/// `test result:` summaries: a summary aggregates every test binary cargo
/// ran, so reading it would report hundreds of unrelated tests as this task's
/// selection. A prefixed test that was ignored is recorded with the schema's
/// `unknown` status rather than counted as a pass, so the report can never
/// claim an assertion it did not run.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = finished(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            suite.assertions.push((name, status));
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("test ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(" ... ") else {
            continue;
        };
        if !carries_prefix(name) {
            continue;
        }
        match finished(tail.trim()) {
            Some(status) => suite.assertions.push((name.to_owned(), status)),
            None => pending.push_back(name.to_owned()),
        }
    }
    suite.passed = count(&suite, "pass");
    suite.failed = count(&suite, "fail");
    suite.ignored = count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// Whether a libtest name is one of this task's tests, matched on the name's
/// last path segment so an in-module unit test under a module path still
/// counts.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(|leaf| leaf.starts_with(ACCEPTANCE_PREFIX))
}

fn finished(line: &str) -> Option<&'static str> {
    if line.starts_with("ok") {
        Some("pass")
    } else if line.starts_with("ignored") || line.starts_with("ignored,") {
        Some("unknown")
    } else if line.starts_with("FAILED") || line.starts_with("failed") {
        Some("fail")
    } else {
        None
    }
}

fn count(suite: &Suite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, found)| *found == status)
        .count() as u64
}

// ------------------------------------------------------------- artifacts ---

/// Every file under `root`, as `(relative path, sha256, kind)`, skipping the
/// report this harness is about to write.
fn collect_artifacts(root: &Path, dir: &Path) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()));
    for entry in entries {
        let entry = entry.expect("a directory entry");
        let path = entry.path();
        if path.is_dir() {
            found.extend(collect_artifacts(root, &path));
            continue;
        }
        let name = path
            .strip_prefix(root)
            .expect("the entry is under the evidence root")
            .to_string_lossy()
            .replace('\\', "/");
        if name == "acceptance.json" {
            continue;
        }
        let kind = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("file")
            .to_owned();
        found.push((name, sha256_file(&path), kind));
    }
    found
}

/// One artifact's digest, computed with the production SHA-256: the
/// validator re-hashes the same file with `hashlib` independently.
fn sha256_file(path: &Path) -> String {
    let bytes =
        fs::read(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    cs_assets::install::sha256(&bytes).to_hex()
}

struct Engine {
    rust: String,
    bevy: String,
    avian: String,
}

fn engine_json(engine: &Engine) -> String {
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&engine.rust),
        jstr(&engine.bevy),
        jstr(&engine.avian)
    )
}

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    let items: Vec<String> = artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
