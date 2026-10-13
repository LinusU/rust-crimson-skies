//! Evidence-report harness for task F40-D2
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f40_d2_*`: it is not part
//! of the acceptance suite, and it fails loudly when its inputs are missing
//! instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F40-D2
//!    cargo test --workspace --locked -- accept_f40_d2_ --include-ignored \
//!      2>&1 | tee private/evidence/F40-D2/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F40-D2 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f40_d2_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test evidence_report_f40_d2 -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F40-D2/acceptance.json \
//!      --artifact-root private/evidence/F40-D2 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F40-D2.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded
//! test log, the environment, production discovery of `$CS_GAME_DIR`, and a
//! second production re-measurement of the cinematic trigger inventory
//! through the production container discovery and the production ROF tree
//! reader — the same readers the retail acceptance test drives — written
//! into the hashed `cinematic-inventory.json` artifact with each source
//! member's decoded SHA-256 and every measured fragment's offset. The only
//! texts this file holds are the [`review_identity`] literal the committed
//! `docs/findings/evidence/F40-D2.json` carries and the product-coverage
//! limitations quoted into `review.method`; everything else is measured.
//!
//! `unknowns` is `[]` and the report validates with `--require-pass`: the
//! **stage's** acceptance is complete — the ten videos' triggers,
//! skippability, semantic ends and explicit unknowns are measured member by
//! member, and every selected test passed. What remains genuinely unresolved
//! — the invocation sites of the engine FMV sequences, the value domain of
//! native callbacks 2151 and 3104, the codecs, and the engine player's
//! pause/clock behavior — is named in `UNKNOWN_LIMITATIONS` below, in
//! `review.method`, in the hashed artifact and in
//! `docs/findings/2026-10-13-f40-d2-cinematic-trigger-inventory.md`, each
//! with the evidence that bounds it. A failing run produces a failing report,
//! which the validator rejects.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::cinematics::{
    RETAIL_FMV_ARCHIVE, RETAIL_FMV_MEMBER, RetailCinematicSources, RetailTrigger, UiPageSource,
    measure_retail_cinematics,
};

/// The task's own prefix, which the log parser selects on.
const TASK_PREFIX: &str = "accept_f40_d2_";

/// The retail half of the prefix; its presence in the log is what licenses
/// the `retail` capability below.
const RETAIL_PREFIX: &str = "accept_f40_d2_retail_";

/// The ten videos the measurement must name on the owner's installation.
const RETAIL_VIDEOS: [&str; 10] = [
    "msopen1.mpg",
    "zipper.mpg",
    "chap0.mpg",
    "crimflag.mpg",
    "final.mpg",
    "chap1.mpg",
    "chap2.mpg",
    "chap3.mpg",
    "chap4.mpg",
    "chap5.mpg",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f40_d2_writes_the_acceptance_report() {
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
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; old \
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
                "the retail acceptance test did not run: F40-D2 requires capability `retail`, \
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
            .any(|(name, _)| name.contains(TASK_PREFIX) && !name.contains(RETAIL_PREFIX)),
        "synthetic task tests must be present alongside the retail one"
    );

    // `source` hashes describe the real installation, measured by the
    // production discovery code.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The consumer trace: the trigger inventory re-measured through the
    // production readers — the same ones the retail acceptance test drives —
    // and written into the evidence directory with each source member's
    // decoded SHA-256 and every measured fragment's offset.
    let fmv = retail_fmv_member(&game_dir);
    let layout = retail_rof_member(&game_dir, "ASSETS/LAYOUT.CSV");
    let cabin = retail_rof_member(&game_dir, "ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT");
    let campaignintro = retail_rof_member(&game_dir, "ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT");
    let finalcinema = retail_rof_member(&game_dir, "ASSETS/SCRIPTS/FINALCINEMA.SCRIPT");
    let control = retail_rof_member(&game_dir, "ASSETS/SCRIPTS/CTL.SCRIPT");
    let mainmenu = retail_rof_member(&game_dir, "ASSETS/SCRIPTS/MAINMENU.SCRIPT");
    let save = retail_rof_member(&game_dir, "ASSETS/SCRIPTS/SAVE.SCRIPT");
    let load = retail_rof_member(&game_dir, "ASSETS/SCRIPTS/LOAD.SCRIPT");
    let preferences = retail_rof_member(&game_dir, "ASSETS/SCRIPTS/PREFERENCES.SCRIPT");
    let mpg = retail_mpg_names(&game_dir);
    let mpg_refs: Vec<&str> = mpg.iter().map(String::as_str).collect();
    let pages = [
        UiPageSource {
            page: "mainmenu",
            script: &mainmenu,
        },
        UiPageSource {
            page: "save",
            script: &save,
        },
        UiPageSource {
            page: "load",
            script: &load,
        },
        UiPageSource {
            page: "preferences",
            script: &preferences,
        },
        UiPageSource {
            page: "campaignintro",
            script: &campaignintro,
        },
        UiPageSource {
            page: "finalcinema",
            script: &finalcinema,
        },
    ];
    let sources = RetailCinematicSources {
        fmv_member: &fmv,
        layout_member: &layout,
        ui_pages: &pages,
        passenger_cabin: &cabin,
        control: &control,
        mpg_files: &mpg_refs,
    };
    let inventory =
        measure_retail_cinematics(&sources).expect("the retail installation measures for evidence");
    let names: Vec<&str> = inventory
        .cinematics()
        .iter()
        .map(|cinematic| cinematic.video())
        .collect();
    for expected in RETAIL_VIDEOS {
        assert!(
            names
                .iter()
                .any(|found| found.eq_ignore_ascii_case(expected)),
            "{expected} is inventoried by the re-measurement; measured names: {names:?}"
        );
    }
    assert_eq!(
        inventory.cinematics().len(),
        RETAIL_VIDEOS.len(),
        "exactly the ten retail videos: {names:?}"
    );

    // The hashed artifact: the whole measured inventory plus each source
    // member's digest, so a reviewer can re-derive every claim from the
    // artifact alone (the members themselves stay in the installation).
    let member_bytes: Vec<(&str, &str, &[u8])> = vec![
        (RETAIL_FMV_ARCHIVE, RETAIL_FMV_MEMBER, fmv.as_slice()),
        (
            "GOSDATA/ASSETS/crimson.rof",
            "ASSETS/LAYOUT.CSV",
            layout.as_slice(),
        ),
        (
            "GOSDATA/ASSETS/crimson.rof",
            "ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT",
            cabin.as_slice(),
        ),
        (
            "GOSDATA/ASSETS/crimson.rof",
            "ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT",
            campaignintro.as_slice(),
        ),
        (
            "GOSDATA/ASSETS/crimson.rof",
            "ASSETS/SCRIPTS/FINALCINEMA.SCRIPT",
            finalcinema.as_slice(),
        ),
        (
            "GOSDATA/ASSETS/crimson.rof",
            "ASSETS/SCRIPTS/CTL.SCRIPT",
            control.as_slice(),
        ),
    ];
    let mut members_json = Vec::new();
    for (container, member, bytes) in member_bytes {
        members_json.push(format!(
            "{{\"container\": {}, \"member\": {}, \"decoded_len\": {}, \"decoded_sha256\": {}}}",
            jstr(container),
            jstr(member),
            bytes.len(),
            jstr(&sha256(bytes).to_hex())
        ));
    }
    let mut rows_json = Vec::new();
    for cinematic in inventory.cinematics() {
        let triggers: Vec<String> = cinematic
            .triggers()
            .iter()
            .map(|trigger| match trigger {
                RetailTrigger::FmvSequence {
                    sequence,
                    archive,
                    member,
                } => format!(
                    "{{\"kind\": \"fmv_sequence\", \"sequence\": {}, \"archive\": {}, \
                     \"member\": {}}}",
                    jstr(sequence),
                    jstr(archive),
                    jstr(member)
                ),
                RetailTrigger::UiMoviePage {
                    page,
                    layout_key,
                    loops,
                    freturntogame_guarded,
                    file_superseded_by_constructed_name,
                } => format!(
                    "{{\"kind\": \"ui_movie_page\", \"page\": {}, \"layout_key\": {}, \
                     \"loops\": {}, \"freturntogame_guarded\": {freturntogame_guarded}, \
                     \"file_superseded_by_constructed_name\": \
                     {file_superseded_by_constructed_name}}}",
                    jstr(page),
                    jstr(layout_key),
                    match loops {
                        Some(loops) => loops.to_string(),
                        None => "null".to_owned(),
                    }
                ),
                RetailTrigger::ChapterConstruction {
                    page,
                    control_member,
                    callback_id,
                } => format!(
                    "{{\"kind\": \"chapter_construction\", \"page\": {}, \"control_member\": {}, \
                     \"callback_id\": {callback_id}}}",
                    jstr(page),
                    jstr(control_member)
                ),
            })
            .collect();
        let (skippable_claim, skippable_value) = resolved_bool(cinematic.skippable());
        let (pause_claim, pause_value) = resolved_known_claim(cinematic.pause());
        let (clock_claim, clock_value) = resolved_known_claim(cinematic.clock());
        let (format_claim, format_value) = resolved_string(cinematic.format());
        let semantic_end = match cinematic.semantic_end() {
            cs_content::cinematics::RetailSemanticEnd::UiScriptResume { page } => {
                format!(
                    "{{\"kind\": \"ui_script_resume\", \"page\": {}}}",
                    jstr(page)
                )
            }
            cs_content::cinematics::RetailSemanticEnd::UiScriptRun { script } => {
                format!(
                    "{{\"kind\": \"ui_script_run\", \"script\": {}}}",
                    jstr(script)
                )
            }
            cs_content::cinematics::RetailSemanticEnd::LoopForever => {
                "{\"kind\": \"loop_forever\"}".to_owned()
            }
            cs_content::cinematics::RetailSemanticEnd::EngineUnread => {
                "{\"kind\": \"engine_unread\"}".to_owned()
            }
        };
        rows_json.push(format!(
            "{{\"video\": {}, \"triggers\": [{}], \"skippable\": {{\"known\": {}, \"claim\": \
             {skippable_claim}}}, \"pause\": {{\"known\": {pause_value}, \"claim\": \
             {pause_claim}}}, \"clock\": {{\"known\": {clock_value}, \"claim\": {clock_claim}}}, \
             \"format\": {{\"value\": {}, \"claim\": {format_claim}}}, \"semantic_end\": \
             {semantic_end}, \"declared_actions\": 0}}",
            jstr(cinematic.video()),
            triggers.join(", "),
            match skippable_value {
                Some(true) => "true".to_owned(),
                Some(false) => "false".to_owned(),
                None => "null".to_owned(),
            },
            jstr(&format_value.unwrap_or_default()),
        ));
    }
    let mut fragments_json = Vec::new();
    for fragment in inventory.fragments() {
        fragments_json.push(format!(
            "{{\"source\": {}, \"fragment\": {}, \"occurrences\": {}, \"first_offset\": {}}}",
            jstr(&fragment.source),
            jstr(&fragment.fragment),
            fragment.occurrences,
            match fragment.first_offset {
                Some(offset) => offset.to_string(),
                None => "null".to_owned(),
            }
        ));
    }
    let inventory_path = evidence_dir.join("cinematic-inventory.json");
    fs::write(
        &inventory_path,
        format!(
            "{{\"schema\": \"cs-content-cinematic-inventory/1\", \"install_sha256\": {}, \
             \"members\": [{}], \"videos\": [{}], \"fragments\": [{}], \"playavi_not_text\": {}, \
             \"movie_record_without_mpg_name\": {}}}\n",
            jstr(&install_sha256),
            members_json.join(", "),
            rows_json.join(", "),
            fragments_json.join(", "),
            inventory.playavi_not_text(),
            inventory.movie_record_without_mpg_name(),
        ),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", inventory_path.display()));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&inventory_path, "json", &evidence_dir),
    ];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F40-D2\",\n\
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
        // module doc — the stage's own acceptance is complete, and the
        // product-coverage limits live in `review.method`, the hashed
        // artifact and `docs/findings/` instead of being deleted.
        "",
        jstr(&review_identity()),
        jstr(&review_method()),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    // A cheap self-check without a JSON dependency: the validator runs
    // next, but a structurally empty write must fail here first.
    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F40-D2\"",
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

/// The claim id and value of a `Resolved<bool>`, as the artifact records it.
fn resolved_bool(resolved: &cs_types::content::Resolved<bool>) -> (String, Option<bool>) {
    match resolved {
        cs_types::content::Resolved::Known(known) => {
            (jstr(known.provenance.claim_id.as_str()), Some(known.value))
        }
        cs_types::content::Resolved::Unknown { claim_id, .. } => (jstr(claim_id.as_str()), None),
    }
}

/// The claim id and "is it known" of a `Resolved<T>` whose value the artifact
/// does not carry (pause policy, master clock — unknown everywhere).
fn resolved_known_claim<T>(resolved: &cs_types::content::Resolved<T>) -> (String, bool) {
    match resolved {
        cs_types::content::Resolved::Known(known) => {
            (jstr(known.provenance.claim_id.as_str()), true)
        }
        cs_types::content::Resolved::Unknown { claim_id, .. } => (jstr(claim_id.as_str()), false),
    }
}

/// The claim id and value of the `Resolved<String>` format label.
fn resolved_string(resolved: &cs_types::content::Resolved<String>) -> (String, Option<String>) {
    match resolved {
        cs_types::content::Resolved::Known(known) => (
            jstr(known.provenance.claim_id.as_str()),
            Some(known.value.clone()),
        ),
        cs_types::content::Resolved::Unknown { claim_id, .. } => (jstr(claim_id.as_str()), None),
    }
}

/// Reads one member of the retail `crimson.rof` through the production tree
/// walk and member decode.
fn retail_rof_member(root: &Path, member: &str) -> Vec<u8> {
    let bytes = fs::read(root.join("GOSDATA/ASSETS/crimson.rof")).expect("crimson.rof reads");
    let mut context = cs_formats::ParseContext::with_defaults("crimson.rof");
    let tree = cs_formats::read_tree(&mut context, &bytes).expect("the container tree walks");
    let wanted: Vec<&str> = member.split('/').collect();
    let entry = tree
        .members()
        .iter()
        .find(|entry| {
            entry.path.len() == wanted.len()
                && entry
                    .path
                    .iter()
                    .zip(&wanted)
                    .all(|(segment, name)| segment.eq_ignore_ascii_case(name.as_bytes()))
        })
        .unwrap_or_else(|| panic!("{member} is a member of crimson.rof"));
    cs_formats::read_member(&context, &bytes, entry, &cs_formats::RofLimits::default())
        .unwrap_or_else(|error| panic!("{member} must read: {error}"))
        .data
}

/// The retail `fmv.zrd` member through the production container discovery.
fn retail_fmv_member(root: &Path) -> Vec<u8> {
    let bytes = fs::read(root.join("ZBD/zrdr.zbd")).expect("zrdr.zbd reads");
    let path =
        cs_types::install::RelativePath::new("ZBD/zrdr.zbd").expect("the spelling is relative");
    let discovery = cs_formats::script_raw::discover_container("ZBD/zrdr.zbd", &path, &bytes);
    let fmv = discovery
        .programs()
        .iter()
        .find(|program| program.locator().member() == Some("fmv.zrd"))
        .expect("zrdr.zbd holds fmv.zrd");
    fmv.bytes().to_vec()
}

/// The retail `.mpg` file names under the engine's own MPG directory.
fn retail_mpg_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join("GOSDATA/ASSETS/GRAPHICS/MPG"))
        .expect("the MPG directory lists")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.to_ascii_lowercase().ends_with(".mpg"))
        .collect();
    names.sort();
    names
}

/// The `review.identity` this harness records.
///
/// The literal below is the text this repository's committed
/// `docs/findings/evidence/F40-D2.json` carries, and it is the first string
/// in this function on purpose:
/// `tools/tests/test_evidence_review_identity.py` reads a harness's
/// recorded identity out of exactly this shape (`fn review_identity() ->
/// String`, a literal) and cross-checks it against the committed report, so
/// the text here and the report cannot drift apart silently.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW`
/// and replaces the literal with it in the same commit, which is how the
/// report then names the review that actually happened.
fn review_identity() -> String {
    let recorded = String::from(
        "implementer: mimo-1/mimo-1 (Rally #661, implement claim of \
         2026-10-13T16:54:50Z), model MiMo. This report is the implementer's own record: \
         every check it reports was run by the implementer, it names no reviewer because \
         none has reviewed it, and the reviewing agent records their own identity in its \
         place (through CS_EVIDENCE_REVIEW) when they regenerate it; the format, trigger \
         and identity claims still deserve a different agent instance or model with a \
         fresh context. No agent review awards more than checked, and nothing here is \
         verified_original: retail is read access to the installation's own files, the \
         original game was never run, and the trigger inventory is structural. No agent \
         review replaces the owner's human approval",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records: what was run, and the
/// product-completeness limits this report does not claim away.
fn review_method() -> String {
    let recorded = String::from(
        "acceptance suite run locally with the retail capability; this harness derives \
         every field from the recorded log, production discovery of $CS_GAME_DIR and a \
         second production re-measurement of the cinematic trigger inventory through the \
         production container discovery (ZBD/zrdr.zbd member fmv.zrd) and the production \
         ROF tree reader (GOSDATA/ASSETS/crimson.rof members ASSETS/LAYOUT.CSV and the \
         CAMPAIGNINTRO/FINALCINEMA/PASSENGERCABIN/CTL script members), rustc and \
         Cargo.lock; validated with tools/validate_evidence.py --require-pass. The \
         consumer trace is cs_content::cinematics::measure_retail_cinematics, and the \
         hashed cinematic-inventory.json artifact records each source member's decoded \
         sha256, every video's measured triggers and Resolved fields with their claim \
         ids, and every fragment's occurrence count and first offset, so a reviewer can \
         re-derive every claim from the artifact alone. The inventory names exactly the \
         ten retail videos: MSopen1 and zipper from the INTRO FMV sequence, Chap0 from \
         the CHAP0 sequence, CrimFlag from five layout movie records (one superseded by \
         the constructed chapter name), Final from the final-cinema record, and \
         chap1 through chap5 from the measured chapter construction. Skippability is \
         Known(true) only where a page's own skip handlers and done handler measured it \
         (the chapter intro and the final cinema); pause policy and master clock are \
         explicit unknowns everywhere, the .mpg label is the original's own extension, \
         and no declared-vocabulary semantic action is stated by any named source.",
    );
    recorded
        + &UNKNOWN_LIMITATIONS
            .iter()
            .map(|limitation| format!(" LIMITATION: {limitation}"))
            .collect::<String>()
}

/// The limitations this stage records instead of guessing, each naming the
/// affected content and the evidence that bounds it (AGENTS owner
/// directive, 2026-09-28: a limitation must survive into machine-readable
/// evidence).
const UNKNOWN_LIMITATIONS: &[&str] = &[
    "Which engine event invokes the INTRO and CHAP0 FMV sequences — and so when \
     msopen1.mpg, zipper.mpg and chap0.mpg play — was not read: the decrypted engine \
     image registers both sequence names against fmv.zrd at startup, but the invocation \
     sites were not followed, and no named script source handles these sequences. \
     Affected content: msopen1.mpg, zipper.mpg, chap0.mpg, whose skippability and \
     semantic end are explicit unknowns for the same reason. Resolving evidence: an \
     executable-analysis stage over the engine image, or an original-run capture.",
    "The value domains of native callbacks 2151 (the chapter movie's FC / nmissid) and \
     3104 (the final-cinema gate) are engine state no installation byte states, so which \
     chapter number each chap<N>.mpg belongs to, and when the final cinema is gated off, \
     stay unread — exactly the F14-D chapter-binding unknown, now with the 3104 gate \
     added. Affected content: chap1.mpg through chap5.mpg and final.mpg. Resolving \
     evidence: an original-run capture of the callbacks' returns (human/original-run \
     only) or a verified executable-analysis stage.",
    "The simulation-pause policy and master clock of every retail scene are explicit \
     unknowns (claims f40.d2.retail.pause.unmeasured and f40.d2.retail.clock.unmeasured): \
     the named sources run in the UI shell and no byte read here states the engine \
     player's behavior; the framerate settings the movie control sends around \
     play/loop/stop are interface frame-rate measures, not a pause statement. Affected \
     content: all ten videos. Resolving task: F40-B/F40-C (the media player consumer) or \
     an original-run capture.",
    "No codec, duration or frame size is claimed for any video: the streams are not \
     decoded, and the measured label is the original's own .mpg extension (claim \
     f40.d2.retail.format.mpg), not a codec identity. Affected content: all ten videos. \
     Resolving task: F40-B's decoder stage.",
    "Retail is read access to the installation's own files, not an original run: the \
     inventory verifies that the named sources state each trigger, never that the \
     original plays a given file for a given player action. Affected content: every \
     fidelity claim about the ten videos. Resolving evidence: an original-run capture.",
];

// ------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module \
             doc (crates/cs_content/tests/evidence_report_f40_d2.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F40-D2` written relative to the
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

/// The locked version of one `Cargo.lock` package: read, never asserted
/// from memory.
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
    /// `(test name, "pass" | "fail" | "unknown")`, in log order,
    /// deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f40_d2_` tests from a recorded `cargo test` output.
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
        // `test <name> ... <status>`, possibly several per interleaved
        // line.
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
