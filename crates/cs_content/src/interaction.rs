//! The declared interaction schema: provenance-carrying docking, pickup,
//! boarding and plane-swap records (F36-A).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module is the **content half** of the interaction contract — the
//! normalized record a content importer produces and the catalog consumes.
//! Its runtime counterpart is `cs_sim::interaction`; the lowering boundary
//! between them is `cs_app::interaction`. This crate cannot depend on
//! `cs_sim`, so the declared record keeps its own typed vocabulary — the four
//! interaction kinds, the completion events, the eligibility envelope and the
//! transfer policy — and the boundary maps it field-wise.
//!
//! Every load-bearing value is a [`Resolved`], so an unmeasured envelope
//! value or authorization stays an explicit unknown with its claim id and
//! reason instead of a silent default (F14 non-negotiable behavior 3).
//! Nothing here is measured original data.

use std::fmt;

use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};

/// The four interactions the schema declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredInteractionKind {
    /// Docking onto a moving hook.
    Docking,
    /// Collecting passengers or cargo.
    PassengerPickup,
    /// Boarding a carrier.
    Boarding,
    /// Changing the player's aircraft.
    AircraftSwap,
}

impl DeclaredInteractionKind {
    /// Every kind, in a stable order.
    pub const ALL: &'static [DeclaredInteractionKind] = &[
        Self::Docking,
        Self::PassengerPickup,
        Self::Boarding,
        Self::AircraftSwap,
    ];

    /// The stable label used in catalog keys and reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Docking => "docking",
            Self::PassengerPickup => "passenger_pickup",
            Self::Boarding => "boarding",
            Self::AircraftSwap => "aircraft_swap",
        }
    }

    /// Looks a kind up by its label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.label() == label)
    }

    /// The completion event this kind requires.
    #[must_use]
    pub const fn completion(self) -> DeclaredInteractionCompletion {
        match self {
            Self::Docking => DeclaredInteractionCompletion::Docked,
            Self::PassengerPickup => DeclaredInteractionCompletion::PassengersDelivered,
            Self::Boarding => DeclaredInteractionCompletion::Boarded,
            Self::AircraftSwap => DeclaredInteractionCompletion::AircraftSwapped,
        }
    }
}

impl fmt::Display for DeclaredInteractionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The semantic completion event a declared interaction requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredInteractionCompletion {
    /// A docking completed.
    Docked,
    /// Passengers or cargo were delivered.
    PassengersDelivered,
    /// A boarding completed.
    Boarded,
    /// The player swapped aircraft.
    AircraftSwapped,
}

impl DeclaredInteractionCompletion {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Docked => "docked",
            Self::PassengersDelivered => "passengers_delivered",
            Self::Boarded => "boarded",
            Self::AircraftSwapped => "aircraft_swapped",
        }
    }
}

impl fmt::Display for DeclaredInteractionCompletion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Declared velocity handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredVelocityTransfer {
    /// Take the target's velocity.
    MatchTarget,
    /// Keep the initiator's velocity.
    PreserveInitiator,
}

/// Declared pilot handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredPilotTransfer {
    /// Move the pilot to the target.
    MoveToTarget,
    /// No pilot changes.
    None,
}

/// Declared inventory handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredInventoryTransfer {
    /// Move inventory to the target.
    MoveToTarget,
    /// No inventory moves.
    None,
}

/// Declared camera handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredCameraTransfer {
    /// Follow the target.
    FollowTarget,
    /// Follow the initiator.
    FollowInitiator,
}

/// The declared control owner after release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredControlOwner {
    /// The initiator keeps control.
    Initiator,
    /// The target owns control.
    Target,
    /// A dedicated latch controller owns both until release.
    LatchController,
}

/// The declared eligibility envelope, every value resolvable.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredEligibility {
    /// The capture radius, in metres.
    pub capture_radius_m: Resolved<f64>,
    /// The largest relative speed, in m/s.
    pub max_relative_speed_m_s: Resolved<f64>,
    /// The smallest closing speed, in m/s.
    pub min_closing_speed_m_s: Resolved<f64>,
    /// The largest approach angle, in degrees.
    pub max_approach_angle_deg: Resolved<f64>,
    /// The unit approach axis in the target anchor's local frame.
    pub approach_axis_local: Resolved<[f64; 3]>,
}

/// Why a declared eligibility envelope was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum EligibilitySchemaError {
    /// A known field was NaN or infinite.
    NonFinite {
        /// Which field.
        field: &'static str,
    },
    /// The capture radius was not strictly positive.
    NonPositiveRadius {
        /// The rejected value.
        value: f64,
    },
    /// A speed bound was negative.
    NegativeSpeed {
        /// Which field.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// The approach angle was outside `[0, 180]`.
    AngleOutOfRange {
        /// The rejected value.
        value: f64,
    },
    /// The approach axis was zero length.
    ZeroAxis,
}

impl fmt::Display for EligibilitySchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositiveRadius { value } => {
                write!(f, "capture radius {value} must be greater than zero")
            }
            Self::NegativeSpeed { field, value } => {
                write!(f, "{field} {value} must not be negative")
            }
            Self::AngleOutOfRange { value } => {
                write!(f, "approach angle {value} must be within [0, 180] degrees")
            }
            Self::ZeroAxis => write!(f, "the approach axis must have a non-zero direction"),
        }
    }
}

impl std::error::Error for EligibilitySchemaError {}

impl DeclaredEligibility {
    /// Validates the known values in the envelope; unknowns stay unknown.
    ///
    /// # Errors
    ///
    /// [`EligibilitySchemaError`] naming the first invalid known field.
    pub fn validate(&self) -> Result<(), EligibilitySchemaError> {
        validate_finite(&self.capture_radius_m, "capture_radius_m")?;
        if let Resolved::Known(known) = &self.capture_radius_m
            && known.value <= 0.0
        {
            return Err(EligibilitySchemaError::NonPositiveRadius { value: known.value });
        }
        validate_speed(&self.max_relative_speed_m_s, "max_relative_speed_m_s")?;
        validate_speed(&self.min_closing_speed_m_s, "min_closing_speed_m_s")?;
        validate_finite(&self.max_approach_angle_deg, "max_approach_angle_deg")?;
        if let Resolved::Known(known) = &self.max_approach_angle_deg
            && !(0.0..=180.0).contains(&known.value)
        {
            return Err(EligibilitySchemaError::AngleOutOfRange { value: known.value });
        }
        if let Resolved::Known(known) = &self.approach_axis_local {
            if !known.value.iter().all(|v| v.is_finite()) {
                return Err(EligibilitySchemaError::NonFinite {
                    field: "approach_axis_local",
                });
            }
            let length = known.value.iter().map(|v| v * v).sum::<f64>().sqrt();
            if length <= 0.0 {
                return Err(EligibilitySchemaError::ZeroAxis);
            }
        }
        Ok(())
    }
}

fn validate_finite(
    value: &Resolved<f64>,
    field: &'static str,
) -> Result<(), EligibilitySchemaError> {
    if let Resolved::Known(known) = value
        && !known.value.is_finite()
    {
        return Err(EligibilitySchemaError::NonFinite { field });
    }
    Ok(())
}

fn validate_speed(
    value: &Resolved<f64>,
    field: &'static str,
) -> Result<(), EligibilitySchemaError> {
    validate_finite(value, field)?;
    if let Resolved::Known(known) = value
        && known.value < 0.0
    {
        return Err(EligibilitySchemaError::NegativeSpeed {
            field,
            value: known.value,
        });
    }
    Ok(())
}

/// The declared per-transition transfer policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeclaredTransferPolicy {
    /// Velocity handling.
    pub velocity: DeclaredVelocityTransfer,
    /// Pilot handling.
    pub pilot: DeclaredPilotTransfer,
    /// Inventory handling.
    pub inventory: DeclaredInventoryTransfer,
    /// Camera handling.
    pub camera: DeclaredCameraTransfer,
    /// Control owner after release.
    pub control_after_release: DeclaredControlOwner,
}

/// Why a declared interaction was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum InteractionSchemaError {
    /// The eligibility envelope was invalid.
    Eligibility(EligibilitySchemaError),
    /// The authorization named content that is not an objective.
    AuthorizationNotObjective {
        /// The offending id.
        id: ContentId,
    },
}

impl fmt::Display for InteractionSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eligibility(source) => write!(f, "invalid eligibility envelope: {source}"),
            Self::AuthorizationNotObjective { id } => {
                write!(f, "authorization {id} is not an objective")
            }
        }
    }
}

impl std::error::Error for InteractionSchemaError {}

/// One declared interaction: a catalog subject, where it came from, its kind,
/// the objective that authorizes it, its eligibility envelope and its
/// transfer policy.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredInteraction {
    subject: ContentId,
    origin: Origin,
    provenance: Provenance,
    kind: DeclaredInteractionKind,
    authorization: Resolved<ContentId>,
    envelope: DeclaredEligibility,
    transfer: DeclaredTransferPolicy,
}

impl DeclaredInteraction {
    /// Builds a declared interaction, validating its envelope and its known
    /// authorization.
    ///
    /// # Errors
    ///
    /// [`InteractionSchemaError`] for an invalid envelope or a known
    /// authorization that is not an objective.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        provenance: Provenance,
        kind: DeclaredInteractionKind,
        authorization: Resolved<ContentId>,
        envelope: DeclaredEligibility,
        transfer: DeclaredTransferPolicy,
    ) -> Result<Self, InteractionSchemaError> {
        envelope
            .validate()
            .map_err(InteractionSchemaError::Eligibility)?;
        if let Resolved::Known(known) = &authorization
            && known.value.kind() != ContentKind::Objective
        {
            return Err(InteractionSchemaError::AuthorizationNotObjective {
                id: known.value.clone(),
            });
        }
        Ok(Self {
            subject,
            origin,
            provenance,
            kind,
            authorization,
            envelope,
            transfer,
        })
    }

    /// The catalog subject.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The record's provenance.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The interaction kind.
    #[must_use]
    pub const fn kind(&self) -> DeclaredInteractionKind {
        self.kind
    }

    /// The authorizing objective, or an explicit unknown.
    #[must_use]
    pub const fn authorization(&self) -> &Resolved<ContentId> {
        &self.authorization
    }

    /// The eligibility envelope.
    #[must_use]
    pub const fn envelope(&self) -> &DeclaredEligibility {
        &self.envelope
    }

    /// The transfer policy.
    #[must_use]
    pub const fn transfer(&self) -> &DeclaredTransferPolicy {
        &self.transfer
    }

    /// The completion event the kind requires.
    #[must_use]
    pub const fn completion(&self) -> DeclaredInteractionCompletion {
        self.kind.completion()
    }
}

/// The designed synthetic docking interaction: a moving hook with a 5 m
/// capture radius and a +X approach axis.
#[must_use]
pub fn declared_synthetic_interaction() -> DeclaredInteraction {
    let provenance = Provenance::designed(
        cs_types::evidence::ClaimId::new("f36a.synthetic").expect("the claim id is valid"),
    );
    let designed =
        |value: f64| Resolved::Known(cs_types::content::Known::new(value, provenance.clone()));
    let designed_axis =
        |value: [f64; 3]| Resolved::Known(cs_types::content::Known::new(value, provenance.clone()));
    let objective = Resolved::Known(cs_types::content::Known::new(
        ContentId::from_source(ContentKind::Objective, "m01.dock")
            .expect("the synthetic objective id is valid"),
        provenance.clone(),
    ));
    DeclaredInteraction::try_new(
        ContentId::from_source(ContentKind::Mission, "synthetic.interaction")
            .expect("the synthetic subject id is valid"),
        Origin::SyntheticFixture,
        provenance.clone(),
        DeclaredInteractionKind::Docking,
        objective,
        DeclaredEligibility {
            capture_radius_m: designed(5.0),
            max_relative_speed_m_s: designed(10.0),
            min_closing_speed_m_s: designed(0.0),
            max_approach_angle_deg: designed(35.0),
            approach_axis_local: designed_axis([1.0, 0.0, 0.0]),
        },
        DeclaredTransferPolicy {
            velocity: DeclaredVelocityTransfer::MatchTarget,
            pilot: DeclaredPilotTransfer::None,
            inventory: DeclaredInventoryTransfer::None,
            camera: DeclaredCameraTransfer::FollowInitiator,
            control_after_release: DeclaredControlOwner::Target,
        },
    )
    .expect("the synthetic interaction is valid")
}
