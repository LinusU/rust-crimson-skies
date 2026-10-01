//! The F51-LOCALE-SET evidence harness: it writes
//! `private/evidence/F51-LOCALE-SET/acceptance.json` for the measured
//! supported-locale set and the id-stability answer, plus the derived
//! installation language census beside it.
//!
//! Contract: `docs/contracts/CLI-EVIDENCE.md` § "Report production during
//! trusted gates", schema `schemas/evidence.schema.json`. The task claims the
//! `retail` capability (it reads the owner's installation) and `synthetic` (its
//! unignored half), and it is not a review: it awards no `verified_original` and
//! no `release_approved`.
//!
//! How to run it, in this order, from the workspace root on a clean checkout:
//!
//! ```sh
//! export CS_EVIDENCE_DIR=private/evidence/F51-LOCALE-SET
//! export CS_CANDIDATE_TREE="$(git rev-parse 'HEAD^{tree}')"
//! export CS_GAME_DIR=/path/to/the/original/installation
//! ARGV="cargo test --workspace --locked -- accept_f51_locale_set_ --include-ignored"
//! export CS_EVIDENCE_ARGV="$ARGV"
//! eval "$ARGV" 2>&1 | tee "$CS_EVIDENCE_DIR/cargo-test.log"
//! export CS_EVIDENCE_EXIT_CODE="${pipestatus[1]}"
//! cargo test -p cs_app --test text -- evidence_report_f51_locale_set_ \
//!   --include-ignored --exact
//! python3 tools/validate_evidence.py "$CS_EVIDENCE_DIR/acceptance.json" \
//!   --artifact-root "$CS_EVIDENCE_DIR" --require-pass
//! ```
//!
//! Nothing this harness writes contains original text, a byte of an original
//! file or a screenshot of original content: the census holds paths, sizes,
//! digests, the numeric resource language ids and counts.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::localization::{IdStability, measure_id_stability};

use crate::common::{RETAIL_STRING_IMAGES, measured_locale, retail_locale_measurement};

/// The acceptance tests whose capabilities this report declares: the retail
/// measurement test plus the four synthetic ones, which must be present beside
/// it so the report shows the contract is pinned without original data.
const REQUIRED_TESTS: &[&str] = &[
    "accept_f51_locale_set_measured_resource_languages_derive_the_declared_locale_set",
    "accept_f51_locale_set_the_audit_follows_the_measurement_and_never_a_guessed_locale",
    "accept_f51_locale_set_id_numbering_is_compared_across_two_measured_locales",
    "accept_f51_locale_set_one_measured_locale_reports_no_comparison_rather_than_stability",
    "accept_f51_locale_set_retail_the_installation_declares_only_measured_locales",
];

/// The derived installation language census, written beside the report and
/// referenced by digest: paths, sizes, resource language ids and counts only.
const CENSUS_ARTIFACT: &str = "installation-language-census.json";

/// The prefix this task's acceptance selection uses; the log parser keeps only
/// these tests' results.
const TASK_PREFIX: &str = "accept_f51_locale_set_";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f51_locale_set_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    fs::create_dir_all(&evidence_dir)
        .unwrap_or_else(|error| panic!("create {}: {error}", evidence_dir.display()));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; old \
         reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `{TASK_PREFIX}` tests were recorded in {}",
        log_path.display()
    );
    for required in REQUIRED_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == required)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{required} did not run: F51-LOCALE-SET requires the `retail` capability, run \
                     step 1 with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }

    // `source` hashes describe the real installation, measured by production
    // discovery.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The measurement itself, re-run over that installation: the report's
    // numbers are this run's, not a transcription of a test message.
    let measured = retail_locale_measurement(&game_dir);
    let census = cs_app::text::measure_installation_languages(&game_dir)
        .expect("the installation language census reads");
    let stability =
        measure_id_stability(measured.catalog(0).catalog(), &measured_locale(1033), None);
    let census_path = evidence_dir.join(CENSUS_ARTIFACT);
    fs::write(&census_path, census_json(&measured, &census, &stability))
        .unwrap_or_else(|error| panic!("write {}: {error}", census_path.display()));

    let artifacts = vec![artifact(&log_path, "log"), artifact(&census_path, "json")];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F51-LOCALE-SET\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(&review_identity()),
        jstr(&review_method()),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &document).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F51-LOCALE-SET\"",
        "\"capabilities\": [\"retail\", \"synthetic\"]",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written honestly \
         and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The derived census of the installation's measured resource languages, as
/// JSON: every PE image's path, size, leaf/block counts and numeric resource
/// language ids, the measured localization surface, the declaration derived
/// from it and the id-stability verdict. Counts and ids only — never original
/// text, never a stored byte.
fn census_json(
    measured: &crate::common::RetailLocaleMeasurement,
    census: &cs_app::text::InstallationLanguages,
    stability: &IdStability,
) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!(
        "  \"install_sha256\": {},\n",
        jstr(&measured.install.to_hex())
    ));
    out.push_str(&format!("  \"files\": {},\n", census.files));
    out.push_str(&format!(
        "  \"images_with_resources\": {},\n",
        census.images.len()
    ));
    out.push_str(&format!(
        "  \"images_without_resources\": {},\n",
        census.without_resources.len()
    ));
    out.push_str(&format!("  \"files_not_pe\": {},\n", census.not_pe.len()));

    out.push_str("  \"measured_surface\": [\n");
    let rows: Vec<String> = measured
        .table
        .observations()
        .iter()
        .map(|observation| {
            format!(
                "{{\"container\": {}, \"language\": {}, \"rows\": {}}}",
                jstr(observation.source.container_path()),
                observation.language,
                observation.rows
            )
        })
        .collect();
    out.push_str(&rows.join(",\n"));
    out.push_str("\n  ],\n");

    out.push_str("  \"declared_locales\": [");
    out.push_str(
        &measured
            .declared
            .supported()
            .locales()
            .iter()
            .map(|locale| jstr(locale.as_str()))
            .collect::<Vec<String>>()
            .join(", "),
    );
    out.push_str("],\n");
    out.push_str(&format!(
        "  \"measured_languages\": [{}],\n",
        measured
            .declared
            .languages()
            .iter()
            .map(|language| language.to_string())
            .collect::<Vec<String>>()
            .join(", ")
    ));

    out.push_str("  \"pe_resource_languages\": [\n");
    let images: Vec<String> = census
        .images
        .iter()
        .map(|image| {
            let languages: Vec<String> = image
                .languages
                .iter()
                .map(|language| language.to_string())
                .collect();
            format!(
                "{{\"path\": {}, \"bytes\": {}, \"leaves\": {}, \"string_blocks\": {}, \
                  \"languages\": [{}]}}",
                jstr(&image.path),
                image.bytes,
                image.leaves,
                image.string_blocks,
                languages.join(", ")
            )
        })
        .collect();
    out.push_str(&images.join(",\n"));
    out.push_str("\n  ],\n");

    out.push_str(&format!(
        "  \"string_images\": {},\n",
        str_array(
            &RETAIL_STRING_IMAGES
                .iter()
                .map(|spelling| (*spelling).to_owned())
                .collect::<Vec<String>>()
        )
    ));
    out.push_str(&format!(
        "  \"id_stability\": {}\n",
        match stability {
            IdStability::SingleLocale { measured, ids } => format!(
                "{{\"state\": \"single_locale\", \"locale\": {}, \"ids\": {}}}",
                jstr(measured.as_str()),
                ids
            ),
            IdStability::Compared {
                first,
                second,
                numbering,
            } => format!(
                "{{\"state\": \"compared\", \"first\": {}, \"second\": {}, \"shared\": {}, \
                  \"only_first\": {}, \"only_second\": {}, \"renumbered\": {}, \"changed_text\": {}}}",
                jstr(first.as_str()),
                jstr(second.as_str()),
                numbering.compared(),
                numbering.only_first.len(),
                numbering.only_second.len(),
                numbering.renumbered(),
                numbering.changed()
            ),
        }
    ));
    out.push_str("}\n");
    out
}

fn review_identity() -> String {
    String::from(
        "implementer: bunny-alpha-2/bunny-alpha-2 (opencode, model stealth/space-bunny-alpha, \
         Rally #467, session of 2026-10-01). The measured locale declaration, the installation \
         language census, the id-numbering comparison and every `accept_f51_locale_set_` test were \
         written by the implementer; this report was produced from that same session and is NOT a \
         review. A separate Rally review claim owns the independent check the task requests for \
         locale and format semantics. Neither agent review is independent original-reference \
         evidence and neither replaces the owner's human approval",
    )
}

fn review_method() -> String {
    String::from(
        "the acceptance suite re-run locally with the `retail` capability, the whole \
         `accept_f51_locale_set_` selection together with `--include-ignored`; this harness \
         derives every field from the recorded log, production discovery of $CS_GAME_DIR \
         (`cs_assets::install::discover`), and this run's own measurements: the resource language \
         table and the derived declaration \
         (`cs_app::text::locale_measure::measure_string_image_languages` over the three routed PE \
         string images read by `cs_content::config::StringCatalog::read`, then \
         `cs_content::localization::MeasuredLocales::from_table`) and the installation-wide PE \
         census (`cs_app::text::measure_installation_languages`); validated with \
         tools/validate_evidence.py --require-pass. What one installation cannot answer is the \
         report's own verdict, not a dropped field: the original release's supported-locale list \
         and F12 AC04 id stability across locales need a second, localized installation, so the \
         census records `\"state\": \"single_locale\"` and the unknowns live in \
         docs/findings/2026-10-01-f51-locale-set-measured-locales-and-id-stability.md. `claim` is \
         `implemented` only. `candidate_tree` is the tree of the commit the suite ran on: the only \
         later delta is this report's own copy under docs/findings/evidence/, whose bytes are that \
         file",
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/text/locale_evidence.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F51-LOCALE-SET` written relative to
/// the workspace root in the module doc must be re-anchored here.
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

fn sha256(bytes: &[u8]) -> cs_types::evidence::ContentHash {
    cs_assets::install::sha256(bytes)
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
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `{TASK_PREFIX}` tests from a recorded `cargo test` output.
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
        // A status on its own line completes the earliest test that was started
        // on an earlier line without an inline status.
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
            let full = &after[..separator];
            if !full.contains(TASK_PREFIX) {
                cursor = &after[separator + 5..];
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
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
    suite.assertions.push((name, status));
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
            // The recorded acceptance log is the evidence for every assertion;
            // the census artifact carries the measurement this run derived.
            format!(
                "{{\"id\": {}, \"status\": {}, \"evidence\": [\"cargo-test.log\",                  \"{CENSUS_ARTIFACT}\"]}}",
                jstr(name),
                jstr(status)
            )
        })
        .collect();
    items.join(", ")
}

/// One referenced artifact: hashed here with the production SHA-256 of this
/// workspace (the validator re-hashes it with `hashlib` independently).
fn artifact(source: &Path, kind: &str) -> (String, String, String) {
    let bytes =
        fs::read(source).unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
    let name = source
        .file_name()
        .expect("an artifact has a file name")
        .to_string_lossy()
        .into_owned();
    (name, sha256(&bytes).to_hex(), kind.to_owned())
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    let items: Vec<String> = artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {}, \"kind\": {}}}",
                jstr(name),
                jstr(digest),
                jstr(kind)
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
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat` accepts
/// after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs();
    let (year, month, day, hour, minute, second) = civil_from_unix(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The proleptic Gregorian date of a Unix timestamp (Howard Hinnant's
/// `civil_from_days`), so the harness needs no date dependency.
fn civil_from_unix(seconds: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        m,
        d,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
