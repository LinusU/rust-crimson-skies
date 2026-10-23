//! Acceptance tests of SIDEWINDER-FFB2 (#1968): the Microsoft SideWinder Force
//! Feedback 2 flies the game, and its force feedback starts and stops safely.
//!
//! The tests drive the production path end to end without the physical stick:
//! input reports are built in the stick's own report layout (the bundled
//! descriptor, byte-identical to the one the owner's stick returned on
//! 2026-10-23), decoded by `Layout`, handed through the `HostJoysticks` seam
//! as the device thread's messages, and pumped through an `InputSession` with
//! the mission's own action map (`ControlsMap::original_default`). Force
//! feedback runs `FlightFeel` into the USB PID driver over a transport that
//! records the reports a real stick would receive. What only the hardware can
//! show — that the motors push the stated way — was checked on the owner's
//! stick with `cs --joystick-probe --test-force-feedback`
//! (`docs/findings/2026-10-23-sidewinder-ffb2-macos.md`).
//!
//! What makes them discriminating:
//!
//! * Each axis is checked for its sign at both ends, so a binding that flew
//!   pitch or the throttle backwards fails, and the stick's resting offset
//!   (`Y = -22` of 512 on the owner's stick) must read as no pitch at all.
//! * A held button must be one edge, and an unplug must release it.
//! * Every force-feedback exit (pause, focus, no control, no device,
//!   shutdown) must send the reset that stops the motors, and nothing may
//!   play before the player flies.

use std::ffi::CString;
use std::sync::mpsc::channel;

use cs_app::cli::{CliRequest, parse};
use cs_app::force_feedback::{
    DESIGNED_DEFAULT_GAIN_PERCENT, FeelGate, FlightCues, FlightFeel, ForceFeedbackSettings,
    SPRING_RAMP_S, ids,
};
use cs_app::input::hid::descriptor::{
    Descriptor, PAGE_BUTTON, PAGE_GENERIC_DESKTOP, Report, ReportKind, sidewinder_ff2,
    sidewinder_ff2_bytes, usage,
};
use cs_app::input::hid::device::{Candidate, JoystickMessage};
use cs_app::input::hid::effect::Command;
use cs_app::input::hid::layout::{
    HAT_DOWN, HAT_LEFT, HAT_RIGHT, HAT_UP, HatDirection, JoystickState, Layout,
};
use cs_app::input::hid::pid::{Pid, Transport};
use cs_app::input::hid::{HostJoystickOptions, HostJoysticks};
use cs_app::input::{DeviceEvent, FrameInput, InputSession, SessionMode};
use cs_sim::control::LocalSeatId;
use cs_types::Tick;
use cs_types::input::{Action, DeviceClass, DeviceId, FlightCommand, Key};
use cs_types::keymap::ControlsMap;

/// The controls of one SideWinder input report, in the stick's raw units.
#[derive(Clone, Copy)]
struct Raw {
    x: i32,
    y: i32,
    twist: i32,
    throttle: i32,
    /// `0..=7` clockwise from up, `8` centred (the null value).
    hat: i32,
    /// Bit `n` is button `n + 1`.
    buttons: u32,
}

/// The owner's stick at rest, as recorded on 2026-10-23.
const REST: Raw = Raw {
    x: 0,
    y: -22,
    twist: 0,
    throttle: 127,
    hat: 8,
    buttons: 0,
};

/// One input report in the stick's own layout, built through its descriptor.
fn report(d: &Descriptor, raw: Raw) -> Report {
    let mut r = d.new_report(ReportKind::Input, 1);
    let gd = |id| usage(PAGE_GENERIC_DESKTOP, id);
    for (u, v) in [
        (gd(0x30), raw.x),
        (gd(0x31), raw.y),
        (gd(0x35), raw.twist),
        (gd(0x36), raw.throttle),
        (gd(0x39), raw.hat),
    ] {
        let (f, i) = d.find(ReportKind::Input, 0, u).expect("the stick has it");
        r.set(f, i, v);
    }
    for b in 0..8 {
        let (f, i) = d
            .find(ReportKind::Input, 0, usage(PAGE_BUTTON, b + 1))
            .expect("the stick has eight buttons");
        r.set(f, i, i32::from(raw.buttons & (1 << b) != 0));
    }
    r
}

fn decode(raw: Raw) -> JoystickState {
    let d = sidewinder_ff2();
    let mut st = JoystickState::default();
    Layout::new(&d).decode(&report(&d, raw), &mut st);
    st
}

fn sidewinder(same_model: usize, serial: Option<&str>) -> Candidate {
    Candidate {
        name: "SideWinder Force Feedback 2 Joystick".into(),
        vid: 0x045e,
        pid: 0x001b,
        serial: serial.map(str::to_owned),
        path: CString::new("DevSrvsID:4294971234").expect("no NUL"),
        same_model,
    }
}

/// The mission's session, its joystick seam and the thread's sender.
struct Rig {
    session: InputSession,
    joysticks: HostJoysticks,
    thread: std::sync::mpsc::Sender<JoystickMessage>,
    delivered: Vec<Action>,
    /// Other devices' events for the next frame.
    extra: Vec<DeviceEvent>,
}

impl Rig {
    fn new() -> Self {
        let map = ControlsMap::original_default().to_action_map(0.5);
        let (thread, rx) = channel();
        Self {
            session: InputSession::new(map, LocalSeatId(0), SessionMode::SinglePlayer, Tick(0)),
            joysticks: HostJoysticks::from_messages(HostJoystickOptions::default(), rx),
            thread,
            delivered: Vec::new(),
            extra: Vec::new(),
        }
    }

    fn connect(&mut self) {
        self.thread
            .send(JoystickMessage::Connected {
                info: sidewinder(1, None),
                axes: vec![0, 1, 2, 3],
                buttons: 8,
                hat: true,
                force_feedback: true,
                force_feedback_error: None,
            })
            .expect("the seam listens");
    }

    /// One render frame of two fixed ticks, the way `pump_platform_input` runs it.
    fn frame(&mut self, raw: Option<Raw>) {
        if let Some(raw) = raw {
            self.thread
                .send(JoystickMessage::State(decode(raw)))
                .expect("the seam listens");
        }
        self.joysticks.drain(&mut self.session);
        let mut events = std::mem::take(&mut self.extra);
        events.extend(self.joysticks.take_events());
        let outcome = self
            .session
            .pump_frame(FrameInput::Devices(&events), 2)
            .expect("the frame is accepted");
        self.delivered.extend(outcome.delivered);
    }

    fn axis(&self, command: FlightCommand) -> f32 {
        self.session.controls().axis(command).unwrap_or(0.0)
    }

    fn throttle(&self) -> f32 {
        self.session.throttle().position()
    }

    fn edges(&self, action: Action) -> usize {
        self.delivered.iter().filter(|a| **a == action).count()
    }
}

#[test]
fn accept_sidewinder_ffb2_descriptor_is_the_owners_stick() {
    // The bundled capture is what the owner's stick returned, byte for byte
    // (1343 bytes); the decoder finds every control in it.
    assert_eq!(sidewinder_ff2_bytes().len(), 1343);
    let layout = Layout::new(&sidewinder_ff2());
    assert_eq!((layout.axis_count(), layout.button_count()), (4, 8));
    assert!(layout.has_hat());
    assert!(cs_app::input::hid::pid::supports(&sidewinder_ff2()));
}

#[test]
fn accept_sidewinder_ffb2_axes_fly_the_original_assignment_with_project_signs() {
    let mut rig = Rig::new();
    rig.connect();
    rig.frame(Some(REST));
    assert!(
        rig.axis(FlightCommand::Pitch).abs() < 1e-6,
        "the resting offset is inside the dead zone"
    );
    assert!(rig.axis(FlightCommand::Roll).abs() < 1e-6);

    // Stick right is roll right, pulled back is nose up, twisted clockwise is
    // nose right: each at full deflection, and the other way at the other end.
    for (raw, command, expected) in [
        (Raw { x: 511, ..REST }, FlightCommand::Roll, 1.0),
        (Raw { x: -512, ..REST }, FlightCommand::Roll, -1.0),
        (Raw { y: 511, ..REST }, FlightCommand::Pitch, 1.0),
        (Raw { y: -512, ..REST }, FlightCommand::Pitch, -1.0),
        (Raw { twist: 31, ..REST }, FlightCommand::Yaw, 1.0),
        (Raw { twist: -32, ..REST }, FlightCommand::Yaw, -1.0),
    ] {
        rig.frame(Some(raw));
        let value = rig.axis(command);
        assert!(
            (value - expected).abs() < 0.01,
            "{command:?} reads {value}, expected {expected}"
        );
        rig.frame(Some(REST));
    }

    // The lever pushed forward (its minimum) is full throttle and pulled back
    // idle — the original's `0.5 - 0.5 * normalised` — reached exactly at both
    // ends and continuous between them.
    rig.frame(Some(Raw {
        throttle: 0,
        ..REST
    }));
    assert!((rig.throttle() - 1.0).abs() < 1e-6, "{}", rig.throttle());
    rig.frame(Some(Raw {
        throttle: 127,
        ..REST
    }));
    assert!(rig.throttle().abs() < 1e-6, "{}", rig.throttle());
    rig.frame(Some(Raw {
        throttle: 32,
        ..REST
    }));
    let mid = rig.throttle();
    assert!((0.7..0.8).contains(&mid), "a quarter forward is 3/4: {mid}");
}

#[test]
fn accept_sidewinder_ffb2_all_eight_buttons_and_every_hat_direction() {
    for b in 0..8u16 {
        let st = decode(Raw {
            buttons: 1 << b,
            ..REST
        });
        assert_eq!(st.pressed(), [b], "button {}", b + 1);
    }
    let expected: [&[u16]; 8] = [
        &[HAT_UP],
        &[HAT_UP, HAT_RIGHT],
        &[HAT_RIGHT],
        &[HAT_DOWN, HAT_RIGHT],
        &[HAT_DOWN],
        &[HAT_DOWN, HAT_LEFT],
        &[HAT_LEFT],
        &[HAT_UP, HAT_LEFT],
    ];
    for (value, buttons) in (0..8).zip(expected) {
        let st = decode(Raw { hat: value, ..REST });
        assert_eq!(st.hat, Some(HatDirection::ALL[value as usize]));
        assert_eq!(st.pressed(), buttons, "hat {value}");
    }
    assert_eq!(decode(REST).hat, None, "the null value is centred");

    // Through the mission's map: button 1 is the guns and button 2 the
    // rockets (the original's joystick column), and a held button is one edge.
    let mut rig = Rig::new();
    rig.connect();
    for _ in 0..4 {
        rig.frame(Some(Raw {
            buttons: 0b01,
            ..REST
        }));
    }
    rig.frame(Some(Raw {
        buttons: 0b10,
        ..REST
    }));
    assert_eq!(rig.edges(Action::Flight(FlightCommand::FirePrimary)), 1);
    assert_eq!(rig.edges(Action::Flight(FlightCommand::FireSecondary)), 1);
}

#[test]
fn accept_sidewinder_ffb2_unplug_releases_the_trigger_and_the_stick() {
    let mut rig = Rig::new();
    rig.connect();
    rig.frame(Some(Raw {
        x: 511,
        throttle: 32,
        buttons: 0b01,
        ..REST
    }));
    assert!(rig.axis(FlightCommand::Roll) > 0.9);
    let throttle = rig.throttle();
    let device = rig.joysticks.connected().expect("connected").device.clone();
    rig.thread
        .send(JoystickMessage::Disconnected {
            info: sidewinder(1, None),
            reason: "device disconnected".into(),
        })
        .expect("the seam listens");
    rig.frame(None);
    let losses = rig.session.take_losses();
    let loss = losses
        .iter()
        .find(|l| l.device == device)
        .expect("the unplug is reported");
    assert!(
        loss.released_edges
            .contains(&Action::Flight(FlightCommand::FirePrimary))
    );
    assert!(loss.neutralized_axes.contains(&FlightCommand::Roll));
    rig.frame(None);
    assert!(rig.axis(FlightCommand::Roll).abs() < 1e-6, "no stuck roll");
    assert!(
        (rig.throttle() - throttle).abs() < 1e-6,
        "the released lever leaves the throttle where it was, not at half: {}",
        rig.throttle()
    );
    assert!(rig.joysticks.connected().is_none());
}

#[test]
fn accept_sidewinder_ffb2_identity_is_stable_only_when_it_can_be() {
    assert_eq!(
        sidewinder(1, None).stable_identity().as_deref(),
        Some("usb:045e:001b"),
        "the only stick of its model is its model"
    );
    assert_eq!(
        sidewinder(2, None).stable_identity(),
        None,
        "two identical sticks without serials cannot be told apart"
    );
    assert_eq!(
        sidewinder(2, Some("A1")).stable_identity().as_deref(),
        Some("usb:045e:001b:serial=A1")
    );
    let mut rig = Rig::new();
    rig.connect();
    rig.frame(Some(REST));
    let device = &rig.joysticks.connected().expect("connected").device;
    assert!(device.is_stable());
    assert_eq!(device.stable_identity(), Some("usb:045e:001b"));
}

#[test]
fn accept_sidewinder_ffb2_lever_and_keys_share_the_throttle() {
    let mut rig = Rig::new();
    let keyboard = DeviceId::enumeration_fallback(DeviceClass::Keyboard, 0);
    rig.connect();
    rig.extra.push(DeviceEvent::Connected {
        device: keyboard.clone(),
    });
    rig.frame(Some(Raw {
        throttle: 0,
        ..REST
    }));
    assert!((rig.throttle() - 1.0).abs() < 1e-6);
    // A resting lever leaves the keys in charge: the original's `-` (command
    // 8) steps the throttle down, and the lever's unchanged reports do not
    // take it back.
    for keys in [vec![Key::Minus], Vec::new()] {
        rig.extra.push(DeviceEvent::KeyboardFrame {
            device: keyboard.clone(),
            keys,
        });
        rig.frame(Some(Raw {
            throttle: 0,
            ..REST
        }));
    }
    let stepped = rig.throttle();
    assert!(stepped < 0.99, "the key stepped it down: {stepped}");
    for _ in 0..3 {
        rig.frame(Some(Raw {
            throttle: 0,
            ..REST
        }));
    }
    assert!(
        (rig.throttle() - stepped).abs() < 1e-6,
        "an unmoved lever does not take the throttle back: {}",
        rig.throttle()
    );
    // One bit of lever jitter is not a movement; a real movement is.
    rig.frame(Some(Raw {
        throttle: 1,
        ..REST
    }));
    assert!((rig.throttle() - stepped).abs() < 1e-6, "jitter ignored");
    rig.frame(Some(Raw {
        throttle: 64,
        ..REST
    }));
    assert!(
        (rig.throttle() - 0.5).abs() < 0.01,
        "the moved lever has it: {}",
        rig.throttle()
    );
}

/// A transport with the SideWinder's PID behaviour: it hands out effect blocks
/// and records every report it is sent.
#[derive(Default)]
struct RecordingStick {
    writes: Vec<Vec<u8>>,
    next_block: u8,
}

impl Transport for RecordingStick {
    fn write(&mut self, r: &[u8]) -> Result<(), String> {
        self.writes.push(r.to_vec());
        Ok(())
    }
    fn set_feature(&mut self, _r: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn get_feature(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        match buf[0] {
            2 => {
                self.next_block = self.next_block.max(1) + 1;
                buf[1] = self.next_block;
                buf[2] = 1;
                Ok(5)
            }
            3 => {
                buf[1..5].copy_from_slice(&[0x46, 0x05, 0x0f, 0x00]);
                Ok(5)
            }
            _ => Err("unknown feature report".into()),
        }
    }
}

fn flying(airspeed_mps: f32) -> FlightCues {
    FlightCues {
        airspeed_mps,
        ..FlightCues::default()
    }
}

#[test]
fn accept_sidewinder_ffb2_effects_wait_for_flight_ramp_in_and_stop_on_every_exit() {
    let mut feel = FlightFeel::new();
    let mut out: Vec<Command> = Vec::new();
    for gate in [FeelGate::NoControl, FeelGate::Paused, FeelGate::NoDevice] {
        feel.update(gate, 0.016, &flying(100.0), &mut out);
    }
    assert!(
        out.is_empty(),
        "nothing plays before the player flies: {out:?}"
    );

    // The first frame of flight starts the spring soft, not at full stiffness.
    feel.update(FeelGate::Flying, 0.016, &flying(100.0), &mut out);
    assert!(feel.playing().contains(&ids::SPRING));
    assert!(feel.playing().contains(&ids::DAMPER));
    assert!(feel.spring() < 200, "no jolt: {}", feel.spring());
    let frames = (SPRING_RAMP_S / 0.016).ceil() as usize + 1;
    for _ in 0..frames {
        feel.update(FeelGate::Flying, 0.016, &flying(100.0), &mut out);
    }
    assert!(feel.spring() >= 5800, "ramped in: {}", feel.spring());

    // Firing, a stall and a hit add their effects; quiet flight stops them.
    let busy = FlightCues {
        airspeed_mps: 30.0,
        stall_margin: 0.2,
        guns_firing: true,
        hit: 1.0,
        impact: 0.0,
    };
    out.clear();
    feel.update(FeelGate::Flying, 0.016, &busy, &mut out);
    assert!(feel.playing().contains(&ids::GUNS));
    assert!(feel.playing().contains(&ids::BUFFET));
    assert!(out.iter().any(|c| matches!(
        c,
        Command::Start {
            id: ids::HIT,
            iterations: 1,
            ..
        }
    )));
    feel.update(FeelGate::Flying, 0.016, &flying(30.0), &mut out);
    assert!(!feel.playing().contains(&ids::GUNS));
    assert!(!feel.playing().contains(&ids::BUFFET));

    // Every way out of flight resets the device, once, and plays nothing more.
    for exit in [
        FeelGate::Paused,
        FeelGate::Unfocused,
        FeelGate::NoControl,
        FeelGate::NoDevice,
        FeelGate::Shutdown,
    ] {
        feel.update(FeelGate::Flying, 0.016, &busy, &mut out);
        out.clear();
        feel.update(exit, 0.016, &busy, &mut out);
        feel.update(exit, 0.016, &busy, &mut out);
        assert_eq!(out, [Command::Reset], "{exit:?} stops everything once");
        assert!(feel.playing().is_empty() && !feel.is_live());
    }
}

#[test]
fn accept_sidewinder_ffb2_the_driver_plays_the_feel_at_a_bounded_strength() {
    assert_eq!(
        ForceFeedbackSettings::default().gain_percent,
        DESIGNED_DEFAULT_GAIN_PERCENT
    );
    assert!(ForceFeedbackSettings::with_gain(100).is_ok());
    assert!(
        ForceFeedbackSettings::with_gain(101).is_err(),
        "a strength above full is refused, not clamped"
    );

    let mut pid = Pid::new(
        sidewinder_ff2(),
        RecordingStick::default(),
        DESIGNED_DEFAULT_GAIN_PERCENT,
        false,
    )
    .expect("the stick takes force feedback");
    // Start-up: reset, actuators on, device gain at 25% of 255 — and no effect.
    assert_eq!(pid.io.writes, [vec![12, 4], vec![12, 1], vec![13, 64]]);

    let mut feel = FlightFeel::new();
    let mut out: Vec<Command> = Vec::new();
    feel.update(FeelGate::Flying, 0.5, &flying(100.0), &mut out);
    feel.update(FeelGate::Paused, 0.016, &flying(100.0), &mut out);
    pid.io.writes.clear();
    for command in out {
        pid.command(command).expect("the stick accepts it");
    }
    let w = &pid.io.writes;
    assert!(
        w.iter().any(|r| r[0] == 3),
        "a Set Condition report for the spring and damper"
    );
    assert!(
        w.iter().any(|r| r[0] == 10 && r[2] == 1),
        "an Effect Operation start"
    );
    let reset = w
        .iter()
        .position(|r| *r == [12, 4])
        .expect("pausing resets the device");
    assert!(
        w[reset..].iter().all(|r| r[0] == 12),
        "after the reset only device control follows (actuators back on, no effect): {:?}",
        &w[reset..]
    );
}

#[test]
fn accept_sidewinder_ffb2_cli_flags() {
    let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let CliRequest::Mission(mission) = parse(words(&[
        "--cs-path",
        "/cs",
        "--mission",
        "M01",
        "--force-feedback-gain",
        "60",
    ])) else {
        panic!("a mission");
    };
    assert!(mission.joystick.enabled);
    assert_eq!(mission.joystick.force_feedback.gain_percent, 60);
    let CliRequest::Mission(off) = parse(words(&[
        "--cs-path",
        "/cs",
        "--mission",
        "M01",
        "--no-force-feedback",
    ])) else {
        panic!("a mission");
    };
    assert!(off.joystick.enabled && !off.joystick.force_feedback.enabled);
    assert!(matches!(
        parse(words(&[
            "--cs-path",
            "/cs",
            "--mission",
            "M01",
            "--force-feedback-gain",
            "150"
        ])),
        CliRequest::Invalid { .. }
    ));
    assert!(matches!(
        parse(words(&["--force-feedback-gain", "50"])),
        CliRequest::Invalid { .. }
    ));
    let CliRequest::JoystickProbe(probe) = parse(words(&[
        "--joystick-probe",
        "--joystick",
        "045e:001b",
        "--probe-seconds",
        "5",
    ])) else {
        panic!("a probe");
    };
    assert_eq!(probe.filter.as_deref(), Some("045e:001b"));
    assert_eq!(probe.seconds, 5);
    assert!(
        !probe.test_force_feedback,
        "the probe sends no force unasked"
    );
}
