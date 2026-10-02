//! Task #374 (Rally): the numbering a PE `RT_STRING` block's units carry.
//!
//! `cs_formats::string_id(block, index) = (block - 1) * 16 + index` is the
//! Win32 string-table numbering: Microsoft's `STRINGTABLE` reference documents
//! that RC allocates 16 strings per section and that strings whose identifiers
//! differ only in the bottom 4 bits share a section, and Win32's string-table
//! sections are resource entries counted from one. The task description
//! proposed the contrary reading `block * 16 + index`, which treats the
//! second-level directory-entry name as the string identifier and names a
//! string one block (16) too high; it is recorded and rejected in
//! `docs/findings/2026-10-02-t374-string-id-numbering.md`.
//!
//! - `accept_string_id_win32_sections_are_one_based` (synthetic, CI) pins the
//!   formula against hard-coded values, including the measured M01-A row.
//! - `accept_string_id_retail_langui_rows_use_the_win32_block_numbering`
//!   (`#[ignore = "requires CS_GAME_DIR"]`, retail) reads the installation's
//!   `langui.dll` through the production PE resource reader and pins the same
//!   numbering against the measured sections. It fails loudly without
//!   `CS_GAME_DIR`.
//! - `evidence_report_t374_writes_the_acceptance_report` is the evidence
//!   harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test.
//!
//! No original string text is read into an assertion or written to Git: the
//! tests assert ids, indices, block entry names and unit counts.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_formats::{
    LANG_ENGLISH_US, ParseContext, PeResources, RT_STRING, STRING_UNITS_PER_BLOCK,
    read_pe_resources, string_id,
};

/// The installed image the task measured: the one whose resource-compiler
/// headers task #368 correlated with its `RT_STRING` blocks.
const LANGUI_DLL: &str = "GOSDATA/ASSETS/BINARIES/langui.dll";

/// The M01-A title row as measured: `RT_STRING` second-level directory entry
/// `218`, index `8`. Under the Win32 numbering its string id is `3480 =
/// (218 - 1) * 16 + 8`; the contrary reading is `3496 = 218 * 16 + 8`.
const M01_A_BLOCK: u16 = 218;
const M01_A_INDEX: u8 = 8;
const M01_A_ID: u32 = 3_480;
const M01_A_CONTRARY_ID: u32 = 3_496;

/// The contiguous identifier run the resource-compiler header names, its
/// sections under the Win32 numbering, and the section the contrary reading
/// would start at. Measured by task #368 and re-read here.
const HEADER_RUN_FIRST: u32 = 40_000;
const HEADER_RUN_LAST: u32 = 40_170;
const HEADER_RUN_SECTIONS: std::ops::RangeInclusive<u16> = 2_501..=2_511;
const CONTRARY_FIRST_SECTION: u16 = 2_500;

// ------------------------------------------------- synthetic acceptance ---

#[test]
fn accept_string_id_win32_sections_are_one_based() {
    assert_eq!(STRING_UNITS_PER_BLOCK, 16, "documented strings per section");

    // Sections are resource entries counted from one: section 1 holds ids
    // 0..=15, section 2 holds 16..=31.
    assert_eq!(string_id(1, 0), 0);
    assert_eq!(string_id(1, 15), 15);
    assert_eq!(string_id(2, 0), 16);
    assert_eq!(string_id(2, 15), 31);
    assert_eq!(string_id(3, 0), 32);

    // The measured M01-A row: hard-coded 3480, never recomputed from the
    // formula, so a change to `string_id` cannot hide behind the reader.
    assert_eq!(string_id(M01_A_BLOCK, M01_A_INDEX), M01_A_ID);
    for index in 0..STRING_UNITS_PER_BLOCK as u8 {
        assert_eq!(
            string_id(M01_A_BLOCK, index),
            3_472 + u32::from(index),
            "block {M01_A_BLOCK} index {index}"
        );
    }

    // The contrary `block * 16 + index` reading is exactly one block high and
    // is not what this function computes.
    assert_eq!(M01_A_CONTRARY_ID, M01_A_ID + 16);
    assert_ne!(string_id(M01_A_BLOCK, M01_A_INDEX), M01_A_CONTRARY_ID);
    assert_ne!(
        string_id(M01_A_BLOCK, M01_A_INDEX),
        u32::from(M01_A_BLOCK) * 16 + u32::from(M01_A_INDEX)
    );

    // The header ids map to the measured sections under the one-based rule,
    // and never to section 2500, which the installation does not have.
    assert_eq!(
        HEADER_RUN_FIRST / 16 + 1,
        u32::from(*HEADER_RUN_SECTIONS.start())
    );
    assert_eq!(
        HEADER_RUN_LAST / 16 + 1,
        u32::from(*HEADER_RUN_SECTIONS.end())
    );
    assert_eq!(
        HEADER_RUN_FIRST / 16,
        u32::from(CONTRARY_FIRST_SECTION),
        "the contrary reading stores id {HEADER_RUN_FIRST} in section \
         {CONTRARY_FIRST_SECTION}"
    );

    // The largest id a `u16` directory entry can name.
    assert_eq!(
        string_id(u16::MAX, STRING_UNITS_PER_BLOCK as u8 - 1),
        1_048_559
    );
}

// ------------------------------------------------------ retail acceptance ---

/// Reads `langui.dll` through the production PE resource reader and pins the
/// Win32 numbering against the installation: the measured M01-A row is
/// `(218 - 1) * 16 + 8 = 3480`, and the header run `40000..=40170` lives in
/// sections `2501..=2511` while `2500` does not exist.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_string_id_retail_langui_rows_use_the_win32_block_numbering() {
    let dir = game_dir();
    let image = read_install_file(&dir, LANGUI_DLL);
    let resources = read_resources(&image, LANGUI_DLL);

    // The measured row's leaf is the task's `[type 6, name 218, language
    // 1033]`, as the production walk reaches it.
    assert!(
        has_leaf(
            &resources,
            [RT_STRING, u32::from(M01_A_BLOCK), LANG_ENGLISH_US]
        ),
        "langui.dll should hold leaf [type {RT_STRING}, name {M01_A_BLOCK}, \
         language {LANG_ENGLISH_US}]"
    );

    // The measured M01-A row, reached through the production reader.
    let block = resources
        .string_block(M01_A_BLOCK)
        .unwrap_or_else(|| panic!("{LANGUI_DLL} has no RT_STRING block {M01_A_BLOCK}"));
    assert_eq!(block.block_id, M01_A_BLOCK, "the block entry names itself");
    assert_eq!(block.language, LANG_ENGLISH_US);
    let unit = block
        .units
        .iter()
        .find(|unit| unit.index == M01_A_INDEX)
        .unwrap_or_else(|| panic!("block {M01_A_BLOCK} has no unit {M01_A_INDEX}"));
    // Hard-coded, not `string_id(block, index)`: only a literal catches a
    // changed formula, because the reader recomputes the same id under test.
    assert_eq!(unit.id, M01_A_ID);
    assert_ne!(unit.id, M01_A_CONTRARY_ID, "not the block*16+index reading");
    assert!(
        !unit.code_units.is_empty(),
        "the measured M01-A unit is a non-empty string"
    );
    assert!(
        unit.text.as_deref().is_some_and(|text| !text.is_empty()),
        "the measured M01-A unit decodes as text"
    );

    // The one-based discriminator: the header run's sections all exist, and
    // the section the contrary reading starts at does not.
    assert!(
        resources.string_block(CONTRARY_FIRST_SECTION).is_none(),
        "section {CONTRARY_FIRST_SECTION} must not exist: id {HEADER_RUN_FIRST} is in section {}",
        HEADER_RUN_FIRST / 16 + 1
    );
    for block_id in HEADER_RUN_SECTIONS {
        let block = resources
            .string_block(block_id)
            .unwrap_or_else(|| panic!("{LANGUI_DLL} should hold section {block_id}"));
        assert_eq!(
            block.units.len(),
            STRING_UNITS_PER_BLOCK,
            "section {block_id} holds a full sixteen units"
        );
    }

    // The first section holds ids 0..=15 under the Win32 numbering; the
    // contrary reading would put unit 0 at id 16.
    if let Some(first) = resources.string_block(1)
        && let Some(unit) = first.units.iter().find(|unit| unit.index == 0)
    {
        assert_eq!(unit.id, 0, "section 1 unit 0 carries id 0");
    }
}

// ------------------------------------------------------- evidence harness ---

/// Evidence-report harness for task #374 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. Run from the workspace root:
///
/// 1. ```sh
///    mkdir -p private/evidence/T374
///    cargo test --workspace --locked -- accept_string_id_ --include-ignored \
///      2>&1 | tee private/evidence/T374/cargo-test.log
///    ```
///    (record the exit status of `cargo test`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T374 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_string_id_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_formats --test string_id -- evidence_report_t374 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T374/acceptance.json \
///      --artifact-root private/evidence/T374 --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T374.json`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t374_writes_the_acceptance_report() {
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
    let game_dir = game_dir();

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
    let suite = parse_suite(&log, "accept_string_id_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_string_id_` tests were recorded in {}",
        log_path.display()
    );
    let retail = "accept_string_id_retail_langui_rows_use_the_win32_block_numbering";
    let status = suite
        .assertions
        .iter()
        .find(|(name, _)| short_name(name) == retail)
        .map(|(_, status)| *status)
        .unwrap_or_else(|| panic!("{retail} did not run: run step 1 with --include-ignored"));
    assert_eq!(status, "pass", "{retail} must pass");

    let found = cs_assets::install::discover(&game_dir)
        .expect("production discovery reads the original installation");
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();

    // The measurement artifact: the production reader over langui.dll, but
    // only ids, indices and counts — never any string text.
    let image = read_install_file(&game_dir, LANGUI_DLL);
    let image_sha256 = cs_assets::install::sha256(&image).to_hex();
    let resources = read_resources(&image, LANGUI_DLL);
    let m01_block = resources
        .string_block(M01_A_BLOCK)
        .expect("the measured M01-A block is present");
    let m01_unit = m01_block
        .units
        .iter()
        .find(|unit| unit.index == M01_A_INDEX)
        .expect("the measured M01-A unit is present");
    let section_rows: Vec<String> = (CONTRARY_FIRST_SECTION..=*HEADER_RUN_SECTIONS.end())
        .map(|block_id| match resources.string_block(block_id) {
            Some(block) => format!(
                "{{\"block_id\": {block_id}, \"present\": true, \"units\": {}, \
                 \"non_empty_units\": {}}}",
                block.units.len(),
                block
                    .units
                    .iter()
                    .filter(|unit| !unit.code_units.is_empty())
                    .count()
            ),
            None => format!("{{\"block_id\": {block_id}, \"present\": false}}"),
        })
        .collect();
    let blocks_path = evidence_dir.join("string-id-langui-blocks.json");
    fs::write(
        &blocks_path,
        format!(
            "{{\n \"task_id\": \"T374\",\n \"candidate_tree\": {},\n \"install_sha256\": {},\n \
             \"image\": {},\n \"image_sha256\": {},\n \"string_blocks\": {},\n \
             \"m01_a_block\": {},\n \"m01_a_index\": {},\n \"m01_a_measured_id\": {},\n \
             \"m01_a_contrary_id\": {},\n \"m01_a_unit_code_units\": {},\n \
             \"m01_a_unit_text_present\": {},\n \"sections\": [\n  {}\n ]\n}}\n",
            jstr(&candidate_tree),
            jstr(&install_sha256),
            jstr(LANGUI_DLL),
            jstr(&image_sha256),
            resources.strings().len(),
            M01_A_BLOCK,
            M01_A_INDEX,
            m01_unit.id,
            M01_A_CONTRARY_ID,
            m01_unit.code_units.len(),
            m01_unit.text.is_some(),
            section_rows.join(",\n  ")
        ),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", blocks_path.display()));

    let artifacts = [artifact(&log_path, "log"), artifact(&blocks_path, "json")];
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T374\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {}}},\n\
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
        jstr(&command_output("rustc", &["--version"])),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d")),
        jstr(&iso_utc_now()),
        argv.iter()
            .map(|arg| jstr(arg))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.passed + suite.failed + suite.ignored,
        suite.passed + suite.failed,
        suite.passed,
        suite.failed,
        suite.ignored,
        suite
            .assertions
            .iter()
            .map(|(name, status)| format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(short_name(name))
            ))
            .collect::<Vec<_>>()
            .join(", "),
        artifacts
            .iter()
            .map(|(name, digest, kind)| format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(
            "implementer: deepseek-1 (Rally #374, DeepSeek V4.1 Flash, session of \
             2026-10-02T03:03Z); reviewer: deepseek-1 — the same agent and model, as the Rally \
             reviewing agent on the review claim of 2026-10-02T03:58Z. The reviewer's context was \
             fresh (a new session that re-read the tree, the installation and the task history), \
             but a fresh context does not make this independent: it is NOT independent review and \
             is not independent original-reference evidence. No agent review replaces the owner's \
             approval. The reviewer re-ran the acceptance suite and regenerated this report on the \
             rebased commit."
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer and re-run \
             by the reviewer on the rebased commit; this harness derives every field from the \
             recorded log, production discovery of $CS_GAME_DIR, the production PE resource reader \
             over the installation's langui.dll (string-id-langui-blocks.json, ids and counts \
             only), rustc and Cargo.lock; validated with \
             tools/validate_evidence.py --require-pass. No original string text is asserted or \
             written. The runtime API the original engine uses to resolve these ids is not \
             observed here and is recorded as a deferred boundary (not a blocker) in \
             docs/findings/2026-10-02-t374-string-id-numbering.md: the numbering itself is \
             settled by Microsoft's documented 16-strings-per-section rule and the measured \
             sections."
        ),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must not validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ------------------------------------------------------------ test inputs ---

/// The root of the read-only original installation, or a loud failure: a
/// retail test must fail, not skip, when `CS_GAME_DIR` is absent.
fn game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this test needs the original installation (capability `retail`)",
    );
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// The bytes of the installation file `spelling` names (a `/`-separated path
/// relative to the installation root), read only, never written.
fn read_install_file(dir: &Path, spelling: &str) -> Vec<u8> {
    let mut path = dir.to_path_buf();
    for segment in spelling.split('/') {
        path.push(segment);
    }
    fs::read(&path)
        .unwrap_or_else(|error| panic!("{spelling}: the installation must hold it: {error}"))
}

/// `image`'s resource tree, read through the production PE resource reader.
fn read_resources(image: &[u8], name: &str) -> PeResources {
    let mut context = ParseContext::with_defaults(name);
    read_pe_resources(&mut context, image)
        .unwrap_or_else(|error| panic!("{name}: the production reader must read it: {error}"))
}

/// Whether `resources` holds the leaf at the numeric path `[type, name,
/// language]`.
fn has_leaf(resources: &PeResources, ids: [u32; 3]) -> bool {
    resources.leaves().iter().any(|leaf| {
        leaf.path.len() == 3
            && leaf
                .path
                .iter()
                .zip(ids)
                .all(|(key, id)| key.id() == Some(id))
    })
}

// ------------------------------------------------------ harness helpers ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_formats/tests/string_id.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a workspace-relative path is re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn command_output(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("{program} runs: {error}"));
    assert!(output.status.success(), "{program} {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git(args: &[&str]) -> String {
    command_output("git", args)
}

/// The locked version of one `Cargo.lock` package.
fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines().map(str::trim) {
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

/// A test name without its module path.
fn short_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

#[derive(Debug, Default)]
struct Suite {
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// The libtest summaries plus the per-test results of the tests whose short
/// name starts with `prefix` in a recorded `cargo test` output.
fn parse_suite(log: &str, prefix: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if let Some(summary) = trimmed.strip_prefix("test result:") {
            for segment in summary.split(';') {
                let words: Vec<&str> = segment.split_whitespace().collect();
                if let Some(pair) = words.windows(2).find(|pair| pair[0].parse::<u64>().is_ok()) {
                    let count: u64 = pair[0].parse().expect("checked");
                    match pair[1] {
                        "passed" => suite.passed += count,
                        "failed" => suite.failed += count,
                        "ignored" => suite.ignored += count,
                        _ => {}
                    }
                }
            }
            continue;
        }
        if !pending.is_empty() && (trimmed == "ok" || trimmed == "FAILED") {
            let name = pending.pop_front().expect("pending test");
            record(
                &mut suite,
                name,
                if trimmed == "ok" { "pass" } else { "fail" },
            );
            continue;
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !short_name(&name).starts_with(prefix) {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if !suite.assertions.iter().any(|(seen, _)| *seen == name) {
        suite.assertions.push((name, status));
    }
}

/// `(file name, sha256, kind)` of one artifact inside the evidence directory.
fn artifact(path: &Path, kind: &str) -> (String, String, String) {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let name = path
        .file_name()
        .expect("artifact file name")
        .to_string_lossy()
        .into_owned();
    (
        name,
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
}

/// A JSON string literal.
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

/// RFC 3339 UTC with whole seconds.
fn iso_utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs() as i64;
    // Howard Hinnant's `civil_from_days`.
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        (rest % 3_600) / 60,
        rest % 60
    )
}
