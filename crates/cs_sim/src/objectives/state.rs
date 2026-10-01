//! Objective lifecycle states and legal transitions.

/// The seven states of an objective.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectiveState {
    /// Not shown to the player; reveal rules have not fired.
    Hidden,
    /// Shown, not yet being pursued.
    Pending,
    /// Being pursued.
    Active,
    /// Completed successfully (terminal).
    Succeeded,
    /// Failed (terminal).
    Failed,
    /// Shown as optional; never gates mission success.
    Optional,
    /// Replaced by another objective (terminal).
    Superseded,
}

impl ObjectiveState {
    /// Whether no further transition is legal.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Superseded)
    }

    /// Whether the player may see the objective.
    #[must_use]
    pub const fn is_visible(self) -> bool {
        !matches!(self, Self::Hidden)
    }

    /// Whether `self -> next` is a legal transition.
    #[must_use]
    pub const fn can_become(self, next: Self) -> bool {
        use ObjectiveState::{Active, Failed, Hidden, Optional, Pending, Succeeded, Superseded};
        matches!(
            (self, next),
            (Hidden, Pending | Active | Optional)
                | (Pending, Active | Superseded | Failed)
                | (Active, Succeeded | Failed | Superseded)
                | (Optional, Active | Succeeded | Failed | Superseded)
        )
    }
}

/// A refused transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IllegalTransition {
    pub from: ObjectiveState,
    pub to: ObjectiveState,
}

/// One objective's state; the only way to change it is [`Self::transition`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectiveCell {
    state: ObjectiveState,
}

impl ObjectiveCell {
    /// A new objective in `initial`.
    #[must_use]
    pub const fn new(initial: ObjectiveState) -> Self {
        Self { state: initial }
    }

    #[must_use]
    pub const fn state(&self) -> ObjectiveState {
        self.state
    }

    /// Moves to `to`.
    ///
    /// # Errors
    ///
    /// [`IllegalTransition`] when the move is not allowed; the state is kept.
    pub fn transition(&mut self, to: ObjectiveState) -> Result<(), IllegalTransition> {
        if self.state.can_become(to) {
            self.state = to;
            Ok(())
        } else {
            Err(IllegalTransition {
                from: self.state,
                to,
            })
        }
    }
}
