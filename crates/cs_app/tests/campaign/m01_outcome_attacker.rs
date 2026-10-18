//! M01-OUTCOME-A-ATTACKER (#1680): which hostile aircraft can engage the M01
//! player in the first ~70 s (finding
//! `docs/findings/2026-10-19-m01-launch-attackers.md`).
//!
//! The synthetic members run in CI. The retail members read `$CS_GAME_DIR`
//! (never write it) through production readers, recompute what they compare
//! against from the members' own bytes and fail loudly without it.

use std::path::PathBuf;

use cs_app::animation::events::EventClass;
use cs_app::cinematics::launch_attackers::{
    AttackerReadError, ObjectivesWakeGraph, PIRATE_FIGHTER_NODE, TravelTarget,
    survey_cinematic_fighters,
};
use cs_app::mission_start::recover_retail_aircraft_records;
use cs_app::mission_targets::bind_mission_targets;
use cs_content::stunts::ZrdValue;
use cs_types::install::RelativePath;

use super::m01_aiv_layout::member_document;

const ARCHIVE: &str = "zbd/c3/m01/zrdr.zbd";

fn root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"))
}

fn text(s: &str) -> ZrdValue {
    ZrdValue::Text(s.to_owned())
}

fn list(items: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(items)
}

fn objectives_graph(root: &std::path::Path) -> ObjectivesWakeGraph {
    let (document, _) = member_document(root, ARCHIVE, "objectives.zrd");
    ObjectivesWakeGraph::read(&document).expect("objectives.zrd reads")
}

#[test]
fn accept_m01_outcome_attacker_synthetic_graph_reads_timers_edges_and_sites() {
    let document = list(vec![
        text("OBJECTIVE1"),
        list(vec![
            text("BEGIN_DORMANT"),
            list(vec![ZrdValue::Float(2.0)]),
            text("WAKEUP_ZEP_TURRETS"),
            list(vec![text("zep")]),
        ]),
        text("OBJECTIVE2"),
        list(vec![
            text("TRAVELERS"),
            list(vec![
                text("player"),
                text("APPROACHING"),
                text("rock"),
                ZrdValue::Float(200.0),
                ZrdValue::Int(1),
            ]),
            text("NAP_OBJECTIVE_WHEN_I_COMPLETE"),
            list(vec![ZrdValue::Int(3), ZrdValue::Float(1.5)]),
        ]),
        text("OBJECTIVE3"),
        list(vec![
            text("INSTANTWIN"),
            text("BEGIN_DORMANT"),
            list(vec![ZrdValue::Float(-1.0)]),
            text("WAKEUP_ENEMIES"),
            list(vec![text("bandit")]),
        ]),
    ]);
    let graph = ObjectivesWakeGraph::read(&document).expect("reads");
    assert_eq!(graph.get(1).and_then(|o| o.timed_wake_s()), Some(2.0));
    assert!(graph.get(2).is_some_and(|o| o.awake_at_start()));
    assert!(
        graph
            .get(3)
            .is_some_and(|o| !o.awake_at_start() && o.timed_wake_s().is_none())
    );
    assert_eq!(graph.blocks_waking("bandit"), vec![3]);
    assert_eq!(graph.blocks_waking("zep"), vec![1]);
    assert_eq!(graph.ancestors(3).into_iter().collect::<Vec<_>>(), vec![2]);
    assert_eq!(
        graph
            .get(2)
            .and_then(|o| o.travelers.clone())
            .map(|t| (t.target, t.radius)),
        Some((TravelTarget::Node("rock".to_owned()), 200.0))
    );

    let path = RelativePath::new("zbd/c3/cam_anim.zbd").expect("a relative path");
    let error = survey_cinematic_fighters(&path, &[0u8; 64], "generic_intro")
        .expect_err("bytes that are no animation container survey nothing");
    assert!(matches!(error, AttackerReadError::Carrier(_)), "{error}");
    assert!(ObjectivesWakeGraph::read(&list(vec![])).is_err());
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_outcome_attacker_retail_cinematic_fighters_are_scene_nodes_without_weapon_events() {
    let root = root();
    let bytes = std::fs::read(root.join("ZBD/C3/cam_anim.zbd")).expect("cam_anim.zbd is readable");
    let path = RelativePath::new("zbd/c3/cam_anim.zbd").expect("a relative path");
    let survey = survey_cinematic_fighters(&path, &bytes, "generic_intro").expect("the closure");

    assert_eq!(PIRATE_FIGHTER_NODE, "piratefighter");
    // The records that place the pirate fighters.
    assert_eq!(
        survey.with_fighter_node,
        ["generic_intro", "gi_scene1", "gi_pfighter1", "gi_pfighter2"]
    );
    for record in [
        "gi_scene1",
        "gi_scene2",
        "gi_pfighter1",
        "gi_playerdrop",
        "gi_pfighter2",
    ] {
        assert!(
            survey.reached.iter().any(|r| r == record),
            "{record} is reached"
        );
    }
    assert_eq!(survey.called_elsewhere, ["open_pzeplaunchdoors"]);

    // What the whole closure can do: the one gameplay-marker opcode is
    // CALLBACK; no event is a weapon, damage or AI statement.
    let markers: Vec<&str> = survey
        .opcodes
        .values()
        .filter(|o| o.class == EventClass::Marker)
        .map(|o| o.statement)
        .collect();
    assert_eq!(markers, ["CALLBACK"]);
    assert!(!survey.uses_class(EventClass::Unknown));
    let statements: Vec<&str> = survey.opcodes.values().map(|o| o.statement).collect();
    assert!(
        !statements.iter().any(|s| s.contains("FIRE")
            || s.contains("DAMAGE")
            || s.contains("WEAPON")
            || s.contains("AI")),
        "unexpected statements {statements:?}"
    );

    // `piratefighter` is the `nodename` of the Devastator, which the player's
    // and Betty's airframes (and every `devastator` kin) also use, and no
    // `aiv.zrd` record of the mission is a pirate fighter.
    let aircraft = recover_retail_aircraft_records(&root, "zbd/c3/m01").expect("aiv.zrd reads");
    let names: Vec<&str> = aircraft.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "player",
            "medkestrel_1",
            "medkestrel_2",
            "wingman_1",
            "medkestrel_3"
        ]
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_outcome_attacker_retail_the_only_hostile_aircraft_records_start_asleep() {
    let root = root();
    let aircraft = recover_retail_aircraft_records(&root, "zbd/c3/m01").expect("aiv.zrd reads");
    let player = aircraft
        .iter()
        .find(|a| a.name == "player")
        .expect("player");
    let player_team = player.team;
    let hostile: Vec<_> = aircraft
        .iter()
        .filter(|a| a.name != "player" && a.team != player_team)
        .collect();
    assert_eq!(
        hostile.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        ["medkestrel_1", "medkestrel_2", "medkestrel_3"]
    );
    for record in &hostile {
        assert_eq!(record.team, Some(2));
        assert_eq!(
            record.starts_asleep,
            Some(true),
            "{} starts asleep",
            record.name
        );
    }
    let wingman = aircraft
        .iter()
        .find(|a| a.name == "wingman_1")
        .expect("wingman_1");
    assert_eq!(wingman.team, player_team);
    assert_eq!(wingman.starts_asleep, Some(false));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_outcome_attacker_retail_activation_set_in_the_first_seventy_seconds() {
    let root = root();
    let graph = objectives_graph(&root);

    // Time-driven wakes: exactly OBJECTIVE1, at mission-clock 2.0 s; the site
    // it carries wakes the Pandora's turrets, not an aircraft.
    let timed: Vec<(u32, f32)> = graph
        .objectives()
        .iter()
        .filter_map(|o| o.timed_wake_s().map(|s| (o.number, s)))
        .collect();
    assert_eq!(timed, [(1, 2.0)]);
    let first = graph.get(1).expect("OBJECTIVE1");
    assert_eq!(first.wakeup_zep_turrets, ["piratezep"]);
    assert!(first.wakeup_enemies.is_empty() && first.wakeup_turrets.is_empty());

    // The only sites that wake an aircraft: OBJECTIVE23 (the three Kestrels).
    let enemies: Vec<(u32, Vec<String>)> = graph
        .objectives()
        .iter()
        .filter(|o| !o.wakeup_enemies.is_empty())
        .map(|o| (o.number, o.wakeup_enemies.clone()))
        .collect();
    assert_eq!(
        enemies,
        [
            (
                23,
                vec![
                    "medkestrel_1".to_owned(),
                    "medkestrel_2".to_owned(),
                    "medkestrel_3".to_owned()
                ]
            ),
            (39, vec!["cargozep1".to_owned()]),
        ]
    );
    for name in ["medkestrel_1", "medkestrel_2", "medkestrel_3"] {
        assert_eq!(graph.blocks_waking(name), [23]);
    }

    // OBJECTIVE23 is reached only through 22 <- 21 <- 27 <- 12; the one root
    // that starts awake is 12 (the shipwreck), not 2 (the chamber).
    let ancestors = graph.ancestors(23);
    assert_eq!(ancestors.into_iter().collect::<Vec<_>>(), [12, 21, 22, 27]);
    for number in [21, 22, 23, 27] {
        assert!(!graph.get(number).expect("block").awake_at_start());
    }
    let root_block = graph.get(12).expect("OBJECTIVE12");
    assert!(root_block.awake_at_start());
    let travelers = root_block.travelers.clone().expect("a TRAVELERS condition");
    assert_eq!(travelers.target, TravelTarget::Node("shipwreck".to_owned()));
    assert_eq!(travelers.radius, 200.0);
    assert!(
        graph.get(21).expect("OBJECTIVE21").has_anim_state,
        "21 also needs `texdrop` executed"
    );

    // No awake-at-start objective's TRAVELERS condition is met at the stored
    // start (-1426, 150, -1813): each target is farther than its radius.
    let targets = bind_mission_targets(&root, "zbd/c3/m01", "zbd/c3");
    let start = [-1426.0_f64, 150.0, -1813.0];
    let mut checked = 0;
    for o in graph.objectives().iter().filter(|o| o.awake_at_start()) {
        let Some(t) = &o.travelers else { continue };
        let at = match &t.target {
            TravelTarget::Point(p) => *p,
            TravelTarget::Node(n) => match targets.targets.iter().find(|b| &b.node == n) {
                Some(b) => b.position_m,
                None => continue, // `piratezep` is a zeppelin record, dormant objective
            },
        };
        let d =
            ((at[0] - start[0]).powi(2) + (at[1] - start[1]).powi(2) + (at[2] - start[2]).powi(2))
                .sqrt();
        assert!(
            d > f64::from(t.radius),
            "OBJECTIVE{} would already be met ({d} m)",
            o.number
        );
        checked += 1;
    }
    assert!(checked >= 4, "the conditions this checks: {checked}");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_outcome_attacker_retail_medusa_net_patrols_the_wreck() {
    let root = root();
    // `ne000001.zrd` is net 1 `M1Medusas` (`neindex.zrd`); its waypoints loop
    // around the shipwreck, 7+ km from the start.
    let (index, _) = member_document(&root, "zbd/c3/zrdr.zbd", "neindex.zrd");
    let flat = format!("{index:?}");
    assert!(flat.contains("M1Medusas"));
    let (net, _) = member_document(&root, "zbd/c3/zrdr.zbd", "ne000001.zrd");
    let rows = net.as_list().expect("a list");
    let points = rows[0]
        .as_list()
        .and_then(|row| {
            row.iter()
                .find_map(|v| v.as_list().filter(|l| l.len() == 10))
        })
        .expect("ten waypoints");
    let wreck = [-8891.4_f32, 5.0, -4685.45];
    for p in points {
        let c: Vec<f32> = p
            .as_list()
            .expect("a point")
            .iter()
            .map(|v| match v {
                ZrdValue::Float(x) => *x,
                other => panic!("{other:?}"),
            })
            .collect();
        let d = ((c[0] - wreck[0]).powi(2) + (c[2] - wreck[2]).powi(2)).sqrt();
        assert!(d < 600.0, "waypoint {c:?} is {d} m from the wreck");
    }
}
