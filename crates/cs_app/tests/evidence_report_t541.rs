//! Evidence-report harness for task #541 (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_t541_*`: it is not part of
//! the acceptance suite, it fails loudly when its inputs are missing instead of
//! passing vacuously, and the task's test selection must never pick it up as an
//! acceptance test. Run from the workspace root, after the acceptance suite,
//! exactly as:
//!
//! 1. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T541 \
//!      CS_ENGINE_IMAGE="$CS_ENGINE_IMAGE" \
//!      CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --workspace --locked -- accept_t541_ --include-ignored \
//!      2>&1 | tee private/evidence/T541/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.) `CS_EVIDENCE_DIR` must be set **in this step**,
//!    while the acceptance suite runs: the retail census test derives its
//!    `horizon-bindings.json` and the image test its `horizon-mechanism.json`
//!    into that directory only while the variable is set — so without it step 2
//!    finds the directory holding no artifacts and panics on the missing files.
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T541 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t541_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!    CS_ENGINE_IMAGE="$CS_ENGINE_IMAGE" \
//!      cargo test --locked -p cs_app --test evidence_report_t541 -- --ignored
//!    ```
//!    (`CS_EVIDENCE_DIR` tells this harness where step 1's run left the
//!    artifacts it must read, hash and re-check, so the census this report
//!    hashes is this run's own; `CS_EVIDENCE_ARGV` stays the acceptance
//!    command's bare argv, because an environment prefix is not part of a
//!    command's argv.)
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/T541/acceptance.json \
//!      --artifact-root private/evidence/T541 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T541.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the binding census the
//! retail acceptance test derived over that installation, the mechanism record
//! the image test derived over `$CS_ENGINE_IMAGE`, and `rustc --version` and
//! `Cargo.lock`. Nothing is typed in by hand.
//!
//! The measured chain is re-run **in this harness** as a second production
//! observation — `decode_interp` over the real `ZBD/interp.zbd`,
//! `discover_view_controls` over the two horizon commands,
//! `horizon_bindings` over that census (twenty bindings: sixteen `all_axes`
//! `horizon` anchors and four `keep_own_height` anchors, `zone2_cloud_floor`
//! and `moon_reflection` twice each), `read_gamez_nodes` over the eight
//! world-group archives (`horizon` an `object3d` under `world1` with the
//! zone-sky containers as children, and both lateral-anchor names absent),
//! and `load_engine_image` + `mechanism_source` + the byte pins over the
//! owner-supplied decrypted executable (the stored command names, the two
//! field stores at `+0x8`/`+0xc`, and the apply routine's five-call target
//! list). A stale or hand-written record cannot fake those numbers, because
//! this run recomputes them.
//!
//! The `capabilities` are checked, not assumed: `retail` is carried by the
//! tests that read `$CS_GAME_DIR` and `$CS_ENGINE_IMAGE` and `synthetic` by
//! the six unignored authored-corpus tests; every one must appear in the
//! recorded log and pass.
//!
//! The report's `unknowns` are this task's own **blockers**, and they are
//! empty because the acceptance run passed. What the pass deliberately does
//! not answer — where the original instantiates `zone2_cloud_floor` and
//! `moon_reflection`, and how the original's renderer composes the anchored
//! nodes — stays written out in
//! `docs/findings/2026-10-11-t541-horizon-commands-anchor-scenery-to-camera.md`.
//! The claim is `implemented`, never `checked` or `verified_original`: byte
//! inspection of the owner-supplied image is static analysis, not a run of
//! the original.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::camera::{
    CameraOperation, HorizonAnchorRule, HorizonMechanism, ViewCommandClaim, ViewControlCoverage,
    ViewControlEffect, discover_view_controls, horizon_bindings, mechanism_source,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::coordinates::{EngineImage, load_engine_image, original_image_digest};
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::interp::decode_interp;
use cs_formats::io::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

/// The acceptance tests whose `retail` capability this report declares: they
/// read `$CS_GAME_DIR` and `$CS_ENGINE_IMAGE`.
const REQUIRED_TESTS: &[&str] = &[
    "accept_t541_retail_the_loading_scripts_bind_twenty_named_scenery_nodes",
    "accept_t541_retail_the_bound_horizon_is_an_object3d_under_world1",
    "accept_t541_image_the_mechanism_spans_cover_the_measured_instructions",
];

/// The synthetic half of the suite, which must be present beside the
/// capability-bound one: an evidence report that only ever ran
/// capability-bound tests would not show that the pass's contract is pinned
/// without them.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_t541_the_two_measured_commands_name_their_rules",
    "accept_t541_the_measured_write_anchors_the_node_to_the_camera",
    "accept_t541_the_binding_takes_a_position_never_a_rotation",
    "accept_t541_a_binding_records_node_rule_origin_and_provenance",
    "accept_t541_bindings_derive_from_a_measured_census",
    "accept_t541_the_mechanism_refuses_an_image_it_was_not_measured_in",
];

/// The derived binding census the retail acceptance test wrote.
const BINDING_ARTIFACT: &str = "horizon-bindings.json";
/// The derived mechanism record the image acceptance test wrote.
const MECHANISM_ARTIFACT: &str = "horizon-mechanism.json";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_t541_writes_the_acceptance_report() {
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
        "no `accept_t541_` tests were recorded in {}",
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
                    "{required} did not run: T541 declares `retail`, so step 1 must run with \
                     `--include-ignored` and CS_GAME_DIR/CS_ENGINE_IMAGE set"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }
    assert_eq!(
        suite.assertions.len(),
        REQUIRED_TESTS.len() + SYNTHETIC_TESTS.len(),
        "and every `accept_t541_` test in the selection is one this report names: {:?}",
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

    // The census the retail acceptance test wrote over that installation, and
    // the mechanism record the image test wrote over the owner-supplied
    // executable. Both are checked against the tree and the hashes rather
    // than trusted, because files left over from an earlier commit would
    // otherwise be reported as this run's.
    let binding_path = evidence_dir.join(BINDING_ARTIFACT);
    let bindings_json = fs::read_to_string(&binding_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; step 1 must set CS_EVIDENCE_DIR so the retail census test \
             writes its derived census",
            binding_path.display()
        )
    });
    for needle in [
        format!("\"candidate_tree\":\"{candidate_tree}\""),
        format!("\"install_sha256\":\"{install_sha256}\""),
        format!("\"content_sha256\":\"{content_sha256}\""),
        "\"horizon\":16".to_owned(),
        "\"horizon_xz\":4".to_owned(),
    ] {
        assert!(
            bindings_json.contains(&needle),
            "the binding census does not describe this run ({needle:?}); a stale artifact \
             cannot be reported as measured evidence"
        );
    }
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
    remeasure_bindings(
        &game_dir,
        &manifest_sha256("zbd/interp.zbd"),
        fingerprint(&found.manifest),
    );
    remeasure_world_archives(&game_dir, &manifest_sha256);
    remeasure_mechanism();

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&binding_path, "json", &evidence_dir),
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
         \x20\"task_id\": \"T541\",\n\
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
        "\"task_id\": \"T541\"",
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
    `CS_ENGINE_IMAGE` set, every `accept_t541_` test selected by prefix with \
    `--include-ignored`; this harness derives every field from the recorded log, production \
    discovery of $CS_GAME_DIR, the binding census the retail test derived over that \
    installation and the mechanism record the image test derived over the owner-supplied \
    decrypted executable, and a second full production re-run of the measured chain inside \
    the harness itself (`decode_interp`, `discover_view_controls`, `horizon_bindings`, \
    `read_gamez_nodes` over the eight world-group `gamez.zbd` archives, `load_engine_image`, \
    `mechanism_source` and the instruction byte pins), which re-derives twenty declared \
    bindings (sixteen `all_axes` `horizon` anchors and four `keep_own_height` anchors — \
    `zone2_cloud_floor` and `moon_reflection` twice each), the `horizon` object3d under \
    `world1` in every world group with the zone-sky containers as children, and the apply \
    routine's five-call target list rather than trusting the log; both artifacts are checked \
    against this report's own tree and hashes before they are referenced, so a stale file \
    cannot be reported as this run's. Validated with tools/validate_evidence.py \
    --require-pass. LIMITS OF WHAT WAS MEASURED: `retail` is read access to original files \
    and byte inspection of the owner-supplied decrypted executable is static analysis — \
    neither is a run of the original; where the original's runtime name registry resolves \
    `zone2_cloud_floor` and `moon_reflection` from is unrecorded (the names are not stored \
    nodes in any world-group `gamez.zbd`); whether the original's renderer draws the anchored \
    subtrees exactly at the written translation each pass is unrecorded runtime behavior. \
    `claim` is `implemented` only. `candidate_tree` is the tree of the commit the suite ran \
    on; the only later delta is this report's own copy under docs/findings/evidence/, whose \
    bytes are that file.";

/// The claim id the harness's own measured provenance is recorded under.
fn claim() -> ClaimId {
    ClaimId::new("t541.evidence-harness").expect("a valid claim id")
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

/// The claims over the two measured horizon commands — the same measured
/// effects and consumer the acceptance suite's claim list carries.
fn horizon_claims(provenance: Provenance) -> Vec<ViewCommandClaim> {
    [
        (
            "CameraSetHorizon",
            ViewControlEffect::AnchorsSceneryToCamera,
        ),
        (
            "CameraSetHorizonXZ",
            ViewControlEffect::AnchorsSceneryToCameraLaterally,
        ),
    ]
    .into_iter()
    .map(|(command, effect)| {
        ViewCommandClaim::try_new(
            command,
            2,
            effect,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::SceneryAnchors,
            },
            provenance.clone(),
        )
        .expect("the measured claims are well formed")
    })
    .collect()
}

/// Re-runs the script half of the measured chain over `$CS_GAME_DIR`: decode
/// the real `ZBD/interp.zbd`, census the two horizon commands and derive the
/// binding set — the numbers the report claims, recomputed here.
fn remeasure_bindings(game_dir: &Path, interp_sha256: &ContentHash, install_sha256: ContentHash) {
    let container_label = "ZBD/interp.zbd";
    let bytes = fs::read(game_dir.join("ZBD").join("interp.zbd"))
        .expect("the loading-script container must be there");
    assert_eq!(
        sha256(&bytes),
        *interp_sha256,
        "the bytes this re-measurement walks are the manifest's own"
    );
    let decoded = decode_interp(&mut ParseContext::with_defaults(container_label), &bytes)
        .expect("the real container decodes");
    let claim_provenance = retail_provenance(install_sha256, container_label, 18_563, 1_012);
    let census = discover_view_controls(
        &decoded,
        &horizon_claims(claim_provenance.clone()),
        container_label,
        claim_provenance,
    )
    .expect("the real container stores both measured commands");

    let install_span = SourceSpan::new(
        install_sha256,
        container_label,
        None,
        0,
        bytes.len() as u64,
        None,
    )
    .expect("a span over the real container is valid");
    let bindings = horizon_bindings(
        &census,
        &Origin::Installation {
            source: install_span,
        },
    )
    .expect("every measured occurrence carries a node to bind");

    assert_eq!(bindings.len(), 20, "sixteen plus four measured occurrences");
    let mut lateral: BTreeMap<&str, usize> = BTreeMap::new();
    for binding in &bindings {
        match binding.rule() {
            HorizonAnchorRule::AllAxes => {
                assert_eq!(binding.node(), "horizon");
            }
            HorizonAnchorRule::KeepOwnHeight => {
                *lateral.entry(binding.node()).or_default() += 1;
            }
        }
        assert!(
            matches!(binding.origin(), Origin::Installation { .. }),
            "a binding over the real container is installation data"
        );
    }
    assert_eq!(
        bindings
            .iter()
            .filter(|binding| binding.rule() == HorizonAnchorRule::AllAxes)
            .count(),
        16,
        "sixteen all-axes anchors, two cameras in each of eight world groups"
    );
    assert_eq!(
        lateral,
        BTreeMap::from([("moon_reflection", 2), ("zone2_cloud_floor", 2)]),
        "C1 binds the cloud floor and C1B the moon reflection"
    );
}

/// Re-runs the world half: `horizon` an `object3d` under `world1` with zone
/// children in every world group's `gamez.zbd`, and the two lateral-anchor
/// names absent from every stored node array.
fn remeasure_world_archives(game_dir: &Path, manifest_sha256: &dyn Fn(&str) -> ContentHash) {
    let mut checked = 0_usize;
    for group in ["C1", "C1B", "C1C", "C2", "C2B", "C3", "C4", "C5"] {
        let rel = format!("ZBD/{group}/gamez.zbd");
        let label = format!("zbd/{}/gamez.zbd", group.to_ascii_lowercase());
        let bytes = fs::read(game_dir.join(&rel)).expect("the world group's archive must be there");
        assert_eq!(
            sha256(&bytes),
            manifest_sha256(&label),
            "the node array this re-measurement reads is the manifest's own: {rel}"
        );
        let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(&rel), &bytes)
            .unwrap_or_else(|error| panic!("{rel}: the node array reads: {error}"));
        let horizon = nodes
            .nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case("horizon"))
            .unwrap_or_else(|| panic!("{rel}: a `horizon` node is stored"));
        assert!(
            horizon.object3d().is_some(),
            "{rel}: `horizon` is an object3d"
        );
        let parent = horizon
            .parent
            .and_then(|slot| nodes.get(slot))
            .unwrap_or_else(|| panic!("{rel}: `horizon`'s parent resolves"));
        assert_eq!(parent.name, "world1", "{rel}: `horizon` hangs off world1");
        assert!(
            horizon
                .children
                .iter()
                .filter_map(|slot| nodes.get(*slot))
                .any(|child| child.name.starts_with("zone")),
            "{rel}: the zone-sky containers are `horizon`'s children"
        );
        for name in ["zone2_cloud_floor", "moon_reflection"] {
            assert!(
                !nodes
                    .nodes
                    .iter()
                    .any(|node| node.name.eq_ignore_ascii_case(name)),
                "{rel}: {name} is not a stored world-archive node"
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 8, "every world group's archive was re-read");
}

/// Re-runs the image half: load the owner-supplied decrypted executable and
/// re-derive the mechanism's spans, then re-check the measured bytes — the
/// stored command names, the two field stores and the apply routine's call
/// targets — inside this harness.
fn remeasure_mechanism() {
    let image: EngineImage =
        load_engine_image().expect("CS_ENGINE_IMAGE names the owner-supplied decrypted executable");
    let mechanism: HorizonMechanism =
        mechanism_source(&image).expect("the measured image yields its spans");
    let bytes = |span: &SourceSpan| {
        &image.bytes[span.offset() as usize..(span.offset() + span.length()) as usize]
    };
    assert_eq!(bytes(&mechanism.command), b"CameraSetHorizon\0");
    assert_eq!(bytes(&mechanism.command_xz), b"CameraSetHorizonXZ\0");
    assert_eq!(
        &bytes(&mechanism.bind)[0x6e..0x71],
        &[0x89, 0x41, 0x08],
        "the CameraSetHorizon handler stores the resolved node at +0x8"
    );
    assert_eq!(
        &bytes(&mechanism.bind_xz)[0x6e..0x71],
        &[0x89, 0x41, 0x0c],
        "the CameraSetHorizonXZ handler stores the resolved node at +0xc"
    );
    let apply = bytes(&mechanism.apply);
    assert_eq!(&apply[5..8], &[0x8b, 0x48, 0x08]);
    assert_eq!(&apply[0x30..0x33], &[0x8b, 0x48, 0x0c]);
    let mut targets: Vec<u32> = (0..apply.len().saturating_sub(5))
        .filter(|&index| apply[index] == 0xe8)
        .map(|index| {
            let insn_va = 0x40_0000_u64 + mechanism.apply.offset() + index as u64;
            let relative =
                i32::from_le_bytes(apply[index + 1..index + 5].try_into().expect("four bytes"));
            insn_va.wrapping_add(5).wrapping_add_signed(relative as i64) as u32
        })
        .collect();
    targets.sort_unstable();
    assert_eq!(
        targets,
        vec![0x4d0210, 0x4d0210, 0x4d1cc0, 0x4d1d50, 0x4d1d50],
        "the apply routine's own calls, recomputed in this harness"
    );
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_t541.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T541` written relative to the
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
/// `accept_t541_` tests from a recorded `cargo test` output.
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
        // A status on its own own line completes the earliest test that was
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
            if !full.contains("accept_t541_") {
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
                 \"horizon-bindings.json\", \"horizon-mechanism.json\"]}}",
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
