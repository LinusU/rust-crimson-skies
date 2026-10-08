//! F46-C: the in-flight `HudSession` — the cockpit frame, the map's
//! authored-geography and revealed-only contacts, the objectives and recon
//! pages, the mode-aware pause, and teardown/retry rebinding.

use bevy::ecs::world::World;

use cs_app::input::{PauseDecision, PauseReason, SessionMode};
use cs_app::objectives::{ObjectiveSession, lower_program};
use cs_app::scene::SceneGeneration;
use cs_app::targeting::{
    TargetableBinding, TargetableState, TargetingSession, apply_selection_edges,
    apply_target_consumers, lower_rules, lower_selection_actions, sync_targetable_roster,
};
use cs_app::ui::hud::{
    HudError, HudSession, HudSources, MissionPage, MissionSources, PageView, PauseCommand,
};
use cs_app::world::fixture::{self, arch_world, harbor_world, world_instance};
use cs_content::hud::HudPolicy;
use cs_content::objectives::declared_synthetic_objectives;
use cs_content::ordnance::{
    declared_synthetic_direct, declared_synthetic_flak, declared_synthetic_guided,
};
use cs_content::target_rules::{
    declared_synthetic_selection_actions, declared_synthetic_target_rules,
};
use cs_content::weapons::DeclaredGunMountKind;
use cs_content::world::{WorldObjectCondition, WorldObjectId};
use cs_script::runtime::SessionGeneration;
use cs_sim::damage::{AttributionRule, DamagePolicy, DamageResolver, synthetic_airframe_graph};
use cs_sim::targeting::{
    Allegiance, SelectionFrame, TargetClass, TargetStore, synthetic_allegiance_table,
    synthetic_roster, synthetic_target_policy,
};
use cs_sim::weapons::{GunBank, GunMountKind};
use cs_types::Tick;
use cs_types::input::{Action, FlightCommand};

use crate::{
    DAMAGE_PRODUCER, NOSE_MOUNT, WING_MOUNT, actor, declared_on, key, level_sample, ordnance_for,
    session, weapon_session,
};

/// The session generation every authority here stamps.
const GEN: u64 = 7;
/// The mount keys the swap's second aircraft carries.
const TAIL_MOUNT: &str = "tail_mount";
const GONDOLA_MOUNT: &str = "gondola_mount";

/// One `DamageResolver` for `s` with every serial's airframe graph — the
/// session has one damage authority describing every aircraft, so a swap
/// reads another actor's nodes out of the same resolver.
fn damage_resolver(s: u64, serials: &[u64]) -> DamageResolver {
    let mut damage = DamageResolver::new(session(s), DAMAGE_PRODUCER);
    for serial in serials {
        damage
            .register_actor(
                actor(s, *serial),
                synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("the fixture graph registers");
    }
    damage
}

/// The synthetic roster registered in a real `TargetStore` for `s`.
fn roster(s: u64) -> TargetStore {
    let mut store = TargetStore::new(s, synthetic_target_policy(), synthetic_allegiance_table());
    for record in synthetic_roster(s) {
        store
            .register(record)
            .expect("the fixture roster registers");
    }
    store
}

/// The synthetic objective program launched into a real session: its
/// `Immediate` primary is born revealed, the secondary waits on a signal.
fn objectives() -> ObjectiveSession {
    ObjectiveSession::launch(
        lower_program(&declared_synthetic_objectives()).expect("the fixture program lowers"),
        SessionGeneration(1),
    )
    .expect("the fixture program launches")
}

/// A two-actor world with a targeting session and a published
/// `TargetConsumers` — the same fixture F46-B's target tests drive.
fn consumer_world() -> World {
    let declared = declared_synthetic_target_rules();
    let rules = lower_rules(&declared).expect("the fixture rules lower");
    let bindings = lower_selection_actions(&declared_synthetic_selection_actions())
        .expect("the fixture actions lower");
    let mut world = World::new();
    world.insert_resource(TargetingSession::new(
        session(GEN),
        rules,
        bindings,
        declared.subject().clone(),
        SceneGeneration::default().next(),
    ));
    for (serial, faction) in [
        (1, cs_sim::targeting::synthetic_player_faction()),
        (9, cs_sim::targeting::synthetic_raider_faction()),
    ] {
        world.spawn((
            TargetableBinding {
                actor: actor(GEN, serial),
                rules: declared.subject().clone(),
                generation: SceneGeneration::default().next(),
            },
            {
                let mut state = TargetableState::aircraft(
                    faction,
                    cs_types::space::WorldPosition::try_new([0.0, 0.0, 0.0])
                        .expect("the fixture position is finite"),
                );
                state.class = TargetClass::Aircraft;
                state
            },
        ));
    }
    world
}

/// The minimum scenario, AC02's swap half: rebinding to another aircraft
/// rebinds every instrument and gauge — the previous aircraft's mounts,
/// rounds and launchers are never drawn.
#[test]
fn accept_f46_c_aircraft_swap_rebinds_instruments_and_gauges() {
    // Actor 1 flies a nose+wing ship with a direct-fire launcher; actor 2 a
    // tail+gondola ship with flak and guided racks. One authority holds
    // both, so the swap re-reads, never carries.
    let mut weapons = weapon_session(
        GEN,
        1,
        &[
            (NOSE_MOUNT, DeclaredGunMountKind::Nose),
            (WING_MOUNT, DeclaredGunMountKind::WingLeft),
        ],
        &[NOSE_MOUNT],
        30,
    );
    let b_guns: Vec<_> = [
        (TAIL_MOUNT, DeclaredGunMountKind::Tail),
        (GONDOLA_MOUNT, DeclaredGunMountKind::Gondola),
    ]
    .iter()
    .map(|(mount, kind)| declared_on(mount, *kind))
    .collect();
    weapons
        .register(
            actor(GEN, 2),
            &b_guns,
            GunBank::try_new([key(TAIL_MOUNT), key(GONDOLA_MOUNT)]).expect("a valid bank"),
            5,
        )
        .expect("the second arsenal registers");
    let mut ordnance = ordnance_for(GEN, 1, &[declared_synthetic_direct()]);
    ordnance
        .register(
            actor(GEN, 2),
            &[declared_synthetic_flak(), declared_synthetic_guided()],
        )
        .expect("the second loadout registers");
    let damage = damage_resolver(GEN, &[1, 2]);

    let mut hud = HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).unwrap();
    hud.bind(session(GEN), actor(GEN, 1)).unwrap();
    let sources = MissionSources {
        hud: HudSources {
            weapons: Some(&weapons),
            ordnance: Some(&ordnance),
            damage: Some(&damage),
            ..HudSources::default()
        },
        ..MissionSources::default()
    };

    let PageView::Cockpit(frame) = hud.view(&level_sample(GEN, 1), &sources).unwrap() else {
        panic!("a bound session opens on the cockpit page");
    };
    assert_eq!(frame.instruments.actor, actor(GEN, 1));
    let gauge = frame.weapons.expect("actor 1's arsenal is a gauge");
    assert_eq!(gauge.guns.len(), 2);
    assert_eq!(gauge.guns[0].kind, GunMountKind::Nose);
    assert_eq!(gauge.selected_rounds, 30);
    assert_eq!(
        frame.ordnance.expect("actor 1's launcher rows").len(),
        1,
        "the direct component only"
    );

    // The swap: the display rebinds, and the very next view is the new
    // aircraft's own cockpit.
    hud.bind(session(GEN), actor(GEN, 2)).unwrap();
    let PageView::Cockpit(frame) = hud.view(&level_sample(GEN, 2), &sources).unwrap() else {
        panic!("the cockpit page is still up after the swap");
    };
    assert_eq!(frame.instruments.actor, actor(GEN, 2));
    let gauge = frame.weapons.expect("actor 2's arsenal is a gauge");
    let kinds: Vec<GunMountKind> = gauge.guns.iter().map(|gun| gun.kind).collect();
    assert_eq!(kinds.len(), 2);
    for kind in [GunMountKind::Tail, GunMountKind::Gondola] {
        assert!(
            kinds.contains(&kind),
            "the new aircraft's {kind} mount is a row — the old nose/wing rows are gone"
        );
    }
    assert!(
        gauge.guns.iter().all(|gun| gun.selected && gun.rounds == 5),
        "the new ship's bank, not 30 rounds on a nose mount"
    );
    assert_eq!(gauge.selected_rounds, 10);
    assert_eq!(
        frame.ordnance.expect("actor 2's launcher rows").len(),
        2,
        "the flak and guided components"
    );
    assert!(
        frame
            .airframe
            .expect("actor 2's airframe panel")
            .iter()
            .all(|zone| zone.state == cs_sim::damage::PartState::Intact),
        "the panel is actor 2's own graph, freshly read"
    );

    // And the previous aircraft's sample is stale, on every page — the map
    // included.
    let err = hud.view(&level_sample(GEN, 1), &sources).unwrap_err();
    assert!(matches!(err, HudError::Stale { .. }), "{err}");
    hud.pause();
    let err = hud.view(&level_sample(GEN, 1), &sources).unwrap_err();
    assert!(
        matches!(err, HudError::Stale { .. }),
        "a stale sample cannot draw the map either: {err}"
    );
}

/// The map's geography is the load's own record: the definition's objects,
/// filtered to what this load activates, with the load's initial condition.
#[test]
fn accept_f46_c_map_draws_the_loads_active_geography() {
    let definition = arch_world().expect("the fixture world builds");
    // Two of the arch's nine objects are active, one authored damaged.
    let load = world_instance(
        &definition,
        None,
        &[fixture::OBJECT_LEG_LEFT, fixture::OBJECT_LINTEL],
        &[fixture::OBJECT_LINTEL],
    )
    .expect("the load record builds");
    load.validate_against(&definition)
        .expect("the fixture load validates");

    let mut hud = HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).unwrap();
    hud.bind(session(GEN), actor(GEN, 1)).unwrap();
    hud.pause();
    let view = hud
        .view(
            &level_sample(GEN, 1),
            &MissionSources {
                world: Some((&definition, &load)),
                ..MissionSources::default()
            },
        )
        .unwrap();
    let PageView::Map(map) = view else {
        panic!("pause opens the map");
    };
    let leg = WorldObjectId::new(fixture::OBJECT_LEG_LEFT).unwrap();
    let lintel = WorldObjectId::new(fixture::OBJECT_LINTEL).unwrap();
    assert_eq!(
        map.geography.len(),
        2,
        "only the load's active objects are marks — the other seven are not drawn"
    );
    let mark = |id: &WorldObjectId| map.geography.iter().find(|g| &g.object == id).unwrap();
    assert_eq!(mark(&leg).condition, WorldObjectCondition::Authored);
    assert_eq!(mark(&lintel).condition, WorldObjectCondition::Damaged);
    // Positions are the records' own authored translations.
    assert_eq!(
        mark(&leg).position,
        definition.object(&leg).unwrap().transform().translation()
    );
    // The pause menu the page offers is the front-end's own rows.
    assert_eq!(map.commands, PauseCommand::ALL);
    assert_eq!(map.pause, Some(PauseReason::PlayerRequest));
}

/// A hidden contact is never a map mark: the fixture roster's unrevealed
/// raider is registered and present, and still absent from the page.
#[test]
fn accept_f46_c_map_never_marks_an_unrevealed_contact() {
    let store = roster(GEN);
    let mut hud = HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).unwrap();
    hud.bind(session(GEN), actor(GEN, 1)).unwrap();
    hud.pause();

    let view = hud
        .view(
            &level_sample(GEN, 1),
            &MissionSources {
                roster: Some(&store),
                ..MissionSources::default()
            },
        )
        .unwrap();
    let PageView::Map(map) = view else {
        panic!("pause opens the map");
    };
    let marks: Vec<u64> = map.contacts.iter().map(|mark| mark.actor.serial).collect();
    // The synthetic roster: self 1, raiders 9/2/5, wingman 3, trader 4 and
    // the hidden raider 7 — which must not appear.
    assert_eq!(marks.len(), 5, "every revealed live actor is a mark");
    assert!(!marks.contains(&1), "the observer is not its own contact");
    assert!(!marks.contains(&7), "the unrevealed raider is never a mark");
    let contact = |serial| {
        map.contacts
            .iter()
            .find(|mark| mark.actor == actor(GEN, serial))
            .unwrap()
    };
    assert_eq!(contact(9).allegiance, Some(Allegiance::Hostile));
    assert_eq!(contact(4).allegiance, Some(Allegiance::Neutral));
    assert!(contact(4).objective, "the trader is flagged objective");
    assert_eq!(
        contact(3).allegiance,
        Some(Allegiance::Friendly),
        "a faction is Friendly to itself by the table's own rule"
    );

    // The player mark is the roster's own record plus the instruments'
    // heading.
    let player = map.player.expect("the bound actor is registered");
    assert_eq!(player.position.to_array(), [0.0, 0.0, 0.0]);
    assert_eq!(
        player.heading.map(|heading| heading.0),
        Some(0.0),
        "level flight on canonical forward heads 0"
    );
}

/// The objectives page and the map's objective strip are the mission's
/// event-driven display: the born-revealed primary is listed, the
/// signal-gated secondary is not.
#[test]
fn accept_f46_c_objective_rows_are_the_displays_visible_only() {
    let session_objectives = objectives();
    let display = session_objectives.display();
    assert_eq!(display.len(), 2, "the fixture declares two objectives");

    let mut hud = HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).unwrap();
    hud.bind(session(GEN), actor(GEN, 1)).unwrap();

    hud.open(MissionPage::Objectives);
    let PageView::Objectives(rows) = hud
        .view(
            &level_sample(GEN, 1),
            &MissionSources {
                objectives: Some(display),
                ..MissionSources::default()
            },
        )
        .unwrap()
    else {
        panic!("the objectives page is up");
    };
    // The page is `visible()` verbatim — the same rows, in the same order.
    assert_eq!(rows, display.visible());
    assert_eq!(rows.len(), 1, "the hidden secondary is never listed");

    // The map carries the same strip.
    hud.open(MissionPage::Map);
    let PageView::Map(map) = hud
        .view(
            &level_sample(GEN, 1),
            &MissionSources {
                objectives: Some(display),
                ..MissionSources::default()
            },
        )
        .unwrap()
    else {
        panic!("the map page is up");
    };
    assert_eq!(map.objectives, display.visible());
}

/// The recon page is the published spyglass readout for this observer — and
/// another observer's published view is never drawn.
#[test]
fn accept_f46_c_recon_page_is_the_bound_observers_spyglass() {
    let mut world = consumer_world();
    sync_targetable_roster(&mut world);
    let report = apply_selection_edges(
        &mut world,
        actor(GEN, 1),
        &[Action::Flight(FlightCommand::TargetNext)],
        SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    let selected = report.selection.expect("a raider is selected");
    apply_target_consumers(&mut world, actor(GEN, 1), Tick(21)).expect("published");
    let consumers = world
        .resource::<cs_app::targeting::TargetConsumers>()
        .clone();
    assert!(consumers.spyglass().is_some(), "a readout is published");

    let mut hud = HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).unwrap();
    hud.bind(session(GEN), actor(GEN, 1)).unwrap();
    hud.open(MissionPage::Recon);
    let view = hud
        .view(
            &level_sample(GEN, 1),
            &MissionSources {
                hud: HudSources {
                    targets: Some(&consumers),
                    ..HudSources::default()
                },
                ..MissionSources::default()
            },
        )
        .unwrap();
    let PageView::Recon(readout) = view else {
        panic!("the recon page is up");
    };
    // The page is the authority's own readout, verbatim.
    assert_eq!(readout, consumers.spyglass().cloned());
    assert_eq!(
        readout.and_then(|readout| readout.target.map(|t| t.actor)),
        Some(selected)
    );

    // A re-publish for another observer gives this session's page nothing.
    apply_target_consumers(&mut world, actor(GEN, 9), Tick(22)).expect("published");
    let foreign = world
        .resource::<cs_app::targeting::TargetConsumers>()
        .clone();
    let PageView::Recon(readout) = hud
        .view(
            &level_sample(GEN, 1),
            &MissionSources {
                hud: HudSources {
                    targets: Some(&foreign),
                    ..HudSources::default()
                },
                ..MissionSources::default()
            },
        )
        .unwrap()
    else {
        panic!("the recon page is still up");
    };
    assert_eq!(readout, None, "another observer's view is never drawn");
}

/// Single-player pause holds and releases; a networked session's pause is a
/// request the mode refuses — the map opens but the world keeps flying.
#[test]
fn accept_f46_c_pause_holds_singleplayer_never_the_server() {
    let mut solo = HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).unwrap();
    solo.bind(session(GEN), actor(GEN, 1)).unwrap();
    assert_eq!(solo.page(), MissionPage::Cockpit);

    let outcome = solo.pause();
    assert_eq!(outcome.page, MissionPage::Map);
    assert_eq!(
        outcome.pause,
        PauseDecision::Paused(PauseReason::PlayerRequest)
    );
    assert!(solo.is_paused());

    // Opening another menu page keeps the first pause's reason.
    let outcome = solo.open(MissionPage::Objectives);
    assert_eq!(
        outcome.pause,
        PauseDecision::AlreadyPaused(PauseReason::PlayerRequest)
    );
    assert_eq!(solo.pause_reason(), Some(PauseReason::PlayerRequest));

    let outcome = solo.resume();
    assert_eq!(outcome.page, MissionPage::Cockpit);
    assert_eq!(outcome.resumed, Some(PauseReason::PlayerRequest));
    assert!(!solo.is_paused());

    // Multiplayer: the same request is `NoLocalAuthority` — the surface
    // opens, nothing pauses, and the map says so.
    let mut networked = HudSession::new(SessionMode::Multiplayer, HudPolicy::designed()).unwrap();
    networked.bind(session(GEN), actor(GEN, 1)).unwrap();
    let outcome = networked.pause();
    assert_eq!(outcome.page, MissionPage::Map);
    assert_eq!(outcome.pause, PauseDecision::NoLocalAuthority);
    assert!(!networked.is_paused());
    let PageView::Map(map) = networked
        .view(&level_sample(GEN, 1), &MissionSources::default())
        .unwrap()
    else {
        panic!("the map page is up");
    };
    assert_eq!(map.pause, None, "no pause is held to display");
    assert_eq!(
        map.commands,
        PauseCommand::ALL,
        "the menu rows are still there — the world just keeps flying"
    );
}

/// Teardown unbinds everything the page held; retry tears down and rebinds
/// the next generation in one step, and a mismatched retry is refused
/// before anything is released.
#[test]
fn accept_f46_c_teardown_and_retry_rebind_the_next_generation() {
    let mut hud = HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).unwrap();
    hud.bind(session(GEN), actor(GEN, 1)).unwrap();
    hud.pause();
    assert_eq!(hud.page(), MissionPage::Map);

    // Retry mid-pause: the report names the old binding, page and pause,
    // and the display is the new generation's.
    let report = hud.retry(session(GEN + 1), actor(GEN + 1, 1)).unwrap();
    assert_eq!(report.bound, Some((session(GEN), actor(GEN, 1))));
    assert_eq!(report.page, MissionPage::Map);
    assert_eq!(report.pause, Some(PauseReason::PlayerRequest));
    assert_eq!(hud.bound(), Some((session(GEN + 1), actor(GEN + 1, 1))));
    assert_eq!(hud.page(), MissionPage::Cockpit);
    assert!(!hud.is_paused());

    // The new generation reads its own authority; the old one is refused.
    let weapons = weapon_session(
        GEN + 1,
        1,
        &[(NOSE_MOUNT, DeclaredGunMountKind::Nose)],
        &[NOSE_MOUNT],
        3,
    );
    let view = hud
        .view(
            &level_sample(GEN + 1, 1),
            &MissionSources {
                hud: HudSources {
                    weapons: Some(&weapons),
                    ..HudSources::default()
                },
                ..MissionSources::default()
            },
        )
        .unwrap();
    assert!(matches!(view, PageView::Cockpit(_)));
    let stale = weapon_session(
        GEN,
        1,
        &[(NOSE_MOUNT, DeclaredGunMountKind::Nose)],
        &[NOSE_MOUNT],
        3,
    );
    let err = hud
        .view(
            &level_sample(GEN + 1, 1),
            &MissionSources {
                hud: HudSources {
                    weapons: Some(&stale),
                    ..HudSources::default()
                },
                ..MissionSources::default()
            },
        )
        .unwrap_err();
    assert!(
        matches!(err, HudError::ForeignAuthority { source: "weapons", found, .. } if found == GEN),
        "{err}"
    );

    // A retry whose actor belongs to another session is refused before the
    // old display is touched.
    hud.pause();
    let err = hud.retry(session(GEN + 2), actor(GEN + 1, 1)).unwrap_err();
    assert!(matches!(err, HudError::ActorSessionMismatch), "{err}");
    assert_eq!(hud.bound(), Some((session(GEN + 1), actor(GEN + 1, 1))));
    assert_eq!(hud.page(), MissionPage::Map);
    assert!(hud.is_paused(), "the refused retry left the pause held");

    // And an explicit teardown leaves nothing to draw at all.
    let report = hud.teardown();
    assert_eq!(report.bound, Some((session(GEN + 1), actor(GEN + 1, 1))));
    assert_eq!(report.pause, Some(PauseReason::PlayerRequest));
    assert!(hud.bound().is_none());
    let err = hud
        .view(&level_sample(GEN + 1, 1), &MissionSources::default())
        .unwrap_err();
    assert!(matches!(err, HudError::Unbound), "{err}");
}

/// A load record of another world and a roster of another generation are
/// wiring faults the map refuses — never drawn as an empty page.
#[test]
fn accept_f46_c_foreign_world_and_roster_are_refused() {
    let arch = arch_world().expect("the fixture world builds");
    let harbor = harbor_world().expect("the fixture world builds");
    let load = world_instance(&harbor, None, &[fixture::HARBOR_OBJECT_GROUND], &[])
        .expect("the load record builds");

    let mut hud = HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).unwrap();
    hud.bind(session(GEN), actor(GEN, 1)).unwrap();
    hud.pause();

    let err = hud
        .view(
            &level_sample(GEN, 1),
            &MissionSources {
                world: Some((&arch, &load)),
                ..MissionSources::default()
            },
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            HudError::WorldMismatch {
                ref definition,
                ref load
            } if definition == arch.id() && load == harbor.id()
        ),
        "{err}"
    );

    let foreign = roster(GEN + 9);
    let err = hud
        .view(
            &level_sample(GEN, 1),
            &MissionSources {
                roster: Some(&foreign),
                ..MissionSources::default()
            },
        )
        .unwrap_err();
    assert!(
        matches!(err, HudError::ForeignAuthority { source: "roster", found, .. } if found == GEN + 9),
        "{err}"
    );
}
