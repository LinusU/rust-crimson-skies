//! Objective lifecycle states and legal transitions.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39 owns objective state), stages `### F39-A` (the vocabulary) and
//! `### F39-D` (acceptance case AC04: complete supported objectives out of the
//! common order without deadlocking the program).
//!
//! # The state machine is order-independent (F39-D)
//!
//! The table below answers *"may `self` become `next`"*, and nothing in it
//! mentions **which** objectives a mission completes first. That matters,
//! because a mission's objectives are completed in whatever order the player
//! reaches them, and AC04 requires a supported objective to be completable out
//! of the common order without deadlocking the program.
//!
//! A missing row is not a neutral omission: it is a latch that can never be
//! set. `Pending` could once fail and supersede but never succeed, so an
//! objective a reveal rule had just shown — `Hidden` to `Pending`, never
//! `Active` — could not be completed at all, and a completion whose
//! `on_complete` requests the mission outcome could never request it. The
//! mission then had no reachable terminal outcome: a deadlock produced by the
//! state machine rather than by the program.
//! `Pending -> Succeeded` is therefore legal, and the rule the table now
//! carries is the one F39-D measured the engine against: from any state a
//! declaration can reach, every terminal outcome and `Active` is still
//! reachable, so nothing an objective's own history did can close the door on
//! completing it.
//! [`ObjectiveState::is_outcome_reachable`] is that rule as a queryable fact,
//! so a table edit that reintroduces an order-dependent row fails
//! `accept_f39_d_the_state_machine_is_order_independent`.
//!
//! `Hidden` stays the exception, and it stays the exception **by rule**: the
//! only moves out of `Hidden` are the reveal itself and the states a
//! declaration may be *born* in, so a declared action can never complete an
//! objective that was never shown. `cs_content::objectives` enforces the same
//! fact from the other side, by refusing a declared move or watch that targets
//! `Hidden` or `Pending`.
//!
//! # `Optional` is reachable again, by a completion effect
//!
//! The two rows into `Optional` that F39-E5 added — `Pending -> Optional` and
//! `Active -> Optional` — exist for one declared case: **completing one
//! objective puts another aside**. The nap that first needed them has since
//! moved to [`ObjectiveState::Napping`] below, on measured grounds; the rows
//! stay because a *declared* set-aside move still lands there, and a
//! *wakeup* moves it back out to `Active`, which the table already allowed.
//! Before them, `Optional` was reachable only from `Hidden`, so such a move
//! could only ever land on an objective the player had not been shown, and the
//! state table could not express "this objective is set aside" at all.
//!
//! Adding rows cannot break the order-independence above: nothing is removed,
//! `Optional` already reached all three terminal states and `Active`, and
//! `accept_f39_e5_a_set_aside_objective_can_still_be_completed_or_resumed`
//! re-checks the property from the states that can now reach `Optional`.
//!
//! # `Napping` and `Asleep`: the engine's timed pause and sleep (M01-B-OBJ-1-FU1)
//!
//! The engine image measured (#1358) that a `NAP` completion effect writes
//! engine state 2 — a **timed pause** — and a `SLEEP` effect writes engine
//! state 3, and that a wake moves an inactive target back to engine state 1
//! whatever it was doing. Two states carry those writes, named after the
//! effects that set them like `cs_content::objectives_engine::EngineState`
//! names them:
//!
//! * [`ObjectiveState::Napping`] — an objective a `NAP` put aside. It is not
//!   being pursued, and the declared number the `NAP` carried is its nap time
//!   in the engine's own unit — a unit this runtime never converts into ticks,
//!   so nothing here ends a nap by itself; a wake or a declared move does.
//! * [`ObjectiveState::Asleep`] — an objective a `SLEEP` effect set. (The
//!   engine also enters state 3 when an objective *completes*; this table's
//!   completion still lands in the sheet's terminal states, which is the
//!   designed vocabulary this runtime kept.)
//!
//! The rows are exactly the measured writes: a nap may land on any live shown
//! objective and on an asleep one (the engine's write carries no state guard),
//! a sleep likewise, and both states leave through a wake (`-> Active`) or a
//! declared completion. `Dormant` is deliberately **not** a state here: the
//! `BEGIN_DORMANT` timers that would end it are unmeasured, and a state this
//! runtime could enter but never leave by a measured rule would be an
//! invention. F39-D's order-independence is extended to the two new states and
//! pinned by `accept_m01_b_obj_1_fu1_napping_and_asleep_keep_every_outcome_reachable`;
//! `accept_f39_d_the_state_machine_is_order_independent` keeps checking the
//! original three.

/// The nine states of an objective.
///
/// Seven are the sheet's player-facing vocabulary; [`Self::Napping`] and
/// [`Self::Asleep`] are the two engine states the measured `NAP` and `SLEEP`
/// completion effects write (M01-B-OBJ-1-FU1), added so those effects carry
/// their measured semantics instead of borrowing a sheet state.
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
    /// Shown, but put aside by a `NAP` (engine state 2): a timed pause this
    /// runtime never ends by itself, because the nap time's unit is unmeasured
    /// and nothing converts it into ticks. A wake or a declared move ends it.
    Napping,
    /// Shown, but set to sleep (engine state 3) by a `SLEEP` effect. A wake or
    /// a declared move ends it.
    Asleep,
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
    ///
    /// Every row is about the two states alone. Which objectives a mission
    /// happens to complete first is not an input, so no completion depends on
    /// the order the player reached it (F39-D, AC04).
    #[must_use]
    pub const fn can_become(self, next: Self) -> bool {
        use ObjectiveState::{
            Active, Asleep, Failed, Hidden, Napping, Optional, Pending, Succeeded, Superseded,
        };
        matches!(
            (self, next),
            // `Hidden` leaves only through the reveal rule, which moves it to
            // `Pending` directly, or through a declaration it was born into.
            (Hidden, Pending | Active | Optional)
                // `Pending -> Succeeded` is the F39-D repair: an objective a
                // reveal rule has just shown, and which the player completed
                // before it was ever marked `Active`, used to be unable to
                // complete at all.
                | (Pending, Active | Succeeded | Superseded | Failed)
                | (Active, Succeeded | Failed | Superseded)
                // The two F39-E5 rows: a declared set-aside move lands on
                // `Optional` and a wakeup takes it out again (`Optional` ->
                // `Active`, which the last line already allowed). The nap that
                // first needed them now lands on `Napping` (M01-B-OBJ-1-FU1);
                // the rows stay for declared moves. No row leaves `Optional`
                // for a final state it could not already reach, so
                // order-independence is unchanged.
                | (Pending | Active, Optional)
                | (Optional, Active | Succeeded | Failed | Superseded)
                // The M01-B-OBJ-1-FU1 rows: the measured writes of the `NAP`
                // and `SLEEP` completion effects. Both land on any live shown
                // objective and on each other's state, because the engine's
                // writes carry no state guard; both leave through a wake or a
                // declared completion, which keeps every outcome reachable
                // from them like from every other live state.
                | (Pending | Active | Optional | Asleep, Napping)
                | (Pending | Active | Optional | Napping, Asleep)
                | (Napping | Asleep, Active | Succeeded | Failed | Superseded)
        )
    }

    /// Whether every terminal state is reachable from here, and `Active` is
    /// reachable or already held — so whether an objective can still be
    /// pursued, completed, failed or superseded does not depend on the order the
    /// player reached it.
    ///
    /// This is the table's order-independence as a fact a caller can ask for,
    /// and as the assertion F39-D pins it with. It is deliberately *not* a
    /// loop over [`can_become`](Self::can_become), because a property checked
    /// with the very table it constrains cannot notice a row that was deleted:
    /// the rows are enumerated here, so a transition-table edit that
    /// reintroduces an order-dependent row fails
    /// `accept_f39_d_the_state_machine_is_order_independent`.
    ///
    /// `Hidden` is `false` because only the reveal rule may leave it, and that
    /// rule is not a transition this table carries — a hidden objective is
    /// correctly not completable until it is shown.
    #[must_use]
    pub const fn is_outcome_reachable(self) -> bool {
        use ObjectiveState::{
            Active, Asleep, Failed, Hidden, Napping, Optional, Pending, Succeeded, Superseded,
        };
        match self {
            Hidden => false,
            // The M01-B-OBJ-1-FU1 states join the live set: a napped or asleep
            // objective can still be woken and can still be completed by a
            // declared move, so neither is a dead end.
            Pending | Active | Optional | Napping | Asleep => {
                self.can_become(Succeeded)
                    && self.can_become(Failed)
                    && self.can_become(Superseded)
                    // `Active` is the one target that may already be held: no
                    // row moves an objective back to itself, so `Active` is
                    // reachable-or-held and the other three are plain
                    // reachability.
                    && (matches!(self, Active) || self.can_become(Active))
            }
            Succeeded | Failed | Superseded => false,
        }
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

#[cfg(test)]
mod tests {
    use super::ObjectiveState::*;

    /// F39-D: the table must not depend on which objectives a mission completes
    /// first, so from every state a declaration can reach, every terminal
    /// outcome and `Active` is still reachable.
    ///
    /// The removed row this pins: `Pending -> Succeeded`. An objective a reveal
    /// rule had just shown (`Hidden` to `Pending`, never `Active`) could not be
    /// completed at all, so a completion whose `on_complete` requests the
    /// mission outcome could never request it and the program had no reachable
    /// ending — F39 AC04's out-of-order deadlock, produced by the table rather
    /// than by the program.
    #[test]
    fn accept_f39_d_the_state_machine_is_order_independent() {
        for state in [Pending, Active, Optional] {
            assert!(
                state.is_outcome_reachable(),
                "{state:?} can no longer reach Succeeded, Failed and Superseded"
            );
            assert!(state.can_become(Succeeded), "{state:?} cannot succeed");
            assert!(state.can_become(Failed), "{state:?} cannot fail");
            assert!(state.can_become(Superseded), "{state:?} cannot supersede");
        }
        // The row this stage added: an objective a reveal rule has just shown is
        // `Pending`, never `Active`, and must still be completable.
        assert!(Pending.can_become(Succeeded));
        assert!(Pending.can_become(Active));
        assert!(!Active.can_become(Pending), "an objective never un-pursues");

        // `Hidden` is the one deliberate exception: only the reveal rule leaves
        // it, so a hidden objective is correctly not completable yet.
        assert!(Hidden.can_become(Pending));
        assert!(Hidden.can_become(Active));
        assert!(Hidden.can_become(Optional));
        assert!(!Hidden.is_outcome_reachable());

        // Every terminal state is terminal: no row leaves one.
        for final_state in [Succeeded, Failed, Superseded] {
            assert!(final_state.is_final());
            for next in [
                Hidden, Pending, Active, Succeeded, Failed, Optional, Superseded, Napping, Asleep,
            ] {
                assert!(
                    !final_state.can_become(next),
                    "{final_state:?} must not become {next:?}"
                );
            }
        }
    }

    /// F39-E5: a completion effect that puts an objective aside needs a row into
    /// `Optional` from every state a live objective can be in, and the wakeup
    /// that takes it out again must still work from there — while every terminal
    /// outcome stays reachable, which is the property the rows could have broken.
    #[test]
    fn accept_f39_e5_a_set_aside_objective_can_still_be_completed_or_resumed() {
        // A nap lands on a shown objective (`Pending`) or a pursued one
        // (`Active`); a wakeup takes it out again. Both were unreachable before
        // this stage added the rows, so a nap could only ever have hit an
        // objective the player had never been shown.
        for live in [Pending, Active] {
            assert!(live.can_become(Optional), "{live:?} must be settable aside");
            assert!(
                Optional.can_become(Active),
                "a set-aside objective must resume"
            );
        }
        // And the order-independence the F39-D rows established still holds from
        // the states that can now reach `Optional`: a set-aside objective is not
        // a dead end.
        for state in [Pending, Active, Optional] {
            assert!(
                state.is_outcome_reachable(),
                "{state:?} can no longer reach Succeeded, Failed and Superseded"
            );
        }
        // Nothing else changed: an objective never un-pursues, and no row leaves a
        // final state, so no completion effect can ever move an objective out of
        // one — the fact `CompletionEffect`'s dead-effect refusal relies on.
        assert!(!Active.can_become(Pending));
        assert!(!Optional.can_become(Pending));
        assert!(!Optional.can_become(Optional));
    }

    /// M01-B-OBJ-1-FU1: the two engine states the measured `NAP` and `SLEEP`
    /// completion effects write carry their measured reachability — either may
    /// land on any live shown objective (the engine's writes carry no state
    /// guard), and both still reach `Active` and every terminal outcome, so
    /// neither is the order-dependent dead end a missing row makes.
    #[test]
    fn accept_m01_b_obj_1_fu1_napping_and_asleep_keep_every_outcome_reachable() {
        // The measured writes: `NAP` lands from any live shown state and from
        // `Asleep`; `SLEEP` lands from any live shown state and from `Napping`.
        for from in [Pending, Active, Optional, Asleep] {
            assert!(from.can_become(Napping), "{from:?} must be nappable");
        }
        for from in [Pending, Active, Optional, Napping] {
            assert!(from.can_become(Asleep), "{from:?} must be sleepable");
        }
        // A wake ends both (the engine's wake sets state 1 whatever the target
        // was doing), and a declared completion still lands on either — F39-D's
        // order-independence extended to the new states.
        for paused in [Napping, Asleep] {
            assert!(paused.can_become(Active), "{paused:?} must be wakeable");
            assert!(
                paused.is_outcome_reachable(),
                "{paused:?} can no longer reach Succeeded, Failed and Superseded"
            );
            for final_state in [Succeeded, Failed, Superseded] {
                assert!(
                    paused.can_become(final_state),
                    "{paused:?} cannot become {final_state:?}"
                );
            }
        }
        // What the rows do not say: `Hidden` still leaves only through its
        // reveal rule, no state returns to `Pending`, and neither new state
        // may repeat itself (a same-state declared move stays refused).
        assert!(!Hidden.can_become(Napping));
        assert!(!Hidden.can_become(Asleep));
        assert!(!Napping.can_become(Pending));
        assert!(!Asleep.can_become(Pending));
        assert!(!Napping.can_become(Napping));
        assert!(!Asleep.can_become(Asleep));
        // Terminal states stay terminal.
        for final_state in [Succeeded, Failed, Superseded] {
            assert!(!final_state.can_become(Napping));
            assert!(!final_state.can_become(Asleep));
        }
    }
}
