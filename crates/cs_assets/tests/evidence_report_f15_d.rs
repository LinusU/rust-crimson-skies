//! Evidence-report harness for task F15-D (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f15_d_*`: it is not part of
//! the acceptance suite, and it fails loudly when its inputs are missing
//! instead of passing vacuously. Run from the workspace root, after
//! the acceptance suite, exactly as:
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_f15_d_ --include-ignored \
//!      2>&1 | tee private/evidence/F15-D/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; with `pipefail` or by checking the
//!    first command's status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F15-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f15_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<who regenerated this report> \
//!      cargo test --locked -p cs_assets --test evidence_report_f15_d -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F15-D/acceptance.json \
//!      --artifact-root private/evidence/F15-D --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F15-D.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded
//! test log, the environment, production discovery of `$CS_GAME_DIR`, the
//! production cold/warm/restart cycle this task measured
//! (`cycle.json`), `rustc --version` and `Cargo.lock`. Nothing is typed in by
//! hand, and the report describes the actual execution — a failing run
//! produces a failing report, which the validator rejects.
//!
//! The cycle deliberately uses the **production store** rather than
//! `LoadingSession`, so it is a second, independent observation of the same
//! claim rather than a replay of the suite. What it must share with the
//! suite is the thing it measures: the derived form and the converter
//! identity come from `common`, the same definitions the acceptance suite
//! uses, so the two can never drift into measuring different bytes.

mod common;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::cache::{CacheBudget, CacheDirectory, CacheKey, CacheStore, ConversionOptions};
use cs_assets::install::{self, Discovery, content_fingerprint, discover, fingerprint, sha256};
use cs_assets::vfs::{ContentSession, MountBuilder, SessionBuilder};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};

/// How many base levels of each archive the derived asset covers: the bound
/// [`common::DERIVED_TEXTURES_PER_ARCHIVE`] the acceptance suite uses.
const DERIVED_TEXTURES_PER_ARCHIVE: usize = common::DERIVED_TEXTURES_PER_ARCHIVE;

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f15_d_writes_the_acceptance_report() {
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
        "no `accept_f15_d_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: the report declares
    // `retail` only because every retail acceptance test of this stage is
    // in this log and passed, and the spec's required capability for F15-D
    // is exactly that.
    for retail_test in [
        "accept_f15_d_retail_cold_and_warm_loads_deliver_equal_content_and_gameplay_state",
        "accept_f15_d_retail_restart_after_a_killed_cache_write_recovers_and_reproduces_the_same_state",
        "accept_f15_d_retail_corrupt_cache_entry_is_rebuilt_and_never_changes_the_delivered_content",
        "accept_f15_d_retail_cache_never_serves_an_entry_under_another_identity",
        "accept_f15_d_retail_budget_eviction_never_changes_the_delivered_content",
    ] {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: F15-D requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    // Nothing named with this task's prefix may be left unaccounted for: a
    // new scenario added to the suite must be declared here, not silently
    // dropped from the report.
    for (name, _) in &suite.assertions {
        assert!(
            retail_test_names().contains(&name.as_str()),
            "{name} ran under the accept_f15_d_ prefix but is not one of this stage's declared \
             scenarios; add it to the harness's list so the report describes the whole suite"
        );
    }

    // `source` hashes describe the real installation, measured by the very
    // production code this task runs its loads through.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    // The cold/warm/restart cycle, re-measured here by the production store
    // and the production ZBD reader over the mounted original installation.
    // It is a second, independent observation of the same claim the suite
    // makes, and it is what gives the report its `cycle.json` artifact.
    let cycle = ColdWarmRestart::measure(&game_dir, &found, evidence_dir.join("cycle-cache"));
    assert!(
        cycle.cold_equal_warm,
        "the re-measured cycle disagrees: warm and cold delivered different content"
    );
    assert!(
        cycle.restart_equal_reference,
        "the re-measured cycle disagrees: a restart after a killed cache write changed the content"
    );
    let cycle_path = evidence_dir.join("cycle.json");
    fs::write(&cycle_path, cycle.json(&candidate_tree, &install_sha256))
        .unwrap_or_else(|error| panic!("write {}: {error}", cycle_path.display()));

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
         \x20\"task_id\": \"F15-D\",\n\
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
             claim: (1) the measured closure is each world group's own texture archives \
             (texture.zbd and its rtexture*.zbd tiers) resolved through the designed baseline \
             mount, not a mission's full dependency closure — the mission closure, the script \
             adapter's dynamic candidate sets and every non-texture content kind are outside \
             this stage; (2) the derived asset is a project-defined canonical form (name, extent \
             and decoded base-level texels of the first {DERIVED_TEXTURES_PER_ARCHIVE} textures \
             of an archive), not an original-engine derived format, and which assets the original \
             engine precomputes, with which options, is unmeasured. Both are limits of what was \
             measured, not unresolved defects in it: `unknowns` is empty because nothing in \
             F15-D's own scope is left unresolved. The claim is `implemented`, and it gates no \
             fidelity or release statement. Validated with tools/validate_evidence.py \
             --require-pass.",
            DERIVED_TEXTURES_PER_ARCHIVE = DERIVED_TEXTURES_PER_ARCHIVE,
        )),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    // A cheap self-check without a JSON dependency: the validator runs next,
    // but a structurally empty write must fail here first.
    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F15-D\"",
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
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The scenarios this stage declares. The suite is exactly these.
fn retail_test_names() -> [&'static str; 5] {
    [
        "accept_f15_d_retail_cold_and_warm_loads_deliver_equal_content_and_gameplay_state",
        "accept_f15_d_retail_restart_after_a_killed_cache_write_recovers_and_reproduces_the_same_state",
        "accept_f15_d_retail_corrupt_cache_entry_is_rebuilt_and_never_changes_the_delivered_content",
        "accept_f15_d_retail_cache_never_serves_an_entry_under_another_identity",
        "accept_f15_d_retail_budget_eviction_never_changes_the_delivered_content",
    ]
}

/// How the evidence was produced, written into the report's `review.method`.
const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability, cross-checked by re-measuring the cold/warm/restart cycle here through the production store, the production session and the production ZBD texture reader over the mounted original installation. Every field is derived from the recorded log, that independent re-measurement, production discovery of $CS_GAME_DIR, rustc and Cargo.lock; nothing is typed in by hand, and a failing run would produce a report the validator rejects.";

// ------------------------------------------- the re-measured cycle ---------

/// One world's cold, warm and restarted loads, re-measured through the
/// production store so the report carries an observation of its own rather
/// than only a test log.
struct ColdWarmRestart {
    world: String,
    archives: Vec<String>,
    cold: Pass,
    warm: Pass,
    restart: Pass,
    cold_closure_hash: String,
    warm_closure_hash: String,
    restart_closure_hash: String,
    cold_entries: u64,
    warm_entries: u64,
    cold_hits: usize,
    warm_hits: usize,
    restart_swept: usize,
    cold_equal_warm: bool,
    restart_equal_reference: bool,
}

impl ColdWarmRestart {
    fn measure(game_dir: &Path, found: &Discovery, cache_root: PathBuf) -> Self {
        let _ = fs::remove_dir_all(&cache_root);
        fs::create_dir_all(&cache_root).expect("the cycle's private cache root");

        // The first world group, mounted exactly as
        // `SessionBuilder::mount_installation` mounts each of them.
        let spelling = found.diagnosis.world_groups[0].clone();
        let group = WorldGroup::from_relative(spelling.clone());
        let install = fingerprint(&found.manifest);
        let context = ResolveContext::new(install).with_world_group(group.clone());
        let mut builder = SessionBuilder::new(context);
        let mut dir = game_dir.to_path_buf();
        dir.extend(spelling.as_str().split(['/', '\\']));
        builder
            .mount_directory(
                MountBuilder::new(
                    MountId::new("world-0").expect("a valid mount id"),
                    MountNamespace::new("world").expect("a valid namespace"),
                    PrecedenceClass::MissionWorld,
                    spelling.as_str(),
                )
                .with_world_group(group.clone())
                .retail(),
                &dir,
            )
            .expect("the world's own directory mounts");
        let session = builder.open();
        assert!(
            session.rejected().is_empty(),
            "{spelling}: {:?}",
            session.rejected()
        );

        // The world's own texture archives, from what the mount holds.
        let mut archives: Vec<String> = session
            .mounts()
            .flat_map(|mount| mount.members())
            .map(|(_, member)| member.spelling().as_str().to_owned())
            .filter(|name| {
                let lower = name.to_ascii_lowercase();
                lower == "texture.zbd" || (lower.starts_with("rtexture") && lower.ends_with(".zbd"))
            })
            .collect();
        archives.sort();
        assert!(
            !archives.is_empty(),
            "{spelling}: the world's own texture archives"
        );

        let open = || {
            CacheStore::open(
                CacheDirectory::open(&cache_root, game_dir)
                    .expect("a cache root outside the installation"),
                CacheBudget::new(64, 8 << 20).expect("a nonzero budget"),
            )
            .expect("the private store opens")
        };

        // A fresh process would find nothing published yet, so the cold
        // pass is the reference: every entry here is written by this pass.
        let cold = run(&session, &archives, install, open());
        assert_eq!(
            cold.hits, 0,
            "the cold pass must publish, not read: {} entries were already readable",
            cold.hits
        );
        // The warm pass re-opens the same store and must find every entry
        // already published, reading it and writing nothing.
        let warm = run(&session, &archives, install, open());
        assert_eq!(
            warm.hits,
            archives.len(),
            "the warm pass must be served entirely from verified entries: {} of {} were",
            warm.hits,
            archives.len()
        );
        // A restart: an interrupted write is left in the store's scratch
        // area, exactly as a killed process would, and the next open must
        // sweep it and deliver the same content.
        let interrupted = leave_an_interrupted_write(&cache_root, &archives[0], install, game_dir);
        assert!(
            interrupted > 0,
            "the interrupted write must really be on disk before the restart"
        );
        let restart = run(&session, &archives, install, open());
        assert_eq!(
            restart.recovery_swept, 1,
            "the next open must sweep exactly the interrupted write"
        );
        assert_eq!(
            restart.hits,
            archives.len(),
            "the restarted pass must be served entirely from verified entries"
        );

        // Every verdict and every reported scalar is computed before the
        // passes are moved into the record, so nothing is measured twice
        // and nothing is measured after it moved.
        let cold_equal_warm =
            cold.payloads == warm.payloads && cold.closure_hash == warm.closure_hash;
        let restart_equal_reference =
            restart.payloads == cold.payloads && restart.closure_hash == cold.closure_hash;
        let cold_closure_hash = cold.closure_hash.clone();
        let warm_closure_hash = warm.closure_hash.clone();
        let restart_closure_hash = restart.closure_hash.clone();
        let cold_entries = cold.entries;
        let warm_entries = warm.entries;
        let cold_hits = cold.hits;
        let warm_hits = warm.hits;
        let restart_swept = restart.recovery_swept;

        Self {
            world: spelling.as_str().to_owned(),
            archives,
            cold,
            warm,
            restart,
            cold_closure_hash,
            warm_closure_hash,
            restart_closure_hash,
            cold_entries,
            warm_entries,
            cold_hits,
            warm_hits,
            restart_swept,
            cold_equal_warm,
            restart_equal_reference,
        }
    }

    fn json(&self, candidate_tree: &str, install_sha256: &str) -> String {
        let render = |pass: &Pass| {
            let items: Vec<String> = pass
                .payloads
                .iter()
                .map(|(key, digest)| {
                    format!(
                        "{{\"member\": {}, \"derived_sha256\": {}}}",
                        jstr(key),
                        jstr(digest)
                    )
                })
                .collect();
            items.join(",\n    ")
        };
        format!(
            "{{\n\
             \x20\"task_id\": \"F15-D\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"created_at\": {},\n\
             \x20\"install_sha256\": {},\n\
             \x20\"layout\": \"SessionBuilder::mount_directory with the world MountBuilder of \
             SessionBuilder::mount_installation (designed)\",\n\
             \x20\"world_group\": {},\n\
             \x20\"texture_archives\": [{}],\n\
             \x20\"derived_textures_per_archive\": {},\n\
             \x20\"converter\": {{\"decoder\": {}, \"decoder_version\": {}, \"ir\": {}}},\n\
             \x20\"cold\": {{\"closure_hash\": {}, \"entries\": {}, \"cache_hits\": {}, \"items\": [\n    {}\n  ]}},\n\
             \x20\"warm\": {{\"closure_hash\": {}, \"entries\": {}, \"cache_hits\": {}, \"items\": [\n    {}\n  ]}},\n\
             \x20\"restart\": {{\"closure_hash\": {}, \"swept_interrupted_writes\": {}, \"items\": [\n    {}\n  ]}},\n\
             \x20\"verdicts\": {{\"cold_equals_warm\": {}, \"restart_equals_cold\": {}}}\n\
             }}\n",
            jstr(candidate_tree),
            jstr(&iso_utc_now()),
            jstr(install_sha256),
            jstr(&self.world),
            self.archives
                .iter()
                .map(|name| jstr(name))
                .collect::<Vec<_>>()
                .join(", "),
            DERIVED_TEXTURES_PER_ARCHIVE,
            jstr(common::DERIVED_TEXTURE_DECODER),
            common::DERIVED_TEXTURE_DECODER_VERSION,
            common::DERIVED_TEXTURE_IR_VERSION,
            jstr(&self.cold_closure_hash),
            self.cold_entries,
            self.cold_hits,
            render(&self.cold),
            jstr(&self.warm_closure_hash),
            self.warm_entries,
            self.warm_hits,
            render(&self.warm),
            jstr(&self.restart_closure_hash),
            self.restart_swept,
            render(&self.restart),
            self.cold_equal_warm,
            self.restart_equal_reference,
        )
    }
}

/// What one pass of the cycle measured.
#[derive(Clone)]
struct Pass {
    payloads: Vec<(String, String)>,
    closure_hash: String,
    entries: u64,
    hits: usize,
    recovery_swept: usize,
}

fn converter() -> cs_assets::cache::ConverterVersion {
    common::derived_texture_converter()
}

/// One pass of the cycle over every archive, through the production store
/// with the same lookup-first decision the production driver makes: a
/// verified entry is read, a miss is derived from the source and
/// published.
fn run(
    session: &ContentSession,
    archives: &[String],
    install: cs_types::evidence::ContentHash,
    mut store: CacheStore,
) -> Pass {
    let recovery_swept = store.recovery().swept_staging;
    let mut payloads = Vec::new();
    let mut hits = 0;
    for name in archives {
        let key = AssetKey::from_spelling("world", name, "default").expect("a valid asset key");
        let asset = session.resolve(&key).expect("the archive resolves");
        let span = asset.resolved().span.clone();
        let cache_key = CacheKey::new(install, &[span], converter(), ConversionOptions::none())
            .expect("a key with an input");

        let digest = match store
            .begin_read(&cache_key)
            .expect("the entry is looked up")
        {
            cs_assets::cache::CacheLookup::Hit(pending) => {
                hits += 1;
                // Only a verified entry becomes bytes: the payload is read
                // back through `verify_entry` first.
                let entry = pending.complete().expect("the entry verifies");
                sha256(entry.payload()).to_hex()
            }
            cs_assets::cache::CacheLookup::Miss => {
                let source = session.read_all(&asset).expect("the archive reads");
                let derived = derive(name, &source).expect("the real conversion runs");
                let digest = sha256(&derived).to_hex();
                let mut write = store
                    .begin_write(&cache_key, derived.len() as u64)
                    .expect("the write begins");
                write.write_all(&derived).expect("the payload is staged");
                write.seal().expect("the write is sealed");
                store.commit(write).expect("the derived asset is published");
                digest
            }
            other => panic!("{name}: a stored entry must be served or rebuilt, not {other:?}"),
        };
        payloads.push((name.clone(), digest));
    }
    // A closure hash over the measured per-item digests, so the three
    // passes can be compared with one value.
    let mut hasher = install::Sha256::new();
    for (name, digest) in &payloads {
        hasher.update(name.as_bytes());
        hasher.update(digest.as_bytes());
    }
    let closure_hash = hasher.finalize().to_hex();
    let entries = store.usage().entries;
    Pass {
        payloads,
        closure_hash,
        entries,
        hits,
        recovery_swept,
    }
}

/// The real conversion: the production ZBD texture reader and the real
/// base-level decode, in the one canonical form
/// [`common::derive_texture_base_levels`] defines — the same bytes the
/// acceptance suite stores, not a parallel derivation of its own.
fn derive(name: &str, source: &[u8]) -> Result<Vec<u8>, String> {
    common::derive_texture_base_levels(name, source)
}

/// Writes a real entry into the store's scratch area and leaves it there,
/// which is what a process killed between `begin_write` and `commit` leaves.
fn leave_an_interrupted_write(
    cache_root: &Path,
    member: &str,
    install: cs_types::evidence::ContentHash,
    game_dir: &Path,
) -> usize {
    let store = CacheStore::open(
        CacheDirectory::open(cache_root, game_dir).expect("a cache root outside the installation"),
        CacheBudget::new(64, 8 << 20).expect("a nonzero budget"),
    )
    .expect("the store opens");
    let key = CacheKey::new(
        install,
        &[
            cs_types::asset_id::SourceSpan::new(install, member, Some("interrupted"), 0, 1, None)
                .expect("a valid span"),
        ],
        converter(),
        ConversionOptions::none(),
    )
    .expect("a key with an input");
    let mut write = store
        .begin_write(&key, 4096)
        .expect("the interrupted write begins");
    write
        .write_all(b"partial")
        .expect("a partial payload is staged");
    // Deliberately not sealed and not committed: the scratch directory and
    // its partial payload are left on disk, and `store` is dropped without
    // cleaning them up, exactly as a killed process would.
    let staging = write.staging().to_path_buf();
    // `PendingStoreWrite`'s `Drop` removes the scratch directory; a killed
    // process does not run it, so it is suppressed here to reproduce what
    // the kill leaves behind.
    std::mem::forget(write);
    drop(store);
    fs::read_dir(&staging)
        .map(|entries| entries.count())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_assets/tests/evidence_report_f15_d.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F15-D` written relative to the
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
/// `accept_f15_d_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_f15_d_") {
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
