//! A rebinding session that cannot strand the user (F52-A, non-negotiable
//! behavior 3).
//!
//! Edits go to a working copy of the `ActionMap`. [`RemapSession::cancel`]
//! returns to the last committed map, [`RemapSession::reset`] to the designed
//! one, and [`RemapSession::commit`] refuses a map that leaves the keyboard or
//! the controller unable to navigate the front end
//! ([`super::navigation::navigation_gaps`]). A rebind that conflicts with an
//! existing binding is refused, never resolved by quietly stealing the source.

use std::fmt;

use cs_types::input::DeviceClass;
use cs_types::input::{ActionMap, ActionMapError, Binding, BindingSource, BindingTarget};

use super::navigation::{NavDevice, navigation_gaps};

/// Why a remap step was refused. The session is unchanged.
#[derive(Clone, Debug, PartialEq)]
pub enum RemapError {
    /// The resulting map is invalid (a conflict, a bad axis).
    Map(ActionMapError),
    /// Committing would leave a device unable to navigate.
    Strands {
        /// The device.
        device: NavDevice,
        /// The UI actions it would lose.
        missing: Vec<cs_types::input::UiAction>,
    },
}

impl fmt::Display for RemapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Map(error) => write!(f, "{error}"),
            Self::Strands { device, missing } => {
                write!(f, "{device:?} would lose {missing:?}")
            }
        }
    }
}

impl std::error::Error for RemapError {}

/// An in-progress rebinding.
#[derive(Clone, Debug)]
pub struct RemapSession {
    committed: ActionMap,
    designed: ActionMap,
    working: ActionMap,
}

impl RemapSession {
    /// Starts from the committed map; `designed` is what [`Self::reset`] restores.
    #[must_use]
    pub fn new(committed: ActionMap, designed: ActionMap) -> Self {
        Self {
            working: committed.clone(),
            committed,
            designed,
        }
    }

    /// The map as edited so far.
    #[must_use]
    pub fn working(&self) -> &ActionMap {
        &self.working
    }

    /// Binds `target` to `source`, replacing `target`'s bindings on the same
    /// device class.
    ///
    /// # Errors
    ///
    /// [`RemapError::Map`] if the result conflicts.
    pub fn rebind(
        &mut self,
        target: BindingTarget,
        source: BindingSource,
    ) -> Result<(), RemapError> {
        let mut bindings = self.without(target, source.device_class());
        bindings.push(Binding { source, target });
        self.working = ActionMap::try_new(bindings).map_err(RemapError::Map)?;
        Ok(())
    }

    /// Removes `target`'s bindings on one device class.
    pub fn unbind(&mut self, target: BindingTarget, class: DeviceClass) {
        let bindings = self.without(target, class);
        self.working = ActionMap::try_new(bindings).expect("removing a binding cannot conflict");
    }

    fn without(&self, target: BindingTarget, class: DeviceClass) -> Vec<Binding> {
        self.working
            .bindings()
            .iter()
            .copied()
            .filter(|b| !(b.target == target && b.source.device_class() == class))
            .collect()
    }

    /// Drops every uncommitted edit.
    pub fn cancel(&mut self) {
        self.working = self.committed.clone();
    }

    /// Restores the designed map in the working copy; still needs a commit.
    pub fn reset(&mut self) {
        self.working = self.designed.clone();
    }

    /// Makes the working map the committed one.
    ///
    /// # Errors
    ///
    /// [`RemapError::Strands`]; the working copy is kept so it can be fixed,
    /// cancelled or reset.
    pub fn commit(&mut self) -> Result<&ActionMap, RemapError> {
        for device in [NavDevice::Keyboard, NavDevice::Controller] {
            let missing = navigation_gaps(&self.working, device);
            if !missing.is_empty() {
                return Err(RemapError::Strands { device, missing });
            }
        }
        self.committed = self.working.clone();
        Ok(&self.committed)
    }
}
