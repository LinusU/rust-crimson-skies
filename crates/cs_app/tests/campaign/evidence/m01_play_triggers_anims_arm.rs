//! Evidence-report harness for task `M01-PLAY-TRIGGERS-ANIMS-ARM` (#1497):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! A member of the campaign binary (see `evidence.rs`), deliberately not named
//! `accept_m01_play_triggers_anims_arm_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_play_triggers_anims_arm_
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-PLAY-TRIGGERS-ANIMS-ARM/cargo-test.log` (note the
//!    exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-ANIMS-ARM \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_anims_arm_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked --test campaign evidence_report_m01_play_triggers_anims_arm -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-PLAY-TRIGGERS-ANIMS-ARM/acceptance.json
//!    --artifact-root private/evidence/M01-PLAY-TRIGGERS-ANIMS-ARM`
//! 4. Commit a copy as
//!    `docs/findings/evidence/M01-PLAY-TRIGGERS-ANIMS-ARM.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness binds M01's real animation
//! closure through `bind_mission_animation` and resolves the four names the
//! mission's own `WAKE_ANIM` sites spell, reporting each name's carrier,
//! record index, declaring member, measured duration and playability as JSON —
//! no original bytes.
//!
//! `--require-pass` is deliberately **not** used. The report carries the
//! unresolved issues the owner directive of 2026-09-28 says must stay in
//! machine-readable evidence: the designed node-arming rule is unmeasured, and
//! the landing half that makes the gated records run is
//! M01-PLAY-TRIGGERS-ANIMS-LAND (#1498), a sibling this task does not own.
//! Deleting them to turn the flag green is exactly what the directive forbids.
//! The pass/fail facts are carried by `command.exit_code` and the `tests`
//! counts, which the plain structural validation does check.

use super::*;

use cs_app::animation::mission::{NamedAnimationResolution, bind_mission_animation};

/// This task's own acceptance prefix, so the selection cannot credit a
/// sibling task's assertions to this report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_triggers_anims_arm_";

/// The mission scope whose animation closure is bound.
const M01: &str = "zbd/c1c/m01";

/// The four names M01's own `WAKE_ANIM` sites spell, in the order the
/// mission-language finding records them.
const WOKEN_NAMES: [&str; 4] = [
    "activate_dropoff_node",
    "activate_pickup_node",
    "pzhomebase",
    "fadein_bszep",
];

/// What the second production observation measured.
#[derive(Debug)]
struct Census {
    /// One row per woken name: its resolution as JSON.
    rows: Vec<String>,
    /// How many records M01's two carriers' own tables hold.
    record_count: usize,
}

impl Census {
    /// The census as JSON: names, carrier keys, record indices, members and
    /// measured durations, no original bytes.
    fn to_json(&self, install_sha256: &str, candidate_tree: &str) -> String {
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"scope\": {},\n \
             \"record_count\": {},\n \"woken\": [\n{}],\n \
             \"claim\": \"implemented\",\n \"capabilities\": [\"retail\", \"synthetic\"]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            jstr(M01),
            self.record_count,
            self.rows.join(",\n"),
        )
    }
}

/// The second production observation: bind M01's animation closure and report
/// where each `WAKE_ANIM` name lands.
fn observe(game_dir: &Path) -> Census {
    let binding = bind_mission_animation(game_dir, M01)
        .expect("M01's animation closure reads through the production readers");
    let names = binding.names();
    let mut rows = Vec::new();
    for name in WOKEN_NAMES {
        let row = match names.resolve(name) {
            NamedAnimationResolution::Bound(row) => row,
            NamedAnimationResolution::Unbound { reason, matches } => {
                let matched: Vec<String> = matches
                    .iter()
                    .map(|(carrier, index)| format!("[{}, {index}]", jstr(carrier.label())))
                    .collect();
                rows.push(format!(
                    "  {{\"name\": {}, \"unbound_reason\": {}, \"matches\": [{}]}}",
                    jstr(name),
                    jstr(reason),
                    matched.join(", ")
                ));
                continue;
            }
        };
        let facts = row
            .bound_record()
            .unwrap_or_else(|| panic!("{name} resolved without a record"));
        let member = row
            .declaration()
            .resolution()
            .single()
            .map_or_else(String::new, |site| site.member().to_owned());
        rows.push(format!(
            "  {{\"name\": {}, \"carrier\": {}, \"carrier_key\": {}, \"record_index\": {}, \
             \"anim_name\": {}, \"declaring_member\": {}, \"playable\": {}, \
             \"duration_time\": {:.1}, \"refusals\": {}}}",
            jstr(name),
            jstr(facts.carrier().label()),
            jstr(facts.carrier_key()),
            facts.index(),
            jstr(facts.anim_name()),
            jstr(&member),
            row.is_playable(),
            row.playback()
                .map_or(0.0, |playback| playback.duration_time()),
            str_array(
                &row.refusals()
                    .iter()
                    .map(|refusal| refusal.label().to_owned())
                    .collect::<Vec<String>>()
            ),
        ));
    }
    Census {
        rows,
        record_count: names.record_count(),
    }
}

/// How this run was reviewed; the measured counts are interpolated from the
/// census this same run produced, so a report cannot describe another run's
/// numbers.
fn review_method(census: &Census, tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production \
         observation (m01-play-triggers-anims-arm-census.json) that binds M01's real animation \
         closure and resolves the four names its WAKE_ANIM sites spell. Claim is implemented only, \
         not checked: no independent reviewer has looked at this run, no original executable ran \
         and nothing here is verified_original. MEASURED (retail, read-only): M01's two carriers \
         hold {records} records; each woken name's carrier, record index, declaring member, \
         measured duration and playability are in the census. LIMITS OF WHAT WAS MEASURED: the \
         node-arming rule this task records (claim m01-anim.object-active-state-arming-designed) \
         is DESIGNED, because ACTIVATION_PREREQUISITE / OBJECT_ACTIVE_LIST evaluation and the \
         event payloads' node fields are unmeasured; the landing half that runs the gated records \
         when the player docks is M01-PLAY-TRIGGERS-ANIMS-LAND (#1498), a sibling this task does \
         not own, so no ANIM_STATE gate is closed here; and the retail fold member supplies the \
         gate observations the landing path will publish, which its own module documents. The \
         {tests} assertion(s) discovered under this task's own prefix are the tests of this \
         report.",
        records = census.record_count,
        tests = tests,
    )
}

/// The acceptance run's own counts, keyed by this task's prefix only.
#[derive(Debug, Default)]
struct ArmSuite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of this task's tests from a recorded `cargo
/// test` output, ignoring every `test result:` summary: a summary aggregates
/// every test binary cargo ran.
fn parse_arm_suite(log: &str) -> ArmSuite {
    let mut suite = ArmSuite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = arm_status(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            arm_record(&mut suite, name, status);
            continue;
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let full = &after[..separator];
            let tail = &after[separator + 5..];
            cursor = tail;
            if !full.contains(ACCEPTANCE_PREFIX) {
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
            match arm_status(tail.trim()) {
                Some(status) => arm_record(&mut suite, name, status),
                None => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.passed = arm_count(&suite, "pass");
    suite.failed = arm_count(&suite, "fail");
    suite.ignored = arm_count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// The libtest result word at the head of a test's tail line.
fn arm_status(tail: &str) -> Option<&'static str> {
    match tail.split_whitespace().next() {
        Some("ok") => Some("pass"),
        Some("FAILED") => Some("fail"),
        Some("ignored") => Some("unknown"),
        _ => None,
    }
}

fn arm_count(suite: &ArmSuite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, seen)| *seen == status)
        .count() as u64
}

fn arm_record(suite: &mut ArmSuite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

/// One assertion's entry: both artifacts of this report carry it.
fn arm_assertions(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"m01-play-triggers-anims-arm-census.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn evidence_report_m01_play_triggers_anims_arm_writes_the_acceptance_report() {
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
    assert!(
        std::env::var("CS_ENGINE_IMAGE").is_ok_and(|path| !path.is_empty()),
        "CS_ENGINE_IMAGE must name the owner's decrypted image: M01's player start is only known \
         from it, so the composed observation cannot run without it"
    );

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );
    fs::create_dir_all(&evidence_dir)
        .unwrap_or_else(|error| panic!("create {}: {error}", evidence_dir.display()));

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_arm_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation.
    let census = observe(&game_dir);
    let census_path = evidence_dir.join("m01-play-triggers-anims-arm-census.json");
    fs::write(
        &census_path,
        census.to_json(&install_sha256, &candidate_tree),
    )
    .expect("write m01-play-triggers-anims-arm-census.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    // The unresolved issues stay in machine-readable evidence (owner directive
    // of 2026-09-28): the designed arming rule is unmeasured, and the landing
    // half is the sibling M01-PLAY-TRIGGERS-ANIMS-LAND (#1498).
    let unknowns: Vec<String> = vec![
        "the node-arming rule is designed (claim m01-anim.object-active-state-arming-designed): \
         ACTIVATION_PREREQUISITE / OBJECT_ACTIVE_LIST evaluation and the event payloads' node \
         fields are unmeasured, so nothing here claims the original armed a node",
        "M01-PLAY-TRIGGERS-ANIMS-LAND (#1498) owns running the gated records when the player \
         docks; no ANIM_STATE gate is closed by this task",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-PLAY-TRIGGERS-ANIMS-ARM\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {}}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": {},\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        census.record_count,
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        arm_assertions(&suite.assertions),
        artifact_array(&artifacts),
        str_array(&unknowns),
        jstr(&reviewer),
        jstr(&review_method(&census, suite.discovered as usize)),
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
