//! The animation `HEALTH` hook that starts the M01 tank records (#2088).
//! Task test prefix: `accept_m01_hydrogen_fire_hook_`.
//!
//! The engine's weapon-hit handler of an `ANIMATION_DEFINITION` (static,
//! `[[animation_health_fact]]` of `data/owner-static/engine-facts.toml`, finding
//! `docs/findings/2026-10-24-m01-animation-health-consumer.md`) lowers the
//! record's `HEALTH` pool by the health channel of every weapon hit on its
//! target and starts the record's default sequence once the pool is at or below
//! zero. These tests drive that through the production path: the player's gun
//! round, the damage schedule's hit tap, the host's stage 3c and the record
//! player, and pin the pool arithmetic on authored records without retail
//! data.

use std::collections::BTreeSet;

use bevy::prelude::App;
use cs_app::animation::mission::{
    AnimationNameIndex, AnimationRecordFacts, CarrierRecords, RecordSequence, SequenceEvents,
};
use cs_app::animation::survey::CarrierKind;
use cs_app::damage::PendingDamageEvents;
use cs_app::mission_actors::MissionGroundTargetReport;
use cs_app::mission_animation_health::{AnimationHealthLedger, WEAPON_HIT_ACTIVATION};
use cs_app::mission_animations::{ActivationOutcome, MissionAnimationPlayer};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionHost, build_headless, stage_for, teardown};
use cs_app::physics::PhysicsTickLedger;
use cs_app::weapons::SessionDamageResolver;
use cs_sim::damage::{ActorId, DamageChannel, HitEvent};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{DamageNodeKey, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::net::{EventId, SessionId};

use crate::campaign::common::label;
use crate::campaign::m01_convoy_gun_hits::{destroyed, game_dir, idle};

// ---------------------------------------------------------------- authored ---

const NODE: &str = "synthetic_tank";
const RECORD: &str = "synthetic_tank_boom";

fn record(name: &str, object: &str, activation: u8, health: f32) -> AnimationRecordFacts {
    AnimationRecordFacts {
        carrier: CarrierKind::Mission,
        carrier_key: "zbd/synthetic/mis_anim.zbd".to_owned(),
        index: 0,
        span: SourceSpan::new(
            ContentHash::from_bytes([7u8; 32]),
            "zbd/synthetic/mis_anim.zbd",
            None,
            4_096,
            672,
            None,
        )
        .expect("a valid span"),
        provenance: Provenance::new(
            ClaimId::new("m01-hydrogen-fire-hook.accept-test").expect("a claim"),
            ClaimStatus::ObservedTool,
            None,
        )
        .expect("an observed-tool provenance"),
        anim_name: name.to_owned(),
        object_name: object.to_owned(),
        root_name: object.to_owned(),
        flags: 0,
        status: 0,
        activation,
        execution_priority: 4,
        reset_time: -1.0,
        max_health: health,
        objects: Vec::new(),
        nodes: Vec::new(),
        animation_refs: Vec::new(),
        call_targets: Vec::new(),
        sequences: vec![RecordSequence {
            kind: cs_formats::zbd::AnimationRecordSequenceKind::Sequence,
            name: String::new(),
            event_bytes: 0,
            events: SequenceEvents::Decoded(Vec::new()),
        }],
    }
}

fn names(records: Vec<AnimationRecordFacts>) -> AnimationNameIndex {
    let records = records
        .into_iter()
        .enumerate()
        .map(|(index, mut facts)| {
            facts.index = index;
            facts
        })
        .collect();
    AnimationNameIndex::new(
        vec![CarrierRecords::authored(
            CarrierKind::Mission,
            "zbd/synthetic/mis_anim.zbd",
            records,
        )],
        Vec::new(),
        None,
    )
}

fn session() -> SessionId {
    SessionId::new(1).expect("a session id")
}

fn hit(serial: u64, node: &str, channel: DamageChannel, damage: f64) -> HitEvent {
    HitEvent::try_new(
        EventId {
            session: session(),
            tick: Tick(serial),
            producer: 0,
            sequence: 0,
        },
        None,
        ActorId {
            session: session(),
            serial: 1,
        },
        DamageNodeKey::new(node).expect("a node key"),
        channel,
        damage,
    )
    .expect("a finite, non-negative hit")
}

fn player() -> MissionAnimationPlayer {
    MissionAnimationPlayer::new(session(), 60).expect("a player")
}

/// **Only a weapon-hit record with a positive `HEALTH` on a bound target arms
/// a pool**, and the pool starts full.
#[test]
fn accept_m01_hydrogen_fire_hook_arms_only_weapon_hit_records_on_bound_targets() {
    let index = names(vec![
        record(RECORD, NODE, WEAPON_HIT_ACTIVATION, 15.0),
        record("on_call", NODE, 3, 15.0),
        record("no_health", NODE, WEAPON_HIT_ACTIVATION, 0.0),
        record("elsewhere", "other_node", WEAPON_HIT_ACTIVATION, 15.0),
    ]);
    let ledger = AnimationHealthLedger::arm(&index, [NODE]);
    let pools = ledger.pools();
    assert_eq!(pools.len(), 1, "{pools:?}");
    assert_eq!(pools[0].animation(), RECORD);
    assert_eq!(pools[0].node(), NODE);
    assert_eq!(pools[0].maximum(), 15.0);
    assert_eq!(pools[0].remaining(), 15.0);
}

/// **The pool takes the internal (health) channel of each hit on its node,
/// nothing else, and starts the record once when it reaches zero.**
#[test]
fn accept_m01_hydrogen_fire_hook_the_pool_counts_health_hits_and_starts_once() {
    let index = names(vec![record(RECORD, NODE, WEAPON_HIT_ACTIVATION, 15.0)]);
    let mut ledger = AnimationHealthLedger::arm(&index, [NODE]);
    let mut player = player();

    // An armor-channel hit and a hit on another node count nothing.
    let none = ledger.consume(
        &[
            hit(1, NODE, DamageChannel::Armor, 100.0),
            hit(2, "other_node", DamageChannel::Internal, 100.0),
        ],
        &index,
        &mut player,
        Tick(1),
    );
    assert!(none.is_empty());
    assert_eq!(ledger.pools()[0].remaining(), 15.0);

    // Two health hits of 6 leave 3: still no start.
    let none = ledger.consume(
        &[
            hit(3, NODE, DamageChannel::Internal, 6.0),
            hit(4, NODE, DamageChannel::Internal, 6.0),
        ],
        &index,
        &mut player,
        Tick(2),
    );
    assert!(none.is_empty());
    assert_eq!(ledger.pools()[0].remaining(), 3.0);

    // The third takes the pool below zero (no clamp): the record starts.
    let started = ledger.consume(
        &[hit(5, NODE, DamageChannel::Internal, 6.0)],
        &index,
        &mut player,
        Tick(3),
    );
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].animation, RECORD);
    assert_eq!(started[0].at, Tick(3));
    assert_eq!(ledger.pools()[0].remaining(), -3.0);

    // Later hits start nothing again.
    let again = ledger.consume(
        &[hit(6, NODE, DamageChannel::Internal, 6.0)],
        &index,
        &mut player,
        Tick(4),
    );
    assert!(again.is_empty());
    assert_eq!(ledger.starts().len(), 1);
}

/// **A hit of zero health does not move the pool, and an exact zero starts.**
#[test]
fn accept_m01_hydrogen_fire_hook_zero_and_exact_hits() {
    let index = names(vec![record(RECORD, NODE, WEAPON_HIT_ACTIVATION, 10.0)]);
    let mut ledger = AnimationHealthLedger::arm(&index, [NODE]);
    let mut player = player();
    assert!(
        ledger
            .consume(
                &[hit(1, NODE, DamageChannel::Internal, 0.0)],
                &index,
                &mut player,
                Tick(1),
            )
            .is_empty()
    );
    assert_eq!(ledger.pools()[0].remaining(), 10.0);
    let started = ledger.consume(
        &[hit(2, NODE, DamageChannel::Internal, 10.0)],
        &index,
        &mut player,
        Tick(2),
    );
    assert_eq!(started.len(), 1, "a pool of exactly zero starts the record");
    // No declaration joins the authored record, so the player refuses it and
    // the start says so.
    assert_eq!(started[0].outcome, ActivationOutcome::Refused);
}

// ------------------------------------------------------------------ retail ---

fn retail_app() -> App {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    idle(&mut app);
    app
}

fn host(app: &App) -> &MissionHost {
    app.world().resource::<MissionHost>()
}

fn running(app: &App) -> BTreeSet<String> {
    host(app)
        .animation()
        .running()
        .map(|record| record.identity().to_owned())
        .collect()
}

/// Resolves one health-channel hit on `node` through the session's own damage
/// authority and publishes the batch where the producers do, then runs the
/// schedule and the host's step.
fn strike(app: &mut App, node: &str, damage: f64, serial: u32) {
    let session = host(app).served();
    let tick = Tick(app.world().resource::<PhysicsTickLedger>().ticks);
    let hit = HitEvent::try_new(
        EventId {
            session,
            tick,
            producer: 900,
            sequence: serial,
        },
        None,
        target_actor(app, node),
        DamageNodeKey::new(node).expect("a node key"),
        DamageChannel::Internal,
        damage,
    )
    .expect("a finite, non-negative hit");
    let events = app
        .world_mut()
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .resolve(tick, std::slice::from_ref(&hit))
        .expect("the session resolves the hit")
        .events;
    app.world_mut().init_resource::<PendingDamageEvents>();
    app.world_mut()
        .resource_mut::<PendingDamageEvents>()
        .push(vec![hit], events);
    idle(app);
}

/// **Hits on a `hydrogentank` empty its `HEALTH 15` pool, and the hook asks
/// the record player to start `hydrobombboom<N>`.**
///
/// The four pools are armed from the mission's own records (object
/// `hydrogentankN`, activation 0, `max_health` 15) on the bound ground
/// targets; no tank record runs before a hit; three hits of 6 health points
/// leave 15, 9, 3 and -3, and the record starts on the third, not on the
/// first hit that destroys the target's designed one-unit pool; the other
/// three tank records are started by `call_others`. The player's answer today is
/// a started `hydrobombboom1` whose default sequence runs `hydrogen_fire`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_hydrogen_fire_hook_hits_on_a_tank_empty_its_pool_and_ask_for_its_record() {
    let mut app = retail_app();
    let pools: Vec<(String, String, f32)> = host(&app)
        .animation_health()
        .expect("the pools armed on the first step")
        .pools()
        .iter()
        .filter(|pool| pool.animation().starts_with("hydrobombboom"))
        .map(|pool| {
            (
                pool.animation().to_owned(),
                pool.node().to_owned(),
                pool.maximum(),
            )
        })
        .collect();
    assert_eq!(
        pools,
        (1..=4)
            .map(|n| (
                format!("hydrobombboom{n}"),
                format!("hydrogentank{n}"),
                15.0
            ))
            .collect::<Vec<_>>()
    );
    assert!(
        !running(&app).iter().any(|id| id.starts_with("hydro")),
        "nothing started before a hit"
    );
    let remaining = |app: &App| {
        host(app)
            .animation_health()
            .expect("armed")
            .pools()
            .iter()
            .find(|pool| pool.animation() == "hydrobombboom1")
            .expect("the pool")
            .remaining()
    };

    strike(&mut app, "hydrogentank1", 6.0, 1);
    assert_eq!(remaining(&app), 9.0);
    assert!(
        destroyed(&app, target_actor(&app, "hydrogentank1")),
        "the designed one-unit pool is gone on the first hit"
    );
    assert!(!running(&app).contains("hydrobombboom1"), "9 points left");
    strike(&mut app, "hydrogentank1", 6.0, 2);
    assert_eq!(remaining(&app), 3.0);
    assert!(!running(&app).contains("hydrobombboom1"), "3 points left");
    strike(&mut app, "hydrogentank1", 6.0, 3);
    assert_eq!(remaining(&app), -3.0);

    let starts = host(&app).animation_health().expect("armed").starts();
    assert_eq!(starts.len(), 1, "{starts:?}");
    assert_eq!(starts[0].animation, "hydrobombboom1");
    assert_eq!(starts[0].node, "hydrogentank1");
    // The group damage definition spells the record only as the wildcard
    // state name `hydrobombboom*` over `hydrogentank*`; the join expands it to
    // `hydrobombboom1` on `hydrogentank1`, so the start is made.
    assert_eq!(starts[0].outcome, ActivationOutcome::Started);
    assert!(running(&app).contains("hydrobombboom1"));
    // Its default sequence calls `hydrogen_fire`, which is what OBJECTIVE25's
    // `ANIM_STATE` gate reads as `Running`.
    assert!(
        host(&app)
            .call_animations()
            .activations()
            .iter()
            .any(|call| call.caller() == "hydrobombboom1"
                && call.name() == "hydrogen_fire"
                && call.outcome() == ActivationOutcome::Started),
        "{:?} {:?}",
        host(&app).call_animations().activations(),
        host(&app).call_animations().refused()
    );
    assert!(running(&app).contains("hydrogen_fire"));
    for other in ["hydrobombboom2", "hydrobombboom3", "hydrobombboom4"] {
        // Not hit, but started by the first record's `call_others` call.
        assert!(running(&app).contains(other), "{other}: call_others' job");
    }
    teardown(&mut app);
}

fn target_actor(app: &App, node: &str) -> ActorId {
    app.world()
        .resource::<MissionGroundTargetReport>()
        .target(node)
        .unwrap_or_else(|| panic!("{node} spawned"))
        .actor
}

// ------------------------------------------------- the cited static rows ---

const TABLE: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

/// `(name, va, bytes)` of every `[[animation_health_fact]]` row.
fn health_rows() -> Vec<(String, u64, Vec<u8>)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("an animation health row lacks {name}"))
    };
    TABLE
        .split("[[animation_health_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            assert_eq!(field(chunk, "provenance"), "static");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            (field(chunk, "name"), va, bytes)
        })
        .collect()
}

/// **The committed rows name the consumer the hook models.**
#[test]
fn accept_m01_hydrogen_fire_hook_the_committed_rows_cite_the_consumer() {
    let rows = health_rows();
    let names: Vec<&str> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "anim.object_zeroed",
            "anim.health_store",
            "anim.armor_store",
            "anim.weapon_hit_registration",
            "anim.weapon_hit_dispatch",
            "anim.hit_weapon_filter",
            "anim.hit_amount",
            "anim.health_subtract",
            "anim.start_at_zero",
        ]
    );
    let get = |name: &str| rows.iter().find(|(n, _, _)| n == name).expect("a row");
    // HEALTH fills the maximum (+0xb4) and the current (+0xb8) health.
    assert_eq!(get("anim.health_store").2[2..4], [0xb4, 0x00]);
    assert_eq!(get("anim.health_store").2[8..10], [0xb8, 0x00]);
    // ARMOR is +0xbc.
    assert_eq!(get("anim.armor_store").2[2..4], [0xbc, 0x00]);
    // The amount is [damage+4] less [object+0xbc].
    assert_eq!(get("anim.hit_amount").2[..3], [0xd9, 0x40, 0x04]);
    assert_eq!(
        get("anim.hit_amount").2[3..],
        [0xd8, 0xa6, 0xbc, 0x00, 0x00, 0x00]
    );
    // fsub st, st(1) lowers +0xb8.
    assert_eq!(get("anim.health_subtract").2[6..8], [0xd8, 0xe1]);
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_hydrogen_fire_hook_the_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    let image = std::fs::read(path).expect("the engine image reads");
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    for (name, va, bytes) in health_rows() {
        let offset = facts
            .image_offset(va)
            .unwrap_or_else(|error| panic!("{name}: {error}")) as usize;
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{name} at {va:#x}"
        );
    }
}
