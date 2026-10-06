//! Acceptance scenario F39 follow-up #672: a declared cue -> mission-signal
//! table is checked against the lowered program, so a cue cannot alias a
//! symbol the program already owns.
//!
//! Task test prefix: `accept_f39_`. These tests drive production code only:
//! [`MissionMarkerBindings::checked_against`] over the lowered
//! [`declared_synthetic_objectives`] fixture, and a real [`ObjectiveSession`]
//! step for the aliasing consequence. Every value is synthetic fixture data.

use cs_app::mission_markers::{
    DeclaredSymbolOwner, MarkerBindingError, MissionMarkerBinding, MissionMarkerBindings,
};
use cs_app::objectives::{LoweredObjectives, ObjectiveSession, lower_program};
use cs_content::objectives::{
    SYNTHETIC_APPROACH, SYNTHETIC_PRIMARY, SYNTHETIC_PROTECTED_LOST, SYNTHETIC_RADIO,
    SYNTHETIC_RAIDERS, SYNTHETIC_REACHED_WRECK, SYNTHETIC_SECONDARY, SYNTHETIC_WAVE_TIMERS,
    declared_synthetic_objectives,
};
use cs_script::ir::SymbolId;
use cs_script::runtime::SessionGeneration;
use cs_sim::objectives::runtime::{ObjectiveEventKind, TickInput};
use cs_sim::objectives::timer::TimerRequest;
use cs_types::Tick;

fn lowered() -> LoweredObjectives {
    lower_program(&declared_synthetic_objectives()).expect("the fixture lowers")
}

fn table(signal: SymbolId) -> MissionMarkerBindings {
    MissionMarkerBindings::new([MissionMarkerBinding::new("synthetic.door", signal)])
        .expect("a live, non-reserved signal")
}

#[test]
fn accept_f39_d_a_cue_bound_to_a_declared_symbol_is_refused_by_name() {
    let program = lowered();
    let cases = [
        (SYNTHETIC_PRIMARY.0, DeclaredSymbolOwner::Objective),
        (SYNTHETIC_SECONDARY.0, DeclaredSymbolOwner::Objective),
        (
            SYNTHETIC_PROTECTED_LOST.0,
            DeclaredSymbolOwner::CountCondition,
        ),
        (SYNTHETIC_WAVE_TIMERS[0].0, DeclaredSymbolOwner::Timer),
        (SYNTHETIC_RADIO.0, DeclaredSymbolOwner::Timer),
        (SYNTHETIC_APPROACH.0, DeclaredSymbolOwner::Trigger),
        (SYNTHETIC_RAIDERS.0, DeclaredSymbolOwner::SpawnGroup),
    ];
    for (raw, owner) in cases {
        let signal = SymbolId(raw);
        let refused = table(signal)
            .checked_against(&program)
            .expect_err("a declared symbol must be refused");
        assert_eq!(
            refused,
            MarkerBindingError::DeclaredSymbol {
                cue: "synthetic.door".to_owned(),
                signal,
                owner,
            }
        );
        let message = refused.to_string();
        assert!(message.contains("synthetic.door") && message.contains(&format!("{signal:?}")));
    }
}

#[test]
fn accept_f39_d_a_cue_bound_to_a_declared_signal_is_still_accepted() {
    let program = lowered();
    let bindings = table(SymbolId(SYNTHETIC_REACHED_WRECK.0))
        .checked_against(&program)
        .expect("the program's reveal signal is not a declaration's own symbol");
    assert_eq!(
        bindings.signal_for("synthetic.door"),
        Some(SymbolId(SYNTHETIC_REACHED_WRECK.0))
    );
}

/// Why the refusal matters: the runtime's `Emitter::push` keys an event by
/// `(session, tick, source, sequence)`, and a host-injected signal's source is
/// the signal itself (`collect_signals`), while a timer keys its own events
/// under its own id (`out.push(timer, ...)`). A signal raised under a declared
/// timer's symbol therefore shares `(session, tick, source)` with that timer's
/// own events and differs only by sequence.
#[test]
fn accept_f39_d_a_signal_under_a_declared_symbol_aliases_its_events() {
    let radio = SymbolId(SYNTHETIC_RADIO.0);
    let mut session = ObjectiveSession::launch(lowered(), SessionGeneration(1)).expect("launch");
    let arm = [TimerRequest::Arm(radio)];
    let signals = [radio];
    let mut facts = TickInput::at(Tick(1));
    facts.committed_ticks = 1;
    facts.timer_requests = &arm;
    facts.signals = &signals;
    let stepped = session.step(&facts).expect("a legal tick");

    let raised = stepped
        .tick
        .events
        .iter()
        .find(|e| matches!(e.kind, ObjectiveEventKind::SignalRaised { signal } if signal == radio))
        .expect("the host-injected signal is raised");
    let timer_own = stepped
        .tick
        .events
        .iter()
        .find(|e| matches!(e.kind, ObjectiveEventKind::TimerArmed { timer, .. } if timer == radio))
        .expect("the timer's own event");
    assert_eq!(raised.key.session, timer_own.key.session);
    assert_eq!(raised.key.tick, timer_own.key.tick);
    assert_eq!(
        raised.key.source, timer_own.key.source,
        "the signal and the timer's own event are keyed under one source"
    );
    assert_ne!(raised.key.sequence, timer_own.key.sequence);

    // The same table is what the new check refuses.
    assert!(matches!(
        table(radio).checked_against(&lowered()),
        Err(MarkerBindingError::DeclaredSymbol { .. })
    ));
}
