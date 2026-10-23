//! PARITY6-M01-AUDIO-TIMELINE.04 (#1864): the M01 ambient loops — the
//! `snd_planeshake` rattle held by a keyed voice slot and the
//! `snd_waterfall` loop started by the `spew` animation's range gate.
//!
//! Task test prefix: `accept_parity6_m01_audio_04_`. Finding:
//! `docs/findings/2026-10-23-m01-audio-ambient-loops-wiring.md`; the rules
//! are `2026-10-23-m01-audio-rattle-slot-and-sound-sequences.md` (#1918) and
//! `2026-10-22-m01-audio-timeline-voice-laws.md` (.02).
//!
//! The retail scenario reads the installation named by `CS_GAME_DIR` and
//! fails loudly without it; it runs the production measurement.

use std::path::PathBuf;

use avian3d::prelude::LinearVelocity;
use bevy::ecs::system::RunSystemOnce;
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::transform::components::GlobalTransform;
use cs_app::audio::{
    AmbientVoicePlan, AmbientVoices, AudioOutput, EngineGroupLaw, RattleSlot, RattleSpec,
    WaterfallSpec, anim_range_gate, drive_ambient_voices, rattle_gate,
};
use cs_app::mission_session::{LaunchCinematic, MissionPlayerBody, measure_ambient_sound};
use cs_sim::audio_events::{AudioDevice, DeviceCommand, RecordingAudioDevice};
use cs_types::content::ContentId;

fn member(name: &str) -> ContentId {
    cs_app::mission_session::member_content_id("ZBD/soundsh.zbd", name).expect("a member id")
}

#[test]
fn accept_parity6_m01_audio_04_the_slot_starts_once_and_stops_on_the_second_tick() {
    let mut slot = RattleSlot::default();
    // Held for five frames: one start, never restarted.
    let mut starts = 0;
    for _ in 0..5 {
        starts += usize::from(slot.call());
        assert!(!slot.tick(), "the stamp is current, nothing expires");
    }
    assert_eq!(starts, 1);
    assert!(slot.is_held());
    // The gate stops holding: the tick of the last call saw clock == expiry
    // (above), the next tick sees clock > expiry and stops the voice.
    assert!(slot.tick(), "stopped on the second slot tick");
    assert!(!slot.is_held());
    // A new call starts the group again.
    assert!(slot.call());
}

#[test]
fn accept_parity6_m01_audio_04_the_gates_follow_the_recorded_laws() {
    assert!(rattle_gate(true, 113.0, 1.0, 113.0), "speed >= floor * max");
    assert!(!rattle_gate(true, 112.9, 1.0, 113.0));
    assert!(!rattle_gate(false, 200.0, 1.0, 113.0), "camera target only");
    assert!(anim_range_gate(0.0, (0.0, 360_000.0)));
    assert!(anim_range_gate(359_999.0, (0.0, 360_000.0)));
    assert!(
        !anim_range_gate(360_000.0, (0.0, 360_000.0)),
        "max exclusive"
    );
    assert!(!anim_range_gate(99.0, (100.0, 400.0)), "min inclusive");
    assert!(anim_range_gate(100.0, (100.0, 400.0)));
}

fn plan() -> AmbientVoicePlan {
    AmbientVoicePlan {
        rattle: Some(RattleSpec {
            group: "snd_planeshake".to_owned(),
            member: member("plane_shake.wav"),
            volume: 1.0,
            speed_floor: 1.0,
            max_speed_m_s: 100.0,
        }),
        waterfall: Some(WaterfallSpec {
            group: "snd_waterfall".to_owned(),
            member: member("waterfall.wav"),
            law: EngineGroupLaw::try_new(200.0, 1500.0, 1.0).expect("a law"),
            window_sq: (0.0, 600.0 * 600.0),
            nodes: vec![[1000.0, 0.0, 0.0]],
        }),
        refusals: Vec::new(),
    }
}

fn world_with_body(velocity: Vec3, at: Vec3) -> (World, RecordingAudioDevice) {
    let mut world = World::new();
    world.insert_resource(plan());
    let mut device = RecordingAudioDevice::new();
    device.open().expect("the device opens");
    let handle = device.handle();
    world.insert_resource(AudioOutput::new(Box::new(device)));
    world.spawn((
        MissionPlayerBody,
        GlobalTransform::from_translation(at),
        LinearVelocity(velocity),
    ));
    (world, handle)
}

fn run(world: &mut World) {
    world
        .run_system_once(drive_ambient_voices)
        .expect("the driver runs");
}

fn starts(device: &RecordingAudioDevice) -> Vec<ContentId> {
    device
        .commands()
        .into_iter()
        .filter_map(|c| match c {
            DeviceCommand::Started { start, .. } => Some(start.asset),
            _ => None,
        })
        .collect()
}

fn stops(device: &RecordingAudioDevice) -> usize {
    device
        .commands()
        .into_iter()
        .filter(|c| matches!(c, DeviceCommand::Stopped { .. }))
        .count()
}

#[test]
fn accept_parity6_m01_audio_04_the_rattle_holds_one_voice_while_fast_and_stops_after() {
    let (mut world, device) = world_with_body(Vec3::new(0.0, 0.0, -120.0), Vec3::ZERO);
    for _ in 0..4 {
        run(&mut world);
    }
    assert_eq!(
        starts(&device),
        vec![member("plane_shake.wav")],
        "one voice"
    );
    assert_eq!(stops(&device), 0);
    let held = world
        .resource::<AmbientVoices>()
        .rattle
        .clone()
        .expect("held");
    assert_eq!((held.pan, held.pitch), (0.0, 1.0), "2D, native rate");

    // Slow below the floor. The consumer runs before the slot tick within a
    // frame (this schedule's order; the engine's is unread), so the slot tick
    // of the last gated frame saw clock == expiry and the next frame's tick
    // is the second one: it stops the voice.
    let mut query = world.query::<&mut LinearVelocity>();
    for mut velocity in query.iter_mut(&mut world) {
        velocity.0 = Vec3::new(0.0, 0.0, -50.0);
    }
    run(&mut world);
    assert_eq!(stops(&device), 1, "the second slot tick stops it");
    assert!(world.resource::<AmbientVoices>().rattle.is_none());
}

#[test]
fn accept_parity6_m01_audio_04_no_rattle_while_the_cinematic_holds_the_camera() {
    let (mut world, device) = world_with_body(Vec3::new(0.0, 0.0, -120.0), Vec3::ZERO);
    world.insert_resource(LaunchCinematic::new(10).expect("a phase"));
    for _ in 0..3 {
        run(&mut world);
    }
    assert!(starts(&device).is_empty(), "the aircraft is not the camera");
}

#[test]
fn accept_parity6_m01_audio_04_the_waterfall_starts_inside_the_range_gate_and_mixes_in_3d() {
    // 700 m from the node: the animation does not step, no voice.
    let (mut world, device) = world_with_body(Vec3::ZERO, Vec3::new(300.0, 0.0, 0.0));
    run(&mut world);
    assert!(starts(&device).is_empty());
    // Inside 600 m: the loop starts once, at the first step.
    let mut query = world.query::<&mut GlobalTransform>();
    for mut transform in query.iter_mut(&mut world) {
        *transform = GlobalTransform::from_translation(Vec3::new(600.0, 0.0, 0.0));
    }
    run(&mut world);
    run(&mut world);
    assert_eq!(starts(&device), vec![member("waterfall.wav")]);
    let held = world
        .resource::<AmbientVoices>()
        .waterfall
        .clone()
        .expect("held");
    // 400 m from the node, RANGE (200, 1500): inside the rolloff knee
    // x = 162.5, y = 200 -> 1000*log2(162.5/200) = -300 hundredths of a dB.
    let expected = 2.0_f64.powf(-0.3);
    assert!(
        (held.gain - expected).abs() < 0.02,
        "gain {} vs the measured law {expected}",
        held.gain
    );
    // The node is to the listener's +X: a positive pan.
    assert!(held.pan > 0.0, "pan {}", held.pan);
    // Leaving the window does not stop the looped voice.
    for mut transform in query.iter_mut(&mut world) {
        *transform = GlobalTransform::from_translation(Vec3::new(-100.0, 0.0, 0.0));
    }
    run(&mut world);
    assert_eq!(stops(&device), 0);
    assert_eq!(starts(&device).len(), 1, "never restarted");
}

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this test"),
    )
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_04_the_retail_data_binds_both_loops_and_names_the_unknown() {
    let ambient = measure_ambient_sound(&game_dir(), "pdevastator", "zbd/c3")
        .expect("the retail chain measures");
    let rattle = ambient
        .plan
        .rattle
        .as_ref()
        .unwrap_or_else(|| panic!("the rattle binds: {:?}", ambient.plan.refusals));
    assert_eq!(rattle.group, "snd_planeshake");
    assert_eq!(rattle.member, member("plane_shake.wav"));
    assert_eq!(rattle.speed_floor, 1.0);
    assert_eq!(rattle.max_speed_m_s, 113.0, "pdevastator fd_speed");
    let waterfall = ambient
        .plan
        .waterfall
        .as_ref()
        .expect("the waterfall binds");
    assert_eq!(waterfall.member, member("waterfall.wav"));
    assert_eq!(
        waterfall.window_sq,
        (0.0, 360_000.0),
        "EXECUTION_BY_RANGE 600"
    );
    assert_eq!(
        (waterfall.law.range_min, waterfall.law.range_max),
        (200.0, 1500.0)
    );
    assert_eq!(waterfall.nodes.len(), 1, "one `spew` node in the C3 world");
    let [x, y, z] = waterfall.nodes[0];
    assert!(
        (x + 4385.73).abs() < 0.01 && (y - 145.66).abs() < 0.01 && (z + 4872.68).abs() < 0.01,
        "the node's composed world position: {:?}",
        waterfall.nodes[0]
    );
    assert!(
        ambient.plan.refusals.iter().any(|r| r.contains("33864")),
        "the unresolved loop is named: {:?}",
        ambient.plan.refusals
    );
    assert_eq!(ambient.records().expect("records").len(), 2);
}
