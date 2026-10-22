//! Acceptance for Rally #1880 `FLAK-B1-TURRET-WAKE`: the turret entries, the
//! live byte each record's `ACTIVATED` sets, and the `WAKEUP_TURRETS` /
//! `WAKEUP_ZEP_TURRETS` consumer the mission host runs (findings
//! `docs/findings/2026-10-21-flak-a-turret-update-law.md` and
//! `docs/findings/2026-10-21-m01-flak-and-site-defences.md`).
//!
//! Nothing fires here. The synthetic tests run in CI; the retail test drives
//! M01's own win chain through the production host. `retail` is read access to
//! the installation, nothing here is `verified_original`.

use std::time::Duration;

use avian3d::prelude::Position;
use bevy::prelude::{App, Entity, Vec3, With};
use bevy::time::TimeUpdateStrategy;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionPlayerBody, build_headless, mission_world_observation, stage_for,
};
use cs_app::mission_turrets::{
    DEFAULT_DETECTION_RANGE_M, DEFAULT_TEAM, TURRET_WAKE_ARGUMENTS_REASON,
    TURRET_WAKE_NO_MATCH_REASON, TurretEntries, TurretMount, TurretWakeLedger,
    ZEPPELIN_WAKE_NO_MATCH_REASON, read_turret_specs,
};
use cs_app::physics::PhysicsTickLedger;
use cs_content::campaign_bindings::MissionLabel;
use cs_content::stunts::ZrdValue;
use cs_script::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective, SymbolId, Value,
};
use cs_script::runtime::{MissionFacts, SessionGeneration};
use cs_sim::mission::MissionSession;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn list(values: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(values)
}

fn field(key: &str, values: Vec<ZrdValue>) -> [ZrdValue; 2] {
    [text(key), list(values)]
}

/// A synthetic `ai.zrd`: one record that starts asleep with stated fields, one
/// that states none of the defaulted ones, one with no `NODES`.
fn synthetic_ai() -> ZrdValue {
    let mut asleep = Vec::new();
    asleep.extend(field("NODES", vec![text("gun**")]));
    asleep.extend(field("ACTIVATED", vec![ZrdValue::Int(0)]));
    asleep.extend(field("TEAM", vec![ZrdValue::Int(1)]));
    asleep.extend(field("DETECTION_RANGE", vec![ZrdValue::Float(750.0)]));
    asleep.extend(field(
        "FIRE_RATE",
        vec![ZrdValue::Float(1.0), ZrdValue::Float(1.8)],
    ));
    asleep.extend(field(
        "PITCH",
        vec![ZrdValue::Float(-10.0), ZrdValue::Float(60.0)],
    ));
    asleep.extend(field(
        "WEAPON",
        vec![text("NAME"), list(vec![text("wep_x")])],
    ));
    asleep.extend(field("MYSTERY", vec![ZrdValue::Int(7)]));
    let mut live = Vec::new();
    live.extend(field("NODES", vec![text("flak**")]));
    let mut zep = Vec::new();
    zep.extend(field(
        "NODES",
        vec![list(vec![text("zep"), text("cannon*")])],
    ));
    zep.extend(field("ACTIVATED", vec![ZrdValue::Int(0)]));
    let mut none = Vec::new();
    none.extend(field("ACTIVATED", vec![ZrdValue::Int(0)]));
    list(vec![list(vec![
        text("TURRET"),
        list(vec![list(none), list(asleep), list(live), list(zep)]),
    ])])
}

fn world_nodes() -> Vec<(String, [f64; 3])> {
    ["gun01", "gun02", "gunner", "flak07", "other"]
        .iter()
        .enumerate()
        .map(|(i, name)| ((*name).to_owned(), [i as f64, 0.0, 0.0]))
        .collect()
}

fn wake_program(operation: DirectiveOperation, args: Vec<Value>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-flak-b1"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(ContentKind::Objective, "synthetic-flak-b1-wake"),
            condition: Condition::Const(true),
            actions: vec![Action::Directive { operation, args }],
            span: None,
        }],
    }
}

fn state_after(program: MissionProgram) -> MissionSession {
    let mut session = MissionSession::launch_seeded(program, SessionGeneration(1), 1, [])
        .expect("the synthetic program launches");
    session
        .advance(&MissionFacts::default(), Tick(1))
        .expect("the tick advances");
    session
}

fn str_arg(value: &str) -> Value {
    Value::Str(value.to_owned())
}

#[test]
fn accept_flak_b1_the_reader_takes_activated_and_the_defaults_from_the_record() {
    let specs = read_turret_specs(&synthetic_ai());
    // The record without NODES is skipped; indexes are list positions.
    assert_eq!(specs.iter().map(|s| s.index).collect::<Vec<_>>(), [1, 2, 3]);
    let asleep = &specs[0];
    assert_eq!(asleep.activated, Some(false));
    assert!(!asleep.starts_live());
    assert_eq!(asleep.team_id(), 1);
    assert_eq!(asleep.detection_range(), 750.0);
    assert_eq!(
        asleep.fire_rate.as_deref(),
        Some(&[1.0, f64::from(1.8_f32)][..])
    );
    assert_eq!(asleep.pitch.as_deref(), Some(&[-10.0, 60.0][..]));
    assert_eq!(asleep.weapon.as_deref(), Some("wep_x"));
    assert_eq!(
        asleep.other_keys,
        ["MYSTERY"],
        "unknown fields stay unknown"
    );
    let live = &specs[1];
    assert_eq!(live.activated, None);
    assert!(live.starts_live(), "ACTIVATED absent is live");
    assert_eq!(live.team_id(), DEFAULT_TEAM);
    assert_eq!(live.detection_range(), DEFAULT_DETECTION_RANGE_M);
}

#[test]
fn accept_flak_b1_entries_bind_by_pattern_and_the_wake_ledger_sets_the_live_byte_once() {
    let specs = read_turret_specs(&synthetic_ai());
    let chains = vec![
        vec!["zep".to_owned(), "cannon1".to_owned()],
        vec!["zep".to_owned(), "other".to_owned()],
    ];
    let entries = TurretEntries::bind(&specs, Ok(world_nodes()), &chains);
    let names: Vec<_> = entries.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["gun01", "gun02", "flak07", "cannon1"]);
    assert!(!entries.entry("gun01").unwrap().live);
    assert!(entries.entry("flak07").unwrap().live);
    assert_eq!(
        entries.entry("gun02").unwrap().position_m,
        Some([1.0, 0.0, 0.0])
    );
    assert_eq!(
        entries.entry("cannon1").unwrap().mount,
        TurretMount::Zeppelin
    );

    let session = state_after(wake_program(
        DirectiveOperation::WakeTurrets,
        vec![str_arg("gun**"), str_arg("nothing**")],
    ));
    let mut ledger = TurretWakeLedger::new(&entries);
    ledger.consume(session.state(), Tick(1));
    assert!(ledger.entry("gun01").unwrap().live && ledger.entry("gun02").unwrap().live);
    assert!(
        !ledger.entry("cannon1").unwrap().live,
        "a world wake never wakes a zeppelin part"
    );
    assert_eq!(ledger.wakes().len(), 1);
    assert_eq!(ledger.wakes()[0].woken, ["gun01", "gun02"]);
    assert_eq!(
        ledger.refused().len(),
        1,
        "the miss is named, not swallowed"
    );
    assert_eq!(ledger.refused()[0].name, "nothing**");
    assert_eq!(ledger.refused()[0].reason, TURRET_WAKE_NO_MATCH_REASON);
    // The same emission is applied exactly once.
    ledger.consume(session.state(), Tick(2));
    assert_eq!(ledger.applied(), 1);
    assert_eq!(ledger.wakes().len(), 1);
    assert_eq!(ledger.refused().len(), 1);
    // A fresh ledger over the seed is asleep again (restart).
    assert!(!TurretWakeLedger::new(&entries).entry("gun01").unwrap().live);
}

#[test]
fn accept_flak_b1_the_zeppelin_wake_reaches_the_named_zeppelin_and_a_bad_site_is_refused() {
    let specs = read_turret_specs(&synthetic_ai());
    let chains = vec![vec!["zep".to_owned(), "cannon1".to_owned()]];
    let entries = TurretEntries::bind(&specs, Ok(world_nodes()), &chains);
    let session = state_after(wake_program(
        DirectiveOperation::WakeZeppelinTurrets,
        vec![str_arg("zep"), str_arg("ghostzep")],
    ));
    let mut ledger = TurretWakeLedger::new(&entries);
    assert!(!ledger.entry("cannon1").unwrap().live);
    ledger.consume(session.state(), Tick(1));
    assert!(ledger.entry("cannon1").unwrap().live);
    assert!(!ledger.entry("gun01").unwrap().live);
    assert_eq!(ledger.refused().len(), 1);
    assert_eq!(ledger.refused()[0].name, "ghostzep");

    let session = state_after(wake_program(
        DirectiveOperation::WakeTurrets,
        vec![Value::Int(3)],
    ));
    let mut ledger = TurretWakeLedger::new(&entries);
    ledger.consume(session.state(), Tick(1));
    assert_eq!(ledger.refused()[0].reason, TURRET_WAKE_ARGUMENTS_REASON);
}

// ---------------------------------------------------------------- retail ---

/// One driven step of the production host. The player's body is moved to `at`
/// and the world's own report is built from it (`mission_world_observation`),
/// so M01's `TRAVELERS player APPROACHING <site>` objectives read the same rows
/// a flown run files.
fn step_host(app: &mut App, at: [f64; 3]) {
    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    world.get_mut::<Position>(body).expect("a pose").0 =
        Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32);
    mission_world_observation(world);
    world.resource_mut::<PhysicsTickLedger>().ticks += 1;
    let mut host = world
        .remove_resource::<MissionHost>()
        .expect("the composed app carries the host");
    let produced = host
        .step(world, Duration::from_secs(1))
        .expect("the driven step answers");
    host.settle(world, &produced);
    world.insert_resource(host);
}

#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_flak_b1_retail_the_trucks_sleep_until_objective6_and_the_island_guns_never_wake() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set for the retail leg"),
    );
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M01").expect("a mission label"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    assert!(
        stage.host.turrets.refusals.is_empty(),
        "{:?}",
        stage.host.turrets.refusals
    );

    let trucks = ["t_truck01", "t_truck02", "t_truck03"];
    let island = ["aagun01", "aagun02", "aagun30", "aagun31", "aagun32"];
    for name in trucks.iter().chain(&island) {
        let entry = stage
            .host
            .turrets
            .entry(name)
            .unwrap_or_else(|| panic!("{name} has no turret entry"));
        assert_eq!(entry.mount, TurretMount::World, "{name}");
        assert!(!entry.live, "{name}'s record states ACTIVATED 0");
        assert!(entry.position_m.is_some(), "{name} has a composed position");
        assert_eq!(entry.team, DEFAULT_TEAM, "{name} states no TEAM");
    }
    let truck = stage.host.turrets.entry("t_truck01").unwrap();
    let position = truck.position_m.unwrap();
    assert!(
        (position[0] + 5459.3).abs() < 0.5 && (position[2] + 5339.2).abs() < 0.5,
        "{position:?}"
    );

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    // The spawn-only frame: zero fixed ticks run, the roster and the player's
    // body exist, and the host has consumed no directive.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();
    let host = |app: &App| app.world().resource::<MissionHost>().wake_turrets().clone();
    assert_eq!(host(&app).applied(), 0);
    assert!(trucks.iter().all(|n| !host(&app).entry(n).unwrap().live));

    // The player visits every declared objective site in turn and ends each
    // lap at the bridge, so each `TRAVELERS` objective of the chain sees it.
    let mut route: Vec<[f64; 3]> = stage
        .host
        .targets
        .declared_targets()
        .map(|target| target.position_m)
        .collect();
    route.push(position);
    let mut woke_at = None;
    for step in 0..600 {
        step_host(&mut app, route[step % route.len()]);
        let ledger = host(&app);
        if woke_at.is_none() && trucks.iter().all(|n| ledger.entry(n).unwrap().live) {
            woke_at = Some(step);
            let wake = ledger
                .wakes()
                .iter()
                .find(|wake| wake.name == "t_truck**")
                .expect("the t_truck** wake is recorded");
            assert_eq!(wake.woken, trucks, "{wake:?}");
        }
        if app.world().resource::<MissionHost>().terminal().is_some() {
            break;
        }
    }
    assert!(
        woke_at.is_some(),
        "OBJECTIVE6's WAKEUP_TURRETS wakes the trucks"
    );
    let ledger = host(&app);
    for name in island {
        assert!(
            !ledger.entry(name).unwrap().live,
            "{name} stays asleep in M01"
        );
    }
    // M01's turret wake sites: the one `t_truck**` wake, and OBJECTIVE1's
    // `WAKEUP_ZEP_TURRETS piratezep`, which is **named** as woken nothing — the
    // program names none of the pirate zeppelin's gun chains, so no entry of
    // it is bound (an unknown the finding records), and the miss is kept.
    assert_eq!(ledger.wakes().len(), 1, "{:?}", ledger.wakes());
    assert_eq!(ledger.wakes()[0].name, "t_truck**");
    let pirate = ledger
        .refused()
        .iter()
        .find(|refusal| refusal.name == "piratezep")
        .unwrap_or_else(|| panic!("piratezep is refused by name: {:?}", ledger.refused()));
    assert_eq!(pirate.reason, ZEPPELIN_WAKE_NO_MATCH_REASON);
}
