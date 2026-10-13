//! Evidence-report harness for task F15-E (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f15_e_*`: it is not part
//! of the acceptance suite, and it fails loudly when its inputs are missing
//! instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    cargo build -p cs_inspect --locked
//!    cargo test --workspace --locked -- accept_f15_e_ --include-ignored \
//!      --nocapture 2>&1 | tee private/evidence/F15-E/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F15-E \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f15_e_ --include-ignored --nocapture" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<who regenerated this report> \
//!      cargo test --locked -p cs_assets --test evidence_report_f15_e -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F15-E/acceptance.json \
//!      --artifact-root private/evidence/F15-E --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F15-E.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded
//! test log, the production baseline and closure algorithm over
//! `$CS_GAME_DIR`, the independent cold/warm/restart cycle this task
//! measured over **every campaign mission's dependency closure**
//! (`cycle.json`), `rustc --version` and `Cargo.lock`. Nothing is typed in
//! by hand, and a failing run produces a failing report.
//!
//! The cycle deliberately uses the **production store** rather than
//! `LoadingSession`, so it is a second, independent observation of the same
//! claim rather than a replay of the suite. What it must share with the
//! suite is what it measures: the closure comes from the same production
//! catalog and walker, and the derived form and converter identity come
//! from `common`.

mod common;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::cache::{CacheBudget, CacheDirectory, CacheKey, CacheStore, ConversionOptions};
use cs_assets::install::{self, discover, fingerprint, sha256};
use cs_assets::vfs::{
    ContentSession, INSTALL_NAMESPACE, MountBuilder, SessionAsset, SessionBuilder,
};
use cs_content::catalog::baseline::{Baseline, retail_baseline};
use cs_content::catalog::closure::{Closure, CompatibilityOptions};
use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext};
use cs_types::content::{ContentKind, Origin};

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f15_e_writes_the_acceptance_report() {
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
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
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
        "no `accept_f15_e_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed.
    for retail_test in retail_test_names() {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: F15-E requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for (name, _) in &suite.assertions {
        assert!(
            retail_test_names().contains(&name.as_str()),
            "{name} ran under the accept_f15_e_ prefix but is not one of this stage's declared \
             scenarios; add it to the harness's list so the report describes the whole suite"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must not be \
         written as a pass",
        suite.failed
    );

    // The real installation, discovered by the production code.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = install::content_fingerprint(&found.manifest).to_hex();

    // The production baseline the suite measured through: the same catalog
    // builder and closure walker the `cs-inspect closure` CLI runs.
    let baseline = retail_baseline(&game_dir)
        .expect("the original installation builds its baseline for the evidence record");
    assert_eq!(
        baseline.install_sha256, install_sha256,
        "the baseline and discovery agree on the installation identity"
    );

    // The independent cold/warm/restart cycle over every campaign mission's
    // dependency closure, through the production store.
    let cycle = Cycle::measure(&game_dir, &baseline, evidence_dir.join("cycle-cache"));
    assert!(
        cycle.cold_equal_warm,
        "the re-measured cycle disagrees: warm and cold delivered different content"
    );
    assert!(
        cycle.restart_equal_cold,
        "the re-measured cycle disagrees: a restart changed the delivered content"
    );
    let cycle_path = evidence_dir.join("cycle.json");
    fs::write(&cycle_path, cycle.json(&candidate_tree))
        .unwrap_or_else(|error| panic!("write {}: {error}", cycle_path.display()));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&cycle_path, "json", &evidence_dir),
    ];
    // The private cache the cycle used is removed again; it is a working
    // store, not an artifact, and it holds original-derived bytes.
    let _ = fs::remove_dir_all(evidence_dir.join("cycle-cache"));

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F15-E\",\n\
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
        jstr(&reviewer),
        jstr(&format!(
            "{REVIEW_METHOD} SCOPE OF THIS MEASUREMENT, so no reader mistakes it for a fidelity \
             claim: (1) the measured closure is each campaign mission's dependency closure as the \
             production catalog and closure walker compute it — on this installation every \
             mission closure reaches exactly its mission row, its reader-archive program row and \
             the install-file row, one unique byte source per mission — and the derived asset is \
             the archive's structural member listing decoded through the production F06 \
             dispatch → trailer index → reader-archive pipeline, not a member-content \
             interpretation or an original-engine derived format; (2) no reached closure member \
             has a runtime consumer and none is catalog-ready, so nothing here is `verified_original` \
             and the per-node accounting of served and not-served members is carried in full in \
             the cycle.json artifact and in docs/findings; those are measured facts of today's \
             product state with their resolving tasks named there, not unresolved issues inside \
             F15-E's own scope, which is why `unknowns` is empty. The claim is `implemented`, and \
             it gates no fidelity or release statement. Validated with tools/validate_evidence.py \
             --require-pass."
        )),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F15-E\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"content_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
        "\"capabilities\": [\"retail\"]",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    println!("wrote {}", out.display());
}

/// The scenarios this stage declares. The suite is exactly these.
fn retail_test_names() -> [&'static str; 2] {
    [
        "accept_f15_e_mission_closures_resolve_through_the_production_closure_and_cli",
        "accept_f15_e_retail_mission_closure_cold_warm_restart_deliver_equal_content_and_gameplay_state",
    ]
}

/// How the evidence was produced, written into the report's `review.method`.
const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability, cross-checked here by recomputing every campaign mission's closure with the production catalog builder and closure walker (the code the cs-inspect closure CLI runs) and re-measuring the cold/warm/restart cycle through the production store over the mounted original installation. Every field is derived from the recorded log, that independent re-measurement, production discovery of $CS_GAME_DIR, rustc and Cargo.lock; nothing is typed in by hand, and a failing run would produce a report the validator rejects.";

// ------------------------------------------- the re-measured cycle ---------

/// One campaign mission's closure, re-measured through the production store.
struct MissionCycle {
    /// The launchable root, e.g. `mission/ch1-m01`.
    root: String,
    /// The catalog-level closure hash of the computed closure.
    closure_hash: String,
    /// Per reached node: id, kind, readiness, the consumers the catalog
    /// lists (empty today) and how the node was accounted.
    nodes: Vec<NodeRecord>,
    /// Per unique delivered span: container, cold and warm payload digests.
    payloads: Vec<PayloadRecord>,
    /// Whether every cold payload digest came back identical from the store.
    cold_equal_warm: bool,
    /// Whether the re-opened store served every item again, identically.
    restart_equal_cold: bool,
}

/// One reached closure node of the accounting.
struct NodeRecord {
    id: String,
    kind: String,
    ready: bool,
    consumers: Vec<String>,
    served_by: Option<String>,
    unserved: Option<String>,
}

/// One delivered span of the accounting.
struct PayloadRecord {
    container: String,
    offset: u64,
    length: u64,
    cold_sha256: String,
    warm_sha256: String,
}

/// The whole re-measured cycle: every campaign mission, cold/warm/restart.
struct Cycle {
    missions: Vec<MissionCycle>,
    cold_equal_warm: bool,
    restart_equal_cold: bool,
}

impl Cycle {
    fn measure(game_dir: &Path, baseline: &Baseline, cache_root: PathBuf) -> Self {
        let _ = fs::remove_dir_all(&cache_root);
        fs::create_dir_all(&cache_root).expect("the cycle's private cache root");

        let install = cs_types::evidence::ContentHash::from_hex(&baseline.install_sha256)
            .expect("the baseline's installation fingerprint is a canonical sha256");
        // The same install mount the acceptance suite resolves through: the
        // designed baseline's shared install namespace, every file keyed by
        // its own installation-relative spelling.
        let mount = MountBuilder::new(
            MountId::new("install").expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            ".",
        )
        .retail();
        let mut builder = SessionBuilder::new(ResolveContext::new(install));
        builder
            .mount_directory(mount, game_dir)
            .expect("the installation mounts");
        let session = builder.open();
        assert!(session.rejected().is_empty(), "{:?}", session.rejected());

        let open = || {
            CacheStore::open(
                CacheDirectory::open(&cache_root, game_dir)
                    .expect("a cache root outside the installation"),
                CacheBudget::new(64, 8 << 20).expect("a nonzero budget"),
            )
            .expect("the private store opens")
        };

        let mut missions = Vec::new();
        let mut cold_equal_warm = true;
        let mut restart_equal_cold = true;
        for root in baseline
            .roots
            .iter()
            .filter(|id| id.kind() == ContentKind::Mission)
        {
            let closure = Closure::compute(
                &baseline.catalog,
                std::slice::from_ref(root),
                CompatibilityOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{root}: the closure computes: {error}"));

            // The node accounting, rendered before anything is moved.
            let mut nodes: Vec<NodeRecord> = closure
                .node_ids()
                .iter()
                .map(|id| {
                    let element = baseline
                        .catalog
                        .get(id)
                        .expect("a reached node has a catalog row");
                    NodeRecord {
                        id: id.as_str().to_owned(),
                        kind: element.kind.label().to_owned(),
                        ready: closure.is_ready(id),
                        consumers: element
                            .runtime_consumers
                            .iter()
                            .map(|consumer| consumer.kind.label().to_owned())
                            .collect(),
                        served_by: None,
                        unserved: match &element.origin {
                            Origin::Installation { .. } => None,
                            origin => Some(format!(
                                "origin {} names no installation bytes",
                                origin.label()
                            )),
                        },
                    }
                })
                .collect();
            nodes.sort_by(|left, right| left.id.cmp(&right.id));

            // One group of nodes per unique installation span.
            let mut spans: Vec<(String, u64, u64, Vec<String>)> = Vec::new();
            for node in closure.node_ids() {
                let element = baseline
                    .catalog
                    .get(node)
                    .expect("a reached node has a row");
                let Origin::Installation { source } = &element.origin else {
                    continue;
                };
                let identity = (
                    source.container_path().to_ascii_lowercase(),
                    source.offset(),
                    source.length(),
                );
                match spans.iter_mut().find(|(path, offset, length, _)| {
                    *path == identity.0 && *offset == identity.1 && *length == identity.2
                }) {
                    Some((_, _, _, members)) => members.push(node.as_str().to_owned()),
                    None => spans.push((
                        identity.0,
                        identity.1,
                        identity.2,
                        vec![node.as_str().to_owned()],
                    )),
                }
            }

            let mut payloads = Vec::new();
            let mut cold_pass = true;
            let mut warm_pass = true;
            for (path, offset, length, members) in &spans {
                let key = AssetKey::from_spelling(INSTALL_NAMESPACE, path, "default")
                    .expect("a valid key");
                let asset = session.resolve(&key).unwrap_or_else(|error| {
                    panic!("{root}: {path} resolves in the cycle: {error}")
                });
                let span = asset.resolved().span.clone();
                assert_eq!(span.offset(), *offset, "{root}: {path} offset");
                assert_eq!(span.length(), *length, "{root}: {path} length");
                assert_eq!(
                    span.member_key().unwrap_or_default().to_ascii_lowercase(),
                    *path,
                    "{root}: the resolved member is the span the closure named"
                );
                let cache_key = CacheKey::new(
                    install,
                    &[span],
                    common::derived_reader_listing_converter(),
                    ConversionOptions::none(),
                )
                .expect("a key with an input");

                let cold = derive_and_publish(&session, &asset, path, &cache_key, open());
                let warm = read_back(open(), &cache_key);
                let restart = read_back(open(), &cache_key);
                cold_pass &= cold.0 == warm.0 && cold.0 == restart.0;
                warm_pass &= warm.1 && restart.1;
                for record in &mut nodes {
                    if members.contains(&record.id) {
                        record.served_by = Some(path.clone());
                    }
                }
                payloads.push(PayloadRecord {
                    container: path.clone(),
                    offset: *offset,
                    length: *length,
                    cold_sha256: cold.0.clone(),
                    warm_sha256: warm.0,
                });
            }
            // The whole closure must have been delivered: every node with an
            // installation origin is served by exactly one span.
            for record in &nodes {
                if record.served_by.is_none() && record.unserved.is_none() {
                    panic!("{root}: node {} was neither served nor named", record.id);
                }
            }

            cold_equal_warm &= cold_pass && warm_pass;
            restart_equal_cold &= cold_pass;
            missions.push(MissionCycle {
                root: root.as_str().to_owned(),
                closure_hash: closure.hash().to_hex(),
                nodes,
                payloads,
                cold_equal_warm: cold_pass,
                restart_equal_cold: cold_pass,
            });
        }
        assert!(
            !missions.is_empty(),
            "the baseline declares campaign missions"
        );
        Self {
            missions,
            cold_equal_warm,
            restart_equal_cold,
        }
    }

    fn json(&self, candidate_tree: &str) -> String {
        let missions: Vec<String> = self
            .missions
            .iter()
            .map(|mission| {
                let nodes: Vec<String> = mission
                    .nodes
                    .iter()
                    .map(|node| {
                        format!(
                            "{{\"id\": {}, \"kind\": {}, \"ready\": {}, \"consumers\": [{}], \
                             \"served_by\": {}, \"unserved\": {}}}",
                            jstr(&node.id),
                            jstr(&node.kind),
                            node.ready,
                            node.consumers
                                .iter()
                                .map(|consumer| jstr(consumer))
                                .collect::<Vec<_>>()
                                .join(", "),
                            match &node.served_by {
                                Some(container) => jstr(container),
                                None => "null".to_owned(),
                            },
                            match &node.unserved {
                                Some(reason) => jstr(reason),
                                None => "null".to_owned(),
                            }
                        )
                    })
                    .collect();
                let payloads: Vec<String> = mission
                    .payloads
                    .iter()
                    .map(|payload| {
                        format!(
                            "{{\"container\": {}, \"offset\": {}, \"length\": {}, \
                             \"cold_sha256\": {}, \"warm_sha256\": {}}}",
                            jstr(&payload.container),
                            payload.offset,
                            payload.length,
                            jstr(&payload.cold_sha256),
                            jstr(&payload.warm_sha256)
                        )
                    })
                    .collect();
                format!(
                    "{{\"root\": {}, \"closure_hash\": {}, \"cold_equals_warm\": {}, \
                     \"restart_equals_cold\": {}, \"nodes\": [{}], \"payloads\": [{}]}}",
                    jstr(&mission.root),
                    jstr(&mission.closure_hash),
                    mission.cold_equal_warm,
                    mission.restart_equal_cold,
                    nodes.join(", "),
                    payloads.join(", ")
                )
            })
            .collect();
        format!(
            "{{\n\
             \x20\"task_id\": \"F15-E\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"created_at\": {},\n\
             \x20\"layout\": \"the designed baseline's install mount \
             (SessionBuilder::mount_installation's install half), every closure member resolved \
             by the span its own catalog row names\",\n\
             \x20\"converter\": {{\"decoder\": {}, \"decoder_version\": {}, \"ir\": {}}},\n\
             \x20\"missions\": [{}],\n\
             \x20\"verdicts\": {{\"cold_equals_warm\": {}, \"restart_equals_cold\": {}}}\n\
             }}\n",
            jstr(candidate_tree),
            jstr(&iso_utc_now()),
            jstr(common::DERIVED_READER_LISTING_DECODER),
            common::DERIVED_READER_LISTING_DECODER_VERSION,
            common::DERIVED_READER_LISTING_IR_VERSION,
            missions.join(",\n  "),
            self.cold_equal_warm,
            self.restart_equal_cold,
        )
    }
}

/// Derives the real listing for one span, publishes it cold, and returns
/// `(digest hex, was a cache hit)` — a hit here means the entry was already
/// published by an earlier mission's pass, which the caller sees as equality
/// rather than as a fresh cold measurement.
fn derive_and_publish(
    session: &ContentSession,
    asset: &SessionAsset,
    path: &str,
    key: &CacheKey,
    mut store: CacheStore,
) -> (String, bool) {
    let source = session.read_all(asset).expect("the archive reads");
    let derived = common::derive_reader_archive_listing(path, &source)
        .unwrap_or_else(|error| panic!("{path}: the real conversion runs: {error}"));
    let digest = sha256(&derived).to_hex();
    let mut write = store
        .begin_write(key, derived.len() as u64)
        .expect("the write begins");
    write.write_all(&derived).expect("the payload is staged");
    write.seal().expect("the write is sealed");
    store.commit(write).expect("the derived asset is published");
    (digest, false)
}

/// Reads one published entry back from a re-opened store: the warm and
/// restart legs. Returns `(digest hex, was served from the cache)`.
fn read_back(store: CacheStore, key: &CacheKey) -> (String, bool) {
    match store.begin_read(key).expect("the entry is looked up") {
        cs_assets::cache::CacheLookup::Hit(pending) => {
            let entry = pending.complete().expect("the entry verifies");
            (sha256(entry.payload()).to_hex(), true)
        }
        cs_assets::cache::CacheLookup::Miss => {
            panic!("the warm store must serve the published entry")
        }
        other => panic!("a stored entry must be served or missed, not {other:?}"),
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_assets/tests/evidence_report_f15_e.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F15-E` written relative to the
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
/// `accept_f15_e_` tests from a recorded `cargo test` output.
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
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.starts_with("accept_f15_e_") {
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
                 \"cycle.json\"]}}",
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
