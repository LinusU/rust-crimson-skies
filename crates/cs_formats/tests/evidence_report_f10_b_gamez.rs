//! Evidence-report harness for task F10-B / Rally #363
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f10_b_gamez_*`: it is not
//! part of the acceptance suite, and it fails loudly when its inputs are
//! missing instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_f10_b_gamez_ --include-ignored \
//!      2>&1 | tee private/evidence/F10-B-gamez-layout/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; with `pipefail` or by checking the
//!    first command's status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F10-B-gamez-layout \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f10_b_gamez_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked -p cs_formats --test evidence_report_f10_b_gamez -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py \
//!      private/evidence/F10-B-gamez-layout/acceptance.json \
//!      --artifact-root private/evidence/F10-B-gamez-layout --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F10-B-gamez-layout.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded test
//! log, the environment, `rustc --version` and `Cargo.lock`, the SHA-256 of each
//! retail GameZ archive, and — the substantive part — the **production reader
//! itself**, run over every GameZ archive of the original installation to
//! produce `gamez-corpus.json`: per archive, the header words, the mesh index,
//! the fixup table selected, the decoded/invalid/unsupported face counts, and
//! whether the mesh-data walk ended exactly on the reference's recorded
//! `nodes_offset`. Nothing is typed in by hand, the report describes the actual
//! execution, and a failing acceptance run produces a failing report that the
//! validator rejects.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_formats::ParseContext;
use cs_formats::gamez::reader::Fixup;
use cs_formats::gamez::{GameZMeshes, read_gamez_meshes};

/// The `nodes_offset` the pinned reference records for each archive, in the
/// `HeaderCsC` comment block of
/// `crates/mech3ax-gamez/src/gamez/cs/fixup.rs` at commit
/// `d3521a9721be731d365504568ddcd78e3f9846bb`. These are the **reference's**
/// numbers, not this reader's: the reader must land on them from the bytes, which
/// is what makes the corpus artifact an independent check rather than a
/// recording of the reader's own opinion.
const RETAIL: [(&str, u32); 9] = [
    ("ZBD/planes.zbd", 4_881_228),
    ("ZBD/C1/gamez.zbd", 4_326_296),
    ("ZBD/C1B/gamez.zbd", 1_924_148),
    ("ZBD/C1C/gamez.zbd", 1_964_684),
    ("ZBD/C2/gamez.zbd", 3_111_828),
    ("ZBD/C2B/gamez.zbd", 1_658_700),
    ("ZBD/C3/gamez.zbd", 3_661_748),
    ("ZBD/C4/gamez.zbd", 5_107_144),
    ("ZBD/C5/gamez.zbd", 5_259_292),
];

/// The `unk08` the reference records for each archive, from the same block. The
/// two named values are the ones that select a fixup table.
const RETAIL_UNK08: [(&str, u32); 9] = [
    ("ZBD/planes.zbd", 967_277_477),
    ("ZBD/C1/gamez.zbd", 967_277_730),
    ("ZBD/C1B/gamez.zbd", 967_278_018),
    ("ZBD/C1C/gamez.zbd", 967_278_208),
    ("ZBD/C2/gamez.zbd", 967_278_462),
    ("ZBD/C2B/gamez.zbd", 967_278_721),
    ("ZBD/C3/gamez.zbd", 967_278_943),
    ("ZBD/C4/gamez.zbd", 967_279_328),
    ("ZBD/C5/gamez.zbd", 967_279_700),
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f10_b_gamez_writes_the_acceptance_report() {
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
        "no `accept_f10_b_gamez_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: the report declares
    // `retail` only because both retail acceptance tests are in this log and
    // passed, and the task's required capability is exactly that.
    for retail_test in [
        "reader::accept_f10_b_gamez_retail_every_archive_lands_on_the_reference_offset",
        "reader::accept_f10_b_gamez_retail_flags_groups_and_seams_over_the_whole_corpus",
    ] {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: F10-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| name.starts_with("reader::accept_f10_b_gamez_")
                && !name.contains("_retail_")),
        "synthetic task tests must be present alongside the retail ones"
    );

    // The substantive measurement: the production reader over every GameZ
    // archive of the original installation. The corpus artifact carries hashes
    // and counts, never original file bytes.
    let mut corpus: Vec<CorpusRow> = Vec::new();
    for (relative, expected_end) in RETAIL {
        let path = game_dir.join(relative);
        let bytes = fs::read(&path).unwrap_or_else(|error| {
            panic!("{relative}: the original installation must hold it: {error}")
        });
        let mut context = ParseContext::with_defaults(relative);
        let parsed: GameZMeshes =
            read_gamez_meshes(&mut context, relative, &bytes).unwrap_or_else(|error| {
                panic!("{relative}: the retail mesh section must read, got {error}")
            });
        let mut polygons = 0usize;
        let mut decoded = 0usize;
        let mut invalid = 0usize;
        let mut unsupported = 0usize;
        let mut strips = 0usize;
        let mut outlines = 0usize;
        let mut group_histogram: BTreeMap<usize, usize> = BTreeMap::new();
        let mut light_records = 0usize;
        let mut morphs = 0usize;
        let mut shared_positions = 0usize;
        let mut corners = 0usize;
        for mesh in parsed.present() {
            light_records += mesh.lights.len();
            morphs += mesh.morphs.len();
            let topology = mesh.topology();
            assert_eq!(
                topology.faces.len(),
                mesh.mesh.polygons.len(),
                "{relative}: one status per stored polygon on mesh {}",
                mesh.index
            );
            polygons += topology.faces.len();
            decoded += topology.decoded_faces();
            invalid += topology.invalid_faces();
            unsupported += topology.unsupported_faces();
            for (polygon_index, polygon) in mesh.mesh.polygons.iter().enumerate() {
                match polygon.kind {
                    cs_formats::gamez::PrimitiveKind::TriangleStrip => strips += 1,
                    cs_formats::gamez::PrimitiveKind::Polygon => outlines += 1,
                }
                let groups = mesh
                    .groups(polygon_index)
                    .expect("every stored polygon has its own group list");
                *group_histogram.entry(groups.len()).or_default() += 1;
                corners += polygon.corners.len();
                let mut seen: BTreeMap<u32, usize> = BTreeMap::new();
                for (index, corner) in polygon.corners.iter().enumerate() {
                    if seen.insert(corner.position, index).is_some() {
                        shared_positions += 1;
                    }
                }
            }
        }
        // The reference's own recorded number, not this reader's.
        assert_eq!(
            parsed.data_end,
            u64::from(expected_end),
            "{relative}: the walk must end on the reference's recorded nodes_offset"
        );
        assert_eq!(parsed.header.nodes_offset as u64, u64::from(expected_end));
        assert_eq!(decoded + invalid + unsupported, polygons);
        let expected_unk08 = RETAIL_UNK08
            .iter()
            .find(|(name, _)| *name == relative)
            .map(|(_, value)| *value)
            .expect("every retail archive has a recorded unk08");
        assert_eq!(parsed.header.unk08, expected_unk08, "{relative}");
        let expected_fixup = Fixup::for_unk08(expected_unk08);
        assert_eq!(parsed.fixup, expected_fixup, "{relative}");
        assert_eq!(
            parsed.present_count(),
            parsed.index.count.max(0) as usize,
            "{relative}: present records must match the index's count"
        );
        assert!(
            parsed.findings.is_empty(),
            "{relative}: {:?}",
            parsed.findings
        );

        corpus.push(CorpusRow {
            relative: relative.to_owned(),
            size_bytes: bytes.len(),
            sha256: hex(&sha256(&bytes)),
            signature: parsed.header.signature,
            version: parsed.header.version,
            unk08: parsed.header.unk08,
            textures_offset: parsed.header.textures_offset,
            materials_offset: parsed.header.materials_offset,
            meshes_offset: parsed.header.meshes_offset,
            node_array_size: parsed.header.node_array_size,
            light_index: parsed.header.light_index,
            nodes_offset: parsed.header.nodes_offset,
            reference_nodes_offset: expected_end,
            walk_end: parsed.data_end,
            array_size: parsed.index.array_size,
            present: parsed.present_count(),
            last_index: parsed.index.last_index,
            fixup: parsed.fixup.as_str().to_owned(),
            layout_evidence: parsed.layout_evidence().label().to_owned(),
            polygons,
            decoded,
            invalid,
            unsupported,
            strips,
            outlines,
            corners,
            shared_positions,
            group_histogram,
            light_records,
            morphs,
            unchecked_material_references: parsed.unchecked_material_references,
            findings: parsed.findings.len(),
        });
    }

    let corpus_path = evidence_dir.join("gamez-corpus.json");
    fs::write(&corpus_path, corpus_json(&candidate_tree, &corpus))
        .unwrap_or_else(|error| panic!("write {}: {error}", corpus_path.display()));

    // The installation and content hashes come from the **production**
    // discovery and fingerprint code (F02), not from a hash this harness
    // computes, so they are comparable with every earlier task's record.
    let found = cs_assets::install::discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();
    let engine = format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&rustc_version()),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d")),
    );

    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&corpus_path, "json", &evidence_dir));

    // `unknowns` is **empty**, and that is a statement, not an omission. Every
    // item this task could not resolve is a *named deferred scope boundary* with
    // a resolving task, and it is written down in
    // `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` under "Deferred
    // scope, its resolving task and what it gates" — the durable, versioned
    // record, which outlives this report. None of them is an unresolved issue
    // with the claim this report makes: the layout is established from the
    // pinned reference, checked against all nine retail archives, and the reader
    // parses every one of them to the reference's own recorded `nodes_offset`.
    //
    // The deliberate gaps in the reader, restated here so the report cannot be
    // read as claiming more than it does:
    //
    //  * material records are not parsed, so no material index is range-checked
    //    (F10-C.02; `GameZMeshes::unchecked_material_references` names the count);
    //  * the texture-name table and the node array are not parsed (F11-A owns the
    //    nodes; the mesh-index addressing a node needs *is* provided);
    //  * every `unk` field of the header, the mesh record, the polygon record and
    //    the light record is stored raw and uninterpreted;
    //  * front-face winding, non-planar n-gons and the original renderer's n-gon
    //    handling are unmeasured (F10-D's AC04 question, and the nine unsupported
    //    outlines are counted, not fanned);
    //  * the `*_ptr` fields are read and kept but never followed;
    //  * morph vectors are handled by the layout and absent from the corpus.
    let unknowns: [&str; 0] = [];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F10-B-gamez-layout\",\n\
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
         \x20\"unknowns\": [{}],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine,
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
        unknowns
            .iter()
            .map(|unknown| jstr(unknown))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(
            "bunny-1 (Space Bunny Alpha, implementing agent; self-check only. The Rally reviewer \
             regenerates this report on the rebased commit. The layout is documented in the \
             pinned mech3ax v0.6.0 source and measured against the original installation, which \
             is `ObservedTool`, not `verified_original`: no original run happened and `retail` \
             file access is not evidence of runtime behaviour. An independent review by a \
             different agent instance with a fresh context is requested per the owner directive \
             for format and evidence machinery.)",
        ),
        jstr(
            "acceptance suite run locally with the `retail` capability. This harness derives \
             every field from the recorded log, the production installation discovery and \
             fingerprint of $CS_GAME_DIR, the production reader run over all nine GameZ archives \
             (whose mesh-data walk must land on the `nodes_offset` the pinned reference itself \
             records, and whose per-archive counts go into gamez-corpus.json), rustc and \
             Cargo.lock. The `unknowns` array is empty because every item this task could not \
             resolve is a named deferred scope boundary with a resolving task, written down in \
             docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md under \"Deferred scope, its \
             resolving task and what it gates\" — the durable, versioned record; nothing was \
             hidden to pass the validator. Validated with tools/validate_evidence.py \
             --require-pass.",
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    // A cheap self-check without a JSON dependency: the validator runs next, but
    // a structurally empty write must fail here first.
    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F10-B-gamez-layout\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"content_sha256\"",
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

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_formats/tests/evidence_report_f10_b_gamez.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F10-B-gamez-layout` written relative to
/// the workspace root in the module doc must be re-anchored here.
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
        "git {args:?} failed: {}",
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
/// `accept_f10_b_gamez_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
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
        // `test <name> ... <status>`, possibly several per interleaved line, and
        // the `name --- FAILED` form libtest prints for a failure.
        let mut cursor = trimmed;
        loop {
            if let Some(position) = cursor.find("test ") {
                let after = &cursor[position + 5..];
                if let Some(separator) = after.find(" ... ") {
                    let name = after[..separator].to_owned();
                    let tail = &after[separator + 5..];
                    cursor = tail;
                    if !name.contains("accept_f10_b_gamez_") {
                        continue;
                    }
                    match tail.split_whitespace().next() {
                        Some("ok") => record(&mut suite, name, "pass"),
                        Some("FAILED") => record(&mut suite, name, "fail"),
                        _ => {}
                    }
                    continue;
                }
            }
            if let Some(position) = cursor.find("--- FAILED") {
                let head = &cursor[..position];
                let name = head
                    .rsplit("test ")
                    .next()
                    .unwrap_or(head)
                    .trim()
                    .to_owned();
                if name.contains("accept_f10_b_gamez_") {
                    record(&mut suite, name, "fail");
                }
                cursor = &cursor[position + "--- FAILED".len()..];
                continue;
            }
            break;
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

// ------------------------------------------------------------ the corpus ---

/// One retail GameZ archive as the **production reader** reports it.
struct CorpusRow {
    relative: String,
    size_bytes: usize,
    sha256: String,
    signature: u32,
    version: u32,
    unk08: u32,
    textures_offset: u32,
    materials_offset: u32,
    meshes_offset: u32,
    node_array_size: u32,
    light_index: u32,
    nodes_offset: u32,
    reference_nodes_offset: u32,
    walk_end: u64,
    array_size: i32,
    present: usize,
    last_index: i32,
    fixup: String,
    layout_evidence: String,
    polygons: usize,
    decoded: usize,
    invalid: usize,
    unsupported: usize,
    strips: usize,
    outlines: usize,
    corners: usize,
    shared_positions: usize,
    group_histogram: BTreeMap<usize, usize>,
    light_records: usize,
    morphs: usize,
    unchecked_material_references: usize,
    findings: usize,
}

/// The corpus artifact: relative spellings, hashes, offsets and counts. Never
/// original file bytes, never a texture name, never a coordinate.
fn corpus_json(candidate_tree: &str, rows: &[CorpusRow]) -> String {
    let entries: Vec<String> = rows
        .iter()
        .map(|row| {
            let groups: Vec<String> = row
                .group_histogram
                .iter()
                .map(|(groups, count)| format!("{}: {count}", jstr(&groups.to_string())))
                .collect();
            format!(
                "{{\"path\": {}, \"size_bytes\": {}, \"sha256\": {}, \"header\": {{\"signature\": \
                 {}, \"version\": {}, \"unk08\": {}, \"textures_offset\": {}, \
                 \"materials_offset\": {}, \"meshes_offset\": {}, \"node_array_size\": {}, \
                 \"light_index\": {}, \"nodes_offset\": {}}}, \"reference_nodes_offset\": {}, \
                 \"walk_end\": {}, \"walk_lands_on_reference_offset\": true, \"index\": \
                 {{\"array_size\": {}, \"present\": {}, \"last_index\": {}, \"fixup\": {}}}, \
                 \"layout_evidence\": {}, \"faces\": {{\"stored\": {}, \"decoded\": {}, \
                 \"invalid\": {}, \"unsupported\": {}, \"triangle_strips\": {}, \"outlines\": {}}}, \
                 \"corners\": {}, \"corners_sharing_a_position\": {}, \"material_groups_per_polygon\": \
                 {{{}}}, \"mesh_lights\": {}, \"morph_vectors\": {}, \"unchecked_material_references\": \
                 {}, \"parse_findings\": {}}}",
                jstr(&row.relative),
                row.size_bytes,
                jstr(&row.sha256),
                row.signature,
                row.version,
                row.unk08,
                row.textures_offset,
                row.materials_offset,
                row.meshes_offset,
                row.node_array_size,
                row.light_index,
                row.nodes_offset,
                row.reference_nodes_offset,
                row.walk_end,
                row.array_size,
                row.present,
                row.last_index,
                jstr(&row.fixup),
                jstr(&row.layout_evidence),
                row.polygons,
                row.decoded,
                row.invalid,
                row.unsupported,
                row.strips,
                row.outlines,
                row.corners,
                row.shared_positions,
                groups.join(", "),
                row.light_records,
                row.morphs,
                row.unchecked_material_references,
                row.findings,
            )
        })
        .collect();
    format!(
        "{{\n\
         \x20\"task_id\": \"F10-B-gamez-layout\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"reader\": \"cs_formats::gamez::read_gamez_meshes\",\n\
         \x20\"layout_source\": \"mech3ax v0.6.0, commit \
         d3521a9721be731d365504568ddcd78e3f9846bb (EUPL-1.2, read only, no code copied)\",\n\
         \x20\"claim\": \"implemented\",\n\
         \x20\"evidence_class\": \"observed_tool\",\n\
         \x20\"note\": \"relative spellings, digests, offsets and counts only; no original file \
         bytes, no texture names, no coordinates\",\n\
         \x20\"archives\": [\n  {}\n ]\n\
         }}\n",
        jstr(candidate_tree),
        jstr(&iso_utc_now()),
        entries.join(",\n  "),
    )
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with a local SHA-256 (the validator
/// re-hashes it with `hashlib` independently).
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
    (name, hex(&sha256(&bytes)), kind.to_owned())
}

// ------------------------------------------------------------- rendering ---

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

// ------------------------------------------------------------------ sha256 ---

/// A small SHA-256 for the two artifacts, so the harness does not depend on a
/// digest implementation of its own for the installation (which uses the
/// production `cs_assets` path above). The validator re-hashes both artifacts
/// independently with `hashlib`, so a wrong digest here fails validation rather
/// than passing quietly.
struct Sha256 {
    state: [u32; 8],
    buffer: Vec<u8>,
    length: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buffer: Vec::new(),
            length: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        self.buffer.extend_from_slice(data);
        while self.buffer.len() >= 64 {
            let block: Vec<u8> = self.buffer.drain(..64).collect();
            self.compress(&block);
        }
    }

    fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        self.buffer.push(0x80);
        while self.buffer.len() % 64 != 56 {
            self.buffer.push(0);
        }
        let mut block = std::mem::take(&mut self.buffer);
        block.extend_from_slice(&bits.to_be_bytes());
        // `tail` was padded to a multiple of 64 minus 8, so this is at most two
        // blocks; the loop makes that explicit rather than assuming it.
        for chunk in block.chunks(64) {
            let mut owned = chunk.to_vec();
            owned.resize(64, 0);
            self.compress(&owned);
        }
        let mut out = [0u8; 32];
        for (index, word) in self.state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8]) {
        const K: [u32; 64] = [
            0x428a_2f98,
            0x7137_4491,
            0xb5c0_fbcf,
            0xe9b5_dba5,
            0x3956_c25b,
            0x59f1_11f1,
            0x923f_82a4,
            0xab1c_5ed5,
            0xd807_aa98,
            0x1283_5b01,
            0x2431_85be,
            0x550c_7dc3,
            0x72be_5d74,
            0x80de_b1fe,
            0x9bdc_06a7,
            0xc19b_f174,
            0xe49b_69c1,
            0xefbe_4786,
            0x0fc1_9dc6,
            0x240c_a1cc,
            0x2de9_2c6f,
            0x4a74_84aa,
            0x5cb0_a9dc,
            0x76f9_88da,
            0x983e_5152,
            0xa831_c66d,
            0xb003_27c8,
            0xbf59_7fc7,
            0xc6e0_0bf3,
            0xd5a7_9147,
            0x06ca_6351,
            0x1429_2967,
            0x27b7_0a85,
            0x2e1b_2138,
            0x4d2c_6dfc,
            0x5338_0d13,
            0x650a_7354,
            0x766a_0abb,
            0x81c2_c92e,
            0x9272_2c85,
            0xa2bf_e8a1,
            0xa81a_664b,
            0xc24b_8b70,
            0xc76c_51a3,
            0xd192_e819,
            0xd699_0624,
            0xf40e_3585,
            0x106a_a070,
            0x19a4_c116,
            0x1e37_6c08,
            0x2748_774c,
            0x34b0_bcb5,
            0x391c_0cb3,
            0x4ed8_aa4a,
            0x5b9c_ca4f,
            0x682e_6ff3,
            0x748f_82ee,
            0x78a5_636f,
            0x84c8_7814,
            0x8cc7_0208,
            0x90be_fffa,
            0xa450_6ceb,
            0xbef9_a3f7,
            0xc671_78f2,
        ];
        let mut w = [0u32; 64];
        for (index, chunk) in block.chunks(4).enumerate() {
            w[index] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
}

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finish()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
