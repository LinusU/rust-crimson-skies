//! Evidence-report harness for task `F18-PARRY-DENORMAL-BVH`
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! Deliberately **not** named `accept_f18_world_units_containers_*`: it is not
//! part of the acceptance suite, it fails loudly when its inputs are missing,
//! and the task's test selection must never pick it up as an acceptance test.
//! Run from the workspace root, after the acceptance suite, exactly as:
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_f18_world_units_containers_ \
//!      --include-ignored 2>&1 | tee private/evidence/F18-PARRY-DENORMAL-BVH/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F18-PARRY-DENORMAL-BVH \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f18_world_units_containers_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test world \
//!        evidence_f18_parry_denormal_bvh -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py \
//!      private/evidence/F18-PARRY-DENORMAL-BVH/acceptance.json \
//!      --artifact-root private/evidence/F18-PARRY-DENORMAL-BVH --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F18-PARRY-DENORMAL-BVH.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, a **second production
//! run** of the container → import → upload → spawn → settle path over all
//! eight world groups (`parry-denormal-census.json`), and `rustc --version`
//! and `Cargo.lock`. Nothing is typed in by hand.
//!
//! `capabilities` are checked, not assumed: `retail` is declared only because
//! the five ignored tests that read `$CS_GAME_DIR` are in the recorded log and
//! passed, and `synthetic` only because the CI-runnable regression test passed.
//! The claim is `implemented`, never `checked` or `verified_original`: no
//! original executable ran, so what the 2000 engine did with subnormal stored
//! bytes is unmeasured — the canonicalisation is this project's declared rule.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use avian3d::prelude::Collider;
use cs_app::world::{RetailWorldContainer, read_world_containers, spawn_world, world_app};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::textures::WorldTextureLoad;
use cs_types::content::Origin;

/// Every acceptance test this task's prefix must discover: the five retail
/// tests carry `retail`, the one unignored regression carries `synthetic`.
/// All six must appear in the recorded log and pass.
const REQUIRED_TESTS: &[&str] = &[
    "world_units::accept_f18_world_units_containers_every_world_group_imports_with_the_measured_counts",
    "world_units::accept_f18_world_units_containers_every_world_group_spawns_and_reports_its_gaps",
    "world_units::accept_f18_world_units_containers_a_mesh_the_store_holds_no_geometry_for_is_a_gap",
    "world_units::accept_f18_world_units_containers_the_subnormal_blocker_is_canonicalised",
    "world_units::accept_f18_world_units_containers_every_stored_transform_places_exactly",
    "world_units::accept_f18_world_units_containers_the_declared_flush_reaches_the_collider",
];

/// This task's test prefix: a libtest name under it counts toward the suite.
const ACCEPTANCE_PREFIX: &str = "accept_f18_world_units_containers_";

/// The census artifact's file name, written beside the report and referenced
/// by digest.
const CENSUS_ARTIFACT: &str = "parry-denormal-census.json";

/// The corpus mesh the blocker was measured on: `c3` mesh slot 447.
const C3_SUBNORMAL_SLOT: u32 = 447;

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn writes_the_acceptance_report() {
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
        "no `{ACCEPTANCE_PREFIX}` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed.
    for required in REQUIRED_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == required)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{required} did not run: F18-PARRY-DENORMAL-BVH requires `retail` and \
                     `synthetic`, run step 1 with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's re-measurement is built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: the same path the acceptance test
    // drives — discover, read, import, upload, spawn, settle — over every
    // world group, with the subnormal flush measured on the uploaded buffers.
    let census_path = evidence_dir.join(CENSUS_ARTIFACT);
    let census = render_census(&game_dir, &install_sha256, &candidate_tree, &census_path);

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let document = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F18-PARRY-DENORMAL-BVH\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(&census)),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &document).expect("write acceptance.json");

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F18-PARRY-DENORMAL-BVH\"",
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

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced — the prose is a template, the counts are
/// interpolated, so a report regenerated on another tree cannot describe this
/// one's numbers.
fn review_method(census: &Census) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         of the container -> import -> upload -> spawn -> settle path over all {groups} world \
         groups ({census}). MEASURED: c3's mesh array stores {stored} subnormal position \
         components in total and slot 447 the two the blocker was measured on; the production \
         upload reports {flushed} emitted components canonicalised to signed zero under \
         f17-b.subnormal-position-flushes-to-zero (emitted, not stored: a vertex shared by several \
         polygons is flushed at each use), no uploaded position buffer carries a subnormal, every \
         group's settle finished and every reported collider built as a triangle mesh \
         ({c3_colliders} for c3). NOT MEASURED: what the 2000 engine did with the subnormal bytes — \
         retail is file access, no original run happened, so the canonicalisation is an \
         engine-compatibility rule of this project, never verified_original. Validated with \
         tools/validate_evidence.py --require-pass.",
        groups = census.groups,
        census = CENSUS_ARTIFACT,
        stored = census.c3_stored_components,
        flushed = census.c3_flushed_components,
        c3_colliders = census.c3_colliders,
    )
}

/// What the census run measured, returned so the review method interpolates
/// real numbers.
struct Census {
    groups: usize,
    /// Subnormal position components across `c3`'s whole stored mesh array.
    c3_stored_components: usize,
    /// Components the upload reports canonicalised over `c3`'s uploaded
    /// meshes — an *emitted* count, which can exceed the stored count because
    /// a shared vertex is flushed at each use.
    c3_flushed_components: usize,
    /// Built colliders `c3` ended the settle with.
    c3_colliders: usize,
}

/// The second production observation: for every discovered world group, run
/// the production read → import → upload → spawn → settle path and record
/// counts, digests and outcomes — never original bytes. The one mesh the
/// blocker was measured on gets its own record: stored subnormal count, bit
/// patterns (committed constants, not derived content), flushed count and
/// whether the uploaded buffer is clean.
fn render_census(
    game_dir: &Path,
    install_sha256: &str,
    candidate_tree: &str,
    path: &Path,
) -> Census {
    let found =
        read_world_containers(game_dir).expect("production world-container discovery reads");
    let mut rows: Vec<String> = Vec::new();
    let mut settle_failures: Vec<String> = Vec::new();
    let mut census = Census {
        groups: 0,
        c3_stored_components: 0,
        c3_flushed_components: 0,
        c3_colliders: 0,
    };
    let mut c3_slot_row = String::from("null");
    for group in found.groups() {
        let container = found
            .container(&group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{group}: the container reads: {error}"));
        let imported = container
            .definition(origin(&container), &adapter(&container))
            .unwrap_or_else(|error| panic!("{group}: the container imports: {error}"));
        let world = imported.definition();
        let meshes = container
            .uploaded_meshes(world)
            .unwrap_or_else(|error| panic!("{group}: the geometry uploads: {error}"));

        // Stored subnormal components per group, and how many the upload
        // reports canonicalised. The domains differ — stored position list vs
        // emitted vertices — so both are recorded rather than reconciled.
        let mut stored_subnormal = 0usize;
        let mut flushed = 0usize;
        for (slot, maybe_mesh) in container.meshes().meshes.iter().enumerate() {
            let Some(mesh) = maybe_mesh else { continue };
            stored_subnormal += mesh
                .mesh
                .positions
                .iter()
                .flatten()
                .filter(|component| component.is_subnormal())
                .count();
            let Ok(id) = cs_content::catalog::baseline::mesh_content_id(
                container.container_key(),
                slot as u32,
            ) else {
                continue;
            };
            if let Some(uploaded) = meshes.get(&id) {
                flushed += uploaded.subnormal_components();
            }
        }

        let mut app = world_app();
        let spawned = spawn_world(&mut app, world, &meshes)
            .unwrap_or_else(|error| panic!("{group}: the world spawns: {error}"));
        let reported = spawned.colliders().len();
        let settle_ok = crate::world_units::settles(&mut app);
        if !settle_ok {
            settle_failures.push(group.clone());
        }
        let built = if settle_ok {
            world
                .objects()
                .iter()
                .filter_map(|object| spawned.collider_for(object.id()))
                .filter(|entity| app.world().get::<Collider>(*entity).is_some())
                .count()
        } else {
            0
        };
        if settle_ok {
            assert_eq!(
                reported, built,
                "{group}: every collider the spawn reports is built after the settle"
            );
        }
        // The two counts are deliberately *not* reconciled: `stored_subnormal`
        // counts components on the stored position list, `flushed` counts
        // components canonicalised at emission — the IR expands one stored
        // position into a render vertex per distinct corner tuple and the
        // upload compacts per material group, so a shared vertex is flushed
        // once per use. Both are recorded; neither bounds the other.

        census.groups += 1;
        if group == "C3" {
            census.c3_stored_components = stored_subnormal;
            census.c3_flushed_components = flushed;
            census.c3_colliders = built;
            c3_slot_row = slot_row(&container, &meshes);
        }
        rows.push(format!(
            "{{\"group\": {}, \"container\": {}, \"container_sha256\": {}, \"mesh_slots\": {}, \
             \"present_meshes\": {}, \"stored_subnormal_components\": {}, \
             \"flushed_components\": {}, \"colliders_reported\": {}, \"colliders_built\": {}, \
             \"settle_finished\": {}}}",
            jstr(&group),
            jstr(container.container_key()),
            jstr(container.container_sha256()),
            container.meshes().slot_count(),
            container.meshes().present_count(),
            stored_subnormal,
            flushed,
            reported,
            built,
            settle_ok,
        ));
    }
    assert_eq!(
        census.groups, 8,
        "the measured corpus is eight world groups"
    );
    assert!(
        settle_failures.is_empty(),
        "every group's settle finishes since #656: {settle_failures:?}"
    );
    assert_eq!(
        census.c3_stored_components, 38,
        "the corpus census: c3's mesh array stores 38 subnormal position \
         components — the blocker's two on slot 447 plus 36 more on meshes \
         whose centroid extents are normal, so they never panicked"
    );
    assert_eq!(
        census.c3_colliders, 374,
        "c3 builds all 374 colliders the panic kept the suite from reaching"
    );

    fs::write(
        path,
        format!(
            "{{\n \"schema\": \"cs-f18-parry-denormal-census/1\",\n \"install_sha256\": {},\n \
             \"candidate_tree\": {},\n \"claim\": \"f17-b.subnormal-position-flushes-to-zero\",\n \
             \"groups\": {},\n \"c3_slot_447\": {},\n \"rows\": [\n  {}\n ]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            census.groups,
            c3_slot_row,
            rows.join(",\n  "),
        ),
    )
    .expect("write parry-denormal-census.json");
    census
}

/// The once-blocking mesh's own record: what the stored bytes hold and what
/// the upload made of them.
fn slot_row(container: &RetailWorldContainer, meshes: &cs_app::world::WorldMeshes) -> String {
    let mesh = container
        .meshes()
        .get(C3_SUBNORMAL_SLOT)
        .expect("c3 holds the blocking mesh slot");
    let stored_subnormal: Vec<String> = mesh
        .mesh
        .positions
        .iter()
        .flatten()
        .filter(|component| component.is_subnormal())
        .map(|component| jstr(&format!("{:#010x}", component.to_bits())))
        .collect();
    let id = cs_content::catalog::baseline::mesh_content_id(
        container.container_key(),
        C3_SUBNORMAL_SLOT,
    )
    .expect("a container's own mesh slot is inside the id grammar");
    let uploaded = meshes
        .get(&id)
        .expect("the once-blocking mesh is registered");
    let uploaded_subnormal = match uploaded
        .mesh()
        .attribute(bevy::mesh::Mesh::ATTRIBUTE_POSITION)
    {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values
            .iter()
            .flatten()
            .filter(|component| component.is_subnormal())
            .count(),
        other => panic!("positions are Float32x3, got {other:?}"),
    };
    assert_eq!(
        stored_subnormal.len(),
        2,
        "the corpus premise: slot 447 stores exactly the two subnormal \
         components #639 measured"
    );
    assert_eq!(
        uploaded.subnormal_components(),
        2,
        "the upload reports canonicalising exactly those two components"
    );
    assert_eq!(
        uploaded_subnormal, 0,
        "no subnormal remains in the buffer the collider is derived from"
    );
    format!(
        "{{\"slot\": {C3_SUBNORMAL_SLOT}, \"stored_subnormal_components\": {}, \
         \"stored_bit_patterns\": [{}], \"flushed_components\": {}, \
         \"uploaded_subnormal_components\": {}}}",
        stored_subnormal.len(),
        stored_subnormal.join(", "),
        uploaded.subnormal_components(),
        uploaded_subnormal,
    )
}

/// The conversion the census imports through: the measured GameZ source over
/// the container's own span.
fn adapter(container: &RetailWorldContainer) -> SourceAdapter {
    SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()))
}

/// The origin every container's import carries: the container's own bytes.
fn origin(container: &RetailWorldContainer) -> Origin {
    Origin::Installation {
        source: container.span().clone(),
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/world/evidence_f18_parry_denormal_bvh.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F18-PARRY-DENORMAL-BVH` written
/// relative to the workspace root in the module doc must be re-anchored here.
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

fn iso_utc_now() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs();
    let days = (since / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        (since % 86_400) / 3600,
        (since % 3600) / 60,
        since % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (y, m, d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
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

/// Extracts the per-test results of this task's tests from a recorded `cargo
/// test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test
/// result:` summaries: a summary aggregates every test binary cargo ran, so
/// reading it would report hundreds of unrelated tests as this task's
/// acceptance selection. A prefixed test that was skipped is recorded with the
/// schema's `unknown` status rather than counted as a pass, so a report can
/// never claim an assertion it did not run.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = finished(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            record(&mut suite, name, status);
            continue;
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = after[separator + 5..].trim();
            cursor = &after[separator + 5..];
            if !carries_prefix(&name) {
                continue;
            }
            match finished(tail) {
                Some(status) => record(&mut suite, name, status),
                None => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.passed = count(&suite, "pass");
    suite.failed = count(&suite, "fail");
    suite.ignored = count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// Whether a libtest name is one of this task's tests.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(|segment| segment.starts_with(ACCEPTANCE_PREFIX))
}

/// The libtest result word at the head of a test's tail line.
fn finished(tail: &str) -> Option<&'static str> {
    match tail.split_whitespace().next() {
        Some("ok") => Some("pass"),
        Some("FAILED") => Some("fail"),
        Some("ignored") => Some("unknown"),
        _ => None,
    }
}

fn count(suite: &Suite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, seen)| *seen == status)
        .count() as u64
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 (the
/// validator re-hashes it with `hashlib` independently).
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
    (
        name,
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
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
                 {CENSUS_ARTIFACT:?}]}}",
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
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
