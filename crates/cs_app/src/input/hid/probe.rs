//! `cs --joystick-probe`: check a host joystick before flying with it
//! (SIDEWINDER-FFB2, #1968).
//!
//! It lists the host's HID joysticks, describes one through its own report
//! descriptor (controls found, PID force-feedback reports, whether the
//! descriptor is the bundled SideWinder Force Feedback 2 capture) and prints
//! its controls live, in the terms the game reads them. Without
//! `--test-force-feedback` it never takes over the force feedback and sends no
//! force. With it, it plays a short, low-strength test sequence and resets the
//! device afterwards, also when interrupted by a panic or an error.

use std::io::Write;
use std::time::{Duration, Instant};

use super::descriptor::{ReportKind, sidewinder_ff2_bytes};
use super::device::{self, Controller};
use super::effect::{Axis, Command, Condition, ConditionKind, Effect, EffectKind, Waveform};
use super::layout::{AXIS_THROTTLE, AXIS_TWIST, AXIS_X, AXIS_Y, JoystickState};
use super::pid;
use crate::cli::JoystickProbeRequest;

/// Why the probe failed.
#[derive(Debug)]
pub struct ProbeError(pub String);

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "joystick probe: {}", self.0)
    }
}

impl std::error::Error for ProbeError {}

fn err(text: impl Into<String>) -> ProbeError {
    ProbeError(text.into())
}

/// One line of live controls.
#[must_use]
pub fn describe_state(st: &JoystickState) -> String {
    let axis = |index: u16| {
        let i = usize::from(index);
        match (st.axes[i], st.raw_axes[i]) {
            (Some(v), Some(raw)) => format!("{v:+.3} ({raw:>4})"),
            _ => "  unknown   ".to_owned(),
        }
    };
    let buttons: String = (0..8)
        .map(|b| if st.buttons & (1 << b) != 0 { '1' } else { '.' })
        .collect();
    format!(
        "x {}  y {}  twist {}  throttle {}  hat {:<10}  buttons 1-8 {buttons}",
        axis(AXIS_X),
        axis(AXIS_Y),
        axis(AXIS_TWIST),
        axis(AXIS_THROTTLE),
        st.hat.map_or("centred", |h| h.label()),
    )
}

/// Run the probe.
///
/// # Errors
///
/// [`ProbeError`] when HID is unavailable, the named joystick is missing or
/// cannot be opened, or the force-feedback test is refused by the device.
pub fn run(request: &JoystickProbeRequest) -> Result<(), ProbeError> {
    let api = device::api().map_err(|e| err(format!("HID unavailable: {e}")))?;
    let found = device::candidates(&api);
    println!("backend: hidapi ({})", std::env::consts::OS);
    if found.is_empty() {
        println!("No HID joysticks, gamepads or multi-axis controllers found.");
        return Err(err("no joystick is connected"));
    }
    for c in &found {
        println!(
            "found: {c}  identity {}",
            c.stable_identity()
                .unwrap_or_else(|| "none (another of the same model is connected)".to_owned())
        );
    }
    let c = match &request.filter {
        Some(f) => found
            .iter()
            .find(|c| c.matches(f))
            .ok_or_else(|| err(format!("no joystick matches {f:?}")))?,
        None => &found[0],
    };
    let ff = request.test_force_feedback.then_some(request.gain_percent);
    let mut ctl = Controller::open(&api, c, ff).map_err(|e| {
        err(format!(
            "{e}. On macOS, a denied Input Monitoring permission (System Settings > Privacy & \
             Security) also fails here"
        ))
    })?;
    describe(&ctl);
    if let Some(path) = &request.save_descriptor {
        std::fs::write(path, &ctl.descriptor)
            .map_err(|e| err(format!("{}: {e}", path.display())))?;
        println!("report descriptor written to {}", path.display());
    }
    if request.test_force_feedback {
        test_force_feedback(&mut ctl, request.gain_percent)?;
    }
    live(&mut ctl, request.seconds)
}

fn describe(ctl: &Controller) {
    println!("opened: {}", ctl.info);
    println!(
        "  descriptor: {} bytes, {}",
        ctl.descriptor.len(),
        if ctl.descriptor == sidewinder_ff2_bytes() {
            "identical to the bundled SideWinder Force Feedback 2 capture"
        } else {
            "not the bundled SideWinder Force Feedback 2 capture"
        }
    );
    let has = |axis: u16| {
        if ctl.layout.has_axis(axis) {
            "yes"
        } else {
            "no"
        }
    };
    println!(
        "  controls: x {}, y {}, twist {}, throttle {}; {} buttons; hat {}",
        has(AXIS_X),
        has(AXIS_Y),
        has(AXIS_TWIST),
        has(AXIS_THROTTLE),
        ctl.layout.button_count(),
        if ctl.layout.has_hat() { "yes" } else { "no" }
    );
    for kind in [ReportKind::Input, ReportKind::Output, ReportKind::Feature] {
        println!("  {kind:?} reports: {:?}", ctl.desc.report_ids(kind));
    }
    let ff = if ctl.has_force_feedback() {
        "USB PID, taken over for the test".to_owned()
    } else if let Some(e) = &ctl.ff_error {
        format!("unavailable: {e}")
    } else if pid::supports(&ctl.desc) {
        "USB PID (not opened; add --test-force-feedback to play the test)".to_owned()
    } else {
        "none".to_owned()
    };
    println!("  force feedback: {ff}");
    for (name, value) in ctl.ff_info() {
        println!("    {name}: {value}");
    }
    for note in ctl.ff_notes() {
        println!("    note: {note}");
    }
}

fn live(ctl: &mut Controller, seconds: u32) -> Result<(), ProbeError> {
    println!(
        "Move the stick, twist it, move the throttle lever, push the hat, press every button \
         ({seconds} s). The SideWinder reports only changes: an axis reads unknown until moved."
    );
    let end = Instant::now() + Duration::from_secs(u64::from(seconds));
    let mut st = JoystickState::default();
    let mut last = String::new();
    let mut printed = Instant::now();
    while Instant::now() < end {
        ctl.read(&mut st, 20)
            .map_err(|e| err(format!("{} stopped reporting: {e}", ctl.info)))?;
        let line = describe_state(&st);
        if line != last && printed.elapsed() >= Duration::from_millis(100) {
            println!("  {line}");
            let _ = std::io::stdout().flush();
            last = line;
            printed = Instant::now();
        }
    }
    println!(
        "reports read: {} controls, {} force-feedback states ({} with an effect playing)",
        ctl.stats.controls, ctl.stats.effect_states, ctl.stats.playing
    );
    Ok(())
}

fn condition(kind: ConditionKind, coefficient: i32) -> Effect {
    let c = Condition {
        offset: 0,
        positive_coefficient: coefficient,
        negative_coefficient: coefficient,
        positive_saturation: 10000,
        negative_saturation: 10000,
        dead_band: 0,
    };
    effect(
        EffectKind::Condition {
            kind,
            axes: vec![c, c],
        },
        0,
        1_500_000,
    )
}

fn effect(kind: EffectKind, direction: u32, duration_us: u32) -> Effect {
    Effect {
        kind,
        duration_us,
        sample_period_us: 0,
        gain: 10000,
        trigger_button: None,
        trigger_repeat_us: 0,
        axes: vec![Axis::X, Axis::Y],
        direction,
        envelope: None,
        start_delay_us: 0,
    }
}

/// The test sequence: each step is downloaded, played once and given time to
/// finish while the stick's PID state reports are counted.
fn test_force_feedback(ctl: &mut Controller, gain: u32) -> Result<(), ProbeError> {
    if !ctl.has_force_feedback() {
        return Err(err(format!(
            "{} has no USB PID force feedback to test{}",
            ctl.info,
            ctl.ff_error
                .as_ref()
                .map(|e| format!(" ({e})"))
                .unwrap_or_default()
        )));
    }
    println!("Force-feedback test at {gain}% strength. Hold the grip loosely.");
    let periodic = |waveform, magnitude, period_us| EffectKind::Periodic {
        waveform,
        magnitude,
        offset: 0,
        phase: 0,
        period_us,
    };
    let steps: Vec<(&str, Effect)> = vec![
        ("centring spring", condition(ConditionKind::Spring, 6000)),
        (
            "damper (move the stick)",
            condition(ConditionKind::Damper, 6000),
        ),
        (
            "push right",
            effect(EffectKind::Constant { magnitude: 6000 }, 9000, 600_000),
        ),
        (
            "push left",
            effect(EffectKind::Constant { magnitude: 6000 }, 27000, 600_000),
        ),
        (
            "push forward",
            effect(EffectKind::Constant { magnitude: 6000 }, 0, 600_000),
        ),
        (
            "push back",
            effect(EffectKind::Constant { magnitude: 6000 }, 18000, 600_000),
        ),
        (
            "stall buffet (sine)",
            effect(periodic(Waveform::Sine, 5000, 80_000), 0, 1_200_000),
        ),
        (
            "gun rattle (square)",
            effect(periodic(Waveform::Square, 3000, 60_000), 18000, 1_200_000),
        ),
    ];
    let mut st = JoystickState::default();
    for (id, (name, e)) in (1u32..).zip(steps) {
        println!("  {name}");
        let wait_ms = u64::from(e.duration_us / 1000) + 300;
        let result = ctl
            .force(Command::Download { id, effect: e })
            .and_then(|()| {
                ctl.force(Command::Start {
                    id,
                    iterations: 1,
                    solo: false,
                })
            });
        if let Err(e) = result {
            let _ = ctl.force(Command::Reset);
            return Err(err(format!("{name}: the device refused it: {e}")));
        }
        // How far the stick moved from where the step found it, the largest
        // deflection each way, so a push's direction is in the log.
        let start = (st.axes[usize::from(AXIS_X)], st.axes[usize::from(AXIS_Y)]);
        let (mut dx, mut dy) = (0.0f32, 0.0f32);
        let end = Instant::now() + Duration::from_millis(wait_ms);
        while Instant::now() < end {
            let _ = ctl.read(&mut st, 10);
            if let (Some(x0), Some(x)) = (start.0, st.axes[usize::from(AXIS_X)])
                && (x - x0).abs() > dx.abs()
            {
                dx = x - x0;
            }
            if let (Some(y0), Some(y)) = (start.1, st.axes[usize::from(AXIS_Y)])
                && (y - y0).abs() > dy.abs()
            {
                dy = y - y0;
            }
        }
        println!("    stick moved: x {dx:+.2} (+ right), y {dy:+.2} (+ back)");
        let _ = ctl.force(Command::Unload { id });
    }
    ctl.force(Command::Reset)
        .map_err(|e| err(format!("reset after the test: {e}")))?;
    println!(
        "  done; the stick's PID state reports said an effect was playing {} times",
        ctl.stats.playing
    );
    Ok(())
}
