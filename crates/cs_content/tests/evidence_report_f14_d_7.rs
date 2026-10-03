//! Evidence-report harness for task F14-D.7 (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f14_d_7_*`: it is not part of
//! the acceptance suite, and it fails loudly when its inputs are missing instead
//! of passing vacuously. Run from the workspace root, after the acceptance
//! suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F14-D.7
//!    cargo test --workspace --locked -- accept_f14_d_7_ --include-ignored \
//!      2>&1 | tee private/evidence/F14-D.7/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F14-D.7 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f14_d_7_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test evidence_report_f14_d_7 -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F14-D.7/acceptance.json \
//!      --artifact-root private/evidence/F14-D.7 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F14-D.7.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded test
//! log, the environment, production discovery of `$CS_GAME_DIR`, the production
//! baseline builder's own report over that installation (the consumer trace),
//! `rustc --version` and `Cargo.lock`. The only texts this file holds are the
//! [`review_identity`] literal the committed
//! `docs/findings/evidence/F14-D.7.json` carries — which now names the review of
//! Rally #490 that actually ran, and says plainly that it was not independent —
//! and the product-coverage limitations quoted into `review.method`; everything
//! else is measured.
//!
//! `unknowns` is `[]` and the report validates with `--require-pass`: the
//! **task's** acceptance is complete — the `sound` collection is populated from
//! the installation's own bytes by the producing stage's sound reader, `music`
//! and `dialogue` name the reason they hold no row instead of vanishing from the
//! accounting report, the denominator they must not move did not move, and every
//! selected test passed. The product incompleteness is a different state and is
//! moved, never deleted (the 2026-09-28 owner directive, and the same split F14-D
//! documents): the collections that still hold no row are named in
//! `UNKNOWN_LIMITATIONS` below, in `review.method`, in the hashed baseline-report
//! artifact, in
//! `docs/findings/2026-10-03-f14-d-7-sound-cue-collection.md` and in the
//! follow-up tasks. A failing run produces a failing report, which the validator
//! rejects.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::campaign_bindings::{CampaignInventory, campaign_layout};
use cs_content::catalog::baseline::{
    Baseline, SOUND_CONTAINER_PATTERN, baseline_report_json, retail_baseline,
};
use cs_types::content::{CatalogElement, ContentKind};

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f14_d_7_writes_the_acceptance_report() {
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
        "no `accept_f14_d_7_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: the report declares
    // `retail` only because the retail acceptance test is in this log.
    let retail_line = suite
        .assertions
        .iter()
        .find(|(name, _)| name.contains("accept_f14_d_7_retail_"))
        .unwrap_or_else(|| {
            panic!(
                "the retail acceptance test did not run: F14-D.7 requires capability `retail`, \
                 run step 1 with `--include-ignored` and CS_GAME_DIR set"
            )
        });
    assert_eq!(
        retail_line.1, "pass",
        "the retail acceptance test must pass; got status {}",
        retail_line.1
    );
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| name.contains("accept_f14_d_7_")
                && !name.contains("accept_f14_d_7_retail_")),
        "synthetic task tests must be present alongside the retail one"
    );

    // `source` hashes describe the real installation, measured by the very
    // production code the task's consumer wires in.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The consumer trace: the production baseline builder's own report over the
    // original installation, written into the evidence directory. It is the same
    // function `cs-inspect catalog --cs-path` writes.
    let report_path = evidence_dir.join("baseline-report.json");
    let baseline = retail_baseline(&game_dir)
        .expect("the production baseline must read the original installation");
    assert_eq!(
        baseline.install_sha256, install_sha256,
        "the baseline's installation fingerprint is production discovery's"
    );

    // The collection this task adds, measured from the rows themselves.
    let rows: Vec<&CatalogElement> = baseline
        .catalog
        .elements()
        .filter(|element| element.kind == ContentKind::Sound)
        .collect();
    assert_eq!(
        rows.len(),
        4951,
        "the two sound containers declare 2475 + 2476 cues once byte-identical repeats and the \
         one case-folded name are resolved; a different count must be re-measured and the \
         acceptance test updated, never silently accepted"
    );
    assert_eq!(
        count_kind(&baseline, ContentKind::Music),
        0,
        "no member name is a cue class: the music collection holds no row"
    );
    assert_eq!(
        count_kind(&baseline, ContentKind::Dialogue),
        0,
        "no member name is a cue class: the dialogue collection holds no row"
    );

    // Every sound row is a member of a sound container, located by that member's
    // own bytes and pointing at the container's inventory row. The spans are read
    // back out of the read-only installation here, so the report does not rest on
    // the builder's own bookkeeping.
    let mut low = 0usize;
    let mut high = 0usize;
    for row in &rows {
        assert!(
            row.origin.is_original(),
            "{} must be installation data",
            row.id
        );
        let span = row
            .origin
            .source()
            .expect("a sound row is located by a checked span");
        let member = span
            .member_key()
            .unwrap_or_else(|| panic!("{} must name its member", row.id));
        assert!(
            member.to_ascii_lowercase().ends_with(".wav"),
            "{}: a sound cue is a recording",
            row.id
        );
        assert!(span.length() > 0, "{}: a member has bytes", row.id);
        assert_eq!(span.install_sha256().to_hex(), install_sha256);
        assert_eq!(
            span.member_sha256().map(|digest| digest.to_hex()),
            Some(
                sha256(&read_member(
                    &game_dir,
                    span.container_path(),
                    span.offset(),
                    span.length(),
                ))
                .to_hex()
            ),
            "{}: the span names the member's own stored bytes",
            row.id
        );
        assert!(
            !row.unsupported_reasons.is_empty(),
            "{} is honest about what it does not state",
            row.id
        );
        assert!(
            row.runtime_consumers.is_empty(),
            "{} must not claim a media player consumer without evidence",
            row.id
        );
        assert_eq!(
            row.dependencies.len(),
            1,
            "{}: one static edge onto the container's inventory row",
            row.id
        );
        assert_eq!(
            row.dependencies[0].target,
            Baseline::install_file_id(span.container_path()).expect("the container's inventory id"),
            "{}: the edge target is that container's row",
            row.id
        );
        assert_eq!(row.dependencies[0].kind.label(), "static");
        match span.container_path().to_ascii_lowercase().as_str() {
            "zbd/soundsl.zbd" => low += 1,
            "zbd/soundsh.zbd" => high += 1,
            other => panic!("{other} is not a sound container of this installation"),
        }
    }
    assert_eq!(low, 2475);
    assert_eq!(high, 2476);

    // The collection record: rows, no diagnostic, the byte-identical repeats
    // counted rather than duplicated.
    let sound = baseline
        .collection_status
        .iter()
        .find(|status| status.kind == ContentKind::Sound)
        .expect("the sound collection reports its status");
    assert_eq!(sound.source, SOUND_CONTAINER_PATTERN);
    assert_eq!(sound.language, None);
    assert_eq!(sound.rows, rows.len());
    assert_eq!(
        sound.gaps,
        BTreeMap::from([("duplicate_member", 90)]),
        "every member of both archives is a row except the byte-identical repeats, so any other \
         gap code means a member stopped being accounted for and this report's counts have to be \
         re-measured"
    );
    assert_eq!(sound.diagnostic, None);
    for kind in [ContentKind::Music, ContentKind::Dialogue] {
        let status = baseline
            .collection_status
            .iter()
            .find(|status| status.kind == kind)
            .unwrap_or_else(|| panic!("the {kind} collection names itself"));
        assert_eq!(status.source, SOUND_CONTAINER_PATTERN);
        assert_eq!(status.rows, 0);
        assert!(
            status.diagnostic.is_some(),
            "a collection that holds no row must say why, not vanish"
        );
    }

    // The sound collection added no launchable row: the denominator is still the
    // campaign missions plus the scenario directories F14-D.1 classified. That is
    // what makes the coverage claim honest, so it is measured here rather than
    // asserted from memory.
    assert!(
        !ContentKind::Sound.is_launchable()
            && !ContentKind::Music.is_launchable()
            && !ContentKind::Dialogue.is_launchable(),
        "no audio kind is launchable; if this ever changes the denominator moved and this \
         report must say so"
    );
    let layout = campaign_layout(&game_dir).expect("the shared campaign walk reads the layout");
    let scenarios = baseline
        .classified_reader_dirs
        .iter()
        .filter(|dir| dir.role.is_launchable())
        .count();
    assert_eq!(
        baseline.roots.len(),
        layout.len() + scenarios,
        "the denominator declares exactly the campaign missions the shared walk finds plus the \
         scenario directories F14-D.1 classified; this stage declares no root"
    );
    let inventory_path = workspace_root().join("missions/bindings/campaign-inventory.tsv");
    let inventory = CampaignInventory::load(&inventory_path)
        .unwrap_or_else(|error| panic!("{} reads: {error}", inventory_path.display()));
    assert_eq!(
        layout.len(),
        inventory.len(),
        "the campaign part of the denominator equals the frozen F50 campaign denominator"
    );
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Sound),
        "the sound collection is not a closure root"
    );
    assert_eq!(
        baseline.catalog.launchable_count(),
        baseline.roots.len(),
        "every launchable row is a declared root, and this stage declares none"
    );

    let report = baseline_report_json(&baseline);
    fs::write(&report_path, &report)
        .unwrap_or_else(|error| panic!("write {}: {error}", report_path.display()));
    for needle in [
        "\"schema\":\"cs-content-baseline/1\"",
        "\"retail\":true",
        "\"synthetic_launchable\":0",
        &format!("\"install_sha256\":\"{install_sha256}\""),
        &format!("\"launchable\":{}", baseline.roots.len()),
        &format!("\"sound\":{}", rows.len()),
        "\"duplicate_member\":90",
        "\"kind\":\"sound\"",
        "\"kind\":\"music\"",
        "\"kind\":\"dialogue\"",
        "\"collection_status\":[",
        "\"unrecognized_program_dirs\":[",
    ] {
        assert!(
            report.contains(needle),
            "the consumer report is missing {needle:?}"
        );
    }
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&report_path, "json", &evidence_dir),
    ];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F14-D.7\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \
         \"ignored\": {}}},\n\
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
        // Deliberately empty (the brackets are the template's): see the
        // module doc — the task's own acceptance is complete, and the
        // product-completeness limits live in `review.method`, the hashed
        // artifact, `docs/findings/` and the follow-up tasks instead of being
        // deleted.
        "",
        jstr(&review_identity()),
        jstr(&review_method()),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    // A cheap self-check without a JSON dependency: the validator runs next,
    // but a structurally empty write must fail here first.
    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F14-D.7\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
        "\"unknowns\": [],",
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

/// How many catalog rows the baseline holds of `kind`.
fn count_kind(baseline: &Baseline, kind: ContentKind) -> usize {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == kind)
        .count()
}

/// Reads the exact bytes a row's span names, straight from the read-only
/// installation.
fn read_member(dir: &Path, container: &str, offset: u64, length: u64) -> Vec<u8> {
    use std::io::{Read as _, Seek as _};
    let mut file = fs::File::open(dir.join(container)).expect("the container opens");
    file.seek(std::io::SeekFrom::Start(offset))
        .expect("the member offset is seekable");
    let mut bytes = vec![0u8; usize::try_from(length).expect("fits")];
    file.read_exact(&mut bytes)
        .expect("the member's own bytes read");
    bytes
}

/// The `review.identity` this harness records.
///
/// The literal below is the text this repository's committed
/// `docs/findings/evidence/F14-D.7.json` carries, and it is the first string in
/// this function on purpose: `tools/tests/test_evidence_review_identity.py`
/// reads a harness's recorded identity out of exactly this shape
/// (`fn review_identity() -> String`, a literal) and cross-checks it against the
/// committed report, so the text here and the report cannot drift apart
/// silently.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW` and
/// replaces the literal with it in the same commit, which is how the report
/// names the review that actually happened. The literal below already carries the
/// review of Rally #490, so a later review of the same branch replaces it again
/// rather than leaving this one standing.
fn review_identity() -> String {
    let recorded = String::from(
        "implementer: openrouter/stealth-space-bunny-alpha (bunny-alpha-2, Rally #490, implement \
         claim of 2026-10-03T06:16:44Z, branch commit 1fd3d88f). reviewer: \
         openrouter/stealth-space-bunny-alpha (bunny-alpha-2, Rally #490, review claim of \
         2026-10-03T07:26:53Z) — the same agent instance and model as the implementer, so this \
         review is not independent evidence: it is a self-review and it cannot stand in for a \
         fresh-context agent or for the owner's human approval. Its context was not fresh: this \
         session started from the implementer's own hand-over summary and read the branch diff from \
         there, so the summary was treated as a claim to re-check rather than as evidence; every \
         retail count below was re-derived from the installation, and what the re-checking changed \
         is listed under \"Review corrections\" in \
         docs/findings/2026-10-03-f14-d-7-sound-cue-collection.md. \
         `checked` is the ceiling for an agent review",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records: what was run, and the
/// product-completeness limits this report does not claim away.
///
/// The limitations are quoted here rather than deleted from the record (the
/// 2026-09-28 owner directive), so a reader of the committed report sees them
/// next to the passing assertions.
fn review_method() -> String {
    let recorded = String::from(
        "acceptance suite run locally with the retail capability; this harness derives \
         every field from the recorded log, production discovery of $CS_GAME_DIR, the \
         production baseline report over that installation, rustc and Cargo.lock; \
         validated with tools/validate_evidence.py --require-pass. The consumer trace is \
         cs_content::catalog::baseline::retail_baseline + baseline_report_json, the same \
         functions `cs-inspect catalog --cs-path` writes. The sound rows are the members \
         the producing stage's own readers listed: the containers are chosen by \
         cs_formats::zbd::role_for_path and re-checked with cs_formats::zbd::dispatch, and \
         each is read through cs_formats::zbd::read_version_one_index + \
         cs_formats::zbd::read_sound_archive, so no reader was derived for this task. A \
         cue's identity is its container plus the name that container's own member index \
         declares, never a bare member name (the same name is declared in both containers \
         with different bytes) and never a position in a walk; its span is that member's \
         own extent with the container path, the member key and the member's digest, and \
         its one static edge points at the container's inventory row. A name declared \
         twice with identical bytes is one cue with the repeat counted \
         (duplicate_member), and a name declared twice with different bytes is no row at \
         all (ambiguous_member_name); a member whose extent is not inside the container, \
         whose name is not keyable text, or whose RIFF/WAVE header does not read is a named \
         gap under the producing reader's own code. Implementer mutation probes on this \
         branch: removing the sound_rows call from retail_baseline, keying a cue by the \
         member name without the container, turning a member with an unreadable header \
         into a row, minting a music or dialogue row from a member-name prefix, and \
         dropping the byte-identical repeats instead of counting them each fail the \
         acceptance tests named in the finding.",
    );
    recorded
        + &UNKNOWN_LIMITATIONS
            .iter()
            .map(|limitation| format!(" LIMITATION: {limitation}"))
            .collect::<String>()
}

/// The limitations this stage records instead of guessing, each naming the
/// affected content and the task that resolves it (AGENTS owner directive,
/// 2026-09-28: a limitation must survive into machine-readable evidence).
const UNKNOWN_LIMITATIONS: &[&str] = &[
    "A member the sound family could not turn into a row is a named gap, not a row: IDENTITY-CONTENT \
     requires a collection to keep failed entries (`an opaque unparsed member is still an inventory \
     row`), while this stage's own rule is that a cue named only by a member name is not a row. On \
     this installation the two rules cost nothing measurable — all 5041 members of both archives read \
     their RIFF/WAVE header and are accounted for as 4951 rows plus 90 counted byte-identical repeats \
     — but a member whose extent is out of bounds, whose name is not keyable text, whose name has no \
     valid id key, or whose header does not read is currently counted in collection_status.gaps \
     instead of being given an inventory row with parse_state `failed`. Affected content: any such \
     member in another installation or another build. Resolving task: the owner rules which of the \
     two statements wins (a reconciliation of this stage's rule with IDENTITY-CONTENT, to be taken \
     up by the next F14-D collection follow-up); until then both readings are recorded here and in \
     docs/findings/2026-10-03-f14-d-7-sound-cue-collection.md.",
    "Nine of the collections IDENTITY-CONTENT requires are now populated: install files, \
     campaign missions, mission programs, the instant-action and multiplayer scenario \
     directories (F14-D.1), multiplayer rules (F14-D.2), world groups (F14-D.3), factions \
     and paint masks (F14-D.5) and sound cues (this stage). Affected content: world \
     variants, scene nodes, meshes, materials and images, airframes and flight equipment, \
     weapons, music, dialogue, video, stunts, scrapbook rewards and legacy custom-plane \
     resources, which still have no source-derived row. Resolving tasks: the follow-up \
     tasks created from #389, one per collection, each adding at most the rows its producing \
     stage's parser can honestly produce; the report's collections and collection_status \
     objects state what exists today.",
    "No container family in this installation states a cue class. The ZBD sound family names \
     every member as a cue and stores a recording, so the music and dialogue collections hold \
     no row; members whose name begins `music_` and the `VO_` voice lines stay name \
     observations. Affected content: every music cue's bus and transition rules and every \
     dialogue cue's speaker, text binding and radio ordering. Resolving tasks: F39 (dialogue \
     cues) and F33-C (AI roles, dialogue voices and mission callbacks) once a mission program \
     names a cue with its speaker, and F41-B/F41-C (mixing, music and radio routing) plus an \
     original-run capture; the sound containers themselves declare no class.",
    "A sound row is one readable recording: its header was decoded and its bytes located, but \
     no sample is decoded and nothing is normalized, so every row carries \
     `f14.d.7.baseline.sound_playback` as an explicit unknown for bus, level, one-shot/loop \
     mode and runtime consumer. Affected content: every cue's audible playback. Resolving \
     tasks: #444 (ADPCM decode, since most retail members are MS/IMA ADPCM), F06-C \
     (sound_sample decoding) and #445 (mixer/device consumer); no retail member declares a \
     `smpl` chunk, so loop points stay unknown for every cue (the 2026-09-28 T344 finding).",
    "Nothing states which of the two sound containers the original engine played for a given \
     cue: `soundsl.zbd` and `soundsh.zbd` declare 2476 and 2477 names of which 565 differ in \
     bytes, a low-rate and a high-rate recording of one cue. Affected content: which bytes \
     the original engine played, and therefore any rate-dependent behavior. Resolving task: \
     an original-run capture or a reference that shows the selection, because nothing in the \
     containers states it.",
    "No row claims a runtime consumer, so the coverage accounting reports 0 ready and the \
     sound collection stays unreachable from the declared roots, counted in \
     coverage.unreachable_by_kind. Affected content: every cue's reachability and readiness. \
     Resolving tasks: F37 (mission IR) and F38 (native behavior bindings), which are what \
     name a cue from a mission program.",
];

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f14_d_7.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F14-D.7` written relative to the
/// workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    workspace_root().join(path)
}

/// The workspace root, located through git rather than through the package
/// layout, so the harness cannot read a neighbouring checkout by accident.
fn workspace_root() -> PathBuf {
    PathBuf::from(git(&["rev-parse", "--show-toplevel"]))
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
    let lock_path = workspace_root().join("Cargo.lock");
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
/// `accept_f14_d_7_` tests from a recorded `cargo test` output.
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
            // `cargo test -- <prefix>` matches the prefix anywhere in the test
            // name, so a unit test inside a module counts exactly as the
            // selection counts it.
            if !name.contains("accept_f14_d_7_") {
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

// ------------------------------------------------------------ artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 the sibling
/// crate implements (the validator re-hashes it with `hashlib` independently).
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

// ------------------------------------------------------------ rendering ---

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
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat` accepts
/// after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC calendar
/// date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
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
