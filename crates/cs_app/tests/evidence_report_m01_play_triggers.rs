//! Evidence-report harness for the parent task #1439 `M01-PLAY-TRIGGERS`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_play_triggers_*`: it is not part of the acceptance
//! suite and it fails loudly when its inputs are missing, so the task's own
//! test selection can never pick it up as an acceptance test.
//!
//! Run it from the workspace root, after the acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/M01-PLAY-TRIGGERS
//!    cargo test --workspace --locked -- accept_m01_play_triggers_route_ \
//!      --include-ignored --nocapture 2>&1 | tee \
//!      private/evidence/M01-PLAY-TRIGGERS/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`. The one composed retail run costs around a
//!    quarter of an hour on the shared host. `--nocapture` is not decoration:
//!    libtest discards a *passing* test's output, so without it the route's own
//!    progress lines and the `route: settled tick …` line never reach the
//!    recorded log, and the report's tick range would be a number written down
//!    instead of the run's.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_route_ --include-ignored --nocapture" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app --test evidence_report_m01_play_triggers -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/M01-PLAY-TRIGGERS/acceptance.json \
//!      --artifact-root private/evidence/M01-PLAY-TRIGGERS
//!    ```
//!    (`structurally_valid`, exit 0). `--require-pass` is deliberately **not**
//!    used: this report carries the parent's designed readings and its
//!    resolving measurements (owner directive of 2026-09-28 — an unresolved
//!    issue is never deleted from machine-readable evidence to turn that flag
//!    green). The pass/fail facts are carried by `command.exit_code` and the
//!    `tests` counts, which the plain structural validation does check.
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/M01-PLAY-TRIGGERS.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, `Cargo.lock`, and a
//! **second production observation**: the harness surveys M01's own lowered
//! control program through `mission_control::survey_mission_control_programs`
//! and records the measured block map the headless route closes — the one
//! `TRAVELERS` proximity site with its operands, the three `INACTIVE` engine
//! ladders, the three `ANIM_STATE` gates, the two `DEDG` groups and the
//! terminal block's own `Finish` — plus the committed reports of the child
//! tasks this parent integrates, read back as they are. Nothing is typed in by
//! hand. Names, numbers and digests only, never original bytes.
//!
//! # The selection this report describes
//!
//! The parent's acceptance prefix (`accept_m01_play_triggers_`) is a prefix of
//! every child's test names, so the whole family's retail members are one
//! selection. The parent's **own** acceptance member is the headless route
//! (`accept_m01_play_triggers_route_a_scripted_flight_reaches_instantwin`,
//! `#[ignore = "requires CS_GAME_DIR"]`), and that member alone is this
//! report's selection: each child carries its own report under
//! `docs/findings/evidence/`, and they are linked here rather than re-run.
//! The windowed clause of the parent acceptance is #1486's own task
//! (`M01-PLAY-TRIGGERS-WINDOWED`), done and merged with its own report and its
//! own windowed run; this report links that report and claims nothing about
//! the original's windowed behaviour beyond it.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_control::survey_mission_control_programs;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_script::ir::{
    Action, Condition, DirectiveOperation, MissionProgram, TravelersAnchor, Value,
};

/// Every acceptance test the report must see pass. The route member's own
/// prefix: the parent's own acceptance clause is the headless scripted route.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_triggers_route_";
/// The task identity this report is written for.
const TASK_ID: &str = "M01-PLAY-TRIGGERS";
/// M01's mission-scope label, as the census keys its rows.
const SCOPE: &str = "zbd/c1c/m01";
/// `OBJECTIVE3` — the one `TRAVELERS` proximity site.
const PROXIMITY_BLOCK: u32 = 2;
/// The record's measured proximity radius, in world units.
const RADIUS: f64 = 700.0;
/// The measured subject name M01's `TRAVELERS` anchor spells.
const ANCHOR: &str = "workersvoyagezep";
/// `OBJECTIVE6` / `OBJECTIVE8` / `OBJECTIVE9` — the three engine ladders the
/// route completes by taking the eighteen engine parts out of play.
const ENGINE_LADDER_BLOCKS: [u32; 3] = [5, 7, 8];
/// `OBJECTIVE11` / `OBJECTIVE15` / `OBJECTIVE18` — the three `ANIM_STATE`
/// gates the route closes with real landings.
const ANIMATION_GATE_BLOCKS: [u32; 3] = [10, 14, 17];
/// `OBJECTIVE16` / `OBJECTIVE17` — the `DEDG` blocks, with the measured group
/// each one reads.
const DEPLETION_BLOCKS: [(u32, i32); 2] = [(15, 1), (16, 2)];
/// `OBJECTIVE24` — the `INSTANTWIN` block.
const WIN_BLOCK: u32 = 23;
/// The three names the mission program's own `WAKE_ANIM` sites spell; the
/// route's win chain is reachable because a wake-role directive runs on the
/// block's wake transition.
const WAKE_ANIM_NAMES: [&str; 3] = [
    "activate_dropoff_node",
    "activate_pickup_node",
    "pzhomebase",
];
/// The fourth name M01's whole animation closure spells at a `WAKE_ANIM` site
/// outside the mission program (`fadein_bszep`). It is measured by the child
/// task `M01-PLAY-TRIGGERS-ANIMS`, whose report records that it matches carrier
/// record 344 while no member declares it (it is the `NAME1` state of
/// `bswan_gasbags.zrd`), so the mission program the route runs does not carry
/// it. Recorded, never asserted about the program this task's census reads.
const WAKE_ANIM_CLOSURE_NAME: &str = "fadein_bszep";
/// The child tasks this parent integrates, and the committed report each one
/// already carries. Every path is read back and its own claim and counts are
/// recorded: an aggregate report links the artifacts under it.
const CHILD_REPORTS: [(&str, &str); 10] = [
    (
        "M01-PLAY-TRIGGERS-OBSERVE",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-OBSERVE.json",
    ),
    (
        "M01-PLAY-TRIGGERS-PARTS",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-PARTS.json",
    ),
    (
        "M01-PLAY-TRIGGERS-ANIMS",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-ANIMS.json",
    ),
    (
        "M01-PLAY-TRIGGERS-ANIMS-ARM",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-ANIMS-ARM.json",
    ),
    (
        "M01-PLAY-TRIGGERS-ANIMS-LAND",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-ANIMS-LAND.json",
    ),
    (
        "M01-PLAY-TRIGGERS-DEPLETION",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-DEPLETION.json",
    ),
    (
        "M01-PLAY-TRIGGERS-ACTOR-BIT",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT.json",
    ),
    (
        "M01-PLAY-TRIGGERS-TURRET-CLASS",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-TURRET-CLASS.json",
    ),
    (
        "M01-PLAY-TRIGGERS-TURRET-WIRE",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-TURRET-WIRE.json",
    ),
    (
        "M01-PLAY-TRIGGERS-WINDOWED",
        "docs/findings/evidence/M01-PLAY-TRIGGERS-WINDOWED.json",
    ),
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_play_triggers_writes_the_acceptance_report() {
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
    fs::create_dir_all(&evidence_dir)
        .unwrap_or_else(|error| panic!("create {}: {error}", evidence_dir.display()));

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );
    let settle = settle_tick(&log).unwrap_or_else(|| {
        panic!(
            "the acceptance log carries no `route: settled` line: the route member must run with \
             --include-ignored --nocapture and print the tick it settled on"
        )
    });

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: M01's own lowered block map, and the
    // committed reports of the children this parent integrates.
    let observation = observe(&game_dir, &candidate_tree);
    let observation_path = evidence_dir.join("route-observation.json");
    fs::write(
        &observation_path,
        format!(
            "{}\n",
            observation.to_json(&install_sha256, &candidate_tree)
        ),
    )
    .expect("write route-observation.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": {},\n \"candidate_tree\": {},\n \
         \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \
         \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \
         \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {}}},\n \"overrides\": [],\n \
         \"capabilities\": [\"retail\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \
         \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \
         \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": \
         {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(TASK_ID),
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        settle,
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        unknown_array(&observation.unknowns),
        jstr(&reviewer),
        jstr(&review_method(
            &observation,
            suite.discovered as usize,
            settle
        )),
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

/// The mission clock tick the acceptance run's own route settled on, read out
/// of the log line the route member prints. Typed, never written down.
fn settle_tick(log: &str) -> Option<u64> {
    log.lines()
        .filter_map(|line| line.trim().strip_prefix("route: settled tick "))
        .find_map(|rest| rest.split_whitespace().next()?.parse().ok())
}

/// What the second production observation measured.
///
/// Typed values, not pre-rendered strings: this document is an artifact named
/// `route-observation.json`, so every number has to render as JSON (`null`, a
/// number, an array) rather than as Rust's `Debug` (`Some(3)`, which is not
/// JSON and would make the artifact unparseable).
#[derive(Debug, Default)]
struct Observation {
    /// How many numbered blocks M01's lowered program declares.
    blocks: usize,
    /// One row per route beat: `index`, the condition kinds found in it and
    /// the terminal it requests (if any), as JSON.
    beat_rows: Vec<String>,
    /// The one `TRAVELERS` site's operands, as JSON.
    proximity_rows: Vec<String>,
    /// The `INACTIVE` ladders' `(member count, threshold)`, as JSON.
    ladder_rows: Vec<String>,
    /// The `DEDG` sites' `(group, remaining)`, as JSON.
    depletion_rows: Vec<String>,
    /// The `ANIM_STATE` gates' `(block, required, animation count)`, as JSON.
    gate_rows: Vec<String>,
    /// The names the program's own `WAKE_ANIM` sites spell, in program order.
    wake_anim_names: Vec<String>,
    /// The child tasks' committed reports, each as JSON.
    child_rows: Vec<String>,
    /// The absences the report names instead of filling in.
    unknowns: Vec<String>,
}

impl Observation {
    /// The observation as JSON: the measured block map and the child links,
    /// no original bytes.
    fn to_json(&self, install_sha256: &str, candidate_tree: &str) -> String {
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"scope\": {},\n \
             \"blocks\": {},\n \"proximity_radius\": {},\n \"proximity_anchor\": {},\n \
             \"beats\": [\n{}\n ],\n \"proximity\": [\n{}],\n \"inactive_ladders\": [\n{}],\n \
             \"depletions\": [\n{}],\n \"animation_gates\": [\n{}],\n \"wake_anim_names\": {},\n \
             \"wake_anim_closure_name\": {},\n \
             \"children\": [\n{}\n ],\n \"unknowns\": [\n{}\n ],\n \"claim\": \"implemented\",\n \
             \"capabilities\": [\"retail\"]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            jstr(SCOPE),
            self.blocks,
            RADIUS,
            jstr(ANCHOR),
            self.beat_rows.join(",\n"),
            self.proximity_rows.join(",\n"),
            self.ladder_rows.join(",\n"),
            self.depletion_rows.join(",\n"),
            self.gate_rows.join(",\n"),
            str_array(&self.wake_anim_names),
            jstr(WAKE_ANIM_CLOSURE_NAME),
            self.child_rows.join(",\n"),
            self.unknowns
                .iter()
                .map(|entry| jstr(entry))
                .collect::<Vec<_>>()
                .join(",\n"),
        )
    }
}

/// The second production observation: M01's own lowered block map through the
/// production census, and the children's committed reports read back.
fn observe(game_dir: &Path, candidate_tree: &str) -> Observation {
    let census =
        survey_mission_control_programs(game_dir).expect("the census surveys the installation");
    let row = census
        .row(SCOPE)
        .unwrap_or_else(|| panic!("the census measures {SCOPE}: the row must exist"));
    let program: MissionProgram = row
        .lowering_attempt()
        .expect("a measured row carries the lowering attempt")
        .program()
        .expect("every one of M01's sites bound, so the program exists")
        .clone();

    let mut observation = Observation {
        blocks: program.objectives.len(),
        ..Observation::default()
    };
    assert!(
        observation.blocks >= (WIN_BLOCK + 1) as usize,
        "M01's program declares the route's blocks: {} blocks",
        observation.blocks
    );

    // The blocks, keyed by the index every cross-objective directive spells.
    let mut by_index: std::collections::BTreeMap<u32, &cs_script::ir::Objective> =
        std::collections::BTreeMap::new();
    for objective in &program.objectives {
        assert!(
            by_index.insert(objective.id.0, objective).is_none(),
            "block index {} is declared once",
            objective.id.0
        );
    }

    let mut wanted: Vec<u32> = vec![PROXIMITY_BLOCK, WIN_BLOCK];
    wanted.extend(ENGINE_LADDER_BLOCKS);
    wanted.extend(ANIMATION_GATE_BLOCKS);
    wanted.extend(DEPLETION_BLOCKS.iter().map(|(block, _)| *block));

    for block in wanted {
        let objective = by_index.get(&block).unwrap_or_else(|| {
            panic!("M01's lowered program declares block {block}: {by_index:?}")
        });
        let mut kinds: Vec<&'static str> = Vec::new();
        walk_condition(&objective.condition, &mut |condition| {
            let name = condition_kind(condition);
            if !kinds.contains(&name) {
                kinds.push(name);
            }
        });

        let mut proximity: Vec<Proximity> = Vec::new();
        walk_condition(&objective.condition, &mut |condition| {
            if let Condition::Travelers {
                subject,
                anchor,
                radius,
                approaching,
            } = condition
            {
                proximity.push(Proximity {
                    block,
                    subject: subject.clone(),
                    anchor: anchor_text(anchor),
                    radius: *radius,
                    approaching: *approaching,
                });
            }
            if let Condition::InactiveMembers { members, threshold } = condition {
                observation.ladder_rows.push(format!(
                    "  {{\"block\": {block}, \"members\": {}, \"threshold\": {threshold}}}",
                    members.len()
                ));
            }
            if let Condition::EnemyGroupDepletion {
                group, remaining, ..
            } = condition
            {
                observation.depletion_rows.push(format!(
                    "  {{\"block\": {block}, \"group\": {group}, \"remaining\": {remaining}}}"
                ));
            }
            if let Condition::AnimationStates {
                required,
                animations,
            } = condition
            {
                observation.gate_rows.push(format!(
                    "  {{\"block\": {block}, \"required\": {required}, \"animations\": {}}}",
                    animations.len()
                ));
            }
        });

        let mut finish: Option<String> = None;
        let mut wake_anim: Vec<Vec<String>> = Vec::new();
        walk_actions(&objective.actions, &mut |action| {
            if let Action::Finish(outcome) | Action::FinishStandard(outcome) = action
                && finish.is_none()
            {
                finish = Some(format!("{outcome:?}"));
            }
            if let Action::Directive { operation, args } = action
                && *operation == DirectiveOperation::WakeAnimation
            {
                let mut names = Vec::new();
                for value in args {
                    value_strings(value, &mut names);
                }
                wake_anim.push(names);
            }
        });

        if block == PROXIMITY_BLOCK {
            assert_eq!(
                proximity.len(),
                1,
                "M01 has exactly one TRAVELERS site, in block {block}: {proximity:?}"
            );
            let site = &proximity[0];
            assert_eq!(
                site.subject,
                vec!["player".to_owned()],
                "the proximity subject is the record's own spelling"
            );
            assert!(
                site.approaching,
                "the site spells APPROACHING (inside the radius)"
            );
            assert!(
                (site.radius - RADIUS).abs() < f64::EPSILON,
                "the record's proximity radius is {RADIUS}: {}",
                site.radius
            );
            assert_eq!(
                site.anchor,
                format!("[{}]", jstr(ANCHOR)),
                "the proximity anchor is the record's own spelling"
            );
            observation.proximity_rows.push(format!(
                "  {{\"block\": {}, \"subject\": {}, \"anchor\": {}, \"radius\": {}, \
                 \"approaching\": {}}}",
                site.block,
                str_array(&site.subject),
                site.anchor,
                site.radius,
                site.approaching,
            ));
        }
        if ENGINE_LADDER_BLOCKS.contains(&block) {
            assert!(
                kinds.contains(&"InactiveMembers"),
                "block {block} is an INACTIVE engine ladder: {kinds:?}"
            );
        }
        if ANIMATION_GATE_BLOCKS.contains(&block) {
            assert!(
                kinds.contains(&"AnimationStates"),
                "block {block} gates on an ANIM_STATE: {kinds:?}"
            );
        }
        if let Some((_, group)) = DEPLETION_BLOCKS.iter().find(|(index, _)| *index == block) {
            assert!(
                kinds.contains(&"EnemyGroupDepletion"),
                "block {block} reads a DEDG group: {kinds:?}"
            );
            assert!(
                observation
                    .depletion_rows
                    .iter()
                    .any(|row| row.contains(&format!("\"group\": {group}"))),
                "block {block} reads DEDG group {group}: {:?}",
                observation.depletion_rows
            );
        }
        if block == WIN_BLOCK {
            assert_eq!(
                finish.as_deref(),
                Some("Succeeded"),
                "the terminal block requests the record's own INSTANTWIN"
            );
        }
        for names in &wake_anim {
            for name in names {
                if !observation.wake_anim_names.contains(name) {
                    observation.wake_anim_names.push(name.clone());
                }
            }
        }

        observation.beat_rows.push(format!(
            "  {{\"block\": {block}, \"conditions\": {}, \"finish\": {}}}",
            str_array(
                &kinds
                    .iter()
                    .map(|kind| (*kind).to_owned())
                    .collect::<Vec<_>>()
            ),
            finish.as_deref().map_or_else(|| "null".to_owned(), jstr),
        ));
    }

    for name in WAKE_ANIM_NAMES {
        assert!(
            observation.wake_anim_names.iter().any(|seen| seen == name),
            "M01's mission program's own WAKE_ANIM sites spell {name}: {:?}",
            observation.wake_anim_names
        );
    }
    assert!(
        !observation.ladder_rows.is_empty()
            && !observation.gate_rows.is_empty()
            && !observation.depletion_rows.is_empty(),
        "the route's world-shaped blocks are all measured: {:?}",
        observation
    );

    // The children's committed reports, read back: the aggregate link.
    let workspace_root = git(&["rev-parse", "--show-toplevel"]);
    let root = Path::new(&workspace_root);
    // The report is written on the tree of the tested commit; the children's
    // reports are read from that same tree, so the link is to the artifacts of
    // this commit and not of a working tree that has since moved.
    assert!(
        !candidate_tree.is_empty(),
        "the candidate tree names the commit the children's reports are read from"
    );
    for (task, path) in CHILD_REPORTS {
        let text = fs::read_to_string(root.join(path))
            .unwrap_or_else(|error| panic!("read the child report {path}: {error}"));
        let document = parse_json(text.trim())
            .map(|(value, _)| value)
            .unwrap_or_else(|| panic!("{path} is not JSON"));
        assert_eq!(
            document.get_str("task_id"),
            Some(task),
            "{path} is {task}'s own report"
        );
        let claim = document
            .get_str("claim")
            .unwrap_or_else(|| panic!("{path} names a claim"));
        let passed = document
            .get("tests")
            .and_then(|tests| tests.get_number("passed"))
            .unwrap_or(-1.0) as i64;
        let failed = document
            .get("tests")
            .and_then(|tests| tests.get_number("failed"))
            .unwrap_or(-1.0) as i64;
        let artifacts = document
            .get("artifacts")
            .and_then(Json::as_arr)
            .map(<[Json]>::len)
            .unwrap_or(0);
        assert_eq!(failed, 0, "{path} records its acceptance run as passing");
        assert!(
            artifacts > 0,
            "{path} links at least one artifact of its own"
        );
        observation.child_rows.push(format!(
            "  {{\"task_id\": {}, \"report\": {}, \"claim\": {}, \"passed\": {passed}, \
             \"failed\": {failed}, \"artifacts\": {artifacts}}}",
            jstr(task),
            jstr(path),
            jstr(claim),
        ));
    }

    observation.unknowns = vec![
        "the drop-off beat is a designed placeholder (claim m01-dropoff.designed-placeholder): \
         the original plays a cutscene when the wreck is passed a second time low, and the route \
         lands at the record's own armed wv_manual_land instead. Resolving measurement: an owner \
         original capture of the wreck pass. Affected content: OBJECTIVE11's drop-off gate."
            .to_owned(),
        "the tick boundary of a wake-role emission is a designed reading (claim \
         m01-wake-role-emission-tick-designed): the wake-transition timing itself is measured (the \
         engine-image 0x469af0 wake call's own effect list carries WAKE_ANIM, and WAKE_ANIM is not \
         in the 0x46a94c completion pipeline), but this runtime emits each wake-role site on the \
         first tick its block's facts row reads Awake, not inside the wake call. Resolving \
         measurement: the wake call's own call site inside a stepped game loop. Affected content: \
         OBJECTIVE11/OBJECTIVE15/OBJECTIVE18."
            .to_owned(),
        "what starts a spelled CALL_ANIMATION is designed (claim \
         m01-anim.call-animation-activation-designed): the payload's NUL-terminated name is \
         measured over 3 893 events, but the named record is started on the tick its statement \
         publishes by this reimplementation's own rule (the same designed rule the landing \
         path's animation_refs chain states). Whether the original started, queued or replaced a \
         running sibling, and what STOP_ANIMATION/INVALIDATE_ANIMATION do (the same claim id), are \
         unmeasured. Resolving measurement: tie a payload name to its statement's timing in the \
         original. Affected content: OBJECTIVE11's wv_drop_copilot gate."
            .to_owned(),
        "the route is driven on a compressed mission clock (one host step covers 0.5 s) because \
         the win chain's own NAP re-wakes spell 123 s; M01-B-WAKE's drive measures that nothing in \
         the lowered program reads the tick-to-second ratio, so the compressed clock changes only \
         how fast the measured states arrive, never which ones do. The run's wall-clock is not \
         claimed to match the original's. Affected content: the whole route."
            .to_owned(),
        "the route's placements are test placements, not measurements: the body approaches the \
         anchor from above (the drop-off node hangs astern and below it and is armed from the \
         mission's intro records, so a level approach would land there before the gate block is \
         awake), and group 1's four rusdevastator aircraft are despawned rather than shot down \
         (the F27 gun step is the production seam, and the destruction beats' ordinary play \
         belongs to M01-PLAY-AI and M01-PLAY-WEAPONS). Group 2's zero is the measured enabled == 0 \
         spawn skip its own child report records. Affected content: the route's flight and the two \
         DEDG blocks."
            .to_owned(),
        "the windowed clause of the parent acceptance is met by M01-PLAY-TRIGGERS-WINDOWED \
         (#1486), whose own report (docs/findings/evidence/M01-PLAY-TRIGGERS-WINDOWED.json, \
         capability retail,gpu,audio) is linked here: its placement is a derived test placement, \
         not the measured start, and no original executable ran there either. This report claims \
         nothing about the original's windowed behaviour beyond that report."
            .to_owned(),
        "the parent's own selection is its headless route member alone; every child's acceptance \
         assertions are carried by that child's own committed report, linked in this document's \
         children array, and are not re-run into this log."
            .to_owned(),
    ];
    observation.unknowns.sort();
    observation
}

/// One `TRAVELERS` site's own operands.
#[derive(Debug)]
struct Proximity {
    block: u32,
    subject: Vec<String>,
    anchor: String,
    radius: f64,
    approaching: bool,
}

/// Every condition node reachable from `condition`, in declaration order.
fn walk_condition(condition: &Condition, visit: &mut impl FnMut(&Condition)) {
    visit(condition);
    match condition {
        Condition::All(items) | Condition::Any(items) => {
            for item in items {
                walk_condition(item, visit);
            }
        }
        Condition::Not(inner) => walk_condition(inner, visit),
        _ => {}
    }
}

/// Every action reachable from `actions`, following the deferred work items.
fn walk_actions(actions: &[Action], visit: &mut impl FnMut(&Action)) {
    for action in actions {
        visit(action);
        if let Action::Schedule { actions, .. } = action {
            walk_actions(actions, visit);
        }
    }
}

/// A condition's own variant name, for the recorded block map.
fn condition_kind(condition: &Condition) -> &'static str {
    match condition {
        Condition::Const(_) => "Const",
        Condition::Compare { .. } => "Compare",
        Condition::ActorIs { .. } => "ActorIs",
        Condition::ObjectiveAwake { .. } => "ObjectiveAwake",
        Condition::InactiveMembers { .. } => "InactiveMembers",
        Condition::EnemyGroupDepletion { .. } => "EnemyGroupDepletion",
        Condition::Travelers { .. } => "Travelers",
        Condition::TravelersCount { .. } => "TravelersCount",
        Condition::AnimationStates { .. } => "AnimationStates",
        Condition::DangerZoneFlags { .. } => "DangerZoneFlags",
        Condition::Not(_) => "Not",
        Condition::All(_) => "All",
        Condition::Any(_) => "Any",
        Condition::Unknown { .. } => "Unknown",
    }
}

/// The anchor as JSON: an object chain as a quoted array, an explicit point as
/// three numbers.
fn anchor_text(anchor: &TravelersAnchor) -> String {
    match anchor {
        TravelersAnchor::Object(name) => format!(
            "[{}]",
            name.iter()
                .map(|part| jstr(part))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TravelersAnchor::Point(point) => format!("[{}, {}, {}]", point[0], point[1], point[2]),
    }
}

/// Every string a directive argument carries, nested lists flattened.
fn value_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Str(text) => out.push(text.clone()),
        Value::List(items) => {
            for item in items {
                value_strings(item, out);
            }
        }
        _ => {}
    }
}

/// How this run was reviewed, with every measured number interpolated from the
/// observation this same run produced, so a report cannot describe another
/// run's numbers.
///
/// The implementer and reviewer identities are recorded as the facts they are
/// (AGENTS.md review policy). The four slices this parent integrates were
/// implemented by `bunny-alpha-1` (#1480, #1482's halves) and `bunny-alpha-2`
/// (#1481, #1483); the two runtime mechanisms were authored by
/// `Devin SWE-2/swe2-max-1`; the headless route and this report were
/// implemented by `bunny-alpha-2`. The report is regenerated by the reviewer
/// named in `review.identity` (`CS_EVIDENCE_REVIEWER`) on the rebased commit.
/// A reviewer that is the same agent identity as the implementer is a
/// self-review and not independent evidence; when the two differ the
/// reviewer's context is fresh, and a different agent instance or model is
/// preferred for these semantics. Nothing here awards more than `checked`; no
/// agent review replaces the owner's human approval.
fn review_method(observation: &Observation, tests: usize, settle: u64) -> String {
    format!(
        "Regenerated by the reviewing agent on the reviewed and rebased commit, as \
         docs/contracts/CLI-EVIDENCE.md requires: the acceptance member ran locally with the \
         retail capability (one composed retail M01, about a quarter of an hour), and this \
         harness derives every field from the recorded log, production discovery of \
         $CS_GAME_DIR, and a second production observation (route-observation.json) that surveys \
         M01's own lowered control program through \
         mission_control::survey_mission_control_programs. Implementer: bunny-alpha-2 (the headless \
         route, the two runtime mechanisms' evidence and this report; the four slices were \
         bunny-alpha-1 and bunny-alpha-2, and the two runtime mechanisms were authored by \
         Devin SWE-2/swe2-max-1). Reviewer: the identity in review.identity, a later session with \
         a fresh context; when it differs from the implementer the review is by a different agent \
         instance and is not a self-review. Either way no agent review awards more than checked \
         and a different agent instance or model remains preferred for these semantics. Claim is \
         implemented only: no original executable ran and nothing here is verified_original. \
         MEASURED (retail, read-only): M01's lowered program declares {blocks} numbered blocks; the \
         one TRAVELERS site sits in block {PROXIMITY_BLOCK} with subject player, anchor {ANCHOR} \
         and radius {RADIUS}; the route's beats are the three INACTIVE engine ladders {ladders}, \
         the {gates} ANIM_STATE gates, the DEDG blocks {depletions} and the terminal block \
         {WIN_BLOCK}, whose own INSTANTWIN requests Finish(Succeeded); the mission program's \
         WAKE_ANIM sites spell {wake_names}, while the fourth name M01's animation closure spells \
         ({WAKE_ANIM_CLOSURE_NAME}) is not carried by the mission program and is the ANIMS child \
         report's subject. The scripted route settled on tick {settle}. The parent's \
         windowed clause is met by M01-PLAY-TRIGGERS-WINDOWED (#1486); the {children} child \
         reports this document links are read back with their own claims and counts. DESIGNED, \
         each with its claim id and its resolving measurement: the drop-off beat \
         (m01-dropoff.designed-placeholder), the wake-role emission's tick boundary \
         (m01-wake-role-emission-tick-designed, its timing measured) and what starts a spelled \
         CALL_ANIMATION (m01-anim.call-animation-activation-designed). LIMITS: the route's \
         placements and its compressed clock are the test's own, not the original's; the \
         destruction beats' ordinary play belongs to M01-PLAY-AI and M01-PLAY-WEAPONS; no original \
         executable ran. The {tests} assertion(s) discovered under this task's own acceptance \
         prefix are the tests of this report.",
        blocks = observation.blocks,
        ladders = observation.ladder_rows.len(),
        gates = observation.gate_rows.len(),
        depletions = observation.depletion_rows.len(),
        wake_names = str_array(&observation.wake_anim_names),
        children = observation.child_rows.len(),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_play_triggers.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-TRIGGERS` written relative
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

/// Extracts the per-test results of the `accept_m01_play_triggers_route_`
/// tests from a recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test
/// result:` summaries: a summary aggregates every test binary cargo ran, so
/// reading it would report hundreds of unrelated tests as this task's
/// selection. A prefixed test that was skipped is recorded with the schema's
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
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let full = &after[..separator];
            let tail = after[separator + 5..].trim();
            cursor = &after[separator + 5..];
            if !carries_prefix(full) {
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
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

/// Whether a libtest name is one of this task's tests, matched on the name's
/// **last** path segment so an in-module unit test under a module path still
/// counts.
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

/// One referenced artifact: hashed here with the production SHA-256 (the
/// validator re-hashes it with `hashlib` independently).
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
                 \"route-observation.json\"]}}",
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

fn unknown_array(unknowns: &[String]) -> String {
    unknowns
        .iter()
        .map(|entry| jstr(entry))
        .collect::<Vec<_>>()
        .join(", ")
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

// ------------------------------------------------------------ tiny JSON ---

/// Just enough JSON to read the children's committed reports: objects keep
/// their key order, numbers are `f64`, and nothing here is ever written back
/// out.
#[derive(Clone, Debug)]
enum Json {
    Null,
    Bool,
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Obj(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Self::Str(text)) => Some(text),
            _ => None,
        }
    }

    fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Self::Arr(items) => Some(items),
            _ => None,
        }
    }

    fn get_number(&self, key: &str) -> Option<f64> {
        match self.get(key) {
            Some(Self::Num(value)) => Some(*value),
            _ => None,
        }
    }
}

/// Parses one JSON value, returning it and the rest of the input.
fn parse_json(text: &str) -> Option<(Json, &str)> {
    let text = text.trim_start();
    let mut chars = text.chars();
    match chars.next()? {
        '{' => {
            let mut entries = Vec::new();
            let mut rest = chars.as_str().trim_start();
            if let Some(after) = rest.strip_prefix('}') {
                return Some((Json::Obj(entries), after));
            }
            loop {
                let (key, after) = parse_string(rest)?;
                let after = after.trim_start().strip_prefix(':')?.trim_start();
                let (value, after) = parse_json(after)?;
                entries.push((key, value));
                let after = after.trim_start();
                if let Some(after) = after.strip_prefix('}') {
                    return Some((Json::Obj(entries), after));
                }
                rest = after.strip_prefix(',')?.trim_start();
            }
        }
        '[' => {
            let mut items = Vec::new();
            let mut rest = chars.as_str().trim_start();
            if let Some(after) = rest.strip_prefix(']') {
                return Some((Json::Arr(items), after));
            }
            loop {
                let (value, after) = parse_json(rest)?;
                items.push(value);
                let after = after.trim_start();
                if let Some(after) = after.strip_prefix(']') {
                    return Some((Json::Arr(items), after));
                }
                rest = after.strip_prefix(',')?.trim_start();
            }
        }
        '"' => {
            let (value, rest) = parse_string(text)?;
            Some((Json::Str(value), rest))
        }
        't' => text.strip_prefix("true").map(|rest| (Json::Bool, rest)),
        'f' => text.strip_prefix("false").map(|rest| (Json::Bool, rest)),
        'n' => text.strip_prefix("null").map(|rest| (Json::Null, rest)),
        _ => {
            let end = text
                .find(|character: char| {
                    !matches!(character, '0'..='9' | '-' | '+' | '.' | 'e' | 'E')
                })
                .unwrap_or(text.len());
            let (number, rest) = text.split_at(end);
            number
                .parse::<f64>()
                .ok()
                .map(|value| (Json::Num(value), rest))
        }
    }
}

/// Parses a JSON string, returning it unescaped and the rest of the input.
fn parse_string(text: &str) -> Option<(String, &str)> {
    let mut rest = text.strip_prefix('"')?;
    let mut out = String::new();
    loop {
        let mut chars = rest.char_indices();
        let (index, character) = chars.next()?;
        match character {
            '"' => return Some((out, &rest[index + 1..])),
            '\\' => {
                let (escape_index, escape) = chars.next()?;
                let after = &rest[escape_index + escape.len_utf8()..];
                match escape {
                    '"' | '\\' | '/' => out.push(escape),
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let digits = after.get(0..4)?;
                        let code = u32::from_str_radix(digits, 16).ok()?;
                        out.push(char::from_u32(code)?);
                        rest = &after[4..];
                        continue;
                    }
                    other => panic!("the report holds an unknown JSON escape `\\{other}`"),
                }
                rest = after;
            }
            other => {
                out.push(other);
                rest = &rest[index + other.len_utf8()..];
            }
        }
    }
}
