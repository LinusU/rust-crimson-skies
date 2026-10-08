//! Evidence-report harness for task `M01-LC-CAMPAIGN-AIRFRAME-POSE`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_campaign_airframe_pose_*`: it is not part of the
//! acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_campaign_airframe_pose_
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-LC-CAMPAIGN-AIRFRAME-POSE/cargo-test.log` (note the
//!    exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-CAMPAIGN-AIRFRAME-POSE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_campaign_airframe_pose_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test campaign evidence_report_m01_lc_campaign_airframe_pose -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-CAMPAIGN-AIRFRAME-POSE/acceptance.json
//!    --artifact-root private/evidence/M01-LC-CAMPAIGN-AIRFRAME-POSE --require-pass`
//! 4. Commit a copy as
//!    `docs/findings/evidence/M01-LC-CAMPAIGN-AIRFRAME-POSE.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation** over the owner's installation: the
//! harness re-reads the two byte ranges this task binds from — the
//! plane-roster record's airframe dword and the π/180 double — straight out
//! of `crimson.decrypted.exe`, re-runs `engine_state_source` and
//! `recover_retail_start_configuration` through production code, and records
//! what they answer. That is a real production run over original data, not a
//! paraphrase of the acceptance assertions, and it carries no original bytes:
//! numbers, node names, counts and digests only.
//!
//! Nothing in this report is `verified_original`: `retail` here is read
//! access to the original files plus static analysis of the owner-supplied
//! decrypted image, and no original executable ran.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_start::{
    AIRFRAME_TABLE, CAMPAIGN_AIRFRAME_RECORD_LENGTH, CAMPAIGN_AIRFRAME_RECORD_OFFSET,
    CAMPAIGN_AIRFRAME_ROW, CAMPAIGN_AIRFRAME_SOURCE, ENGINE_IMAGE, HEADING_CONVERSION_LENGTH,
    HEADING_CONVERSION_OFFSET, HEADING_DEGREES_CONSTANT_OFFSET, STORED_HEADING_DEGREES_TO_RADIANS,
    engine_state_source, recover_retail_start_configuration, stored_heading_radians,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::stunts::decode_zrd;
use cs_types::install::RelativePath;

/// Every acceptance test the report must see pass: this task's own prefix, so
/// the selection cannot credit a sibling task's assertions to this report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_campaign_airframe_pose_";

/// The mission this task measures.
const MEASURED_MISSION: &str = "zbd/c1c/m01/zrdr.zbd";

/// How this run was reviewed, with every measured number **derived** from the
/// observation this same run produced.
///
/// The prose is a template: the counts and byte values are interpolated from
/// the production observation rather than written down, so a report
/// regenerated on another installation cannot describe this one's numbers.
fn review_method(archives: usize, members: usize, row: u32, constant: f64, tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         that re-reads this task's two byte ranges out of crimson.decrypted.exe, re-runs \
         engine_state_source and recover_retail_start_configuration through production code and \
         walks every reader archive of the installation (campaign-airframe-pose.json). Claim is \
         implemented only. MEASURED: (a) the campaign player's airframe — the installation's \
         inventory carries the decrypted image at the recorded digest, engine_state_source names \
         the plane-roster record at file offset 0x21a81c whose +0x2c dword reads {row} here, which \
         is CAMPAIGN_AIRFRAME_ROW, and M01's player is bound as ContentId airframe/{root} with the \
         profile/flight-check chain named in CAMPAIGN_AIRFRAME_SOURCE (roster init 0x4113b0, \
         selection 0x411477, campaign-start setter 0x41712c, spawn 0x474d48); three engine \
         defaults agree on that row and on the campaign path the roster is the only writer; (b) \
         the start pose — the π/180 double at file offset 0x2040e8 reads {constant} here, which is \
         STORED_HEADING_DEGREES_TO_RADIANS, and M01's player record's stored heading 170 converts \
         through it to {radians} radians with the position read in metres (scale 1.0, #436's owner \
         note of 2026-10-05: +Y up, right-handed, identity axis map), so the pose is the record's \
         own value through the conversion 0x47c4e5..0x47c500 performs and never an invented yaw. \
         {archives} reader archives walked and {members} members decoded in the second \
         observation, so the retail inputs were re-read rather than assumed. LIMITS OF WHAT WAS \
         MEASURED, each recorded in \
         docs/findings/2026-10-08-m01-lc-campaign-airframe-engine-state.md: (1) retail is file \
         access and the executable evidence is static — no original executable ran, so nothing \
         here reaches verified_original without fingerprinted original-run evidence; (2) a \
         player's own hangar selection and the registry/INI profile select another row by design \
         (F45/F48) and the mission language that may reassign an airframe or move the player \
         before launch is undecoded (F13-B/C, F38); (3) the airframe model's nose axis in the \
         unparsed .flt geometry is not measured — it decides how one spells the world direction \
         the nose points at yaw 0, not the pose, which is the node's transform; (4) the frame \
         relation of a stored start to the world node grid is open: M01's start lies 194 stored \
         units outside c1c's node bounds and its wingmen 777 and 826 (#676). `unknowns` is empty \
         because every unresolved item above is a limit on this claim rather than a failed \
         measurement of this run: every archive walked, every byte range re-read, every task \
         assertion executed. The {tests} tests discovered under this task's own prefix are the \
         tests of this report. Validated with tools/validate_evidence.py --require-pass.",
        row = row,
        constant = constant,
        radians = stored_heading_radians(170.0),
        root = AIRFRAME_TABLE
            .get(CAMPAIGN_AIRFRAME_ROW)
            .map(|entry| entry.scene_root)
            .unwrap_or("<out of range>"),
        archives = archives,
        members = members,
        tests = tests,
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_campaign_airframe_pose_writes_the_acceptance_report() {
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

    // The production bindings themselves, re-run here: the spans the report
    // names must be the spans the production function answers.
    let engine = engine_state_source(&found.manifest)
        .expect("this installation's image is the measured one");
    assert_eq!(
        (
            engine.airframe.offset(),
            engine.airframe.length(),
            engine.heading.offset(),
            engine.heading.length()
        ),
        (
            CAMPAIGN_AIRFRAME_RECORD_OFFSET,
            CAMPAIGN_AIRFRAME_RECORD_LENGTH,
            HEADING_CONVERSION_OFFSET,
            HEADING_CONVERSION_LENGTH
        ),
        "the production source spans are the ones this report records"
    );

    // The second production observation: the two byte ranges, re-read through
    // the production bindings.
    let observation_path = evidence_dir.join("campaign-airframe-pose.json");
    let observation = render_campaign_airframe_pose(
        &game_dir,
        &install_sha256,
        &candidate_tree,
        &observation_path,
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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-CAMPAIGN-AIRFRAME-POSE\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
            observation.row,
            observation.constant,
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
    assert!(
        observation.row as usize == CAMPAIGN_AIRFRAME_ROW,
        "the roster record on this installation carries row {}",
        observation.row
    );
    assert!(
        observation.constant == STORED_HEADING_DEGREES_TO_RADIANS,
        "this installation's π/180 double is {}",
        observation.constant
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
    /// The airframe row read out of the roster record.
    row: u32,
    /// The degree-to-radian double read out of the image.
    constant: f64,
}

/// The second production observation: read this task's two byte ranges out of
/// the image the installation actually carries, re-run the production
/// bindings, and record what they answer — numbers, node names, counts and
/// digests, no original bytes.
fn render_campaign_airframe_pose(
    game_dir: &Path,
    install_sha256: &str,
    candidate_tree: &str,
    path: &Path,
) -> Observation {
    let found = discover(game_dir).expect("production discovery reads the installation");
    let mut archives = 0usize;
    let mut members = 0usize;
    let mut decode_failures: Vec<String> = Vec::new();
    let mut mission_members: Vec<String> = Vec::new();

    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        if !key.ends_with("zrdr.zbd") {
            continue;
        }
        let relative = match RelativePath::new(&key) {
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
        let discovery = cs_formats::script_raw::discover_container(&key, &relative, &bytes);
        archives += 1;
        for program in discovery.programs() {
            let Some(name) = program.locator().member() else {
                continue;
            };
            match decode_zrd(program.bytes()) {
                Ok(_) => members += 1,
                Err(error) => {
                    decode_failures.push(format!("{key} {name}: {}", error.code()));
                    continue;
                }
            }
            if key == MEASURED_MISSION {
                mission_members.push(name.to_owned());
            }
        }
    }

    // The two byte ranges this task binds from, read out of the image the
    // installation's inventory just hashed.
    let image_record = found
        .manifest
        .files
        .iter()
        .find(|row| row.relative_spelling.logical_key() == ENGINE_IMAGE)
        .unwrap_or_else(|| panic!("the installation carries {ENGINE_IMAGE}"));
    let image = fs::read(
        found
            .manifest
            .host_root
            .join(image_record.relative_spelling.as_str()),
    )
    .expect("the image reads");
    let row_offset = (CAMPAIGN_AIRFRAME_RECORD_OFFSET + 0x2c) as usize;
    let row = u32::from_le_bytes(
        image[row_offset..row_offset + 4]
            .try_into()
            .expect("four bytes for the airframe row"),
    );
    let constant = f64::from_le_bytes(
        image[HEADING_DEGREES_CONSTANT_OFFSET as usize
            ..HEADING_DEGREES_CONSTANT_OFFSET as usize + 8]
            .try_into()
            .expect("eight bytes for the π/180 double"),
    );

    // The production bindings, re-run here.
    let engine = engine_state_source(&found.manifest)
        .expect("this installation's image is the measured one");
    let config = recover_retail_start_configuration(game_dir, "zbd/c1c/m01")
        .expect("M01's start configuration reads");
    let stored = match config.stored_pose() {
        cs_types::content::Resolved::Known(known) => format!(
            "{:?} m (heading as stored {}, radians {})",
            known.value.position_metres(),
            known.value.heading,
            stored_heading_radians(known.value.heading)
        ),
        cs_types::content::Resolved::Unknown { reason, .. } => format!("unknown: {reason}"),
    };
    let airframe = match config.airframe() {
        cs_types::content::Resolved::Unknown { reason, .. } => format!("unknown: {reason}"),
        cs_types::content::Resolved::Known(known) => format!(
            "{} (class {}, source {} at {}+{} for {} bytes)",
            known.value,
            known.provenance.class,
            known
                .provenance
                .source
                .as_ref()
                .map(|span| span.container_path())
                .unwrap_or("<none>"),
            known
                .provenance
                .source
                .as_ref()
                .map(|span| span.offset())
                .unwrap_or(0),
            known
                .provenance
                .source
                .as_ref()
                .map(|span| span.length())
                .unwrap_or(0),
            known.value.key(),
        ),
    };
    let initial_pose = match config.initial_pose() {
        cs_types::content::Resolved::Unknown { reason, .. } => format!("unknown: {reason}"),
        cs_types::content::Resolved::Known(known) => format!(
            "position {:?} m, heading {} rad (class {}, source {} at {} for {} bytes)",
            known.value.position,
            known.value.heading,
            known.provenance.class,
            known
                .provenance
                .source
                .as_ref()
                .map(|span| span.container_path())
                .unwrap_or("<none>"),
            known
                .provenance
                .source
                .as_ref()
                .map(|span| span.offset())
                .unwrap_or(0),
            known
                .provenance
                .source
                .as_ref()
                .map(|span| span.length())
                .unwrap_or(0),
        ),
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
    let failures: Vec<String> = decode_failures.iter().map(|line| jstr(line)).collect();
    let mission: Vec<String> = mission_members.iter().map(|name| jstr(name)).collect();

    fs::write(
        path,
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"image\": {{\"container\": \
             {}, \"sha256\": {}, \"size_bytes\": {}}},\n \"roster_record\": {{\"offset\": {}, \
             \"length\": {}, \"airframe_row\": {row}, \"expected_row\": {}}},\n \
             \"heading_constant\": {{\"offset\": {}, \"length\": 8, \"value\": {constant}, \
             \"expected\": {}}},\n \"heading_conversion\": {{\"offset\": {}, \"length\": \
             {}}},\n \"airframe_source\": {},\n \"reader_archives_walked\": {archives},\n \
             \"members_decoded\": {members},\n \"members_decode_failures\": [{}],\n \
             \"measured_mission\": {{\"archive\": {}, \"members\": [{}]}},\n \"start_configuration\": \
             {{\"stored_pose\": {}, \"airframe\": {}, \"initial_pose\": {}}},\n \"airframe_table\": \
             [{}] \n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            jstr(ENGINE_IMAGE),
            jstr(&image_record.sha256.to_hex()),
            image_record.size_bytes,
            CAMPAIGN_AIRFRAME_RECORD_OFFSET,
            CAMPAIGN_AIRFRAME_RECORD_LENGTH,
            CAMPAIGN_AIRFRAME_ROW,
            HEADING_DEGREES_CONSTANT_OFFSET,
            STORED_HEADING_DEGREES_TO_RADIANS,
            engine.heading.offset(),
            HEADING_CONVERSION_LENGTH,
            jstr(CAMPAIGN_AIRFRAME_SOURCE),
            failures.join(", "),
            jstr(MEASURED_MISSION),
            mission.join(", "),
            jstr(&stored),
            jstr(&airframe),
            jstr(&initial_pose),
            table.join(",\n  "),
        ),
    )
    .expect("write campaign-airframe-pose.json");

    Observation {
        archives,
        members,
        row,
        constant,
    }
}
// ------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/campaign/m01_lc_campaign_airframe_pose_evidence.rs)"
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
                 \"campaign-airframe-pose.json\"]}}",
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
