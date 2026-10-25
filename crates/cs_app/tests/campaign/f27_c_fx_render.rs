//! F27-C-FX-RENDER acceptance (#1595): an accepted shot in the composed mission
//! draws a muzzle flash at its emitted muzzle position.
//!
//! Task test prefix: `accept_f27_c_fx_render_`.
//!
//! The scheduled presentation (#1304) spawns one `MuzzleEffectSpawn` record per
//! accepted shot; nothing drew it. These tests drive the production
//! composition ([`build_headless`], the window's own plugins minus the window)
//! with the real `Space` key and assert on the drawing the record resolves to:
//! its stand-in label, its once-only draw, its lifetime in committed fixed
//! ticks, the pause freeze and the session teardown.
//!
//! Since #2106 the record resolves to the measured muzzle burst instead of the
//! designed stand-in. These synthetic stages carry no retail art, so each test
//! installs a newly authored synthetic [`MuzzleBurstArt`] (one flash quad, three
//! spin quads, no textures); the retail art is exercised by the
//! `accept_f27_c_fx_orig_draw_c_` tests.
//!
//! Every value is newly authored synthetic fixture data. No `CS_GAME_DIR`.

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::{App, ChildOf, Children, Entity, Mesh3d};

use cs_app::input::platform::PlatformInput;
use cs_app::input::session::PauseReason;
use cs_app::mission_session::muzzle_burst_art::{BurstNodeArt, BurstQuad, MuzzleBurstArt};
use cs_app::mission_session::{
    MUZZLE_FLASH_LIFETIME_TICKS, MuzzleBurstArtResource, MuzzleDrawable, MuzzleFlash, WeaponVisual,
    WeaponVisualKind, build_headless, teardown,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::weapon_effects::MuzzleEffectSpawn;
use cs_app::weapons::WeaponSession;

use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

/// The synthetic burst's quads: one flash quad and three spin quads.
const QUADS_PER_BURST: usize = 4;

/// A newly authored synthetic burst: one flash quad and three spin quads.
pub(super) fn synthetic_art() -> MuzzleBurstArt {
    let quad = |z: f32| BurstQuad {
        corners: [
            [-0.1, 0.0, z],
            [0.1, 0.0, z],
            [0.1, 0.0, z + 0.5],
            [-0.1, 0.0, z + 0.5],
        ],
        uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        texture: "synthetic.tif".to_owned(),
    };
    MuzzleBurstArt {
        weapon: "wep_synthetic".to_owned(),
        burst: "muzzle_burst_synthetic".to_owned(),
        world_group: "SYNTHETIC".to_owned(),
        texture_archive: String::new(),
        flash: BurstNodeArt {
            quads: vec![quad(-0.2)],
        },
        spin: BurstNodeArt {
            quads: vec![quad(-0.2), quad(-0.1), quad(0.0)],
        },
        spin_rolls_deg: vec![0.0, 60.0, 120.0],
        textures: Vec::new(),
    }
}

fn composed(label: &str) -> (Scratch, App) {
    let scratch = Scratch::new(label);
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.insert_resource(MuzzleBurstArtResource(synthetic_art()));
    app.update();
    (scratch, app)
}

fn set_key(app: &mut App, key: KeyCode, down: bool) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    if down {
        input.press(key);
    } else {
        input.release(key);
    }
}

fn records(app: &App) -> Vec<Entity> {
    app.world()
        .iter_entities()
        .filter(|entity| entity.contains::<MuzzleEffectSpawn>())
        .map(|entity| entity.id())
        .collect()
}

fn flashes(app: &App) -> Vec<Entity> {
    app.world()
        .iter_entities()
        .filter(|entity| {
            entity
                .get::<WeaponVisual>()
                .is_some_and(|visual| visual.kind == WeaponVisualKind::MuzzleFlash)
        })
        .map(|entity| entity.id())
        .collect()
}

fn ticks(app: &App) -> u64 {
    app.world().resource::<PhysicsTickLedger>().ticks
}

/// Fires one burst and returns once at least one record is live.
fn fire_once(app: &mut App) {
    set_key(app, KeyCode::Space, true);
    app.update();
    set_key(app, KeyCode::Space, false);
    assert!(!records(app).is_empty(), "the press was accepted");
}

/// **An accepted shot draws the measured burst: drawn children of its record,
/// at the record's muzzle position, naming the weapon row's burst.**
#[test]
fn accept_f27_c_fx_render_an_accepted_shot_draws_a_labelled_flash_child_of_its_record() {
    let (_scratch, mut app) = composed("f27-c-fx-render-draw");
    fire_once(&mut app);

    let records = records(&app);
    let flashes = flashes(&app);
    assert_eq!(
        flashes.len(),
        records.len() * QUADS_PER_BURST,
        "one burst (four quads) per record"
    );
    for record in records {
        let world = app.world();
        let spawn = world.get::<MuzzleEffectSpawn>(record).expect("record");
        let flash = world.get::<MuzzleFlash>(record).expect("record is drawn");
        let MuzzleDrawable::MeasuredBurst {
            declared,
            weapon,
            burst,
            ..
        } = &flash.drawable
        else {
            panic!(
                "an installed art draws the measured burst, got {:?}",
                flash.drawable
            );
        };
        assert_eq!(declared, &spawn.effect, "the declared id is kept");
        assert_eq!(
            (weapon.as_str(), burst.as_str()),
            ("wep_synthetic", "muzzle_burst_synthetic")
        );
        assert_eq!(flash.expires_at, spawn.at.0 + MUZZLE_FLASH_LIFETIME_TICKS);
        let children = world.get::<Children>(record).expect("record has a drawing");
        let drawn: Vec<_> = children
            .iter()
            .filter(|child| {
                let child = **child;
                world.get::<Mesh3d>(child).is_some()
                    && world
                        .get::<WeaponVisual>(child)
                        .is_some_and(|v| v.kind == WeaponVisualKind::MuzzleFlash)
                    && world.get::<ChildOf>(child).map(ChildOf::parent) == Some(record)
            })
            .collect();
        assert_eq!(drawn.len(), 4, "one flash quad and three spin quads");
    }
    teardown(&mut app);
}

/// **A record is never drawn twice, however many frames pass.**
#[test]
fn accept_f27_c_fx_render_a_spawning_pass_never_draws_the_same_record_twice() {
    let (_scratch, mut app) = composed("f27-c-fx-render-once");
    set_key(&mut app, KeyCode::Space, true);
    for _ in 0..6 {
        app.update();
        let records = records(&app);
        assert_eq!(
            flashes(&app).len(),
            records.len() * QUADS_PER_BURST,
            "one burst (four quads) per record"
        );
    }
    teardown(&mut app);
}

/// **A flash expires after its one-tick lifetime in committed ticks, taking
/// its record with it.**
#[test]
fn accept_f27_c_fx_render_a_flash_expires_after_its_designed_lifetime() {
    let (_scratch, mut app) = composed("f27-c-fx-render-expiry");
    fire_once(&mut app);
    let mut guard = 0;
    while !records(&app).is_empty() {
        for record in records(&app) {
            let flash = app.world().get::<MuzzleFlash>(record).expect("drawn");
            // A burst is held past its tick only until a rendered frame has
            // shown it; a frame that ended with the record live showed it.
            assert!(
                ticks(&app) < flash.expires_at || flash.presented,
                "a live flash is in lifetime or was just shown"
            );
        }
        app.update();
        guard += 1;
        assert!(guard < 60, "a flash that never expires is a leak");
    }
    assert!(flashes(&app).is_empty(), "the drawing went with its record");
    teardown(&mut app);
}

/// **A paused fixed clock freezes a flash exactly as it freezes the tick.**
#[test]
fn accept_f27_c_fx_render_a_paused_clock_freezes_the_flash() {
    let (_scratch, mut app) = composed("f27-c-fx-render-pause");
    fire_once(&mut app);
    // The session's own pause: the playtest's `sync_pause` freezes the fixed
    // clock from it every frame.
    app.world_mut()
        .resource_mut::<PlatformInput>()
        .session_mut()
        .pause(PauseReason::PlayerRequest);
    app.update();
    let frozen_at = ticks(&app);
    let before = records(&app).len();
    for _ in 0..40 {
        app.update();
    }
    assert_eq!(ticks(&app), frozen_at, "the fixed clock did not advance");
    assert_eq!(records(&app).len(), before, "the flash did not age");
    assert_eq!(flashes(&app).len(), before * QUADS_PER_BURST);
    teardown(&mut app);
}

/// **Removing the weapon session leaves neither the record nor its drawing.**
#[test]
fn accept_f27_c_fx_render_removing_the_weapon_session_takes_the_flash_with_it() {
    let (_scratch, mut app) = composed("f27-c-fx-render-teardown");
    fire_once(&mut app);
    assert!(!flashes(&app).is_empty());
    app.world_mut().remove_resource::<WeaponSession>();
    app.update();
    assert!(records(&app).is_empty(), "no record outlives its session");
    assert!(flashes(&app).is_empty(), "no drawing outlives its session");
    teardown(&mut app);
}
