//! The handling probe vocabulary: which maneuver is flown and which quantity
//! its reference envelope measures (F26-A).
//!
//! Spec: `specs/F26-handling-probes-and-original-behavior-calibration.md`,
//! stage `### F26-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! The sheet lists the maneuvers a calibration must cover: "straight
//! acceleration, coast-down, climb, dive, turn, roll, yaw, stall recovery,
//! damage and boost". [`ProbeKind`] is exactly that closed set, so a reference
//! envelope that omits one is visible ([`super::ReferenceEnvelope::missing_maneuvers`])
//! rather than silently partial.
//!
//! [`ProbeQuantity`] is the one quantity each maneuver's envelope primarily
//! bounds, with its canonical unit. The pair is the join key between a
//! reference envelope and the measurements of a probe run
//! ([`super::ProbeMeasurement`]). The vocabulary is newly authored project
//! design: naming a maneuver implies nothing about how the original game flew
//! it, and no value here is an original measurement.

/// One handling maneuver a probe flies.
///
/// The set is the F26 sheet's list, and it is closed: a maneuver that is not
/// declared here has no label, so it cannot be written into a record or a
/// report as if it were known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProbeKind {
    /// Straight full-throttle acceleration.
    Acceleration,
    /// Throttle-cut coast-down.
    CoastDown,
    /// Sustained climb.
    Climb,
    /// Sustained dive.
    Dive,
    /// Sustained turn.
    Turn,
    /// Roll response.
    Roll,
    /// Yaw response.
    Yaw,
    /// Stall entry and recovery.
    StallRecovery,
    /// Handling with damaged control authority.
    Damage,
    /// Boost engagement and burn.
    Boost,
}

impl ProbeKind {
    /// Every declared maneuver, in the sheet's order.
    pub const ALL: [Self; 10] = [
        Self::Acceleration,
        Self::CoastDown,
        Self::Climb,
        Self::Dive,
        Self::Turn,
        Self::Roll,
        Self::Yaw,
        Self::StallRecovery,
        Self::Damage,
        Self::Boost,
    ];

    /// The stable label used in records and reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Acceleration => "acceleration",
            Self::CoastDown => "coast_down",
            Self::Climb => "climb",
            Self::Dive => "dive",
            Self::Turn => "turn",
            Self::Roll => "roll",
            Self::Yaw => "yaw",
            Self::StallRecovery => "stall_recovery",
            Self::Damage => "damage",
            Self::Boost => "boost",
        }
    }

    /// Looks a maneuver up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|maneuver| maneuver.label() == label)
    }

    /// The quantity this maneuver's reference envelope primarily bounds.
    #[must_use]
    pub const fn quantity(self) -> ProbeQuantity {
        match self {
            Self::Acceleration => ProbeQuantity::SpeedGainMps,
            Self::CoastDown => ProbeQuantity::SpeedLossMps,
            Self::Climb => ProbeQuantity::ClimbRateMps,
            Self::Dive => ProbeQuantity::DiveSpeedGainMps,
            Self::Turn => ProbeQuantity::TurnRadiusM,
            Self::Roll => ProbeQuantity::RollRateRadps,
            Self::Yaw => ProbeQuantity::YawRateRadps,
            Self::StallRecovery => ProbeQuantity::StallRecoveryS,
            Self::Damage => ProbeQuantity::ControlAuthorityFraction,
            Self::Boost => ProbeQuantity::BoostSpeedGainMps,
        }
    }
}

/// The measured quantity of one probe maneuver, with its canonical unit.
///
/// A quantity's [`ProbeQuantity::unit`] is the unit a record must declare for
/// it; a record that declares a different unit is refused
/// ([`super::HandlingError::UnitMismatch`]) instead of being silently
/// reinterpreted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProbeQuantity {
    /// Airspeed gained over a straight full-throttle run.
    SpeedGainMps,
    /// Airspeed lost after the throttle is cut.
    SpeedLossMps,
    /// Sustained climb rate.
    ClimbRateMps,
    /// Airspeed gained in a sustained dive.
    DiveSpeedGainMps,
    /// The radius of a sustained turn.
    TurnRadiusM,
    /// Peak or sustained roll rate.
    RollRateRadps,
    /// Peak or sustained yaw rate.
    YawRateRadps,
    /// Time from stall entry to recovery.
    StallRecoveryS,
    /// The fraction of control authority retained under damage.
    ControlAuthorityFraction,
    /// Airspeed gained under boost.
    BoostSpeedGainMps,
}

impl ProbeQuantity {
    /// Every declared quantity, in a stable order.
    pub const ALL: [Self; 10] = [
        Self::SpeedGainMps,
        Self::SpeedLossMps,
        Self::ClimbRateMps,
        Self::DiveSpeedGainMps,
        Self::TurnRadiusM,
        Self::RollRateRadps,
        Self::YawRateRadps,
        Self::StallRecoveryS,
        Self::ControlAuthorityFraction,
        Self::BoostSpeedGainMps,
    ];

    /// The stable label used in records and reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SpeedGainMps => "speed_gain_mps",
            Self::SpeedLossMps => "speed_loss_mps",
            Self::ClimbRateMps => "climb_rate_mps",
            Self::DiveSpeedGainMps => "dive_speed_gain_mps",
            Self::TurnRadiusM => "turn_radius_m",
            Self::RollRateRadps => "roll_rate_radps",
            Self::YawRateRadps => "yaw_rate_radps",
            Self::StallRecoveryS => "stall_recovery_s",
            Self::ControlAuthorityFraction => "control_authority_fraction",
            Self::BoostSpeedGainMps => "boost_speed_gain_mps",
        }
    }

    /// The canonical unit a record must declare for this quantity.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Self::SpeedGainMps
            | Self::SpeedLossMps
            | Self::ClimbRateMps
            | Self::DiveSpeedGainMps
            | Self::BoostSpeedGainMps => "m/s",
            Self::TurnRadiusM => "m",
            Self::RollRateRadps | Self::YawRateRadps => "rad/s",
            Self::StallRecoveryS => "s",
            Self::ControlAuthorityFraction => "fraction",
        }
    }

    /// Looks a quantity up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|quantity| quantity.label() == label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every maneuver and quantity round-trips through its label, and each
    /// maneuver names a quantity whose unit is declared.
    #[test]
    fn accept_f26_a_probe_vocabulary_round_trips_and_units_are_declared() {
        assert_eq!(ProbeKind::ALL.len(), 10);
        assert_eq!(ProbeQuantity::ALL.len(), 10);
        for maneuver in ProbeKind::ALL {
            assert_eq!(ProbeKind::from_label(maneuver.label()), Some(maneuver));
            let quantity = maneuver.quantity();
            assert_eq!(ProbeQuantity::from_label(quantity.label()), Some(quantity));
            assert!(!quantity.unit().is_empty());
        }
        for quantity in ProbeQuantity::ALL {
            assert_eq!(ProbeQuantity::from_label(quantity.label()), Some(quantity));
        }
        assert_eq!(ProbeKind::from_label("helicopter"), None);
        assert_eq!(ProbeKind::from_label("sustained-turn"), None);
        assert_eq!(ProbeQuantity::from_label("knots"), None);
    }

    /// Every maneuver maps to a distinct quantity, so the (maneuver, quantity)
    /// join key is unambiguous.
    #[test]
    fn accept_f26_a_every_maneuver_names_a_distinct_quantity() {
        let mut seen = Vec::new();
        for maneuver in ProbeKind::ALL {
            let quantity = maneuver.quantity();
            assert!(
                !seen.contains(&quantity),
                "{} reuses a quantity others already name",
                maneuver.label()
            );
            seen.push(quantity);
        }
        assert_eq!(seen.len(), ProbeKind::ALL.len());
    }
}
