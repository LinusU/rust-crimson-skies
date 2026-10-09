//! The playtest's key bindings and the one mapping from the input session to
//! the production flight command.
//!
//! The bindings are the F22 [`ActionMap::designed_default`] with the two
//! changes the playtest's own meta keys need: `R` resets the playtest, so the
//! default's `R` throttle step is moved to `Left Shift`. Everything else
//! (`W`/`S` pitch, `Q`/`E` roll, `A`/`D` yaw, `F` throttle down, `1`/`4` idle
//! and full throttle, `L` Level-Off) is the shipped default, except that a held
//! flight key deflects the stick only [`KEY_DEFLECTION`] of the way: the
//! uncalibrated synthetic airframe pitched 80 degrees in two seconds at full
//! deflection, which is not flyable from a keyboard. Designed, not original:
//! the original game's bindings and its keyboard feel remain unknown.
//!
//! The original's Level-Off assist (command 47) is bound to Shift+L in the
//! original; the playtest binds the F22 default's plain `L` instead, because
//! `Left Shift` is already this map's throttle step-up and the action map has
//! no chord sources — a designed slot for the original command, not a
//! reproduction of the original's chord.

use cs_sim::flight::FlightInput;
use cs_types::input::{ActionMap, Binding, BindingSource, BindingTarget, FlightCommand, Key};

use crate::input::InputSession;

/// The throttle the aircraft spawns with and a reset returns to.
///
/// Designed, not measured: the original game's own spawn throttle was not
/// recovered. It is the level-cruise setting the synthetic fixed-wing declares
/// (`level_cruise` in `cs_sim::flight::synthetic`), and over original content
/// the original law starts and resets at the same declared cruise while its
/// engine slews toward whatever command the session holds
/// (`THROTTLE_SLEW_PER_S`). What that fraction is of the original's own cruise
/// throttle is unknown, so this is a development choice and says so.
pub const CRUISE_THROTTLE: f32 = 0.75;

/// How far a held flight key deflects its axis, `0..=1`. Designed, not
/// measured, and it applies to **both** scenes: over original content the
/// original law clamps the same deflection at its own boundary, and the
/// original's own keyboard scaling was not recovered (#796 records joystick
/// scaling as unknown), so nothing here is an original input rate.
pub const KEY_DEFLECTION: f32 = 0.5;

/// The playtest's action map: the designed default, with `R` freed for reset.
///
/// # Panics
///
/// Never in practice: the map is the validated default with one source
/// replaced, and the replacement source is unbound in the default.
#[must_use]
pub fn playtest_action_map() -> ActionMap {
    let mut bindings: Vec<Binding> = ActionMap::designed_default()
        .bindings()
        .iter()
        .copied()
        .filter(|binding| binding.source != BindingSource::Key(Key::R))
        .map(|binding| match (binding.source, binding.target) {
            (BindingSource::Key(_), BindingTarget::Axis { command, scale }) => Binding {
                source: binding.source,
                target: BindingTarget::Axis {
                    command,
                    scale: scale * KEY_DEFLECTION,
                },
            },
            _ => binding,
        })
        .collect();
    bindings.push(Binding {
        source: BindingSource::Key(Key::LeftShift),
        target: BindingTarget::Command(FlightCommand::ThrottleStepUp),
    });
    ActionMap::try_new(bindings).expect("the designed default stays conflict free without R")
}

/// The flight command the session currently holds.
///
/// The session's axes are the keyboard/mouse/gamepad deflections after the
/// F22 context gate, pause and focus policy, so a paused or unfocused session
/// reads as neutral here without this function knowing why. Pitch and roll
/// pass through (`S` is positive pitch, nose up; `E` is positive roll, right
/// wing down) and yaw is negated, because the flight model's positive yaw is
/// nose-left while the binding's positive `D` is nose-right.
#[must_use]
pub fn flight_command(session: &InputSession) -> FlightInput {
    let axis = |command| f64::from(session.controls().axis(command).unwrap_or(0.0));
    FlightInput::clamped(
        axis(FlightCommand::Pitch),
        axis(FlightCommand::Roll),
        -axis(FlightCommand::Yaw),
        f64::from(session.throttle().position()),
        false,
    )
    .expect("session axes are finite by construction")
}
