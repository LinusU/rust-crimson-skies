//! Acceptance scenarios for F22-G: a flight context may *request* a domain
//! transaction (the pause) without being able to execute a menu action.
//!
//! Whether the original game could pause from flight, and on which key, is
//! unknown; the binding driven here is a disclosed designed default. Every
//! device identity and key is newly authored development data and no test
//! needs the original installation.

use cs_app::input::{
    DeviceEvent, FrameInput, HandoverReason, InputSession, PauseDecision, PauseReason, SessionMode,
};
use cs_sim::control::LocalSeatId;
use cs_types::Tick;
use cs_types::input::{
    Action, ActionMap, BindingSource, BindingTarget, DeviceClass, DeviceId, DomainRequest,
    InputContext, Key, UiAction,
};

/// The designed default's flight-context pause source.
const PAUSE_KEY: Key = Key::Digit9;

fn keyboard() -> DeviceId {
    DeviceId::stable(DeviceClass::Keyboard, "kbd.fixture/0").expect("the fixture identity is valid")
}

fn session() -> InputSession {
    let mut session =
        InputSession::designed_default(LocalSeatId(0), SessionMode::SinglePlayer, Tick(0));
    session
        .collector_mut()
        .connect_device(keyboard())
        .expect("the fixture keyboard connects");
    session
}

fn press(keys: &[Key]) -> Vec<DeviceEvent> {
    vec![DeviceEvent::KeyboardFrame {
        device: keyboard(),
        keys: keys.to_vec(),
    }]
}

#[test]
fn accept_f22_g_the_pause_source_in_flight_is_a_request_and_not_a_command() {
    let mut session = session();
    assert_eq!(session.context(), InputContext::Flight);

    let outcome = session
        .pump_frame(FrameInput::Devices(&press(&[PAUSE_KEY])), 1)
        .expect("the frame applies");
    assert!(outcome.is_clean(), "{:?}", outcome.faults);
    assert!(
        outcome.delivered.is_empty(),
        "a request reaches no flight consumer: {:?}",
        outcome.delivered
    );
    assert_eq!(outcome.ui_requests, 1);
    assert_eq!(session.controls().pending_edges(), 0);

    let requests = session.take_ui_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].action, UiAction::Pause);
    assert_eq!(requests[0].context, InputContext::Flight);

    // The input layer performed nothing: the pause path owns the transaction.
    assert!(!session.is_paused());
    let paused = session.pause(PauseReason::PlayerRequest);
    assert_eq!(
        paused.decision,
        PauseDecision::Paused(PauseReason::PlayerRequest)
    );
    assert_eq!(paused.released.reason, Some(HandoverReason::Paused));
    assert!(session.is_paused());
}

#[test]
fn accept_f22_g_text_entry_and_cinematics_produce_nothing_from_the_pause_source() {
    for context in [InputContext::TextEntry, InputContext::Cinematic] {
        let mut session = session();
        session.set_context(context);
        let outcome = session
            .pump_frame(FrameInput::Devices(&press(&[PAUSE_KEY])), 1)
            .expect("the frame applies");
        assert!(outcome.delivered.is_empty(), "{context}");
        assert_eq!(outcome.ui_requests, 0, "{context} queues no request");
        assert!(session.take_ui_requests().is_empty(), "{context}");
        assert!(!session.is_paused(), "{context}");
    }
}

#[test]
fn accept_f22_g_a_menu_cannot_fire_and_flight_cannot_run_a_menu_action() {
    let map = ActionMap::designed_default();
    let source = BindingSource::Key(PAUSE_KEY);
    assert_eq!(
        map.resolve(InputContext::Flight, source),
        Some(Action::Request(DomainRequest::Pause))
    );
    assert_eq!(
        map.resolve(InputContext::UiNavigation, source),
        None,
        "the request source is not a menu source"
    );
    assert_eq!(map.resolve(InputContext::TextEntry, source), None);
    assert_eq!(map.resolve(InputContext::Cinematic, source), None);

    // Flight accepts the request but still no menu action; a menu accepts no
    // request and still no flight command.
    for action in UiAction::ALL {
        assert!(!InputContext::Flight.accepts(Action::Ui(*action)));
    }
    assert!(InputContext::Flight.accepts(Action::Request(DomainRequest::Pause)));
    assert!(!InputContext::UiNavigation.accepts(Action::Request(DomainRequest::Pause)));
    assert!(!InputContext::TextEntry.accepts(Action::Request(DomainRequest::Pause)));
    assert!(!InputContext::Cinematic.accepts(Action::Request(DomainRequest::Pause)));

    assert_eq!(
        map.target_for(source),
        Some(BindingTarget::Request(DomainRequest::Pause))
    );
}
