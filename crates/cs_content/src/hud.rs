//! Declared HUD display-unit policy (F46-A).
//!
//! Spec: `specs/F46-hud-instruments-mission-map-and-pause.md`, stage `### F46-A`
//! ("Show original display units where verified while simulation remains SI.
//! Airspeed and ground speed, altitude datum and low-altitude warnings require
//! explicit definitions.").
//!
//! A [`HudPolicy`] says how SI simulation values become the numbers on a
//! gauge: the unit of each, which speed the airspeed gauge reads, which datum
//! altitude is measured from and when the low-altitude warning sets and clears.
//! Every choice is a [`Declared`] value carrying its [`Basis`], so a designed
//! default can never be mistaken for a verified original unit. No original unit,
//! datum or threshold is recorded here: [`HudPolicy::designed`] is an authored
//! stand-in, and the original values are F46-B's to import and F46-D's to check.

use std::fmt;

/// Where a declared value comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Basis {
    /// Authored for this engine; not an original measurement.
    Designed,
    /// Confirmed against the original, named by its evidence record.
    OriginalVerified {
        /// The evidence record that confirms the value.
        evidence: String,
    },
}

impl Basis {
    /// Whether the value is confirmed against the original.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        matches!(self, Self::OriginalVerified { .. })
    }
}

/// A policy value and its basis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declared<T> {
    /// The value.
    pub value: T,
    /// Where it comes from.
    pub basis: Basis,
}

impl<T> Declared<T> {
    /// A designed (unverified) value.
    #[must_use]
    pub fn designed(value: T) -> Self {
        Self {
            value,
            basis: Basis::Designed,
        }
    }
}

/// The unit a speed gauge shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedUnit {
    /// Metres per second (the simulation unit).
    MetersPerSecond,
    /// Kilometres per hour.
    KilometersPerHour,
    /// Statute miles per hour.
    MilesPerHour,
    /// Nautical miles per hour.
    Knots,
}

impl SpeedUnit {
    /// How many of this unit make one metre per second.
    #[must_use]
    pub const fn per_meter_per_second(self) -> f64 {
        match self {
            Self::MetersPerSecond => 1.0,
            Self::KilometersPerHour => 3.6,
            Self::MilesPerHour => 3600.0 / 1609.344,
            Self::Knots => 3600.0 / 1852.0,
        }
    }
}

/// The unit an altitude gauge shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltitudeUnit {
    /// Metres (the simulation unit).
    Meters,
    /// International feet.
    Feet,
}

impl AltitudeUnit {
    /// How many of this unit make one metre.
    #[must_use]
    pub const fn per_meter(self) -> f64 {
        match self {
            Self::Meters => 1.0,
            Self::Feet => 1.0 / 0.3048,
        }
    }
}

/// Which velocity the airspeed gauge reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedReference {
    /// The speed through the air: `|v_world - wind|`.
    AirRelative,
    /// The speed over the ground: the horizontal part of `v_world`.
    Ground,
}

/// The height altitude is measured from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltitudeDatum {
    /// The canonical world's `y = 0` plane.
    WorldOrigin,
    /// The terrain directly below the aircraft; needs a ground height.
    TerrainBelow,
}

/// The whole display policy.
#[derive(Clone, Debug, PartialEq)]
pub struct HudPolicy {
    /// Unit of the airspeed gauge.
    pub speed_unit: Declared<SpeedUnit>,
    /// Which speed the airspeed gauge reads.
    pub airspeed_reference: Declared<SpeedReference>,
    /// Unit of the altitude gauge.
    pub altitude_unit: Declared<AltitudeUnit>,
    /// Datum of the altitude gauge and the low-altitude warning.
    pub altitude_datum: Declared<AltitudeDatum>,
    /// The warning sets when altitude drops below this many metres.
    pub low_altitude_warn_m: Declared<f64>,
    /// The warning clears when altitude rises above this many metres.
    pub low_altitude_clear_m: Declared<f64>,
}

/// Why a [`HudPolicy`] was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum HudPolicyError {
    /// A threshold is not a finite non-negative number.
    BadThreshold {
        /// The field.
        field: &'static str,
        /// The offending value.
        value: f64,
    },
    /// The clear threshold is not above the warn threshold, so the warning
    /// would chatter at the boundary.
    NoHysteresis {
        /// Warn threshold in metres.
        warn: f64,
        /// Clear threshold in metres.
        clear: f64,
    },
}

impl fmt::Display for HudPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadThreshold { field, value } => {
                write!(f, "{field} must be finite and non-negative, got {value}")
            }
            Self::NoHysteresis { warn, clear } => write!(
                f,
                "low-altitude clear threshold {clear} m must exceed the warn threshold {warn} m"
            ),
        }
    }
}

impl std::error::Error for HudPolicyError {}

impl HudPolicy {
    /// An authored, unverified stand-in: m/s, metres, air-relative speed,
    /// altitude above the terrain, warn below 100 m and clear above 150 m.
    #[must_use]
    pub fn designed() -> Self {
        Self {
            speed_unit: Declared::designed(SpeedUnit::MetersPerSecond),
            airspeed_reference: Declared::designed(SpeedReference::AirRelative),
            altitude_unit: Declared::designed(AltitudeUnit::Meters),
            altitude_datum: Declared::designed(AltitudeDatum::TerrainBelow),
            low_altitude_warn_m: Declared::designed(100.0),
            low_altitude_clear_m: Declared::designed(150.0),
        }
    }

    /// Checks the thresholds.
    ///
    /// # Errors
    ///
    /// [`HudPolicyError::BadThreshold`] for a non-finite or negative threshold,
    /// [`HudPolicyError::NoHysteresis`] when clear is not above warn.
    pub fn validate(&self) -> Result<(), HudPolicyError> {
        for (field, value) in [
            ("low_altitude_warn_m", self.low_altitude_warn_m.value),
            ("low_altitude_clear_m", self.low_altitude_clear_m.value),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(HudPolicyError::BadThreshold { field, value });
            }
        }
        let (warn, clear) = (
            self.low_altitude_warn_m.value,
            self.low_altitude_clear_m.value,
        );
        if clear <= warn {
            return Err(HudPolicyError::NoHysteresis { warn, clear });
        }
        Ok(())
    }

    /// Whether every choice is confirmed against the original. Fidelity mode
    /// may claim original display units only when this holds.
    #[must_use]
    pub fn fully_verified(&self) -> bool {
        self.speed_unit.basis.is_verified()
            && self.airspeed_reference.basis.is_verified()
            && self.altitude_unit.basis.is_verified()
            && self.altitude_datum.basis.is_verified()
            && self.low_altitude_warn_m.basis.is_verified()
            && self.low_altitude_clear_m.basis.is_verified()
    }
}
