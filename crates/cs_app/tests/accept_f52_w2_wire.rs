//! F52-W2-wire: the presenters are wired into the production draw loop and
//! the gameplay event producers (Rally #2140).
//!
//! Every test drives the real schedule: [`cs_app::playtest::headless_app`] is
//! the same `PlaytestPlugin` composition the window runs — one `update` one
//! rendered frame, the fixed loop inside it — and the session is installed
//! through the same [`install_live_presentation`] the windowed entries call.
//! Nothing here calls the helper functions alone.

use std::fs;
use std::path::PathBuf;

use bevy::prelude::{First, RunFixedMainLoop};

use cs_app::audio::AudioSession;
use cs_app::audio::mixer::{AudioOutput, with_live_device};
use cs_app::camera::effects::{
    CONTACT_IMPACT_STRENGTH, GameEvent, GameEventQueue, MUZZLE_FLASH_INTENSITY, PresentedEffects,
};
use cs_app::cli;
use cs_app::mission_session::{MissionOrdnanceReport, attach_player_victim};
use cs_app::physics::{BodyMode, BodySpec, spawn_body};
use cs_app::playtest;
use cs_app::render::presentation::{
    FrameFilterReport, LivePresentation, install_live_presentation, present_captured_frame,
};
use cs_app::scene::SceneGeneration;
use cs_app::targeting::{PendingTargetDamage, SelectionReport, TargetingScheduleReport};
use cs_app::ui::hud::presentation::PresentedHud;
use cs_app::user_data::{Environment, Platform};
use cs_content::audio::{AudioBus as SettingsBus, AudioLevel};
use cs_content::settings::ColourFilter;
use cs_sim::audio_events::{
    AudioBus, AudioRouter, DeviceCommand, RadioLine, RecordingAudioDevice, VoiceStart,
};
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::damage::{ActorId, DamageChannel, HitEvent};
use cs_sim::weapons::guns::ProjectileId;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, DamageNodeKey};
use cs_types::net::{EventId, SessionId};

fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("{name}_{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("the scratch dir must be reusable");
    }
    fs::create_dir_all(&dir).expect("the scratch dir must be creatable");
    dir
}

/// The session opener the boot path uses (F52-W1), on a writable scratch base:
/// `apply` persists the file the same way the player's own writes do.
fn open_session(base: &std::path::Path) -> cs_app::accessibility::session::SettingsSession {
    cli::open_accessibility_session(true, Some(base), Platform::Linux, &Environment::default())
        .expect("a given profile base needs no environment")
}

fn session_id() -> SessionId {
    SessionId::new(1).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn hit(tick: u64, target: ActorId, damage: f64) -> HitEvent {
    HitEvent::try_new(
        EventId {
            session: session_id(),
            tick: Tick(tick),
            producer: 0,
            sequence: 0,
        },
        Some(actor(900)),
        target,
        DamageNodeKey::new("hull").expect("a valid node key"),
        DamageChannel::Internal,
        damage,
    )
    .expect("positive finite damage is a valid hit")
}

/// Advances the world through its real fixed loop without running `Update`:
/// `First` moves the manual clock one frame, `RunFixedMainLoop` turns the
/// accumulated time into the same `FixedFirst..=FixedLast` runs a rendered
/// frame drives — so the world sits exactly where `Update` begins, with the
/// queue still undrained.
fn advance_fixed_only(app: &mut bevy::prelude::App) {
    app.world_mut().run_schedule(First);
    app.world_mut().run_schedule(RunFixedMainLoop);
}

/// The wire installs the live session before the first frame and the
/// presentation resources with the plugin; a session-less world presents
/// nothing but still drains.
#[test]
fn accept_f52_w2_wire_live_session_is_installed_for_every_presenter() {
    let base = scratch_dir("f52w2_install");
    let mut app = playtest::headless_app();
    install_live_presentation(&mut app, open_session(&base));

    assert!(
        app.world().get_resource::<LivePresentation>().is_some(),
        "the live session is the world's presentation owner"
    );
    assert!(
        app.world().get_resource::<GameEventQueue>().is_some()
            && app.world().get_resource::<PresentedEffects>().is_some()
            && app.world().get_resource::<PresentedHud>().is_some()
            && app.world().get_resource::<FrameFilterReport>().is_some(),
        "the plugin owns the queue, the presented frame, the HUD pass and the filter report"
    );

    app.update();
    assert!(
        app.world().resource::<PresentedEffects>().frame.is_some(),
        "a world with the live session installed presents a frame every update"
    );
}

/// Gameplay events reach the production queue through the production emit
/// systems, and `present_game_events` turns them into the frame's effects.
#[test]
fn accept_f52_w2_wire_combat_events_reach_the_presented_frame() {
    let base = scratch_dir("f52w2_events");
    let mut app = playtest::headless_app();
    install_live_presentation(&mut app, open_session(&base));

    // A real contact: an Aircraft-layer body overlapping StaticWorld geometry,
    // recorded by the same `record_contact_reports` the tick stamps.
    for spec in [
        BodySpec {
            layer: CollisionLayer::Aircraft,
            shape: ShapeClass::Solid,
            mode: BodyMode::Dynamic,
            mass_kg: 10.0,
            half_extents_m: [2.0; 3],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0; 3],
        },
        BodySpec {
            layer: CollisionLayer::StaticWorld,
            shape: ShapeClass::Solid,
            mode: BodyMode::Static,
            mass_kg: 0.0,
            half_extents_m: [4.0; 3],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0; 3],
        },
    ] {
        spawn_body(app.world_mut(), &spec).expect("the spec validates");
    }

    // The player's victim, attached the way the mission attaches it.
    let player = app.world_mut().spawn_empty().id();
    let victim = attach_player_victim(app.world_mut(), player, actor(1), SceneGeneration(1))
        .expect("the production victim attachment accepts a bare player");
    app.world_mut().insert_resource(victim);
    // A hit on the player landing on the tick about to run, and an accepted
    // ordnance launch the report folded in.
    app.world_mut().insert_resource(PendingTargetDamage::new(
        vec![hit(1, actor(1), 50.0)],
        vec![],
    ));
    app.world_mut().insert_resource(MissionOrdnanceReport {
        projectiles: vec![ProjectileId {
            session: 1,
            serial: 7,
        }],
        ..MissionOrdnanceReport::default()
    });

    // Two frames of the real fixed loop, observed before `Update`'s
    // presentation pass: the broad phase registers the new colliders on the
    // first tick and reports the overlap on a later one, while the events the
    // emitters raised sit in the production queue, undrained.
    for _ in 0..2 {
        advance_fixed_only(&mut app);
    }
    let queued = &app.world().resource::<GameEventQueue>().events;
    assert!(
        queued.contains(&GameEvent::Impact {
            strength: CONTACT_IMPACT_STRENGTH,
        }),
        "an aircraft contact becomes an Impact event; queue: {queued:?}"
    );
    assert!(
        queued.contains(&GameEvent::PlayerHit { damage: 50.0 }),
        "a hit on the player's victim becomes a PlayerHit; queue: {queued:?}"
    );
    assert!(
        queued.contains(&GameEvent::WeaponFlash {
            intensity: MUZZLE_FLASH_INTENSITY,
        }),
        "an accepted ordnance launch becomes a WeaponFlash; queue: {queued:?}"
    );

    // The rendered frame drains the queue through the live session and
    // publishes what was drawn.
    app.update();
    assert!(
        app.world().resource::<GameEventQueue>().events.is_empty(),
        "present_game_events drains the queue once per rendered frame"
    );
    let presented = &app
        .world()
        .resource::<PresentedEffects>()
        .frame
        .clone()
        .expect("a presented frame")
        .presented;
    use cs_app::accessibility::motion::Effect;
    assert!(
        presented.contains(&Effect::DamageNotice),
        "a PlayerHit always presents its required notice; presented: {presented:?}"
    );
    assert!(
        presented
            .iter()
            .any(|effect| matches!(effect, Effect::CameraShake(_))),
        "Impact and PlayerHit present their shake; presented: {presented:?}"
    );
    assert!(
        presented
            .iter()
            .any(|effect| matches!(effect, Effect::ScreenFlash(_))),
        "PlayerHit and WeaponFlash present their flash; presented: {presented:?}"
    );
}

/// Reduced motion drops the cosmetic shake and flash while the required
/// damage and target notices still present — the live session filters, the
/// emitters do not.
#[test]
fn accept_f52_w2_wire_reduced_motion_keeps_the_required_notices() {
    let base = scratch_dir("f52w2_reduced");
    let mut session = open_session(&base);
    session
        .apply(|settings| {
            settings.presentation.reduce_shake = true;
            settings.presentation.reduce_flash = true;
        })
        .expect("the reduced-motion pair is valid");

    let mut app = playtest::headless_app();
    install_live_presentation(&mut app, session);

    let player = app.world_mut().spawn_empty().id();
    let victim = attach_player_victim(app.world_mut(), player, actor(1), SceneGeneration(1))
        .expect("the production victim attachment accepts a bare player");
    app.world_mut().insert_resource(victim);
    app.world_mut().insert_resource(PendingTargetDamage::new(
        vec![hit(1, actor(1), 50.0)],
        vec![],
    ));
    app.world_mut().insert_resource(TargetingScheduleReport {
        selection: Some(SelectionReport {
            acted: Vec::new(),
            ignored: Vec::new(),
            selection: Some(actor(2)),
            phase: None,
        }),
        ..TargetingScheduleReport::default()
    });

    app.update();
    let frame = app
        .world()
        .resource::<PresentedEffects>()
        .frame
        .clone()
        .expect("a presented frame");
    use cs_app::accessibility::motion::Effect;
    assert_eq!(
        frame.presented,
        vec![Effect::DamageNotice, Effect::TargetNotice],
        "reduced motion drops shake and flash; the damage and target notices are required"
    );
    assert!(
        app.world().resource::<PresentedHud>().flash == 0.0,
        "a filtered-out flash reaches the HUD as 0.0, not as drawn-then-hidden"
    );
}

/// The subtitle row reads the radio queue's active line under the live
/// session's switch and scale.
#[test]
fn accept_f52_w2_wire_subtitles_follow_the_radio_line_and_the_switch() {
    let base = scratch_dir("f52w2_subtitles");
    let mut session = open_session(&base);
    session
        .apply(|settings| {
            settings.presentation.subtitles = true;
            settings.presentation.ui_scale_percent = 150;
        })
        .expect("subtitles and a larger scale are valid");

    let mut app = playtest::headless_app();
    install_live_presentation(&mut app, session);

    let mut audio = AudioSession::new(
        AudioRouter::new(session_id()),
        SceneGeneration(1),
        std::iter::empty(),
    );
    audio.radio.enqueue(
        RadioLine::try_new(
            EventId {
                session: session_id(),
                tick: Tick(0),
                producer: 0,
                sequence: 0,
            },
            "Wingman",
            ContentId::from_source(ContentKind::Dialogue, "radio-line.f52w2")
                .expect("a dialogue id"),
            0,
            false,
            Some("Bandits at three o'clock"),
            60,
        )
        .expect("a valid radio line"),
        Tick(0),
    );
    app.world_mut().insert_resource(audio);

    app.update();
    let hud = &app.world().resource::<PresentedHud>().hud;
    assert_eq!(
        hud.subtitles.len(),
        1,
        "the active radio line reaches the subtitle row"
    );
    assert_eq!(hud.subtitles[0].speaker, "Wingman");
    assert_eq!(hud.subtitles[0].text, "Bandits at three o'clock");
    assert_eq!(
        hud.text_px,
        cs_app::accessibility::cues::scaled(
            app.world().resource::<LivePresentation>().presentation()
        )
        .text_px,
        "the row draws at the session's scaled size, not the base"
    );

    // The switch off is the live reading, not a copy: a session change is the
    // next frame's HUD.
    app.world_mut()
        .resource_mut::<LivePresentation>()
        .0
        .apply(|settings| settings.presentation.subtitles = false)
        .expect("switching subtitles off is valid");
    app.update();
    assert!(
        app.world()
            .resource::<PresentedHud>()
            .hud
            .subtitles
            .is_empty(),
        "subtitles off is drawn the next frame, from the live session"
    );
}

/// A captured frame is filtered under the live session's colour filter before
/// it is measured or saved, and a non-RGBA buffer is a recorded error.
#[test]
fn accept_f52_w2_wire_captured_frames_take_the_live_colour_filter() {
    let base = scratch_dir("f52w2_filter");
    let mut session = open_session(&base);
    session
        .apply(|settings| settings.presentation.colour_filter = ColourFilter::Monochrome)
        .expect("a colour filter is valid");

    let mut app = playtest::headless_app();
    install_live_presentation(&mut app, session);

    let mut image = bevy::image::Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        &[255_u8, 0, 0, 200, 0, 255, 0, 255],
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    app.world_mut()
        .resource_scope(|world, mut report: bevy::prelude::Mut<FrameFilterReport>| {
            present_captured_frame(
                world.get_resource::<LivePresentation>(),
                &mut report,
                &mut image,
            );
        });
    let data = image.data.as_deref().expect("the capture buffer");
    // Monochrome collapses the channels; alpha is never touched.
    assert_eq!(data[0], data[1], "monochrome zeroes the red/green split");
    assert_eq!(data[0], data[2]);
    assert_eq!(data[3], 200, "alpha is preserved verbatim");
    assert_eq!(data[4], data[5]);
    assert_eq!(data[7], 255);
    assert_eq!(
        app.world().resource::<FrameFilterReport>().filtered,
        1,
        "the filter pass is recorded, not silent"
    );
}

/// Every voice passes through the bus-level wrapper at the device boundary:
/// the Radio level the settings file holds scales the voice the device sees.
#[test]
fn accept_f52_w2_wire_voices_take_the_live_bus_levels() {
    let base = scratch_dir("f52w2_buses");
    let mut session = open_session(&base);
    session
        .apply(|settings| {
            settings.presentation.bus_levels.insert(
                SettingsBus::Radio,
                AudioLevel::try_new(0.25).expect("a level"),
            );
        })
        .expect("a bus level is valid");

    let mut app = playtest::headless_app();
    install_live_presentation(&mut app, session);

    let device = RecordingAudioDevice::new();
    let handle = device.handle();
    app.insert_resource(AudioOutput::new(Box::new(device)));

    // The seam every direct device call site uses.
    let start = VoiceStart {
        asset: ContentId::from_source(ContentKind::Dialogue, "radio-voice.f52w2")
            .expect("a dialogue id"),
        bus: AudioBus::Radio,
        gain: 0.8,
        pan: 0.0,
        pitch: 1.0,
    };
    with_live_device(app.world_mut(), |device| {
        device.open().expect("the recording device opens")
    });
    let voiced = with_live_device(app.world_mut(), |device| {
        device
            .start_one_shot(start.clone())
            .expect("an open device voices")
    });
    assert!(voiced.is_some(), "an AudioOutput was installed");

    let scaled = handle
        .commands()
        .into_iter()
        .find_map(|command| match command {
            DeviceCommand::OneShotStarted { start, .. } => Some(start.gain),
            _ => None,
        })
        .expect("the one-shot reached the device");
    assert!(
        (scaled - 0.2).abs() < 1e-9,
        "the Radio bus level scales the voice the device sees: {scaled}"
    );
}

/// The presentation settings never reach the simulation's view: a change to
/// every presentation field leaves `gameplay_inputs()` untouched (AC03).
#[test]
fn accept_f52_w2_wire_presentation_never_reaches_gameplay_inputs() {
    let base = scratch_dir("f52w2_separation");
    let mut session = open_session(&base);
    let before = session.gameplay_inputs();
    session
        .apply(|settings| {
            settings.presentation.reduce_shake = true;
            settings.presentation.reduce_flash = true;
            settings.presentation.subtitles = true;
            settings.presentation.ui_scale_percent = 150;
            settings.presentation.colour_filter = ColourFilter::Monochrome;
            settings.presentation.bus_levels.insert(
                SettingsBus::Radio,
                AudioLevel::try_new(0.25).expect("a level"),
            );
        })
        .expect("every presentation change together is valid");
    assert_eq!(
        before,
        session.gameplay_inputs(),
        "no presentation field is a gameplay input"
    );
}
