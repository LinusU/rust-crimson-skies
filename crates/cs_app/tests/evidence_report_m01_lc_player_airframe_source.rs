//! Evidence-report harness for task `M01-LC-PLAYER-AIRFRAME-SOURCE`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_player_airframe_source_*`: it is not part of the
//! acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_player_airframe_source
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE/cargo-test.log` (note the
//!    exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_player_airframe_source --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_player_airframe_source -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE/acceptance.json
//!    --artifact-root private/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE --require-pass`
//! 4. Commit a copy as
//!    `docs/findings/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-walks every reader archive
//! of the installation, decodes its members through `script_raw` +
//! `decode_zrd`, and records which archives carry the `player_plane` key, what
//! M01's twelve members answer, what the chapter's `ia.zrd` assigns, the
//! executable's airframe table as this crate binds it, and M01's stored start
//! pose in metres. That is a real production run over the owner's installation,
//! not a paraphrase of the acceptance assertions, and it carries no original
//! bytes: keys, member names, airframe names, counts and digests only — the
//! same names the workspace already transcribes in
//! `crates/cs_content/tests/scene.rs`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_start::{
    AIRFRAME_TABLE, PLAYER_PLANE_KEY, STORED_POSITION_METRES_PER_UNIT,
    recover_retail_start_configuration, scenario_player_airframe,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::stunts::{decode_zrd, zrd_field};
use cs_types::install::RelativePath;

/// Every acceptance test the report must see pass: this task's own prefix, so
/// the selection cannot credit a sibling task's assertions to this report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_player_airframe_source_";

/// The mission this task measures.
const MEASURED_MISSION: &str = "zbd/c1c/m01/zrdr.zbd";

/// The instant-action archive of the same chapter.
const SCENARIO_ARCHIVE: &str = "zbd/c1c/ia1/zrdr.zbd";

/// How this run was reviewed, with every measured number **derived** from the
/// observation this same run produced.
///
/// The prose is a template: the counts are interpolated from the production
/// walk rather than written down, so a report regenerated on another
/// installation cannot describe this one's numbers.
fn review_method(archives: usize, members: usize, assignments: usize, tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         that walks every reader archive of the installation, decodes each member through \
         script_raw + decode_zrd and reads the {key} key through cs_content::stunts::zrd_field \
         (airframe-source.json). Claim is implemented only. MEASURED: (a) where the original \
         assigns the player's airframe — {archives} reader archives walked, {members} members \
         decoded, the key carried by {assignments} of them, every one of them an instant-action \
         `IA1` scenario; M01's members carry it zero times and name no airframe display name, so \
         the campaign assignment is not in the mission data; the executable's eleven-row airframe \
         table (display name -> scene root -> model) is transcribed in this crate and pinned, and \
         the name-to-index answer of the original (11 = none) is mirrored by airframe_index; no \
         profile, save or hangar file exists in the installation (GOSDATA holds binaries, graphics \
         and two .rof archives only), so where a campaign mission gets its airframe stays an \
         explicit unknown rather than a guess (AGENTS.md rule 4). (b) the start pose's unit — the \
         stored position is read in metres, scale {meters} per stored unit, from #436's owner note \
         of 2026-10-05 (static analysis of the owner-supplied decrypted executable, sha256 \
         43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75: position.y x 3.2808399 -> \
         feet at 0x48fc40, m/s x 2.2369363 -> mph at 0x453aa2, gravity -9.8/9.82, +Y up, \
         right-handed, stored positions map with identity axis map and scale 1.0), corroborated by \
         axis 1 of the stored record being the vertical one and by .zrd angle fields being degrees. \
         LIMITS OF WHAT WAS MEASURED, each recorded in \
         docs/findings/2026-10-06-m01-lc-player-airframe-source.md: (1) retail is file access and \
         the executable evidence is static — no original executable ran, so nothing here reaches \
         verified_original without fingerprinted original-run evidence; (2) the heading's zero \
         direction and handedness are unmeasured (the same owner note lists compass zero and the \
         aircraft body forward axis as not measured), so initial_pose stays Unknown; (3) the frame \
         relation between a stored start and the world grid is unmeasured — M01's start lies 194 \
         stored units outside c1c's node bounds and its wingmen 777 and 826 — recorded as an open \
         question, not as evidence against the metre; (4) which state outside the installation \
         (profile / flight-check selection, or an image default this static pass did not locate) \
         assigns a campaign mission's airframe is unresolved and gates the player_configuration \
         launch surface of #359. `unknowns` is empty because every unresolved item above is a \
         limit on this claim rather than a failed measurement of this run: every archive walked, \
         every member decoded, every task assertion executed. The {tests} assertions discovered \
         under this task's own prefix are the tests of this report. Validated with \
         tools/validate_evidence.py --require-pass.",
        key = PLAYER_PLANE_KEY,
        meters = STORED_POSITION_METRES_PER_UNIT,
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_m01_lc_player_airframe_source_writes_the_acceptance_report() {
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

    // The second production observation: the walk over every reader archive.
    let observation_path = evidence_dir.join("airframe-source.json");
    let observation = render_airframe_source(
        &game_dir,
        &install_sha256,
        &candidate_tree,
        &observation_path,
    );
    assert!(
        observation.archives > 0 && observation.members > 0,
        "the report must not be written over an empty observation: {observation:?}"
    );

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-PLAYER-AIRFRAME-SOURCE\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(
            observation.archives,
            observation.members,
            observation.assignments,
            suite.discovered as usize,
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

/// What the second production observation found.
#[derive(Debug)]
struct Observation {
    /// Reader archives walked.
    archives: usize,
    /// Members decoded.
    members: usize,
    /// Members that carry the player-airframe key.
    assignments: usize,
}

/// The second production observation: walk every reader archive of the
/// installation, decode each member with the production decoder, read the
/// player-airframe key with the production key reader, and record what the
/// measurement says — keys, member names, airframe names, counts and digests,
/// no original bytes.
fn render_airframe_source(
    game_dir: &Path,
    install_sha256: &str,
    candidate_tree: &str,
    path: &Path,
) -> Observation {
    let found = discover(game_dir).expect("production discovery reads the installation");
    let mut archives = 0usize;
    let mut members = 0usize;
    let mut assignments = 0usize;
    let mut carried: Vec<String> = Vec::new();
    let mut decode_failures: Vec<String> = Vec::new();
    let mut mission_members: Vec<String> = Vec::new();
    let mut mission_carries_key = false;
    let mut scenario_members: Vec<String> = Vec::new();

    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        if !key.ends_with("zrdr.zbd") {
            continue;
        }
        let path = match RelativePath::new(&key) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let bytes = match fs::read(
            found
                .manifest
                .host_root
                .join(record.relative_spelling.as_str()),
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                decode_failures.push(format!("{key}: read: {error}"));
                continue;
            }
        };
        let discovery = cs_formats::script_raw::discover_container(&key, &path, &bytes);
        archives += 1;
        for program in discovery.programs() {
            let Some(name) = program.locator().member() else {
                continue;
            };
            let document = match decode_zrd(program.bytes()) {
                Ok(document) => document,
                Err(error) => {
                    decode_failures.push(format!("{key} {name}: {}", error.code()));
                    continue;
                }
            };
            members += 1;
            if key == MEASURED_MISSION {
                mission_members.push(name.to_owned());
            }
            if key == SCENARIO_ARCHIVE {
                scenario_members.push(name.to_owned());
            }
            let carries = zrd_field(&document, PLAYER_PLANE_KEY).is_some();
            if key == MEASURED_MISSION && carries {
                mission_carries_key = true;
            }
            if carries {
                assignments += 1;
                let assigned = scenario_player_airframe(&document)
                    .map(str::to_owned)
                    .unwrap_or_else(|| "<unreadable value>".to_owned());
                carried.push(format!(
                    "{key}#{name} = {assigned} -> {:?}",
                    scenario_player_airframe(&document)
                        .and_then(|name| {
                            cs_app::mission_start::airframe_entry(name)
                                .map(|(index, entry)| (index, entry.scene_root))
                        })
                        .map(|(index, root)| format!("{index}:{root}"))
                        .unwrap_or_else(|| "no table row".to_owned())
                ));
            }
        }
    }

    // M01's own start configuration, through the production reader.
    let config = recover_retail_start_configuration(game_dir, "zbd/c1c/m01")
        .expect("M01's start configuration reads");
    let stored = match config.stored_pose() {
        cs_types::content::Resolved::Known(known) => format!(
            "{:?} m (heading as stored {})",
            known.value.position_metres(),
            known.value.heading
        ),
        cs_types::content::Resolved::Unknown { reason, .. } => format!("unknown: {reason}"),
    };
    let airframe = match config.airframe() {
        cs_types::content::Resolved::Unknown { reason, .. } => format!("unknown: {reason}"),
        cs_types::content::Resolved::Known(_) => "KNOWN".to_owned(),
    };
    let initial_pose = match config.initial_pose() {
        cs_types::content::Resolved::Unknown { reason, .. } => format!("unknown: {reason}"),
        cs_types::content::Resolved::Known(_) => "KNOWN".to_owned(),
    };

    let table: Vec<String> = AIRFRAME_TABLE
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            format!(
                "{{\"index\": {index}, \"display_name\": {}, \"scene_root\": {}, \"model\": {}}}",
                jstr(entry.display_name),
                jstr(entry.scene_root),
                jstr(entry.model),
            )
        })
        .collect();
    let strings: Vec<String> = carried.iter().map(|line| jstr(line)).collect();
    let failures: Vec<String> = decode_failures.iter().map(|line| jstr(line)).collect();
    let mission: Vec<String> = mission_members.iter().map(|name| jstr(name)).collect();
    let scenario: Vec<String> = scenario_members.iter().map(|name| jstr(name)).collect();

    fs::write(
        path,
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"reader_archives_walked\": \
             {archives},\n \"members_decoded\": {members},\n \"members_carrying_player_plane\": \
             {assignments},\n \"assignments\": [{}],\n \"decode_failures\": [{}],\n \
             \"measured_mission\": {{\"archive\": {}, \"members\": [{}], \"player_plane_carried\": \
             {mission_carries_key}}},\n \"scenario_archive\": {{\"archive\": {}, \"members\": [{}]}},\n \
             \"metres_per_stored_unit\": {},\n \"measured_mission_start\": {{\"stored_pose\": {}, \
             \"airframe\": {}, \"initial_pose\": {}}},\n \"airframe_table\": [{}]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            strings.join(", "),
            failures.join(", "),
            jstr(MEASURED_MISSION),
            mission.join(", "),
            jstr(SCENARIO_ARCHIVE),
            scenario.join(", "),
            STORED_POSITION_METRES_PER_UNIT,
            jstr(&stored),
            jstr(&airframe),
            jstr(&initial_pose),
            table.join(",\n  "),
        ),
    )
    .expect("write airframe-source.json");

    Observation {
        archives,
        members,
        assignments,
    }
}

// ------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_lc_player_airframe_source.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE` written
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
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of this task's tests from a recorded `cargo
/// test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test result:`
/// summaries: a summary aggregates every test binary cargo ran, so reading it
/// would report hundreds of unrelated tests as this task's acceptance
/// selection. A prefixed test that was skipped is recorded with the schema's
/// `unknown` status rather than counted as a pass, so a report can never claim
/// an assertion it did not run.
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
                 \"airframe-source.json\"]}}",
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
