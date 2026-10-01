//! Capital-ship propulsion: engine records and the thrust sum a destroyed
//! engine removes (F35-A).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Non-negotiable behavior 1 makes an engine's destruction a *movement*
//! change, and the F35-A minimum scenario measures exactly that: engines
//! disabled, motion response changes, the hull is not destroyed. This module
//! is the typed engine record ([`EngineSpec`]) and the pure arithmetic the
//! ship applies — no integration, no tick, no Avian body. The whole runtime
//! is F35-B.
//!
//! A declared thrust is a [`Resolved<f64>`]: an engine whose thrust is not
//! measured stays `Unknown` and [`EngineSpec::known_thrust_n`] refuses to
//! compute rather than substituting a zero. Nothing here is a measured
//! original coefficient.

use cs_types::content::Resolved;

use super::subsystem::SubsystemKey;

/// One engine: a thrust along a body-frame axis.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineSpec {
    /// The subsystem this engine is; it must be an
    /// [`SubsystemKind::Engine`] of the ship's graph.
    pub key: SubsystemKey,
    /// The engine's rated thrust in newtons, or an explicit unknown.
    pub thrust_n: Resolved<f64>,
    /// The unit thrust direction in the ship body frame.
    pub axis: [f64; 3],
}

impl EngineSpec {
    /// Validates and wraps an engine record.
    ///
    /// # Errors
    ///
    /// [`PropulsionError::NonFiniteAxis`] or [`PropulsionError::ZeroAxis`]
    /// for a bad axis, and [`PropulsionError::NonFiniteThrust`] or
    /// [`PropulsionError::NegativeThrust`] for a bad known thrust.
    pub fn try_new(
        key: SubsystemKey,
        thrust_n: Resolved<f64>,
        axis: [f64; 3],
    ) -> Result<Self, PropulsionError> {
        let axis = validated_axis(&key, axis)?;
        if let Resolved::Known(known) = &thrust_n {
            if !known.value.is_finite() {
                return Err(PropulsionError::NonFiniteThrust { key });
            }
            if known.value < 0.0 {
                return Err(PropulsionError::NegativeThrust {
                    key,
                    value: known.value,
                });
            }
        }
        Ok(Self {
            key,
            thrust_n,
            axis,
        })
    }

    /// The engine's known thrust, or a refusal naming the unknown.
    ///
    /// # Errors
    ///
    /// [`PropulsionError::UnknownThrust`] when the thrust is unresolved.
    pub fn known_thrust_n(&self) -> Result<f64, PropulsionError> {
        match &self.thrust_n {
            Resolved::Known(known) => Ok(known.value),
            Resolved::Unknown { .. } => Err(PropulsionError::UnknownThrust {
                key: self.key.clone(),
            }),
        }
    }

    /// The engine's force at `throttle` in `[0, 1]`, in the body frame.
    ///
    /// # Errors
    ///
    /// [`PropulsionError::UnknownThrust`] when the thrust is unresolved and
    /// [`PropulsionError::NonFiniteThrottle`] for a non-finite throttle.
    pub fn force_n(&self, throttle: f64) -> Result<[f64; 3], PropulsionError> {
        if !throttle.is_finite() {
            return Err(PropulsionError::NonFiniteThrottle {
                key: self.key.clone(),
            });
        }
        let thrust = self.known_thrust_n()? * throttle;
        Ok([
            self.axis[0] * thrust,
            self.axis[1] * thrust,
            self.axis[2] * thrust,
        ])
    }
}

/// A refused propulsion computation.
#[derive(Clone, Debug, PartialEq)]
pub enum PropulsionError {
    /// The engine's thrust is unresolved.
    UnknownThrust {
        /// The engine.
        key: SubsystemKey,
    },
    /// The axis was not finite.
    NonFiniteAxis {
        /// The engine.
        key: SubsystemKey,
    },
    /// The axis was the zero vector.
    ZeroAxis {
        /// The engine.
        key: SubsystemKey,
    },
    /// The known thrust was not finite.
    NonFiniteThrust {
        /// The engine.
        key: SubsystemKey,
    },
    /// The known thrust was negative.
    NegativeThrust {
        /// The engine.
        key: SubsystemKey,
        /// The refused value.
        value: f64,
    },
    /// The throttle was not finite.
    NonFiniteThrottle {
        /// The engine.
        key: SubsystemKey,
    },
    /// The mass was not finite.
    NonFiniteMass,
    /// The mass was not positive.
    NonPositiveMass {
        /// The refused mass.
        mass_kg: f64,
    },
    /// The summed force was not finite.
    NonFiniteForce,
}

impl std::fmt::Display for PropulsionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownThrust { key } => {
                write!(f, "engine {key} has an unresolved thrust")
            }
            Self::NonFiniteAxis { key } => write!(f, "engine {key} has a non-finite axis"),
            Self::ZeroAxis { key } => write!(f, "engine {key} has a zero axis"),
            Self::NonFiniteThrust { key } => {
                write!(f, "engine {key} has a non-finite thrust")
            }
            Self::NegativeThrust { key, value } => {
                write!(f, "engine {key} has negative thrust {value}")
            }
            Self::NonFiniteThrottle { key } => {
                write!(f, "engine {key} was commanded a non-finite throttle")
            }
            Self::NonFiniteMass => write!(f, "the ship mass is not finite"),
            Self::NonPositiveMass { mass_kg } => {
                write!(f, "the ship mass {mass_kg} is not positive")
            }
            Self::NonFiniteForce => write!(f, "the summed thrust force is not finite"),
        }
    }
}

impl std::error::Error for PropulsionError {}

/// Normalizes a thrust axis to unit length, refusing non-finite and zero
/// axes so an engine can never point nowhere.
///
/// # Errors
///
/// [`PropulsionError::NonFiniteAxis`] or [`PropulsionError::ZeroAxis`].
pub fn validated_axis(key: &SubsystemKey, axis: [f64; 3]) -> Result<[f64; 3], PropulsionError> {
    if !axis.iter().all(|value| value.is_finite()) {
        return Err(PropulsionError::NonFiniteAxis { key: key.clone() });
    }
    let norm = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if norm <= f64::EPSILON {
        return Err(PropulsionError::ZeroAxis { key: key.clone() });
    }
    Ok([axis[0] / norm, axis[1] / norm, axis[2] / norm])
}

/// The acceleration a summed body-frame force produces for `mass_kg`.
///
/// # Errors
///
/// [`PropulsionError::NonFiniteForce`], [`PropulsionError::NonFiniteMass`]
/// or [`PropulsionError::NonPositiveMass`].
pub fn acceleration_m_s2(force_n: [f64; 3], mass_kg: f64) -> Result<[f64; 3], PropulsionError> {
    if !force_n.iter().all(|value| value.is_finite()) {
        return Err(PropulsionError::NonFiniteForce);
    }
    if !mass_kg.is_finite() {
        return Err(PropulsionError::NonFiniteMass);
    }
    if mass_kg <= 0.0 {
        return Err(PropulsionError::NonPositiveMass { mass_kg });
    }
    Ok([
        force_n[0] / mass_kg,
        force_n[1] / mass_kg,
        force_n[2] / mass_kg,
    ])
}
