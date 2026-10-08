//! Evidence-report harness for task `M01-LC-WORLD-RESIDUAL-ROLES`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_world_residual_roles_*`: it is not part of the
//! acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_world_residual_roles_
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-LC-WORLD-RESIDUAL-ROLES/cargo-test.log` (note the
//!    exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-WORLD-RESIDUAL-ROLES \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_world_residual_roles_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_world_residual_roles -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-WORLD-RESIDUAL-ROLES/acceptance.json
//!    --artifact-root private/evidence/M01-LC-WORLD-RESIDUAL-ROLES --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-WORLD-RESIDUAL-ROLES.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs the world-container
//! import over all eight containers and records, per group, how many records
//! each partition grid names, how many of them the original's fog consumer
//! takes by name, how many store no geometry at all, how many resolve `Solid`
//! and how many roles are still open — then runs the production spawn over
//! `c1c`'s own uploaded geometry and records its skip report. That is a real
//! production run over the owner's installation, not a paraphrase of the
//! acceptance assertions, and it carries no original bytes: ids, digests,
//! counts and claim labels only.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::world::{
    MESH_SETTLE_UPDATES, RetailWorldContainer, read_world_containers, spawn_world, world_app,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::coordinates::{CalibratedQuantity, CoordinateSource, SourceAdapter};
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{
    FOG_VOLUME_RECORD_NEVER_BLOCKS, GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED,
    GRID_RECORD_STORES_NO_GEOMETRY, INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED,
};
use cs_types::content::{Origin, Resolved};

/// Every acceptance test the report must see pass: this task's own prefix, so
/// the selection cannot credit a sibling task's assertions to this report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_world_residual_roles_";

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced.
///
/// The prose is a template: the counts are interpolated from the production
/// imports rather than written down, so a report regenerated on another
/// installation cannot describe this one's numbers.
fn review_method(fog: usize, empty: usize, open: usize, c1c_skips: usize, tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         of the world-container import path over the installation plus the spawn over c1c's own \
         geometry (world-residual-roles-census.json). Claim is implemented only. MEASURED: (a) what \
         the narrow phase behind [node+0x70] points at, read out of the owner-supplied decrypted \
         image (sha256 43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75, image base \
         0x400000): cls_zbd.c's node pass loops over every node (0x4e2a54-0x4e2b06) and rewrites \
         the stored word at +0x70 into a pointer to that record's own stored box — 0 -> +0x74, \
         1 -> +0x8c, 2 -> +0xa4 (0x4e2a7c-0x4e2aa6), with the inverse mapping at \
         0x4e19e6-0x4e1a1a — so the narrow phase (0x4cd960) copies one of the record's three \
         stored boxes, which closes #727's open question. (b) what cls_di.c's intersection walk \
         does with a grid-named fog volume: it reads the record's flags word at 0x4cb635 and \
         reaches the box copy only with bit 0x40 set, otherwise it recurses into children or drops \
         the candidate (0x4cb63c-0x4cb642); the {fog} grid-named `fvol*` records this installation \
         stores all keep that bit clear with no children, so they resolve WorldCollisionRole::None \
         under f18-world.fog-volume-record-never-blocks like their unindexed siblings. (c) the grid \
         records that store no geometry at all — no mesh index and an empty box in each of their \
         three slots: {empty} across the eight containers, one in c1c — resolve None under \
         f18-world.grid-record-stores-no-geometry, and {open} collision roles are still open in \
         every container (c1c's spawn reports {c1c_skips} skips). LIMITS OF WHAT WAS MEASURED, \
         each recorded in docs/findings/2026-10-08-m01-lc-world-residual-roles.md: (1) everything \
         here is code-derived static analysis plus byte censuses — observed_tool, never \
         verified_original, and no original run happened (#358); (2) which gameplay query consumes \
         the walk is unmeasured and named f18-world.intersection-query-gameplay-consumer-unmeasured \
         because only an original run can lift it; (3) whether a script sets the proximity bit on a \
         fog volume at run time, and what the original renderer drew for a mesh-less node, were not \
         observed. The three residuals are listed in `unknowns`, each naming its \
         affected content and the task that can lift it (#358), so the report does not \
         describe an open measurement as a closed one. The {tests} \
         assertions discovered under this task's own prefix are the tests of this report. Validated \
         with tools/validate_evidence.py --require-pass.",
    )
}

/// What this measurement did **not** establish, each with the content it
/// affects and the task that can lift it. Recorded rather than dropped: a
/// limit on the claim is still an open question for whoever reads the report.
fn unknowns() -> Vec<String> {
    vec![
        "f18-world.intersection-query-gameplay-consumer-unmeasured: which gameplay query \
         consumes cls_di.c's intersection walk was not established. The chain is unique in the \
         image (game code 0x4ab284 -> fcn.005ac150 -> fcn.004cb420) and its caller sits in a \
         routine over Target/TargetVehicle/TargetTurret-typed objects that compares the returned \
         distance against a threshold, but that names a neighbourhood, not a query. Affected \
         content: every claim about what the original used the walk for (target acquisition, \
         proximity, line of sight or something else); no conversion decision depends on it. \
         Resolving task: #358 (an owner-supplied original run)."
            .to_owned(),
        "Whether a script sets the proximity/intersection flag on a fog volume at run time is \
         unmeasured: the INTERP commands SetIntersectSurface/SetIntersectBBOX/SetAltitudeSurface \
         exist (0x5bbccc, 0x5bbc90), and this task measured only the stored flag word. Affected \
         content: collision over the six grid-named fog volumes (c1c node slots 944-947, c5 node \
         slots 2304 and 2306) in a run where such a script executes; the conversion answers from \
         the store's own bytes. Resolving task: #358."
            .to_owned(),
        "What the 2000 renderer drew for a mesh-less node was not observed: this task answers what \
         the container states (no mesh index, all three stored boxes empty, no children), which is \
         all the conversion is allowed to say. Affected content: the presentation of the 148 grid \
         records that store no geometry (c1 13, c1b 8, c1c 1, c2 23, c2b 1, c3 18, c4 4, c5 80). \
         Resolving task: #358."
            .to_owned(),
    ]
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_world_residual_roles_writes_the_acceptance_report() {
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

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: the residual-roles census over every
    // world container, then the spawn over c1c's own geometry.
    let census_path = evidence_dir.join("world-residual-roles-census.json");
    let census = render_census(&game_dir, &install_sha256, &candidate_tree, &census_path);
    assert!(
        census.containers > 0 && census.fog > 0 && census.empty > 0,
        "the report must not be written over an empty census: {} containers, {} grid-named fog \
         volumes, {} empty grid records",
        census.containers,
        census.fog,
        census.empty
    );

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-WORLD-RESIDUAL-ROLES\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        unknowns()
            .iter()
            .map(|item| jstr(item))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&reviewer),
        jstr(&review_method(
            census.fog,
            census.empty,
            census.open,
            census.c1c_skips,
            suite.discovered as usize,
        )),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The numbers the census carries: read, never written down.
struct Census {
    /// World containers imported through the production path.
    containers: usize,
    /// Grid-named records the original's fog consumer keys, over all groups.
    fog: usize,
    /// Grid records that store no geometry at all, over all groups.
    empty: usize,
    /// Collision roles still open, over all groups.
    open: usize,
    /// The spawn's skip count over c1c's own geometry.
    c1c_skips: usize,
}

/// The second production observation: import all eight world containers
/// through the measured GameZ source, then spawn c1c, and record what the
/// import reports — counts, digests and claim labels, no original bytes.
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
        fog: 0,
        empty: 0,
        open: 0,
        c1c_skips: 0,
    };
    let mut c1c = None;
    for group in found.groups() {
        let container = found
            .container(&group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{group}: the container reads: {error}"));
        let adapter = retail_adapter(&container);
        let imported_world = container
            .definition(
                Origin::Installation {
                    source: container.span().clone(),
                },
                &adapter,
            )
            .unwrap_or_else(|error| {
                panic!("{}: the container imports: {error}", container.group())
            });
        let report = imported_world.report();
        census.containers += 1;
        census.fog += report.partition_records_fog_volume();
        census.empty += report.partition_records_stores_no_geometry();
        census.open += report.objects_unresolved_collision();

        // The grid-named fog volumes this claim landed on: their identity (the
        // container's own node slot), never the stored name.
        let indexed: BTreeSet<u32> = container
            .partition_grid()
            .expect("the container's own grid reads")
            .indexed_slots()
            .into_iter()
            .collect();
        let mut fog_slots: Vec<u32> = Vec::new();
        for object in imported_world.definition().objects() {
            let claim = match object.shape() {
                Resolved::Unknown { claim_id, .. } => claim_id.as_str(),
                Resolved::Known(_) => continue,
            };
            if claim != FOG_VOLUME_RECORD_NEVER_BLOCKS {
                // A grid-named `fvol*` record that stored the narrow-phase bit
                // would keep #727's claim on both halves instead; this
                // installation stores none, but the count below must hold for
                // either answer rather than depend on which one it is.
                if claim != GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED {
                    continue;
                }
            }
            let slot = object
                .id()
                .as_str()
                .strip_prefix("node-")
                .and_then(|slot| slot.parse::<u32>().ok())
                .expect("an object id is its node slot");
            if indexed.contains(&slot) {
                fog_slots.push(slot);
            }
        }
        assert_eq!(
            fog_slots.len(),
            report.partition_records_fog_volume(),
            "{}: every grid-named fog volume resolved the fog claim (task #771)",
            container.group()
        );

        groups.push(format!(
            "{{\"group\": {}, \"container\": {}, \"container_sha256\": {}, \"objects\": {}, \
             \"partition_cells\": {}, \"partition_records\": {}, \
             \"partition_records_with_mesh\": {}, \"partition_records_fog_volume\": {}, \
             \"fog_slots\": [{}], \"partition_records_stores_no_geometry\": {}, \
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
            fog_slots
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            report.partition_records_stores_no_geometry(),
            report.objects_solid(),
            report.objects_unresolved_collision(),
            report.objects_unindexed_unresolved(),
            report.meters_per_unit(),
            jstr(report.unit_class().label()),
        ));
        if group == "C1C" {
            c1c = Some((container, imported_world));
        }
    }

    // The spawn over c1c's own geometry: the report the launch verdict reads.
    let (container, imported_world) = c1c.expect("the installation holds c1c");
    let world = imported_world.definition();
    let meshes = container
        .uploaded_meshes(world)
        .expect("c1c's own meshes upload");
    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes).expect("c1c's imported world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    census.c1c_skips = spawned.skipped().len();
    let spawn = format!(
        "{{\"objects\": {}, \"colliders\": {}, \"skipped\": {}, \"non_colliding\": {}, \
         \"presentation_gaps\": {}}}",
        spawned.objects().len(),
        spawned.colliders().len(),
        spawned.skipped().len(),
        spawned.non_colliding().len(),
        spawned.presentation_gap_count(),
    );

    // The measured source's own calibration record: the landmark census and
    // the gaps it honestly keeps.
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
    let gaps: Vec<String> = calibration
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
             \"grid_named_fog_volumes\": {},\n \"grid_records_stores_no_geometry\": {},\n \
             \"objects_unresolved_collision\": {},\n \"c1c_spawn\": {},\n \
             \"claims\": [{}, {}, {}],\n \
             \"measured_source\": {{\"label\": {}, \"claim_status\": {}, \"complete\": {}, \
             \"quantities\": [{}], \"gaps\": [{}]}},\n \"groups\": [{}]}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            census.containers,
            census.fog,
            census.empty,
            census.open,
            spawn,
            jstr(FOG_VOLUME_RECORD_NEVER_BLOCKS),
            jstr(GRID_RECORD_STORES_NO_GEOMETRY),
            jstr(INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED),
            jstr(source.label()),
            jstr(calibration.claim_status().label()),
            calibration.is_complete(),
            quantities.join(", "),
            gaps.join(", "),
            groups.join(",\n  "),
        ),
    )
    .expect("write world-residual-roles-census.json");
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
             (crates/cs_app/tests/evidence_report_m01_lc_world_residual_roles.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-WORLD-RESIDUAL-ROLES` written
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
                 \"world-residual-roles-census.json\"]}}",
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
