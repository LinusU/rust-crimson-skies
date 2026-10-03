//! The simulation-side mission session (F37-A, F37-B).
//!
//! Owns one launched mission: refuses to launch a program that fails
//! validation (an unsupported instruction means the mission is
//! [`TerminalState::Unsupported`], with no progression and no reward) and
//! drives [`MissionState`] one integer tick at a time under its
//! [`WorkLimits`]. Host effect application is F37-C.

use cs_script::ir::{MissionProgram, ValidatedProgram, ValidationError};
use cs_script::runtime::{
    MissionFacts, MissionState, SessionGeneration, TerminalState, TickError, TickResult, WorkLimits,
};
use cs_types::Tick;

/// One launched mission.
#[derive(Debug)]
pub struct MissionSession {
    program: ValidatedProgram,
    state: MissionState,
}

/// A refused launch: the mission stays Unsupported.
#[derive(Debug, PartialEq, Eq)]
pub struct LaunchRefused {
    pub terminal: TerminalState,
    pub error: ValidationError,
}

impl MissionSession {
    /// Validates and launches `program`.
    ///
    /// # Errors
    ///
    /// [`LaunchRefused`] with the precise validation trace.
    pub fn launch(
        program: MissionProgram,
        session: SessionGeneration,
    ) -> Result<Self, LaunchRefused> {
        let program = program.validate().map_err(|error| LaunchRefused {
            terminal: TerminalState::Unsupported,
            error,
        })?;
        let state = MissionState::new(&program, session);
        Ok(Self { program, state })
    }

    /// Advances one tick.
    ///
    /// # Errors
    ///
    /// [`TickError`] when the tick does not advance.
    pub fn step(&mut self, facts: &MissionFacts, tick: Tick) -> Result<TickResult, TickError> {
        self.state.step(&self.program, facts, tick)
    }

    /// Overrides the evaluator's work/queue bounds; the default is
    /// [`WorkLimits::default`].
    pub fn set_limits(&mut self, limits: WorkLimits) {
        self.state.set_limits(limits);
    }

    pub fn state(&self) -> &MissionState {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_script::ir::{Action, Condition, IR_VERSION, Objective, SymbolId};
    use cs_types::content::{ContentId, ContentKind};

    fn program(action: Action) -> MissionProgram {
        MissionProgram {
            version: IR_VERSION,
            mission: ContentId::from_source(ContentKind::Mission, "synthetic").unwrap(),
            variables: vec![],
            objectives: vec![Objective {
                id: SymbolId(1),
                content: ContentId::from_source(ContentKind::Objective, "o").unwrap(),
                condition: Condition::Const(true),
                actions: vec![action],
                span: None,
            }],
        }
    }

    #[test]
    fn accept_f37_a_session_refuses_unknown_instruction_and_runs_valid_program() {
        let bad = program(Action::Unknown {
            instruction: "op".into(),
        });
        let refused = MissionSession::launch(bad, SessionGeneration(1)).unwrap_err();
        assert_eq!(refused.terminal, TerminalState::Unsupported);

        let ok = program(Action::Finish(cs_script::ir::Outcome::Succeeded));
        let mut s = MissionSession::launch(ok, SessionGeneration(1)).unwrap();
        let r = s.step(&MissionFacts::default(), Tick(1)).unwrap();
        assert_eq!(r.terminal, TerminalState::Succeeded);
        assert_eq!(s.state().terminal(), TerminalState::Succeeded);
    }

    #[test]
    fn accept_f37_b_session_self_schedule_stops_on_budget_not_hang() {
        use cs_script::runtime::StopReason;
        // A zero-delay `Reschedule` re-queues its list forever; the session's
        // work budget is what bounds it.
        let looping = program(Action::Reschedule { delay_ticks: 0 });
        let mut s = MissionSession::launch(looping, SessionGeneration(1)).unwrap();
        s.set_limits(WorkLimits {
            max_work_per_tick: 8,
            ..WorkLimits::default()
        });
        let r = s.step(&MissionFacts::default(), Tick(1)).unwrap();
        assert!(matches!(r.stop, Some(StopReason::WorkBudget { .. })));
        assert_eq!(s.state().terminal(), TerminalState::Running);
        assert_eq!(s.state().queued_items(), 1);
    }
}
