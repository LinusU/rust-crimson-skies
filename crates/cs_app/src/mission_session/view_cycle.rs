//! The mission's view-cycle state: the original's `Cockpit views` command
//! (PARITY3-COCKPIT-VIEW.01, #1809).
//!
//! The original registers command id 49 (`F8`, joystick button 4 in its
//! default table; label `MSG_LOOK_FORWARD`) with the handler at `0x489340`
//! (registration `push 0x489340 / push 0x31` at `0x4896ba`). The handler reads
//! the camera-mode index at `[camera + 0x14c]` and calls the mode setter
//! `0x42c280` with:
//!
//! | current original mode | next original mode |
//! | --- | --- |
//! | 0 | 7 |
//! | 7 | 6 |
//! | 6 | 0 |
//! | anything else | the last mode of {0, 6, 7} entered (`[0x62137c]`, initially 6) |
//!
//! so F8 cycles 0 → 7 → 6 → 0. Modes 6 and 7 both reach the cockpit-eye
//! transform (`0x42c61d` → `0x42d980`); mode 0 is the other member of the
//! remembered set and is what the owner saw as the chase view. The original
//! names of the three modes are **not** measured, and neither is the mode a
//! mission starts in. The facts are committed in
//! `data/owner-static/engine-facts.toml` (`[[view_cycle]]`) and
//! `docs/findings/2026-10-21-parity3-cockpit-view-01-original-view-cycle.md`;
//! provenance `static`, never `verified_original`.
//!
//! This module owns only the state and the cycle. The cockpit eye, canopy and
//! panel that the non-chase modes draw are later stages of the parent task.

use bevy::prelude::Resource;

use super::look_view::MissionLook;

/// The original's `Cockpit views` command id (`MSG_LOOK_FORWARD`).
pub const VIEW_CYCLE_COMMAND_ID: u16 = 49;

/// The command ids of the original's external-camera keys
/// (`docs/findings/2026-10-21-parity3-camera-modes-01-measured.md`).
pub const CAMERA_DOWN_COMMAND_ID: u16 = 50;
/// `Camera back` (`F10`): original mode 2.
pub const CAMERA_BACK_COMMAND_ID: u16 = 51;
/// `Camera left` (`F11`): original mode 3.
pub const CAMERA_LEFT_COMMAND_ID: u16 = 52;
/// `Camera right` (`F12`): original mode 4.
pub const CAMERA_RIGHT_COMMAND_ID: u16 = 53;
/// `Chase view` (`F7`): original mode **9**, a held-point camera, not mode 0.
pub const CHASE_VIEW_COMMAND_ID: u16 = 54;
/// `Zoom in` (numpad `+`), polled while held.
pub const ZOOM_IN_COMMAND_ID: u16 = 67;
/// `Zoom out` (numpad `-`), polled while held.
pub const ZOOM_OUT_COMMAND_ID: u16 = 68;

/// The mission's current view. The variants are this project's own names for
/// the original camera modes the view keys select; `original_mode` gives each
/// its measured mode index.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MissionViewMode {
    /// Original mode 0: the third-person view the mission already draws.
    /// The project starts here; the original's start mode is unmeasured.
    #[default]
    Chase,
    /// Original mode 7: the first stop of the cycle, a cockpit-eye mode.
    Cockpit,
    /// Original mode 6: the second stop of the cycle, also a cockpit-eye
    /// mode. What distinguishes it from mode 7 is unmeasured, hence no name.
    SecondEye,
    /// Original mode 1 (`Camera down`, `F9`), placed from the target's
    /// velocity.
    ExternalDown,
    /// Original mode 2 (`Camera back`, `F10`): the chase routine with the
    /// back distances.
    ExternalBack,
    /// Original mode 3 (`Camera left`, `F11`): beside the aircraft.
    ExternalLeft,
    /// Original mode 4 (`Camera right`, `F12`): beside the aircraft.
    ExternalRight,
    /// Original mode 9 (`Chase view`, `F7`): a camera held at a world point.
    /// Kept distinct from [`Self::Chase`] (mode 0); that the owner's "chase"
    /// is mode 9 is the key's measured target, not an observed fact.
    HeldPoint,
}

impl MissionViewMode {
    /// The original camera-mode index this view stands for.
    #[must_use]
    pub const fn original_mode(self) -> u32 {
        match self {
            Self::Chase => 0,
            Self::ExternalDown => 1,
            Self::ExternalBack => 2,
            Self::ExternalLeft => 3,
            Self::ExternalRight => 4,
            Self::SecondEye => 6,
            Self::Cockpit => 7,
            Self::HeldPoint => 9,
        }
    }

    /// The view a view key selects, `None` for a command that is not one.
    #[must_use]
    pub const fn for_command(command_id: u16) -> Option<Self> {
        Some(match command_id {
            CAMERA_DOWN_COMMAND_ID => Self::ExternalDown,
            CAMERA_BACK_COMMAND_ID => Self::ExternalBack,
            CAMERA_LEFT_COMMAND_ID => Self::ExternalLeft,
            CAMERA_RIGHT_COMMAND_ID => Self::ExternalRight,
            CHASE_VIEW_COMMAND_ID => Self::HeldPoint,
            _ => return None,
        })
    }

    /// Whether this mode is one of the three the `Cockpit views` key cycles
    /// through (original modes 0, 7 and 6).
    #[must_use]
    pub const fn in_cycle(self) -> bool {
        matches!(self, Self::Chase | Self::Cockpit | Self::SecondEye)
    }

    /// The view after one press of the `Cockpit views` key: 0 → 7 → 6 → 0 for
    /// a mode inside the cycle; from any other mode, the last cycle mode
    /// entered (`remembered`).
    #[must_use]
    pub const fn next_from(self, remembered: Self) -> Self {
        match self {
            Self::Chase => Self::Cockpit,
            Self::Cockpit => Self::SecondEye,
            Self::SecondEye => Self::Chase,
            _ => remembered,
        }
    }

    /// [`Self::next_from`] with the initial memory (mode 6).
    #[must_use]
    pub const fn next(self) -> Self {
        self.next_from(Self::SecondEye)
    }

    /// Whether the view is placed at the pilot's eye (original modes 6 and 7).
    #[must_use]
    pub const fn at_pilot_eye(self) -> bool {
        matches!(self, Self::Cockpit | Self::SecondEye)
    }

    /// Whether the view is one of the external cameras this project places
    /// itself (original modes 1 to 4 and 9).
    #[must_use]
    pub const fn is_external_camera(self) -> bool {
        matches!(
            self,
            Self::ExternalDown
                | Self::ExternalBack
                | Self::ExternalLeft
                | Self::ExternalRight
                | Self::HeldPoint
        )
    }

    /// Whether switching from `self` to `to` keeps the running mode state
    /// (the original's keep table `0x621380`): 0 and 2 keep each other, 1 and
    /// 9 keep themselves, 3 and 4 keep each other, anything keeps 6 and 7.
    #[must_use]
    pub const fn keeps_state_into(self, to: Self) -> bool {
        use MissionViewMode::{
            Chase, Cockpit, ExternalBack, ExternalDown, ExternalLeft, ExternalRight, HeldPoint,
            SecondEye,
        };
        matches!(
            (self, to),
            (Chase | ExternalBack, Chase | ExternalBack)
                | (ExternalDown, ExternalDown)
                | (ExternalLeft | ExternalRight, ExternalLeft | ExternalRight)
                | (_, Cockpit | SecondEye)
                | (HeldPoint, HeldPoint)
        )
    }
}

/// The last cycle mode (0, 6 or 7) entered, which `Cockpit views` returns to
/// from a mode outside the cycle (`[0x62137c]`, initially mode 6).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CycleMemory(pub MissionViewMode);

impl Default for CycleMemory {
    fn default() -> Self {
        Self(MissionViewMode::SecondEye)
    }
}

/// The view state the original's camera keeps beside the mode: the zoom.
///
/// `target` moves by 2 per second while a zoom key is held, clamped to
/// `[0, 1]`; `smoothed` follows it at rate 1.5. Only the chase view (mode 0)
/// reads it, adding `smoothed * ZOOM_SCALE_M` to the camera distance.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct MissionZoom {
    /// The zoom target, `0` (in) to `1` (out).
    pub target: f32,
    /// The smoothed zoom the camera reads.
    pub smoothed: f32,
    /// Whether the zoom-in key is held this frame.
    pub in_held: bool,
    /// Whether the zoom-out key is held this frame.
    pub out_held: bool,
}

/// Zoom-target change per second while a zoom key is held (`0x42d8bd`).
pub const ZOOM_RATE_PER_S: f32 = 2.0;
/// Rate of the smoothed zoom towards its target (`0x42d95c`).
pub const ZOOM_SMOOTH_RATE: f32 = 1.5;
/// Extra chase distance at full zoom, in the original's distance unit
/// (`0x603390`). The unit is unmeasured; this project reads it as metres.
pub const ZOOM_SCALE_M: f32 = 10.0;

impl MissionZoom {
    /// One frame of `dt` seconds: moves the target by the held keys, clamps it
    /// and smooths. Zoom-out wins nothing over zoom-in: both held cancel.
    pub fn step(&mut self, dt: f32) {
        if self.in_held {
            self.target -= ZOOM_RATE_PER_S * dt;
        }
        if self.out_held {
            self.target += ZOOM_RATE_PER_S * dt;
        }
        self.target = self.target.clamp(0.0, 1.0);
        // `v = target + (v - target) * e^(-rate dt)`: the original's smoother.
        let keep = (-ZOOM_SMOOTH_RATE * dt).exp();
        self.smoothed = self.target + (self.smoothed - self.target) * keep;
    }

    /// Entering the chase view clears the zoom (`setter.zero_*`).
    pub fn clear(&mut self) {
        self.target = 0.0;
        self.smoothed = 0.0;
    }

    /// The extra distance the chase camera sits at, metres.
    #[must_use]
    pub fn extra_distance_m(&self) -> f32 {
        self.smoothed * ZOOM_SCALE_M
    }
}

/// Applies one view key: sets the mode, remembers a cycle mode, clears the
/// zoom and the look on entering the chase view and the look on entering a
/// pilot-eye view. Returns whether the mode changed.
pub fn select_view(
    view: &mut MissionViewMode,
    memory: &mut CycleMemory,
    zoom: &mut MissionZoom,
    look: &mut MissionLook,
    to: MissionViewMode,
) -> bool {
    let changed = *view != to;
    if to.in_cycle() {
        memory.0 = to;
    }
    if to == MissionViewMode::Chase {
        zoom.clear();
    }
    if to == MissionViewMode::Chase || to.at_pilot_eye() {
        look.clear();
    }
    *view = to;
    changed
}
