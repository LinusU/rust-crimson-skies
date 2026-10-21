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

/// The original's `Cockpit views` command id (`MSG_LOOK_FORWARD`).
pub const VIEW_CYCLE_COMMAND_ID: u16 = 49;

/// The mission's current view. The variants are this project's own names for
/// the three original camera modes the command cycles; `original_mode` gives
/// each its measured mode index.
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
}

impl MissionViewMode {
    /// The original camera-mode index this view stands for.
    #[must_use]
    pub const fn original_mode(self) -> u32 {
        match self {
            Self::Chase => 0,
            Self::Cockpit => 7,
            Self::SecondEye => 6,
        }
    }

    /// The view after one press of the `Cockpit views` key: 0 → 7 → 6 → 0.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Chase => Self::Cockpit,
            Self::Cockpit => Self::SecondEye,
            Self::SecondEye => Self::Chase,
        }
    }

    /// Whether the view is placed at the pilot's eye (original modes 6 and 7).
    #[must_use]
    pub const fn at_pilot_eye(self) -> bool {
        !matches!(self, Self::Chase)
    }
}
