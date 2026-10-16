//! Evidence-report harness for the parent task `M01-PLAY-TRIGGERS-ANIMS`
//! (Rally #1482): `docs/contracts/CLI-EVIDENCE.md`, schema
//! `schemas/evidence.schema.json`. A member of the campaign binary (see
//! `evidence.rs`), deliberately not named `accept_m01_play_triggers_anims_*`:
//! it is not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_play_triggers_anims_
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-PLAY-TRIGGERS-ANIMS/cargo-test.log` (note the
//!    exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-ANIMS \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_anims_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked --test campaign evidence_report_m01_play_triggers_anims -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-PLAY-TRIGGERS-ANIMS/acceptance.json
//!    --artifact-root private/evidence/M01-PLAY-TRIGGERS-ANIMS`
//! 4. Commit a copy as
//!    `docs/findings/evidence/M01-PLAY-TRIGGERS-ANIMS.json`.
//!
//! The parent's acceptance prefix is a **prefix of its two halves'**:
//! `accept_m01_play_triggers_anims_arm_` and `…_land_` both begin with it, so
//! the selection this harness reads runs the whole chain on one commit — the
//! parent's own two members plus every arm and land assertion. The counts and
//! the assertion list below therefore describe the integration selection, not
//! a subset.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness binds M01's real animation
//! closure (`bind_mission_animation`) and resolves the four names its
//! `WAKE_ANIM` sites spell, and reads M01's real landing table
//! (`read_landings` over `zbd/c1c`) — reporting the measured event → record
//! join and the landing rows as JSON, with no original bytes.
//!
//! `--require-pass` is deliberately **not** used. The report carries the
//! unresolved issues the owner directive of 2026-09-28 says must stay in
//! machine-readable evidence: the designed node-arming surface, the designed
//! landing geometry, the designed window reading, the designed node-position
//! join and the designed call-chain timing; whether a prerequisite gates a
//! call at all (`f20-anim.activation-prerequisite-unevaluated`); the
//! `wv_drop_copilot` disagreement between record 505's stored reference table
//! and its declaration (which leaves `OBJECTIVE11`'s gate uncloseable through
//! the prescribed chain); the `pz` nodes' arming circularity (which leaves
//! `OBJECTIVE18`'s gate unreachable in a composed run); and `fadein_bszep`,
//! which no member declares. Deleting them to turn the flag green is exactly
//! what the directive forbids. The pass/fail facts are carried by
//! `command.exit_code` and the `tests` counts, which the plain structural
//! validation does check.

use super::*;

use cs_app::animation::mission::bind_mission_animation;
use cs_app::animation::survey::CarrierKind;
use cs_app::mission_session::{MissionLandings, read_landings};

/// This task's own acceptance prefix, so the selection cannot credit a
/// sibling task's assertions to this report. The two halves' prefixes extend
/// it, and their tests are part of this chain's selection on purpose.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_triggers_anims_";

/// M01's animation scope, the same one `bind_mission_animation` is asked for
/// elsewhere.
const SCOPE: &str = "zbd/c1c/m01";

/// The world group whose landing table this task reads.
const C1C: &str = "zbd/c1c";

/// The four names M01's own `WAKE_ANIM` sites spell, in the order the record
/// stores them; the join resolves the first three and refuses the fourth.
const WAKE_NAMES: [&str; 4] = [
    "activate_dropoff_node",
    "activate_pickup_node",
    "pzhomebase",
    "fadein_bszep",
];

/// What the second production observation measured.
#[derive(Debug, Default)]
struct Census {
    /// One entry per `WAKE_ANIM` name, as JSON.
    joins: Vec<String>,
    /// One entry per landing-table row, as JSON.
    rows: Vec<String>,
    /// The landing reader's named refusals, as JSON.
    refusals: Vec<String>,
    /// How many of the names bound a playable row.
    bound: usize,
}

impl Census {
    /// The census as JSON: the measured event → record join, the landing rows,
    /// the reader's refusals, no original bytes.
    fn to_json(&self, install_sha256: &str, candidate_tree: &str) -> String {
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"scope\": {},\n \
             \"wake_name_count\": {},\n \"bound_count\": {},\n \"rows_count\": {},\n \
             \"joins\": [\n{}],\n \"rows\": [\n{}],\n \"refusals\": [\n{}],\n \
             \"claim\": \"implemented\",\n \"capabilities\": [\"retail\", \"synthetic\"]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            jstr(SCOPE),
            self.joins.len(),
            self.bound,
            self.rows.len(),
            self.joins.join(",\n"),
            self.rows.join(",\n"),
            self.refusals.join(",\n"),
        )
    }
}

/// The second production observation: bind M01's animation closure and resolve
/// the four `WAKE_ANIM` names, then read M01's landing table.
fn observe(game_dir: &Path) -> Census {
    let binding = bind_mission_animation(game_dir, SCOPE).expect(
        "M01's animation closure reads through \
                                                        production",
    );
    let names = binding.names();
    let mut census = Census::default();
    for name in WAKE_NAMES {
        let row = names
            .row(name)
            .unwrap_or_else(|| panic!("{name} must match a carrier record"));
        let (carrier, index, declaring) = match row.bound_record() {
            Some(record) => {
                let carrier = match record.carrier() {
                    CarrierKind::Mission => "mission",
                    CarrierKind::Camera => "camera",
                };
                let declaring = row
                    .declaration()
                    .resolution()
                    .single()
                    .map(|site| site.member().to_owned())
                    .unwrap_or_else(|| "none".to_owned());
                (carrier.to_owned(), record.index(), declaring)
            }
            None => ("none".to_owned(), usize::MAX, "none".to_owned()),
        };
        let playable = row.is_playable();
        if playable {
            census.bound += 1;
        }
        let duration = row.playback().map_or_else(
            || "null".to_owned(),
            |playback| playback.duration_time().to_string(),
        );
        let record = if index == usize::MAX {
            "null".to_owned()
        } else {
            index.to_string()
        };
        census.joins.push(format!(
            "  {{\"name\": {}, \"carrier\": {}, \"record\": {record}, \"declaring_member\": {}, \
             \"duration\": {duration}, \"playable\": {playable}}}",
            jstr(name),
            jstr(&carrier),
            jstr(&declaring),
        ));
    }

    let table: MissionLandings =
        read_landings(game_dir, C1C).expect("M01's landing table reads through production");
    for row in &table.rows {
        let numbers = |values: &[f64]| {
            let items: Vec<String> = values.iter().map(ToString::to_string).collect();
            format!("[{}]", items.join(", "))
        };
        census.rows.push(format!(
            "  {{\"row\": {}, \"anim\": {}, \"node\": {}, \"angle\": {}, \"speed\": {}, \
             \"auto\": {}}}",
            row.row,
            jstr(&row.anim),
            jstr(&row.node),
            numbers(&row.angle),
            numbers(&row.speed),
            row.auto,
        ));
    }
    census.refusals = table
        .refusals
        .iter()
        .map(|refusal| format!("  {}", jstr(refusal)))
        .collect();
    census
}

/// How this run was reviewed; the measured counts are interpolated from the
/// census this same run produced, so a report cannot describe another run's
/// numbers.
///
/// The implementer and reviewer identities are recorded as the facts they are
/// (AGENTS.md review policy). The dependency halves were implemented by
/// `bunny-alpha-1` (#1498) and `bunny-alpha-2` (#1497); this integration step
/// was implemented by `bunny-alpha-2`. The report is regenerated by the
/// reviewer named in `review.identity` (`CS_EVIDENCE_REVIEWER`) on the rebased
/// commit. A reviewer that is the same agent identity as the implementer is a
/// self-review and not independent evidence; when the two differ the
/// reviewer's context is fresh, and a different agent instance or model is
/// preferred for this evidence machinery and these semantics. Nothing here
/// awards more than `checked`; no agent review replaces the owner's human
/// approval.
fn review_method(census: &Census, tests: usize) -> String {
    format!(
        "Regenerated by the reviewing agent on the reviewed and rebased commit, as \
         docs/contracts/CLI-EVIDENCE.md requires: the acceptance suite ran locally with the \
         retail capability; this harness derives every field from the recorded log, production \
         discovery of $CS_GAME_DIR, and a second production observation \
         (m01-play-triggers-anims-join.json) that binds M01's own animation closure and resolves \
         the four names its WAKE_ANIM sites spell, and reads M01's own landings.zrd, through the \
         production readers. Implementer: bunny-alpha-2 (this integration step; the halves were \
         bunny-alpha-2 #1497 and bunny-alpha-1 #1498). Reviewer: the identity in \
         review.identity, a later session with a fresh context; when it differs from the \
         implementer the review is by a different agent instance and is not a self-review. \
         Either way no agent review awards more than checked and a different agent instance or \
         model remains preferred for this evidence machinery and these semantics. Claim is \
         implemented only: no original executable ran and nothing here is verified_original. \
         MEASURED (retail, read-only): the event to record join resolves {bound} of the four \
         WAKE_ANIM names to a playable carrier record (each a distinct record from its block's \
         ANIM_STATE name), and the landing table holds {rows} rows read verbatim; the reader \
         named {refusals} refusal(s). DESIGNED, each with its claim id and its resolving task: \
         the node-arming surface (m01-anim.object-active-state-arming-designed), what counts as \
         a landing (m01-landing.player-landing-detection-designed), what a row's angle/speed/\
         auto numbers gate (m01-landing.row-window-designed), how a node's world position is \
         joined (m01-landing.node-position-designed), and when a started record's \
         animation_refs fire (m01-landing.call-chain-at-start-designed). LIMITS OF WHAT WAS \
         MEASURED: record 505 (wv_initiate_hookup) stores an animation-reference table of four \
         names while its declaration's own sequence calls eight, so wv_drop_copilot is not \
         reachable through the prescribed chain and OBJECTIVE11's gate is not closed here; the \
         pz nodes arm only through pzhomebase, whose block OBJECTIVE18 is gated by the \
         landing's own output, so rows 1 and 2 cannot fire in a composed run; fadein_bszep \
         matches carrier record 344 but no member declares it (it is a NAME1 state); and \
         whether a prerequisite gates a call at all is unmeasured \
         (f20-anim.activation-prerequisite-unevaluated). All are recorded in the finding with \
         their resolving path. The {tests} assertion(s) discovered under this task's own prefix \
         (the parent's own members plus the arm and land halves, whose prefixes extend it) are \
         the tests of this report.",
        bound = census.bound,
        rows = census.rows.len(),
        refusals = census.refusals.len(),
        tests = tests,
    )
}

/// The acceptance run's own counts, keyed by this task's prefix only.
#[derive(Debug, Default)]
struct ParentSuite {
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
fn parse_parent_suite(log: &str) -> ParentSuite {
    let mut suite = ParentSuite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = parent_status(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            parent_record(&mut suite, name, status);
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
            match parent_status(tail.trim()) {
                Some(status) => parent_record(&mut suite, name, status),
                None => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.passed = parent_count(&suite, "pass");
    suite.failed = parent_count(&suite, "fail");
    suite.ignored = parent_count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// The libtest result word at the head of a test's tail line.
fn parent_status(tail: &str) -> Option<&'static str> {
    match tail.split_whitespace().next() {
        Some("ok") => Some("pass"),
        Some("FAILED") => Some("fail"),
        Some("ignored") => Some("unknown"),
        _ => None,
    }
}

fn parent_count(suite: &ParentSuite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, seen)| *seen == status)
        .count() as u64
}

fn parent_record(suite: &mut ParentSuite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

/// One assertion's entry: both artifacts of this report carry it.
fn parent_assertions(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"m01-play-triggers-anims-join.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn evidence_report_m01_play_triggers_anims_writes_the_acceptance_report() {
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
        "CS_ENGINE_IMAGE must name the owner's decrypted image: the composed retail members \
         build M01's stage, whose player start is only known from it"
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
    let suite = parse_parent_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: the measured join and the rows.
    let census = observe(&game_dir);
    let census_path = evidence_dir.join("m01-play-triggers-anims-join.json");
    fs::write(
        &census_path,
        census.to_json(&install_sha256, &candidate_tree),
    )
    .expect("write m01-play-triggers-anims-join.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    // The unresolved issues stay in machine-readable evidence (owner
    // directive of 2026-09-28): the designed rules the chain runs on and the
    // three gates the measured chain cannot close.
    let unknowns: Vec<String> = vec![
        "the node-arming rule is designed (claim m01-anim.object-active-state-arming-designed): \
         whether an animation's ACTIVATION_PREREQUISITE / OBJECT_ACTIVE_LIST is evaluated, and \
         which node one OBJECT_ACTIVE_STATE statement addresses, are unmeasured; no claim here \
         says the original armed a node from such a statement",
        "what counts as a landing is designed (claim m01-landing.player-landing-detection-\
         designed): the authored half_cone/sphere landing volumes are not decoded as extents, so \
         no measurement sizes the radius or the vertical-speed bound, and no original run was \
         watched",
        "what a row's angle/speed/auto numbers gate is designed (claim m01-landing.row-window-\
         designed): the table's unit, the angle's meaning and auto's switch are unmeasured",
        "how a landing node's world position is joined is designed (claim \
         m01-landing.node-position-designed): the container states its static offset from the \
         anchor actor's node while the mission's zeppelin is a bound actor at zeppelins.zrd's \
         pose; whether the original composed them this way is unmeasured",
        "when a started record's animation_refs fire is designed (claim \
         m01-landing.call-chain-at-start-designed): no payload field is read out of a \
         CALL_ANIMATION event, so a ref cannot be tied to its statement's START_TIME",
        "whether the original evaluated an animation's ACTIVATION_PREREQUISITE / \
         OBJECT_ACTIVE_LIST before a CALL_ANIMATION is unmeasured (claim \
         f20-anim.activation-prerequisite-unevaluated): the designed chain starts each name \
         without evaluating any prerequisite, so a call it starts may in the original have been \
         gated — neither is claimed here",
        "the stored animation-reference table of record 505 (wv_initiate_hookup) names four \
         animations while its declaration's sequence calls eight (among them wv_drop_copilot): \
         OBJECTIVE11's gate wv_drop_copilot RUNNING is therefore not reachable through the \
         prescribed chain, and this task claims neither that it is nor what the original did",
        "under the designed arming surface the pz nodes (pz_manual_land, pz_auto_land) arm only \
         through pzhomebase, whose block OBJECTIVE18 is gated by hooked_to_klondike EXECUTED — \
         the landing's own output, so landing rows 1 and 2 cannot fire in a composed run until \
         the original's node-arming rule is recovered, and OBJECTIVE18's gate is not closed here",
        "fadein_bszep matches carrier record 344 but no member declares it (it is the NAME1 state \
         of bswan_gasbags.zrd), so the WAKE_ANIM join refuses the row as undeclared; whether the \
         original resolved it through that NAME1 state is unmeasured",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-PLAY-TRIGGERS-ANIMS\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": {},\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        parent_assertions(&suite.assertions),
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
