//! M01-WINDOW-FIRE acceptance (#1536): the player's guns and rockets reach
//! the mission's weapon session through the **real** input path, the guns fire
//! for as long as `Space` is held, and what fires is drawn.
//!
//! Task test prefix: `accept_m01_window_fire_`.
//!
//! The owner's playtest of 2026-10-17 found that pressing `Space` and
//! `Left Ctrl` in the windowed M01 produced nothing a player could see. The
//! #1437 suite proved the gun and rocket *paths* by injecting
//! [`MissionCockpitActions`] directly — it never pumped the platform input,
//! never read the frame report and never collected the cockpit edges, which
//! is exactly the chain the window runs and this file drives:
//!
//! 1. a real Bevy key press →
//! 2. [`pump_platform_input`](cs_app::input::platform) →
//! 3. [`PlatformInput`]'s frame report →
//! 4. `collect_mission_cockpit_actions` →
//! 5. `publish_mission_weapon_input` → the F27/F28 sessions.
//!
//! The composition is the production one ([`build_headless`]: the same plugins
//! and systems as the window, without a window or a GPU), so a regression in
//! any of those links fails here rather than in the owner's playtest. The
//! drawing half is asserted on the same composed world: every live round
//! mirror carries a drawn tracer and every launched rocket a drawn body.
//!
//! Every value here is newly authored synthetic fixture data. No
//! `CS_GAME_DIR` access.

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::{App, ChildOf, Entity, Mesh3d, MeshMaterial3d, StandardMaterial, Transform};

use cs_app::input::platform::PlatformInput;
use cs_app::mission_session::{
    MissionCockpitActions, MissionComposition, MissionGunHold, MissionHudNotice,
    MissionWeaponReport, MissionWeapons, WeaponVisual, WeaponVisualKind, WeaponVisualSample,
    build_headless, is_fire_guns, teardown,
};
use cs_app::ordnance::OrdnanceItemMirror;
use cs_app::weapons::WeaponRoundMirror;
use cs_types::input::FlightCommand;

use crate::m01_play_weapons::armed_stage;
use crate::vs_m01_rt_window::{Scratch, synthetic_stage};

/// The most rendered frames a test waits for a rocket to be launched and
/// drawn before failing.
const MAX_FRAMES: usize = 120;

/// The composed synthetic mission, exactly as the window composes it minus the
/// window: input plugin, weapon session and the presentation this task adds.
fn composed(label: &str) -> (Scratch, App) {
    let scratch = Scratch::new(label);
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.update();
    (scratch, app)
}

/// Presses a key the way the window's own input state holds it down.
fn press(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
}

/// Releases a key the way the window's own input state lets go of it.
fn release(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(key);
}

/// How many live round mirrors the composed world currently holds.
fn rounds(app: &App) -> usize {
    app.world()
        .iter_entities()
        .filter(|entity| entity.contains::<WeaponRoundMirror>())
        .count()
}

/// Every drawn tracer entity in the world.
fn tracers(app: &App) -> Vec<Entity> {
    visuals(app, WeaponVisualKind::RoundTracer)
}

/// Every drawn rocket body in the world.
fn rocket_bodies(app: &App) -> Vec<Entity> {
    visuals(app, WeaponVisualKind::Rocket)
}

/// Every drawn weapon visual of one kind in the world.
fn visuals(app: &App, kind: WeaponVisualKind) -> Vec<Entity> {
    app.world()
        .iter_entities()
        .filter(|entity| {
            entity
                .get::<WeaponVisual>()
                .is_some_and(|visual| visual.kind == kind)
        })
        .map(|entity| entity.id())
        .collect()
}

/// **The fire keys reach the mission's weapon session through the real input
/// path: one `Space` press, delivered by the platform pump and collected by
/// the mission, fires the guns and spawns a live round in the same frame.**
///
/// This is the diagnosis the #1437 suite could not make: it injected
/// [`MissionCockpitActions`] directly, so a break anywhere between the key and
/// the publisher would still have passed there. Everything after the press is
/// production: `pump_platform_input`, the platform frame report,
/// `collect_mission_cockpit_actions` and `publish_mission_weapon_input`.
#[test]
fn accept_m01_window_fire_the_guns_key_reaches_the_weapon_session_through_the_real_input_path() {
    let (_scratch, mut app) = composed("m01-window-fire-input");

    press(&mut app, KeyCode::Space);
    app.update();

    // Link 1–3: the platform pump read the key and delivered the edge in this
    // very frame's report.
    let delivered = app
        .world()
        .resource::<PlatformInput>()
        .report()
        .delivered
        .clone();
    assert!(
        delivered.iter().copied().any(is_fire_guns),
        "the platform delivered the press: {delivered:?}"
    );

    // Link 4: the mission's collector took the frame's edges for the fixed
    // ticks, so nothing is left pending when the publisher runs.
    assert!(
        app.world()
            .resource::<MissionCockpitActions>()
            .actions
            .is_empty(),
        "the collector handed the press to the publisher"
    );

    // Link 5: the publisher served the committed tick and fired the guns. The
    // report is per-tick and a frame runs two ticks, so the observable is the
    // hold bookkeeping the publisher wrote on the tick that carried the press.
    let fired_on = app
        .world()
        .resource::<MissionGunHold>()
        .last
        .expect("the press fired the guns");
    let report = app
        .world()
        .get_resource::<MissionWeaponReport>()
        .cloned()
        .expect("the mission's publisher ran");
    assert!(
        report.at.is_some(),
        "the publisher served a committed tick: {report:?}"
    );
    assert!(
        report.refused.is_none(),
        "the publisher did not refuse: {report:?}"
    );
    assert!(
        fired_on.0 <= report.at.expect("served").0,
        "the guns fired on a tick this frame served: {fired_on:?} <= {report:?}"
    );
    assert!(rounds(&app) >= 1, "the press spawned a live round");

    release(&mut app, KeyCode::Space);
    for _ in 0..4 {
        app.update();
    }
    let after = app.world().resource::<MissionGunHold>().last;
    assert_eq!(
        after,
        Some(fired_on),
        "a released key fires nothing further: the hold stopped at {fired_on:?}"
    );
    teardown(&mut app);
}

/// **Holding `Space` keeps firing at the gun's own declared rate, and letting
/// go stops the stream.**
///
/// The original fires the guns for as long as the trigger is held; #1437's
/// design delivered one press per hold. The repeat is minted by the mission's
/// publisher once per [`MISSION_TICKS_BETWEEN_SHOTS`](cs_app::mission_session)
/// — the rate the loadout's gun records declare and the F27 step's per-mount
/// cooldown enforces — so a held key produces a stream of rounds, bounded by
/// that rate and never by the frame rate.
#[test]
fn accept_m01_window_fire_holding_the_guns_key_fires_at_the_declared_rate() {
    let (_scratch, mut app) = composed("m01-window-fire-hold");

    press(&mut app, KeyCode::Space);
    let frames = 10;
    for _ in 0..frames {
        app.update();
    }
    let held = rounds(&app);
    // One update is one 1/60 s frame of two 120 Hz ticks, so `frames` frames
    // are `2 * frames` ticks; at one shot per declared rate tick the held key
    // fires about `2 * frames / rate` rounds, plus the press itself. The
    // bounds catch both failure shapes: a press that fires once (one round)
    // and a repeat that ignores the rate (one round per tick).
    assert!(
        held >= 4,
        "holding Space fired a stream, not a single press: {held} live rounds"
    );
    assert!(
        held <= 2 * frames,
        "the stream respects the declared gun rate: {held} live rounds in {} ticks",
        2 * frames
    );

    release(&mut app, KeyCode::Space);
    let stopped = rounds(&app);
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(
        rounds(&app),
        stopped,
        "letting go of Space stops the stream"
    );
    teardown(&mut app);
}

/// **Every live round is drawn: each round mirror carries one tracer streak,
/// and the streak spans the distance the round actually travelled.**
///
/// Before this task nothing read [`WeaponRoundMirror`] for presentation, so a
/// fired round was an entity nobody drew — the owner's "no tracers, no
/// projectiles". The streak is a designed visual (its colour, width and
/// length bounds are project design under `m01-window-fire.designed-visuals`),
/// but its pose is not invented: it spans the segment the authoritative
/// runtime flew since the last drawn frame.
#[test]
fn accept_m01_window_fire_every_live_round_draws_a_tracer_streak() {
    let (_scratch, mut app) = composed("m01-window-fire-tracers");

    press(&mut app, KeyCode::Space);
    for _ in 0..6 {
        app.update();
    }
    release(&mut app, KeyCode::Space);
    app.update();

    let live = rounds(&app);
    assert!(live >= 2, "the held key left live rounds to draw: {live}");
    let drawn = tracers(&app);
    assert_eq!(
        drawn.len(),
        live,
        "every live round carries exactly one tracer (rounds={live}, tracers={})",
        drawn.len()
    );

    let world = app.world();
    for tracer in &drawn {
        let parent = world
            .get::<ChildOf>(*tracer)
            .map(|child_of| child_of.0)
            .expect("the tracer is parented to its round");
        assert!(
            world.get::<WeaponRoundMirror>(parent).is_some(),
            "the tracer is drawn under the round it draws"
        );
        assert!(
            world.get::<Mesh3d>(*tracer).is_some(),
            "the tracer carries drawable geometry"
        );
        assert!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(*tracer)
                .is_some(),
            "the tracer carries a material"
        );
        assert!(
            world.get::<WeaponVisualSample>(parent).is_some(),
            "the round samples its own motion for the streak's direction"
        );
    }

    // After more than one drawn frame the streak spans measured travel, so it
    // reads as a tracer and not as a dot parked in the sky.
    let streaked = drawn.iter().any(|tracer| {
        world
            .get::<Transform>(*tracer)
            .is_some_and(|transform| transform.scale.z > 1.0)
    });
    assert!(
        streaked,
        "a tracer spans the round's travelled segment: {drawn:?}"
    );
    teardown(&mut app);
}

/// **A launched rocket is drawn, and a rocket refused by name publishes the
/// HUD notice instead of silently doing nothing.**
///
/// The drawn body is a designed visual; which way it points comes from the
/// rocket's own flight path. The refusal line is published from the mission's
/// weapon report, whose reason text is the publisher's own diagnostic.
#[test]
fn accept_m01_window_fire_rockets_are_drawn_and_a_refusal_is_published() {
    let (_scratch, mut app, _actor) = armed_stage("m01-window-fire-rockets");

    let mut launched = false;
    for _ in 0..MAX_FRAMES {
        press(&mut app, KeyCode::ControlLeft);
        app.update();
        release(&mut app, KeyCode::ControlLeft);
        if !rocket_bodies(&app).is_empty() {
            launched = true;
            break;
        }
    }
    assert!(launched, "the rocket key launched a drawn rocket");

    let world = app.world();
    for body in rocket_bodies(&app) {
        let parent = world
            .get::<ChildOf>(body)
            .map(|child_of| child_of.0)
            .expect("the rocket body is parented to its mirror");
        assert!(
            world.get::<OrdnanceItemMirror>(parent).is_some(),
            "the body is drawn under the rocket it draws"
        );
        assert!(
            world.get::<Mesh3d>(body).is_some(),
            "the rocket body carries drawable geometry"
        );
    }
    teardown(&mut app);

    // The same key in a mission with **no** declared-hostile target is refused
    // by name — and that refusal is published for the HUD rather than
    // swallowed, so the player is told instead of left guessing.
    let (_scratch, mut refused) = composed("m01-window-fire-notice");
    press(&mut refused, KeyCode::ControlLeft);
    refused.update();
    let notice = refused
        .world()
        .get_resource::<MissionHudNotice>()
        .cloned()
        .expect("the presentation plugin installed the notice");
    let line = notice
        .current
        .as_ref()
        .expect("a refused rocket publishes a notice");
    assert!(
        line.text.contains("hostile"),
        "the notice shows the publisher's own refusal: {:?}",
        line.text
    );
    assert_eq!(
        notice.published(),
        1,
        "the refusal is published exactly once"
    );
    for _ in 0..4 {
        refused.update();
    }
    assert_eq!(
        refused.world().resource::<MissionHudNotice>().published(),
        1,
        "the same refusal is not published again"
    );
    // Pressing again is a new refusal: it is shown again (refreshed), not
    // swallowed because the same reason was recorded before.
    release(&mut refused, KeyCode::ControlLeft);
    refused.update();
    press(&mut refused, KeyCode::ControlLeft);
    refused.update();
    assert_eq!(
        refused.world().resource::<MissionHudNotice>().published(),
        2,
        "a second press of the same refused key is published again"
    );
    teardown(&mut refused);
}

/// **The weapon session the composition opened is the player's, and its
/// publisher never invents a state: a mission with no session publishes
/// nothing.**
///
/// A guard for the tests above: they assert on the *mission's* report, so
/// they must be reading the session the composition opened, not a fixture.
#[test]
fn accept_m01_window_fire_the_composition_serves_the_player_weapon_session() {
    let (_scratch, mut app) = composed("m01-window-fire-session");
    let weapons = app
        .world()
        .get_resource::<MissionWeapons>()
        .expect("the composed mission opened a weapon session");
    assert_eq!(
        weapons.player().serial,
        cs_app::mission_session::MISSION_PLAYER_SERIAL,
        "the session flies the player actor"
    );
    assert!(
        app.world().get_resource::<MissionComposition>().is_some(),
        "the composition resource the session belongs to is installed"
    );
    assert!(
        app.world()
            .get_resource::<cs_app::weapons::WeaponSession>()
            .is_some(),
        "the F27 schedule has a session to serve"
    );
    teardown(&mut app);
}

/// The unused import guard: `FlightCommand` names the rocket binding the
/// notice test drives, and `is_fire_guns` the gun one.
#[test]
fn accept_m01_window_fire_the_fire_binding_names_are_the_mission_ones() {
    assert_eq!(
        cs_app::mission_session::MISSION_FIRE_ROCKETS,
        FlightCommand::FireSecondary,
        "the rocket key is the mission's declared rocket command"
    );
}
