//! Evidence-report harness for task T452-REG-SILENT-EXPIRY (#2175) —
//! `docs/contracts/CLI-EVIDENCE.md`, `schemas/evidence.schema.json`. Not named
//! `accept_t452_silent_*`: it is not part of the acceptance suite and fails
//! loudly when its inputs are missing.
//!
//! 1. `cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t452_silent_ --include-ignored 2>&1 |
//!    tee private/evidence/T452-REG-SILENT-EXPIRY/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T452-REG-SILENT-EXPIRY \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t452_silent_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test cs_app_tests_a evidence_report_t452_silent_expiry -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T452-REG-SILENT-EXPIRY/acceptance.json
//!    --artifact-root private/evidence/T452-REG-SILENT-EXPIRY`
//!    (structurally: the report's `unknowns` carry the measured values the
//!    registration still cannot express for *other* ids, so `--require-pass`
//!    is not used — a limitation is never deleted to satisfy a validator,
//!    AGENTS.md "Evidence-bound tasks")
//! 4. Commit a copy as
//!    `docs/findings/evidence/T452-REG-SILENT-EXPIRY.json`.
//!
//! `original-consumption.json` is a second production observation: the
//! installation's catalogue registered, the `wep_13` component's lowered fuse,
//! and one session flight of it from a stationary and from a moving launcher —
//! the tick it was consumed at, the bound it reached and the zero detonations,
//! areas and damage. Identifiers, numbers and digests only: no original display
//! text and no script text.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId, OrdnanceLauncherBinding, OrdnanceOrder,
    OrdnanceSession, OrdnanceStep, OriginalRegistrationOutcome,
    read_and_register_measured_ordnance, register_measured_ordnance, step_ordnance_session,
};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::MountPoseBinding;
use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_sim::damage::{ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::{FuseRule, OrdnanceId, OrdnanceRegistry, StatusEffectTarget};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

const RETAIL_TESTS: [&str; 2] = [
    "accept_t452_silent_retail_the_measured_smoke_record_is_consumed_at_range",
    "accept_t452_silent_retail_the_kept_claims_are_not_touched",
];

/// Every id the measurement can express once this stage's fuse exists and the
/// tangler field has its own runtime (task T452-REG-TANGLER-RUNTIME), in member
/// order.
const EXPECTED_REGISTERED: [&str; 16] = [
    "wep_04", "wep_05", "wep_06", "wep_07", "wep_08", "wep_09", "wep_10", "wep_11", "wep_12",
    "wep_13", "wep_14", "wep_15", "wep_24", "wep_26", "wep_25", "wep_27",
];

/// The ids a measured value still keeps out of the runtime registry: none.
const EXPECTED_BLOCKED: [&str; 0] = [];

const SESSION: u64 = 93;
const PRODUCER: u32 = 94;
const DAMAGE_PRODUCER: u32 = 95;
const TICKS_PER_SECOND: u32 = 30;
const GENERATION: SceneGeneration = SceneGeneration(1);

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES=retail,gpu,audio); this report is derived from the recorded log plus a second production pass recorded in original-consumption.json. MEASURED (identifiers, numbers, counts and digests only, no original display text, no script text): the sixteen hardpoint components T452.01's rules classify are read from the owner's weapons.zrd by read_original_component_table, read_original_weapon_records, read_original_weapon_media, read_original_ammo_declarations and read_original_weapons_provenance and lowered by cs_app::ordnance::measured_ordnance_declaration; the end of flight this stage expresses is T452-R.02 section 3's static measurement of the projectile step at 0x5b01ec and 0x5b0318 - when neither LOCK_ON nor DETONATE_AT_RANGE is declared the step marks the projectile dead with no detonation - which FuseRule now carries as ExpiresAtRange with the record's own RANGE as its bound (the parser default 500.0 measured at 0x5ad80b for wep_13, which declares no RANGE). The acceptance suite shows a registered unguided area-denial record launched through step_ordnance_session and consumed at that bound with no detonation, no area and no damage, from a stationary launcher and, in fewer ticks, from a launcher flying along the launch line (the bound is a distance, not a clock); it also shows that a swept contact still detonates a range-bounded item, that a zero, negative or non-finite bound is refused by name, and that an AreaDenialEngine still may not be impact-fused. The registration outcome over the owner's installation is recorded per id: all sixteen ids register (wep_13 and wep_26 among them, the records the silent expiry alone held back once the lateral-remainder ramp landed; wep_12 once the tangler field got its own runtime in task T452-REG-TANGLER-RUNTIME) and none is blocked. NOT CLAIMED: any original-run behavior (retail is read access and no agent ran the original executable); that a consumed projectile is what the original's screen showed; what a collision does with these records (nothing measured says a contact cannot detonate them, so the runtime keeps the contact ahead of the range end). The kept claims T452-R.02 left on these records are untouched: the smoke emitter's unmeasured radius (t452_r02.limit.smoke_radius) is still named on wep_13, and the tangler's two unpinned parts (the force scalar's identity and the field origin) are named on wep_12. Nothing was defaulted to make an id register. The claim is implemented: a code and test pass awards nothing above that, and no agent review replaces the owner's human approval. Validated structurally with tools/validate_evidence.py - not with --require-pass, because the report's `unknowns` list carries every measured value the registration still cannot express, derived (never typed) from the production report's own blockers and kept claims. None is deleted to satisfy a validator.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t452_silent_expiry_writes_the_acceptance_report() {
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
        "CS_CANDIDATE_TREE must be the tree of the tested commit; a stale report cannot be \
         reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed >= 4 && suite.assertions.len() as u64 >= suite.passed,
        "the acceptance log was not understood: {suite:?}"
    );
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: T452-REG-SILENT-EXPIRY requires capability \
                     `retail`, run step 1 with --include-ignored and CS_GAME_DIR set"
                )
            });
        assert_eq!(status, "pass", "{retail_test}");
    }

    let manifest = install::discover(&game_dir)
        .unwrap_or_else(|error| panic!("the installation must be discoverable: {error:?}"))
        .manifest;
    let install_sha256 = fingerprint(&manifest).to_hex();
    let content_sha256 = content_fingerprint(&manifest).to_hex();

    // Second observation: the same production path, run again.
    let mut registry = OrdnanceRegistry::new();
    let report = read_and_register_measured_ordnance(&game_dir, &mut registry, TICKS_PER_SECOND)
        .expect("the installation's records read");
    assert_eq!(report.declared, 16, "sixteen declared ids");
    assert_eq!(report.registered_records(), EXPECTED_REGISTERED);
    assert_eq!(report.blocked_records(), EXPECTED_BLOCKED);

    let wep_13 = ordnance_id("wep_13");
    let component = registry
        .get(&wep_13)
        .expect("wep_13 registered as a flyable component");
    let projectile = component.as_projectile().expect("a projectile");
    assert_eq!(
        projectile.fuse(),
        FuseRule::ExpiresAtRange { range_m: 500.0 },
        "wep_13's bound is the parser's measured RANGE default"
    );

    let stationary = fly(Vec3::ZERO);
    let moving = fly(Vec3::new(0.0, 0.0, -100.0));
    assert!(
        moving.tick < stationary.tick,
        "the bound is a distance: {:?} vs {:?}",
        moving,
        stationary
    );

    let mut outcomes = Vec::new();
    for outcome in &report.outcomes {
        match outcome {
            OriginalRegistrationOutcome::Registered { record, family, .. } => {
                outcomes.push(format!(
                    "{{\"record\": {}, \"status\": \"registered\", \"family\": {}}}",
                    jstr(record),
                    jstr(family.label())
                ))
            }
            OriginalRegistrationOutcome::Blocked { record, blockers } => outcomes.push(format!(
                "{{\"record\": {}, \"status\": \"blocked\", \"blockers\": [{}]}}",
                jstr(record),
                blockers
                    .iter()
                    .map(|blocker| format!(
                        "{{\"field\": {}, \"claim\": {}, \"resolving_task\": {}}}",
                        jstr(blocker.field),
                        jstr(&blocker.claim),
                        jstr(blocker.resolving_task)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
    let observation = format!(
        "{{\"task\": \"T452-REG-SILENT-EXPIRY\", \"declared\": {}, \"registered\": {}, \
         \"blocked\": {}, \"wep_13_fuse\": {}, \"wep_13_lifetime_ticks\": {}, \
         \"flights\": [{{\"launcher_velocity_m_s\": {}, \"consumed_at_tick\": {}, \
         \"range_m\": {}, \"detonations\": {}, \"areas\": {}, \"damage_events\": {}}}, \
         {{\"launcher_velocity_m_s\": {}, \"consumed_at_tick\": {}, \
         \"range_m\": {}, \"detonations\": {}, \"areas\": {}, \"damage_events\": {}}}], \
         \"components\": [{}]}}\n",
        report.declared,
        report.registered_records().len(),
        report.blocked_records().len(),
        jstr(&projectile.fuse().label()),
        projectile.lifetime_ticks(),
        vec3_json(stationary.velocity),
        stationary.tick,
        stationary.range_m,
        stationary.detonations,
        stationary.areas,
        stationary.damage_events,
        vec3_json(moving.velocity),
        moving.tick,
        moving.range_m,
        moving.detonations,
        moving.areas,
        moving.damage_events,
        outcomes.join(", ")
    );
    let observation_path = evidence_dir.join("original-consumption.json");
    fs::write(&observation_path, &observation).expect("write original-consumption.json");
    // The artifact must be parseable JSON, not just hashable text.
    assert!(
        observation.starts_with("{\"task\"") && observation.trim_end().ends_with('}'),
        "the observation is one JSON object"
    );

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];
    let unknowns = unknowns_json(&report);
    let report_json = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T452-REG-SILENT-EXPIRY\",\n \
         \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \
         \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \
         \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \
         \"ticks\": {{\"start\": 0, \"end\": {}}},\n \"overrides\": [],\n \
         \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \
         \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \
         \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": {},\n \
         \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        stationary.tick,
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        unknowns,
        jstr(&reviewer),
        jstr(REVIEW_METHOD),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report_json).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// One session flight of `wep_13` from a launcher moving at `velocity`, over a
/// registry filled by the production registration path.
fn fly(velocity: Vec3) -> Flight {
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));
    let table = cs_content::ordnance::read_original_component_table(&game_dir).expect("table");
    let records = cs_content::ordnance::read_original_weapon_records(&game_dir).expect("records");
    let media = cs_content::ordnance::read_original_weapon_media(&game_dir).expect("media");
    let ammo = cs_content::ordnance::read_original_ammo_declarations(&game_dir).expect("ammo");
    let provenance = cs_content::ordnance::read_original_weapons_provenance(
        &game_dir,
        cs_app::ordnance::T452_ORIGINAL_WEAPONS_CLAIM,
    )
    .expect("provenance");
    let mut catalogue = OrdnanceRegistry::new();
    let registration = register_measured_ordnance(
        &table,
        &records,
        &media,
        &ammo,
        &provenance,
        &mut catalogue,
        TICKS_PER_SECOND,
    );
    assert!(registration.registered_records().contains(&"wep_13"));
    let declared = cs_app::ordnance::measured_ordnance_declaration(
        table
            .rows
            .iter()
            .find(|row| row.record == "wep_13")
            .expect("wep_13"),
        records
            .iter()
            .find(|record| record.record == "wep_13")
            .expect("wep_13 record"),
        media
            .iter()
            .find(|media| media.record == "wep_13")
            .expect("wep_13 media"),
        ammo.iter()
            .find(|ammo| ammo.record == "wep_13")
            .expect("wep_13 ammo"),
        &provenance,
        TICKS_PER_SECOND,
    )
    .expect("wep_13's measured values are expressible");

    let mut session = OrdnanceSession::new(
        SESSION,
        Tick(0),
        TickRate::new(TICKS_PER_SECOND).expect("rate"),
        PRODUCER,
        catalogue,
    )
    .expect("a nonzero session opens");
    session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured component registers");

    let mut world = World::new();
    launcher(&mut world, velocity);
    let mut damage = damage_resolver();
    let hull_before = damage
        .remaining_integrity(&actor(2), &key("hull"))
        .expect("the hull is registered");
    let launch = step_ordnance_session(
        &mut world,
        &mut session,
        &mut damage,
        &[OrdnanceOrder::Launch(OrdnanceLaunch {
            id: OrdnanceLaunchId {
                session: SESSION,
                tick: Tick(0),
                producer: PRODUCER,
                sequence: 0,
            },
            shooter: actor(1),
            ordnance: ordnance_id("wep_13"),
            target: None,
            engagement: OrdnanceEngagement {
                damage_target: actor(2),
                node: key("hull"),
                status_recipient: StatusEffectTarget::whole_actor(actor(2)),
            },
        })],
        &tick_step(0),
    );
    let projectile = launch.launched[0];
    let mut flight = Flight {
        velocity,
        ..Flight::default()
    };
    for at in 1..=200 {
        let recipients = [cs_sim::weapons::BlastRecipient::new(
            actor(2),
            key("hull"),
            WorldPosition::try_new([0.0, 0.0, -10.0]).expect("a finite position"),
        )];
        let tick = step_ordnance_session(
            &mut world,
            &mut session,
            &mut damage,
            &[],
            &OrdnanceStep {
                blast_recipients: Some(&recipients),
                ..tick_step(at)
            },
        );
        flight.detonations += tick.detonations.len() + tick.guidance_blasts.len();
        flight.areas += tick.areas_expired.len();
        if let Some(entry) = tick.consumed.first().copied() {
            assert_eq!(entry.projectile, projectile);
            flight.tick = at;
            flight.range_m = entry.range_m;
            assert!(
                session.runtime().get(&projectile).is_none(),
                "a consumed item leaves the live set"
            );
            flight.damage_events = usize::from(
                damage.remaining_integrity(&actor(2), &key("hull")) != Some(hull_before),
            );
            return flight;
        }
    }
    panic!("the item was never consumed");
}

/// One step's inputs at `at`, with no recipients unless a caller supplies them.
fn tick_step(at: u64) -> OrdnanceStep<'static> {
    OrdnanceStep {
        at: Tick(at),
        dt_s: 1.0 / f64::from(TICKS_PER_SECOND),
        wind_velocity_m_s: [0.0; 3],
        generation: GENERATION,
        guidance: None,
        targets: &[],
        impacts: &[],
        blast_recipients: None,
        victim_nodes: None,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Flight {
    velocity: Vec3,
    tick: u64,
    range_m: f64,
    detonations: usize,
    areas: usize,
    damage_events: usize,
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SessionId::new(SESSION).expect("nonzero"),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("a valid node key")
}

fn ordnance_id(record: &str) -> OrdnanceId {
    OrdnanceId::try_new(ContentId::from_source(ContentKind::Weapon, record).expect("a weapon id"))
        .expect("the weapon namespace")
}

fn damage_resolver() -> DamageResolver {
    let mut damage =
        DamageResolver::new(SessionId::new(SESSION).expect("nonzero"), DAMAGE_PRODUCER);
    damage
        .register_actor(
            actor(2),
            cs_sim::damage::synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the fixture graph registers");
    damage
}

fn launcher(world: &mut World, velocity: Vec3) {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![ContentId::from_source(ContentKind::Weapon, "wep_13").expect("id")],
                loadout: ContentId::from_source(ContentKind::Loadout, "fixture.loadout")
                    .expect("a loadout id"),
                generation: GENERATION,
            },
            LinearVelocity(velocity),
        ))
        .id();
    world.spawn((
        MountPoseBinding {
            mount: key("weapon_mount_13"),
            generation: GENERATION,
        },
        cs_app::scene::NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, 10.0)),
        ChildOf(root),
    ));
}

/// A `Vec3` as a JSON array of its three components.
fn vec3_json(value: Vec3) -> String {
    format!("[{}, {}, {}]", value.x, value.y, value.z)
}

/// Every value this stage could not express or could not carry, one
/// `"claim: content (resolving task)"` string per distinct claim, sorted.
///
/// Derived, never typed: the blockers and the kept claims come from the
/// production registration report, so a new limitation appears here the moment
/// the code records it, and none may be deleted to satisfy a validator.
fn unknowns_json(report: &cs_app::ordnance::OriginalRegistrationReport) -> String {
    let mut rows: Vec<(String, String)> = Vec::new();
    for blocker in report.blockers() {
        let text = format!(
            "{}: {} ({}) — resolving task {}",
            blocker.claim, blocker.field, blocker.affects, blocker.resolving_task
        );
        if !rows.iter().any(|(claim, _)| *claim == blocker.claim) {
            rows.push((blocker.claim.clone(), text));
        }
    }
    for (record, claims) in &report.claims_kept {
        for claim in claims {
            if rows.iter().any(|(seen, _)| seen == claim) {
                continue;
            }
            let text = match cs_app::ordnance::original_kept_claim_context(claim) {
                Some((affects, task)) => {
                    format!("{claim}: {affects} (kept on {record}) — resolving task {task}")
                }
                None => {
                    format!("{claim}: kept on {record}; no measurement-backed consumer names it")
                }
            };
            rows.push((claim.clone(), text));
        }
    }
    rows.sort();
    format!(
        "[{}]",
        rows.iter()
            .map(|(_, text)| jstr(text))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set: this harness only runs through the sequence in its module doc")
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/...` written relative to the
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

fn engine_json() -> String {
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&rustc_version()),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d"))
    )
}

// ------------------------------------------------------------ log parsing ---

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
/// `accept_t452_silent_` tests from a recorded `cargo test` output.
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
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            // The suite is a module of `tests/suite_a.rs`: drop the module path.
            let name = after[..separator]
                .rsplit("::")
                .next()
                .unwrap_or_default()
                .to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.starts_with("accept_t452_silent_") {
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

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 the task
/// implements (the validator re-hashes it with `hashlib` independently).
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

// --------------------------------------------------------------- encoding ---

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {}, \"evidence\": [\"cargo-test.log\", \
                 \"original-consumption.json\"]}}",
                jstr(name),
                jstr(status)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    artifacts
        .iter()
        .map(|(path, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {}, \"kind\": {}}}",
                jstr(path),
                jstr(digest),
                jstr(kind)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn str_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| jstr(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A JSON string literal.
fn jstr(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn iso_utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs();
    let days = seconds / 86_400;
    let remainder = seconds % 86_400;
    let (year, month, day) = civil_from_days(i64::try_from(days).expect("days fit"));
    let hour = remainder / 3600;
    let minute = (remainder % 3600) / 60;
    let second = remainder % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`, for a UTC date without a dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        u32::try_from(m).expect("month fits"),
        u32::try_from(d).expect("day fits"),
    )
}
