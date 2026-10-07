//! F46-B: the target display is the published `HudTargetReadout` verbatim,
//! shown only while the `TargetConsumers` are bound to this observer — the
//! previous aircraft's target is never drawn.

use bevy::ecs::world::World;

use cs_app::scene::SceneGeneration;
use cs_app::targeting::{
    TargetConsumers, TargetableBinding, TargetableState, TargetingSession, apply_selection_edges,
    apply_target_consumers, lower_rules, lower_selection_actions, sync_targetable_roster,
};
use cs_app::ui::hud::{Hud, HudSources};
use cs_content::hud::HudPolicy;
use cs_content::target_rules::{
    declared_synthetic_selection_actions, declared_synthetic_target_rules,
};
use cs_sim::targeting::{SelectionFrame, TargetClass};
use cs_types::Tick;
use cs_types::input::{Action, FlightCommand};
use cs_types::space::WorldPosition;

use crate::{actor, level_sample, session};

/// The generation the roster, the published views and the HUD all share.
const TARGET_SESSION: u64 = 7;

fn pos(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

/// A world with the targeting session and a two-actor roster: the observer
/// at the origin and one raider.
fn bound_world(generation: SceneGeneration) -> World {
    let declared = declared_synthetic_target_rules();
    let rules = lower_rules(&declared).expect("the fixture rules lower");
    let bindings = lower_selection_actions(&declared_synthetic_selection_actions())
        .expect("the fixture actions lower");
    let mut world = World::new();
    world.insert_resource(TargetingSession::new(
        session(TARGET_SESSION),
        rules,
        bindings,
        declared.subject().clone(),
        generation,
    ));
    for (serial, faction, position) in [
        (
            1,
            cs_sim::targeting::synthetic_player_faction(),
            [0.0, 0.0, 0.0],
        ),
        (
            9,
            cs_sim::targeting::synthetic_raider_faction(),
            [100.0, 0.0, 0.0],
        ),
    ] {
        let mut state = TargetableState::aircraft(faction, pos(position));
        state.class = TargetClass::Aircraft;
        world.spawn((
            TargetableBinding {
                actor: actor(TARGET_SESSION, serial),
                rules: declared.subject().clone(),
                generation,
            },
            state,
        ));
    }
    world
}

fn targets_only(consumers: &TargetConsumers) -> HudSources<'_> {
    HudSources {
        targets: Some(consumers),
        ..HudSources::default()
    }
}

/// The frame's target is the very record `apply_target_consumers` published:
/// the reticle names the selected actor, verbatim, no re-shaping.
#[test]
fn accept_f46_b_the_target_display_is_the_published_readout() {
    let mut world = bound_world(SceneGeneration::default().next());
    sync_targetable_roster(&mut world);
    let report = apply_selection_edges(
        &mut world,
        actor(TARGET_SESSION, 1),
        &[Action::Flight(FlightCommand::TargetNext)],
        SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    let selected = report.selection.expect("a raider is selected");
    apply_target_consumers(&mut world, actor(TARGET_SESSION, 1), Tick(21)).expect("registered");
    let consumers = world.resource::<TargetConsumers>().clone();

    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(TARGET_SESSION), actor(TARGET_SESSION, 1))
        .unwrap();
    let frame = hud
        .frame(&level_sample(TARGET_SESSION, 1), &targets_only(&consumers))
        .unwrap();
    let readout = frame
        .target
        .as_ref()
        .expect("the published view is the HUD's");
    assert_eq!(
        readout.reticle.as_ref().map(|reticle| reticle.target),
        Some(selected),
        "the reticle names the authority's selection"
    );
    // The frame carries the record verbatim, not a re-derived copy.
    assert_eq!(frame.target.as_ref(), consumers.hud());
}

/// A `TargetConsumers` published for another observer is never this HUD's
/// target — the swap cannot leak the previous aircraft's reticle.
#[test]
fn accept_f46_b_another_observers_views_are_never_drawn() {
    let mut world = bound_world(SceneGeneration::default().next());
    sync_targetable_roster(&mut world);
    apply_selection_edges(
        &mut world,
        actor(TARGET_SESSION, 9),
        &[Action::Flight(FlightCommand::TargetNext)],
        SelectionFrame::at(Tick(20)),
    )
    .expect("registered");
    apply_target_consumers(&mut world, actor(TARGET_SESSION, 9), Tick(21)).expect("registered");
    let consumers = world.resource::<TargetConsumers>().clone();
    assert!(
        consumers.hud().is_some(),
        "a view is published — for the other observer"
    );

    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(TARGET_SESSION), actor(TARGET_SESSION, 1))
        .unwrap();
    let frame = hud
        .frame(&level_sample(TARGET_SESSION, 1), &targets_only(&consumers))
        .unwrap();
    assert!(
        frame.target.is_none(),
        "the other observer's view is never this HUD's"
    );

    // And with nothing published at all there is nothing to draw.
    let mut cleared = consumers.clone();
    cleared.clear();
    let frame = hud
        .frame(&level_sample(TARGET_SESSION, 1), &targets_only(&cleared))
        .unwrap();
    assert!(frame.target.is_none());
}
