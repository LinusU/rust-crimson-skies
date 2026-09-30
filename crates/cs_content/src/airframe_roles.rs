//! Provenance-bearing airframe roles and session-scoped forced launch
//! assignment (F25-A).
//!
//! Spec: `specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! stage `### F25-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! sections "Inputs and outputs" and "Boost and special models".
//!
//! The sheet's deliverable is that exceptional aircraft are identified by a
//! **role**, not by filename conditionals in gameplay, and that "special
//! mission vehicles and oversized craft have explicit controllability, launch
//! and weapon constraints". This module is that role record, plus the one pure
//! function that decides which airframe a session launches:
//!
//! * [`AirframeRole`] says what an airframe *is*: its [`ModelKind`] label, its
//!   [`Availability`] (roster presence and ordinary menu availability are
//!   different things), whether it is pilotable, and its explicit
//!   [`LaunchConstraints`] and [`WeaponConstraints`]. Every role carries an
//!   [`Origin`] and a [`Provenance`], and a numeric value is either known or
//!   an explicit unknown with a reason — never a silent zero
//!   (`IDENTITY-CONTENT`; spec F14 non-negotiable behavior 3).
//! * [`AirframeRoles::resolve_launch`] is AC01's whole mechanism: a mission's
//!   [`ForcedAssignment`] wins over the player's [`OwnedLoadout`] selection
//!   **for that session only**, and the owned loadout is taken by shared
//!   reference so a forced mission cannot write back into what the player owns
//!   (non-negotiable behavior 2). A forced assignment carrying the wrong
//!   session generation, naming an airframe the roster does not contain, or
//!   naming one the role declares unlaunchable or unpilotable is refused by
//!   name — it never silently falls back to the hangar plane.
//! * A [`MissionOnly`] airframe is present and pilotable, so a mission that
//!   requires it can launch it even though the shop does not list it
//!   (non-negotiable behavior 4: "never hide a required model because the
//!   normal shop does not list it").
//!
//! **Designed and synthetic, not original data.** No roster has been read from
//! the owner's installation by this stage: the only declared roles come from
//! [`declared_synthetic_roles`] and carry [`Origin::SyntheticFixture`], and the
//! rotor mapping they need is an explicit *unknown* because no original
//! measurement of it exists. Roster presence, the Hoplite name/prefix and the
//! mission bindings remain source-observed leads with unverified identities
//! (`F25` "Research boundary"; `missions/M17.md`), so nothing here may be read
//! as an original roster, catalog id or control law. Wiring this record into the
//! runtime launch path is F25-C's.

use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};

/// The model-kind labels this crate uses, in the consuming model's vocabulary.
///
/// `cs_content` must not depend on `cs_sim` (`docs/01-ARCHITECTURE.md`), so the
/// label is carried as a string here and mapped into
/// `cs_sim::flight::ModelKind` by the stage that wires the two (F25-C), exactly
/// as `cs_content::flight_tuning` does for F24.
pub const MODEL_KINDS: [&str; 2] = ["fixed_wing", "exceptional"];

/// The model kind that declares itself exceptional, matching
/// `cs_sim::flight::ModelKind::Exceptional::label()`.
pub const EXCEPTIONAL_MODEL_KIND: &str = "exceptional";

/// Whether an airframe in the roster is listed by the ordinary menu.
///
/// "Roster presence and ordinary menu availability are different"
/// (`F25` "Research boundary"), so they are two separate facts: presence is
/// membership of the roster, and this only says whether the shop lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Availability {
    /// The ordinary shop lists this airframe as a purchase or a selection.
    ShopListed,
    /// The airframe exists in the data and is pilotable, but the ordinary shop
    /// does not list it. A mission that requires it may still launch it.
    MissionOnly,
}

impl Availability {
    /// Every declared availability, in a stable order.
    pub const ALL: [Self; 2] = [Self::ShopListed, Self::MissionOnly];

    /// The stable label used in reports and persisted records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ShopListed => "shop_listed",
            Self::MissionOnly => "mission_only",
        }
    }

    /// Whether the ordinary shop lists the airframe.
    #[must_use]
    pub const fn is_shop_listed(self) -> bool {
        matches!(self, Self::ShopListed)
    }
}

/// What a mission may do with an airframe at launch.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchConstraints {
    /// Whether a mission may **assign** this airframe to a session. It gates
    /// the forced path of [`AirframeRoles::resolve_launch`]: a mission that
    /// hands the session an airframe this role forbids cannot launch it. A role
    /// that is not pilotable must set this to `false`.
    ///
    /// It is deliberately not checked on the hangar path, where the player
    /// launches their own plane and [`LaunchConstraints::hangar_selectable`]
    /// decides: a shop-listed airframe no mission assigns stays flyable. Whether
    /// a *mission* session may use the garage plane at all is F25-C's wiring
    /// decision, and this record does not invent a third rule for it.
    pub mission_launchable: bool,
    /// Whether the hangar may select this airframe as the player's own plane.
    /// A [`Availability::MissionOnly`] airframe must declare this `false`, or
    /// an airframe the shop does not list would become a hangar selection.
    /// A shop-listed airframe may still declare it `false` when it is not the
    /// player's plane.
    pub hangar_selectable: bool,
    /// The declared launch airspeed, or an explicit unknown. It is known for a
    /// synthetic fixture and unknown until a mission's spawn data is read.
    pub initial_airspeed_mps: Resolved<f64>,
}

/// What weapons an airframe may carry.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponConstraints {
    /// Whether the airframe may be armed at all.
    pub armed: bool,
    /// The declared hardpoint count, or an explicit unknown.
    pub hardpoint_count: Resolved<u32>,
}

/// The rotor facts an exceptional role must state.
///
/// Only the *explicit mapping* between the authoritative physical rotor rate
/// and the rate the mesh is drawn at is required (non-negotiable behavior 3),
/// and it is deliberately allowed to be an explicit unknown: the numeric
/// consumer then reports **no** visual rate rather than assuming `1.0`.
#[derive(Clone, Debug, PartialEq)]
pub struct RotorRole {
    /// Visual rad/s per physical rad/s, or an explicit unknown.
    pub visual_radps_per_physical_radps: Resolved<f64>,
}

/// Why an [`AirframeRole`] was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum AirframeRoleError {
    /// The id does not name an airframe.
    NotAnAirframe {
        /// The kind the id actually names.
        kind: ContentKind,
    },
    /// The model-kind label is empty.
    EmptyModelKind,
    /// The model-kind label is not one of [`MODEL_KINDS`].
    UnknownModelKind {
        /// The rejected label.
        value: String,
    },
    /// A mission-only role also declared itself hangar-selectable, which would
    /// expose an airframe the shop does not list.
    MissionOnlyButSelectable {
        /// The airframe whose declaration is inconsistent.
        airframe: ContentId,
    },
    /// An exceptional role declared no rotor facts.
    MissingRotorRole,
    /// A fixed-wing role declared rotor facts.
    UnexpectedRotorRole,
    /// A role that is not pilotable also declared itself mission-launchable.
    NotPilotableButLaunchable,
    /// A known `initial_airspeed_mps` was NaN or infinite.
    NonFiniteAirspeed,
    /// A known `initial_airspeed_mps` was negative.
    NegativeAirspeed,
    /// The roster declared the same airframe twice.
    DuplicateAirframe {
        /// The airframe that appeared more than once.
        airframe: ContentId,
    },
}

impl std::fmt::Display for AirframeRoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnAirframe { kind } => {
                write!(f, "an airframe role must reference an airframe, got {kind}")
            }
            Self::EmptyModelKind => write!(f, "an airframe role must state a model kind"),
            Self::UnknownModelKind { value } => write!(
                f,
                "the model kind {value} is not one of {}",
                MODEL_KINDS.join(", ")
            ),
            Self::MissionOnlyButSelectable { airframe } => write!(
                f,
                "{airframe} is declared mission_only, so the hangar must not select it"
            ),
            Self::MissingRotorRole => write!(
                f,
                "an {EXCEPTIONAL_MODEL_KIND} airframe role must declare its rotor facts"
            ),
            Self::UnexpectedRotorRole => {
                write!(f, "a fixed_wing airframe role must not declare rotor facts")
            }
            Self::NotPilotableButLaunchable => {
                write!(
                    f,
                    "an airframe that is not pilotable cannot be mission-launchable"
                )
            }
            Self::NonFiniteAirspeed => {
                write!(
                    f,
                    "launch.initial_airspeed_mps must be finite when it is known"
                )
            }
            Self::NegativeAirspeed => {
                write!(f, "launch.initial_airspeed_mps must not be negative")
            }
            Self::DuplicateAirframe { airframe } => {
                write!(f, "the airframe {airframe} has more than one role")
            }
        }
    }
}

impl std::error::Error for AirframeRoleError {}

/// What one airframe is, as declared by the data.
///
/// The role is the identity gameplay keys on instead of a filename
/// conditional (F25 deliverable), so it separates three facts that used to be
/// conflated: which control law the airframe uses, whether the ordinary menu
/// lists it, and what a mission may do with it.
#[derive(Clone, Debug, PartialEq)]
pub struct AirframeRole {
    /// The airframe element this role describes.
    pub id: ContentId,
    /// The control law's label, one of [`MODEL_KINDS`].
    pub model_kind: String,
    /// Whether the ordinary shop lists the airframe.
    pub availability: Availability,
    /// Whether a session may put the player in this airframe.
    pub pilotable: bool,
    /// The declared launch constraints.
    pub launch: LaunchConstraints,
    /// The declared weapon constraints.
    pub weapons: WeaponConstraints,
    /// The rotor facts, present exactly when `model_kind` is
    /// [`EXCEPTIONAL_MODEL_KIND`].
    pub rotor: Option<RotorRole>,
    /// Where the declaration came from.
    pub origin: Origin,
    /// The claim the declaration backs.
    pub provenance: Provenance,
}

impl AirframeRole {
    /// Whether this role declares the exceptional control law.
    #[must_use]
    pub fn is_exceptional(&self) -> bool {
        self.model_kind == EXCEPTIONAL_MODEL_KIND
    }

    /// Checks the role's identity, the agreement between its declared facts and
    /// its bounds.
    ///
    /// # Errors
    ///
    /// [`AirframeRoleError`] naming the first problem: an id that is not an
    /// airframe, an unknown model-kind label, a mission-only role that also
    /// declares itself hangar-selectable, rotor facts on the wrong model kind,
    /// a non-pilotable role that claims to launch, or a corrupt known launch
    /// airspeed.
    pub fn validate(&self) -> Result<(), AirframeRoleError> {
        if self.id.kind() != ContentKind::Airframe {
            return Err(AirframeRoleError::NotAnAirframe {
                kind: self.id.kind(),
            });
        }
        if self.model_kind.trim().is_empty() {
            return Err(AirframeRoleError::EmptyModelKind);
        }
        if !MODEL_KINDS.contains(&self.model_kind.as_str()) {
            return Err(AirframeRoleError::UnknownModelKind {
                value: self.model_kind.clone(),
            });
        }

        if self.availability == Availability::MissionOnly && self.launch.hangar_selectable {
            return Err(AirframeRoleError::MissionOnlyButSelectable {
                airframe: self.id.clone(),
            });
        }
        if self.is_exceptional() {
            if self.rotor.is_none() {
                return Err(AirframeRoleError::MissingRotorRole);
            }
        } else if self.rotor.is_some() {
            return Err(AirframeRoleError::UnexpectedRotorRole);
        }
        if !self.pilotable && self.launch.mission_launchable {
            return Err(AirframeRoleError::NotPilotableButLaunchable);
        }
        if let Some(airspeed) = self.launch.initial_airspeed_mps.clone().known() {
            if !airspeed.is_finite() {
                return Err(AirframeRoleError::NonFiniteAirspeed);
            }
            if airspeed < 0.0 {
                return Err(AirframeRoleError::NegativeAirspeed);
            }
        }
        Ok(())
    }
}

/// The player's own, persisted hangar selection.
///
/// It is the loadout a mission must not corrupt: a forced mission assignment
/// overrides it for one session only (non-negotiable behavior 2), so the
/// resolver takes this by shared reference and never writes it back.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnedLoadout {
    /// The airframe the player owns and has selected in the hangar.
    pub airframe: ContentId,
    /// The session generation this selection belongs to.
    pub session_generation: u64,
}

/// A mission's forced airframe assignment, valid for exactly one session.
#[derive(Clone, Debug, PartialEq)]
pub struct ForcedAssignment {
    /// The airframe the mission requires.
    pub airframe: ContentId,
    /// The session generation the assignment applies to.
    pub session_generation: u64,
    /// Where the assignment came from.
    pub origin: Origin,
    /// The claim the assignment backs.
    pub provenance: Provenance,
}

/// Which rule produced the airframe a session launches with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LaunchSource {
    /// The player's own hangar selection, with no forced assignment.
    HangarSelection,
    /// A mission's forced assignment, which overrides the hangar selection for
    /// this session only.
    ForcedMissionAssignment,
}

impl LaunchSource {
    /// Every declared source, in a stable order.
    pub const ALL: [Self; 2] = [Self::HangarSelection, Self::ForcedMissionAssignment];

    /// The stable label used in reports and persisted records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::HangarSelection => "hangar_selection",
            Self::ForcedMissionAssignment => "forced_mission_assignment",
        }
    }

    /// Whether this source overrides the player's own hangar selection.
    #[must_use]
    pub const fn is_forced(self) -> bool {
        matches!(self, Self::ForcedMissionAssignment)
    }
}

/// The airframe one session launches with, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedLaunch {
    /// The airframe to spawn.
    pub airframe: ContentId,
    /// Which rule chose it.
    pub source: LaunchSource,
    /// The session generation this launch belongs to.
    pub session_generation: u64,
}

impl ResolvedLaunch {
    /// Whether the launch overrides the player's own hangar selection.
    #[must_use]
    pub const fn is_forced(&self) -> bool {
        self.source.is_forced()
    }

    /// Whether persisting this launch may update the player's owned loadout.
    ///
    /// Only an unforced launch may. A forced mission assignment is scoped to
    /// its session, so a save made during it must not leave the player's hangar
    /// selection pointing at the mission's airframe.
    #[must_use]
    pub const fn persists_to_owned_loadout(&self) -> bool {
        matches!(self.source, LaunchSource::HangarSelection)
    }
}

/// Why a launch assignment was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum LaunchAssignmentError {
    /// The forced assignment belongs to a different session generation, so it
    /// must not apply here ("only for that session").
    SessionGenerationMismatch {
        /// The generation the assignment carries.
        assignment: u64,
        /// The generation of the session being launched.
        session: u64,
    },
    /// The roster contains no role for the airframe, so there is nothing to
    /// spawn and no declared capability to check.
    UnknownAirframe {
        /// The airframe that was asked for.
        airframe: ContentId,
    },
    /// The role declares the airframe unpilotable.
    NotPilotable {
        /// The airframe that was asked for.
        airframe: ContentId,
    },
    /// The role forbids a mission from launching the airframe.
    NotMissionLaunchable {
        /// The airframe that was asked for.
        airframe: ContentId,
    },
    /// The hangar may not select the airframe, so it can only be reached
    /// through a forced assignment.
    NotHangarSelectable {
        /// The airframe that was asked for.
        airframe: ContentId,
    },
    /// A declared role failed its own boundary.
    Role(AirframeRoleError),
}

impl std::fmt::Display for LaunchAssignmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionGenerationMismatch {
                assignment,
                session,
            } => write!(
                f,
                "the forced assignment for session generation {assignment} does not apply to session generation {session}"
            ),
            Self::UnknownAirframe { airframe } => {
                write!(f, "the roster contains no role for {airframe}")
            }
            Self::NotPilotable { airframe } => {
                write!(f, "the airframe {airframe} is not pilotable")
            }
            Self::NotMissionLaunchable { airframe } => {
                write!(f, "no mission may launch the airframe {airframe}")
            }
            Self::NotHangarSelectable { airframe } => {
                write!(f, "the hangar may not select the airframe {airframe}")
            }
            Self::Role(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LaunchAssignmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Role(error) => Some(error),
            Self::SessionGenerationMismatch { .. }
            | Self::UnknownAirframe { .. }
            | Self::NotPilotable { .. }
            | Self::NotMissionLaunchable { .. }
            | Self::NotHangarSelectable { .. } => None,
        }
    }
}

impl From<AirframeRoleError> for LaunchAssignmentError {
    fn from(error: AirframeRoleError) -> Self {
        Self::Role(error)
    }
}

/// The roster's airframe roles, and the launch resolution they govern.
#[derive(Clone, Debug, PartialEq)]
pub struct AirframeRoles {
    roles: Vec<AirframeRole>,
}

impl AirframeRoles {
    /// Builds the roster, validating every role and refusing a duplicate
    /// airframe.
    ///
    /// # Errors
    ///
    /// [`AirframeRoleError`] for the first role that fails its own boundary, or
    /// [`AirframeRoleError::DuplicateAirframe`].
    pub fn new(roles: Vec<AirframeRole>) -> Result<Self, AirframeRoleError> {
        for (index, role) in roles.iter().enumerate() {
            role.validate()?;
            if roles[..index].iter().any(|earlier| earlier.id == role.id) {
                return Err(AirframeRoleError::DuplicateAirframe {
                    airframe: role.id.clone(),
                });
            }
        }
        Ok(Self { roles })
    }

    /// Every declared role, in declaration order.
    #[must_use]
    pub fn roles(&self) -> &[AirframeRole] {
        &self.roles
    }

    /// The role for `airframe`, or `None` when the roster does not contain it.
    #[must_use]
    pub fn role(&self, airframe: &ContentId) -> Option<&AirframeRole> {
        self.roles.iter().find(|role| &role.id == airframe)
    }

    /// The airframe one session launches with.
    ///
    /// With `forced = Some(assignment)` the assignment wins over the player's
    /// hangar selection for this session only: the returned airframe is the
    /// assignment's even when it differs from `owned.airframe`, and `owned` is
    /// borrowed, never written. A `SessionGenerationMismatch` is refused rather
    /// than applied, because an assignment scoped to another session must not
    /// decide this one.
    ///
    /// With `forced = None` the hangar selection must name a role that is pilotable
    /// and hangar-selectable, otherwise the player's own plane could not be
    /// launched; [`LaunchConstraints::mission_launchable`] is not consulted
    /// there, because the garage plane is the player's own plane rather than a
    /// mission's assignment.
    ///
    /// # Errors
    ///
    /// [`LaunchAssignmentError`] naming the first refusal: a generation
    /// mismatch, an airframe the roster does not contain, a role that is
    /// unpilotable, a role no session may launch, or a hangar selection the
    /// hangar may not make.
    pub fn resolve_launch(
        &self,
        owned: &OwnedLoadout,
        forced: Option<&ForcedAssignment>,
    ) -> Result<ResolvedLaunch, LaunchAssignmentError> {
        let Some(assignment) = forced else {
            let role = self.role(&owned.airframe).ok_or_else(|| {
                LaunchAssignmentError::UnknownAirframe {
                    airframe: owned.airframe.clone(),
                }
            })?;
            if !role.pilotable {
                return Err(LaunchAssignmentError::NotPilotable {
                    airframe: owned.airframe.clone(),
                });
            }
            if !role.launch.hangar_selectable {
                return Err(LaunchAssignmentError::NotHangarSelectable {
                    airframe: owned.airframe.clone(),
                });
            }
            return Ok(ResolvedLaunch {
                airframe: owned.airframe.clone(),
                source: LaunchSource::HangarSelection,
                session_generation: owned.session_generation,
            });
        };

        if assignment.session_generation != owned.session_generation {
            return Err(LaunchAssignmentError::SessionGenerationMismatch {
                assignment: assignment.session_generation,
                session: owned.session_generation,
            });
        }
        let role = self.role(&assignment.airframe).ok_or_else(|| {
            LaunchAssignmentError::UnknownAirframe {
                airframe: assignment.airframe.clone(),
            }
        })?;
        if !role.pilotable {
            return Err(LaunchAssignmentError::NotPilotable {
                airframe: assignment.airframe.clone(),
            });
        }
        if !role.launch.mission_launchable {
            return Err(LaunchAssignmentError::NotMissionLaunchable {
                airframe: assignment.airframe.clone(),
            });
        }
        Ok(ResolvedLaunch {
            airframe: assignment.airframe.clone(),
            source: LaunchSource::ForcedMissionAssignment,
            session_generation: owned.session_generation,
        })
    }
}

/// The synthetic roster F25-A declares: one shop-listed fixed wing and one
/// mission-only exceptional airframe.
///
/// Both are [`Origin::SyntheticFixture`] development data with no original
/// counterpart read by this stage, and the exceptional role's rotor mapping is
/// an explicit **unknown** — no original measurement of a visual/physical rotor
/// ratio exists — so a consumer built from it reports no visual rotor rate at
/// all instead of assuming one. The two roles exist to make the distinctions the
/// sheet asks for checkable: roster presence versus menu availability, and a
/// mission-only airframe the shop does not list.
#[must_use]
pub fn declared_synthetic_roles() -> AirframeRoles {
    AirframeRoles::new(vec![synthetic_fixed_wing_role(), synthetic_autogyro_role()])
        .expect("the declared synthetic roles are valid")
}

/// The synthetic shop-listed fixed-wing role, keyed to the F24 synthetic
/// fixture's cruise airspeed.
#[must_use]
pub fn synthetic_fixed_wing_role() -> AirframeRole {
    AirframeRole {
        id: ContentId::from_source(ContentKind::Airframe, "fixture.synthetic-fixed-wing")
            .expect("the synthetic airframe id is valid"),
        model_kind: "fixed_wing".to_owned(),
        availability: Availability::ShopListed,
        pilotable: true,
        launch: LaunchConstraints {
            mission_launchable: true,
            hangar_selectable: true,
            initial_airspeed_mps: Resolved::unknown(
                claim("f25a.role.synthetic-fixed-wing.launch-airspeed"),
                "no original spawn data was read for the synthetic fixture",
            )
            .expect("a reason is present"),
        },
        weapons: WeaponConstraints {
            armed: true,
            hardpoint_count: Resolved::unknown(
                claim("f25a.role.synthetic-fixed-wing.hardpoints"),
                "no original loadout data was read for the synthetic fixture",
            )
            .expect("a reason is present"),
        },
        rotor: None,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim("f25a.role.synthetic-fixed-wing")),
    }
}

/// The synthetic mission-only exceptional role.
///
/// It is present in the roster and pilotable, so a mission may launch it, but
/// the shop does not list it and the hangar cannot select it. Nothing about its
/// *handling* is declared: the exact exceptional control law is measurement-
/// dependent (`F25` "Research boundary") and F25-B's, so this role states
/// capability, not dynamics — and it declares no hover.
#[must_use]
pub fn synthetic_autogyro_role() -> AirframeRole {
    AirframeRole {
        id: ContentId::from_source(ContentKind::Airframe, "fixture.synthetic-autogyro")
            .expect("the synthetic airframe id is valid"),
        model_kind: EXCEPTIONAL_MODEL_KIND.to_owned(),
        availability: Availability::MissionOnly,
        pilotable: true,
        launch: LaunchConstraints {
            mission_launchable: true,
            hangar_selectable: false,
            initial_airspeed_mps: Resolved::unknown(
                claim("f25a.role.synthetic-autogyro.launch-airspeed"),
                "no original spawn data was read for the synthetic fixture",
            )
            .expect("a reason is present"),
        },
        weapons: WeaponConstraints {
            armed: false,
            hardpoint_count: Resolved::unknown(
                claim("f25a.role.synthetic-autogyro.hardpoints"),
                "no original loadout data was read for the synthetic fixture",
            )
            .expect("a reason is present"),
        },
        rotor: Some(RotorRole {
            visual_radps_per_physical_radps: Resolved::unknown(
                claim("f25a.role.synthetic-autogyro.rotor-ratio"),
                "no original measurement of a visual/physical rotor ratio exists",
            )
            .expect("a reason is present"),
        }),
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim("f25a.role.synthetic-autogyro")),
    }
}

fn claim(id: &str) -> cs_types::evidence::ClaimId {
    cs_types::evidence::ClaimId::new(id).expect("the declared claim id is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned_loadout(airframe: &str, generation: u64) -> OwnedLoadout {
        OwnedLoadout {
            airframe: ContentId::from_source(ContentKind::Airframe, airframe)
                .expect("a valid airframe id"),
            session_generation: generation,
        }
    }

    fn forced_assignment(airframe: &str, generation: u64) -> ForcedAssignment {
        ForcedAssignment {
            airframe: ContentId::from_source(ContentKind::Airframe, airframe)
                .expect("a valid airframe id"),
            session_generation: generation,
            origin: Origin::SyntheticFixture,
            provenance: Provenance::designed(claim("f25a.test.forced")),
        }
    }

    /// The declared roster is valid, separates menu availability from roster
    /// presence, and states no original rotor ratio.
    #[test]
    fn accept_f25_a_declared_roles_separate_availability_from_presence() {
        let roster = declared_synthetic_roles();
        assert_eq!(roster.roles().len(), 2);

        let fixed = roster
            .role(&owned_loadout("fixture.synthetic-fixed-wing", 0).airframe)
            .expect("the fixed-wing role is in the roster");
        assert_eq!(fixed.validate(), Ok(()));
        assert!(!fixed.is_exceptional());
        assert!(fixed.availability.is_shop_listed());
        assert!(fixed.launch.hangar_selectable);
        assert_eq!(fixed.origin, Origin::SyntheticFixture);
        assert!(!fixed.origin.is_original());

        let autogyro = roster
            .role(&owned_loadout("fixture.synthetic-autogyro", 0).airframe)
            .expect("the autogyro role is in the roster");
        assert_eq!(autogyro.validate(), Ok(()));
        assert!(autogyro.is_exceptional());
        assert_eq!(autogyro.availability, Availability::MissionOnly);
        assert!(
            !autogyro.launch.hangar_selectable,
            "a mission-only airframe is never hangar-selectable"
        );
        assert!(
            autogyro.launch.mission_launchable,
            "a mission-only airframe must still be launchable"
        );
        assert!(
            autogyro.pilotable,
            "non-negotiable behavior 4: a mission-only model is pilotable"
        );
        let ratio = autogyro
            .rotor
            .as_ref()
            .expect("an exceptional role declares its rotor facts")
            .visual_radps_per_physical_radps
            .clone()
            .known();
        assert_eq!(
            ratio, None,
            "no original rotor ratio was measured, so it stays unknown"
        );
        assert!(!autogyro.weapons.armed);
    }

    /// The role boundary refuses an inconsistent declaration by name instead of
    /// reconciling it: a mission-only role that is also hangar-selectable, rotor
    /// facts on the wrong kind, an invented model kind, and a duplicate
    /// airframe. A shop-listed plane that no mission may launch is *not* a
    /// contradiction: it stays a valid hangar-only airframe.
    #[test]
    fn accept_f25_a_role_boundary_refuses_inconsistent_declarations() {
        let mut mismatch = synthetic_autogyro_role();
        mismatch.launch.hangar_selectable = true;
        assert_eq!(
            mismatch.validate(),
            Err(AirframeRoleError::MissionOnlyButSelectable {
                airframe: mismatch.id.clone()
            })
        );

        let mut hangar_only = synthetic_fixed_wing_role();
        hangar_only.launch.mission_launchable = false;
        assert_eq!(
            hangar_only.validate(),
            Ok(()),
            "a shop-listed plane no mission may launch stays a hangar airframe"
        );

        let mut no_rotor = synthetic_autogyro_role();
        no_rotor.rotor = None;
        assert_eq!(
            no_rotor.validate(),
            Err(AirframeRoleError::MissingRotorRole)
        );

        let mut wing_rotor = synthetic_fixed_wing_role();
        wing_rotor.rotor = Some(RotorRole {
            visual_radps_per_physical_radps: Resolved::unknown(
                claim("f25a.test.wing-rotor"),
                "not applicable",
            )
            .expect("a reason is present"),
        });
        assert_eq!(
            wing_rotor.validate(),
            Err(AirframeRoleError::UnexpectedRotorRole)
        );

        let mut unknown_kind = synthetic_fixed_wing_role();
        unknown_kind.model_kind = "helicopter".to_owned();
        assert_eq!(
            unknown_kind.validate(),
            Err(AirframeRoleError::UnknownModelKind {
                value: "helicopter".to_owned()
            }),
            "the word helicopter is not a model kind this project declares"
        );

        let mut not_a_plane = synthetic_fixed_wing_role();
        not_a_plane.id = ContentId::from_source(ContentKind::Mesh, "fixture.synthetic-fixed-wing")
            .expect("a valid id");
        assert_eq!(
            not_a_plane.validate(),
            Err(AirframeRoleError::NotAnAirframe {
                kind: ContentKind::Mesh
            })
        );

        let mut unpilotable = synthetic_fixed_wing_role();
        unpilotable.pilotable = false;
        assert_eq!(
            unpilotable.validate(),
            Err(AirframeRoleError::NotPilotableButLaunchable)
        );

        let mut corrupt = synthetic_fixed_wing_role();
        corrupt.launch.initial_airspeed_mps = Resolved::Known(cs_types::content::Known::new(
            -5.0,
            Provenance::designed(claim("f25a.test.negative-airspeed")),
        ));
        assert_eq!(corrupt.validate(), Err(AirframeRoleError::NegativeAirspeed));

        assert_eq!(
            AirframeRoles::new(vec![
                synthetic_fixed_wing_role(),
                synthetic_fixed_wing_role()
            ])
            .err(),
            Some(AirframeRoleError::DuplicateAirframe {
                airframe: ContentId::from_source(
                    ContentKind::Airframe,
                    "fixture.synthetic-fixed-wing"
                )
                .expect("a valid airframe id"),
            })
        );
    }

    /// AC01: a forced mission assignment launches the requested actor even when
    /// the player's hangar selection names a different, shop-listed plane.
    #[test]
    fn accept_f25_a_forced_launch_uses_the_requested_actor_not_the_garage_plane() {
        let roster = declared_synthetic_roles();
        let owned = owned_loadout("fixture.synthetic-fixed-wing", 41);
        let assignment = forced_assignment("fixture.synthetic-autogyro", 41);

        let resolved = roster
            .resolve_launch(&owned, Some(&assignment))
            .expect("a mission-only forced airframe is launchable");

        assert_eq!(resolved.airframe, assignment.airframe);
        assert_ne!(
            resolved.airframe, owned.airframe,
            "the forced actor wins over the garage plane"
        );
        assert_eq!(resolved.source, LaunchSource::ForcedMissionAssignment);
        assert!(resolved.is_forced());
        assert_eq!(resolved.session_generation, 41);
    }

    /// Non-negotiable behavior 2: the forced assignment is scoped to its
    /// session. The owned loadout is borrowed, so it still names the garage
    /// plane afterwards, resolving again gives the same answer (nothing
    /// accumulates), and the result may not be persisted into the owned loadout.
    #[test]
    fn accept_f25_a_forced_launch_does_not_corrupt_the_owned_loadout() {
        let roster = declared_synthetic_roles();
        let owned = owned_loadout("fixture.synthetic-fixed-wing", 41);
        let before = owned.clone();
        let assignment = forced_assignment("fixture.synthetic-autogyro", 41);

        let first = roster
            .resolve_launch(&owned, Some(&assignment))
            .expect("the forced launch resolves");
        let second = roster
            .resolve_launch(&owned, Some(&assignment))
            .expect("the forced launch resolves again");
        assert_eq!(first, second, "resolution is a pure function");
        assert_eq!(
            owned, before,
            "the owned loadout is untouched by a forced launch"
        );
        assert!(
            !first.persists_to_owned_loadout(),
            "a forced mission assignment must not become the saved hangar plane"
        );

        // Without an assignment the hangar plane launches and may be persisted.
        let unforced = roster
            .resolve_launch(&owned, None)
            .expect("the hangar selection resolves");
        assert_eq!(unforced.airframe, owned.airframe);
        assert_eq!(unforced.source, LaunchSource::HangarSelection);
        assert!(!unforced.is_forced());
        assert!(unforced.persists_to_owned_loadout());
        assert_eq!(owned, before);

        // After the session ends the hangar plane is what launches again.
        let next = roster
            .resolve_launch(&owned, None)
            .expect("the next session uses the hangar plane");
        assert_eq!(next.airframe, owned.airframe);
    }

    /// The refusals are named and never degrade into a silent fallback to the
    /// hangar plane: a stale session generation, an airframe the roster does not
    /// contain, and a role that declares the airframe unlaunchable.
    #[test]
    fn accept_f25_a_forced_launch_failures_are_refused_not_fallen_back() {
        let roster = declared_synthetic_roles();
        let owned = owned_loadout("fixture.synthetic-fixed-wing", 41);

        let stale = forced_assignment("fixture.synthetic-autogyro", 40);
        assert_eq!(
            roster.resolve_launch(&owned, Some(&stale)).err(),
            Some(LaunchAssignmentError::SessionGenerationMismatch {
                assignment: 40,
                session: 41,
            })
        );

        let unknown = forced_assignment("fixture.synthetic-helicopter", 41);
        assert_eq!(
            roster.resolve_launch(&owned, Some(&unknown)).err(),
            Some(LaunchAssignmentError::UnknownAirframe {
                airframe: unknown.airframe.clone()
            })
        );

        // A role the hangar may not select is still reachable through a forced
        // assignment, but not through the hangar itself.
        let mission_only = owned_loadout("fixture.synthetic-autogyro", 41);
        assert_eq!(
            roster.resolve_launch(&mission_only, None).err(),
            Some(LaunchAssignmentError::NotHangarSelectable {
                airframe: mission_only.airframe.clone()
            })
        );

        let mut scoped = synthetic_autogyro_role();
        scoped.launch.mission_launchable = false;
        scoped.launch.hangar_selectable = false;
        let roster = AirframeRoles::new(vec![scoped]).expect("the scoped role is valid");
        let assignment = forced_assignment("fixture.synthetic-autogyro", 41);
        assert_eq!(
            roster.resolve_launch(&owned, Some(&assignment)).err(),
            Some(LaunchAssignmentError::NotMissionLaunchable {
                airframe: assignment.airframe.clone()
            })
        );

        let mut unpilotable = synthetic_fixed_wing_role();
        unpilotable.pilotable = false;
        unpilotable.launch.mission_launchable = false;
        let roster = AirframeRoles::new(vec![unpilotable]).expect("the unpilotable role is valid");
        let assignment = forced_assignment("fixture.synthetic-fixed-wing", 41);
        assert_eq!(
            roster.resolve_launch(&owned, Some(&assignment)).err(),
            Some(LaunchAssignmentError::NotPilotable {
                airframe: assignment.airframe.clone()
            })
        );
        assert_eq!(
            roster.resolve_launch(&owned, None).err(),
            Some(LaunchAssignmentError::NotPilotable {
                airframe: owned.airframe.clone()
            })
        );
    }
}
