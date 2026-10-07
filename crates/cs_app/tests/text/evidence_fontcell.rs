//! Evidence-report harness for task #466 `F51-FONTCELL`
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f51_fontcell_*`: it is not
//! part of the acceptance suite, it fails loudly when its inputs are missing
//! instead of passing vacuously, and the task's test selection must never pick
//! it up as an acceptance test. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F51-FONTCELL
//!    CS_EVIDENCE_DIR=private/evidence/F51-FONTCELL \
//!      cargo test --workspace --locked -- accept_f51_fontcell_ --include-ignored \
//!      2>&1 | tee private/evidence/F51-FONTCELL/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F51-FONTCELL \
//!      CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!      CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f51_fontcell_ --include-ignored" \
//!      CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked -p cs_app --test text evidence_report_f51_fontcell -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F51-FONTCELL/acceptance.json \
//!      --artifact-root private/evidence/F51-FONTCELL --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F51-FONTCELL.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the production
//! bitmap-font scan and localization audit re-run over that installation, and
//! `rustc --version` + `Cargo.lock`. Nothing is typed in by hand.
//!
//! The `capabilities` are checked, not assumed: `retail` is declared only
//! because both `$CS_GAME_DIR` tests in [`RETAIL_TESTS`] appear in the
//! recorded log and passed, and `synthetic` only because the authored-fixture
//! tests in [`SYNTHETIC_TESTS`] did.
//!
//! The report's `unknowns` are *this task's* blockers. The bitmap-font
//! question this task answers is not among them; what the audit still counts
//! on the original installation — the `langui.dll` overflow F51-D measured —
//! is written into the census artifact below rather than dropped, because a
//! working audit may legitimately report incomplete product support. The
//! claim is `implemented`, never `checked` or `verified_original`: static
//! analysis of the executable and a scan of retail files are not a run of the
//! original.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint};

use cs_app::text::original_font::{ORIGINAL_BITMAP_FONT_NAMES, gfont3d_coverage};
use cs_app::text::{GlyphEvidence, measure_rimage_bitmap_fonts};

use crate::common::{RETAIL_FONT_MEDIA, RETAIL_RIMAGE, retail_audit};

/// The acceptance tests whose `retail` capability this report declares: both
/// read `$CS_GAME_DIR`, one through the production texture reader and one
/// through the production localization audit. Both must appear in the
/// recorded log and pass.
const RETAIL_TESTS: &[&str] = &[
    "accept_f51_fontcell_retail_ten_bitmap_fonts_measure_94_cells_each",
    "accept_f51_fontcell_retail_no_media_is_unmeasured_and_every_blocker_names_its_cause",
];

/// The synthetic half of the suite: authored fixtures that prove the cell rule
/// and its refusals without original data. An evidence report that only ever
/// ran capability-bound tests would not show the contract is pinned by CI.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_f51_fontcell_cell_rule_maps_authored_cells_to_characters_in_order",
    "accept_f51_fontcell_an_unmapped_character_is_a_named_audit_blocker_not_a_claim",
    "accept_f51_fontcell_a_declared_font_the_package_lacks_is_reported",
    "accept_f51_fontcell_a_font_image_without_a_stored_colour_key_is_refused",
];

/// The derived font census, written beside the report and referenced by
/// digest: measured numbers and digests only, never a glyph pixel.
const CENSUS_ARTIFACT: &str = "bitmap-font-census.json";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f51_fontcell_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
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
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
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
        "no `accept_f51_fontcell_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed.
    for required in RETAIL_TESTS.iter().chain(SYNTHETIC_TESTS) {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == required)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{required} did not run: F51-FONTCELL requires capabilities `retail` and \
                     `synthetic`, run step 1 with `--include-ignored` and CS_GAME_DIR set"
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

    // The two production observations this task adds, re-run over that
    // installation: the census is this report's own measurement, not a
    // transcription of a test message.
    let census = census(&game_dir, &install_sha256, &candidate_tree);
    let census_path = evidence_dir.join(CENSUS_ARTIFACT);
    fs::write(&census_path, &census)
        .unwrap_or_else(|error| panic!("write {}: {error}", census_path.display()));

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F51-FONTCELL\",\n\
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
        "\"task_id\": \"F51-FONTCELL\"",
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
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ------------------------------------------------------------- census ---

/// The derived font census: every declared font's measured shape, the two
/// loose TGAs' verdict with its recorded evidence, and the audit's remaining
/// blockers with their causes — all re-measured from the installation in this
/// report's own run.
fn census(game_dir: &Path, install_sha256: &str, candidate_tree: &str) -> String {
    let bytes = fs::read(game_dir.join(RETAIL_RIMAGE))
        .unwrap_or_else(|error| panic!("read {RETAIL_RIMAGE}: {error}"));
    let measurement = measure_rimage_bitmap_fonts(&bytes)
        .expect("the production scan measures the retail bitmap fonts");

    let fonts: Vec<String> = ORIGINAL_BITMAP_FONT_NAMES
        .iter()
        .map(|name| {
            let font = measurement
                .font(name)
                .unwrap_or_else(|| panic!("{name} was measured"));
            format!(
                "{{\"name\":{},\"stored_name\":{},\"width\":{},\"height\":{},\"cells\":{},\
                  \"coverage_chars\":{},\"unresolved\":{},\"stray_cells\":{},\
                  \"average_advance\":{}}}",
                jstr(&font.name),
                jstr(&font.stored_name),
                font.width,
                font.height,
                font.cells.len(),
                font.coverage.len(),
                font.unresolved.len(),
                font.stray_cells,
                font.average_advance,
            )
        })
        .collect();
    let missing: Vec<String> = measurement.missing.clone();

    let audit = retail_audit(game_dir);
    let tgas: Vec<String> = RETAIL_FONT_MEDIA
        .iter()
        .map(|path| {
            let media = audit
                .media(path)
                .unwrap_or_else(|| panic!("{path} is audited as media"));
            let reason = media.glyphs.unused_reason().unwrap_or_default();
            format!(
                "{{\"path\":{},\"verdict\":{},\"distributable\":{},\"sha256\":{},\"reason\":{}}}",
                jstr(path),
                jstr(glyph_verdict(&media.glyphs)),
                media.distributable,
                jstr(&media.sha256.to_hex()),
                jstr(reason),
            )
        })
        .collect();
    let rimage = audit
        .media(RETAIL_RIMAGE)
        .expect("rimage.zbd is audited as media");
    let blockers: Vec<String> = audit
        .blockers
        .iter()
        .map(|blocker| {
            format!(
                "{{\"code\":{},\"cause\":{}}}",
                jstr(blocker.code()),
                jstr(&blocker.to_string())
            )
        })
        .collect();

    format!(
        "{{\"schema\":\"cs-f51-fontcell-bitmap-font-census/1\",\"candidate_tree\":{},\
          \"install_sha256\":{},\"rimage_sha256\":{},\"fonts\":[{}],\"missing\":[{}],\
          \"fully_mapped\":{},\"gfont3d_coverage_chars\":{},\"tga_verdicts\":[{}],\
          \"rimage_verdict\":{},\"rimage_sha256_by_audit\":{},\"audit_complete\":{},\
          \"audit_blockers\":[{}]}}",
        jstr(candidate_tree),
        jstr(install_sha256),
        jstr(&sha256(&bytes).to_hex()),
        fonts.join(","),
        str_array(&missing),
        measurement.is_fully_mapped(),
        gfont3d_coverage().len(),
        tgas.join(","),
        jstr(glyph_verdict(&rimage.glyphs)),
        jstr(&rimage.sha256.to_hex()),
        audit.is_complete(),
        blockers.join(","),
    )
}

/// The recorded verdict one media file carries.
fn glyph_verdict(glyphs: &GlyphEvidence) -> &'static str {
    match glyphs {
        GlyphEvidence::Declared { .. } => "declared",
        GlyphEvidence::Unmeasured { .. } => "unmeasured",
        GlyphEvidence::UnusedByOriginal { .. } => "unused_in_original",
        GlyphEvidence::BitmapFonts { .. } => "bitmap_fonts",
    }
}

/// Who ran this report and what they ran it with.
fn review_identity() -> String {
    String::from(
        "implementer: bunny-1/bunny-1 (production code and the first retail measurement, Rally \
         #466 implement claim of 2026-10-06); branch composed onto the task branch, the \
         2026-10-01 supersession decided and all four checks re-run by \
         bunny-alpha-2/bunny-alpha-2 (implement claim of 2026-10-07); reviewer: to \
         be recorded by the Rally review claim. No agent review replaces the owner's human \
         approval, and static code evidence plus a retail scan is never `verified_original`",
    )
}

fn review_method() -> String {
    String::from(
        "the `accept_f51_fontcell_` selection re-run locally with the retail capability, together \
         with `--include-ignored`; this harness derives every field from the recorded log, \
         production discovery of $CS_GAME_DIR, the production bitmap-font scan \
         (`cs_app::text::original_font::measure_rimage_bitmap_fonts` over `ZBD/rimage.zbd` \
         through `cs_formats::texture::read_zbd_textures`) and the production localization audit \
         (`cs_app::text::audit::audit_localization`) re-run over that installation; validated \
         with tools/validate_evidence.py --require-pass. The report's `unknowns` are this task's \
         own blockers and are empty because the acceptance run passed; the product state the \
         audit still reports — the 21 `langui.dll` strings that overflow the declared panel at \
         declared development metrics, F51-D's own measurement — is written into the census \
         artifact with its cause, not dropped, and the two loose TGAs are recorded there as \
         unused by the original (owner static analysis, Rally #466 owner note 2026-10-05). \
         `claim` is `implemented` only. `candidate_tree` is the tree of the commit the suite ran \
         on: the only later delta is this report's own copy under docs/findings/evidence/, whose \
         bytes are that file",
    )
}

// ------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/text/evidence_fontcell.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a relative `private/evidence/...` must be re-anchored here.
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

// --------------------------------------------------------- log parsing ---

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
/// `accept_f51_fontcell_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    const PREFIX: &str = "accept_f51_fontcell_";
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
            let full = &after[..separator];
            if !full.contains(PREFIX) {
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
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ---------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 of this
/// workspace (the validator re-hashes it with `hashlib` independently).
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

// ---------------------------------------------------------- rendering ---

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
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
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
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = era * 400 + year_of_era;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 {
        year_of_day + 1
    } else {
        year_of_day
    };
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
