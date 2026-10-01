//! Capital-ship bays and their explicit time-varying weakpoint states
//! (F35-A).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Non-negotiable behavior 2: a broadside or weapon bay opening is an
//! explicit, tick-indexed weakpoint state, and a *closed* bay is not an
//! always-hittable invisible health bar. The contract here is a pure cycle —
//! [`ExposureWindow::state_at`] takes one [`Tick`] and returns the
//! [`BayState`] — plus the [`Bay`] record tying that window to a subsystem
//! key. Whether a hit lands is the F35-B weakpoint resolver's job; it must
//! consult [`ExposureWindow::is_weakpoint`] rather than assume a bay is
//! always open. Destruction is separate from exposure: a destroyed bay is
//! [`BayState::Destroyed`] at every tick.
//!
//! The cycle shape and every duration are newly authored design, not a
//! measured original rule.

use cs_types::Tick;

use super::subsystem::{SubsystemKey, SubsystemKind};

/// Which kind of bay a record describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BayKind {
    /// A broadside or weapon bay.
    Weapon,
    /// A hangar / launch bay.
    Launch,
}

impl BayKind {
    /// The subsystem kind a bay of this kind must be.
    #[must_use]
    pub const fn subsystem_kind(self) -> SubsystemKind {
        match self {
            Self::Weapon => SubsystemKind::WeaponBay,
            Self::Launch => SubsystemKind::LaunchBay,
        }
    }
}

/// The weakpoint state of an intact bay at one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BayState {
    /// Closed: not a weakpoint.
    Concealed,
    /// Opening: the doors are moving; still not an exposed weakpoint.
    Opening,
    /// Open: the bay is an exposed weakpoint.
    Exposed,
    /// Closing: the doors are moving shut; no longer an exposed weakpoint.
    Closing,
    /// Destroyed: the bay is gone and never a weakpoint again.
    Destroyed,
}

/// Why an [`ExposureWindow`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposureError {
    /// A window whose open phase is empty never exposes a weakpoint.
    NoExposedTicks,
    /// The sum of all phases was zero, so no cycle exists.
    ZeroCycle,
    /// The four phases summed past `u64::MAX`. A cycle that cannot be
    /// represented is refused rather than wrapped into a shorter (or
    /// zero-length) one.
    CycleOverflow,
}

/// A repeating open/close cycle for one bay, in simulation ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExposureWindow {
    concealed_ticks: u64,
    opening_ticks: u64,
    exposed_ticks: u64,
    closing_ticks: u64,
}

impl ExposureWindow {
    /// Validates and wraps a cycle. At least one tick must be exposed and
    /// the cycle must not be zero-length or overflow the tick counter.
    ///
    /// # Errors
    ///
    /// [`ExposureError::NoExposedTicks`], [`ExposureError::ZeroCycle`] or
    /// [`ExposureError::CycleOverflow`].
    pub fn try_new(
        concealed_ticks: u64,
        opening_ticks: u64,
        exposed_ticks: u64,
        closing_ticks: u64,
    ) -> Result<Self, ExposureError> {
        let mut total = 0_u64;
        for part in [concealed_ticks, opening_ticks, exposed_ticks, closing_ticks] {
            total = total
                .checked_add(part)
                .ok_or(ExposureError::CycleOverflow)?;
        }
        if total == 0 {
            return Err(ExposureError::ZeroCycle);
        }
        if exposed_ticks == 0 {
            return Err(ExposureError::NoExposedTicks);
        }
        Ok(Self {
            concealed_ticks,
            opening_ticks,
            exposed_ticks,
            closing_ticks,
        })
    }

    /// The number of ticks in one full cycle.
    #[must_use]
    pub fn cycle_ticks(&self) -> u64 {
        self.concealed_ticks + self.opening_ticks + self.exposed_ticks + self.closing_ticks
    }

    /// The tick index inside the cycle for `tick`.
    #[must_use]
    pub fn phase_ticks(&self, tick: Tick) -> u64 {
        tick.0 % self.cycle_ticks()
    }

    /// The state of an intact bay at `tick`. Never [`BayState::Destroyed`]:
    /// destruction is the subsystem's state, not the cycle's.
    #[must_use]
    pub fn state_at(&self, tick: Tick) -> BayState {
        let phase = self.phase_ticks(tick);
        let open_at = self.concealed_ticks + self.opening_ticks;
        let close_at = open_at + self.exposed_ticks;
        if phase < self.concealed_ticks {
            BayState::Concealed
        } else if phase < open_at {
            BayState::Opening
        } else if phase < close_at {
            BayState::Exposed
        } else {
            BayState::Closing
        }
    }

    /// Whether an intact bay exposes a hittable weakpoint at `tick`.
    /// Only [`BayState::Exposed`] counts; the opening and closing frames do
    /// not.
    #[must_use]
    pub fn is_weakpoint(&self, tick: Tick) -> bool {
        self.state_at(tick) == BayState::Exposed
    }
}

/// One bay: its subsystem key, its kind and its exposure cycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bay {
    /// The subsystem this bay is.
    pub key: SubsystemKey,
    /// Whether it is a weapon bay or a launch bay.
    pub kind: BayKind,
    /// The authored open/close cycle.
    pub exposure: ExposureWindow,
}

impl Bay {
    /// Assembles a bay record.
    #[must_use]
    pub fn new(key: SubsystemKey, kind: BayKind, exposure: ExposureWindow) -> Self {
        Self {
            key,
            kind,
            exposure,
        }
    }
}
