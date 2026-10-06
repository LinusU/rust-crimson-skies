//! Evidence-report harness for task #693 (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_t693_*`: it is not part of
//! the acceptance suite, and it fails loudly when its inputs are missing
//! instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_t693_ --include-ignored \
//!      2>&1 | tee private/evidence/T693/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; with `pipefail`, or by checking the
//!    first command's status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T693 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t693_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test evidence_report_t693 -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/T693/acceptance.json \
//!      --artifact-root private/evidence/T693
//!    ```
//!    **without** `--require-pass`: the report's `unknowns` are the task's
//!    honest residue (section G of
//!    `docs/findings/2026-10-06-t693-metaopenfile-name-matching.md`), not
//!    failed assertions, and `--require-pass` rejects any report carrying an
//!    unresolved issue — removing them to satisfy it would state that this
//!    task knows things it does not. The acceptance run itself must still be
//!    green (exit 0, all discovered tests passing), which the harness asserts.
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T693.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded
//! test log, the environment, production discovery of `$CS_GAME_DIR`, the
//! production GOS chain of that installation (`pairs.json`), `rustc --version`
//! and `Cargo.lock`. Nothing is typed in by hand, and the report describes the
//! actual execution — a failing run produces a failing report, which the
//! validator rejects.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::vfs::{
    ContentSession, ExePathOrigin, GosChain, GosInstall, GosNameMatch, GosSource, MAIN_MOUNT_ID,
    SessionBuilder, gos_key,
};
use cs_types::asset_id::ResolveContext;

/// The acceptance prefix of this task.
const _PREFIX: &str = "accept_t693_";

/// The one retail test this task's acceptance suite consists of. The report
/// may only declare `retail` when it is in the log and it passed.
const RETAIL_TEST: &str =
    "accept_t693_metaopenfile_name_matching_retail_pairs_and_container_spelling";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t693_writes_the_acceptance_report() {
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

    // The candidate tree must be the tree that was actually tested: a stale
    // report from another commit is exactly what this check refuses.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );
    assert!(
        (candidate_tree.len() == 40 || candidate_tree.len() == 64)
            && candidate_tree
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "CS_CANDIDATE_TREE must be a hex Git tree id, got {candidate_tree:?}"
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
        (suite.assertions.len() as u64) >= suite.passed,
        "fewer per-test results than passing tests were parsed from {} — the log format was \
         not understood; inspect it rather than reporting guessed counts",
        log_path.display()
    );
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_t693_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: the report declares
    // `retail` only because the retail acceptance test is in this log and
    // passed, and the retail capability is the only one this task uses.
    let status = suite
        .assertions
        .iter()
        .find(|(name, _)| name == RETAIL_TEST)
        .map(|(_, status)| *status)
        .unwrap_or_else(|| {
            panic!(
                "{RETAIL_TEST} did not run: #693 requires capability `retail`, run step 1 with \
                 `--include-ignored` and CS_GAME_DIR set"
            )
        });
    assert_eq!(
        status, "pass",
        "{RETAIL_TEST} must pass; got status {status}"
    );
    assert!(
        suite
            .assertions
            .iter()
            .all(|(name, _)| name.starts_with("accept_t693_")),
        "only this task's tests may appear as its assertions"
    );

    // `source` hashes describe the real installation, measured by the very
    // production code this task exercises.
    let found = install::discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    // The measured answer, produced by the production GOS chain of this
    // installation: what the containers and the loose tree hold, and what the
    // chain serves for each case-only pair.
    let measured = measure(&game_dir, &found);
    let pairs_path = evidence_dir.join("pairs.json");
    fs::write(
        &pairs_path,
        pairs_json(&candidate_tree, &install_sha256, &content_sha256, &measured),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", pairs_path.display()));
    artifacts.push(artifact(&pairs_path, "json", &evidence_dir));

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T693\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [{}],\n\
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
        unknown_array(&[
            "The case and separator spelling the game's GOS requests carry at runtime is not \
             established: #693 pinned what the original does *to* a request (roffile.dll \
             upper-cases it for a container and hands it unfolded to CreateFileA for a loose \
             file) and that the exe's own wrapper passes it through unchanged at 0x411e31 and \
             0x411ed0, but no original run shows what game code asks for. Affected content: every \
             GOS request at runtime; resolving it needs an owner-supplied original run.",
            "The host filesystem's case behaviour for a loose name is a property of the machine \
             the original ran on, not of roffile.dll: CreateFileA at 0x100077bd decides it. \
             Affected content: the loose copies of ASSETS/GRAPHICS/arial8.tga and font.tga and \
             any loose-only name whose case differs from a request; resolving it needs an \
             original run on the target host.",
            "Non-ASCII/DBCS name folding follows CharUpperA's ANSI-codepage behaviour and was \
             not measured: every retail GOS name in both containers and the loose tree is ASCII. \
             Affected content: none on this installation.",
        ]),
        jstr(
            "implementer: bunny-2/bunny-2 (Rally #693 implement claim of 2026-10-06T17:39:16Z); \
             reviewer: recorded by the reviewing agent — no review had happened when this report \
             was generated, and no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR, the production \
             GOS chain of that installation (pairs.json: container and loose member sets, the \
             two case-only pairs and the bytes each serves), rustc and Cargo.lock. The rule \
             itself is static analysis of roffile.dll recorded in \
             docs/findings/2026-10-06-t693-metaopenfile-name-matching.md, so the claim is only \
             `implemented`, never `verified_original`; validated with tools/validate_evidence.py \
             (without --require-pass, because the three unknowns above are the task's residue)"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    // A cheap self-check without a JSON dependency: the validator runs next,
    // but a structurally empty write must fail here first.
    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"T693\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
        "\"unknowns\": [",
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

// ------------------------------------------------------- what was measured ---

/// What the production GOS chain of the original installation holds and
/// serves: the premise of #693's rule (every container name uppercase), the
/// two case-only pairs, and the bytes each pair answers with.
struct Measured {
    name_match: GosNameMatch,
    order: Vec<&'static str>,
    patch_members: usize,
    main_members: usize,
    loose_members: usize,
    /// `(container spelling, loose spelling, served mount, length, sha256 of
    /// the served bytes, sha256 of the loose file's bytes)`.
    pairs: Vec<(String, String, String, usize, String, String)>,
}

fn measure(root: &Path, found: &install::Discovery) -> Measured {
    let context = ResolveContext::new(fingerprint(&found.manifest));
    let install = GosInstall::new(root, ExePathOrigin::inspect_registry(root));
    let mut builder = SessionBuilder::new(context);
    let chain = builder
        .mount_gos_chain(&install)
        .expect("the chain mounts for the evidence record");
    let session = builder.open();
    assert!(
        session.rejected().is_empty(),
        "no member of this installation is refused: {:?}",
        session.rejected()
    );
    let mounted = Chain { session, chain };

    let patch = members_of(&mounted, GosSource::PatchContainer);
    let main = members_of(&mounted, GosSource::MainContainer);
    let loose = members_of(&mounted, GosSource::LooseUiAssets);
    assert!(
        patch
            .values()
            .chain(main.values())
            .all(|(spelling, ..)| !spelling.chars().any(|c| c.is_ascii_lowercase())),
        "every container name must be ASCII-uppercase, which is the premise the rule's retail \
         consequence rests on"
    );

    let mut pairs = Vec::new();
    for key in loose.keys() {
        let Some((container_spelling, ..)) = main.get(key) else {
            continue;
        };
        let (loose_spelling, ..) = &loose[key];
        let served = mounted
            .session
            .resolve(&gos_key(container_spelling).expect("a valid key"))
            .unwrap_or_else(|error| panic!("{container_spelling} resolves: {error}"));
        assert_eq!(
            served.resolved().mount.as_str(),
            MAIN_MOUNT_ID,
            "{container_spelling} is answered by crimson.rof, not by the loose copy"
        );
        let served_bytes = mounted
            .chain
            .read(&mounted.session, &served)
            .expect("the container member reads through the production reader");
        let loose_bytes = fs::read(
            root.join("GOSDATA")
                .join(loose_spelling.replace('/', std::path::MAIN_SEPARATOR_STR)),
        )
        .unwrap_or_else(|error| panic!("{loose_spelling} is readable: {error}"));
        assert_eq!(
            served_bytes, loose_bytes,
            "{container_spelling} and its loose copy must be the same bytes"
        );
        pairs.push((
            container_spelling.clone(),
            loose_spelling.clone(),
            served.resolved().mount.as_str().to_owned(),
            served_bytes.len(),
            sha256(&served_bytes).to_hex(),
            sha256(&loose_bytes).to_hex(),
        ));
    }
    pairs.sort();

    let measured = Measured {
        name_match: mounted.chain.name_match(),
        order: mounted.chain.order(),
        patch_members: patch.len(),
        main_members: main.len(),
        loose_members: loose.len(),
        pairs,
    };
    assert_eq!(
        measured.pairs.len(),
        2,
        "exactly the two case-only pairs exist on both sides of the chain"
    );
    measured
}

/// One mounted chain plus the session its sources are read through.
struct Chain {
    session: ContentSession,
    chain: GosChain,
}

/// Every member a chain registered for `source`, keyed by its logical
/// (case-folded) path and holding its own spelling and its mounted digest.
fn members_of(chain: &Chain, source: GosSource) -> BTreeMap<String, (String, u64, String)> {
    let step = chain
        .chain
        .step_of(source)
        .unwrap_or_else(|| panic!("{} is registered", source.label()));
    let mut members = BTreeMap::new();
    for attempt in chain.session.mounts() {
        if attempt.id() != &step.mount {
            continue;
        }
        for (_, member) in attempt.members() {
            members.insert(
                member.spelling().logical_key().to_owned(),
                (
                    member.spelling().as_str().to_owned(),
                    member.size_bytes(),
                    member
                        .sha256()
                        .unwrap_or_else(|| panic!("{} is hashed", member.spelling().as_str()))
                        .to_hex(),
                ),
            );
        }
    }
    assert_eq!(
        members.len(),
        step.members,
        "{} contributes exactly what its step counted",
        source.label()
    );
    members
}

/// The measured artifact: spellings, counts and pair digests of the
/// installation's GOS chain, with relative spellings and hashes only — never
/// original file bytes.
fn pairs_json(
    candidate_tree: &str,
    install_sha256: &str,
    content_sha256: &str,
    measured: &Measured,
) -> String {
    let pairs: Vec<String> = measured
        .pairs
        .iter()
        .map(|(container, loose, mount, length, digest, loose_digest)| {
            format!(
                "{{\"container\": {}, \"loose\": {}, \"served_by\": {}, \"length\": {length}, \
                     \"served_sha256\": {digest:?}, \"loose_sha256\": {loose_digest:?}, \
                     \"identical\": {}}}",
                jstr(container),
                jstr(loose),
                jstr(mount),
                digest == loose_digest,
            )
        })
        .collect();
    let order: Vec<String> = measured.order.iter().map(|step| jstr(step)).collect();
    format!(
        "{{\n\
         \x20\"task_id\": \"T693\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"content_sha256\": {},\n\
         \x20\"source\": \"SessionBuilder::mount_gos_chain over the original installation\",\n\
         \x20\"name_match\": {},\n\
         \x20\"order\": [{}],\n\
         \x20\"container_names_ascii_uppercase\": true,\n\
         \x20\"patch_members\": {},\n\
         \x20\"main_members\": {},\n\
         \x20\"loose_members\": {},\n\
         \x20\"case_only_pairs\": [\n  {}\n ]\n\
         }}\n",
        jstr(candidate_tree),
        jstr(&iso_utc_now()),
        jstr(install_sha256),
        jstr(content_sha256),
        jstr(measured.name_match.label()),
        order.join(", "),
        measured.patch_members,
        measured.main_members,
        measured.loose_members,
        pairs.join(",\n  "),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_assets/tests/evidence_report_t693.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T693` written relative to the
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
/// `accept_t693_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_t693_") {
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
/// exercises (the validator re-hashes it with `hashlib` independently).
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

fn unknown_array(unknowns: &[&str]) -> String {
    let items: Vec<String> = unknowns.iter().map(|entry| jstr(entry)).collect();
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
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = year_of_era + era * 400;
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
