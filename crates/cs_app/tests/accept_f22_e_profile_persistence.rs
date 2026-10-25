//! Acceptance scenario F22-E: bindings and calibration persist atomically,
//! and an index-only device is refused.
//!
//! The tests drive `cs_app::input::InputProfile` through real files in a
//! per-test temporary directory. Every device identity, binding and
//! calibration value is newly authored development data; no test needs the
//! original installation. The format is a new-engine design, not the
//! original's (which is unknown).

use std::fs;
use std::path::PathBuf;

use cs_app::input::profile::{PROFILE_FORMAT, temporary_path};
use cs_app::input::{InputCollector, InputProfile, ProfileError, ProfileField};
use cs_types::Tick;
use cs_types::input::{
    ActionMap, AxisCalibration, AxisChannel, Binding, BindingSource, BindingTarget, DeviceClass,
    DeviceId, FlightCommand, GamepadAxis, ResponseCurve, UiAction,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cs_f22_e_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self) -> PathBuf {
        self.0.join("input.profile")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stick() -> DeviceId {
    DeviceId::stable(DeviceClass::Joystick, "usb:045e/001b serial 7%x").unwrap()
}

fn calibrated() -> InputCollector {
    let mut collector = InputCollector::designed_default(Tick(0));
    let store = collector.devices_mut().calibration_mut();
    store.set(
        &stick(),
        AxisChannel::Joystick(1),
        AxisCalibration::try_new(0.125, true, ResponseCurve::Power(1.7), 0.85, 0.4).unwrap(),
    );
    store.set(
        &DeviceId::stable(DeviceClass::Gamepad, "pad-0").unwrap(),
        AxisChannel::Gamepad(GamepadAxis::LeftTrigger),
        AxisCalibration::designed_default(),
    );
    collector
}

fn parse(text: &str) -> Result<InputProfile, ProfileError> {
    InputProfile::parse(text.as_bytes())
}

#[test]
fn accept_f22_e_saved_profile_reloads_the_same_map_and_calibration() {
    let dir = TempDir::new("roundtrip");
    let collector = calibrated();
    InputProfile::capture(&collector).save(&dir.file()).unwrap();

    let loaded = InputProfile::load(&dir.file()).unwrap();
    assert_eq!(loaded.map(), collector.bindings().map());
    assert_eq!(
        loaded.calibration(),
        collector.devices().calibration(),
        "calibration must reload exactly, floats included"
    );
    // The text is stable: saving the reloaded profile reproduces the bytes.
    assert_eq!(
        loaded.to_text().unwrap(),
        fs::read_to_string(dir.file()).unwrap()
    );
    assert!(!temporary_path(&dir.file()).exists());

    // Applying it to a fresh session reproduces the saved session.
    let mut fresh = InputCollector::new(ActionMap::empty(), Tick(0));
    loaded.apply(&mut fresh);
    assert_eq!(fresh.bindings().map(), collector.bindings().map());
    assert_eq!(
        fresh.devices().calibration(),
        collector.devices().calibration()
    );
}

#[test]
fn accept_f22_e_index_only_device_is_refused_and_adoption_rekeys_it() {
    let dir = TempDir::new("unstable");
    let mut collector = calibrated();
    let provisional = DeviceId::enumeration_fallback(DeviceClass::Joystick, 2);
    collector
        .devices_mut()
        .connect(provisional.clone())
        .unwrap();
    collector.devices_mut().calibration_mut().set(
        &provisional,
        AxisChannel::Joystick(0),
        AxisCalibration::try_new(0.2, false, ResponseCurve::Linear, 1.0, 0.5).unwrap(),
    );

    let before = InputProfile::capture(&calibrated());
    before.save(&dir.file()).unwrap();
    let bytes = fs::read(dir.file()).unwrap();

    let error = InputProfile::capture(&collector)
        .save(&dir.file())
        .unwrap_err();
    assert!(
        matches!(&error, ProfileError::UnstableDevice { device } if *device == provisional),
        "{error}"
    );
    assert_eq!(
        fs::read(dir.file()).unwrap(),
        bytes,
        "the old profile stays"
    );

    // Once the platform reports an identity, the re-keyed record persists.
    let adopted = DeviceId::stable(DeviceClass::Joystick, "serial-2").unwrap();
    collector
        .devices_mut()
        .adopt_identity(&provisional, adopted.clone())
        .unwrap();
    InputProfile::capture(&collector).save(&dir.file()).unwrap();
    let text = fs::read_to_string(dir.file()).unwrap();
    assert!(
        text.contains("calibration joystick serial-2 joystick.0 "),
        "{text}"
    );
    assert!(!text.contains("index:"), "{text}");
}

#[test]
fn accept_f22_e_failed_write_leaves_the_previous_profile_intact() {
    let dir = TempDir::new("failed_write");
    InputProfile::capture(&calibrated())
        .save(&dir.file())
        .unwrap();
    let previous = fs::read(dir.file()).unwrap();

    // A directory squatting on the temporary path makes the write fail
    // before the rename.
    fs::create_dir(temporary_path(&dir.file())).unwrap();
    let mut changed = InputCollector::designed_default(Tick(0));
    changed.bindings_mut().set_map(ActionMap::empty());
    let error = InputProfile::capture(&changed)
        .save(&dir.file())
        .unwrap_err();
    assert!(matches!(error, ProfileError::Io { .. }), "{error}");
    assert_eq!(fs::read(dir.file()).unwrap(), previous);

    // A crash mid-write leaves a half temporary file; the profile still loads
    // and the next save replaces the stale temporary.
    fs::remove_dir(temporary_path(&dir.file())).unwrap();
    fs::write(temporary_path(&dir.file()), &previous[..previous.len() / 2]).unwrap();
    assert!(InputProfile::load(&dir.file()).is_ok());
    InputProfile::capture(&changed).save(&dir.file()).unwrap();
    assert!(
        InputProfile::load(&dir.file())
            .unwrap()
            .map()
            .bindings()
            .is_empty()
    );
    assert!(!temporary_path(&dir.file()).exists());
}

#[test]
fn accept_f22_e_load_failure_preserves_the_running_session() {
    let dir = TempDir::new("load_failure");
    let mut session = calibrated();
    let map = session.bindings().map().clone();
    let store = session.devices().calibration().clone();

    let good = InputProfile::capture(&session).to_text().unwrap();
    // Truncated mid-file, as a crash on a non-atomic writer would leave it.
    fs::write(dir.file(), &good.as_bytes()[..good.len() - 4]).unwrap();
    let error = InputProfile::load(&dir.file()).unwrap_err();
    assert!(matches!(error, ProfileError::Truncated), "{error}");
    // Only a successful load can reach `apply`, so nothing changed.
    assert_eq!(session.bindings().map(), &map);
    assert_eq!(session.devices().calibration(), &store);

    let missing = InputProfile::load(&dir.0.join("absent")).unwrap_err();
    assert!(matches!(missing, ProfileError::Io { .. }));

    // Applying a good profile replaces stable records, keeps provisional ones.
    let provisional = DeviceId::enumeration_fallback(DeviceClass::Mouse, 0);
    session.devices_mut().calibration_mut().set(
        &provisional,
        AxisChannel::Joystick(0),
        AxisCalibration::designed_default(),
    );
    InputProfile::new(ActionMap::empty(), Default::default()).apply(&mut session);
    assert!(session.bindings().map().bindings().is_empty());
    assert_eq!(session.devices().calibration().len(), 1);
    assert_eq!(session.devices().calibration().unstable_devices().len(), 1);
}

#[test]
fn accept_f22_e_conflicting_bindings_are_reported_not_resolved() {
    let text = format!(
        "{PROFILE_FORMAT}\nbind joystick_button.0 command:fire_primary\nbind joystick_button.0 command:eject\nend\n"
    );
    // Sanity: the same source in two different contexts is legitimate.
    let ok = ActionMap::try_new(vec![
        Binding {
            source: BindingSource::JoystickButton(0),
            target: BindingTarget::Command(FlightCommand::FirePrimary),
        },
        Binding {
            source: BindingSource::JoystickButton(0),
            target: BindingTarget::Ui(UiAction::Confirm),
        },
    ])
    .unwrap();
    assert_eq!(
        parse(
            &InputProfile::new(ok.clone(), Default::default())
                .to_text()
                .unwrap()
        )
        .unwrap()
        .map(),
        &ok
    );
    let error = parse(&text).unwrap_err();
    assert!(matches!(error, ProfileError::Bindings { .. }), "{error}");
}

#[test]
fn accept_f22_e_malformed_fields_are_refused_by_name() {
    let head = format!("{PROFILE_FORMAT}\n");
    let cal = "calibration joystick serial-1 joystick.1 deadzone=0.1 inverted=0 response=linear saturation=1 activation=0.5";
    let case = |line: &str| parse(&format!("{head}{line}\nend\n")).unwrap_err();

    assert!(matches!(parse(""), Err(ProfileError::Truncated)));
    assert!(matches!(
        parse("nope\nend\n"),
        Err(ProfileError::MissingHeader)
    ));
    assert!(matches!(
        parse("cs-input-profile 2\nend\n"),
        Err(ProfileError::UnsupportedVersion { .. })
    ));
    assert!(matches!(
        parse(&format!("{head}end\nbind\n")),
        Err(ProfileError::TrailingContent { line: 3 })
    ));
    assert!(matches!(
        case("frobnicate"),
        ProfileError::Malformed {
            line: 2,
            field: ProfileField::Record,
            ..
        }
    ));
    assert!(matches!(
        case("bind key.nonsense command:eject"),
        ProfileError::Malformed {
            field: ProfileField::Source,
            ..
        }
    ));
    assert!(matches!(
        case("bind joystick_button.007 command:eject"),
        ProfileError::Malformed {
            field: ProfileField::Source,
            ..
        }
    ));
    assert!(matches!(
        case("bind joystick_button.1 command:nonsense"),
        ProfileError::Malformed {
            field: ProfileField::Target,
            ..
        }
    ));
    assert!(matches!(
        case("bind joystick_button.1"),
        ProfileError::Malformed {
            field: ProfileField::Arity,
            ..
        }
    ));
    assert!(matches!(
        case("bind joystick_axis.1 axis:pitch:0"),
        ProfileError::Bindings { .. }
    ));
    assert!(parse(&format!("{head}{cal}\nend\n")).is_ok());
    let bad = |from: &str, to: &str| case(&cal.replace(from, to));
    assert!(matches!(
        bad("joystick serial", "plane serial"),
        ProfileError::Malformed {
            field: ProfileField::DeviceClass,
            ..
        }
    ));
    assert!(matches!(
        bad("serial-1", "bad%zz"),
        ProfileError::Malformed {
            field: ProfileField::DeviceIdentity,
            ..
        }
    ));
    assert!(matches!(
        bad("serial-1", "bad%0Aid"),
        ProfileError::DeviceId { .. }
    ));
    assert!(matches!(
        bad("joystick.1", "joystick.x"),
        ProfileError::Malformed {
            field: ProfileField::Channel,
            ..
        }
    ));
    assert!(matches!(
        bad("joystick.1", "mouse.x"),
        ProfileError::ChannelClassMismatch { line: 2, .. }
    ));
    assert!(matches!(
        bad("deadzone=0.1", "deadzone=abc"),
        ProfileError::Malformed {
            field: ProfileField::Calibration("deadzone"),
            ..
        }
    ));
    assert!(matches!(
        bad("deadzone=0.1", "deadzone=1.5"),
        ProfileError::Calibration { .. }
    ));
    assert!(matches!(
        bad("inverted=0", "inverted=2"),
        ProfileError::Malformed {
            field: ProfileField::Calibration("inverted"),
            ..
        }
    ));
    assert!(matches!(
        bad("response=linear", "response=power:9"),
        ProfileError::Calibration { .. }
    ));
    assert!(matches!(
        bad("response=linear", "response=cubic"),
        ProfileError::Malformed {
            field: ProfileField::Calibration("response"),
            ..
        }
    ));
    assert!(matches!(
        bad("saturation=1", "saturation=NaN"),
        ProfileError::Calibration { .. }
    ));
    assert!(matches!(
        parse(&format!("{head}{cal}\n{cal}\nend\n")),
        Err(ProfileError::DuplicateCalibration { line: 3, .. })
    ));
    assert!(matches!(
        parse(&format!("{head}{}\nend\n", "x".repeat(2000))),
        Err(ProfileError::LineTooLong { line: 2, .. })
    ));
    assert!(matches!(
        InputProfile::parse(&vec![b'a'; (1 << 20) + 1]),
        Err(ProfileError::TooLarge { .. })
    ));
    assert!(matches!(
        InputProfile::parse(&[0xff, 0xfe]),
        Err(ProfileError::NotText)
    ));
}
