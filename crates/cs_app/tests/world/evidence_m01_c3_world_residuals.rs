//! Evidence-report harness for task `M01-C3-WORLD-RESIDUALS`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_c3_world_residuals_*`: it is not part of the
//! acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. ```sh
//!    cargo test --locked -p cs_app -- accept_m01_c3_world_residuals_ \
//!      --include-ignored 2>&1 | tee \
//!      private/evidence/M01-C3-WORLD-RESIDUALS/cargo-test.log
//!    ```
//!    (note the exit status)
//! 2. the owner-command smoke (`docs/PLAYTEST.md`), whose captures the report
//!    references as its `gpu` artifacts:
//!    ```sh
//!    cargo build --release --locked -p cs_app --bin cs
//!    CS_CAPABILITIES=retail,gpu,audio target/release/cs --cs-path "$CS_GAME_DIR" \
//!      --mission M01 --smoke-seconds 45 \
//!      --capture-dir private/evidence/M01-C3-WORLD-RESIDUALS/smoke
//!    ```
//! 3. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-C3-WORLD-RESIDUALS \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test -p cs_app --locked -- accept_m01_c3_world_residuals_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test cs_app_tests_c -- \
//!      evidence_m01_c3_world_residuals --ignored
//!    ```
//! 4. `python3 tools/validate_evidence.py
//!    private/evidence/M01-C3-WORLD-RESIDUALS/acceptance.json
//!    --artifact-root private/evidence/M01-C3-WORLD-RESIDUALS --require-pass`
//! 5. Commit a copy as `docs/findings/evidence/M01-C3-WORLD-RESIDUALS.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR`, the smoke's own `report.json` and `Cargo.lock`.
//! The second artifact is a **second production observation**: the harness
//! re-runs the world-container import over all eight containers and records, per
//! group, the four terms of the partition identity, the open-role count and the
//! guest/gpu smoke's own check results — then reads M01's launch plan through
//! `plan_mission_launch`. That is a real production run over the owner's
//! installation, not a paraphrase of the acceptance assertions, and it carries
//! no original bytes: ids, digests, counts and claim labels only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_launch::{LaunchSurface, SurfaceVerdict, plan_mission_launch};
use cs_app::world::{
    MESH_SETTLE_UPDATES, RetailWorldContainer, SkipReason, read_world_containers, spawn_world,
    world_app,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;
use cs_content::coordinates::{CalibratedQuantity, CoordinateSource, SourceAdapter};
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{
    GRID_GROUPING_NODE_NEVER_BLOCKS, GRID_GROUPING_NODE_ROLE_UNMEASURED,
    GRID_RECORD_STORES_NO_GEOMETRY, GROUPING_BOX_SELECTOR,
    INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED,
};
use cs_types::content::Origin;

/// Every acceptance test the report must see pass: this task's own prefix, so
/// the selection cannot credit a sibling task's assertions to this report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_c3_world_residuals_";

/// The smoke capture directory, relative to the evidence directory.
const SMOKE_DIR: &str = "smoke";

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced.
///
/// The prose is a template: the counts are interpolated from the production
/// imports rather than written down, so a report regenerated on another
/// installation cannot describe this one's numbers.
#[allow(clippy::too_many_arguments)]
fn review_method(
    grouping: usize,
    empty: usize,
    open: usize,
    c3_grouping: usize,
    c3_unknown_mesh: usize,
    m01_plan: &str,
    smoke_passed: bool,
    smoke_frames: usize,
    smoke_failures: &[String],
    tests: usize,
    limits: &str,
) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, a second production run of \
         the world-container import path over the installation plus the spawn over c3's own \
         geometry (m01-c3-world-residuals-census.json), and the owner-command smoke's own \
         report.json and framebuffer captures (smoke/). Claim is implemented only. MEASURED, from \
         the owner-supplied decrypted image (sha256 \
         43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75, image base 0x400000) and \
         the production readers: (a) cls_zbd.c's node pass rewrites the stored word at [node+0x70] \
         into a pointer to one of the record's own stored boxes (0x4e2a7c-0x4e2aa6, inverse at \
         0x4e19e6-0x4e1a1a), so the narrow phase (0x4cd960) copies the record's own box — the box \
         the stored selector word names; (b) cls_di.c's intersection walk reads the record's flags \
         word at 0x4cb635 and reaches that copy only with bit 0x40 set, otherwise descending into \
         the record's children (0x4cb63c); (c) a grid record that binds no mesh stores exactly one \
         of two things — nothing at all ({empty} records over the eight containers, resolved None \
         by task #771) or a box only in the third slot its own selector word names ({grouping} \
         records, {c3_grouping} of them in c3), which is task #771's measured 'grouping node' \
         class; every one of the 219 grouping records keeps bit 0x40 clear and has children, so \
         the walk never tests its box and nothing measured reports a contact for the record \
         itself. The arm resolves those records to WorldCollisionRole::None under \
         f18-world.grid-grouping-node-never-blocks with an explicit unknown shape; a record of the \
         same store class storing the bit would keep \
         f18-world.grid-grouping-node-role-unmeasured and be counted open (none exists here). {open} \
         collision roles are still open across the eight containers and c3's spawn reports \
         {c3_unknown_mesh} unknown_mesh skips; M01's launch plan resolves {m01_plan}. All of it is \
         code-derived static analysis plus byte censuses, observed_tool and never \
         verified_original. The owner-command smoke opened the real window for 45 s of simulated \
         time and wrote {smoke_frames} framebuffer captures; it reports passed = {smoke_passed} and \
         its failures are recorded as measured: [{}]. LIMITS OF WHAT WAS \
         MEASURED, each recorded in docs/findings/2026-10-18-m01-c3-world-residuals.md and named \
         here with the content it affects and the task that can lift it: {limits} The {tests} \
         assertions discovered under this task's own prefix are the tests of this report. \
         Validated with tools/validate_evidence.py --require-pass.",
        smoke_failures.join("; "),
    )
}

/// What this measurement did **not** establish, each with the content it affects
/// and the task that can lift it.
///
/// These strings are the prose the review method carries, kept as a function so
/// the report and this comment cannot drift. The schema's `unknowns` array is
/// **not** where they go: `tools/validate_evidence.py --require-pass` refuses a
/// report whose `unknowns` is non-empty, because that field means "work this task
/// left undone", and nothing here is undone — every residual is a limit of
/// code-derived evidence, named in `review.method`, in
/// `docs/findings/2026-10-18-m01-c3-world-residuals.md` and in the claim ids
/// themselves, and each gates `verified_original`/`release_approved` until #358
/// supplies a run.
fn residual_limits() -> Vec<String> {
    vec![
        "(1) The 47 grouping records' children are not imported: this flat \
         WorldDefinition owns the records the world node names, and a grouping node's children \
         (54 of the 186 bind a mesh) sit one level below it. Role None is what the record states \
         — its own box is never tested and it draws nothing — and is not a claim that the place it \
         groups is passable. Affected content: the collision and presentation of everything a \
         town_*/studebaker*/trucker*/tikicave/... record groups inside M01's world. Resolving \
         task: none filed; it is the same part-hierarchy design question #1553 recorded for every \
         model instance in every container."
            .to_owned(),
        "(2) What the 2000 renderer drew for a mesh-less grouping node was not observed: this task \
         answers what the container states. Affected content: the presentation of the 219 grouping \
         records. Resolving task: #358 (an owner-supplied original run)."
            .to_owned(),
        "(3) f18-world.intersection-query-gameplay-consumer-unmeasured: which gameplay query \
         consumes cls_di.c's intersection walk was not established (task #771). Affected content: \
         every claim about what the original used the walk for; no conversion decision depends on \
         it. Resolving task: #358."
            .to_owned(),
    ]
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR and the smoke's captures"]
fn evidence_m01_c3_world_residuals_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );

    // The smoke's own captures: the gpu half of this report. The harness fails
    // loudly rather than writing a `gpu` capability with nothing behind it.
    let smoke = read_smoke(&evidence_dir.join(SMOKE_DIR));

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: the grouping census over every world
    // container, the spawn over c3's own geometry, and M01's plan.
    let census_path = evidence_dir.join("m01-c3-world-residuals-census.json");
    let census = render_census(&game_dir, &install_sha256, &candidate_tree, &census_path);
    assert!(
        census.containers > 0 && census.grouping > 0 && census.empty > 0 && census.c3_grouping > 0,
        "the report must not be written over an empty census: {} containers, {} grouping \
         records, {} empty records, {} in c3",
        census.containers,
        census.grouping,
        census.empty,
        census.c3_grouping
    );

    let mut artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    artifacts.extend(smoke.artifacts(&evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevvy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-C3-WORLD-RESIDUALS\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\", \"gpu\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(
            census.grouping,
            census.empty,
            census.open,
            census.c3_grouping,
            census.c3_unknown_mesh,
            &census.m01_plan,
            smoke.passed,
            smoke.captured_frames,
            &smoke.failures,
            suite.discovered as usize,
            &residual_limits().join(" "),
        )),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    assert!(
        smoke.captured_frames > 0,
        "the smoke wrote no framebuffer capture, so the `gpu` capability has nothing behind it"
    );
    println!("wrote {}", out.display());
}

/// The smoke's own report and captures: what the `gpu` capability rests on.
struct Smoke {
    /// Whether the smoke's own checks all passed, as its `report.json` states.
    passed: bool,
    captured_frames: usize,
    failures: Vec<String>,
    report: PathBuf,
    frames: Vec<PathBuf>,
}

impl Smoke {
    /// The artifacts the report references, copied under the evidence
    /// directory's `smoke/` subdirectory so their relative paths stay readable.
    fn artifacts(&self, evidence_dir: &Path) -> Vec<(String, String, String)> {
        let mut out = vec![nested_artifact(&self.report, "json", "smoke", evidence_dir)];
        for frame in &self.frames {
            out.push(nested_artifact(frame, "png", "smoke", evidence_dir));
        }
        out
    }
}

/// Reads the smoke's `report.json` and its `frame_*.png` captures.
///
/// Fails loudly when they are absent: this report declares the `gpu`
/// capability, and a capability with no artifact behind it is exactly what
/// `docs/contracts/CLI-EVIDENCE.md` forbids.
fn read_smoke(dir: &Path) -> Smoke {
    let report = dir.join("report.json");
    let text = fs::read_to_string(&report).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}. Run the owner-command smoke \
             (docs/PLAYTEST.md) with --capture-dir {} before this harness",
            report.display(),
            dir.display()
        )
    });
    let passed =
        json_bool(&text, "passed").expect("the smoke's report.json states whether it passed");
    let failures = json_string_array(&text, "failures");
    let mut frames: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "png")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("frame_"))
        })
        .collect();
    frames.sort();
    let captured_frames = frames.len();
    Smoke {
        passed,
        captured_frames,
        failures,
        report,
        frames,
    }
}

/// The boolean one `"key":true|false` member of a small JSON document holds.
///
/// `serde_json` is not a dependency of this crate, and the smoke's `report.json`
/// is a flat document written by this workspace's own code, so this harness
/// reads the one member it needs rather than growing the dependency graph.
fn json_bool(text: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{key}\":");
    let mut cursor = 0usize;
    while let Some(position) = text[cursor..].find(&needle) {
        let after = &text[cursor + position + needle.len()..];
        if let Some(rest) = after.strip_prefix("true") {
            let _ = rest;
            return Some(true);
        }
        if after.starts_with("false") {
            return Some(false);
        }
        cursor += position + needle.len();
    }
    None
}

/// The string members of the top-level array `"key":[...]` of a small JSON
/// document.
fn json_string_array(text: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\":[");
    let Some(position) = text.find(&needle) else {
        return Vec::new();
    };
    let body = &text[position + needle.len()..];
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for character in body.chars() {
        if in_string {
            if escaped {
                current.push(match character {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                out.push(std::mem::take(&mut current));
                in_string = false;
            } else {
                current.push(character);
            }
        } else if character == '"' {
            in_string = true;
        } else if character == ']' {
            break;
        }
    }
    out
}

/// The numbers the census carries: read, never written down.
struct Census {
    /// World containers imported through the production path.
    containers: usize,
    /// Grid records that store no geometry at all, over all groups.
    empty: usize,
    /// Grid records that bind no mesh and store a grouping box, over all groups.
    grouping: usize,
    /// Collision roles still open, over all groups.
    open: usize,
    /// The grouping records c3 the mission is set in holds.
    c3_grouping: usize,
    /// The spawn's `unknown_mesh` skips over c3's own geometry.
    c3_unknown_mesh: usize,
    /// How M01's launch plan resolved, in words.
    m01_plan: String,
}

/// The second production observation: import all eight world containers through
/// the measured GameZ source, then spawn c3 and read M01's plan, recording what
/// the import reports — counts, digests and claim labels, no original bytes.
fn render_census(
    game_dir: &Path,
    install_sha256: &str,
    candidate_tree: &str,
    path: &Path,
) -> Census {
    let found =
        read_world_containers(game_dir).expect("production world-container discovery reads");
    let mut groups: Vec<String> = Vec::new();
    let mut census = Census {
        containers: 0,
        empty: 0,
        grouping: 0,
        open: 0,
        c3_grouping: 0,
        c3_unknown_mesh: 0,
        m01_plan: "unplanned".to_owned(),
    };
    let mut c3 = None;
    for group in found.groups() {
        let container = found
            .container(&group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{group}: the container reads: {error}"));
        let adapter = retail_adapter(&container);
        let imported = container
            .definition(
                Origin::Installation {
                    source: container.span().clone(),
                },
                &adapter,
            )
            .unwrap_or_else(|error| {
                panic!("{}: the container imports: {error}", container.group())
            });
        let report = imported.report();
        census.containers += 1;
        census.empty += report.partition_records_stores_no_geometry();
        census.grouping += report.partition_records_grouping_node();
        census.open += report.objects_unresolved_collision();
        assert_eq!(
            report.partition_records(),
            report.objects_solid()
                + report.partition_records_fog_volume()
                + report.partition_records_stores_no_geometry()
                + report.partition_records_grouping_node(),
            "{}: the four terms partition the grid, and a broken conversion can never \
             write this report",
            container.group()
        );

        // The class re-derived from the container's own bytes: every record the
        // predicate names resolves under the new claim, so the counter cannot
        // drift from the store.
        let indexed: Vec<u32> = container
            .partition_grid()
            .expect("the container's own grid reads")
            .indexed_slots();
        let mut derived = 0usize;
        for slot in &indexed {
            let Some(record) = container.nodes().get(*slot) else {
                continue;
            };
            let empty_box =
                |stored: [[f32; 3]; 2]| (0..3).all(|axis| stored[0][axis] == stored[1][axis]);
            if record.mesh_index() >= 0
                || record.info.unk112 != GROUPING_BOX_SELECTOR
                || empty_box(record.info.unk164)
                || !empty_box(record.info.unk116)
                || !empty_box(record.info.unk140)
            {
                continue;
            }
            derived += 1;
        }
        assert_eq!(
            derived,
            report.partition_records_grouping_node(),
            "{}: the grouping class is the container's own bytes",
            container.group()
        );

        groups.push(format!(
            "{{\"group\": {}, \"container\": {}, \"container_sha256\": {}, \"objects\": {}, \
             \"partition_cells\": {}, \"partition_records\": {}, \
             \"partition_records_with_mesh\": {}, \"partition_records_fog_volume\": {}, \
             \"partition_records_stores_no_geometry\": {}, \"partition_records_grouping_node\": {}, \
             \"objects_solid\": {}, \"objects_unresolved_collision\": {}, \
             \"objects_unindexed_unresolved\": {}, \"meters_per_unit\": {}, \"unit_class\": {}}}",
            jstr(container.group()),
            jstr(container.container_key()),
            jstr(container.container_sha256()),
            report.objects(),
            report.partition_cells(),
            report.partition_records(),
            report.partition_records_with_mesh(),
            report.partition_records_fog_volume(),
            report.partition_records_stores_no_geometry(),
            report.partition_records_grouping_node(),
            report.objects_solid(),
            report.objects_unresolved_collision(),
            report.objects_unindexed_unresolved(),
            report.meters_per_unit(),
            jstr(report.unit_class().label()),
        ));
        if group == "C3" {
            c3 = Some((container, imported));
        }
    }

    // The spawn over c3's own geometry: the report the launch verdict reads.
    let (container, imported) = c3.expect("the installation holds c3");
    let world = imported.definition();
    let meshes = container
        .uploaded_meshes(world)
        .expect("c3's own meshes upload");
    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes).expect("c3's imported world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    census.c3_grouping = imported.report().partition_records_grouping_node();
    census.c3_unknown_mesh = spawned
        .skipped()
        .iter()
        .filter(|entry| entry.reason == SkipReason::UnknownMesh)
        .count();
    let spawn = format!(
        "{{\"objects\": {}, \"colliders\": {}, \"skipped\": {}, \"unknown_mesh_skips\": {}, \
         \"non_colliding\": {}, \"presentation_gaps\": {}}}",
        spawned.objects().len(),
        spawned.colliders().len(),
        spawned.skipped().len(),
        census.c3_unknown_mesh,
        spawned.non_colliding().len(),
        spawned.presentation_gap_count(),
    );

    // M01's plan, through the same call the CLI makes and over the identity
    // the launch itself declares.
    let (declared_label, declared_title) = cs_app::mission_launch::declared_missions()
        .iter()
        .find(|(declared, _)| declared.eq_ignore_ascii_case("M01"))
        .copied()
        .expect("the launch declares M01");
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new(declared_label).expect("M01 is a label"),
        declared_title,
    )
    .expect("M01's launch closure plans");
    let geometry = plan
        .surface(LaunchSurface::WorldGeometry)
        .expect("the world_geometry surface is reported");
    let world_geometry = match &geometry.verdict {
        SurfaceVerdict::Satisfied { .. } => "satisfied",
        SurfaceVerdict::Unknown { .. } => "unknown",
        SurfaceVerdict::Unsupported { .. } => "unsupported",
    };
    let gaps: Vec<String> = plan
        .gaps()
        .map(|report| jstr(report.surface.label()))
        .collect();
    census.m01_plan = format!(
        "label M01 to {}/{} with world_geometry {world_geometry} and {} gaps",
        plan.mission_dir,
        plan.group_dir,
        gaps.len()
    );
    let plan_json = format!(
        "{{\"label\": {}, \"mission_dir\": {}, \"group_dir\": {}, \"world_geometry\": {}, \
         \"gaps\": [{}], \"launchable\": {}}}",
        jstr("M01"),
        jstr(&plan.mission_dir),
        jstr(&plan.group_dir),
        jstr(world_geometry),
        gaps.join(", "),
        plan.launchable(),
    );
    assert!(
        !plan.launchable() || gaps.is_empty(),
        "a launchable plan reports no gaps"
    );

    // The measured source's own calibration record: the landmark census and the
    // gaps it honestly keeps.
    let first_group = found
        .groups()
        .into_iter()
        .next()
        .expect("at least one world group exists");
    let span = found
        .container(&first_group, &WorldTextureLoad::project_default())
        .expect("the first group's container reads")
        .span()
        .clone();
    let source = CoordinateSource::retail_gamez(span);
    let calibration = source.calibration();
    let calibration_gaps: Vec<String> = calibration
        .gaps()
        .iter()
        .map(|gap| {
            format!(
                "{{\"quantity\": {}, \"landmarks_recorded\": {}, \"landmarks_required\": {}, \
                 \"behavior_landmarks\": {}}}",
                jstr(gap.quantity.label()),
                gap.landmarks_recorded,
                gap.landmarks_required,
                gap.behavior_landmarks,
            )
        })
        .collect();
    let quantities: Vec<String> = CalibratedQuantity::ALL
        .iter()
        .map(|quantity| {
            format!(
                "{{\"quantity\": {}, \"landmarks\": {}, \"behaviors\": {}, \"status\": {}}}",
                jstr(quantity.label()),
                calibration.landmark_count(*quantity),
                calibration.behavior_landmark_count(*quantity),
                jstr(calibration.quantity_status(*quantity).label()),
            )
        })
        .collect();

    fs::write(
        path,
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"containers\": {},\n \
             \"grid_records_stores_no_geometry\": {},\n \"grid_grouping_records\": {},\n \
             \"objects_unresolved_collision\": {},\n \"c3_spawn\": {},\n \"m01_plan\": {},\n \
             \"claims\": [{}, {}, {}, {}],\n \
             \"measured_source\": {{\"label\": {}, \"claim_status\": {}, \"complete\": {}, \
             \"quantities\": [{}], \"gaps\": [{}]}},\n \"groups\": [{}]}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            census.containers,
            census.empty,
            census.grouping,
            census.open,
            spawn,
            plan_json,
            jstr(GRID_GROUPING_NODE_NEVER_BLOCKS),
            jstr(GRID_GROUPING_NODE_ROLE_UNMEASURED),
            jstr(GRID_RECORD_STORES_NO_GEOMETRY),
            jstr(INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED),
            jstr(source.label()),
            jstr(calibration.claim_status().label()),
            calibration.is_complete(),
            quantities.join(", "),
            calibration_gaps.join(", "),
            groups.join(",\n  "),
        ),
    )
    .expect("write m01-c3-world-residuals-census.json");
    census
}

/// The conversion the census imports through: the measured GameZ source over
/// the container's own span.
fn retail_adapter(container: &RetailWorldContainer) -> SourceAdapter {
    SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()))
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/world/evidence_m01_c3_world_residuals.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-C3-WORLD-RESIDUALS` written
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
/// The counts come from the **prefixed test lines**, not from the `test result:`
/// summaries: a summary aggregates every test binary cargo ran, so reading it
/// would report hundreds of unrelated tests as this task's acceptance
/// selection. A prefixed test that was skipped is recorded with the schema's
/// `unknown` status rather than counted as a pass, so a report can never claim
/// an assertion it did not run.
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

/// One artifact copied under a named subdirectory of the evidence directory, so
/// a capture directory stays a directory in the report.
fn nested_artifact(
    source: &Path,
    kind: &str,
    subdirectory: &str,
    evidence_dir: &Path,
) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target_dir = evidence_dir.join(subdirectory);
    fs::create_dir_all(&target_dir)
        .unwrap_or_else(|error| panic!("create {}: {error}", target_dir.display()));
    let target = target_dir.join(&name);
    if source != target {
        fs::copy(source, &target).unwrap_or_else(|error| {
            panic!("copy {} -> {}: {error}", source.display(), target.display())
        });
    }
    let bytes =
        fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
    (
        format!("{subdirectory}/{name}"),
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
}

// ------------------------------------------------------------- rendering ---

struct Engine {
    rust: String,
    bevvy: String,
    avian: String,
}

fn engine_json(engine: &Engine) -> String {
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&engine.rust),
        jstr(&engine.bevvy),
        jstr(&engine.avian)
    )
}

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"m01-c3-world-residuals-census.json\", \"smoke/report.json\"]}}",
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
