use cs_app::accessibility::navigation::{NavDevice, navigation_gaps, navigation_map};
use cs_app::accessibility::remap::{RemapError, RemapSession};
use cs_types::input::{
    Action, ActionMap, ActionMapError, BindingSource, BindingTarget, DeviceClass, GamepadButton,
    InputContext, Key, UiAction,
};

const CONFIRM: BindingTarget = BindingTarget::Ui(UiAction::Confirm);

fn session() -> RemapSession {
    RemapSession::new(navigation_map(), navigation_map())
}

#[test]
fn accept_f52_a_rebind_replaces_on_the_device_and_commits() {
    let mut session = session();
    session
        .rebind(CONFIRM, BindingSource::Key(Key::C))
        .expect("rebind");
    let map = session.commit().expect("commit").clone();
    let ui = |source| map.resolve(InputContext::UiNavigation, source);
    let confirm = Some(Action::Ui(UiAction::Confirm));
    assert_eq!(ui(BindingSource::Key(Key::C)), confirm);
    // The old keyboard binding is gone; the controller's is untouched.
    assert_eq!(ui(BindingSource::Key(Key::Enter)), None);
    assert_eq!(
        ui(BindingSource::GamepadButton(GamepadButton::South)),
        confirm
    );
}

#[test]
fn accept_f52_a_a_conflicting_rebind_is_refused_and_changes_nothing() {
    let mut session = session();
    let before = session.working().clone();
    // `ArrowUp` already navigates up in the UI context.
    let error = session
        .rebind(CONFIRM, BindingSource::Key(Key::ArrowUp))
        .expect_err("conflict");
    assert!(matches!(
        error,
        RemapError::Map(ActionMapError::ConflictingBinding { .. })
    ));
    assert_eq!(session.working(), &before);
}

#[test]
fn accept_f52_a_commit_refuses_a_map_that_strands_a_device_and_cancel_recovers() {
    let mut session = session();
    session.unbind(CONFIRM, DeviceClass::Gamepad);
    match session.commit() {
        Err(RemapError::Strands { device, missing }) => {
            assert_eq!(device, NavDevice::Controller);
            assert_eq!(missing, vec![UiAction::Confirm]);
        }
        other => panic!("expected stranding, got {other:?}"),
    }
    session.cancel();
    assert_eq!(session.working(), &navigation_map());
    assert!(session.commit().is_ok());
}

#[test]
fn accept_f52_a_reset_restores_the_designed_map_from_a_broken_edit() {
    let mut session = RemapSession::new(ActionMap::designed_default(), navigation_map());
    // The committed map is itself stranding for the controller.
    assert!(!navigation_gaps(session.working(), NavDevice::Controller).is_empty());
    assert!(matches!(session.commit(), Err(RemapError::Strands { .. })));
    session.reset();
    assert_eq!(session.working(), &navigation_map());
    assert!(session.commit().is_ok());
}
