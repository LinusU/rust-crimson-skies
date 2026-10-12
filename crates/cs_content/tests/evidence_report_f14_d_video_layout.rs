//! Evidence-report harness for task F14-D.VIDEO-LAYOUT
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f14_d_video_layout_*`: it
//! is not part of the acceptance suite, and it fails loudly when its inputs
//! are missing instead of passing vacuously. Run from the workspace root,
//! after the acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F14-D.VIDEO-LAYOUT
//!    cargo test --workspace --locked -- accept_f14_d_video_layout_ --include-ignored \
//!      2>&1 | tee private/evidence/F14-D.VIDEO-LAYOUT/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F14-D.VIDEO-LAYOUT \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f14_d_video_layout_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test evidence_report_f14_d_video_layout -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F14-D.VIDEO-LAYOUT/acceptance.json \
//!      --artifact-root private/evidence/F14-D.VIDEO-LAYOUT --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F14-D.VIDEO-LAYOUT.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded
//! test log, the environment, production discovery of `$CS_GAME_DIR`, the
//! production baseline builder's own report over that installation (the
//! consumer trace), the production ROF reader over the same container the
//! collection reads, `rustc --version` and `Cargo.lock`. The only texts this
//! file holds are the [`review_identity`] literal the committed
//! `docs/findings/evidence/F14-D.VIDEO-LAYOUT.json` carries and the
//! product-coverage limitations quoted into `review.method`; everything else
//! is measured.
//!
//! `unknowns` is `[]` and the report validates with `--require-pass`: the
//! **stage's** acceptance is complete — the `video` collection is populated
//! from two sources, `ZBD/zrdr.zbd`'s `fmv.zrd` `PLAYAVI` actions and the
//! `M` (movie) records of `GOSDATA/ASSETS/crimson.rof`'s `ASSETS/LAYOUT.CSV`
//! member, every row is located by the named file's own bytes, every edge is
//! located by the record that stated the name (the layout rows inside the
//! decoded member), the denominator it must not move did not move, and every
//! selected test passed. The report does **not** claim the collection is
//! complete: the five `.mpg` files `chap1`…`chap5` that no record of either
//! source reaches — constructed, not named, by
//! `ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT` — the unknown
//! codec/duration/resolution/trigger on every row and the absence of a media
//! player consumer are named in `UNKNOWN_LIMITATIONS` below, in
//! `review.method`, in the hashed baseline-report artifact and in
//! `docs/findings/2026-10-12-f14-d-video-layout-collection.md`, each with
//! the Rally task that resolves it. A failing run produces a failing report,
//! which the validator rejects.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::campaign_bindings::{CampaignInventory, campaign_layout};
use cs_content::catalog::baseline::{
    FMV_ARCHIVE, VIDEO_LAYOUT_CONTAINER, VIDEO_LAYOUT_MEMBER, baseline_report_json,
    install_file_key, retail_baseline,
};
use cs_types::content::ContentKind;

/// The task's own prefix, which the log parser selects on.
const TASK_PREFIX: &str = "accept_f14_d_video_layout_";

/// The retail half of the prefix; its presence in the log is what licenses
/// the `retail` capability below.
const RETAIL_PREFIX: &str = "accept_f14_d_video_layout_retail_";

/// The five video files the two sources name on the owner's installation,
/// the five `.mpg` files no record of either source reaches and the four
/// `CrimFlag.MPG` records the first resolved record leaves over as counted
/// repeats. All three numbers are re-measured here from the rows and the
/// status themselves rather than trusted; the constants are what a drift
/// must be reconciled against.
const RETAIL_VIDEO_ROWS: usize = 5;
const RETAIL_UNNAMED_MPG_FILES: usize = 5;
const RETAIL_DUPLICATE_NAMES: usize = 4;

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f14_d_video_layout_writes_the_acceptance_report() {
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
        "CS_EVIDENCE_ARGV must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; old \
         reports cannot be reused for new code"
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
        "no `{TASK_PREFIX}` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: the report declares
    // `retail` only because the retail acceptance test is in this log.
    let retail_line = suite
        .assertions
        .iter()
        .find(|(name, _)| name.contains(RETAIL_PREFIX))
        .unwrap_or_else(|| {
            panic!(
                "the retail acceptance test did not run: F14-D.VIDEO-LAYOUT requires \
                 capability `retail`, run step 1 with `--include-ignored` and CS_GAME_DIR set"
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
            .any(|(name, _)| name.contains(TASK_PREFIX) && !name.contains(RETAIL_PREFIX)),
        "synthetic task tests must be present alongside the retail one"
    );

    // `source` hashes describe the real installation, measured by the very
    // production code the task's consumer wires in.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The consumer trace: the production baseline builder's own report over
    // the original installation, written into the evidence directory. It is
    // the same function `cs-inspect catalog --cs-path` writes.
    let report_path = evidence_dir.join("baseline-report.json");
    let baseline = retail_baseline(&game_dir)
        .expect("the production baseline must read the original installation");
    assert_eq!(
        baseline.install_sha256, install_sha256,
        "the baseline's installation fingerprint is production discovery's"
    );

    // The layout member, read again through the production ROF mount — the
    // same reader the collection uses — so the edge provenance spans are
    // checked against the member's own stored offset and decoded digest,
    // never against a fixture.
    let mut builder = cs_assets::vfs::SessionBuilder::new(cs_types::asset_id::ResolveContext::new(
        cs_types::evidence::ContentHash::from_hex(&install_sha256)
            .expect("the reported fingerprint is a content hash"),
    ));
    let mount = cs_assets::vfs::MountBuilder::new(
        cs_types::asset_id::MountId::new("rof-video-layout-evidence").expect("a valid mount id"),
        cs_types::asset_id::MountNamespace::new(cs_assets::vfs::INSTALL_NAMESPACE)
            .expect("a valid namespace"),
        cs_types::asset_id::PrecedenceClass::Shared,
        VIDEO_LAYOUT_CONTAINER,
    )
    .retail();
    let source =
        cs_assets::rof::mount_rof_into(&mut builder, mount, &game_dir.join(VIDEO_LAYOUT_CONTAINER))
            .expect("the retail shared archive mounts through the production reader");
    let key = cs_types::asset_id::AssetKey::from_spelling(
        source.namespace().as_str(),
        VIDEO_LAYOUT_MEMBER,
        "default",
    )
    .expect("the layout member key is valid");
    let info = source
        .member(&key)
        .cloned()
        .expect("the layout member is in the container");
    let member = source
        .read(&key)
        .expect("the layout member decodes through the production reader")
        .data;
    let member_sha256 = sha256(&member);

    // The collection this task extends, measured from the rows themselves.
    let video_rows = rows_of(&baseline, ContentKind::Video);
    assert_eq!(
        video_rows.len(),
        RETAIL_VIDEO_ROWS,
        "the five files the two sources name are the five rows; a different count must be \
         re-measured and this test updated, never silently accepted"
    );
    let status = baseline
        .collection_status
        .iter()
        .find(|status| status.kind == ContentKind::Video)
        .expect("the video collection reports its status");
    assert_eq!(
        status.source,
        format!("{FMV_ARCHIVE} + {VIDEO_LAYOUT_CONTAINER}"),
        "the status names both containers the stage consults"
    );
    assert_eq!(
        status.diagnostic, None,
        "both video sources read on the owner's installation"
    );
    assert_eq!(status.rows, video_rows.len());
    for (code, count) in &status.gaps {
        match *code {
            "unnamed_video_file" => assert_eq!(
                *count, RETAIL_UNNAMED_MPG_FILES,
                "the five .mpg files no record of either source reaches are the only \
                 unaccounted-for entries"
            ),
            "duplicate_video_name" => assert_eq!(
                *count, RETAIL_DUPLICATE_NAMES,
                "the first CrimFlag.MPG record makes the row; the other four repeats are \
                 counted"
            ),
            _ => assert_eq!(
                *count, 0,
                "gap code {code} is nonzero on the owner's installation: re-measure it in \
                 docs/findings/2026-10-12-f14-d-video-layout-collection.md"
            ),
        }
    }
    let mut named = Vec::new();
    for element in video_rows.iter().copied() {
        assert!(
            element.origin.is_original(),
            "{} must be installation data",
            element.id
        );
        let span = element
            .origin
            .source()
            .expect("a row is located by a checked span");
        assert_eq!(span.install_sha256().to_hex(), install_sha256);
        assert!(span.length() > 0);
        assert_eq!(span.offset(), 0, "the span is the whole named file");
        let container = span.container_path().to_owned();
        assert_eq!(
            span.member_sha256().map(|digest| digest.to_hex()),
            Some(
                sha256(&fs::read(game_dir.join(&container)).expect("the named file reads"))
                    .to_hex()
            ),
            "{}: the span's digest is the named file's own bytes",
            element.id
        );
        named.push(container.clone());
        assert!(
            container.to_ascii_lowercase().ends_with(".mpg"),
            "{}: a video row is a media file",
            element.id
        );
        assert_eq!(element.readiness, cs_types::content::Readiness::Unavailable);
        assert!(element.runtime_consumers.is_empty());
        assert_eq!(element.parse_state, cs_types::install::ParseState::Unparsed);
        assert_eq!(
            element.dependencies.len(),
            1,
            "{} carries exactly one static edge onto the named file's inventory row",
            element.id
        );
        let edge = &element.dependencies[0];
        assert_eq!(
            edge.target,
            cs_types::content::ContentId::from_source(
                ContentKind::InstallFile,
                &install_file_key(&container)
            )
            .expect("the install file key is valid"),
            "{} points at the inventory row of the named file",
            element.id
        );
        assert!(baseline.catalog.get(&edge.target).is_some());

        // The claim names the record that stated the name, and the claim's
        // source span locates it: the two layout rows inside the decoded
        // member, the three `PLAYAVI` rows at the resolved file's own span.
        let provenance = edge
            .provenance
            .source
            .as_ref()
            .expect("every video edge is located by a span");
        match container.as_str() {
            "GOSDATA/ASSETS/GRAPHICS/MPG/crimflag.mpg"
            | "GOSDATA/ASSETS/GRAPHICS/MPG/final.mpg" => {
                assert_eq!(
                    edge.provenance.claim_id.as_str(),
                    "f14.d.video.baseline.layout",
                    "{container}: the movie record is the observation"
                );
                assert_eq!(provenance.container_path(), VIDEO_LAYOUT_CONTAINER);
                assert_eq!(provenance.member_key(), Some(VIDEO_LAYOUT_MEMBER));
                assert_eq!(
                    provenance.member_sha256(),
                    Some(member_sha256),
                    "{container}: the provenance span digests the decoded member"
                );
                assert!(
                    provenance.offset() >= info.offset
                        && provenance.offset() + provenance.length()
                            <= info.offset + info.declared_decoded_len,
                    "{container}: the record span is inside the member's decoded extent"
                );
                // And the span's own bytes, read back out of the decoded
                // member, are the record line that stated the name: an `M`
                // record whose file-name field spells it.
                let at = (provenance.offset() - info.offset) as usize;
                let line = &member[at..at + provenance.length() as usize];
                let line = std::str::from_utf8(line).expect("the record line is ASCII");
                let value = line
                    .split_once('=')
                    .expect("the record line is a key=value entry")
                    .1;
                let mut fields = value.split(',');
                assert_eq!(
                    fields.next(),
                    Some("M"),
                    "{container}: the span covers the member's movie record"
                );
                assert_eq!(
                    fields.next(),
                    element.display_name.as_deref(),
                    "{container}: the record's file-name field is the name the row carries"
                );
            }
            _ => assert_eq!(
                edge.provenance.claim_id.as_str(),
                "f14.d.video.baseline.playavi",
                "{container}: the PLAYAVI action is the observation"
            ),
        }
    }
    named.sort();
    assert_eq!(
        named,
        vec![
            "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg".to_owned(),
            "GOSDATA/ASSETS/GRAPHICS/MPG/crimflag.mpg".to_owned(),
            "GOSDATA/ASSETS/GRAPHICS/MPG/final.mpg".to_owned(),
            "GOSDATA/ASSETS/GRAPHICS/MPG/msopen1.mpg".to_owned(),
            "GOSDATA/ASSETS/GRAPHICS/MPG/zipper.mpg".to_owned(),
        ],
        "the five named files, as the installation spells them"
    );

    // Every edge of the collection resolves.
    assert_eq!(
        baseline.coverage.unresolved_references, 0,
        "a collection that pointed at ids it did not insert would report orphans"
    );

    // This collection added no launchable row: the denominator is still the
    // campaign missions plus the scenario directories another stage declared.
    // That is what makes the coverage claim honest, so it is measured here
    // rather than asserted from memory.
    assert!(
        !ContentKind::Video.is_launchable(),
        "a video row is not launchable content; if this ever changes the denominator moved and \
         this report must say so"
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
         scenario directories F14-D.1 classified; this collection declares no root"
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
            .all(|id| id.kind() != ContentKind::Video),
        "no video row is a closure root"
    );
    assert_eq!(
        baseline.catalog.launchable_count(),
        baseline.roots.len(),
        "every launchable row is a declared root, and this collection declares none"
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
        &format!("\"video\":{RETAIL_VIDEO_ROWS}"),
        &format!("\"unnamed_video_file\":{RETAIL_UNNAMED_MPG_FILES}"),
        &format!("\"duplicate_video_name\":{RETAIL_DUPLICATE_NAMES}"),
        "\"unreadable_video_source\":0",
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
        "the retail consumer report holds no authored row"
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
         \x20\"task_id\": \"F14-D.VIDEO-LAYOUT\",\n\
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
        // product-coverage limits live in `review.method`, the hashed
        // artifact, `docs/findings/` and the follow-up record instead of
        // being deleted.
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
        "\"task_id\": \"F14-D.VIDEO-LAYOUT\"",
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

/// The rows of one kind, in canonical id order.
fn rows_of(
    baseline: &cs_content::catalog::baseline::Baseline,
    kind: ContentKind,
) -> Vec<&cs_types::content::CatalogElement> {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == kind)
        .collect()
}

/// The `review.identity` this harness records.
///
/// The literal below is the text this repository's committed
/// `docs/findings/evidence/F14-D.VIDEO-LAYOUT.json` carries, and it is the
/// first string in this function on purpose:
/// `tools/tests/test_evidence_review_identity.py` reads a harness's recorded
/// identity out of exactly this shape (`fn review_identity() -> String`, a
/// literal) and cross-checks it against the committed report, so the text
/// here and the report cannot drift apart silently.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW`
/// and replaces the literal with it in the same commit, which is how the
/// report then names the review that actually happened.
fn review_identity() -> String {
    let recorded = String::from(
        "implementer: Devin SWE-2/swe2-max-1 (Rally #1351, implement claim of \
         2026-10-12T17:23:02Z), model SWE-2 Max. reviewer: Devin SWE-2/swe2-max-1 (Rally \
         #1351, review claim of 2026-10-12T19:34Z), model SWE-2 Max - the same agent name \
         and model as the implementer, in a fresh context, so this review is not independent \
         evidence and the format and identity claims still deserve a different agent \
         instance or model. No agent review awards more than checked, and nothing here is \
         verified_original: retail is read access to the installation's own files, the \
         original game was never run, and the video collection is structural. No agent \
         review replaces the owner's human approval",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records: what was run, and the
/// product-completeness limits this report does not claim away.
fn review_method() -> String {
    let recorded = String::from(
        "acceptance suite run locally with the retail capability; this harness derives every \
         field from the recorded log, production discovery of $CS_GAME_DIR, the production \
         baseline report over that installation, the production ROF mount of \
         GOSDATA/ASSETS/crimson.rof, rustc and Cargo.lock; validated with \
         tools/validate_evidence.py --require-pass. The consumer trace is \
         cs_content::catalog::baseline::retail_baseline + baseline_report_json, the same \
         functions `cs-inspect catalog --cs-path` writes. Each row's identity is the resolved \
         .mpg file's own install_file_key and its span is the file's whole extent with the \
         file's own digest; the names come from two sources, the PLAYAVI actions of the \
         fmv.zrd member of ZBD/zrdr.zbd (production container discovery + decode_zrd) and the \
         M (movie) records of ASSETS/LAYOUT.CSV in GOSDATA/ASSETS/crimson.rof, which this \
         stage added through the production ROF mount and the keyed-list configuration \
         reader, classified by the measured RecordSchema::Layout(RecordKind::Movie) schema \
         and counted as video references only when the file-name field states an .mpg name. \
         The layout rows' edge provenance spans are the records' own lines inside the \
         decoded member, located by the member's stored offset and decoded digest; \
         CollectionStatus::source names both containers, and a source that cannot be read \
         is counted under unreadable_video_source and named in the diagnostic rather than \
         failing the collection.",
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
    "Five inventoried .mpg files (chap1.mpg through chap5.mpg) no record of either video \
     source reaches, so they are counted under the collection's `unnamed_video_file` gap and \
     have no row. They are not unnamed everywhere: ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT of \
     GOSDATA/ASSETS/crimson.rof constructs the chapter name as \"chap\" conv$(FC) \".mpg\" \
     rather than spelling it, and this stage reads no script bindings, so no row is minted \
     from them. Affected content: chap1.mpg through chap5.mpg. Resolving task: Rally #1352 \
     (F14-D.VIDEO-CHAPTER, the chapter-name binding), which gates any completeness or \
     fidelity claim for the video collection.",
    "No row carries a codec, duration, resolution or playback trigger: no original byte read \
     here states one, and reading the MPEG stream would produce numbers with no unit contract \
     and no consumer. Affected content: every video row. Resolving task: F40 (cutscenes) and \
     the media player consumer.",
    "No row claims a runtime consumer, so every video row is unreachable from the declared \
     roots and stays counted in coverage.unreachable_by_kind. Affected content: the \
     reachability accounting of the video collection. Resolving task: the cutscene/script \
     playback stage.",
    "A name is only identity when one inventoried file matches it; the records give a file \
     name, never a path, so a name that matched several files would be counted ambiguous and \
     make no row. Affected content: any future same-named .mpg the installation adds. \
     Resolving task: none needed while the inventory holds one file per name; the rule \
     itself is the contract.",
];

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f14_d_video_layout.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F14-D.VIDEO-LAYOUT` written
/// relative to the workspace root in the module doc must be re-anchored here.
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
/// `accept_f14_d_video_layout_` tests from a recorded `cargo test` output.
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
            // `cargo test -- <prefix>` matches the prefix anywhere in the
            // test name, so a unit test inside a module counts exactly as
            // the selection counts it.
            if !name.contains(TASK_PREFIX) {
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

/// One referenced artifact: hashed here with the production SHA-256 the
/// sibling crate implements (the validator re-hashes it with `hashlib`
/// independently).
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
