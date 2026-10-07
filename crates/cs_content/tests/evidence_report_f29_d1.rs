//! Evidence-report harness for task F29-D.1 (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`). Not named `accept_f29_d1_*`: it is
//! not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f29_d1_ --include-ignored 2>&1 |
//!    tee private/evidence/F29-D.1/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F29-D.1 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f29_d1_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test evidence_report_f29_d1 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F29-D.1/acceptance.json
//!    --artifact-root private/evidence/F29-D.1 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F29-D.1.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR`, `rustc` and `Cargo.lock`. `vocabulary.json` is
//! a second, independent run of
//! `cs_content::damage::observe_airframe_damage_vocabulary` — counts, ids,
//! digests, spans and the authored node/texture names the container stores,
//! with no original text beyond those identifiers — so no number in the report
//! is copied from the log it grades.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};

const RETAIL_TESTS: [&str; 1] =
    ["accept_f29_d1_retail_the_installation_names_four_regions_and_eleven_wreck_materials"];

/// The task's unique test prefix (`docs/contracts/CLI-EVIDENCE.md`).
const PREFIX: &str = "accept_f29_d1_";

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES includes retail); this report is derived from the recorded log plus a second, independent production run of cs_content::damage::observe_airframe_damage_vocabulary recorded in vocabulary.json, which re-reads ZBD/planes.zbd through cs_formats::gamez read_gamez_nodes, read_gamez_materials and read_gamez_meshes after checking the bytes against the inventoried per-file digest. MEASURED (identifiers, counts, offsets and digests only): the container install_file/zbd_2f_planes.zbd at SHA-256 45da54a8e1886a8182e84bef03eb5e099481e483d79e458e7db5356538fbc21b; its four damage-region node names, 44 records, each name once per airframe group, in eleven groups whose ancestries cover F11-D2's eleven measured airframes exactly once; every one of those 44 records storing zone_id 255; its eleven _damage materials, each named by exactly one present material record and bound by exactly one airframe subtree's meshes, one per group; the damage-marker names the disclosed selection rule left out; and every span read back out of the container's own bytes. NOT MEASURED, each gating any fidelity claim and kept open in docs/findings/2026-10-07-f29-d1-airframe-damage-vocabulary.md: (1) that a named node is a damage zone, its topology, its parent's role, or how a hit routes to one - zone_id's domain beyond 255 is unrecovered; (2) every integrity, armor, multiplier and overkill value, none of which is recoverable from file access; (3) no original mission event exists for a destroyed or bailed-out plane (F13-C); (4) no production consumer builds a declared airframe graph from installation data yet, so the count refusal is reachable from the acceptance suite only; (5) retail is original-file access, not proof the original executable ran - no original run, no human play, no visual or audible evidence, and nothing here claims verified_original or release_approved. unknowns is empty because the measurement itself has nothing unresolved. Claim is implemented only; the 2026-09-28 owner directive asks for review by a different agent instance or model with a fresh context, whose identity and freshness must be recorded. Validated with tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f29_d1_writes_the_acceptance_report() {
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

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "the acceptance log was not understood: {suite:?}"
    );
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| panic!("{retail_test} did not run: run with --include-ignored"));
        assert_eq!(status, "pass", "{retail_test}");
    }

    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install = fingerprint(&found.manifest);
    let install_sha256 = install.to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // Second observation: the production vocabulary reader, run again here so
    // no number in this report is copied from the log it grades. Only ids,
    // counts, offsets and digests are written out; the container's node and
    // texture names are authored identifiers of the kind findings and tests
    // already pin, and no original text beyond them is recorded.
    let vocabulary = cs_content::damage::observe_airframe_damage_vocabulary(&game_dir)
        .expect("the production reader measures the installation a second time");
    assert_eq!(
        vocabulary.region_count(),
        4,
        "the four measured region names"
    );
    assert_eq!(
        vocabulary.groups().len(),
        11,
        "the eleven measured airframe groups"
    );
    assert_eq!(
        vocabulary.wreck_materials().len(),
        11,
        "the eleven measured materials"
    );
    assert_eq!(
        vocabulary.max_wreck_materials_per_airframe(),
        1,
        "one wreck material per measured airframe"
    );
    let region_names: Vec<String> = vocabulary
        .region_names()
        .iter()
        .map(|name| jstr(name))
        .collect();
    let mut region_records = 0usize;
    let group_items: Vec<String> = vocabulary
        .groups()
        .iter()
        .map(|group| {
            region_records += group.regions.len();
            let ancestry: Vec<String> = group
                .ancestry
                .iter()
                .map(|node| jstr(&node.name))
                .collect();
            let regions: Vec<String> = group
                .regions
                .iter()
                .map(|region| {
                    format!(
                        "{{\"name\": {}, \"node_index\": {}, \"zone_id\": {}, \"offset\": {}, \"length\": {}}}",
                        jstr(&region.name),
                        region.node_index,
                        region.zone_id,
                        region.span.offset(),
                        region.span.length()
                    )
                })
                .collect();
            format!(
                "{{\"parent\": {}, \"ancestry\": [{}], \"regions\": [{}]}}",
                group.parent(),
                ancestry.join(", "),
                regions.join(", ")
            )
        })
        .collect();
    assert_eq!(
        region_records, 44,
        "the four names are stored once per airframe"
    );
    let material_items: Vec<String> = vocabulary
        .wreck_materials()
        .iter()
        .map(|material| {
            let materials: Vec<String> = material
                .material_indices
                .iter()
                .map(u32::to_string)
                .collect();
            let bindings: Vec<String> = material
                .bindings
                .iter()
                .map(|binding| {
                    format!(
                        "{{\"group_parent\": {}, \"node_index\": {}, \"mesh_index\": {}}}",
                        binding.group_parent, binding.node_index, binding.mesh_index
                    )
                })
                .collect();
            format!(
                "{{\"stem\": {}, \"texture_name\": {}, \"texture_index\": {}, \"material_indices\": [{}], \"bindings\": [{}], \"offset\": {}, \"length\": {}}}",
                jstr(&material.stem),
                jstr(&material.texture_name),
                material.texture_index,
                materials.join(", "),
                bindings.join(", "),
                material.span.offset(),
                material.span.length()
            )
        })
        .collect();
    let discarded_items: Vec<String> = vocabulary
        .discarded()
        .iter()
        .map(|entry| {
            format!(
                "{{\"name\": {}, \"occurrences\": {}, \"table\": {}}}",
                jstr(&entry.name),
                entry.occurrences,
                jstr(entry.table.label())
            )
        })
        .collect();
    let origin = vocabulary.origin();
    let provenance = vocabulary.provenance();
    let vocabulary_path = evidence_dir.join("vocabulary.json");
    fs::write(
        &vocabulary_path,
        format!(
            "{{\"install_sha256\": {}, \"container\": {}, \"container_sha256\": {}, \"container_len\": {}, \"origin\": {}, \"provenance_class\": {}, \"provenance_claim\": {}, \"region_names\": [{}], \"region_records\": {}, \"groups\": [{}], \"wreck_materials\": [{}], \"max_wreck_per_airframe\": {}, \"discarded\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&vocabulary.container().to_string()),
            jstr(&vocabulary.container_sha256().to_hex()),
            vocabulary.source().container_len(),
            jstr(origin.label()),
            jstr(provenance.class.label()),
            jstr(provenance.claim_id.as_str()),
            region_names.join(", "),
            region_records,
            group_items.join(", "),
            material_items.join(", "),
            vocabulary.max_wreck_materials_per_airframe(),
            discarded_items.join(", ")
        ),
    )
    .expect("write vocabulary.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&vocabulary_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F29-D.1\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(REVIEW_METHOD),
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
             (crates/cs_content/tests/evidence_report_f29_d1.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F29-D.1` written relative to the
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

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f29_d1_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        // A status on its own line completes the earliest test that was
        // started on an earlier line without an inline status.
        if pending.front().is_some() {
            if trimmed == "ok" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "pass");
                continue;
            }
            if trimmed == "FAILED" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "fail");
                continue;
            }
        }
        // `test <name> ... <status>`, possibly several per interleaved line.
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.starts_with(PREFIX) {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// `(count, kind)` pairs of one `test result:` summary line.
fn summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
        }
    }
    fields
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
    (name, sha256(&bytes).to_hex(), kind.to_owned())
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
                 \"vocabulary.json\"]}}",
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
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
/// accepts after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        m as u32,
        d as u32,
        (rest / 3600) as u32,
        ((rest % 3600) / 60) as u32,
        (rest % 60) as u32,
    )
}
