//! Evidence-report harness for task `M01-B-WAKE` (#1357):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_b_wake_*`: it is not part of the acceptance suite
//! and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_b_wake_ --include-ignored 2>&1 |
//!    tee private/evidence/M01-B-WAKE/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-B-WAKE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_b_wake_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR=<installation> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_b_wake -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-B-WAKE/acceptance.json
//!    --artifact-root private/evidence/M01-B-WAKE --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-B-WAKE.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-reads M01's wake graph —
//! `recover_retail_objectives` for the engine declarations, stage conditions
//! and outcomes, and `survey_mission_control_programs`'s own lowering for the
//! gate conditions — and records it as `wake-graph.json`. That is a real
//! production run over the owner's installation, not a paraphrase of the
//! acceptance assertions, and it carries no original bytes: block numbers,
//! states, times, thresholds, member chains and gate shapes only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_control::survey_mission_control_programs;
use cs_app::objectives::{ObjectiveRecovery, recover_retail_objectives};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::objectives_conditions::ObjectiveOutcome;
use cs_content::objectives_engine::EngineDeclaration;
use cs_script::ir::{Condition, MissionProgram};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_b_wake_";

/// M01's mission directory, as the census and the recovery name it.
const MISSION_DIR: &str = "zbd/c1c/m01";

/// `WAKE` edges on the win chain: (completing number, woken number).
const WIN_WAKE_EDGES: &[(u32, u32)] = &[(6, 8), (10, 49), (50, 11), (18, 24)];
/// `NAP` re-wake edges on both chains: (completing number, re-woken number,
/// record seconds). `OBJECTIVE17`'s `WAKE` of `18` is asserted separately with
/// its other targets.
const NAP_EDGES: &[(u32, u32, f32)] = &[
    (9, 10, 15.0),
    (49, 50, 5.0),
    (11, 12, 15.0),
    (12, 13, 15.0),
    (13, 14, 30.0),
    (14, 15, 30.0),
    (15, 16, 3.0),
    (16, 17, 10.0),
    (36, 37, 15.0),
    (37, 38, 15.0),
    (38, 39, 15.0),
    (39, 40, 15.0),
];
/// The `INACTIVE` ladders on the terminal chains: (number, threshold, member
/// count).
const INACTIVE_GATES: &[(u32, u32, usize)] = &[
    (6, 1, 18),
    (8, 5, 18),
    (9, 9, 18),
    (36, 2, 12),
    (37, 6, 12),
    (38, 10, 12),
    (39, 12, 12),
];
/// The `ANIM_STATE` gates on the win chain: (number, animation, wanted state).
const ANIM_GATES: &[(u32, &str, u32)] = &[
    (11, "wv_drop_copilot", 2),
    (15, "wv_pickup_copilot", 3),
    (18, "hooked_to_klondike", 3),
];
/// The `DEDG` gates on the win chain: (number, group, remaining).
const DEDG_GATES: &[(u32, i32, i32)] = &[(16, 1, 0), (17, 2, 0)];

/// How this run was reviewed.
fn review_method(own_tests: u64) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every \
         field from the recorded log, production discovery of $CS_GAME_DIR, and a second \
         production read of M01's wake graph — recover_retail_objectives plus the control \
         census's own lowering on {MISSION_DIR} (wake-graph.json). Claim is implemented only: \
         the graph is static record evidence (the measured lifecycle model of M01-B-OBJ-1/2 and \
         the measured gate conditions of M01-B-OBJ-3), plus driven headless runs through the \
         production host in the acceptance suite — no original executable ran, so nothing here \
         is verified_original runtime behaviour. MEASURED: both Finish blocks (OBJECTIVE24 \
         INSTANTWIN, OBJECTIVE40 INSTANTLOSS) load dormant behind BEGIN_DORMANT -1, OBJECTIVE1 \
         is the record's only armed self-wake, and every other wake is a completion effect \
         rooted at one of the five awake-at-load blocks (2, 3, 6, 7, 36), each gated on a world \
         fact — the census asserts the whole graph edge by edge so a drift between the \
         findings document and the record fails this run. LIMITS OF WHAT WAS MEASURED, each an \
         `unknowns` entry: the in-play-bit writers the INACTIVE counts read are untraced world \
         code; DEDG's member-field rewrites are a named residual the runtime does not \
         reproduce; TRAVELERS' world-side producers are off the terminal paths and unproduced; \
         TICK_DEPENDS_ON_OBJ's roots descend from dormant world-fact consumers. TEST-SELECTION \
         NOTE: the prefix accept_m01_b_wake_ is unique to this task, so the {own_tests} \
         discovered assertions are exactly this task's tests. Validated with \
         tools/validate_evidence.py --require-pass.",
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_b_wake_writes_the_acceptance_report() {
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

    // The second production observation: M01's wake graph — the engine
    // declarations and outcomes of the record's own recovery, and the gate
    // conditions of the control census's own lowering.
    let recovery = recover_retail_objectives(&game_dir, MISSION_DIR).expect("M01 reads");
    let census = survey_mission_control_programs(&game_dir).expect("the control census reads");
    let program = census
        .row(MISSION_DIR)
        .and_then(|row| row.lowering_attempt())
        .and_then(|attempt| attempt.program())
        .expect("M01's complete lowering produced a program");
    assert_wake_graph(&recovery, program);
    let graph_path = evidence_dir.join("wake-graph.json");
    fs::write(
        &graph_path,
        format!(
            "{}\n",
            render_wake_graph(&recovery, program, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write wake-graph.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&graph_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-B-WAKE\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        unknown_array(),
        jstr(&reviewer),
        jstr(&review_method(suite.discovered)),
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

/// Asserts the wake graph the task relies on, edge by edge: the terminals'
/// dormancy and outcomes, the awake-at-load set, the chain `WAKE`/`NAP`
/// edges, the `INACTIVE` thresholds and the `ANIM_STATE`/`DEDG` gates. A
/// record that stops matching any of them fails the harness, so no report is
/// written over a graph the tests did not drive.
fn assert_wake_graph(recovery: &ObjectiveRecovery, program: &MissionProgram) {
    let declaration = |number: u32| -> EngineDeclaration {
        recovery
            .engine_declarations()
            .get(&number)
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(
        recovery.engine_objective_count(),
        58,
        "M01 loads 58 objectives"
    );
    assert_eq!(
        recovery.outcomes().get(&24),
        Some(&ObjectiveOutcome::InstantWin),
        "OBJECTIVE24 is the INSTANTWIN block"
    );
    assert_eq!(
        recovery.outcomes().get(&40),
        Some(&ObjectiveOutcome::InstantLoss),
        "OBJECTIVE40 is the INSTANTLOSS block"
    );

    // The load states: five blocks awake (no BEGIN_DORMANT), OBJECTIVE1 the
    // only armed self-wake, both terminals behind the -1 sentinel and no
    // armed nap/sleep self-timer anywhere.
    let awake_at_load: Vec<u32> = (1..=58u32)
        .filter(|number| declaration(*number).begin_dormant.is_none())
        .collect();
    assert_eq!(
        awake_at_load,
        vec![2, 3, 6, 7, 36],
        "the graph roots are the five blocks that load awake"
    );
    for number in 1..=58u32 {
        let Some(timers) = declaration(number).begin_dormant else {
            continue;
        };
        if number == 1 {
            assert_eq!(timers.wake_at, 2.0, "OBJECTIVE1's lone self-wake is 2 s");
        } else {
            assert_eq!(
                timers.wake_at, -1.0,
                "OBJECTIVE{number} carries no self-wake"
            );
        }
        assert_eq!(timers.nap_after, -1.0);
        assert_eq!(timers.rewake_after, -1.0);
        assert_eq!(timers.sleep_at, -1.0);
    }

    // The chain edges.
    for (from, to) in WIN_WAKE_EDGES {
        assert!(
            declaration(*from)
                .wake
                .targets()
                .contains(&(*to as usize - 1)),
            "OBJECTIVE{from}'s WAKE must list OBJECTIVE{to}"
        );
    }
    assert_eq!(
        declaration(17).wake.targets(),
        &[17, 24, 48, 57],
        "OBJECTIVE17 wakes 18, 25, 49 and 58"
    );
    for (from, to, seconds) in NAP_EDGES {
        let nap = declaration(*from)
            .nap
            .unwrap_or_else(|| panic!("OBJECTIVE{from} declares a NAP"));
        assert_eq!(
            nap.target,
            Some(*to as usize - 1),
            "OBJECTIVE{from}'s NAP must target OBJECTIVE{to}"
        );
        assert_eq!(nap.time, *seconds);
    }

    // The world-fact gates, read from the census's own lowering.
    let gates = |index: usize| -> Vec<Condition> {
        let mut out = Vec::new();
        flatten_condition(&program.objectives[index].condition, &mut out);
        out
    };
    for (number, threshold, members) in INACTIVE_GATES {
        assert!(
            gates(*number as usize - 1).iter().any(|gate| matches!(
                gate,
                Condition::InactiveMembers {
                    threshold: t,
                    members: m
                } if *t == *threshold && m.len() == *members
            )),
            "OBJECTIVE{number} must gate on INACTIVE {threshold} of {members}"
        );
    }
    for (number, name, state) in ANIM_GATES {
        assert!(
            gates(*number as usize - 1).iter().any(|gate| matches!(
                gate,
                Condition::AnimationStates { animations, .. }
                    if animations
                        .iter()
                        .any(|(n, s)| n == name && s.code() == *state)
            )),
            "OBJECTIVE{number} must gate on ANIM_STATE {name} {state}"
        );
    }
    for (number, group, remaining) in DEDG_GATES {
        assert!(
            gates(*number as usize - 1).iter().any(|gate| matches!(
                gate,
                Condition::EnemyGroupDepletion {
                    group: g,
                    remaining: r,
                    ..
                } if g == group && r == remaining
            )),
            "OBJECTIVE{number} must gate on DEDG [{group}, {remaining}]"
        );
    }
}

/// Flattens a condition tree to its leaf gates (`All`/`Any`/`Not` structure
/// is the program's own and is not itself a gate).
fn flatten_condition(condition: &Condition, out: &mut Vec<Condition>) {
    match condition {
        Condition::All(parts) | Condition::Any(parts) => {
            for part in parts {
                flatten_condition(part, out);
            }
        }
        Condition::Not(inner) => flatten_condition(inner, out),
        leaf => out.push(leaf.clone()),
    }
}

/// M01's wake graph as JSON: every objective's engine declaration (dormant
/// timers, completion effects, tick dependency), its outcome, its declared
/// stage conditions and the leaf gates of the lowered condition. No original
/// bytes — numbers, names and shapes only.
fn render_wake_graph(
    recovery: &ObjectiveRecovery,
    program: &MissionProgram,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    let mut objectives = Vec::new();
    for number in 1..=recovery.engine_objective_count() as u32 {
        let declaration = recovery
            .engine_declarations()
            .get(&number)
            .cloned()
            .unwrap_or_default();
        let dormant = declaration.begin_dormant.map_or_else(
            || "null".to_owned(),
            |t| {
                format!(
                    "{{\"wake_at\": {:?}, \"nap_after\": {:?}, \"rewake_after\": {:?}, \
                     \"sleep_at\": {:?}}}",
                    t.wake_at, t.nap_after, t.rewake_after, t.sleep_at
                )
            },
        );
        let numbers = |targets: &[usize]| -> String {
            format!(
                "[{}]",
                targets
                    .iter()
                    .map(|index| (index + 1).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let nap = declaration.nap.map_or_else(
            || "null".to_owned(),
            |nap| {
                format!(
                    "{{\"target\": {}, \"time\": {:?}}}",
                    nap.target
                        .map_or_else(|| "null".to_owned(), |t| (t + 1).to_string()),
                    nap.time
                )
            },
        );
        let outcome = recovery
            .outcomes()
            .get(&number)
            .map_or_else(|| "null".to_owned(), |o| jstr(&format!("{o:?}")));
        let stages = recovery
            .stage_conditions()
            .get(&number)
            .map(|conditions| {
                let rows: Vec<String> = conditions
                    .stages()
                    .iter()
                    .map(|(n, path)| {
                        let elements: Vec<String> =
                            path.elements().iter().map(|e| jstr(e)).collect();
                        format!("{{\"n\": {n}, \"path\": [{}]}}", elements.join(", "))
                    })
                    .collect();
                format!(
                    "{{\"stages\": [{}], \"threshold\": {}}}",
                    rows.join(", "),
                    conditions
                        .threshold()
                        .map_or_else(|| "null".to_owned(), |t| t.to_string())
                )
            })
            .unwrap_or_else(|| "null".to_owned());
        let mut gates = Vec::new();
        flatten_condition(
            &program.objectives[number as usize - 1].condition,
            &mut gates,
        );
        let gates: Vec<String> = gates.iter().map(render_gate).collect();
        objectives.push(format!(
            "{{\"number\": {number}, \"index\": {}, \"outcome\": {outcome}, \"begin_dormant\": \
             {dormant}, \"wake\": {}, \"nap\": {nap}, \"kill\": {}, \"sleep\": {}, \
             \"wake_when_i_sleep\": {}, \"tick_depends_on\": {}, \
             \"declared_inactive_stages\": {}, \"inactive_completion_count\": {}, \
             \"stage_conditions\": {stages}, \"gates\": [{}]}}",
            number - 1,
            numbers(declaration.wake.targets()),
            numbers(declaration.kill.targets()),
            numbers(declaration.sleep.targets()),
            numbers(declaration.wake_when_i_sleep.targets()),
            declaration
                .tick_depends_on
                .map_or_else(|| "null".to_owned(), |t| (t + 1).to_string()),
            declaration.declared_inactive_stages,
            declaration
                .inactive_completion_count
                .map_or_else(|| "null".to_owned(), |c| c.to_string()),
            gates.join(", "),
        ));
    }
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"member\": \"{MISSION_DIR} \
         objectives.zrd\", \"objectives\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        objectives.join(",\n  "),
    )
}

/// One leaf gate as JSON.
fn render_gate(condition: &Condition) -> String {
    match condition {
        Condition::Const(value) => format!("{{\"const\": {value}}}"),
        Condition::ObjectiveAwake { index } => {
            format!("{{\"objective_awake\": {index}}}")
        }
        Condition::InactiveMembers { members, threshold } => {
            let chains: Vec<String> = members
                .iter()
                .map(|chain| {
                    format!(
                        "[{}]",
                        chain.iter().map(|e| jstr(e)).collect::<Vec<_>>().join(", ")
                    )
                })
                .collect();
            format!(
                "{{\"inactive_members\": {{\"threshold\": {threshold}, \"members\": [{}]}}}}",
                chains.join(", ")
            )
        }
        Condition::EnemyGroupDepletion {
            group,
            remaining,
            generator,
        } => format!(
            "{{\"enemy_group_depletion\": {{\"group\": {group}, \"remaining\": {remaining}, \
             \"generator\": {}}}}}",
            generator.as_deref().map_or_else(|| "null".to_owned(), jstr)
        ),
        Condition::AnimationStates {
            required,
            animations,
        } => {
            let pairs: Vec<String> = animations
                .iter()
                .map(|(name, state)| format!("[{}, {}]", jstr(name), state.code()))
                .collect();
            format!(
                "{{\"animation_states\": {{\"required\": {required}, \"animations\": [{}]}}}}",
                pairs.join(", ")
            )
        }
        Condition::Travelers {
            subject,
            radius,
            approaching,
            ..
        } => {
            let chain: Vec<String> = subject.iter().map(|e| jstr(e)).collect();
            format!(
                "{{\"travelers\": {{\"subject\": [{}], \"radius\": {radius:?}, \
                 \"approaching\": {approaching}}}}}",
                chain.join(", ")
            )
        }
        other => format!("{{\"variant\": {}}}", jstr(&format!("{other:?}"))),
    }
}

/// The residual unknowns this task inherits from the measured findings.
fn unknown_array() -> String {
    [
        cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED,
        cs_script::ir::DEDG_MEMBER_FIELD_REWRITES,
        "TRAVELERS' world-side producers (the position the proximity gate reads) are off both \
         terminal paths and unproduced today",
        "whether the engine's runtime behaviour matches the measured lifecycle model when run \
         — the evidence is a static record reading plus driven headless runs, never an \
         original capture",
    ]
    .iter()
    .map(|u| jstr(u))
    .collect::<Vec<_>>()
    .join(", ")
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_b_wake.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-B-WAKE` written relative
/// to the workspace root in the module doc must be re-anchored here.
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
    // A Gregorian date rendered from the epoch second, so the report needs no
    // calendar dependency.
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
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of the `accept_m01_b_wake_` tests from
/// a recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test result:`
/// summaries: a summary aggregates every test binary cargo ran, so reading it
/// would report hundreds of unrelated tests as this task's acceptance selection.
/// A prefixed test that was skipped is recorded with the schema's `unknown`
/// status rather than counted as a pass, so a report can never claim an
/// assertion it did not run.
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
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = after[separator + 5..].trim();
            cursor = &after[separator + 5..];
            if !carries_prefix(&name) {
                continue;
            }
            match finished(tail) {
                Some(status) => record(&mut suite, name, status),
                None => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.passed = count(&suite, "pass");
    suite.failed = count(&suite, "fail");
    suite.ignored = count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// Whether a libtest name is one of this task's tests.
///
/// The prefix is matched on the name's **last** path segment, because libtest
/// prints an in-module unit test under its module path. Matching the whole
/// name instead would silently drop every in-module acceptance test from
/// `discovered`, from the assertion list and from the log.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(|segment| segment.starts_with(ACCEPTANCE_PREFIX))
}

/// The libtest result word at the head of a test's tail line.
fn finished(tail: &str) -> Option<&'static str> {
    match tail.split_whitespace().next() {
        Some("ok") => Some("pass"),
        Some("FAILED") => Some("fail"),
        Some("ignored") => Some("unknown"),
        _ => None,
    }
}

fn count(suite: &Suite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, seen)| *seen == status)
        .count() as u64
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 the task
/// implements (the validator re-hashes it with `hashlib` independently).
fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target = evidence_dir.join(&name);
    if source != target {
        fs::copy(source, &target).unwrap_or_else(|error| {
            panic!("copy {} -> {}: {error}", source.display(), target.display())
        });
    }
    let bytes =
        fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
    (
        name,
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
}

// ------------------------------------------------------------- rendering ---

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
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"wake-graph.json\"]}}",
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

/// A JSON string literal: quoted and escaped, so no report field can break out
/// of its string.
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
