//! Keyboard-only and controller-only navigation (F52-A, AC01).
//!
//! A [`Navigator`] is bound to one [`NavDevice`]. It resolves a physical source
//! through an `ActionMap` in the UI-navigation context and drives the F45-A
//! [`FrontEnd`] with the result; a source of any other device is
//! [`NavStep::Ignored`], so a run through it proves that device alone can walk
//! the front end. Construction refuses a map that leaves the device without a
//! required UI action ([`navigation_gaps`]).
//!
//! `ActionMap::designed_default` binds no gamepad UI action, so
//! [`navigation_map`] adds a designed set (d-pad, `South` confirm, `East`
//! cancel, `Start` pause); it is not an original layout.

use std::fmt;

use cs_types::input::{Action as InputAction, ActionMap};
use cs_types::input::{
    Binding, BindingSource, BindingTarget, DeviceClass, GamepadButton, InputContext, UiAction,
};

use crate::ui::front_end::{Action, FrontEnd, Outcome, Refusal, Screen};

/// The UI actions a device needs to walk every screen.
pub const REQUIRED: [UiAction; 5] = [
    UiAction::Confirm,
    UiAction::Cancel,
    UiAction::NavigateUp,
    UiAction::NavigateDown,
    UiAction::Pause,
];

/// The device a navigation run is limited to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavDevice {
    /// Keys only.
    Keyboard,
    /// Gamepad buttons only.
    Controller,
}

impl NavDevice {
    fn class(self) -> DeviceClass {
        match self {
            Self::Keyboard => DeviceClass::Keyboard,
            Self::Controller => DeviceClass::Gamepad,
        }
    }

    fn accepts(self, source: BindingSource) -> bool {
        source.device_class() == self.class()
    }
}

/// The designed map with the gamepad navigation bindings added.
///
/// # Panics
///
/// Never in practice: the additions are conflict-free with the default map.
#[must_use]
pub fn navigation_map() -> ActionMap {
    let mut bindings = ActionMap::designed_default().bindings().to_vec();
    let pad = |button, action| Binding {
        source: BindingSource::GamepadButton(button),
        target: BindingTarget::Ui(action),
    };
    bindings.extend([
        pad(GamepadButton::DpadUp, UiAction::NavigateUp),
        pad(GamepadButton::DpadDown, UiAction::NavigateDown),
        pad(GamepadButton::DpadLeft, UiAction::NavigateLeft),
        pad(GamepadButton::DpadRight, UiAction::NavigateRight),
        pad(GamepadButton::South, UiAction::Confirm),
        pad(GamepadButton::East, UiAction::Cancel),
        pad(GamepadButton::Start, UiAction::Pause),
    ]);
    ActionMap::try_new(bindings).expect("the gamepad navigation bindings do not conflict")
}

/// The [`REQUIRED`] UI actions no source of `device` triggers in `map`.
#[must_use]
pub fn navigation_gaps(map: &ActionMap, device: NavDevice) -> Vec<UiAction> {
    REQUIRED
        .into_iter()
        .filter(|wanted| {
            !map.bindings().iter().any(|binding| {
                device.accepts(binding.source) && binding.target == BindingTarget::Ui(*wanted)
            })
        })
        .collect()
}

/// Why navigation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavError {
    /// The map leaves the device unable to reach every screen.
    Gaps {
        /// The device.
        device: NavDevice,
        /// The missing UI actions.
        missing: Vec<UiAction>,
    },
    /// Cancel was pressed on a screen with neither Cancel nor Back; Quit and
    /// the like are never fired by a Cancel press.
    NoEscape {
        /// The screen.
        screen: Screen,
    },
    /// The front end refused the resulting action.
    Front(Refusal),
}

impl fmt::Display for NavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gaps { device, missing } => {
                write!(f, "{device:?} cannot trigger {missing:?}")
            }
            Self::NoEscape { screen } => write!(f, "{screen:?} has no Cancel or Back"),
            Self::Front(refusal) => write!(f, "{refusal}"),
        }
    }
}

impl std::error::Error for NavError {}

/// What a press did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavStep {
    /// Another device, or an unbound source: nothing happened.
    Ignored,
    /// Focus moved.
    Focus,
    /// A discard prompt was answered "keep editing".
    KeptEditing,
    /// A front-end action was applied.
    Applied(Outcome),
}

/// Drives the front end from one device.
#[derive(Clone, Debug)]
pub struct Navigator {
    device: NavDevice,
    map: ActionMap,
}

impl Navigator {
    /// Builds a navigator.
    ///
    /// # Errors
    ///
    /// [`NavError::Gaps`] when `map` leaves `device` without a required action.
    pub fn new(device: NavDevice, map: ActionMap) -> Result<Self, NavError> {
        let missing = navigation_gaps(&map, device);
        if missing.is_empty() {
            Ok(Self { device, map })
        } else {
            Err(NavError::Gaps { device, missing })
        }
    }

    /// Presses one source.
    ///
    /// # Errors
    ///
    /// [`NavError`]; the front end is unchanged.
    pub fn press(
        &self,
        front_end: &mut FrontEnd,
        source: BindingSource,
    ) -> Result<NavStep, NavError> {
        if !self.device.accepts(source) {
            return Ok(NavStep::Ignored);
        }
        let Some(InputAction::Ui(action)) = self.map.resolve(InputContext::UiNavigation, source)
        else {
            return Ok(NavStep::Ignored);
        };
        let front = NavError::Front;
        if front_end.pending_discard().is_some() {
            return match action {
                UiAction::Confirm => front_end
                    .confirm_discard()
                    .map(NavStep::Applied)
                    .map_err(front),
                UiAction::Cancel => front_end
                    .keep_editing()
                    .map(|()| NavStep::KeptEditing)
                    .map_err(front),
                _ => Ok(NavStep::Ignored),
            };
        }
        match action {
            UiAction::NavigateUp | UiAction::NavigateLeft => {
                front_end.move_focus(false);
                Ok(NavStep::Focus)
            }
            UiAction::NavigateDown | UiAction::NavigateRight => {
                front_end.move_focus(true);
                Ok(NavStep::Focus)
            }
            UiAction::Confirm => front_end
                .activate_focus()
                .map(NavStep::Applied)
                .map_err(front),
            UiAction::Pause => front_end
                .apply(Action::Pause)
                .map(NavStep::Applied)
                .map_err(front),
            UiAction::Cancel => {
                let visible = front_end.visible_actions();
                let escape = [Action::Cancel, Action::Back]
                    .into_iter()
                    .find(|action| visible.contains(action))
                    .ok_or(NavError::NoEscape {
                        screen: front_end.screen(),
                    })?;
                front_end.apply(escape).map(NavStep::Applied).map_err(front)
            }
        }
    }
}
