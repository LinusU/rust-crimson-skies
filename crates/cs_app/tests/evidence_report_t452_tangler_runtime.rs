//! Evidence-report harness for task T452-REG-TANGLER-RUNTIME (#2186) —
//! `docs/contracts/CLI-EVIDENCE.md`, `schemas/evidence.schema.json`. Not named
//! `accept_t452_tangler_*`: it is not part of the acceptance suite and fails
//! loudly when its inputs are missing.
//!
//! 1. `cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t452_tangler_ --include-ignored 2>&1 |
//!    tee private/evidence/T452-REG-TANGLER-RUNTIME/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T452-REG-TANGLER-RUNTIME \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t452_tangler_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test cs_app_tests_a evidence_report_t452_tangler_runtime -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T452-REG-TANGLER-RUNTIME/acceptance.json
//!    --artifact-root private/evidence/T452-REG-TANGLER-RUNTIME`
//!    (structurally: the report keeps every unpinned part in `unknowns`, so
//!    `--require-pass` is not used — a limitation is never deleted to satisfy
//!    a validator, AGENTS.md "Evidence-bound tasks")
//! 4. Commit a copy as `docs/findings/evidence/T452-REG-TANGLER-RUNTIME.json`.
//!
//! `original-field.json` is a second production observation: the owner's own
//! `wep_12` registered through the production path, its measured field opened
//! by a real burst (a swept contact) through `step_ordnance_session`, and what
//! that field did per tick. Identifiers, numbers and digests only: no original
//! display text and no script text.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId, OrdnanceLauncherBinding, OrdnanceOrder,
    OrdnanceSession, OrdnanceStep, read_and_register_measured_ordnance, register_measured_ordnance,
    step_ordnance_session,
};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::MountPoseBinding;
use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_sim::damage::{ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::{OrdnanceId, OrdnanceRegistry, StatusEffectTarget, SweptHit, TargetPath};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

const RETAIL_TESTS: [&str; 1] =
    ["accept_t452_tangler_runtime_retail_the_measured_wep_12_opens_its_field"];

/// The id this stage gives a runtime field to. Every other record registers
/// with the values it already had.
const TANGLER_RECORD: &str = "wep_12";

const SESSION: u64 = 99;
const PRODUCER: u32 = 100;
const DAMAGE_PRODUCER: u32 = 101;
const TICKS_PER_SECOND: u32 = 30;
const GENERATION: SceneGeneration = SceneGeneration(1);

/// The mount pose the fire path reads. The field is centred on the point of
/// contact when it bursts, not on this pose, so the two must stay apart.
const MOUNT_ORIGIN: [f32; 3] = [3.0, 4.0, 10.0];

/// The tick a swept contact bursts the projectile: the launch tick plus two
/// advances, so the item's position has moved twice and the third advance (this
/// one) is the swept segment the contact happens on.
const BURST_TICK: u64 = 2;

/// Where in that segment the contact happens: half a tick.
const CONTACT_TIME: f64 = 0.5;

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES=retail,gpu,audio); this report is derived from the recorded log plus a second production pass recorded in original-field.json. MEASURED (identifiers, numbers, counts and digests only, no original display text, no script text): the owner's weapons.zrd is read by the production readers (read_original_component_table, read_original_weapon_records, read_original_weapon_media, read_original_ammo_declarations, read_original_weapons_provenance), every declared id is lowered by cs_app::ordnance::measured_ordnance_declaration and inserted by register_measured_ordnance, and this stage's record wep_12 now registers instead of being blocked. What was unexpressible before this stage was the TANGLER effect's runtime consequence, and it is measured: docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md section 1 measured that a TANGLER record installs a tick callback (0x4ba877), that the fire path builds the field object at 0x4b94e0 with the record's RADIUS (image default 10.0 m at 0x4ba870; no retail wep_ record states RADIUS) and TIME (weP_12 2 s), that its tick 0x4b9590 subtracts the frame time and, while positive, walks the actor list and calls the weapon's per-hit routine for every actor whose squared distance is below RADIUS squared, the owner included, and removes the field at zero; docs/findings/2026-10-28-t452-tangler-effect.md measured what that hit does: the per-hit routine's TANGLER branch (0x4b9ded) zeroes both damage channels and applies a tangle of duration max(2.0 s, 10.0 * (1 - exposure / RADIUS)) with the constants at 0x6032dc/0x62b124/0x62b120, that a re-hit raises the timer at +0x2e0 and never shortens it, that three actor updates count it down and clear the flag, that the force routine 0x48fc40 stores 0.0 into its local force scalar [ebp-8] while the flag is set, and that the boost setter 0x4b2110 refuses to enable while the flag is set (a tangle ends a boost). The runtime expression this report records: the field opens when the projectile bursts, centred on the point of contact for a swept hit (the hit record's position 0x5b05c0 writes into rocket+0x48 before the burst hook 0x4ba660 reads it, interpolated along the item's swept segment by its time of impact; tasks T452-REG-TANGLER-ORIGIN, T452-REG-TANGLER-BURST-OPEN and T452-REG-TANGLER-CONTACT-CENTRE) and on the item's tick-end position for an end of flight that is not a contact, lives TIME in whole 30 Hz ticks, re-applies the tangle to every actor inside RADIUS squared each tick, the owner included, and exposes a timed tangled state whose remaining ticks a flight consumer reads; a launch alone opens nothing, the silent end of RANGE opens nothing, and a launch of a component with no registered field opens none. NOT CLAIMED: any original-run behavior (retail is read access and no agent ran the original executable); what the suppressed force scalar is in physical terms (claim t452_reg.limit.tangler_force_scalar_meaning); and the aircraft-side consumer that feeds a real flight model from this state (the ordnance session exposes it, and no production mission opens a wep_12 field: M01 flies the designed rocket loadout, so nothing in a live mission reads it yet). The claim t452_r02.limit.tangler_flight_effect is resolved by this stage: the effect it once left unnamed is the measured timed force-scalar suppression above; the field origin this stage left unnamed is measured too (the point of contact of a swept hit, or the projectile's tick-end position otherwise, per T452-REG-TANGLER-CONTACT-CENTRE) and its claim t452_reg.limit.tangler_field_origin is resolved by T452-REG-TANGLER-BURST-OPEN, so only the force scalar stays named on the record. The claim is implemented: a code and test pass awards nothing above that, and no agent review replaces the owner's human approval. Validated structurally with tools/validate_evidence.py - not with --require-pass, because the report's `unknowns` list carries every unpinned part, derived (never typed) from the production report's own kept claims and blockers. None is deleted to satisfy a validator.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t452_tangler_runtime_writes_the_acceptance_report() {
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
        suite.passed >= 8 && suite.assertions.len() as u64 >= suite.passed,
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
                    "{retail_test} did not run: T452-REG-TANGLER-RUNTIME requires capability \
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
    assert!(
        report.registered_records().contains(&TANGLER_RECORD),
        "the measured tangler record registers: {:?}",
        report.blocked_records()
    );
    let field = report
        .tangler_fields
        .iter()
        .find(|field| field.record == TANGLER_RECORD)
        .expect("the registered record reports its measured field");
    assert_eq!(field.radius_m, 10.0, "the image's default RADIUS");
    assert_eq!(field.lifetime_s, 2.0, "the record's own TIME");
    assert!(field.created_at_fire_time && field.reapplied_every_tick);
    assert!(field.includes_owner);

    let driven = drive(&game_dir);
    assert_eq!(driven.radius_m, field.radius_m);
    assert_eq!(driven.lifetime_ticks, 60, "TIME 2 s at 30 Hz");

    let mut outcomes = Vec::new();
    for outcome in &report.outcomes {
        match outcome {
            cs_app::ordnance::OriginalRegistrationOutcome::Registered {
                record, family, ..
            } => outcomes.push(format!(
                "{{\"record\": {}, \"status\": \"registered\", \"family\": {}}}",
                jstr(record),
                jstr(family.label())
            )),
            cs_app::ordnance::OriginalRegistrationOutcome::Blocked { record, blockers } => outcomes
                .push(format!(
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
    let kept: Vec<String> = report
        .claims_kept
        .iter()
        .filter(|(record, _)| record == TANGLER_RECORD)
        .map(|(record, claims)| {
            format!(
                "{{\"record\": {}, \"claims\": [{}]}}",
                jstr(record),
                claims
                    .iter()
                    .map(|claim| jstr(claim))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect();
    let observation = format!(
        "{{\"task\": \"T452-REG-TANGLER-RUNTIME\", \"declared\": {}, \"registered\": {}, \
         \"blocked\": {}, \"field\": {{\"record\": {}, \"radius_m\": {}, \"lifetime_s\": {}, \
         \"lifetime_ticks\": {}, \"status_floor_s\": {}, \"status_falloff_scale_s\": {}, \
         \"status_claim\": {}}}, \"launch\": {{\"centre\": {}, \"opened_at\": {}, \
         \"applications\": {}, \"duration_ticks\": {}, \"tangle_cleared_at\": {}, \
         \"boost_refused_while_tangled\": {}, \"boost_after\": {}}}, \"claims_kept\": [{}], \
         \"components\": [{}]}}\n",
        report.declared,
        report.registered_records().len(),
        report.blocked_records().len(),
        jstr(&field.record),
        field.radius_m,
        field.lifetime_s,
        driven.lifetime_ticks,
        field.status.floor_s,
        field.status.falloff_scale_s,
        jstr(field.status.claim),
        vec_json(driven.centre),
        driven.opened_at,
        driven.applications,
        driven.duration_ticks,
        driven.tangle_cleared_at,
        driven.boost_refused_while_tangled,
        driven.boost_after,
        kept.join(", "),
        outcomes.join(", ")
    );
    let observation_path = evidence_dir.join("original-field.json");
    fs::write(&observation_path, &observation).expect("write original-field.json");
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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T452-REG-TANGLER-RUNTIME\",\n \
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
        driven.tangle_cleared_at,
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

/// What one real launch of the owner's `wep_12` produced.
#[derive(Clone, Copy, Debug)]
struct Driven {
    centre: WorldPosition,
    radius_m: f64,
    lifetime_ticks: u64,
    opened_at: u64,
    applications: u64,
    duration_ticks: u64,
    tangle_cleared_at: u64,
    boost_refused_while_tangled: bool,
    boost_after: bool,
}

/// Registers the owner's catalogue, registers `wep_12` for one actor, launches
/// it through [`step_ordnance_session`] and walks the field to its end and the
/// tangle to its own boundary.
fn drive(game_dir: &Path) -> Driven {
    let table = cs_content::ordnance::read_original_component_table(game_dir).expect("table");
    let records = cs_content::ordnance::read_original_weapon_records(game_dir).expect("records");
    let media = cs_content::ordnance::read_original_weapon_media(game_dir).expect("media");
    let ammo = cs_content::ordnance::read_original_ammo_declarations(game_dir).expect("ammo");
    let provenance = cs_content::ordnance::read_original_weapons_provenance(
        game_dir,
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
    let field = registration
        .tangler_fields
        .iter()
        .find(|field| field.record == TANGLER_RECORD)
        .expect("the registered record reports its measured field")
        .clone();
    let declared = cs_app::ordnance::measured_ordnance_declaration(
        table
            .rows
            .iter()
            .find(|row| row.record == TANGLER_RECORD)
            .expect("wep_12"),
        records
            .iter()
            .find(|record| record.record == TANGLER_RECORD)
            .expect("wep_12's record"),
        media
            .iter()
            .find(|media| media.record == TANGLER_RECORD)
            .expect("wep_12's media"),
        ammo.iter()
            .find(|ammo| ammo.record == TANGLER_RECORD)
            .expect("wep_12's ammo"),
        &provenance,
        TICKS_PER_SECOND,
    )
    .expect("wep_12's measured values are expressible");

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
        .expect("the measured record registers");
    session
        .register_tangler_field(&ordnance_id(TANGLER_RECORD), field.clone())
        .expect("the measured field is the shape the runtime opens");

    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();
    let victim = actor(2);
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
            ordnance: ordnance_id(TANGLER_RECORD),
            target: None,
            engagement: OrdnanceEngagement {
                damage_target: victim,
                node: key("hull"),
                status_recipient: StatusEffectTarget::whole_actor(victim),
            },
        })],
        &tick_step(0, &[]),
    );
    assert_eq!(launch.launched.len(), 1, "the record launches");
    assert!(
        launch.tangler_opened.is_empty() && session.tangler_fields().is_empty(),
        "a launch alone opens no field (task #2203)"
    );

    // The projectile leaves the mount along -z at the record's own measured
    // VELOCITY and advances once on each of ticks 0, 1 and 2. The field opens
    // at the point of contact, which is CONTACT_TIME into the third advance
    // (task #2205), not at the tick-end position three advances out.
    let row = table
        .rows
        .iter()
        .find(|row| row.record == TANGLER_RECORD)
        .expect("wep_12");
    let per_tick_m = row.velocity.expect("wep_12 declares VELOCITY") / f64::from(TICKS_PER_SECOND);
    let contact_m = (BURST_TICK as f64 + CONTACT_TIME) * per_tick_m;
    let tick_end_m = (BURST_TICK as f64 + 1.0) * per_tick_m;
    let contact_centre = [
        MOUNT_ORIGIN[0],
        MOUNT_ORIGIN[1],
        MOUNT_ORIGIN[2] - contact_m as f32,
    ];
    let tick_end = [
        MOUNT_ORIGIN[0],
        MOUNT_ORIGIN[1],
        MOUNT_ORIGIN[2] - tick_end_m as f32,
    ];
    let targets = [path(victim, contact_centre)];

    let quiet = step_ordnance_session(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &tick_step(1, &targets),
    );
    assert!(
        quiet.tangler_opened.is_empty() && session.tangler_fields().is_empty(),
        "flight without a burst opens no field"
    );

    let projectile = launch.launched[0];
    let burst = step_ordnance_session(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &intercept_step(
            BURST_TICK,
            &targets,
            &[SweptHit {
                projectile,
                target: victim,
                time_of_impact: CONTACT_TIME,
            }],
        ),
    );
    assert_eq!(burst.tangler_opened.len(), 1, "the burst opens the field");

    let open = session
        .tangler_fields()
        .first()
        .cloned()
        .expect("the field is open");
    assert_eq!(
        open.centre,
        WorldPosition::try_new([
            f64::from(contact_centre[0]),
            f64::from(contact_centre[1]),
            f64::from(contact_centre[2])
        ])
        .expect("the fixture contact position is finite"),
        "the field is centred on the point of contact, not the tick-end position"
    );
    assert_ne!(
        open.centre,
        WorldPosition::try_new([
            f64::from(tick_end[0]),
            f64::from(tick_end[1]),
            f64::from(tick_end[2])
        ])
        .expect("the fixture tick-end position is finite"),
        "the field is not centred on where the item ended the burst tick"
    );
    let mut driven = Driven {
        centre: open.centre,
        radius_m: open.radius_m,
        lifetime_ticks: open.lifetime_ticks,
        opened_at: open.opened_at.0,
        applications: 0,
        duration_ticks: 0,
        tangle_cleared_at: 0,
        boost_refused_while_tangled: false,
        boost_after: false,
    };
    let count = |tick: &cs_app::ordnance::OrdnanceSessionTick, driven: &mut Driven| {
        for pass in &tick.tangler {
            driven.applications += pass.applied.len() as u64;
            for application in &pass.applied {
                driven.duration_ticks = driven.duration_ticks.max(application.duration_ticks);
            }
        }
    };
    count(&burst, &mut driven);
    driven.boost_refused_while_tangled = !session.boost_allowed(&victim);

    let limit = BURST_TICK + open.lifetime_ticks + driven.duration_ticks + 8;
    for at in (BURST_TICK + 1)..=limit {
        let tick = step_ordnance_session(
            &mut world,
            &mut session,
            &mut damage,
            &[],
            &tick_step(at, &targets),
        );
        count(&tick, &mut driven);
        if !tick.tangles_expired.is_empty() {
            driven.tangle_cleared_at = at;
            break;
        }
    }
    driven.boost_after = session.boost_allowed(&victim);
    assert!(
        driven.tangle_cleared_at > 0,
        "the tangle reaches its own boundary"
    );
    assert!(
        driven.opened_at == BURST_TICK,
        "the field opened on the burst tick, not on the launch tick"
    );
    driven
}

fn tick_step(at: u64, targets: &[TargetPath]) -> OrdnanceStep<'_> {
    intercept_step(at, targets, &[])
}

/// One step of a session that is told about swept contacts.
fn intercept_step<'a>(
    at: u64,
    targets: &'a [TargetPath],
    impacts: &'a [SweptHit],
) -> OrdnanceStep<'a> {
    OrdnanceStep {
        at: Tick(at),
        dt_s: 1.0 / f64::from(TICKS_PER_SECOND),
        wind_velocity_m_s: [0.0; 3],
        generation: GENERATION,
        guidance: None,
        targets,
        impacts,
        blast_recipients: None,
        victim_nodes: None,
    }
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

fn path(target: ActorId, at: [f32; 3]) -> TargetPath {
    let at = [f64::from(at[0]), f64::from(at[1]), f64::from(at[2])];
    TargetPath::try_new(target, at, at).expect("a finite fixture path")
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

/// The shooter's live launcher: a root carrying the binding and the mount node
/// whose pose the fire path reads.
fn launcher(world: &mut World) -> Entity {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![
                    ContentId::from_source(ContentKind::Weapon, TANGLER_RECORD).expect("id"),
                ],
                loadout: ContentId::from_source(ContentKind::Loadout, "fixture.loadout")
                    .expect("a loadout id"),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::ZERO),
        ))
        .id();
    world.spawn((
        MountPoseBinding {
            mount: key("weapon_mount_12"),
            generation: GENERATION,
        },
        cs_app::scene::NodeVisualTransform(GlobalTransform::from_xyz(
            MOUNT_ORIGIN[0],
            MOUNT_ORIGIN[1],
            MOUNT_ORIGIN[2],
        )),
        ChildOf(root),
    ));
    root
}

/// A world position as a JSON array of its three components.
fn vec_json(value: WorldPosition) -> String {
    format!("[{}, {}, {}]", value.x(), value.y(), value.z())
}

/// Every value this stage could not pin, one `"claim: content (resolving
/// task)"` string per distinct claim, sorted.
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
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
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
/// `accept_t452_tangler_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_t452_tangler_") {
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
                 \"original-field.json\"]}}",
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
