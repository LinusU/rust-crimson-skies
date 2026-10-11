//! Evidence-report harness for task #542 (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_t542_*`: it is not part of
//! the acceptance suite, it fails loudly when its inputs are missing instead of
//! passing vacuously, and the task's test selection must never pick it up as an
//! acceptance test. Run from the workspace root, after the acceptance suite,
//! exactly as:
//!
//! 1. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T542 \
//!      CS_ENGINE_IMAGE="$CS_ENGINE_IMAGE" \
//!      CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --workspace --locked -- accept_t542_ --include-ignored \
//!      2>&1 | tee private/evidence/T542/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.) `CS_EVIDENCE_DIR` must be set **in this step**,
//!    while the acceptance suite runs: the retail eye test derives its
//!    `eye-placement.json` and the image test its `eye-mechanism.json` into
//!    that directory only while the variable is set — so without it step 2
//!    finds the directory holding no artifacts and panics on the missing files.
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T542 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t542_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!    CS_ENGINE_IMAGE="$CS_ENGINE_IMAGE" \
//!      cargo test --locked -p cs_app --test evidence_report_t542 -- --ignored
//!    ```
//!    (`CS_EVIDENCE_DIR` tells this harness where step 1's run left the
//!    artifacts it must read, hash and re-check, so the census this report
//!    hashes is this run's own; `CS_EVIDENCE_ARGV` stays the acceptance
//!    command's bare argv, because an environment prefix is not part of a
//!    command's argv.)
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/T542/acceptance.json \
//!      --artifact-root private/evidence/T542 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T542.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the eye census the
//! retail acceptance test derived over that installation, the mechanism
//! record the image test derived over `$CS_ENGINE_IMAGE`, and
//! `rustc --version` and `Cargo.lock`. Nothing is typed in by hand.
//!
//! The measured chain is re-run **in this harness** as a second production
//! observation — `decode_interp` over the real `ZBD/interp.zbd`,
//! `discover_cockpit_bindings` over the real claim, `discover_airframe_roster`
//! over the same container, `read_gamez_nodes` over the real
//! `ZBD/planes.zbd`, `audit_cockpit_coverage` and `resolve_cockpit_eye` — all
//! eleven airframes `Declared` with a stored-translation triple — and
//! `load_engine_image` + `eye_mechanism_source` + the byte pins over the
//! owner-supplied decrypted executable (the stored `cockpit_camera` name, the
//! registration's push of that name's address and its eight-call target list,
//! the translate reader's field loads, the dispatch table's two eye entries
//! and the override branches). A stale or hand-written record cannot fake
//! those numbers, because this run recomputes them.
//!
//! The `capabilities` are checked, not assumed: `retail` is carried by the
//! tests that read `$CS_GAME_DIR` and `$CS_ENGINE_IMAGE` and `synthetic` by
//! the seven unignored authored-corpus tests; every one must appear in the
//! recorded log and pass.
//!
//! The report's `unknowns` are this task's own **blockers**, and they are
//! empty because the acceptance run passed. What the pass deliberately does
//! not answer — which world axis each stored component is, what the two
//! dispatched camera modes are called, and what `cocky`/`cockz` scripts write
//! at run time — stays written out in
//! `docs/findings/2026-10-11-t542-cockpit-camera-eye-placement.md`.
//! The claim is `implemented`, never `checked` or `verified_original`: byte
//! inspection of the owner-supplied image is static analysis, not a run of
//! the original.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::camera::coverage::{
    COCKPIT_EYE_NODE, CockpitAirframe, CockpitEyeState, audit_cockpit_coverage,
    discover_cockpit_bindings, resolve_cockpit_eye,
};
use cs_app::camera::{CockpitEyeError, eye_mechanism_source};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::coordinates::{EngineImage, load_engine_image, original_image_digest};
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::interp::decode_interp;
use cs_formats::io::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

/// The acceptance tests whose `retail` capability this report declares: they
/// read `$CS_GAME_DIR` and `$CS_ENGINE_IMAGE`.
const REQUIRED_TESTS: &[&str] = &[
    "accept_t542_retail_every_airframe_declares_its_eye_as_one_stored_translation",
    "accept_t542_image_the_mechanism_spans_cover_the_measured_instructions",
];

/// The synthetic half of the suite, which must be present beside the
/// capability-bound one: an evidence report that only ever ran
/// capability-bound tests would not show that the pass's contract is pinned
/// without them.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_t542_the_eye_is_the_cockpit_camera_nodes_stored_local_translation",
    "accept_t542_the_eye_lookup_is_byte_exact_because_the_measured_search_is",
    "accept_t542_two_eye_nodes_in_one_airframe_are_ambiguous_not_resolved_by_position",
    "accept_t542_an_eye_node_that_is_not_an_object3d_reports_its_kind",
    "accept_t542_the_eye_search_never_leaves_the_airframe_it_started_from",
    "accept_t542_a_declared_eye_binds_the_model_node_the_registration_resolves",
    "accept_t542_an_eye_pass_over_a_different_archive_is_refused",
];

/// The measured eye census the retail acceptance test wrote.
const EYE_ARTIFACT: &str = "eye-placement.json";
/// The measured mechanism record the image acceptance test wrote.
const MECHANISM_ARTIFACT: &str = "eye-mechanism.json";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_t542_writes_the_acceptance_report() {
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

    // The candidate tree must be the tree that was actually tested, and the
    // artifacts must have been derived from it.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; old \
         reports cannot be reused for new code"
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
        "no `accept_t542_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed.
    for required in REQUIRED_TESTS.iter().chain(SYNTHETIC_TESTS) {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == required)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{required} did not run: T542 declares `retail`, so step 1 must run with \
                     `--include-ignored` and CS_GAME_DIR/CS_ENGINE_IMAGE set"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }
    assert_eq!(
        suite.assertions.len(),
        REQUIRED_TESTS.len() + SYNTHETIC_TESTS.len(),
        "and every `accept_t542_` test in the selection is one this report names: {:?}",
        suite
            .assertions
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
    );

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's census is built on.
    let found =
        discover(&game_dir).expect("production discovery must read the original installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The eye census the retail acceptance test wrote over that installation,
    // and the mechanism record the image test wrote over the owner-supplied
    // executable. Both are checked against the tree and the hashes rather
    // than trusted, because files left over from an earlier commit would
    // otherwise be reported as this run's.
    let eye_path = evidence_dir.join(EYE_ARTIFACT);
    let eye_json = fs::read_to_string(&eye_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; step 1 must set CS_EVIDENCE_DIR so the retail eye test \
             writes its derived census",
            eye_path.display()
        )
    });
    let planes_sha256 = sha256(
        &fs::read(game_dir.join("ZBD").join("planes.zbd"))
            .expect("the shared airframe archive must be there"),
    )
    .to_hex();
    for needle in [
        format!("\"candidate_tree\":\"{candidate_tree}\""),
        format!("\"install_sha256\":\"{install_sha256}\""),
        format!("\"content_sha256\":\"{content_sha256}\""),
        format!("\"archive_sha256\":\"{planes_sha256}\""),
        "\"declared\":11".to_owned(),
    ] {
        assert!(
            eye_json.contains(&needle),
            "the eye census does not describe this run ({needle:?}); a stale artifact cannot \
             be reported as measured evidence"
        );
    }
    // The eleven stored-name rows are counted, not just present once.
    let matched = eye_json
        .split("\"name\":\"")
        .skip(1)
        .filter(|part| part.starts_with("cockpit_camera\""))
        .count();
    assert_eq!(
        matched, 11,
        "the census names the measured eye node on all eleven rows"
    );

    let mechanism_path = evidence_dir.join(MECHANISM_ARTIFACT);
    let mechanism_json = fs::read_to_string(&mechanism_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; step 1 must set CS_EVIDENCE_DIR so the image test writes \
             its derived mechanism record",
            mechanism_path.display()
        )
    });
    let image_sha256 = original_image_digest().to_hex();
    for needle in [
        format!("\"candidate_tree\":\"{candidate_tree}\""),
        format!("\"image_sha256\":\"{image_sha256}\""),
        "\"eye_entries\":[6,7]".to_owned(),
    ] {
        assert!(
            mechanism_json.contains(&needle),
            "the mechanism record does not describe this run ({needle:?}); a stale artifact \
             cannot be reported as measured evidence"
        );
    }

    // The second production observation: the whole measured chain, re-run
    // inside this harness — so the numbers the report claims were recomputed
    // here rather than trusted from the tests' own output.
    let manifest_sha256 = |key: &str| {
        found
            .manifest
            .files
            .iter()
            .find(|row| row.relative_spelling.logical_key() == key)
            .map(|row| row.sha256)
            .unwrap_or_else(|| panic!("{key} is inventoried"))
    };
    remeasure_eye(&game_dir, &manifest_sha256, fingerprint(&found.manifest));
    remeasure_mechanism();

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&eye_path, "json", &evidence_dir),
        artifact(&mechanism_path, "json", &evidence_dir),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T542\",\n\
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
        jstr(&reviewer),
        jstr(REVIEW_METHOD),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &document).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"T542\"",
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

/// The review method the report records: what was run, against what, and what
/// this evidence does and does not claim.
const REVIEW_METHOD: &str = "the acceptance suite run locally with `CS_GAME_DIR` and \
    `CS_ENGINE_IMAGE` set, every `accept_t542_` test selected by prefix with \
    `--include-ignored`; this harness derives every field from the recorded log, production \
    discovery of $CS_GAME_DIR, the eye census the retail test derived over that \
    installation and the mechanism record the image test derived over the owner-supplied \
    decrypted executable, and a second full production re-run of the measured chain inside \
    the harness itself (`decode_interp`, `discover_cockpit_bindings`, \
    `discover_airframe_roster`, `read_gamez_nodes` over `ZBD/planes.zbd`, \
    `audit_cockpit_coverage`, `resolve_cockpit_eye`, `load_engine_image`, \
    `eye_mechanism_source` and the instruction byte pins), which re-derives all eleven \
    airframes' `Declared` eyes — each the `cockpit_camera` node's stored translation \
    triple — and the registration's eight-call target list rather than trusting the log; \
    both artifacts are checked against this report's own tree and hashes before they are \
    referenced, so a stale file cannot be reported as this run's. Validated with \
    tools/validate_evidence.py --require-pass. LIMITS OF WHAT WAS MEASURED: `retail` is \
    read access to original files and byte inspection of the owner-supplied decrypted \
    executable is static analysis — neither is a run of the original; which world axis \
    each stored eye component is, and the unit conversion into this project's \
    right/up/forward body frame, is unrecorded (the triple is carried in the archive's \
    source units); what the two dispatched camera modes are called in the original UI is \
    unrecorded; the `cocky`/`cockz` script override channel is measured but the values a \
    script writes are runtime state; whether the original's traversal-order tiebreak is \
    ever needed is unrecorded because no retail airframe stores two matching nodes. \
    `claim` is `implemented` only. `candidate_tree` is the tree of the commit the suite \
    ran on; the only later delta is this report's own copy under docs/findings/evidence/, \
    whose bytes are that file.";

/// The claim id the harness's own measured provenance is recorded under.
fn claim() -> ClaimId {
    ClaimId::new("t542.evidence-harness").expect("a valid claim id")
}

/// Provenance over a real byte range of a real file — the same spans the
/// acceptance tests' claims carry, so the harness's re-measurement describes
/// the same bytes.
fn retail_provenance(
    install_sha256: ContentHash,
    container: &str,
    offset: u64,
    length: u64,
) -> Provenance {
    let span = SourceSpan::new(install_sha256, container, None, offset, length, None)
        .expect("a span over real bytes is valid");
    Provenance::new(claim(), ClaimStatus::ObservedTool, Some(span))
        .expect("an observed_tool claim with a source span is valid")
}

/// Re-runs the archive half of the measured chain over `$CS_GAME_DIR`: decode
/// the real `ZBD/interp.zbd`, discover the real cockpit bindings and roster,
/// read the real `ZBD/planes.zbd` node array, audit and resolve — all eleven
/// airframes `Declared`, each with the `cockpit_camera` node's own stored
/// translation.
fn remeasure_eye(
    game_dir: &Path,
    manifest_sha256: &dyn Fn(&str) -> ContentHash,
    install_sha256: ContentHash,
) {
    let container_label = "ZBD/interp.zbd";
    let bytes = fs::read(game_dir.join("ZBD").join("interp.zbd"))
        .expect("the loading-script container must be there");
    assert_eq!(
        sha256(&bytes),
        manifest_sha256("zbd/interp.zbd"),
        "the bytes this re-measurement walks are the manifest's own"
    );
    let decoded = decode_interp(&mut ParseContext::with_defaults(container_label), &bytes)
        .expect("the real container decodes");
    let claim = cs_app::camera::coverage::CockpitBindingClaim::try_new(
        "support\\cockpit.gw",
        "FindNode",
        "FindSubNode",
        "%player_plane%",
        retail_provenance(install_sha256, container_label, 12_850, 5_713),
    )
    .expect("the real claim is well formed");
    let bindings = discover_cockpit_bindings(
        &decoded,
        &claim,
        container_label,
        retail_provenance(install_sha256, container_label, 12_850, 5_713),
    )
    .expect("the claimed script is the one the container holds");

    use cs_content::scene::{
        AirframeDeclaration, RosterDeclarations, RosterRoleRule, discover_airframe_roster,
    };
    let declarations = RosterDeclarations::new(vec![AirframeDeclaration {
        script: "support\\planes.gw".to_owned(),
        bind_command: "set".to_owned(),
        include_command: "source".to_owned(),
        write_command: "GameZWriteZBDFile".to_owned(),
        create_command: "NewObject3D".to_owned(),
        container_variable: "ZBDFile".to_owned(),
        root_variable: "planeOutput".to_owned(),
        model_variable: "planeInput".to_owned(),
        required_roles: vec![RosterRoleRule {
            role: cs_content::scene::PartRole::Cockpit,
            provenance: retail_provenance(install_sha256, container_label, 187_904, 392),
        }],
        provenance: retail_provenance(install_sha256, container_label, 37_838, 8_650),
    }])
    .expect("one declaration naming one script is valid");
    let roster = discover_airframe_roster(&decoded, &declarations)
        .expect("the real roster discovery reports findings, not a contradiction");
    assert_eq!(roster.airframe_count(), 11, "the measured airframe count");
    let airframes: Vec<CockpitAirframe> = roster
        .discovered()
        .iter()
        .map(CockpitAirframe::from)
        .collect();

    let archive_label = "ZBD/planes.zbd";
    let archive_id = ContentId::from_source(
        ContentKind::InstallFile,
        &cs_content::catalog::baseline::install_file_key(archive_label),
    )
    .expect("the normalized install-file key is a valid id key");
    let planes = fs::read(game_dir.join("ZBD").join("planes.zbd"))
        .expect("the shared airframe archive must be there");
    assert_eq!(
        sha256(&planes),
        manifest_sha256("zbd/planes.zbd"),
        "the node array this re-measurement reads is the manifest's own"
    );
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(archive_label), &planes)
        .expect("the airframe archive's node array reads");

    let coverage = audit_cockpit_coverage(&bindings, &airframes, &nodes, &archive_id)
        .expect("every declared root exists in the real archive");
    let report = resolve_cockpit_eye(&coverage, &nodes, &archive_id)
        .expect("the eye pass resolves over the archive the audit measured");
    assert_eq!(
        report.declared(),
        11,
        "every audited airframe declares its eye, recomputed in this harness"
    );
    for row in report.rows() {
        let CockpitEyeState::Declared {
            node_index,
            translation,
        } = row.state()
        else {
            panic!("{} does not declare its eye", row.airframe())
        };
        let node = nodes
            .get(*node_index)
            .expect("the resolved node is in the array");
        assert_eq!(
            node.name,
            COCKPIT_EYE_NODE,
            "{}: the eye node carries the stored name byte for byte",
            row.airframe()
        );
        assert!(
            translation.iter().all(|component| component.is_finite()),
            "{}: the stored eye is finite",
            row.airframe()
        );
    }
}

/// Re-runs the image half: load the owner-supplied decrypted executable and
/// re-derive the mechanism's spans, then re-check the measured bytes — the
/// stored `cockpit_camera` name, the registration's push of that name's own
/// address, its eight-call target list, the translate reader's field loads,
/// the dispatch table's two eye entries, the apply head's eye-global loads
/// and the `cocky`/`cockz` branch constants — inside this harness.
fn remeasure_mechanism() {
    use cs_app::camera::eye::{EYE_LOOKUP_OFFSET, EYE_READ_OFFSET, EYE_THUNK_OFFSET};

    let image: EngineImage =
        load_engine_image().expect("CS_ENGINE_IMAGE names the owner-supplied decrypted executable");
    assert_eq!(image.digest, original_image_digest());
    let mechanism = eye_mechanism_source(&image).expect("the measured image yields its spans");
    let bytes = |span: &SourceSpan| {
        &image.bytes[span.offset() as usize..(span.offset() + span.length()) as usize]
    };

    assert_eq!(bytes(&mechanism.name), b"cockpit_camera\0");
    assert_eq!(
        bytes(&mechanism.override_names),
        b"cocky\0\0\0cockz\0",
        "the two override names"
    );

    // The registration pushes the name span's own virtual address.
    let register = bytes(&mechanism.register);
    let pushed = u32::from_le_bytes(register[0x81..0x85].try_into().expect("four bytes"));
    assert_eq!(
        pushed,
        0x40_0000 + mechanism.name.offset() as u32,
        "the registration resolves the name the name span holds"
    );
    // Its own call list, decoded here: seven lookups and the one translate
    // read — the same call-decode the image test performs, recomputed.
    let calls_of = |span: &SourceSpan, body: &[u8]| -> Vec<u64> {
        let base = 0x40_0000_u64 + span.offset();
        let mut targets: Vec<u64> = (0..body.len().saturating_sub(5))
            .filter(|&index| body[index] == 0xe8)
            .map(|index| {
                let relative =
                    i32::from_le_bytes(body[index + 1..index + 5].try_into().expect("four bytes"));
                (base + index as u64 + 5).wrapping_add_signed(relative as i64)
            })
            .collect();
        targets.sort_unstable();
        targets
    };
    assert_eq!(
        calls_of(&mechanism.register, register),
        [
            vec![0x40_0000_u64 + EYE_READ_OFFSET],
            vec![0x40_0000_u64 + EYE_LOOKUP_OFFSET; 7]
        ]
        .concat(),
        "seven name searches and the one translate read"
    );

    // The translate reader's three field loads at +0x54/+0x58/+0x5c.
    let read = bytes(&mechanism.read);
    for (index, field) in [(0x6a_usize, 0x54_u8), (0x73, 0x58), (0x7c, 0x5c)] {
        assert_eq!(
            &read[index..index + 3],
            &[0x8b, 0x41, field],
            "the reader loads the +{field:#x} component"
        );
    }

    // The dispatch table's two eye entries reach the thunk, which calls the
    // eye transform.
    let thunk_va = 0x40_0000_u32 + EYE_THUNK_OFFSET as u32;
    let table: Vec<u32> = bytes(&mechanism.dispatch_table)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();
    let eye_entries: Vec<usize> = table
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| (*entry == thunk_va).then_some(index))
        .collect();
    assert_eq!(eye_entries, vec![6, 7], "camera modes 6 and 7 own the eye");
    let thunk = bytes(&mechanism.thunk);
    let relative = i32::from_le_bytes(thunk[6..10].try_into().expect("four bytes"));
    assert_eq!(
        (thunk_va as u64 + 5 + 5).wrapping_add_signed(relative as i64),
        0x40_0000_u64 + mechanism.apply.offset(),
        "the thunk tail-calls the eye transform"
    );

    // The transform head loads the three eye globals, then addresses the
    // aircraft matrix at [edi+0x180].
    let apply = bytes(&mechanism.apply);
    assert_eq!(
        &apply[0x9..0x1a],
        &[
            0xa1, 0x28, 0xc3, 0x71, 0x00, 0x8b, 0x0d, 0x2c, 0xc3, 0x71, 0x00, 0x8b, 0x15, 0x30,
            0xc3, 0x71, 0x00
        ],
        "the registered eye triple is the transform's input"
    );

    // The override branches write the second and third eye globals.
    let override_ = bytes(&mechanism.override_);
    for (index, global) in [(0x2a, 0x71c32c_u32), (0x6d, 0x71c330_u32)] {
        assert_eq!(
            &override_[index..index + 6],
            &[
                0xd9,
                0x1d,
                global.to_le_bytes()[0],
                global.to_le_bytes()[1],
                global.to_le_bytes()[2],
                global.to_le_bytes()[3]
            ],
            "writes the {global:#x} component"
        );
    }

    // And the refusal: an image that is not the measured one yields no spans.
    let wrong =
        ContentHash::from_hex("1111111111111111111111111111111111111111111111111111111111111111")
            .expect("sixty-four nibbles are a digest");
    let other = EngineImage {
        path: PathBuf::from("some/other/image.exe"),
        bytes: vec![0_u8; 16],
        digest: wrong,
    };
    assert_eq!(
        eye_mechanism_source(&other),
        Err(CockpitEyeError::DigestMismatch { found: wrong }),
        "only the measured image's spans can back the mechanism claim"
    );
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_t542.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T542` written relative to the
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
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_t542_` tests from a recorded `cargo test` output.
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
            let full = &after[..separator];
            if !full.contains("accept_t542_") {
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

// ------------------------------------------------------------- artifacts ---

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
                 \"eye-placement.json\", \"eye-mechanism.json\"]}}",
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

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC calendar
/// date, because `std` has no date formatting.
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
