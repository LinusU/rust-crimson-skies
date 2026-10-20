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
use bevy::prelude::{App, ChildOf, Children, Entity, Mesh3d};

use cs_app::input::platform::PlatformInput;
use cs_app::mission_session::{
    MissionCockpitActions, MissionComposition, MissionGunHold, MissionHudNotice,
    MissionWeaponReport, MissionWeapons, WeaponVisual, WeaponVisualKind, build_headless,
    is_fire_guns, teardown,
};
use cs_app::ordnance::OrdnanceItemMirror;
use cs_app::weapons::WeaponRoundMirror;
use cs_types::input::FlightCommand;

use crate::campaign::m01_play_weapons::armed_stage;
use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

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

/// **A live round is not drawn: the owner's capture of the original shows no
/// tracer streaks for a held trigger, so a round's mirror carries no mesh.**
#[test]
fn accept_m01_window_fire_a_live_round_draws_no_tracer() {
    let (_scratch, mut app) = composed("m01-window-fire-tracers");

    press(&mut app, KeyCode::Space);
    for _ in 0..6 {
        app.update();
    }
    release(&mut app, KeyCode::Space);
    app.update();

    let live = rounds(&app);
    assert!(live >= 1, "the held key left live rounds to check: {live}");
    let world = app.world();
    for entity in world
        .iter_entities()
        .filter(|entity| entity.contains::<WeaponRoundMirror>())
    {
        assert!(
            entity.get::<Mesh3d>().is_none(),
            "a round mirror carries no mesh itself"
        );
        assert!(
            entity.get::<Children>().is_none_or(|children| children
                .iter()
                .all(|child| world.get::<Mesh3d>(*child).is_none())),
            "a round mirror owns no drawn child"
        );
    }
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
        press(&mut app, KeyCode::KeyX);
        app.update();
        release(&mut app, KeyCode::KeyX);
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
    press(&mut refused, KeyCode::KeyX);
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
    release(&mut refused, KeyCode::KeyX);
    refused.update();
    press(&mut refused, KeyCode::KeyX);
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

// ---------------------------------------------------------------------------
// The gun's shot sound (#1546): measured per weapon record, not bound to the
// designed M01 gun.
// ---------------------------------------------------------------------------

mod gun_sound {
    use cs_app::mission_session::{
        WeaponSoundError, measure_weapon_sound, sound_like_keys, weapon_sound_from_documents,
    };
    use cs_content::original_airframe::OriginalDocuments;
    use cs_content::stunts::ZrdValue;

    fn text(value: &str) -> ZrdValue {
        ZrdValue::Text(value.to_owned())
    }

    fn list(children: Vec<ZrdValue>) -> ZrdValue {
        ZrdValue::List(children)
    }

    /// The measured shapes, newly authored: a ballistics record naming a
    /// group, and a group record carrying a member and flags.
    fn documents(with_field: bool) -> (ZrdValue, ZrdValue) {
        let mut body = vec![text("DESC"), list(vec![text("x")])];
        if with_field {
            body.extend([text("LOOPED_SOUND_NAME"), list(vec![text("snd_synth")])]);
        }
        let weapons = list(vec![list(vec![
            text("BALLISTICS"),
            list(vec![text("wep_syn"), list(body)]),
        ])]);
        let sounds = list(vec![
            list(vec![text("snd_other"), text("other.wav"), text("SFX")]),
            list(vec![
                text("snd_synth"),
                text("synth.wav"),
                text("3D"),
                text("LOOPED"),
            ]),
        ]);
        (weapons, sounds)
    }

    #[test]
    fn accept_m01_window_fire_gun_sound_chain_reads_group_member_and_loop_flag() {
        let (weapons, sounds) = documents(true);
        let measured = weapon_sound_from_documents(&weapons, &sounds, "wep_syn")
            .expect("the chain is complete");
        assert_eq!(measured.group, "snd_synth");
        assert_eq!(measured.member, "synth.wav");
        assert!(measured.looped, "the group is flagged LOOPED");
    }

    #[test]
    fn accept_m01_window_fire_gun_sound_a_record_without_the_field_is_unmeasured() {
        let (weapons, sounds) = documents(false);
        let refusal = weapon_sound_from_documents(&weapons, &sounds, "wep_syn")
            .expect_err("no field, no binding");
        assert!(
            matches!(refusal, WeaponSoundError::Unmeasured(_)),
            "{refusal}"
        );
        let refusal = weapon_sound_from_documents(&weapons, &sounds, "wep_missing")
            .expect_err("no record, no binding");
        assert!(
            matches!(refusal, WeaponSoundError::Unmeasured(_)),
            "{refusal}"
        );
    }

    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_m01_window_fire_gun_sound_retail_weapon_records_name_their_gun_members() {
        let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
        // (weapon record, group, member): read from the installation on every
        // run, so a stale expectation fails.
        for (weapon, group, member) in [
            ("wep_00", "snd_30cal", "30cal_gun.wav"),
            ("wep_01", "snd_40cal", "40cal_gun.wav"),
            ("wep_02", "snd_50cal", "50cal_gun.wav"),
            ("wep_50", "snd_50cal", "50cal_gun.wav"),
            ("wep_70", "snd_70cal", "70cal_gun.wav"),
        ] {
            let measured = measure_weapon_sound(&root, weapon).expect("the record is measured");
            assert_eq!(measured.group, group, "{weapon}");
            assert_eq!(measured.member, member, "{weapon}");
            assert!(measured.looped, "{weapon}: the group is flagged LOOPED");
        }
        assert!(measure_weapon_sound(&root, "wep_9999").is_err());
    }

    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_m01_window_fire_gun_sound_the_player_airframe_record_names_no_gun_sound() {
        let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
        let documents = OriginalDocuments::read(&root).expect("the installation reads");
        let vehicle = documents.vehicle().root().to_zrd_value();
        // The chain basic_airplane -> player_airplane -> pdevastator: only the
        // two engine sounds are sound-like at the top level of the leaf; the
        // guns' sound is a property of the weapon record, not the airframe.
        let leaf = sound_like_keys(&vehicle, "pdevastator");
        assert_eq!(leaf, ["engine_sound", "cockpit_engine_sound"]);
        let parent = sound_like_keys(&vehicle, "player_airplane");
        assert!(
            parent
                .iter()
                .all(|key| !key.contains("sound") && !key.contains("snd")),
            "no sound field on the parent: {parent:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Which of those records the player's M01 guns carry (#1628): the committed
// (gun, ammunition) map, the measured member's delivery, and the loadout's
// shot sound naming it.
// ---------------------------------------------------------------------------

mod gun_loadout {
    use std::path::PathBuf;

    use bevy::prelude::With;
    use cs_app::audio::{AudioHandoffLog, AudioSession, DeclaredAudioCatalog};
    use cs_app::engine_facts::{FlightCheckPlaneSlot, engine_facts};
    use cs_app::loading::LoadedItemBinding;
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::{
        GunSound, MEASURED_GUN_MEMBER, MissionStage, build_headless, designed_loadout,
        measured_gun_sound_id, stage_for,
    };
    use cs_types::content::Resolved;
    use cs_types::evidence::{ClaimStatus, ContentHash};

    use crate::campaign::common::label;

    /// The record names one stage's measured slots resolve to, in slot order.
    fn slot_records(stage_records: &[cs_app::mission_session::WeaponSound]) -> Vec<String> {
        stage_records
            .iter()
            .map(|sound| sound.weapon.clone())
            .collect()
    }

    /// **The committed (gun, ammunition) map reproduces the measured M01
    /// loadout: the pilot's and the wingman's fresh-campaign records carry
    /// `wep_50`, `wep_40`, `wep_30` and one empty slot.**
    ///
    /// Removing the `[gun_loadout]` table or its map fails the facts parse;
    /// wrong bases or sentinel fail the names.
    #[test]
    fn accept_m01_window_fire_gun_loadout_the_engine_map_resolves_the_player_records() {
        let facts = engine_facts().expect("the committed engine facts parse");
        let loadout = facts.gun_loadout();
        for slot in [FlightCheckPlaneSlot::Pilot, FlightCheckPlaneSlot::Wingman] {
            let default = facts.flight_check().default_for(slot);
            let records: Vec<Option<String>> = default
                .guns
                .iter()
                .zip(default.ammo.iter())
                .map(|(&gun, &ammo)| {
                    loadout
                        .weapon_index(gun, ammo)
                        .map(|index| loadout.record_name(index))
                })
                .collect();
            assert_eq!(
                records,
                [
                    Some("wep_50".to_owned()),
                    Some("wep_40".to_owned()),
                    Some("wep_30".to_owned()),
                    None,
                ],
                "{slot:?}'s gun slots resolve to the measured records"
            );
        }
    }

    /// **Every designed gun's shot sound names the member the composition
    /// delivers — never the retired designed id.**
    ///
    /// Reverting the binding to a designed spelling fails here.
    #[test]
    fn accept_m01_window_fire_gun_loadout_the_shot_sound_names_the_delivered_member() {
        let delivered = measured_gun_sound_id();
        for gun in &designed_loadout().guns {
            let Resolved::Known(known) = gun.sound() else {
                panic!("the designed gun's shot sound is declared");
            };
            assert_eq!(
                &known.value, &delivered,
                "every designed gun names the measured member"
            );
        }
    }

    /// **The declared record for the delivered member is a `Weapons`-bus
    /// loop — the bus authored, the loop measured — and says so.**
    ///
    /// A one-shot declaration, a wrong bus or a claim class inflated to
    /// `verified_original` all fail here.
    #[test]
    fn accept_m01_window_fire_gun_loadout_the_declared_record_is_a_weapons_loop() {
        let gun = GunSound {
            weapons: Vec::new(),
            container: "ZBD/soundsh.zbd".to_owned(),
            container_sha256: ContentHash::from_bytes([0; 32]),
            host: PathBuf::new(),
            content: measured_gun_sound_id(),
            bytes: 0,
        };
        let record = gun.record().expect("the declared record builds");
        let Resolved::Known(bus) = &record.playback().bus else {
            panic!("the bus is declared");
        };
        assert_eq!(bus.value, cs_content::audio::AudioBus::Weapons);
        assert_eq!(
            bus.provenance.class,
            ClaimStatus::Designed,
            "the bus is the project's own routing choice"
        );
        let Resolved::Known(mode) = &record.playback().mode else {
            panic!("the mode is declared");
        };
        assert_eq!(mode.value, cs_content::audio::PlaybackMode::Loop);
        assert_eq!(
            mode.provenance.class,
            ClaimStatus::ObservedTool,
            "the LOOPED flag is measured installation evidence, never verified_original"
        );
    }

    /// **The production composition measures the records, delivers the
    /// selected member beside the launch sound, and lowers the looped
    /// `Weapons` spec the shot id names.**
    ///
    /// Removing the stage's measurement, the second load item or the catalog
    /// record each fails a different assertion here.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_m01_window_fire_gun_loadout_retail_the_member_is_delivered_and_lowered() {
        let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
        let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
            .expect("M01's launch closure plans");
        let stage: MissionStage =
            stage_for(&root, &plan).expect("M01's stage reads through the production readers");
        let gun = stage
            .gun_sound
            .as_ref()
            .expect("the stage measured the player's gun loadout");
        assert_eq!(
            slot_records(&gun.weapons),
            ["wep_50", "wep_40", "wep_30"],
            "the pilot record's gun slots resolve in slot order"
        );
        assert!(
            gun.weapons.iter().all(|sound| sound.looped),
            "every resolved group is flagged LOOPED"
        );
        assert_eq!(gun.weapons[0].group, "snd_50cal");
        assert_eq!(gun.weapons[0].member, MEASURED_GUN_MEMBER);
        assert_eq!(gun.content, measured_gun_sound_id());

        let mut app = build_headless(&stage).expect("the retail stage composes");
        app.update();

        // The member was delivered through the announced load: a binding
        // carrying exactly the measured member's id.
        let mut query = app
            .world_mut()
            .query_filtered::<&LoadedItemBinding, With<LoadedItemBinding>>();
        let delivered = query
            .iter(app.world())
            .filter(|binding| binding.content == gun.content)
            .count();
        assert_eq!(delivered, 1, "the gun member is a delivered binding");

        // The declared catalog holds the record, and the handoff lowered it
        // into the session's spec.
        let catalog = app.world().resource::<DeclaredAudioCatalog>();
        assert!(
            catalog.catalog().get(&gun.content).is_some(),
            "the declared catalog holds the measured member"
        );
        let handoff = app.world().resource::<AudioHandoffLog>();
        let install = handoff
            .installed
            .as_ref()
            .expect("the load installed an audio session");
        assert!(
            install.samples >= 2,
            "the launch and gun members are both decoded: {install:?}"
        );
        let session = app.world().resource::<AudioSession>();
        let spec = session
            .spec(&gun.content)
            .expect("the delivered member lowered into a spec");
        assert_eq!(spec.bus(), cs_sim::audio_events::AudioBus::Weapons);
        assert_eq!(spec.mode(), cs_sim::audio_events::PlaybackMode::Loop);

        cs_app::mission_session::teardown(&mut app);
    }
}
