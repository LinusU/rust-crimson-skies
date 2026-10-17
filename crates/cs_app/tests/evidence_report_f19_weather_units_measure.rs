//! Evidence-report harness for task #1460 `F19-WEATHER-UNITS-MEASURE`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//!
//! Not named `accept_m01_play_weather_*`: it is not part of the acceptance
//! suite and it fails loudly when its inputs are missing, so the task's own
//! test selection can never pick it up as an acceptance test. Run it from the
//! workspace root, after the acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F19-WEATHER-UNITS-MEASURE
//!    CS_EVIDENCE_DIR=private/evidence/F19-WEATHER-UNITS-MEASURE \
//!      cargo test --workspace --locked -- accept_m01_play_weather_ --include-ignored \
//!      2>&1 | tee private/evidence/F19-WEATHER-UNITS-MEASURE/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F19-WEATHER-UNITS-MEASURE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_weather_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app --test evidence_report_f19_weather_units_measure -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F19-WEATHER-UNITS-MEASURE/acceptance.json \
//!      --artifact-root private/evidence/F19-WEATHER-UNITS-MEASURE --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F19-WEATHER-UNITS-MEASURE.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR` and a second
//! production run that re-executes the M01 weather bind and records what it
//! bound in `weather-units-census.json`, plus `rustc --version` and
//! `Cargo.lock`. Nothing is typed in by hand.
//!
//! The claim is `implemented`, never `checked` or `verified_original`: the
//! measurement behind the bind is static disassembly of the owner-supplied
//! image (`docs/findings/2026-10-16-f19-weather-units-measure.md`), and the
//! original executable never ran.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::environment::{RunSeeds, read_mission_weather};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_sim::time::TickRate;
use cs_types::content::Resolved;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_weather_";
/// The mission whose `weather.zrd` the task measured and binds.
const M01: &str = "zbd/c1c/m01";

/// What the second production observation records per bound layer.
struct BoundLayer {
    altitude_m: f64,
    provenance: String,
}

/// The census: what the production bind actually produced from M01's record.
struct Census {
    layer_altitudes: Vec<BoundLayer>,
    fog_color: Option<[f64; 3]>,
    fog_provenance: String,
    unbound: Vec<String>,
}

/// Re-runs the task's production path — M01's own `weather.zrd` through
/// [`read_mission_weather`] into an environment session — and records what it
/// bound. The assertions on it are the measurement's own claims: the cloud
/// floor at `CLOUD_COVER.BOTTOM` metres and the shared normalized zone
/// `FOG_COLOR`, while `TOP`/`THICKNESS` and the fog ranges stay named unbound.
fn observe(game_dir: &Path) -> Census {
    let found = discover(game_dir).expect("production discovery reads the installation");
    let weather =
        read_mission_weather(&found, M01).unwrap_or_else(|error| panic!("{M01}: {error}"));
    let session = weather
        .session(TickRate::new(64).expect("64 Hz"), RunSeeds::from_root(7))
        .expect("M01's weather starts a session");
    let definition = session.definition();
    assert!(definition.origin().is_original(), "the record is M01's own");

    // The spec reports each bound layer's altitude with the definition's own
    // provenance (`WeatherOrigin::Bound(definition.provenance())`), so the
    // census records the same pair.
    let layer_provenance = format!("{:?}", definition.provenance());
    let layer_altitudes: Vec<BoundLayer> = definition
        .cloud_layers()
        .iter()
        .map(|layer| BoundLayer {
            altitude_m: layer.altitude_m(),
            provenance: layer_provenance.clone(),
        })
        .collect();
    let (fog_color, fog_provenance) = match definition.fog().color_linear() {
        Resolved::Known(fog) => (Some(fog.value), format!("{:?}", fog.provenance)),
        Resolved::Unknown { .. } => (None, "unbound".to_owned()),
    };
    let unbound: Vec<String> = weather
        .binding
        .unbound
        .iter()
        .map(|field| field.key.to_owned())
        .collect();
    let census = Census {
        layer_altitudes,
        fog_color,
        fog_provenance,
        unbound,
    };
    assert_eq!(
        census.layer_altitudes.len(),
        1,
        "CLOUD_COVER binds one layer"
    );
    assert_eq!(
        census.layer_altitudes[0].altitude_m, 1055.0,
        "BOTTOM in metres"
    );
    assert_eq!(
        census.fog_color,
        Some([f64::from(0.69f32); 3]),
        "the shared zone FOG_COLOR binds normalized"
    );
    for key in ["CLOUD_COVER.TOP/THICKNESS", "ZONEn.FOG_RANGES/FOG_ALTITUDE"] {
        assert!(
            census.unbound.iter().any(|named| named == key),
            "{key} must stay named as unbound"
        );
    }
    census
}

impl Census {
    fn to_json(&self, install_sha256: &str, candidate_tree: &str) -> String {
        let layers: Vec<String> = self
            .layer_altitudes
            .iter()
            .map(|layer| {
                format!(
                    "{{\"altitude_m\": {}, \"provenance\": {}}}",
                    layer.altitude_m,
                    jstr(&layer.provenance)
                )
            })
            .collect();
        let fog = match self.fog_color {
            Some(value) => format!(
                "{{\"bound\": [{}, {}, {}], \"provenance\": {}}}",
                value[0],
                value[1],
                value[2],
                jstr(&self.fog_provenance)
            ),
            None => "{\"bound\": null}".to_owned(),
        };
        let unbound: Vec<String> = self.unbound.iter().map(|key| jstr(key)).collect();
        format!(
            "{{\n \"mission\": {},\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \
             \"layers\": [{}],\n \"fog_color\": {},\n \"unbound\": [{}]\n}}\n",
            jstr(M01),
            jstr(install_sha256),
            jstr(candidate_tree),
            layers.join(", "),
            fog,
            unbound.join(", ")
        )
    }
}

/// How this run was reviewed.
fn review_method(census: &Census, tests: u64) -> String {
    format!(
        "Acceptance suite run locally with `retail` (CS_GAME_DIR): {tests} \
         `accept_m01_play_weather_` tests passed on the recorded log. This harness derived every \
         field from the recorded log, production discovery of $CS_GAME_DIR, and a second \
         production run (weather-units-census.json) that re-read M01's own weather.zrd through \
         `read_mission_weather` and recorded the bind it produces: one cloud layer at {} m with \
         the record's provenance, the shared zone `FOG_COLOR` bound normalized, and the \
         `TOP`/`THICKNESS` + fog ranges still named unbound. The units claim itself is static \
         disassembly of the owner-supplied image — `docs/findings/\
         2026-10-16-f19-weather-units-measure.md` names every virtual address — not a run of the \
         original. LIMITS: the record's colour domain (normalized floats that most plausibly \
         address the device's display/sRGB fog colour, bound verbatim into `color_linear`) is a \
         documented residual, and how the original renderer drew `cloudlayer`/`cloud1`/`cloud2` \
         stays unmeasured. Validated with tools/validate_evidence.py --require-pass.",
        census.layer_altitudes[0].altitude_m,
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f19_weather_units_measure_writes_the_acceptance_report() {
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

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second observation: the production bind re-run on M01's record.
    let census = observe(&game_dir);
    let census_path = evidence_dir.join("weather-units-census.json");
    fs::write(
        &census_path,
        census.to_json(&install_sha256, &candidate_tree),
    )
    .expect("write weather-units-census.json");

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
            .any(|(name, _, kind)| name == "weather-units-census.json" && kind == "json"),
        "the bind census must be one of the artifacts"
    );

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F19-WEATHER-UNITS-MEASURE\",\n \"candidate_tree\": {},\n \
         \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \
         \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \
         \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \
         \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \
         \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": \
         [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \
         \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(&review_method(&census, suite.passed)),
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

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f19_weather_units_measure.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F19-WEATHER-UNITS-MEASURE` written
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

/// Extracts the per-test results of the `accept_m01_play_weather_` tests from
/// a recorded `cargo test` output.
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
            record(&mut suite, name, status);
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
            Some(status) => record(&mut suite, name.to_owned(), status),
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

fn record(suite: &mut Suite, name: String, status: &'static str) {
    suite.assertions.push((name, status));
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
