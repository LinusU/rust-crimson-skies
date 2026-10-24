//! Acceptance suite M01-TARGET-ROSTER (Rally #1592): M01's bound ground
//! targets are rostered in the F30 targeting store under the faction the
//! engine's node resolver answers for them.
//!
//! `docs/findings/2026-10-18-m01-target-roster.md` records the measurement:
//! `targets.zrd` and the world records state no team, but the loader's node
//! resolver (`0x4bf080` → `0x4bef90`, measured by #1155/#1177) answers one for
//! every subtree — a staged marked-node lane, then an `ai.zrd` `TURRET`
//! binding, then the children, else `0` (`neutral`).
//!
//! 1. **Retail — the resolver's answer for the convoy.** The three gun trucks
//!    resolve to `enemy` through `ai.zrd`'s `t_truck*` turret binding;
//!    `bridge_truck01` through a marked node; the wreck to `neutral`.
//! 2. **Retail — the composed mission rosters them.** The targeting store
//!    holds each convoy truck under `enemy`, class `WorldObject`, and the
//!    player's flight holds the designed relation toward it.
//! 3. **Synthetic (CI) — the roster decision.** An open resolver answer rosters
//!    nothing, a `neutral` node gets no invented hostility, and an enemy node
//!    is hostile to a player of another team.
//!
//! Nothing here is `verified_original`: the *reading* of a team as a relation
//! is the designed one of `#1492`, and no original executable ran.

use std::path::PathBuf;

use cs_app::mission_actors::{
    MissionGroundTargetReport, MissionGroundTargetRosterReport, ground_target_roster,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{build_headless, stage_for};
use cs_app::mission_targets::{BoundTarget, DamageCoupling, bind_mission_targets};
use cs_app::mission_world_actors::{
    AllegianceBinding, AllegianceSource, resolve_world_node_allegiance,
};
use cs_app::targeting::TargetingSession;
use cs_sim::targeting::{Allegiance, TargetClass};

use crate::campaign::common::label;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

fn target_with(allegiance: AllegianceBinding, objective: bool) -> BoundTarget {
    BoundTarget {
        declared: Some(0),
        chain: vec!["t_truck01".to_owned()],
        node: "t_truck01".to_owned(),
        index: 1,
        path: "world1.t_truck01".to_owned(),
        position_m: [1.0, 2.0, 3.0],
        target_point_m: [1.0, 2.0, 3.0],
        target_half_extents_m: [0.0; 3],
        healthy: None,
        destroyed: None,
        intact_slots: Vec::new(),
        destroyed_slots: Vec::new(),
        coupling: DamageCoupling::default(),
        objective,
        other_target: false,
        description: None,
        help_label: None,
        allegiance,
        frame: None,
    }
}

fn measured(team: i64) -> AllegianceBinding {
    AllegianceBinding::Measured {
        team,
        source: AllegianceSource::NoBinding,
    }
}

/// **The resolver answers a team for the convoy, the wreck and the camps.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_target_roster_the_resolver_answers_the_convoy_and_the_wreck() {
    let root = game_dir();
    let binding = bind_mission_targets(&root, "zbd/c3/m01", "zbd/c3");
    for node in ["t_truck01", "t_truck02", "t_truck03", "bridge_truck01"] {
        let target = binding
            .target(node)
            .unwrap_or_else(|| panic!("{node} binds"));
        match &target.allegiance {
            AllegianceBinding::Measured { team, .. } => {
                assert_eq!(*team, 2, "{node} resolves to the enemy team");
            }
            other => panic!("{node}'s allegiance is measured, found {other:?}"),
        }
    }
    assert!(matches!(
        binding.target("t_truck01").unwrap().allegiance,
        AllegianceBinding::Measured {
            source: AllegianceSource::TurretBinding { .. },
            ..
        }
    ));
    assert!(matches!(
        binding.target("bridge_truck01").unwrap().allegiance,
        AllegianceBinding::Measured {
            source: AllegianceSource::MarkedNode { .. },
            ..
        }
    ));
    assert_eq!(
        binding.target("shipwreck").unwrap().allegiance,
        measured_none(),
        "the wreck's subtree binds nothing: the resolver's terminal zero"
    );

    // The public resolver and the binding agree.
    let found = cs_assets::install::discover(&root).expect("the installation discovers");
    let direct =
        resolve_world_node_allegiance(&root, &found, "zbd/c3/m01", "zbd/c3", &["t_truck02"]);
    assert_eq!(direct[0].1, binding.target("t_truck02").unwrap().allegiance);
}

fn measured_none() -> AllegianceBinding {
    measured(0)
}

/// **The composed M01 rosters each convoy truck in the targeting store.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_target_roster_the_composed_mission_rosters_the_convoy() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    app.update();

    let targets = app.world().resource::<MissionGroundTargetReport>().clone();
    let roster = app
        .world()
        .resource::<MissionGroundTargetRosterReport>()
        .clone();
    assert!(
        !roster.rostered.is_empty(),
        "the pass rostered targets: {:?}",
        roster.unresolved
    );
    let targeting = app.world().resource::<TargetingSession>();
    let store = targeting.store();
    for node in ["t_truck01", "t_truck02", "t_truck03", "bridge_truck01"] {
        let bound = targets
            .target(node)
            .unwrap_or_else(|| panic!("{node} spawned"));
        let record = store
            .record(&bound.actor)
            .unwrap_or_else(|| panic!("{node} is in the targeting store"));
        assert_eq!(record.faction.key(), "enemy", "{node}'s measured faction");
        assert_eq!(record.class, TargetClass::WorldObject);
        assert_eq!(roster.faction_of(bound.actor), Some(&record.faction));
    }
    // The wreck is neutral: rostered, and no hostility is declared for it.
    let wreck = targets.target("shipwreck").expect("the wreck spawned");
    let record = store.record(&wreck.actor).expect("the wreck is rostered");
    assert_eq!(record.faction.key(), "neutral");
    assert_eq!(
        roster
            .rostered
            .iter()
            .find(|(actor, ..)| *actor == wreck.actor)
            .map(|(_, _, allegiance)| *allegiance),
        Some(None),
        "no relation is invented for a neutral node"
    );
}

/// **The roster decision, on synthetic targets.**
#[test]
fn accept_m01_target_roster_decision_follows_the_measured_team() {
    // Enemy node, ally player: rostered under `enemy`, hostile.
    let (roster, open) = ground_target_roster(&target_with(measured(2), true), Some(1));
    let roster = roster.expect("a measured team rosters");
    assert_eq!(open, None);
    assert_eq!(roster.faction.key(), "enemy");
    assert_eq!(roster.class, TargetClass::WorldObject);
    assert!(roster.objective, "the declaring row's objective flag rides");
    assert_eq!(roster.player_allegiance, Some(Allegiance::Hostile));

    // Same team as the player: friendly.
    let (roster, _) = ground_target_roster(&target_with(measured(1), false), Some(1));
    assert_eq!(
        roster.unwrap().player_allegiance,
        Some(Allegiance::Friendly)
    );

    // Neutral: rostered, but no hostility is invented.
    let (roster, _) = ground_target_roster(&target_with(measured(0), false), Some(1));
    let roster = roster.unwrap();
    assert_eq!(roster.faction.key(), "neutral");
    assert_eq!(roster.player_allegiance, None);

    // An unread player team states no side.
    let (roster, _) = ground_target_roster(&target_with(measured(2), false), None);
    assert_eq!(roster.unwrap().player_allegiance, None);

    // An open resolver answer, and a team outside the vocabulary, roster nothing.
    let (roster, open) = ground_target_roster(
        &target_with(AllegianceBinding::Open("no subtree".to_owned()), false),
        Some(1),
    );
    assert!(roster.is_none());
    assert_eq!(open.as_deref(), Some("no subtree"));
    let (roster, open) = ground_target_roster(&target_with(measured(3), false), Some(1));
    assert!(roster.is_none());
    assert!(open.unwrap().contains("team 3"));
}
