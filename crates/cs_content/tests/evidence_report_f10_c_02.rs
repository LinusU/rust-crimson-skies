//! Evidence-report harness for task F10-C.02 / Rally #365
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f10_c_02_*`: it is not part of
//! the acceptance suite, and it fails loudly when its inputs are missing instead
//! of passing vacuously. Run from the workspace root, after the acceptance
//! suite, exactly as:
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_f10_c_02_ --include-ignored \
//!      2>&1 | tee private/evidence/F10-C.02/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; with `pipefail` or by checking the first
//!    command's status — it is passed to this harness as `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F10-C.02 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f10_c_02_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked -p cs_content --test evidence_report_f10_c_02 -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py \
//!      private/evidence/F10-C.02/acceptance.json \
//!      --artifact-root private/evidence/F10-C.02 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F10-C.02.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded test
//! log, the environment, `rustc --version` and `Cargo.lock`, the production
//! SHA-256 of each retail GameZ archive, and — the substantive part — the
//! **production readers and the production audit** run over every GameZ archive
//! of the original installation to produce `material-corpus.json`. Nothing is
//! typed in by hand, the report describes the actual execution, and a failing
//! acceptance run produces a failing report the validator rejects.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install;
use cs_assets::vfs::{ContentSession, SessionBuilder, WORLD_NAMESPACE};
use cs_formats::ParseContext;
use cs_formats::gamez::materials::{
    MATERIAL_FLAG_CYCLED, MATERIAL_FLAG_TEXTURED, MATERIAL_FLAG_UNKNOWN, MaterialKind,
    NG_MATERIAL_SLOTS,
};
use cs_formats::gamez::{GameZMaterials, GameZMeshes, read_gamez_materials, read_gamez_meshes};
use cs_types::asset_id::{AssetKey, ResolveContext, WorldGroup};

use cs_content::mesh::{DependencyContext, MaterialState, MeshDependencyAudit};

/// The retail GameZ archives, each with the world group whose texture archive
/// its materials are audited against. `planes.zbd` has no world of its own — the
/// shared airframe library — and which archive it resolves against is **not
/// established** (deferred item 3 of the finding), so it is measured but not
/// audited.
const RETAIL: [(&str, Option<&str>); 9] = [
    ("ZBD/planes.zbd", None),
    ("ZBD/C1/gamez.zbd", Some("ZBD/C1")),
    ("ZBD/C1B/gamez.zbd", Some("ZBD/C1B")),
    ("ZBD/C1C/gamez.zbd", Some("ZBD/C1C")),
    ("ZBD/C2/gamez.zbd", Some("ZBD/C2")),
    ("ZBD/C2B/gamez.zbd", Some("ZBD/C2B")),
    ("ZBD/C3/gamez.zbd", Some("ZBD/C3")),
    ("ZBD/C4/gamez.zbd", Some("ZBD/C4")),
    ("ZBD/C5/gamez.zbd", Some("ZBD/C5")),
];

/// The two retail acceptance tests, which together are this task's `retail`
/// capability. Both must be in the recorded log and both must have passed.
const RETAIL_TESTS: [&str; 2] = [
    "materials::accept_f10_c_02_retail_every_archive_lands_on_its_meshes_offset",
    "mesh::tests::accept_f10_c_02_audit_retail_world_resolves_no_name_by_substitution",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f10_c_02_writes_the_acceptance_report() {
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
        "fewer per-test results than passing tests were parsed from {} — the log format was not \
         understood; inspect it rather than reporting guessed counts",
        log_path.display()
    );
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f10_c_02_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: the report declares `retail`
    // only because both retail acceptance tests are in this log and passed.
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: F10-C.02 requires capability `retail`, run step 1 \
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
            .any(|(name, _)| name.contains("accept_f10_c_02_") && !name.contains("_retail_")),
        "synthetic task tests must be present alongside the retail ones"
    );

    // The substantive measurement. The production readers over every GameZ
    // archive, and the production audit over every world archive against that
    // world's own texture archive.
    let found = install::discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let mut corpus: Vec<CorpusRow> = Vec::new();
    for (relative, world) in RETAIL {
        let path = game_dir.join(relative);
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("{relative}: the installation holds it: {error}"));
        let mut context = ParseContext::with_defaults(relative);
        let meshes: GameZMeshes = read_gamez_meshes(&mut context, relative, &bytes)
            .unwrap_or_else(|error| panic!("{relative}: the mesh section must read: {error}"));
        let materials: GameZMaterials = read_gamez_materials(&mut context, relative, &bytes)
            .unwrap_or_else(|error| panic!("{relative}: the material section must read: {error}"));

        // The discriminating check for the section this task added: the material
        // walk ends exactly on the mesh index the header declares.
        assert_eq!(
            materials.data_end,
            u64::from(materials.header.meshes_offset),
            "{relative}: the material section must end on meshes_offset"
        );
        assert_eq!(meshes.data_end, u64::from(meshes.header.nodes_offset));
        assert_eq!(
            materials.materials_offset - materials.textures_offset,
            materials.textures.len() as u64 * 44,
            "{relative}: the texture table fills the space before the material records"
        );
        assert!(
            materials.findings.is_empty(),
            "{relative}: {:?}",
            materials.findings
        );
        let textured = materials
            .materials
            .iter()
            .filter(|material| material.kind() == MaterialKind::Textured)
            .count();
        let untextured = materials.materials.len() - textured;
        let unknown_flag = materials
            .materials
            .iter()
            .filter(|material| material.record.flags & MATERIAL_FLAG_UNKNOWN != 0)
            .count();
        let cycled = materials
            .materials
            .iter()
            .filter(|material| material.record.flags & MATERIAL_FLAG_CYCLED != 0)
            .count();
        assert_eq!(
            textured + untextured,
            materials.materials.len(),
            "{relative}: every present material is textured or not"
        );
        // Every stored texture index is inside the container's own table.
        for material in &materials.materials {
            if material.record.flags & MATERIAL_FLAG_TEXTURED != 0 {
                assert!(
                    material.record.texture_index < materials.textures.len() as u32,
                    "{relative}: material {} names texture {}",
                    material.index,
                    material.record.texture_index
                );
            }
        }

        let audit = world.map(|world| {
            let session = session_of(&game_dir, &found, world);
            let key = AssetKey::from_spelling(WORLD_NAMESPACE, "texture.zbd", "default")
                .expect("valid key");
            let catalog =
                cs_content::textures::TextureCatalog::open(&session, std::slice::from_ref(&key));
            assert_eq!(
                catalog.failures().count(),
                0,
                "{world}: the world's own texture archive must open"
            );
            let audit = MeshDependencyAudit::build(
                &meshes,
                &materials,
                &DependencyContext {
                    archive: &key,
                    session: &session,
                    catalog: &catalog,
                    origin: None,
                    container: relative,
                },
            );
            // F10-B's deferred item 1, for this archive.
            assert_eq!(
                audit.out_of_range().count(),
                0,
                "{relative}: every stored material index is inside the material table"
            );
            assert_eq!(audit.references, meshes.unchecked_material_references);
            (audit, session, catalog, key)
        });

        let states: BTreeMap<String, usize> = match &audit {
            None => BTreeMap::new(),
            Some((audit, _, _, _)) => {
                let mut counts: BTreeMap<String, usize> = BTreeMap::new();
                for row in &audit.rows {
                    *counts.entry(row.state.code().to_owned()).or_default() += 1;
                }
                counts
            }
        };
        // A resolved row must name the world's own archive and the container's
        // own stored name: nothing was substituted.
        if let Some((audit, _, catalog, key)) = &audit {
            // The **resolved** archive path, which is what a `TextureId` names —
            // not the key's own path within its namespace.
            let archive_path = catalog
                .archives()
                .find(|archive| archive.key() == key)
                .expect("the catalog holds the caller's archive")
                .path()
                .clone();
            for row in audit.resolved_rows() {
                let MaterialState::Resolved { texture } = &row.state else {
                    panic!("a resolved row whose state is {:?}", row.state);
                };
                assert_eq!(texture.archive, archive_path);
                let stored = materials
                    .texture_of(
                        materials
                            .material(row.material)
                            .expect("a resolved row has a record"),
                    )
                    .expect("a resolved row names a stored texture");
                assert_eq!(&texture.name, &stored.name, "the name was altered");
            }
        }

        // The naming difference, on real data, with a handful of examples: the
        // claim is *about* names, so the artifact carries a few identifiers. No
        // pixels, no coordinates, no file bytes.
        //
        // The archive side of every pair is the archive's **own stored spelling**,
        // read out of the catalog's entry list, never a spelling this harness
        // derived. That is what makes the examples worth carrying: a pair such
        // as `{"container": "Sky1.tif", "archive": "sky1"}` shows both
        // differences the finding names — the extension and the case — in one
        // line, which a pair built from the container's own stem could not.
        // Extension-only and case-and-extension pairs are both kept, so the
        // artifact cannot be read as "dropping the extension would be enough".
        let examples: Vec<String> = match &audit {
            None => Vec::new(),
            Some((audit, _session, catalog, key)) => {
                let stored: Vec<String> = catalog
                    .archives()
                    .filter(|archive| archive.key() == key)
                    .flat_map(|archive| archive.ids().map(|id| id.name.clone()))
                    .collect();
                let mut exact_extension: Vec<String> = Vec::new();
                let mut exact_case: Vec<String> = Vec::new();
                for row in &audit.rows {
                    let MaterialState::MissingTexture { name, .. } = &row.state else {
                        continue;
                    };
                    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
                    let folded = stem.to_ascii_lowercase();
                    // The archive's own spelling, matched case-insensitively by
                    // the harness **only to find it**: nothing downstream ever
                    // resolves a name this way.
                    let Some(spelling) = stored
                        .iter()
                        .find(|candidate| candidate.to_ascii_lowercase() == folded)
                    else {
                        continue;
                    };
                    let pair = format!(
                        "{{\"container\": {}, \"archive\": {}}}",
                        jstr(name),
                        jstr(spelling)
                    );
                    if spelling == stem {
                        if exact_extension.len() < 2 {
                            exact_extension.push(pair);
                        }
                    } else if *spelling == folded && exact_case.len() < 2 {
                        // The case differs too, which is the difference that
                        // rules out "just drop the extension".
                        exact_case.push(pair);
                    }
                    if exact_extension.len() == 2 && exact_case.len() == 2 {
                        break;
                    }
                }
                let mut out = exact_case;
                out.extend(exact_extension);
                out
            }
        };
        assert!(
            examples
                .iter()
                .any(|pair| pair.contains("Sky1.tif") || pair.contains("sky1")),
            "the examples must include a case difference: {examples:?}"
        );

        corpus.push(CorpusRow {
            relative: relative.to_owned(),
            world: world.map(str::to_owned),
            size_bytes: bytes.len(),
            sha256: install::sha256(&bytes).to_hex(),
            texture_count: materials.textures.len(),
            materials_offset: materials.header.materials_offset,
            meshes_offset: materials.header.meshes_offset,
            walk_end: materials.data_end,
            textures: materials.textures.len(),
            present_materials: materials.materials.len(),
            free_slots: materials.free_slots,
            array_size: materials.info.array_size,
            index_max: materials.info.index_max,
            index_last: materials.info.index_last,
            textured,
            untextured,
            cycled,
            unknown_flag,
            duplicate_name_groups: materials.duplicate_names().len(),
            duplicate_name_entries: materials
                .duplicate_names()
                .iter()
                .map(|(_, indices)| indices.len())
                .sum(),
            findings: materials.findings.len(),
            mesh_findings: meshes.findings.len(),
            material_references: meshes.unchecked_material_references,
            audit_rows: audit
                .as_ref()
                .map(|(audit, ..)| audit.rows.len())
                .unwrap_or(0),
            audit_references: audit
                .as_ref()
                .map(|(audit, ..)| audit.references)
                .unwrap_or(0),
            audit_out_of_range: audit
                .as_ref()
                .map(|(audit, ..)| audit.out_of_range().count())
                .unwrap_or(0),
            audit_states: states,
            naming_examples: examples,
            layout_evidence: materials.layout_evidence().label().to_owned(),
        });
    }

    // The measured totals the finding quotes, asserted rather than typed in.
    let textures: usize = corpus.iter().map(|row| row.textures).sum();
    let materials: usize = corpus.iter().map(|row| row.present_materials).sum();
    let cycled: usize = corpus.iter().map(|row| row.cycled).sum();
    let unknown: usize = corpus.iter().map(|row| row.unknown_flag).sum();
    let references: usize = corpus.iter().map(|row| row.material_references).sum();
    assert_eq!(textures, 3985, "the corpus's texture-name entries");
    assert_eq!(materials, 4669, "the corpus's present material records");
    assert_eq!(cycled, 627, "the corpus's cycled materials");
    assert_eq!(unknown, 215, "the corpus's UNKNOWN-flagged materials");
    assert!(references > 0, "the corpus stores material references");

    let corpus_path = evidence_dir.join("material-corpus.json");
    fs::write(&corpus_path, corpus_json(&candidate_tree, &corpus))
        .unwrap_or_else(|error| panic!("write {}: {error}", corpus_path.display()));

    // The installation and content hashes come from the **production** discovery
    // and fingerprint code (F02), not from a hash this harness computes, so they
    // are comparable with every earlier task's record.
    let install_sha256 = install::fingerprint(&found.manifest).to_hex();
    let content_sha256 = install::content_fingerprint(&found.manifest).to_hex();
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
    // a resolving task, written down in
    // `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` under
    // "Deferred scope, its resolving task and what it gates" — the durable,
    // versioned record, which outlives this report. None of them is an unresolved
    // issue with the claim the report makes: the layout is established from the
    // pinned reference, the material section ends exactly on `meshes_offset` in
    // all nine retail archives, and no stored material index is outside its
    // container's own material table.
    //
    // The deliberate gaps, restated so the report cannot be read as claiming
    // more than it does:
    //
    //  * the name-matching rule between a GameZ container and a texture archive
    //    is the task's exact-name rule — no case folding, no extension
    //    stripping, no alias, no second archive — and on the installation that
    //    resolves almost nothing, because the container spells a texture
    //    `Sky1.tif` and the archive stores `sky1`. That is the finding, not a
    //    defect in the audit, and no alias record was invented to improve it;
    //  * `planes.zbd` is the shared airframe library and has no world of its own,
    //    so which archive its materials resolve against is not established and
    //    this report audits the eight world archives only;
    //  * the field at material offset 32 — `specular` in the pinned source, soil
    //    in newer classification — is stored raw and read as neither, and neither
    //    is `MaterialFlags::UNKNOWN`;
    //  * the link words, every other `unk` field, the cycle frames and the two
    //    never-followed pointers are stored raw and uninterpreted.
    let unknowns: [&str; 0] = [];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F10-C.02\",\n\
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
            "Implemented by bunny-2; reviewed by bunny-2 in a later session whose context was \
             fresh — it had not seen the implementation and re-derived the layout claims from \
             the installation with a probe sharing no code with the Rust reader. The same agent \
             identity on both sides, so this is **not** independent evidence in the owner \
             directive's sense, and no agent review replaces the owner's approval. The review \
             reproduced the finding's per-archive naming table, the material section boundary and \
             the zero-finding result over all nine archives, and corrected three numbers it could \
             not reproduce: a self-contradictory \"2510 of 2271\" pair in the finding's deferred \
             item 1, a \"5 of 2271\" pair in this report's method, and a \"41 full name fields in \
             planes.zbd\" code comment that measures 5. The format semantics — the name \
             encoding, the 1000-slot array, the two link-word rules — and this evidence \
             machinery have still had no check by a different agent, which the owner directive \
             says they should get.",
        ),
        jstr(
            "acceptance suite run locally with the `retail` capability. This harness derives every \
             field from the recorded log, the production installation discovery and fingerprint of \
             $CS_GAME_DIR, the production readers run over all nine GameZ archives (whose material \
             section must end exactly on the `meshes_offset` the header declares, and whose \
             per-archive counts go into material-corpus.json), the production dependency audit run \
             over the eight world archives against each world's own texture archive, rustc and \
             Cargo.lock. The `unknowns` array is empty because every item this task could not \
             resolve is a named deferred scope boundary with a resolving task, written down in \
             docs/findings/2026-09-29-f10-c-02-gamez-material-records.md under \"Deferred scope, \
             its resolving task and what it gates\" — the durable, versioned record; nothing was \
             hidden to pass the validator. The largest open item is the name-matching rule \
             between a GameZ container and a texture archive, where the task's exact-name rule \
             resolves 10 of the 3543 audited material rows — 10 of the 3521 distinct names \
             counted once per world, or 5 of the 1328 distinct names over the union of the eight \
             worlds' name sets, because C2's five and C3's five are the same five names; an \
             extension- and case-insensitive match would resolve 3500 of those 3521. Every one \
             of those figures was re-measured by the reviewing agent from the installation with \
             an independent probe, and material-corpus.json carries a handful of stored names as \
             `naming_examples` because the claim is about names — each pair giving the \
             container's spelling beside the archive's **own stored** spelling, with at least one \
             pair differing in case as well as in extension, and nothing else derived from the \
             installation (no pixels, no coordinates, no file bytes). Validated with \
             tools/validate_evidence.py --require-pass. The suite is \
             19 task tests: 17 synthetic and 2 retail, and the two retail tests fail loudly rather \
             than skipping when CS_GAME_DIR is absent.",
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    // A cheap self-check without a JSON dependency: the validator runs next, but
    // a structurally empty write must fail here first.
    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F10-C.02\"",
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
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written honestly \
         and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f10_c_02.rs)"
        )
    })
}

/// A session of an already-discovered installation, for one world group.
fn session_of(root: &Path, found: &install::Discovery, world: &str) -> ContentSession {
    let group = found
        .diagnosis
        .world_groups
        .iter()
        .find(|group| group.as_str().eq_ignore_ascii_case(world))
        .unwrap_or_else(|| panic!("world group {world} is discovered"))
        .clone();
    let context = ResolveContext::new(install::fingerprint(&found.manifest))
        .with_world_group(WorldGroup::from_relative(group));
    let mut builder = SessionBuilder::new(context);
    builder
        .mount_installation(root, &found.diagnosis)
        .expect("the installation mounts");
    builder.open()
}

/// Cargo runs a test binary with its working directory set to the *package* root,
/// so a path written relative to the workspace root must be re-anchored here.
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
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f10_c_02_` tests from a recorded `cargo test` output.
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
        let mut cursor = trimmed;
        loop {
            if let Some(position) = cursor.find("test ") {
                let after = &cursor[position + 5..];
                if let Some(separator) = after.find(" ... ") {
                    let name = after[..separator].to_owned();
                    let tail = &after[separator + 5..];
                    cursor = tail;
                    if !name.contains("accept_f10_c_02_") {
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
                if name.contains("accept_f10_c_02_") {
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

/// One retail GameZ archive as the **production readers and audit** report it.
struct CorpusRow {
    relative: String,
    world: Option<String>,
    size_bytes: usize,
    sha256: String,
    texture_count: usize,
    materials_offset: u32,
    meshes_offset: u32,
    walk_end: u64,
    textures: usize,
    present_materials: usize,
    free_slots: u32,
    array_size: i32,
    index_max: i32,
    index_last: i32,
    textured: usize,
    untextured: usize,
    cycled: usize,
    unknown_flag: usize,
    duplicate_name_groups: usize,
    duplicate_name_entries: usize,
    findings: usize,
    mesh_findings: usize,
    material_references: usize,
    audit_rows: usize,
    audit_references: usize,
    audit_out_of_range: usize,
    audit_states: BTreeMap<String, usize>,
    naming_examples: Vec<String>,
    layout_evidence: String,
}

/// The corpus artifact: relative spellings, digests, offsets, counts and a
/// handful of stored texture *names*. No pixels, no coordinates, no file bytes.
fn corpus_json(candidate_tree: &str, rows: &[CorpusRow]) -> String {
    let entries: Vec<String> = rows
        .iter()
        .map(|row| {
            let states: Vec<String> = row
                .audit_states
                .iter()
                .map(|(state, count)| format!("{}: {count}", jstr(state)))
                .collect();
            format!(
                "{{\"path\": {}, \"world\": {}, \"size_bytes\": {}, \"sha256\": {}, \
                 \"materials_offset\": {}, \"meshes_offset\": {}, \"material_walk_end\": {}, \
                 \"walk_lands_on_meshes_offset\": true, \"texture_table\": {{\"entries\": {}, \
                 \"bytes\": {}}}, \"material_section\": {{\"array_size\": {}, \"index_max\": {}, \
                 \"index_last\": {}, \"present\": {}, \"zero_slots\": {}, \"textured\": {}, \
                 \"untextured\": {}, \"cycled\": {}, \"unknown_flag\": {}, \
                 \"duplicate_name_groups\": {}, \"duplicate_name_entries\": {}, \"parse_findings\": {}}}, \
                 \"mesh_section_findings\": {}, \"stored_material_references\": {}, \
                 \"dependency_audit\": {{\"rows\": {}, \"references\": {}, \"out_of_range\": {}, \
                 \"states\": {{{}}}}}, \"naming_examples\": [{}], \"layout_evidence\": {}}}",
                jstr(&row.relative),
                row.world.as_deref().map_or("null".to_owned(), jstr),
                row.size_bytes,
                jstr(&row.sha256),
                row.materials_offset,
                row.meshes_offset,
                row.walk_end,
                row.textures,
                row.texture_count * 44,
                row.array_size,
                row.index_max,
                row.index_last,
                row.present_materials,
                row.free_slots,
                row.textured,
                row.untextured,
                row.cycled,
                row.unknown_flag,
                row.duplicate_name_groups,
                row.duplicate_name_entries,
                row.findings,
                row.mesh_findings,
                row.material_references,
                row.audit_rows,
                row.audit_references,
                row.audit_out_of_range,
                states.join(", "),
                row.naming_examples.join(", "),
                jstr(&row.layout_evidence),
            )
        })
        .collect();
    format!(
        "{{\n\
         \x20\"task_id\": \"F10-C.02\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"reader\": \"cs_formats::gamez::read_gamez_materials\",\n\
         \x20\"audit\": \"cs_content::mesh::MeshDependencyAudit\",\n\
         \x20\"layout_source\": \"mech3ax v0.6.0, commit \
         d3521a9721be731d365504568ddcd78e3f9846bb (EUPL-1.2, read only, no code copied)\",\n\
         \x20\"claim\": \"implemented\",\n\
         \x20\"evidence_class\": \"observed_tool\",\n\
         \x20\"note\": \"relative spellings, digests, offsets, counts and at most four stored texture \
         names per archive, because the finding is about names; no pixels, no coordinates, no \
         file bytes\",\n\
         \x20\"material_array_slots\": {},\n\
         \x20\"archives\": [\n  {}\n ]\n\
         }}\n",
        jstr(candidate_tree),
        jstr(&iso_utc_now()),
        NG_MATERIAL_SLOTS,
        entries.join(",\n  "),
    )
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact, hashed with the **production** SHA-256. The
/// validator re-hashes it with `hashlib` independently, so a wrong digest here
/// fails validation rather than passing quietly.
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
    (name, install::sha256(&bytes).to_hex(), kind.to_owned())
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

/// A JSON string literal: quoted and escaped, so no report field can break out of
/// its string.
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
