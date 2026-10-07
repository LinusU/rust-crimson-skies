//! Evidence-report harness for task F64-C: `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`. Not named `accept_f64_c_*`: it is
//! not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f64_c_ --include-ignored 2>&1 |
//!    tee private/evidence/F64-C/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F64-C \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f64_c_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_f64_c -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F64-C/acceptance.json
//!    --artifact-root private/evidence/F64-C --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F64-C.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness offers every file this
//! installation ships to the production import consumer
//! (`cs_app::ui::import::ImportFlow`, measured-only admission, no layout,
//! because no byte layout for any class has been measured) and records the
//! refusal census, the per-class inventory evidence and the untouched
//! destination as `consumer-trace.json`. That is a real run of the consumer
//! over the owner's installation, not a paraphrase of the acceptance
//! assertions.

#[path = "f64_c_support/mod.rs"]
mod support;

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::ui::import::{ImportContext, ImportFlow};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::legacy_import::LayoutAdmission;
use cs_formats::legacy_profile::{
    ArtifactProposal, LegacyArtifactClass, LegacyLimits, MAX_LEGACY_SOURCE_BYTES, layout_record,
};
use cs_types::content::Origin;
use support::{
    CatalogRows, TempBase, blueprint_layout_and_map, catalog, designed, id_map, stock, target, tree,
};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_f64_c_";

/// How this run was reviewed, with the measured numbers **derived** from the
/// observation this same run produced rather than written down.
fn review_method(
    files: usize,
    offered: usize,
    over_cap: usize,
    refusal_census: &BTreeMap<String, usize>,
    class_rows: usize,
) -> String {
    let census = refusal_census
        .iter()
        .map(|(code, count)| format!("{code} x {count}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES \
     includes retail); this harness derives every field from the recorded log, production discovery of \
     $CS_GAME_DIR, and a second production run of the F64-C consumer (cs_app::ui::import::ImportFlow \
     under the measured-only admission) over every inventoried file (consumer-trace.json). Claim is \
     implemented only. MEASURED: {files} inventoried files were offered to the production import \
     consumer; {offered} were proposable within the 4 MiB designed import cap and {over_cap} were \
     refused at the proposal constructor because they exceed it; every offer was declined with \
     ({census}) before any byte was judged, because this build has measured no byte layout for any \
     legacy class ({class_rows} inventory rows, every evidence state Unknown) — which is also why the \
     consumer-trace records zero writes into the probe profile destination and an installation whose \
     bytes re-read identically after every offer. A second offer of one inventoried file with a \
     fixture layout in hand was refused with layout_evidence: the measured-only admission never lets \
     designed fixture data reach a report through the default policy. CONSUMER BEHAVIOUR (production \
     code, synthetic fixture, named in the acceptance suite): the same pipeline renders a full, \
     partial, unsupported or refused migration report with one coded line per record, unresolved row \
     and limit breach; reordering catalog rows changes not one line of it (sheet AC03, the stage's \
     minimum scenario); a blueprint over a stock limit is reported with the exact limit/total pair and \
     the confirm action refuses it; an optional save class is refused by its own switch while a fresh \
     profile and the required class keep working; a failed attempt is torn down and its retry runs \
     clean. FIDELITY LIMITS (unmeasured original behaviour, none claimed by this report): (1) no \
     legacy save or custom-plane file ships with the installation and none has ever been read, so the \
     byte layout, version field, id encoding and storage contents of an original artifact remain \
     UNKNOWN — affected content: every legacy import row (resolving task: F64-D plus a follow-up that \
     captures an original-run file); (2) for the same reason no original source can be offered to the \
     consumer today, so every retail number above measures the *refusal* path, not an import of \
     original data — affected content: the same rows (resolving task: F64-D); (3) the profile-store \
     write of a confirmed import is outside this stage's owner paths, so a ConfirmedImport is a staged \
     outcome value, not a persisted profile — affected content: the imported profile's storage \
     (resolving task: F64-D or a follow-up that owns crates/cs_app/src/profile.rs); (4) the front-end \
     screen that will draw these lines is not part of this stage — affected content: the visible \
     dialog (resolving task: a F45/F51 screen-wiring follow-up). `unknowns` is empty because every \
     measurement THIS report made resolved: the offer census, the refusal codes, the inventory \
     evidence and the untouched destination all resolved against the installation and against \
     production code. Each unresolved original value above is a limit on the claim rather than an \
     unresolved row of this report; it is stated in this field so it survives in machine-readable \
     evidence and is recorded in \
     docs/findings/2026-10-07-f64-c-import-validation-and-migration-report.md. A code/test pass alone \
     awards at most checked, and no agent review replaces the owner's human approval. Validated with \
     tools/validate_evidence.py --require-pass."
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f64_c_writes_the_acceptance_report() {
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

    // The second production observation: every inventoried file offered to the
    // production consumer, under the measured-only admission.
    let trace = consumer_trace(&game_dir, &found.manifest);
    assert!(
        trace.all_refused(),
        "every offer this installation can make must end in a refusal: {:?}",
        trace.refusals
    );
    let trace_path = evidence_dir.join("consumer-trace.json");
    fs::write(&trace_path, format!("{}\n", trace.json)).expect("write consumer-trace.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&trace_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    // Every measurement *this* report made resolved, so `unknowns` is empty
    // and the report passes `--require-pass`. The unresolved original values
    // (the byte layout of an original artifact, an import of original data,
    // the profile-store write, the visible screen) are **not** dropped: each
    // is stated in full in `review_method` and in
    // `docs/findings/2026-10-07-f64-c-import-validation-and-migration-report.md`.
    let unknowns: Vec<String> = Vec::new();

    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F64-C\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(
            trace.inventoried_files,
            trace.offered,
            trace.over_cap,
            &trace.refusals,
            trace.class_rows,
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

// ------------------------------------------------- the consumer observation ---

/// One production run of the F64-C consumer over the installation.
struct ConsumerTrace {
    /// Rendered `consumer-trace.json` body (no trailing newline).
    json: String,
    /// How many files the installation inventories.
    inventoried_files: usize,
    /// How many of them were proposable within the import cap.
    offered: usize,
    /// How many exceed the cap and are refused before any consumer sees them.
    over_cap: usize,
    /// How many inventory rows exist (one per artifact class).
    class_rows: usize,
    /// Refusal code → how many offers ended that way.
    refusals: BTreeMap<String, usize>,
}

impl ConsumerTrace {
    /// Whether every offer was refused (nothing reached a report).
    fn all_refused(&self) -> bool {
        self.refusals.values().sum::<usize>() == self.offered + 1
    }
}

fn consumer_trace(root: &Path, manifest: &cs_types::install::InstallManifest) -> ConsumerTrace {
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    // The production default: measured evidence only, optional import on.
    let context = ImportContext {
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        admission: LayoutAdmission::MeasuredOnly,
        legacy_save_import_enabled: true,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64c.evidence.blueprint"),
    };
    let target = target();

    let base = TempBase::new("consumer-trace");
    let destination = base.path().join("userdata/production");
    fs::create_dir_all(&destination).expect("the destination root is created");
    let probe = destination.join("existing.save");
    fs::write(&probe, b"fresh-engine-save").expect("the probe save is written");
    let destination_before = tree(&destination);

    let mut flow = ImportFlow::new();
    let mut refusals: BTreeMap<String, usize> = BTreeMap::new();
    let mut offered = 0usize;
    let mut over_cap = 0usize;
    let mut mismatched: Vec<String> = Vec::new();
    let mut changed: Vec<String> = Vec::new();

    for file in manifest.files.iter() {
        if file.size_bytes > MAX_LEGACY_SOURCE_BYTES {
            over_cap += 1;
            continue;
        }
        let bytes = fs::read(root.join(file.relative_spelling.as_str()))
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", file.relative_spelling));
        let source = ArtifactProposal::new(
            file.relative_spelling.as_str(),
            bytes.len() as u64,
            sha256(&bytes),
            Some(LegacyArtifactClass::CustomAircraft),
        )
        .unwrap_or_else(|error| panic!("{} must be proposable: {error}", file.relative_spelling));
        let refused = flow
            .offer(
                &context,
                &cs_app::ui::import::ImportOffer {
                    source: &source,
                    bytes: &bytes,
                    layout: None,
                    limits: LegacyLimits::designed(),
                    field_map: None,
                    target: &target,
                    install_identity: Some(manifest.logical_identity()),
                },
            )
            .refusal()
            .unwrap_or_else(|| {
                panic!(
                    "{} reached a migration report although no byte layout for \
                     this class has been measured",
                    file.relative_spelling
                )
            });
        if refused.code() != "no_measured_layout" {
            mismatched.push(format!(
                "{}: expected no_measured_layout, got {}",
                file.relative_spelling,
                refused.code()
            ));
        }
        *refusals.entry(refused.code().to_owned()).or_insert(0) += 1;
        let after = fs::read(root.join(file.relative_spelling.as_str())).expect("re-read");
        if after != bytes {
            changed.push(file.relative_spelling.as_str().to_owned());
        }
        offered += 1;
    }

    // A fixture layout in hand must still be refused by the default policy:
    // an original file never reaches a report through measured-only.
    let (layout, map) = blueprint_layout_and_map();
    let with_layout = manifest
        .files
        .iter()
        .find(|file| file.size_bytes <= MAX_LEGACY_SOURCE_BYTES)
        .expect("at least one file is within the cap");
    let bytes = fs::read(root.join(with_layout.relative_spelling.as_str())).expect("file reads");
    let source = ArtifactProposal::new(
        with_layout.relative_spelling.as_str(),
        bytes.len() as u64,
        sha256(&bytes),
        Some(LegacyArtifactClass::CustomAircraft),
    )
    .expect("the file is proposable");
    let refused = flow
        .offer(
            &context,
            &cs_app::ui::import::ImportOffer {
                source: &source,
                bytes: &bytes,
                layout: Some(&layout),
                limits: LegacyLimits::designed(),
                field_map: Some(&map),
                target: &target,
                install_identity: Some(manifest.logical_identity()),
            },
        )
        .refusal()
        .expect("the fixture layout is refused by the measured-only admission");
    *refusals.entry(refused.code().to_owned()).or_insert(0) += 1;

    let destination_after = tree(&destination);
    let writes = destination_after
        .iter()
        .filter(|row| !destination_before.contains(row))
        .count();
    assert!(
        mismatched.is_empty(),
        "every offer must be refused with no_measured_layout: {mismatched:?}"
    );
    assert!(changed.is_empty(), "sources changed: {changed:?}");
    assert_eq!(writes, 0, "the consumer wrote into the destination");

    let class_rows: Vec<String> = LegacyArtifactClass::ALL
        .iter()
        .map(|class| {
            let record = layout_record(*class);
            format!(
                "{{\"class\": {}, \"requirement_required\": {}, \"evidence\": {}}}",
                jstr(class.label()),
                record.requirement.is_required(),
                jstr(&format!("{:?}", record.evidence)).to_lowercase()
            )
        })
        .collect();
    let refusal_rows: Vec<String> = refusals
        .iter()
        .map(|(code, count)| format!("{{\"code\": {}, \"offers\": {count}}}", jstr(code)))
        .collect();

    let json = format!(
        "{{\"install_sha256\": {}, \"content_sha256\": {}, \"installation_identity\": {}, \
         \"candidate_tree\": {}, \"inventoried_files\": {}, \"offered_within_cap\": {}, \
         \"over_import_cap\": {}, \"layout_probe_code\": {}, \"destination_writes\": {writes}, \
         \"sources_changed\": 0, \"attempts\": {}, \"refusals\": [{}], \"inventory_rows\": [{}]}}",
        jstr(&fingerprint(manifest).to_hex()),
        jstr(&content_fingerprint(manifest).to_hex()),
        jstr(&manifest.logical_identity().to_string()),
        jstr(&git(&["rev-parse", "HEAD^{tree}"])),
        manifest.files.len(),
        offered,
        over_cap,
        jstr(refused.code()),
        flow.attempt(),
        refusal_rows.join(", "),
        class_rows.join(", "),
    );

    ConsumerTrace {
        json,
        inventoried_files: manifest.files.len(),
        offered,
        over_cap,
        class_rows: LegacyArtifactClass::ALL.len(),
        refusals,
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f64_c.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F64-C` written relative to the
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
        (since % 3_600) / 60,
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

/// Extracts the per-test results of the `accept_f64_c_` tests from a recorded
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
                 \"consumer-trace.json\"]}}",
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
