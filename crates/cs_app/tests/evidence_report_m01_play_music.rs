//! Evidence-report harness for task `M01-PLAY-MUSIC` (#1463):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_music_*`: it is not part of the acceptance suite
//! and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_music_ --include-ignored
//!    2>&1 | tee private/evidence/M01-PLAY-MUSIC/cargo-test.log` (note the
//!    exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-MUSIC \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_music_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR=<installation> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_play_music -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-PLAY-MUSIC/acceptance.json
//!    --artifact-root private/evidence/M01-PLAY-MUSIC --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-PLAY-MUSIC.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs M01's music cue
//! chain — `recover_retail_objectives` → `MissionRadio::read` →
//! `ObjectiveLifecycles::engine` → the completions that wake each music
//! block → `offer_cues` — and records each served request as
//! `music-census.json`: block numbers, group names, selected slots and
//! members, the labelled container and the owner-ruled `ContentKind::Music`
//! id, plus the director's verdict. That is a real production run over the
//! owner's installation through the very code this task added, not a
//! paraphrase of the acceptance assertions, and it carries no original
//! bytes: names, ids, slots and verdicts only.
//!
//! The test counts and the assertions come from the **prefixed test lines**
//! only, never from the `test result:` summaries: a summary aggregates every
//! test binary cargo ran, so reading it would report hundreds of unrelated
//! tests as this task's acceptance selection.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::audio::{AudioSession, MusicSelector, music_member_id, music_state_spec};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionRadio, ObjectiveLifecycles, RadioLineOutcome, declared_engine_objectives,
    declared_sound_groups, offer_cues,
};
use cs_app::objectives::recover_retail_objectives;
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::scene::SceneGeneration;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;
use cs_sim::audio_events::{AudioAssetSpec, AudioRouter, MusicOutcome};
use cs_types::net::SessionId;

/// Every acceptance test the report must see pass: this task's own prefix,
/// so the selection cannot credit a sibling task's assertions to this
/// report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_music_";

/// The fixed seed the acceptance drive and this census share: the draw
/// sequence is deterministic by construction (the measured `MSVCRT!rand`
/// stream is not the project's — the RNG is the labelled designed part of
/// the port).
const SELECTOR_SEED: u64 = 0;

/// One served request as the census records it.
#[derive(Debug)]
struct Served {
    /// The state M01's block cued.
    group: String,
    /// The slot the state's measured rule picked.
    slot: usize,
    /// The track member that slot names.
    member: String,
    /// The labelled container the member was found in.
    container: String,
    /// The owner-ruled `ContentKind::Music` id the request carried.
    asset: String,
    /// `MusicDirector`'s verdict on the request.
    outcome: String,
}

/// The `MusicOutcome` variant as a short label: the verdict's shape is the
/// claim, not the `Debug` payload it embeds.
fn outcome_label(outcome: &MusicOutcome) -> &'static str {
    match outcome {
        MusicOutcome::Started { .. } => "started",
        MusicOutcome::Transition { .. } => "transition",
        MusicOutcome::Unchanged => "unchanged",
        MusicOutcome::SuppressedDuplicate { .. } => "suppressed_duplicate",
        MusicOutcome::RefusedForeignSession { .. } => "refused_foreign_session",
        MusicOutcome::RefusedNotMusic { .. } => "refused_not_music",
    }
}

/// What the second production observation measured.
#[derive(Debug)]
struct Census {
    /// `(music block index, its cued group, the blocks whose WAKE reaches
    /// it)`, in record order.
    chain: Vec<(usize, String, Vec<usize>)>,
    /// The requests `offer_cues` served, in emission order.
    served: Vec<Served>,
    /// Music-state refusals the offer returned — always empty on a pass.
    refusals: Vec<String>,
}

impl Census {
    /// The census as JSON: ids, slots, names and verdicts, no original
    /// bytes.
    fn to_json(&self, install_sha256: &str, candidate_tree: &str) -> String {
        let chain: Vec<String> = self
            .chain
            .iter()
            .map(|(block, group, wakers)| {
                let wakers: Vec<String> = wakers.iter().map(usize::to_string).collect();
                format!(
                    "  {{\"block\": {block}, \"group\": {}, \"wakers\": [{}]}}\n",
                    jstr(group),
                    wakers.join(", ")
                )
            })
            .collect();
        let served: Vec<String> = self
            .served
            .iter()
            .map(|row| {
                format!(
                    "  {{\"group\": {}, \"slot\": {}, \"member\": {}, \"container\": {}, \
                     \"asset\": {}, \"outcome\": {}}}\n",
                    jstr(&row.group),
                    row.slot,
                    jstr(&row.member),
                    jstr(&row.container),
                    jstr(&row.asset),
                    jstr(&row.outcome),
                )
            })
            .collect();
        let refusals: Vec<String> = self.refusals.iter().map(|r| jstr(r)).collect();
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"selector_seed\": \
             {SELECTOR_SEED},\n \"chain\": [\n{}],\n \"served\": [\n{}],\n \"refusals\": [{}],\n \
             \"claim\": \"implemented\",\n \"capabilities\": [\"retail\", \"synthetic\"]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            chain.join(","),
            served.join(","),
            refusals.join(", "),
        )
    }
}

/// The second production observation: rebuild M01's cue chain through the
/// production readers and re-serve every music state request the
/// declarations emit, recording what the director was asked for and what it
/// answered.
fn observe(game_dir: &Path) -> Census {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new("M01").expect("M01 is a valid mission label"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let mission_dir = plan.mission_dir.clone();
    drop(plan);
    let recovery = recover_retail_objectives(game_dir, &mission_dir)
        .expect("M01's objective record reads through the production recovery");
    let engine = declared_engine_objectives(&recovery)
        .expect("M01's engine lifecycle model builds from its own declarations");
    let radio = MissionRadio::read(game_dir, declared_sound_groups(&engine))
        .expect("M01's sound groups read from sounds.zrd");

    // The music blocks and the blocks whose WAKE list reaches them — the
    // record's own chain, not a transcribed constant.
    let mut music_blocks = BTreeMap::new();
    for index in 0.. {
        let Some(objective) = engine.get(index) else {
            break;
        };
        if let Some(group) = objective
            .declaration
            .wakeup_sound_group
            .as_deref()
            .filter(|group| music_state_spec(group).is_some())
        {
            music_blocks.insert(index, group.to_owned());
        }
    }
    assert!(
        !music_blocks.is_empty(),
        "M01's objective record declares music-state blocks"
    );
    let mut wakers: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0.. {
        let Some(objective) = engine.get(index) else {
            break;
        };
        for target in objective.declaration.wake.targets() {
            if music_blocks.contains_key(target) {
                wakers.entry(*target).or_default().push(index);
            }
        }
    }

    let mut lifecycles = ObjectiveLifecycles::engine(engine);
    let mut music = MusicSelector::launch(SELECTOR_SEED, radio.music_arms());
    let session = SessionId::new(9).expect("a nonzero session");
    let mut audio = AudioSession::new(
        AudioRouter::new(session),
        SceneGeneration(1),
        std::iter::empty::<AudioAssetSpec>(),
    );

    let mut census = Census {
        chain: music_blocks
            .iter()
            .map(|(block, group)| {
                (
                    *block,
                    group.clone(),
                    wakers.get(block).cloned().unwrap_or_default(),
                )
            })
            .collect(),
        served: Vec::new(),
        refusals: Vec::new(),
    };
    let serve = |census: &mut Census,
                 lifecycles: &mut ObjectiveLifecycles,
                 music: &mut MusicSelector,
                 audio: &mut AudioSession| {
        for outcome in offer_cues(&lifecycles.take_cues(), &radio, music, Some(audio)) {
            match outcome {
                RadioLineOutcome::MusicRequested {
                    group,
                    slot,
                    member,
                    asset,
                    outcome,
                } => {
                    let container = radio
                        .music_container(&member)
                        .expect("a served member resolves its container")
                        .to_owned();
                    census.served.push(Served {
                        group,
                        slot,
                        member,
                        container,
                        asset: asset.as_str().to_owned(),
                        outcome: outcome_label(&outcome).to_owned(),
                    });
                }
                RadioLineOutcome::Refused { group, refusal }
                    if matches!(
                        refusal,
                        cs_app::mission_session::RadioRefusal::MusicStateUnknown { .. }
                            | cs_app::mission_session::RadioRefusal::MusicMemberAbsent { .. }
                            | cs_app::mission_session::RadioRefusal::MusicIdentity { .. }
                    ) =>
                {
                    census.refusals.push(format!("{group:?}: {refusal}"));
                }
                _ => {}
            }
        }
    };

    // The opening's own wakes, then the completions whose WAKE effects reach
    // each music block.
    for _ in 0..4 * BASELINE_FIXED_HZ {
        lifecycles.advance(1.0 / f64::from(BASELINE_FIXED_HZ));
        serve(&mut census, &mut lifecycles, &mut music, &mut audio);
    }
    let completers: BTreeSet<usize> = wakers.values().flatten().copied().collect();
    for index in completers {
        lifecycles.complete(index as u32);
        serve(&mut census, &mut lifecycles, &mut music, &mut audio);
    }
    assert!(
        census.refusals.is_empty(),
        "every music state request resolves a track and an identity: {:?}",
        census.refusals
    );
    assert_eq!(
        census.served.len(),
        music_blocks.len(),
        "one request per declared music block: {:?}",
        census.served
    );
    for row in &census.served {
        let spec = music_state_spec(&row.group).expect("a served group is a built-in state");
        assert_eq!(
            row.member,
            spec.tracks[row.slot % spec.tracks.len()],
            "the selected member names the image table's track"
        );
        assert_eq!(
            row.asset,
            music_member_id(&row.container, &row.member)
                .expect("the member derives an id")
                .as_str(),
            "the request carries the owner-ruled identity"
        );
    }
    census
}

/// How this run was reviewed.
fn review_method(census: &Census, tests: u64) -> String {
    let states: BTreeSet<&str> = census.served.iter().map(|row| row.group.as_str()).collect();
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every \
         field from the recorded log, production discovery of $CS_GAME_DIR, and a second \
         production run (music-census.json) that re-executes M01's music cue chain — recover → \
         radio → lifecycle → complete-the-wakers → offer_cues — through the very code this task \
         added. Claim is implemented only, not checked: no independent reviewer has looked at \
         this run, no original executable ran and no person heard anything. MEASURED (retail, \
         read-only): M01 declares {} music-state block(s) over {} distinct state(s) ({:?}), each \
         waking only as a WAKE_OBJECTIVE_WHEN_I_COMPLETE effect; the drive served {} request(s), \
         one per block, each under the owner-ruled ContentKind::Music id over the labelled \
         container's member, and the director answered every one. LIMITS OF WHAT WAS MEASURED: \
         the selection tables and rules are static image evidence (the finding's measured VAs), \
         the container preference is the labelled choice #1438/#1465 made — which of \
         ZBD/soundsh.zbd and ZBD/soundsl.zbd the original opens is unmeasured — and the RNG is \
         the owner's labelled designed choice (the original's MSVCRT!rand stream is not the \
         project's). Which completions the mission's own world-fact gates produce is the \
         mission-model's question, not this task's: the harness substitutes `complete(index)` \
         for the conditions holding, exactly the entry point the host calls. The cue's audibility \
         is the F41-D listening gate and is not claimed. The {tests} assertion(s) discovered \
         under this task's own prefix are the tests of this report. Validated with \
         tools/validate_evidence.py --require-pass.",
        census.chain.len(),
        states.len(),
        states,
        census.served.len(),
        tests = tests,
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_play_music_writes_the_acceptance_report() {
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

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: M01's music cue chain re-served.
    let census = observe(&game_dir);
    let census_path = evidence_dir.join("music-census.json");
    fs::write(
        &census_path,
        census.to_json(&install_sha256, &candidate_tree),
    )
    .expect("write music-census.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-PLAY-MUSIC\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": {SELECTOR_SEED},\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(&census, suite.discovered)),
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
             (crates/cs_app/tests/evidence_report_m01_play_music.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-MUSIC` written relative
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

/// Extracts the per-test results of the `accept_m01_music_` tests from a
/// recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test
/// result:` summaries: a summary aggregates every test binary cargo ran, so
/// reading it would report hundreds of unrelated tests as this task's
/// acceptance selection. A prefixed test that was skipped is recorded with
/// the schema's `unknown` status rather than counted as a pass, so a report
/// can never claim an assertion it did not run.
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
                 \"music-census.json\"]}}",
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
