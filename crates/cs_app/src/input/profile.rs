//! Persistent input profile: bindings and calibration on disk (F22-E).
//!
//! Spec: `specs/F22-input-bindings-devices-and-control-ownership.md`,
//! non-negotiable behavior 5 ("Bindings persist atomically and conflicts are
//! visible") and behavior 1 (a device is never persisted by an enumeration
//! index alone). Finding:
//! `docs/findings/2026-10-25-f22-e-profile-persistence.md`.
//!
//! An [`InputProfile`] is the action map plus the calibration store of one
//! session. It serializes to a versioned, line-oriented text format
//! ([`PROFILE_FORMAT`]) with bounded field lengths, loads back refusing every
//! malformed field by name ([`ProfileError`]), and is written atomically
//! ([`InputProfile::save`]): the text goes to a temporary file beside the
//! target, is synced, and is renamed over the target, so a crash mid-write
//! leaves the previous profile or no profile, never half of one.
//!
//! **Designed, not original.** The format and the file's location are newly
//! authored engine decisions. The original 2000 PC game's configuration
//! format and location are unknown and this module claims no parity with
//! them.
//!
//! Loading never touches a running session: [`InputProfile::load`] and
//! [`InputProfile::parse`] return a value or an error, and only
//! [`InputProfile::apply`] changes the session, so a profile that fails to
//! load leaves the session's bindings exactly as they were.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use cs_types::input::{
    ActionMap, ActionMapError, AxisCalibration, AxisChannel, Binding, BindingSource, BindingTarget,
    CalibrationError, CalibrationStore, DeviceClass, DeviceId, DeviceIdError, FlightCommand,
    GamepadAxis, GamepadButton, Key, MouseAxis, MouseButton, ResponseCurve, UiAction,
};

use super::InputCollector;

/// The first line of a profile file: the format name and its version.
pub const PROFILE_FORMAT: &str = "cs-input-profile 1";

/// The most bytes a profile file may hold.
pub const MAX_PROFILE_BYTES: usize = 1 << 20;

/// The most bytes one profile line may hold.
pub const MAX_LINE_BYTES: usize = 1024;

/// The most bindings a profile may hold.
pub const MAX_BINDINGS: usize = 4096;

/// The most calibration records a profile may hold.
pub const MAX_CALIBRATIONS: usize = 1024;

/// Which part of a record was malformed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileField {
    /// The record keyword.
    Record,
    /// A binding's source.
    Source,
    /// A binding's target.
    Target,
    /// A calibration's device class.
    DeviceClass,
    /// A calibration's device identity.
    DeviceIdentity,
    /// A calibration's axis channel.
    Channel,
    /// A named calibration field (`deadzone`, `inverted`, `response`,
    /// `saturation`, `activation`).
    Calibration(&'static str),
    /// The number of tokens on the line.
    Arity,
}

/// Why a profile was refused, on write, on parse or on disk access.
#[derive(Debug)]
pub enum ProfileError {
    /// A calibration record is keyed by an enumeration index and may not be
    /// persisted (non-negotiable behavior 1). Nothing was written.
    UnstableDevice {
        /// The index-only device.
        device: DeviceId,
    },
    /// The profile holds more records than the format allows.
    TooMany {
        /// `"bindings"` or `"calibrations"`.
        what: &'static str,
        /// The limit.
        max: usize,
    },
    /// The file is larger than [`MAX_PROFILE_BYTES`].
    TooLarge {
        /// Its size in bytes.
        len: usize,
    },
    /// The file is not UTF-8.
    NotText,
    /// A line is longer than [`MAX_LINE_BYTES`].
    LineTooLong {
        /// The 1-based line number.
        line: usize,
        /// Its length in bytes.
        len: usize,
    },
    /// The first line is not a profile header.
    MissingHeader,
    /// The header names a version this build does not read.
    UnsupportedVersion {
        /// The header line as found.
        found: String,
    },
    /// The file ends before its `end` line, as a truncated write would.
    Truncated,
    /// Text follows the `end` line.
    TrailingContent {
        /// The 1-based line number.
        line: usize,
    },
    /// A record is malformed in one named field.
    Malformed {
        /// The 1-based line number.
        line: usize,
        /// The field.
        field: ProfileField,
        /// The offending text.
        text: String,
    },
    /// A calibration's device identity was refused by [`DeviceId::stable`].
    DeviceId {
        /// The 1-based line number.
        line: usize,
        /// The refusal.
        error: DeviceIdError,
    },
    /// A calibration field was refused by [`AxisCalibration::try_new`].
    Calibration {
        /// The 1-based line number.
        line: usize,
        /// The refusal.
        error: CalibrationError,
    },
    /// A calibration's channel is read by a different device class.
    ChannelClassMismatch {
        /// The 1-based line number.
        line: usize,
        /// The record's device.
        device: DeviceId,
        /// The record's channel.
        channel: AxisChannel,
    },
    /// Two calibration records name the same device and channel.
    DuplicateCalibration {
        /// The 1-based line number of the second record.
        line: usize,
        /// The device.
        device: DeviceId,
        /// The channel.
        channel: AxisChannel,
    },
    /// The bindings conflict or are malformed: reported, never resolved by
    /// order (non-negotiable behavior 5).
    Bindings {
        /// The action map's refusal.
        error: ActionMapError,
    },
    /// The file system refused an operation.
    Io {
        /// What was being done.
        operation: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The operating system's error.
        source: io::Error,
    },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnstableDevice { device } => write!(
                f,
                "{device} is known only by its enumeration index and may not be persisted"
            ),
            Self::TooMany { what, max } => write!(f, "a profile may hold at most {max} {what}"),
            Self::TooLarge { len } => {
                write!(f, "the profile is {len} bytes, max is {MAX_PROFILE_BYTES}")
            }
            Self::NotText => write!(f, "the profile is not UTF-8 text"),
            Self::LineTooLong { line, len } => {
                write!(f, "line {line} is {len} bytes, max is {MAX_LINE_BYTES}")
            }
            Self::MissingHeader => write!(f, "the profile does not start with {PROFILE_FORMAT:?}"),
            Self::UnsupportedVersion { found } => {
                write!(f, "unsupported profile header {found:?}")
            }
            Self::Truncated => write!(f, "the profile ends before its `end` line"),
            Self::TrailingContent { line } => {
                write!(f, "line {line} follows the `end` line")
            }
            Self::Malformed { line, field, text } => {
                write!(f, "line {line}: malformed {field:?}: {text:?}")
            }
            Self::DeviceId { line, error } => write!(f, "line {line}: {error}"),
            Self::Calibration { line, error } => write!(f, "line {line}: {error}"),
            Self::ChannelClassMismatch {
                line,
                device,
                channel,
            } => write!(
                f,
                "line {line}: {channel} is not read by a {} device",
                device.class()
            ),
            Self::DuplicateCalibration {
                line,
                device,
                channel,
            } => write!(
                f,
                "line {line}: {device} has a second calibration for {channel}"
            ),
            Self::Bindings { error } => write!(f, "the bindings are refused: {error}"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "could not {operation} {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::DeviceId { error, .. } => Some(error),
            Self::Calibration { error, .. } => Some(error),
            Self::Bindings { error } => Some(error),
            _ => None,
        }
    }
}

/// The bindings and calibration of one session, as persisted.
#[derive(Clone, Debug, PartialEq)]
pub struct InputProfile {
    map: ActionMap,
    calibration: CalibrationStore,
}

impl InputProfile {
    /// A profile of the given map and calibration.
    #[must_use]
    pub const fn new(map: ActionMap, calibration: CalibrationStore) -> Self {
        Self { map, calibration }
    }

    /// Captures a collector's current bindings and calibration.
    ///
    /// Records of a device still known only by its enumeration index are
    /// captured as they are and **refused at write time** by
    /// [`to_text`](Self::to_text); a device that has since adopted a stable
    /// identity was re-keyed by `DeviceAdapters::adopt_identity`, so what is
    /// captured here is already keyed by that identity.
    #[must_use]
    pub fn capture(collector: &InputCollector) -> Self {
        Self {
            map: collector.bindings().map().clone(),
            calibration: collector.devices().calibration().clone(),
        }
    }

    /// The action map.
    #[must_use]
    pub const fn map(&self) -> &ActionMap {
        &self.map
    }

    /// The calibration records.
    #[must_use]
    pub const fn calibration(&self) -> &CalibrationStore {
        &self.calibration
    }

    /// Installs this profile in a session: its map replaces the session's, and
    /// its calibration records replace the session's **stable** records. The
    /// session's index-keyed provisional records are left alone, since they
    /// describe devices connected right now that the profile cannot know.
    pub fn apply(self, collector: &mut InputCollector) {
        collector.bindings_mut().set_map(self.map);
        let store = collector.devices_mut().calibration_mut();
        let stable: Vec<DeviceId> = store
            .entries()
            .filter(|(device, _, _)| device.is_stable())
            .map(|(device, _, _)| device.clone())
            .collect();
        for device in stable {
            store.forget_device(&device);
        }
        for (device, channel, calibration) in self.calibration.entries() {
            store.set(device, channel, calibration);
        }
    }

    /// Serializes the profile.
    ///
    /// # Errors
    ///
    /// [`ProfileError::UnstableDevice`] for a record keyed by an enumeration
    /// index, and [`ProfileError::TooMany`] past the format's limits.
    pub fn to_text(&self) -> Result<String, ProfileError> {
        if let Some(device) = self.calibration.unstable_devices().first() {
            return Err(ProfileError::UnstableDevice {
                device: (*device).clone(),
            });
        }
        if self.map.bindings().len() > MAX_BINDINGS {
            return Err(ProfileError::TooMany {
                what: "bindings",
                max: MAX_BINDINGS,
            });
        }
        if self.calibration.len() > MAX_CALIBRATIONS {
            return Err(ProfileError::TooMany {
                what: "calibrations",
                max: MAX_CALIBRATIONS,
            });
        }
        let mut text = String::new();
        text.push_str(PROFILE_FORMAT);
        text.push('\n');
        for binding in self.map.bindings() {
            text.push_str(&format!(
                "bind {} {}\n",
                binding.source.label(),
                target_text(binding.target)
            ));
        }
        for (device, channel, calibration) in self.calibration.entries() {
            let identity =
                device
                    .stable_identity()
                    .ok_or_else(|| ProfileError::UnstableDevice {
                        device: device.clone(),
                    })?;
            text.push_str(&format!(
                "calibration {} {} {} deadzone={} inverted={} response={} saturation={} activation={}\n",
                device.class().label(),
                escape_identity(identity),
                channel.label(),
                calibration.deadzone(),
                u8::from(calibration.inverted()),
                match calibration.response() {
                    ResponseCurve::Linear => "linear".to_owned(),
                    ResponseCurve::Power(exponent) => format!("power:{exponent}"),
                },
                calibration.saturation(),
                calibration.activation(),
            ));
        }
        text.push_str("end\n");
        Ok(text)
    }

    /// Parses a profile.
    ///
    /// # Errors
    ///
    /// A [`ProfileError`] naming the first malformed line and field. Nothing
    /// is clamped or repaired, and conflicting bindings are returned as
    /// [`ProfileError::Bindings`] instead of being resolved by order.
    pub fn parse(bytes: &[u8]) -> Result<Self, ProfileError> {
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(ProfileError::TooLarge { len: bytes.len() });
        }
        let text = std::str::from_utf8(bytes).map_err(|_| ProfileError::NotText)?;
        // Every line, the last included, ends in a newline; a file that does
        // not is a write that stopped short.
        let body = text.strip_suffix('\n').ok_or(ProfileError::Truncated)?;
        let mut lines = body.split('\n').enumerate().map(|(i, l)| (i + 1, l));
        let (_, header) = lines.next().ok_or(ProfileError::MissingHeader)?;
        if header != PROFILE_FORMAT {
            return Err(if header.starts_with("cs-input-profile ") {
                ProfileError::UnsupportedVersion {
                    found: truncate(header),
                }
            } else {
                ProfileError::MissingHeader
            });
        }
        let mut bindings = Vec::new();
        let mut calibration = CalibrationStore::new();
        let mut ended = false;
        for (number, line) in lines {
            if ended {
                return Err(ProfileError::TrailingContent { line: number });
            }
            if line.len() > MAX_LINE_BYTES {
                return Err(ProfileError::LineTooLong {
                    line: number,
                    len: line.len(),
                });
            }
            let tokens: Vec<&str> = line.split(' ').collect();
            match tokens[0] {
                "end" => {
                    if tokens.len() != 1 {
                        return Err(malformed(number, ProfileField::Arity, line));
                    }
                    ended = true;
                }
                "bind" => {
                    if bindings.len() >= MAX_BINDINGS {
                        return Err(ProfileError::TooMany {
                            what: "bindings",
                            max: MAX_BINDINGS,
                        });
                    }
                    bindings.push(parse_binding(number, line, &tokens)?);
                }
                "calibration" => {
                    if calibration.len() >= MAX_CALIBRATIONS {
                        return Err(ProfileError::TooMany {
                            what: "calibrations",
                            max: MAX_CALIBRATIONS,
                        });
                    }
                    parse_calibration(number, line, &tokens, &mut calibration)?;
                }
                other => return Err(malformed(number, ProfileField::Record, other)),
            }
        }
        if !ended {
            return Err(ProfileError::Truncated);
        }
        let map = ActionMap::try_new(bindings).map_err(|error| ProfileError::Bindings { error })?;
        Ok(Self { map, calibration })
    }

    /// Reads and parses the profile at `path`.
    ///
    /// # Errors
    ///
    /// [`ProfileError::Io`] when the file cannot be read (a missing file is
    /// `io::ErrorKind::NotFound` inside it) and every [`parse`](Self::parse)
    /// refusal. The running session is not touched.
    pub fn load(path: &Path) -> Result<Self, ProfileError> {
        let bytes = fs::read(path).map_err(|source| ProfileError::Io {
            operation: "read",
            path: path.to_owned(),
            source,
        })?;
        Self::parse(&bytes)
    }

    /// Writes the profile to `path` atomically.
    ///
    /// The text is built first (so a refused profile never touches the disk),
    /// written to the sibling [`temporary_path`], synced, and renamed over
    /// `path`. On any failure the temporary file is removed and the previous
    /// profile at `path`, if any, is intact.
    ///
    /// # Errors
    ///
    /// [`ProfileError::UnstableDevice`] / [`ProfileError::TooMany`] before
    /// anything is written, [`ProfileError::Io`] otherwise.
    pub fn save(&self, path: &Path) -> Result<(), ProfileError> {
        let text = self.to_text()?;
        write_atomically(path, text.as_bytes())
    }
}

/// The sibling file a save writes before renaming it over the target.
#[must_use]
pub fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
    let temporary = temporary_path(path);
    let io_error = |operation: &'static str, at: &Path| {
        let at = at.to_owned();
        move |source| ProfileError::Io {
            operation,
            path: at,
            source,
        }
    };
    let result = (|| {
        let mut file = fs::File::create(&temporary).map_err(io_error("create", &temporary))?;
        file.write_all(bytes)
            .map_err(io_error("write", &temporary))?;
        file.sync_all().map_err(io_error("sync", &temporary))?;
        drop(file);
        fs::rename(&temporary, path).map_err(io_error("rename into place", path))
    })();
    if result.is_err() {
        // Best effort: a stale temporary is harmless (the next save truncates
        // it and no load reads it), so a failed removal is not an error.
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn truncate(text: &str) -> String {
    text.chars().take(64).collect()
}

fn malformed(line: usize, field: ProfileField, text: &str) -> ProfileError {
    ProfileError::Malformed {
        line,
        field,
        text: truncate(text),
    }
}

fn escape_identity(identity: &str) -> String {
    let mut out = String::with_capacity(identity.len());
    for byte in identity.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn unescape_identity(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            if !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
            {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn target_text(target: BindingTarget) -> String {
    match target {
        BindingTarget::Axis { command, scale } => format!("axis:{}:{scale}", command.label()),
        BindingTarget::Command(command) => format!("command:{}", command.label()),
        BindingTarget::Ui(action) => format!("ui:{}", action.label()),
    }
}

fn parse_target(text: &str) -> Option<BindingTarget> {
    let (kind, rest) = text.split_once(':')?;
    match kind {
        "axis" => {
            let (command, scale) = rest.split_once(':')?;
            Some(BindingTarget::Axis {
                command: FlightCommand::from_label(command)?,
                scale: scale.parse().ok()?,
            })
        }
        "command" => Some(BindingTarget::Command(FlightCommand::from_label(rest)?)),
        "ui" => Some(BindingTarget::Ui(UiAction::from_label(rest)?)),
        _ => None,
    }
}

fn parse_source(text: &str) -> Option<BindingSource> {
    let (kind, rest) = text.split_once('.')?;
    Some(match kind {
        "key" => BindingSource::Key(Key::from_label(rest)?),
        "mouse_button" => BindingSource::MouseButton(MouseButton::from_label(rest)?),
        "mouse_axis" => BindingSource::MouseAxis(MouseAxis::from_label(rest)?),
        "gamepad_button" => BindingSource::GamepadButton(GamepadButton::from_label(rest)?),
        "gamepad_axis" => BindingSource::GamepadAxis(GamepadAxis::from_label(rest)?),
        "joystick_button" => BindingSource::JoystickButton(parse_index(rest)?),
        "joystick_axis" => {
            let (index, inverted) = match rest.split_once('.') {
                Some((index, "inverted")) => (index, true),
                Some(_) => return None,
                None => (rest, false),
            };
            BindingSource::JoystickAxis {
                index: parse_index(index)?,
                inverted,
            }
        }
        _ => return None,
    })
}

fn parse_channel(text: &str) -> Option<AxisChannel> {
    let (kind, rest) = text.split_once('.')?;
    Some(match kind {
        "mouse" => AxisChannel::Mouse(MouseAxis::from_label(rest)?),
        "gamepad" => AxisChannel::Gamepad(GamepadAxis::from_label(rest)?),
        "joystick" => AxisChannel::Joystick(parse_index(rest)?),
        _ => return None,
    })
}

/// A canonical decimal `u16`: no sign, no leading zero, so that a field has
/// exactly one spelling.
fn parse_index(text: &str) -> Option<u16> {
    if text.is_empty()
        || !text.bytes().all(|b| b.is_ascii_digit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return None;
    }
    text.parse().ok()
}

fn parse_binding(number: usize, line: &str, tokens: &[&str]) -> Result<Binding, ProfileError> {
    if tokens.len() != 3 {
        return Err(malformed(number, ProfileField::Arity, line));
    }
    let source = parse_source(tokens[1])
        .ok_or_else(|| malformed(number, ProfileField::Source, tokens[1]))?;
    let target = parse_target(tokens[2])
        .ok_or_else(|| malformed(number, ProfileField::Target, tokens[2]))?;
    Ok(Binding { source, target })
}

fn parse_calibration(
    number: usize,
    line: &str,
    tokens: &[&str],
    store: &mut CalibrationStore,
) -> Result<(), ProfileError> {
    if tokens.len() != 9 {
        return Err(malformed(number, ProfileField::Arity, line));
    }
    let class = DeviceClass::from_label(tokens[1])
        .ok_or_else(|| malformed(number, ProfileField::DeviceClass, tokens[1]))?;
    let identity = unescape_identity(tokens[2])
        .ok_or_else(|| malformed(number, ProfileField::DeviceIdentity, tokens[2]))?;
    let device = DeviceId::stable(class, &identity).map_err(|error| ProfileError::DeviceId {
        line: number,
        error,
    })?;
    let channel = parse_channel(tokens[3])
        .ok_or_else(|| malformed(number, ProfileField::Channel, tokens[3]))?;
    if channel.device_class() != class {
        return Err(ProfileError::ChannelClassMismatch {
            line: number,
            device,
            channel,
        });
    }
    let field = |index: usize, name: &'static str| {
        tokens[index]
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('='))
            .ok_or_else(|| malformed(number, ProfileField::Calibration(name), tokens[index]))
    };
    let float = |index: usize, name: &'static str| {
        field(index, name)?
            .parse::<f32>()
            .map_err(|_| malformed(number, ProfileField::Calibration(name), tokens[index]))
    };
    let deadzone = float(4, "deadzone")?;
    let inverted = match field(5, "inverted")? {
        "0" => false,
        "1" => true,
        _ => {
            return Err(malformed(
                number,
                ProfileField::Calibration("inverted"),
                tokens[5],
            ));
        }
    };
    let response = match field(6, "response")? {
        "linear" => ResponseCurve::Linear,
        other => match other.strip_prefix("power:").map(str::parse::<f32>) {
            Some(Ok(exponent)) => ResponseCurve::Power(exponent),
            _ => {
                return Err(malformed(
                    number,
                    ProfileField::Calibration("response"),
                    tokens[6],
                ));
            }
        },
    };
    let saturation = float(7, "saturation")?;
    let activation = float(8, "activation")?;
    let calibration = AxisCalibration::try_new(
        deadzone, inverted, response, saturation, activation,
    )
    .map_err(|error| ProfileError::Calibration {
        line: number,
        error,
    })?;
    if store.get(&device, channel).is_some() {
        return Err(ProfileError::DuplicateCalibration {
            line: number,
            device,
            channel,
        });
    }
    store.set(&device, channel, calibration);
    Ok(())
}
