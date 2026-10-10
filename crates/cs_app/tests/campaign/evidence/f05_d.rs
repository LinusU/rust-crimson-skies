//! Evidence-report harness for task F05-D, reissued by Rally #1174
//! `EVIDENCE-CONTENT-DIGEST-OUTLIERS-REPAIR` (owner decision of 2026-10-10:
//! option (a)).
//!
//! F05-D's committed report recorded a *task-scoped* digest — SHA-256 over the
//! two ROF containers' `path digest` lines — in `source.content_sha256`, whose
//! contract meaning is the canonical whole-installation content fingerprint
//! (`schemas/evidence.schema.json`, `docs/contracts/CLI-EVIDENCE.md`). Its
//! original writer was an ad-hoc reviewer sequence that was never committed,
//! so this harness is the report's first committed writer. It reissues the
//! report the way Rally #804 reissued the reports that still described the
//! engine image as installation content:
//!
//! * `source` comes from production discovery — `cs_assets::install::
//!   discover`, `fingerprint` and `content_fingerprint` over `$CS_GAME_DIR` —
//!   so `content_sha256` is the canonical value every other committed report
//!   carries;
//! * `review.identity` is the original review's own text, byte-unchanged, and
//!   the regeneration facts are appended to `review.method` instead (the
//!   #804 convention, owner decision on #1174);
//! * the run facts — `candidate_tree`, `created_at`, `command`, the test
//!   counts, the assertions and the artifacts — are this run's own;
//! * `unknowns`, `seed`, `ticks`, `overrides` and `capabilities` keep their
//!   original values, because nothing in this stage's scope changed.
//!
//! Run it from the workspace root, exactly as:
//!
//! 0. ```sh
//!    cargo build -p cs_inspect --locked
//!    ```
//!    (the production `cs-inspect` binary the harness runs the artifact
//!    commands with; it fails loudly if the binary is missing)
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_f05_d_ --include-ignored \
//!      2>&1 | tee private/evidence/F05-D/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F05-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f05_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_S05_EXTRACTOR=<the pinned extract_rof.py, fetched once and sha256-checked> \
//!      cargo test --locked --test campaign evidence_report_f05_d -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F05-D/acceptance.json \
//!      --artifact-root private/evidence/F05-D --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F05-D.json`.
//!
//! `CS_S05_EXTRACTOR` is the pinned S05 extractor — `rozab/crimsonskies2blend`
//! at commit `214b170bf330041b411634dbb9fb392d54c2db7a`, file
//! `extract_rof.py` — fetched from its public upstream into `private/`
//! (git-ignored, never committed, per `docs/research/FORMAT-NOTES.md`) and
//! verified against the recorded pin
//! `96374b5df3911e1bbaa7400b4637cfe5876afb37e674a735b43541157903588e`
//! **before every use**: this harness re-hashes the file itself and refuses to
//! run anything if a single byte differs. The copy it executes is a patched
//! working copy in the evidence directory whose only change is its `ROF_PATH`
//! constant, exactly the modification the original report recorded.

use super::*;
use cs_assets::rof::mount_rof;
use cs_assets::vfs::MountBuilder;
use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass};

/// The pinned S05 extractor's source, commit and file digest: the three
/// values the harness verifies before it runs anything of the file.
const EXTRACTOR_URL: &str = "https://raw.githubusercontent.com/rozab/crimsonskies2blend/214b170bf330041b411634dbb9fb392d54c2db7a/extract_rof.py";
const EXTRACTOR_COMMIT: &str = "214b170bf330041b411634dbb9fb392d54c2db7a";
const EXTRACTOR_SHA256: &str = "96374b5df3911e1bbaa7400b4637cfe5876afb37e674a735b43541157903588e";
/// The one line of the pinned file the harness changes, spelled as the file
/// spells it (the harness replaces it with the container it is about to read).
const EXTRACTOR_ROF_PATH_LINE: &str = "ROF_PATH = r\"C:\\Program Files (x86)\\Microsoft Games\\Crimson Skies\\GOSDATA\\ASSETS\\crimson.rof\"";

/// The two containers F05-D audited, and the work directory each extraction
/// runs in. 846 members plus 1 is the census the report records.
const CONTAINERS: [(&str, &str); 2] = [
    ("GOSDATA/ASSETS/crimson.rof", "crimson"),
    ("GOSDATA/ASSETS/crimptch.rof", "crimptch"),
];
/// Both containers together, as F05-D measured them: the harness refuses to
/// write a report whose census silently drifted out from under its method.
const MEMBERS_BOTH_CONTAINERS: usize = 847;

/// The seven `accept_f05_d_` tests, in the full spelling their original
/// report recorded (three of them live under `rof::tests::`), each with the
/// evidence entries that row cites. Statuses come from the recorded log.
const ASSERTIONS: &[(&str, &[&str])] = &[
    (
        "accept_f05_d_a_declared_length_that_disagrees_with_the_bytes_is_refused",
        &["cargo-test.log"],
    ),
    (
        "accept_f05_d_overlaps_are_computed_from_the_stored_extent",
        &["cargo-test.log"],
    ),
    (
        "accept_f05_d_retail_members_tile_their_container_and_match_the_reference_profile",
        &["cargo-test.log", "reference-comparison.json"],
    ),
    (
        "accept_f05_d_the_decoded_word_is_not_a_range_of_the_container",
        &["cargo-test.log"],
    ),
    (
        "rof::tests::accept_f05_d_rof_audit_reports_both_words_for_every_member",
        &["cargo-test.log"],
    ),
    (
        "rof::tests::accept_f05_d_the_help_text_and_usage_diagnostic_document_the_audit_flag",
        &["cargo-test.log"],
    ),
    (
        "rof::tests::accept_f05_d_the_mount_records_stored_extents_that_tile_the_container",
        &["cargo-test.log"],
    ),
];

/// The original report's `review.method`, byte for byte. The regeneration
/// facts are appended to it, never substituted for it.
const ORIGINAL_METHOD: &str = "Regenerated by the reviewing agent on the rebased commit, as docs/contracts/CLI-EVIDENCE.md requires of a reviewer. The F05-D acceptance suite was run with the retail capability available, so the ignored test ran too: seven tests, all passing (cargo-test.log). The audit of each container is the production `cs-inspect rof --audit` report, which reads every member through the production reader and reports both length words, the decoded count, trailing bytes, the stored and decoded digests and the container's byte coverage. The independent reference is the pinned S05 extractor (commit 214b170bf330041b411634dbb9fb392d54c2db7a, file sha256 96374b5df3911e1bbaa7400b4637cfe5876afb37e674a735b43541157903588e), re-run unmodified except for its ROF_PATH constant and the empty data/ directory it assumes; its per-file sha256 digests were compared with the production decoded digests for every member of both containers, and reference-comparison.json records the result. The reviewer additionally re-derived every stored_sha256 in both audits from the container bytes (847/847 match) and re-measured the whole census, the exact tiling and the first-word profile's 418 overlapping spans and its [127, 1768) out-of-bounds extent with a from-scratch parser that does not use the Rust reader, so none of these numbers rests on the code under test. install_sha256 is the installation fingerprint the production discovery reports; content_sha256 is the sha256 of the two containers' relative paths and digests, sorted, joined with newlines (see the F05-D findings for the individual container digests). No seed, no ticks and no overrides: this stage reads containers, it does not simulate. The report's `unknowns` is empty because nothing in the report's scope is unresolved; the research questions that remain open (a directory record's length words, flag bits other than 1 and 2, non-UTF-8 names, non-zero trailing bytes) are recorded as unknowns in docs/findings/2026-09-28-f05-d-resolve-compressed-length-semantics.md and are not guessed here. The claim is `implemented`, not `checked`: only the merge of this reviewed commit awards `checked`, and neither this agent nor any agent may self-award `verified_original`.";

/// What this reissue adds to `review.method`: what changed, what the artifacts
/// are and why `reference-comparison.json` is a fresh artifact.
const REGENERATION: &str = "Regeneration for Rally #1174 (EVIDENCE-CONTENT-DIGEST-OUTLIERS-REPAIR) on 2026-10-10 by bunny-2/bunny-2: the report was reissued by the committed harness crates/cs_app/tests/campaign/evidence/f05_d.rs, run through the four-step sequence in its module doc, because the sentence above recorded a task-scoped digest in source.content_sha256 — a field whose contract meaning is the canonical whole-installation content fingerprint. source.content_sha256 is now cs_assets::install::content_fingerprint over the manifest production discovery reads (a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d), the value every other committed report carries; the two-container scoped value the sentence above describes (869f5afcfa632f19c9cfef7a4a1fa1024471a15b7747fab373513a61b50cd23e) still re-derives from the same installation bytes through production code in accept_evidence_content_digest_outliers_each_documented_scoped_digest_still_re_derives, and docs/findings/2026-10-10-evidence-content-digest-outliers.md holds the measurement. review.identity above is byte-unchanged: the implementer and reviewer facts stand exactly as the original review wrote them, and this regeneration is recorded here instead of in the identity (#804 convention). Run facts are this run's own: candidate_tree, created_at, command.cwd, the test counts, the assertion statuses and the artifact digests come from the sequence above rather than being carried over, and the assertion ids keep the full spelling the original report recorded. Artifacts: cargo-test.log is step 1's log; rof-audit-crimson.json (c00357ba7dc032a12dc9b51cc37d702c8652abc47f863adcdf3050d31b670799) and rof-audit-crimptch.json (55f9be89ca8db3c01656b100b8ad092f5de8e97a092b71f5c5eb08ac49bcd07b) are the production cs-inspect rof --audit reports, produced by this harness with the production cs-inspect binary itself — one process per container, exactly as the original report's sequence ran it, so both reproduce the digests the original report recorded byte for byte; reference-comparison.json is a FRESH artifact in this harness's documented format, because the ad-hoc comparison sequence that produced the original bytes was never committed and those bytes are unrecoverable under any option. What the new artifact compares: the pinned S05 extractor's per-file sha256 output (source above, re-hashed against its recorded digest before every use, patched only in its ROF_PATH constant) against the production ROF reader's decoded digests, member by member, over both containers — 847 members. The claim stays implemented: a merge awards checked, never verified_original, and nothing here observes the original game running. tools/validate_evidence.py --require-pass applies, because this report's unknowns stay empty. `candidate_tree` is the tree of the commit the suite and this harness ran on; the only later delta is this report's own copy under docs/findings/evidence/, which the sentinel tests read only for its source fingerprints.";

/// The original review's identity, byte for byte: the regeneration must never
/// rewrite who implemented or reviewed this report.
fn review_identity() -> String {
    "implementer: bunny-1/bunny-1 (Rally #24, implement claim of 2026-09-28T20:29:00Z, handed over at 21:20:37Z), which ran the accept_f05_d_ selection; reviewer: bunny-1/bunny-1 again, on the review claim of 2026-09-28T21:20:49Z, which re-ran the accept_f05_d_ selection with the retail capability and regenerated this report on the reviewed and rebased commit before merging it at 22:36:52Z. The same agent instance is on both sides, so this review is not independent and is not independent original-reference evidence; the review claim started twelve seconds after the hand-over, so the activity log cannot prove a fresh context and none is claimed. No agent review replaces the owner's human approval".to_owned()
}

/// Evidence-report harness for task F05-D (Rally #24), reissued under #1174.
/// It refuses every missing input: a report written from a partial sequence
/// would be worse than no report at all.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR, CS_S05_EXTRACTOR"]
fn evidence_report_f05_d_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    fs::create_dir_all(&evidence_dir)
        .unwrap_or_else(|error| panic!("create {}: {error}", evidence_dir.display()));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        argv.iter().any(|arg| arg == "accept_f05_d_"),
        "CS_EVIDENCE_ARGV must be F05-D's own acceptance selection, got {argv:?}"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_f05_d_suite(&log);
    assert_eq!(
        suite.assertions.len(),
        ASSERTIONS.len(),
        "the log holds {} accept_f05_d_ tests and the report records {}: {:?}",
        suite.assertions.len(),
        ASSERTIONS.len(),
        suite.assertions
    );

    // The production audit of each container, written straight into the
    // evidence directory by the function `cs-inspect rof --audit` runs.
    for (container, name) in CONTAINERS {
        run_rof_audit(
            &game_dir,
            container,
            &evidence_dir.join(format!("rof-audit-{name}.json")),
        );
    }

    write_reference_comparison(&game_dir, &evidence_dir);

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(
            &evidence_dir.join("rof-audit-crimson.json"),
            "json",
            &evidence_dir,
        ),
        artifact(
            &evidence_dir.join("rof-audit-crimptch.json"),
            "json",
            &evidence_dir,
        ),
        artifact(
            &evidence_dir.join("reference-comparison.json"),
            "json",
            &evidence_dir,
        ),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let method = format!("{ORIGINAL_METHOD} {REGENERATION}");
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F05-D\",\n\
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
        tabled_assertions(&log, ASSERTIONS),
        artifact_array(&artifacts),
        jstr(&review_identity()),
        jstr(&method),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    let content_line = format!("\"content_sha256\": \"{content_sha256}\"");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F05-D\"",
        "\"claim\": \"implemented\"",
        content_line.as_str(),
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

/// [`parse_suite_prefixed`] with F05-D's own test prefix.
fn parse_f05_d_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_f05_d_")
}

/// One `cs-inspect rof --audit` run over a container, written to `out` by the
/// production command itself — one process per container, exactly as the
/// original report's ad-hoc sequence ran it, so both reports reproduce the
/// bytes (including the session generation) the CLI writes standalone.
fn run_rof_audit(game_dir: &Path, container: &str, out: &Path) {
    let cs_path = game_dir.display().to_string();
    let out_path = out.display().to_string();
    let args = [
        "rof",
        "--cs-path",
        cs_path.as_str(),
        "--container",
        container,
        "--audit",
        "--out",
        out_path.as_str(),
    ];
    run_cs_inspect(
        &cs_inspect_binary(),
        &args,
        out,
        &format!("rof --audit {container}"),
    );
}

/// Every file under `root`, as `(member spelling, path)`, sorted by spelling
/// so the comparison and the artifact it writes are byte-deterministic.
fn walk_files(root: &Path, prefix: &str, files: &mut Vec<(String, PathBuf)>) {
    let entries =
        fs::read_dir(root).unwrap_or_else(|error| panic!("read {}: {error}", root.display()));
    for entry in entries {
        let entry = entry.expect("a directory entry");
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            walk_files(&path, &format!("{prefix}{name}/"), files);
        } else {
            files.push((format!("{prefix}{name}"), path));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
}

/// Runs the pinned extractor over both containers and writes
/// `reference-comparison.json`: the extractor's per-file digests against the
/// production reader's decoded digests, member by member.
///
/// This is the artifact the original report recorded as produced by an ad-hoc
/// sequence that was never committed. Its format is this harness's, documented
/// in full in the report's `review.method`.
fn write_reference_comparison(game_dir: &Path, evidence_dir: &Path) {
    let extractor = workspace_path(&env_var("CS_S05_EXTRACTOR"));
    let pristine = fs::read(&extractor)
        .unwrap_or_else(|error| panic!("read {}: {error}", extractor.display()));
    let measured = sha256(&pristine).to_hex();
    assert_eq!(
        measured,
        EXTRACTOR_SHA256,
        "{} is not the pinned S05 extractor extract_rof.py at {} ({}) — re-fetch it from {} \
         and never commit it",
        extractor.display(),
        EXTRACTOR_COMMIT,
        EXTRACTOR_SHA256,
        EXTRACTOR_URL
    );
    let pristine = String::from_utf8(pristine)
        .expect("the pinned extractor is the text the recorded digest covers");
    assert!(
        pristine.contains(EXTRACTOR_ROF_PATH_LINE),
        "the pinned extractor no longer spells its ROF_PATH constant as recorded: the pin \
         cannot be trusted, stop and re-verify {}",
        EXTRACTOR_SHA256
    );

    let work_root = evidence_dir.join("reference-extraction");
    let mut containers_json = Vec::new();
    let mut total_members = 0usize;
    let mut total_matches = 0usize;

    for (container, name) in CONTAINERS {
        let work = work_root.join(name);
        if work.exists() {
            fs::remove_dir_all(&work)
                .unwrap_or_else(|error| panic!("clear {}: {error}", work.display()));
        }
        let data = work.join("data");
        fs::create_dir_all(&data)
            .unwrap_or_else(|error| panic!("create {}: {error}", data.display()));
        let patched = pristine.replace(
            EXTRACTOR_ROF_PATH_LINE,
            &format!("ROF_PATH = r\"{}\"", game_dir.join(container).display()),
        );
        let script = work.join("extract_rof.py");
        fs::write(&script, patched)
            .unwrap_or_else(|error| panic!("write {}: {error}", script.display()));
        let run = Command::new("python3")
            .arg(&script)
            .current_dir(&work)
            .output()
            .expect("python3 runs the pinned extractor");
        assert!(
            run.status.success(),
            "the pinned extractor failed on {container}: {}",
            String::from_utf8_lossy(&run.stderr)
        );

        let mut extracted = Vec::new();
        walk_files(&data.join("rof_output"), "", &mut extracted);

        // The production reader's own member set and decoded bytes.
        let builder = MountBuilder::new(
            MountId::new(&format!("rof-reference-{name}")).expect("a valid mount id"),
            MountNamespace::new("install").expect("a valid namespace"),
            PrecedenceClass::Shared,
            container,
        )
        .retail();
        let mounted = mount_rof(builder, &game_dir.join(container))
            .unwrap_or_else(|error| panic!("{container} mounts: {error:?}"));
        let key = |spelling: &str| {
            AssetKey::from_spelling("install", spelling, "default")
                .expect("a member spelling is a key")
        };
        let mut members: Vec<String> = mounted
            .source
            .members()
            .map(|member| member.spelling.clone())
            .collect();
        members.sort();
        let spellings: Vec<&str> = extracted
            .iter()
            .map(|(spelling, _)| spelling.as_str())
            .collect();
        assert_eq!(
            spellings,
            members.iter().map(String::as_str).collect::<Vec<_>>(),
            "{container}: the extraction and the production reader disagree on the member set"
        );

        let mut matches = 0usize;
        let mut mismatches = Vec::new();
        let mut rows = Vec::with_capacity(extracted.len());
        for (spelling, path) in &extracted {
            let bytes =
                fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let from_extractor = sha256(&bytes).to_hex();
            let read = mounted
                .source
                .read(&key(spelling))
                .unwrap_or_else(|error| panic!("{spelling} reads: {error:?}"));
            let from_reader = sha256(&read.data).to_hex();
            if from_extractor == from_reader {
                matches += 1;
            } else {
                mismatches.push(spelling.clone());
            }
            rows.push(format!(
                "[{}, {}, {}]",
                jstr(spelling),
                jstr(&from_extractor),
                jstr(&from_reader)
            ));
        }
        total_members += extracted.len();
        total_matches += matches;
        containers_json.push(format!(
            "{{\"container\": {}, \"extracted_files\": {}, \"members\": {}, \
             \"decoded_digest_matches\": {}, \"mismatches\": [{}], \"rows\": [{}]}}",
            jstr(container),
            extracted.len(),
            members.len(),
            matches,
            str_array(&mismatches),
            rows.join(", ")
        ));
        // The extracted originals are the reference's scratch: the digests are
        // recorded above, and the pinned file can reproduce them at any time.
        fs::remove_dir_all(&data)
            .unwrap_or_else(|error| panic!("remove {}: {error}", data.display()));
    }

    assert_eq!(
        total_members, MEMBERS_BOTH_CONTAINERS,
        "F05-D measured {} members over both containers",
        MEMBERS_BOTH_CONTAINERS
    );
    assert_eq!(
        total_matches, total_members,
        "the pinned extractor's digests and the production reader's decoded digests must \
         agree member for member"
    );

    let json = format!(
        "{{\n\
         \x20\"report\": \"reference-comparison/1\",\n\
         \x20\"produced_by\": {},\n\
         \x20\"compares\": {},\n\
         \x20\"extractor\": {{\"source\": {}, \"commit\": {}, \"file_sha256\": {}, \
         \"modification\": {}}},\n\
         \x20\"containers\": [\n\x20\x20{}],\n\
         \x20\"totals\": {{\"members\": {}, \"decoded_digest_matches\": {}}}\n\
         }}\n",
        jstr("crates/cs_app/tests/campaign/evidence/f05_d.rs"),
        jstr(
            "the pinned S05 extractor's per-file sha256 output against the production ROF \
             reader's decoded digests, member by member, over both containers"
        ),
        jstr(EXTRACTOR_URL),
        jstr(EXTRACTOR_COMMIT),
        jstr(EXTRACTOR_SHA256),
        jstr("ROF_PATH constant only; every other byte is the pinned file"),
        containers_json.join(",\n\x20\x20"),
        total_members,
        total_matches,
    );
    let out = evidence_dir.join("reference-comparison.json");
    fs::write(&out, &json).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
}
