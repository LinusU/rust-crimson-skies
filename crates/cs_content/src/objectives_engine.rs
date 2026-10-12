//! What the original engine does with an objective's completion effects and its
//! tick dependency (M01-B-OBJ-1), measured from the owner-supplied decrypted
//! executable (`CS_ENGINE_IMAGE`) and nothing else.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`.
//! Findings: `docs/findings/2026-10-12-m01-b-obj-1-completion-effects-and-tick-dependency.md`
//! holds the addresses, the measurements and what stays unknown. This module is
//! the **content half**: pure data and pure functions, no Bevy and no renderer.
//!
//! # What was measured
//!
//! * **Reading.** The mission loader reads `OBJECTIVE1`, `OBJECTIVE2`, … and
//!   stops at the first number the record does not declare, so objective number
//!   *N* is index *N − 1* of the mission's objective array. Every integer after
//!   `WAKE_OBJECTIVE_WHEN_I_COMPLETE`, `KILL_OBJECTIVE_WHEN_I_COMPLETE`,
//!   `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`, `NAP_OBJECTIVE_WHEN_I_COMPLETE` and
//!   `TICK_DEPENDS_ON_OBJ` is stored as *number − 1*; a stored `-1` ends a list.
//!   The bare key `WAKE_OBJECTIVE` is read as an alias of
//!   `WAKE_OBJECTIVE_WHEN_I_COMPLETE`.
//! * **Completion.** An objective that completes becomes completed and inactive
//!   and enters state [`EngineState::Asleep`]; then, **in this fixed order and
//!   whatever order the record spells them in**, its `WAKE` list, its `NAP`
//!   target, its `KILL` list and its `SLEEP` list are applied
//!   ([`EngineObjectives::complete`]).
//! * **Effects.** `WAKE` wakes a target ([`EngineObjectives::wake`]); `NAP`
//!   puts one target to [`EngineState::Napping`] for the declared time and
//!   clears its completed flag; `KILL` removes the target for good; `SLEEP`
//!   puts targets to [`EngineState::Asleep`].
//! * **Tick dependency.** An objective with `TICK_DEPENDS_ON_OBJ` is evaluated
//!   only while the named objective is [`EngineState::Awake`]
//!   ([`EngineObjectives::is_ticked`]).
//!
//! What is **not** here: how an objective *becomes* complete (its conditions are
//! other field families), the dormant and awake timers, and the rule that picks
//! which of several simultaneously complete objectives completes first in a pass.

use std::fmt;

/// The number of target slots the engine's list readers walk, terminator
/// included. A list longer than [`ENGINE_LIST_CAPACITY`]` - 1` has no room for
/// its terminator and overruns into the next field of the engine's own record,
/// so it is refused here rather than modelled.
pub const ENGINE_LIST_CAPACITY: usize = 15;

/// The nap time the engine substitutes when `NAP_OBJECTIVE_WHEN_I_COMPLETE`
/// names a target but no time. The engine logs "has no nap_time specified" and
/// stores this value; it is `0x3e99999a` as a 32-bit float.
pub const ENGINE_DEFAULT_NAP_TIME: f32 = f32::from_bits(0x3e99_999a);

/// The measured keys the engine reads for the effects and the dependency.
pub mod keys {
    /// Wakes the listed objectives when this one completes.
    pub const WAKE: &str = "WAKE_OBJECTIVE_WHEN_I_COMPLETE";
    /// The engine reads this bare spelling as a `WAKE` alias.
    pub const WAKE_ALIAS: &str = "WAKE_OBJECTIVE";
    /// Napping: one target and a time.
    pub const NAP: &str = "NAP_OBJECTIVE_WHEN_I_COMPLETE";
    /// Removes the listed objectives when this one completes.
    pub const KILL: &str = "KILL_OBJECTIVE_WHEN_I_COMPLETE";
    /// Sleeps the listed objectives. The engine reads it; no retail record
    /// spells it.
    pub const SLEEP: &str = "SLEEP_OBJECTIVE_WHEN_I_COMPLETE";
    /// The objective that must be awake for this one to be evaluated.
    pub const TICK_DEPENDS_ON_OBJ: &str = "TICK_DEPENDS_ON_OBJ";
}

/// The engine's four objective states, named by the value the engine stores.
///
/// The names for 2 and 3 follow the `NAP` and `SLEEP` spellings of the effects
/// that set them; completion also sets [`Asleep`](Self::Asleep).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EngineState {
    /// State 0: set by `BEGIN_DORMANT`, left by a wake.
    Dormant,
    /// State 1: the state in which an objective is evaluated and counts as
    /// awake for the tick dependency.
    Awake,
    /// State 2: set by `NAP`, left when the nap time has elapsed.
    Napping,
    /// State 3: set when the objective completes and by `SLEEP`.
    Asleep,
}

/// One objective's list of targets, as the engine stores it.
///
/// Targets are indices (*number − 1*). The list ends at the first stored `-1`,
/// i.e. at the first record integer `0`, which the engine treats as the
/// terminator rather than a target.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetList(Vec<usize>);

impl TargetList {
    /// Reads the integers of a record value the way the engine does.
    ///
    /// # Errors
    ///
    /// [`EngineEffectError::ListTooLong`] when the list leaves no room for the
    /// engine's terminator.
    pub fn from_record(numbers: &[u32]) -> Result<Self, EngineEffectError> {
        if numbers.len() >= ENGINE_LIST_CAPACITY {
            return Err(EngineEffectError::ListTooLong {
                len: numbers.len(),
                capacity: ENGINE_LIST_CAPACITY - 1,
            });
        }
        Ok(Self(
            numbers
                .iter()
                .map_while(|number| number.checked_sub(1))
                .map(|index| index as usize)
                .collect(),
        ))
    }

    /// The target indices, in the order the record spells them.
    #[must_use]
    pub fn targets(&self) -> &[usize] {
        &self.0
    }
}

/// The `NAP` declaration: at most one target and its time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NapDeclaration {
    /// The target's index, or `None` when the stored target is `-1` (the engine
    /// then does nothing).
    pub target: Option<usize>,
    /// The nap time, in the unit of the engine's per-pass time step.
    pub time: f32,
}

impl NapDeclaration {
    /// Reads the integer target and the optional time of a `NAP` record value.
    ///
    /// `time` is the second value of the key, already converted to a float (the
    /// engine accepts an integer there and converts it); `None` means the
    /// record gave none, and the engine then logs an error and uses
    /// [`ENGINE_DEFAULT_NAP_TIME`].
    #[must_use]
    pub fn from_record(target: u32, time: Option<f32>) -> Self {
        Self {
            target: target.checked_sub(1).map(|index| index as usize),
            time: time.unwrap_or(ENGINE_DEFAULT_NAP_TIME),
        }
    }
}

/// What one objective declares for its completion and its evaluation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineDeclaration {
    /// `WAKE_OBJECTIVE_WHEN_I_COMPLETE` (or its bare alias).
    pub wake: TargetList,
    /// `NAP_OBJECTIVE_WHEN_I_COMPLETE`.
    pub nap: Option<NapDeclaration>,
    /// `KILL_OBJECTIVE_WHEN_I_COMPLETE`.
    pub kill: TargetList,
    /// `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`.
    pub sleep: TargetList,
    /// `TICK_DEPENDS_ON_OBJ`: the index of the objective that must be awake.
    pub tick_depends_on: Option<usize>,
}

/// Why a record value cannot be read into an engine declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEffectError {
    /// A list has no room for the engine's terminator.
    ListTooLong {
        /// How many integers the record spells.
        len: usize,
        /// How many the engine's list can hold.
        capacity: usize,
    },
    /// A target names an objective the record does not declare.
    TargetOutOfRange {
        /// The declaring objective's index.
        from: usize,
        /// The target index.
        target: usize,
        /// How many objectives the record declares.
        count: usize,
    },
}

impl fmt::Display for EngineEffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ListTooLong { len, capacity } => write!(
                formatter,
                "a target list of {len} integers overruns the engine's {capacity}-target list"
            ),
            Self::TargetOutOfRange {
                from,
                target,
                count,
            } => write!(
                formatter,
                "objective index {from} names objective index {target}, but only {count} are declared"
            ),
        }
    }
}

impl std::error::Error for EngineEffectError {}

/// One objective's engine flags and timers.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineObjective {
    /// Cleared by `KILL`; a wake skips an objective that is not present.
    pub present: bool,
    /// Cleared on completion and by every state change.
    pub active: bool,
    /// Set on completion; cleared again by `NAP`.
    pub completed: bool,
    /// The engine state.
    pub state: EngineState,
    /// Time accumulated since the last state change.
    pub elapsed: f32,
    /// The nap time written by `NAP`; negative means none is pending.
    pub nap_time: f32,
    /// What this objective declares.
    pub declaration: EngineDeclaration,
}

impl EngineObjective {
    /// An objective in `state` with nothing pending.
    #[must_use]
    pub fn new(state: EngineState, declaration: EngineDeclaration) -> Self {
        Self {
            present: true,
            active: state == EngineState::Awake,
            completed: false,
            state,
            elapsed: 0.0,
            nap_time: -1.0,
            declaration,
        }
    }
}

/// All of one mission's objectives, in objective-number order.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineObjectives {
    objectives: Vec<EngineObjective>,
}

impl EngineObjectives {
    /// Builds the set, refusing a declaration whose target is out of range.
    ///
    /// # Errors
    ///
    /// [`EngineEffectError::TargetOutOfRange`] for the first such target.
    pub fn new(objectives: Vec<EngineObjective>) -> Result<Self, EngineEffectError> {
        let count = objectives.len();
        for (from, objective) in objectives.iter().enumerate() {
            let declaration = &objective.declaration;
            let nap = declaration.nap.and_then(|nap| nap.target);
            let named = declaration
                .wake
                .targets()
                .iter()
                .chain(declaration.kill.targets())
                .chain(declaration.sleep.targets())
                .copied()
                .chain(nap)
                .chain(declaration.tick_depends_on);
            for target in named {
                if target >= count {
                    return Err(EngineEffectError::TargetOutOfRange {
                        from,
                        target,
                        count,
                    });
                }
            }
        }
        Ok(Self { objectives })
    }

    /// The objective at `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&EngineObjective> {
        self.objectives.get(index)
    }

    /// Whether `index` is evaluated this pass as far as the tick dependency
    /// goes: it has none, or the objective it names is awake.
    #[must_use]
    pub fn is_ticked(&self, index: usize) -> bool {
        let Some(objective) = self.objectives.get(index) else {
            return false;
        };
        objective
            .declaration
            .tick_depends_on
            .is_none_or(|dependency| self.objectives[dependency].state == EngineState::Awake)
    }

    /// Whether the completion conditions of `index` are evaluated: the tick
    /// dependency holds, it is active and it has not completed.
    #[must_use]
    pub fn evaluates_completion(&self, index: usize) -> bool {
        self.is_ticked(index) && self.objectives[index].active && !self.objectives[index].completed
    }

    /// Wakes each target of `targets` in order, as the engine's wake does.
    ///
    /// A target that is not present or has completed is skipped. A target that
    /// is **already awake ends the whole list** (its elapsed time is reset
    /// first): later targets are not woken, and that awake target is what the
    /// engine returns. Otherwise it returns the last objective woken.
    pub fn wake(&mut self, targets: &[usize]) -> Option<usize> {
        let mut last = None;
        for &target in targets {
            let objective = &mut self.objectives[target];
            if !objective.present || objective.completed {
                continue;
            }
            if objective.state == EngineState::Awake {
                objective.elapsed = 0.0;
                return Some(target);
            }
            last = Some(target);
            objective.active = true;
            objective.state = EngineState::Awake;
            objective.elapsed = 0.0;
        }
        last
    }

    /// Sets `target` to `state` the way the engine's state setter does: inactive,
    /// elapsed time zero.
    fn set_state(&mut self, target: usize, state: EngineState) {
        let objective = &mut self.objectives[target];
        objective.active = false;
        objective.state = state;
        objective.elapsed = 0.0;
    }

    /// Completes `index` and applies its effects in the engine's order: the
    /// objective becomes completed, inactive and asleep; then `WAKE`, `NAP`,
    /// `KILL`, `SLEEP`.
    ///
    /// The order is the engine's, not the record's, so two effects on one
    /// objective always resolve the same way: `WAKE` then `NAP` leaves the
    /// target napping and inactive.
    pub fn complete(&mut self, index: usize) {
        let declaration = self.objectives[index].declaration.clone();
        {
            let objective = &mut self.objectives[index];
            objective.completed = true;
            objective.active = false;
            objective.state = EngineState::Asleep;
            objective.elapsed = 0.0;
        }
        self.wake(declaration.wake.targets());
        if let Some(NapDeclaration {
            target: Some(target),
            time,
        }) = declaration.nap
        {
            self.set_state(target, EngineState::Napping);
            self.objectives[target].nap_time = time;
            self.objectives[target].completed = false;
        }
        for &target in declaration.kill.targets() {
            let objective = &mut self.objectives[target];
            objective.present = false;
            objective.active = false;
        }
        for &target in declaration.sleep.targets() {
            self.set_state(target, EngineState::Asleep);
        }
    }

    /// Advances every napping objective by one pass of `step`.
    ///
    /// A napping objective with a pending nap time accumulates `step`; once the
    /// total reaches the nap time it is woken ([`wake`](Self::wake) on itself).
    /// An objective that is not present is not advanced. The dormant and awake
    /// timers are not modelled.
    pub fn advance_naps(&mut self, step: f32) {
        for index in 0..self.objectives.len() {
            if !self.objectives[index].present || !self.is_ticked(index) {
                continue;
            }
            let objective = &mut self.objectives[index];
            if objective.state != EngineState::Napping || objective.nap_time < 0.0 {
                continue;
            }
            objective.elapsed += step;
            if objective.elapsed >= objective.nap_time {
                self.wake(&[index]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(declaration: EngineDeclaration, state: EngineState) -> EngineObjective {
        EngineObjective::new(state, declaration)
    }

    fn list(numbers: &[u32]) -> TargetList {
        TargetList::from_record(numbers).expect("list fits")
    }

    #[test]
    fn a_zero_ends_a_list_and_numbers_are_one_based() {
        assert_eq!(list(&[3, 1, 0, 5]).targets(), &[2, 0]);
        assert!(list(&[]).targets().is_empty());
        assert!(TargetList::from_record(&[1; ENGINE_LIST_CAPACITY]).is_err());
        assert!(TargetList::from_record(&[1; ENGINE_LIST_CAPACITY - 1]).is_ok());
    }

    #[test]
    fn wake_then_nap_on_one_target_leaves_it_napping() {
        let completing = EngineDeclaration {
            wake: list(&[2]),
            nap: Some(NapDeclaration::from_record(2, Some(2.0))),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            declared(completing, EngineState::Awake),
            declared(EngineDeclaration::default(), EngineState::Dormant),
        ])
        .expect("in range");
        set.complete(0);
        let target = set.get(1).expect("target");
        assert_eq!(target.state, EngineState::Napping);
        assert!(!target.active);
        assert!((target.nap_time - 2.0).abs() < f32::EPSILON);
        let source = set.get(0).expect("source");
        assert!(source.completed && !source.active);
        assert_eq!(source.state, EngineState::Asleep);
    }

    #[test]
    fn nap_expiry_wakes_the_target() {
        let completing = EngineDeclaration {
            nap: Some(NapDeclaration::from_record(2, Some(1.0))),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            declared(completing, EngineState::Awake),
            declared(EngineDeclaration::default(), EngineState::Awake),
        ])
        .expect("in range");
        set.complete(0);
        set.advance_naps(0.5);
        assert_eq!(set.get(1).expect("target").state, EngineState::Napping);
        set.advance_naps(0.5);
        let target = set.get(1).expect("target");
        assert_eq!(target.state, EngineState::Awake);
        assert!(target.active);
    }

    #[test]
    fn nap_clears_the_completed_flag_and_kill_blocks_a_later_wake() {
        let completing = EngineDeclaration {
            nap: Some(NapDeclaration::from_record(2, Some(1.0))),
            kill: list(&[3]),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            declared(completing, EngineState::Awake),
            declared(EngineDeclaration::default(), EngineState::Asleep),
            declared(EngineDeclaration::default(), EngineState::Dormant),
        ])
        .expect("in range");
        set.objectives[1].completed = true;
        set.complete(0);
        assert!(!set.get(1).expect("napper").completed);
        assert!(!set.get(2).expect("killed").present);
        assert_eq!(set.wake(&[2]), None);
        assert_eq!(set.get(2).expect("killed").state, EngineState::Dormant);
    }

    #[test]
    fn an_awake_target_ends_the_wake_list() {
        let mut set = EngineObjectives::new(vec![
            declared(EngineDeclaration::default(), EngineState::Dormant),
            declared(EngineDeclaration::default(), EngineState::Awake),
            declared(EngineDeclaration::default(), EngineState::Dormant),
        ])
        .expect("in range");
        assert_eq!(set.wake(&[0, 1, 2]), Some(1));
        assert_eq!(set.get(0).expect("first").state, EngineState::Awake);
        assert_eq!(set.get(2).expect("third").state, EngineState::Dormant);
    }

    #[test]
    fn the_tick_dependency_needs_the_named_objective_awake() {
        let dependent = EngineDeclaration {
            tick_depends_on: Some(0),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            declared(EngineDeclaration::default(), EngineState::Dormant),
            declared(dependent, EngineState::Awake),
        ])
        .expect("in range");
        assert!(!set.is_ticked(1));
        assert!(!set.evaluates_completion(1));
        set.wake(&[0]);
        assert!(set.is_ticked(1));
        assert!(set.evaluates_completion(1));
        set.complete(0);
        assert!(!set.is_ticked(1), "a completed dependency is asleep");
    }

    #[test]
    fn a_target_out_of_range_is_refused() {
        let bad = EngineDeclaration {
            kill: list(&[5]),
            ..EngineDeclaration::default()
        };
        let error = EngineObjectives::new(vec![declared(bad, EngineState::Awake)])
            .expect_err("index 4 of 1");
        assert_eq!(
            error,
            EngineEffectError::TargetOutOfRange {
                from: 0,
                target: 4,
                count: 1
            }
        );
    }
}
