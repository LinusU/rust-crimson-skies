//! Accessibility settings and the fidelity boundary between presentation and
//! gameplay (F52-A).
//!
//! Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! stage `### F52-A`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! [`Settings`] is split in two on purpose:
//!
//! * [`Presentation`] — UI scale, subtitles, colour filter, per-bus audio
//!   levels, reduced shake/flash and resolution. None of it can reach the
//!   simulation: [`Settings::gameplay_inputs`], the only view the simulation
//!   and the evidence runs read, is built from the [`ModernProfile`] alone
//!   (non-negotiable behavior 1, AC03).
//! * [`ModernProfile`] — mouse flight, controller flight and a wider field of
//!   view. These are gameplay assists. They apply only while
//!   [`ProfileKind::ModernAssist`] is active, and every active one is named in
//!   the [`FidelityLabel`] that comparison and replay metadata record (AC04).
//!
//! Everything here is **designed**: no original option, range or default is
//! recorded. The ranges are authored limits that keep a corrupt file from
//! producing an unusable display; see
//! `docs/findings/2026-10-01-f52-a-accessibility-settings.md`.
//!
//! [`Settings::to_text`] / [`Settings::from_text`] are the strict, versioned
//! line format the application persists; the atomic file write is
//! `cs_app::accessibility::store`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::audio::{AudioBus, AudioLevel, AudioLevelError};

/// Smallest UI scale, in percent.
pub const MIN_UI_SCALE_PERCENT: u16 = 100;
/// Largest UI scale, in percent.
pub const MAX_UI_SCALE_PERCENT: u16 = 300;
/// Smallest usable surface side, in pixels.
pub const MIN_SURFACE_SIDE: u32 = 320;
/// Largest accepted surface side, in pixels.
pub const MAX_SURFACE_SIDE: u32 = 16_384;
/// Smallest field of view, in degrees.
pub const MIN_FOV_DEGREES: u16 = 40;
/// Largest field of view, in degrees.
pub const MAX_FOV_DEGREES: u16 = 130;

/// The first line of a persisted settings file.
const HEADER: &str = "cs-settings 1";

/// A colour-vision filter applied to the final image. It never changes what an
/// objective cue says; see `cs_app::accessibility::cues`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColourFilter {
    /// No filter.
    Off,
    /// Red-weak vision.
    Protanopia,
    /// Green-weak vision.
    Deuteranopia,
    /// Blue-weak vision.
    Tritanopia,
    /// No colour at all.
    Monochrome,
}

impl ColourFilter {
    /// Every filter.
    pub const ALL: [Self; 5] = [
        Self::Off,
        Self::Protanopia,
        Self::Deuteranopia,
        Self::Tritanopia,
        Self::Monochrome,
    ];

    /// The stable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Protanopia => "protanopia",
            Self::Deuteranopia => "deuteranopia",
            Self::Tritanopia => "tritanopia",
            Self::Monochrome => "monochrome",
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|filter| filter.label() == label)
    }
}

/// Display surface size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Settings that change what is shown or heard, never what is simulated.
#[derive(Clone, Debug, PartialEq)]
pub struct Presentation {
    /// UI scale in percent, [`MIN_UI_SCALE_PERCENT`]..=[`MAX_UI_SCALE_PERCENT`].
    pub ui_scale_percent: u16,
    /// Whether voice and radio lines are subtitled.
    pub subtitles: bool,
    /// The colour filter.
    pub colour_filter: ColourFilter,
    /// Linear level of each audio bus; a missing bus is unity, and a bus at
    /// unity is stored as missing.
    pub bus_levels: BTreeMap<AudioBus, AudioLevel>,
    /// Drop cosmetic camera shake.
    pub reduce_shake: bool,
    /// Drop cosmetic screen flashes.
    pub reduce_flash: bool,
    /// The display surface.
    pub resolution: Resolution,
}

impl Presentation {
    /// The designed defaults.
    #[must_use]
    pub fn designed() -> Self {
        Self {
            ui_scale_percent: 100,
            subtitles: false,
            colour_filter: ColourFilter::Off,
            bus_levels: BTreeMap::new(),
            reduce_shake: false,
            reduce_flash: false,
            resolution: Resolution {
                width: 1280,
                height: 720,
            },
        }
    }

    /// The level of a bus.
    #[must_use]
    pub fn bus_level(&self, bus: AudioBus) -> AudioLevel {
        self.bus_levels
            .get(&bus)
            .copied()
            .unwrap_or(AudioLevel::UNITY)
    }
}

/// Which rule set the game is played under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    /// The original rules; modern assists are inert.
    OriginalRules,
    /// The modern profile applies.
    ModernAssist,
}

impl ProfileKind {
    /// The stable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OriginalRules => "original-rules",
            Self::ModernAssist => "modern-assist",
        }
    }
}

/// One gameplay assist, named so a record can say exactly what was on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModernAssist {
    /// Fly with the mouse.
    MouseFlight,
    /// Fly with the controller's modern scheme.
    ControllerFlight,
    /// A field of view other than the original's, in degrees.
    Fov(u16),
}

impl fmt::Display for ModernAssist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MouseFlight => f.write_str("mouse-flight"),
            Self::ControllerFlight => f.write_str("controller-flight"),
            Self::Fov(degrees) => write!(f, "fov-{degrees}"),
        }
    }
}

/// The modern options, kept apart from [`Presentation`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModernProfile {
    /// Mouse flight.
    pub mouse_flight: bool,
    /// Modern controller flight.
    pub controller_flight: bool,
    /// A non-original field of view; `None` keeps the original's.
    pub fov_degrees: Option<u16>,
}

impl ModernProfile {
    /// The assists this profile turns on, in a stable order.
    #[must_use]
    pub fn assists(&self) -> Vec<ModernAssist> {
        let mut assists = BTreeSet::new();
        if self.mouse_flight {
            assists.insert(ModernAssist::MouseFlight);
        }
        if self.controller_flight {
            assists.insert(ModernAssist::ControllerFlight);
        }
        if let Some(degrees) = self.fov_degrees {
            assists.insert(ModernAssist::Fov(degrees));
        }
        assists.into_iter().collect()
    }
}

/// What the simulation and the evidence runs may read from the settings.
///
/// It carries no presentation field, so no presentation setting can change
/// gameplay telemetry (AC03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameplayInputs {
    /// The assists in force; empty under the original rules.
    pub assists: Vec<ModernAssist>,
}

/// The fidelity record comparison and replay metadata carry (AC04).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FidelityLabel {
    /// The assists in force; empty means the original rules.
    pub assists: Vec<ModernAssist>,
}

impl FidelityLabel {
    /// Whether the run is under the original rules, with no assist.
    #[must_use]
    pub fn is_original_rules(&self) -> bool {
        self.assists.is_empty()
    }

    /// The metadata entries, in order. A modified run always carries its
    /// assists by name.
    #[must_use]
    pub fn metadata(&self) -> Vec<(String, String)> {
        if self.is_original_rules() {
            return vec![("fidelity".to_owned(), "original-rules".to_owned())];
        }
        let names: Vec<String> = self.assists.iter().map(ToString::to_string).collect();
        vec![
            ("fidelity".to_owned(), "modified".to_owned()),
            ("assists".to_owned(), names.join(",")),
        ]
    }
}

/// Why a settings value was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsError {
    /// The UI scale is outside its range.
    UiScale(u16),
    /// A surface side is outside its range.
    Resolution {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
    },
    /// The field of view is outside its range.
    Fov(u16),
    /// A persisted file did not start with the expected header.
    BadHeader,
    /// A line is not `key=value`.
    BadLine(String),
    /// A key is not a setting.
    UnknownKey(String),
    /// A key appears twice.
    DuplicateKey(String),
    /// A required key is absent.
    MissingKey(&'static str),
    /// A value does not parse for its key.
    BadValue {
        /// The key.
        key: String,
        /// The value.
        value: String,
    },
    /// A bus level is not a valid gain.
    Level(AudioLevelError),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UiScale(v) => write!(
                f,
                "UI scale {v}% is outside {MIN_UI_SCALE_PERCENT}..={MAX_UI_SCALE_PERCENT}"
            ),
            Self::Resolution { width, height } => write!(
                f,
                "resolution {width}x{height} is outside {MIN_SURFACE_SIDE}..={MAX_SURFACE_SIDE} per side"
            ),
            Self::Fov(v) => write!(
                f,
                "field of view {v} is outside {MIN_FOV_DEGREES}..={MAX_FOV_DEGREES}"
            ),
            Self::BadHeader => write!(f, "the settings file does not start with `{HEADER}`"),
            Self::BadLine(line) => write!(f, "`{line}` is not key=value"),
            Self::UnknownKey(key) => write!(f, "`{key}` is not a setting"),
            Self::DuplicateKey(key) => write!(f, "`{key}` appears twice"),
            Self::MissingKey(key) => write!(f, "`{key}` is missing"),
            Self::BadValue { key, value } => write!(f, "`{value}` is not valid for `{key}`"),
            Self::Level(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SettingsError {}

/// All accessibility and modern-option settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Presentation-only settings.
    pub presentation: Presentation,
    /// Which rule set is active.
    pub profile: ProfileKind,
    /// The modern options; inert unless `profile` is
    /// [`ProfileKind::ModernAssist`], and kept so switching back and forth
    /// loses nothing.
    pub modern: ModernProfile,
}

impl Settings {
    /// The designed defaults: original rules, nothing modern.
    #[must_use]
    pub fn designed() -> Self {
        Self {
            presentation: Presentation::designed(),
            profile: ProfileKind::OriginalRules,
            modern: ModernProfile::default(),
        }
    }

    /// Checks every range.
    ///
    /// # Errors
    ///
    /// The first [`SettingsError`] found.
    pub fn validate(&self) -> Result<(), SettingsError> {
        let p = &self.presentation;
        if !(MIN_UI_SCALE_PERCENT..=MAX_UI_SCALE_PERCENT).contains(&p.ui_scale_percent) {
            return Err(SettingsError::UiScale(p.ui_scale_percent));
        }
        let side = MIN_SURFACE_SIDE..=MAX_SURFACE_SIDE;
        if !side.contains(&p.resolution.width) || !side.contains(&p.resolution.height) {
            return Err(SettingsError::Resolution {
                width: p.resolution.width,
                height: p.resolution.height,
            });
        }
        if let Some(fov) = self.modern.fov_degrees
            && !(MIN_FOV_DEGREES..=MAX_FOV_DEGREES).contains(&fov)
        {
            return Err(SettingsError::Fov(fov));
        }
        Ok(())
    }

    /// The assists actually in force.
    #[must_use]
    pub fn effective_assists(&self) -> Vec<ModernAssist> {
        match self.profile {
            ProfileKind::OriginalRules => Vec::new(),
            ProfileKind::ModernAssist => self.modern.assists(),
        }
    }

    /// The only view of the settings the simulation may read.
    #[must_use]
    pub fn gameplay_inputs(&self) -> GameplayInputs {
        GameplayInputs {
            assists: self.effective_assists(),
        }
    }

    /// The label replay and comparison metadata must record.
    #[must_use]
    pub fn fidelity(&self) -> FidelityLabel {
        FidelityLabel {
            assists: self.effective_assists(),
        }
    }

    /// The persisted form: a header and one `key=value` per setting.
    #[must_use]
    pub fn to_text(&self) -> String {
        let p = &self.presentation;
        let mut out = format!("{HEADER}\n");
        let mut line = |key: &str, value: String| out.push_str(&format!("{key}={value}\n"));
        line("ui_scale_percent", p.ui_scale_percent.to_string());
        line("subtitles", p.subtitles.to_string());
        line("colour_filter", p.colour_filter.label().to_owned());
        for bus in AudioBus::ALL {
            line(
                &format!("bus.{}", bus.label()),
                p.bus_level(*bus).to_string(),
            );
        }
        line("reduce_shake", p.reduce_shake.to_string());
        line("reduce_flash", p.reduce_flash.to_string());
        line("width", p.resolution.width.to_string());
        line("height", p.resolution.height.to_string());
        line("profile", self.profile.label().to_owned());
        line("mouse_flight", self.modern.mouse_flight.to_string());
        line(
            "controller_flight",
            self.modern.controller_flight.to_string(),
        );
        line(
            "fov_degrees",
            self.modern
                .fov_degrees
                .map_or_else(|| "none".to_owned(), |v| v.to_string()),
        );
        out
    }

    /// Parses and validates [`Self::to_text`] output. The format is strict: an
    /// unknown, duplicated or missing key is an error, never ignored.
    ///
    /// # Errors
    ///
    /// A [`SettingsError`].
    pub fn from_text(text: &str) -> Result<Self, SettingsError> {
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Err(SettingsError::BadHeader);
        }
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        for line in lines {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| SettingsError::BadLine(line.to_owned()))?;
            if values.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(SettingsError::DuplicateKey(key.to_owned()));
            }
        }
        fn take(
            values: &mut BTreeMap<String, String>,
            key: &'static str,
        ) -> Result<(&'static str, String), SettingsError> {
            values
                .remove(key)
                .ok_or(SettingsError::MissingKey(key))
                .map(|value| (key, value))
        }
        fn bad(key: &str, value: &str) -> SettingsError {
            SettingsError::BadValue {
                key: key.to_owned(),
                value: value.to_owned(),
            }
        }
        fn parse<T: std::str::FromStr>(key: &str, value: &str) -> Result<T, SettingsError> {
            value.parse().map_err(|_| bad(key, value))
        }
        let (k, v) = take(&mut values, "ui_scale_percent")?;
        let ui_scale_percent = parse(k, &v)?;
        let (k, v) = take(&mut values, "subtitles")?;
        let subtitles = parse(k, &v)?;
        let (k, v) = take(&mut values, "colour_filter")?;
        let colour_filter = ColourFilter::from_label(&v).ok_or_else(|| bad(k, &v))?;
        let mut bus_levels = BTreeMap::new();
        for bus in AudioBus::ALL {
            let key = format!("bus.{}", bus.label());
            let raw = values
                .remove(&key)
                .ok_or(SettingsError::MissingKey("bus.<label>"))?;
            let level: f64 = parse(&key, &raw)?;
            let level = AudioLevel::try_new(level).map_err(SettingsError::Level)?;
            // A unity bus is stored as absent, so equal settings compare equal.
            if level != AudioLevel::UNITY {
                bus_levels.insert(*bus, level);
            }
        }
        let (k, v) = take(&mut values, "reduce_shake")?;
        let reduce_shake = parse(k, &v)?;
        let (k, v) = take(&mut values, "reduce_flash")?;
        let reduce_flash = parse(k, &v)?;
        let (k, v) = take(&mut values, "width")?;
        let width = parse(k, &v)?;
        let (k, v) = take(&mut values, "height")?;
        let height = parse(k, &v)?;
        let (k, v) = take(&mut values, "profile")?;
        let profile = match v.as_str() {
            "original-rules" => ProfileKind::OriginalRules,
            "modern-assist" => ProfileKind::ModernAssist,
            _ => return Err(bad(k, &v)),
        };
        let (k, v) = take(&mut values, "mouse_flight")?;
        let mouse_flight = parse(k, &v)?;
        let (k, v) = take(&mut values, "controller_flight")?;
        let controller_flight = parse(k, &v)?;
        let (k, v) = take(&mut values, "fov_degrees")?;
        let fov_degrees = if v == "none" {
            None
        } else {
            Some(parse(k, &v)?)
        };
        if let Some(key) = values.into_keys().next() {
            return Err(SettingsError::UnknownKey(key));
        }
        let settings = Self {
            presentation: Presentation {
                ui_scale_percent,
                subtitles,
                colour_filter,
                bus_levels,
                reduce_shake,
                reduce_flash,
                resolution: Resolution { width, height },
            },
            profile,
            modern: ModernProfile {
                mouse_flight,
                controller_flight,
                fov_degrees,
            },
        };
        settings.validate()?;
        Ok(settings)
    }
}
