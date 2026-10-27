//! The labelled modern control profile on the flight input path (F52-W3).
//!
//! Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! AC03/AC04. [`cs_content::settings::GameplayInputs`] is the only view of the
//! settings the simulation may read; this module is the one place the input
//! path reads it. [`apply_assists`] turns a base [`ActionMap`] and that view
//! into the map the session flies with:
//!
//! * Under the original rules (`assists` empty) the map has **no** modern
//!   binding: the mouse motion axes and the modern controller layout are
//!   removed or left at the base layout, whatever the stored options say.
//! * [`ModernAssist::MouseFlight`] adds the mouse motion axes and nothing else.
//! * [`ModernAssist::ControllerFlight`] re-points the two sticks and nothing
//!   else.
//! * [`ModernAssist::Fov`] is not an input mapping; [`fov_degrees`] hands the
//!   camera its value.
//!
//! **Every mapping here is a project-designed improvement, not the original's.**
//! The original PC control bindings and field of view are unknown (F52-A limit
//! 1, F22); nothing below claims to reproduce them, and the record of a run
//! that used one names it (`FidelityLabel::metadata`).

use cs_content::settings::{GameplayInputs, ModernAssist};
use cs_types::input::{
    ActionMap, Binding, BindingSource, BindingTarget, FlightCommand, GamepadAxis, MouseAxis,
};

use super::session::InputSession;

/// The designed mouse-flight mapping: motion X yaws, motion Y pitches.
const MOUSE_FLIGHT: [(MouseAxis, FlightCommand, f32); 2] = [
    (MouseAxis::X, FlightCommand::Yaw, 1.0),
    (MouseAxis::Y, FlightCommand::Pitch, -1.0),
];

/// The designed modern controller layout: the right stick banks and pitches,
/// the left stick's horizontal axis yaws.
const CONTROLLER_FLIGHT: [(GamepadAxis, FlightCommand, f32); 3] = [
    (GamepadAxis::RightStickX, FlightCommand::Roll, 1.0),
    (GamepadAxis::RightStickY, FlightCommand::Pitch, -1.0),
    (GamepadAxis::LeftStickX, FlightCommand::Yaw, 1.0),
];

fn axis(source: BindingSource, command: FlightCommand, scale: f32) -> Binding {
    Binding {
        source,
        target: BindingTarget::Axis { command, scale },
    }
}

fn is_mouse_motion(source: BindingSource) -> bool {
    matches!(
        source,
        BindingSource::MouseAxis(MouseAxis::X | MouseAxis::Y)
    )
}

fn is_stick(source: BindingSource) -> bool {
    matches!(
        source,
        BindingSource::GamepadAxis(
            GamepadAxis::LeftStickX
                | GamepadAxis::LeftStickY
                | GamepadAxis::RightStickX
                | GamepadAxis::RightStickY
        )
    )
}

/// The action map `inputs` asks for, built from `base`.
///
/// Mouse motion and the two sticks are dropped from `base` and put back only
/// by the assist that owns them, so the result depends on `inputs` alone for
/// those sources; every other binding (keyboard, buttons, triggers, joystick)
/// is copied unchanged.
///
/// With no controller assist the stick bindings of `base` are kept as they
/// are: they are the existing designed layout, not an assist.
#[must_use]
pub fn apply_assists(base: &ActionMap, inputs: &GameplayInputs) -> ActionMap {
    let mouse = inputs.assists.contains(&ModernAssist::MouseFlight);
    let controller = inputs.assists.contains(&ModernAssist::ControllerFlight);

    let kept: Vec<Binding> = base
        .bindings()
        .iter()
        .copied()
        .filter(|binding| {
            !is_mouse_motion(binding.source) && !(controller && is_stick(binding.source))
        })
        .collect();
    let mut bindings = kept.clone();
    if mouse {
        for (source, command, scale) in MOUSE_FLIGHT {
            bindings.push(axis(BindingSource::MouseAxis(source), command, scale));
        }
    }
    if controller {
        for (source, command, scale) in CONTROLLER_FLIGHT {
            bindings.push(axis(BindingSource::GamepadAxis(source), command, scale));
        }
    }
    // Only sources of the dropped kinds were replaced, each exactly once, so
    // the base map's own validity carries over. Were that ever false, fall
    // back to the map without any modern binding, never to `base`, which may
    // still bind mouse motion under the original rules.
    ActionMap::try_new(bindings)
        .or_else(|_| ActionMap::try_new(kept))
        .unwrap_or_else(|_| ActionMap::try_new(Vec::new()).expect("an empty map is valid"))
}

/// The non-original field of view the camera may use, in degrees; `None`
/// keeps the original's (unknown, F52-A limit 1).
#[must_use]
pub fn fov_degrees(inputs: &GameplayInputs) -> Option<u16> {
    inputs.assists.iter().find_map(|assist| match assist {
        ModernAssist::Fov(degrees) => Some(*degrees),
        _ => None,
    })
}

/// Installs the map `inputs` asks for into `session`.
///
/// Call it before the first frame, as [`super::InputBindings::set_map`]
/// requires; `base` is the layout the session was built from.
pub fn install(session: &mut InputSession, base: &ActionMap, inputs: &GameplayInputs) {
    let map = apply_assists(base, inputs);
    session.collector_mut().bindings_mut().set_map(map);
}
