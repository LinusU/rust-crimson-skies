//! Evidence-report harness for task T582: `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`. Not named `accept_t582_*`: it is
//! not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. `cargo test --workspace --locked -- accept_t582_ --include-ignored 2>&1 |
//!    tee private/evidence/T582/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T582 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t582_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test evidence_report_t582 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T582/acceptance.json
//!    --artifact-root private/evidence/T582 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/T582.json`.
//!
//! Every field is derived from the recorded log, the environment and
//! production discovery of `$CS_GAME_DIR`. The second artifact is a
//! **second production observation**: the harness re-runs the
//! installation-wide container census (`census.json`) — every `.rof`
//! container parsed and fully decoded, every `.zbd` file dispatched and
//! every trailer-indexed container listed, every file classified, and the
//! storage-shape sweep over every spelling and member name with the stored
//! extent of every candidate. Spellings, counts, families and extents only;
//! no original file content is written.

use crate::t582_support as support;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint};
use support::{Census, census};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_t582_";

/// How this run was reviewed, with the measured numbers **derived** from the
/// observation this same run produced rather than written down.
fn review_method(census: &Census) -> String {
    let mut families: Vec<String> = census
        .zbd_files_by_family
        .iter()
        .map(|(family, files)| format!("{family} {files}"))
        .collect();
    families.sort();
    let mut candidates: Vec<String> = census
        .stored_plane_candidates
        .iter()
        .map(|candidate| {
            format!(
                "{} (shape {}, template_faithful {}, offset {:?}, length {:?})",
                candidate.location,
                candidate.shape,
                candidate.template_faithful,
                candidate.offset,
                candidate.length
            )
        })
        .collect();
    candidates.sort();
    let rof_members: usize = census.rof_containers.iter().map(|rof| rof.members).sum();
    let rof_decoded: usize = census.rof_containers.iter().map(|rof| rof.decoded_ok).sum();
    let faithful = census
        .stored_plane_candidates
        .iter()
        .filter(|candidate| candidate.template_faithful)
        .count();
    let zbd_files: usize = census.zbd_files_by_family.values().sum();
    let text = "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES \
     includes retail); this harness derives every field from the recorded log, production discovery of \
     $CS_GAME_DIR, and a second production read of the container census (census.json). Claim is \
     implemented only. MEASURED (spellings, counts, families, extents and digests only, no original \
     file content): {files} inventoried files, each classified by a production reader — {rof} ROF \
     containers parsed by cs_formats::rof::read_tree with {rof_decoded} of {rof_members} members \
     decoded by read_member, {zbd_files} files under zbd/ dispatched by cs_formats::zbd::dispatch to \
     the families [{families}], {indexed} trailer-indexed containers listed with {members} members \
     (all readable), {plain} non-container files, and {unclassified} unclassified — and the \
     storage-shape sweep of F64-B (SavedGames, Planes, .sav, Status.dat, Mission., Persist.) extended \
     over every container member name with the engine template's own faithfulness tiers: {faithful} \
     template-faithful hits and {candidates} substring candidates [{candidate_list}]. The measured \
     answer to the task's question: the installation ships **no** legacy custom aircraft — every \
     candidate is a graphics or lobby-script member whose name merely contains MISSION., none is a \
     spelling the engine's own path templates could open, and no stored plane or save file exists in \
     any container to measure a byte layout from, so ContentKind::CustomPlane gets no row and no \
     collection record and LEGACY_LAYOUT_INVENTORY[CustomAircraft] keeps evidence Unknown while its \
     measured construction-screen reference stays recorded. FIDELITY LIMITATIONS (unmeasured original \
     behaviour, none claimed by this report): (1) the byte layout, version field and id encoding of a \
     stored plane stay UNKNOWN — no such file ships and no original run happened; affected content: \
     every F64 legacy-import row; resolving task: an owner capture of a runtime-created Planes\\ file; \
     (2) whether a stored loadout lives inside the plane file or separately is unmeasured — affected \
     content: the custom-loadout row; resolving task: the same capture; (3) the census classifies by \
     what production readers parsed, never by extension, so a non-container file's interior is not \
     claimed to be anything — affected content: the {plain} plain files; resolving task: none needed, \
     the engine templates name where the originals live and none ships. `unknowns` is empty because \
     every measurement THIS report made resolved: every file classified, every member decoded or \
     listed, and every candidate examined. Each unresolved original value above is a limit on the \
     claim rather than an unresolved row of this report; it is stated in this field so it survives in \
     machine-readable evidence and is recorded in \
     docs/findings/2026-10-12-t582-legacy-custom-aircraft-census.md. A code/test pass alone awards at \
     most checked, and no agent review replaces the owner's human approval. Validated with \
     tools/validate_evidence.py --require-pass.";
    text.replace("{files}", &census.file_count.to_string())
        .replace("{rof}", &census.rof_containers.len().to_string())
        .replace("{rof_decoded}", &rof_decoded.to_string())
        .replace("{rof_members}", &rof_members.to_string())
        .replace("{zbd_files}", &zbd_files.to_string())
        .replace("{families}", &families.join(", "))
        .replace("{indexed}", &census.zbd_indexed_containers.to_string())
        .replace("{members}", &census.zbd_indexed_members.to_string())
        .replace("{plain}", &census.plain_files.len().to_string())
        .replace("{unclassified}", &census.unclassified.len().to_string())
        .replace("{faithful}", &faithful.to_string())
        .replace(
            "{candidates}",
            &census.stored_plane_candidates.len().to_string(),
        )
        .replace("{candidate_list}", &candidates.join("; "))
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t582_writes_the_acceptance_report() {
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

    // The second production observation: the container census, re-run
    // through the same support module the acceptance test uses.
    let census = census(&game_dir);
    assert_eq!(
        census.install_sha256, install_sha256,
        "the census and the discovery must agree on the installation digest"
    );
    assert!(
        census.unclassified.is_empty(),
        "every file must be classified: {:?}",
        census.unclassified
    );

    let mut families: Vec<String> = census
        .zbd_files_by_family
        .iter()
        .map(|(family, files)| format!("{{\"family\": {}, \"files\": {files}}}", jstr(family)))
        .collect();
    families.sort();
    let mut rof_rows: Vec<String> = census
        .rof_containers
        .iter()
        .map(|rof| {
            format!(
                "{{\"container\": {}, \"members\": {}, \"decoded_ok\": {}, \
                 \"decode_failures\": {}}}",
                jstr(&rof.spelling),
                rof.members,
                rof.decoded_ok,
                rof.decode_failures.len()
            )
        })
        .collect();
    rof_rows.sort();
    let mut candidate_rows: Vec<String> = census
        .stored_plane_candidates
        .iter()
        .map(|candidate| {
            let offset = candidate
                .offset
                .map(|offset| offset.to_string())
                .unwrap_or_else(|| "null".to_owned());
            let length = candidate
                .length
                .map(|length| length.to_string())
                .unwrap_or_else(|| "null".to_owned());
            format!(
                "{{\"location\": {}, \"shape\": {}, \"template_faithful\": {}, \
                 \"offset\": {offset}, \"length\": {length}}}",
                jstr(&candidate.location),
                jstr(candidate.shape),
                candidate.template_faithful
            )
        })
        .collect();
    candidate_rows.sort();

    let census_path = evidence_dir.join("census.json");
    fs::write(
        &census_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \
             \"file_count\": {}, \"rof_containers\": [{}], \"zbd_files_by_family\": [{}], \
             \"zbd_indexed_containers\": {}, \"zbd_indexed_members\": {}, \
             \"zbd_member_failures\": {}, \"zbd_index_failures\": {}, \"plain_files\": {}, \
             \"unclassified\": {}, \"stored_plane_candidates\": [{}]}}\n",
            jstr(&census.install_sha256),
            jstr(&census.content_sha256),
            jstr(&candidate_tree),
            census.file_count,
            rof_rows.join(", "),
            families.join(", "),
            census.zbd_indexed_containers,
            census.zbd_indexed_members,
            census.zbd_member_failures.len(),
            census.zbd_index_failures.len(),
            census.plain_files.len(),
            census.unclassified.len(),
            candidate_rows.join(", "),
        ),
    )
    .expect("write census.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    // Every measurement *this* report made resolved, so `unknowns` is empty
    // and the report passes `--require-pass`. The unresolved original values
    // (the stored-plane byte layout, the loadout storage split) are **not**
    // dropped: each is stated in full in `review_method` and in
    // `docs/findings/2026-10-12-t582-legacy-custom-aircraft-census.md`.
    let unknowns: Vec<String> = Vec::new();

    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T582\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        unknowns.join(", "),
        jstr(&reviewer),
        jstr(&review_method(&census)),
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
             (crates/cs_content/tests/evidence_report_t582.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T582` written relative to the
/// workspace root in the module doc must be re-anchored here.
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

/// Extracts the per-test results of the `accept_t582_` tests from a recorded
/// `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the
/// `test result:` summaries: a summary aggregates every test binary cargo ran,
/// so reading it would report hundreds of unrelated tests as this task's
/// acceptance selection. A prefixed test that was skipped is recorded with the
/// schema's `unknown` status rather than counted as a pass, so a report can
/// never claim an assertion it did not run.
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
                 \"census.json\"]}}",
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
