//! Acceptance scenario F25-A for the airframe role record and the forced
//! mission launch.
//!
//! Spec: `specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! stage `### F25-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//! Task test prefix: `accept_f25_a_`.
//!
//! The minimum scenario is AC01: a forced-airframe launch uses the requested
//! actor despite a different selected garage plane. These tests use only
//! [`cs_content`]'s public API, so they fail to compile if the role record or
//! the resolver is removed, and they fail at run time if the resolution ever
//! falls back to the hangar plane or writes to the owned loadout.

use cs_content::airframe_roles::{
    AirframeRole, AirframeRoleError, AirframeRoles, Availability, ForcedAssignment,
    LaunchAssignmentError, LaunchSource, OwnedLoadout, declared_synthetic_roles,
    synthetic_autogyro_role, synthetic_fixed_wing_role,
};
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const FIXED_WING: &str = "fixture.synthetic-fixed-wing";
const AUTOGYRO: &str = "fixture.synthetic-autogyro";

fn airframe(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Airframe, key).expect("a valid airframe id")
}

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a valid claim id")
}

fn garage_plane(generation: u64) -> OwnedLoadout {
    OwnedLoadout {
        airframe: airframe(FIXED_WING),
        session_generation: generation,
    }
}

fn forced(key: &str, generation: u64) -> ForcedAssignment {
    ForcedAssignment {
        airframe: airframe(key),
        session_generation: generation,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim("f25a.it.forced")),
    }
}

/// AC01: the mission's requested actor launches even though the player has a
/// different, shop-listed plane selected in the hangar.
#[test]
fn accept_f25_a_forced_launch_uses_the_requested_actor_not_the_garage_plane() {
    let roster = declared_synthetic_roles();
    let garage = garage_plane(41);
    let assignment = forced(AUTOGYRO, 41);

    let resolved = roster
        .resolve_launch(&garage, Some(&assignment))
        .expect("a mission may launch a mission-only exceptional airframe");

    assert_eq!(resolved.airframe, airframe(AUTOGYRO));
    assert_ne!(resolved.airframe, garage.airframe);
    assert_eq!(resolved.source, LaunchSource::ForcedMissionAssignment);
    assert!(resolved.is_forced());
    assert_eq!(resolved.session_generation, 41);

    // The forced actor is reachable *only* through the assignment: the shop does
    // not list it and the hangar cannot select it.
    let role = roster
        .role(&airframe(AUTOGYRO))
        .expect("the roster contains the mission-only airframe");
    assert_eq!(role.availability, Availability::MissionOnly);
    assert!(!role.availability.is_shop_listed());
    assert!(!role.launch.hangar_selectable);
    assert!(role.launch.mission_launchable);
    assert!(role.pilotable);
}

/// A forced assignment overrides the hangar selection for its session and does
/// not corrupt the player's owned loadout.
#[test]
fn accept_f25_a_forced_launch_does_not_corrupt_the_owned_loadout() {
    let roster = declared_synthetic_roles();
    let garage = garage_plane(41);
    let before = garage.clone();
    let assignment = forced(AUTOGYRO, 41);

    let forced_launch = roster
        .resolve_launch(&garage, Some(&assignment))
        .expect("the forced launch resolves");
    assert!(
        !forced_launch.persists_to_owned_loadout(),
        "a save made during a forced mission must not adopt the mission's airframe"
    );
    assert_eq!(
        garage, before,
        "the owned loadout still names the player's own plane"
    );

    // Resolution is a pure function of its inputs: repeating it changes nothing
    // and accumulates nothing into the owned loadout.
    let again = roster
        .resolve_launch(&garage, Some(&assignment))
        .expect("the forced launch resolves again");
    assert_eq!(again, forced_launch);
    assert_eq!(garage, before);

    // The next session without an assignment launches the garage plane again,
    // and only that launch may be persisted.
    let next = roster
        .resolve_launch(&garage, None)
        .expect("the hangar plane resolves");
    assert_eq!(next.airframe, garage.airframe);
    assert_eq!(next.source, LaunchSource::HangarSelection);
    assert!(!next.is_forced());
    assert!(next.persists_to_owned_loadout());
    assert_eq!(garage, before);
}

/// Every failure of a forced assignment is named and refuses to fall back to the
/// hangar plane: a stale session generation, an airframe the roster does not
/// contain, an unpilotable role and a role no mission may launch.
#[test]
fn accept_f25_a_forced_launch_failures_never_fall_back_to_the_garage_plane() {
    let roster = declared_synthetic_roles();
    let garage = garage_plane(41);

    let stale = forced(AUTOGYRO, 40);
    assert_eq!(
        roster.resolve_launch(&garage, Some(&stale)).err(),
        Some(LaunchAssignmentError::SessionGenerationMismatch {
            assignment: 40,
            session: 41,
        })
    );

    let unknown = forced("fixture.synthetic-unknown-model", 41);
    assert_eq!(
        roster.resolve_launch(&garage, Some(&unknown)).err(),
        Some(LaunchAssignmentError::UnknownAirframe {
            airframe: airframe("fixture.synthetic-unknown-model"),
        })
    );

    let mut hangar_only = synthetic_fixed_wing_role();
    hangar_only.launch.mission_launchable = false;
    hangar_only.pilotable = false;
    hangar_only.launch.hangar_selectable = false;
    hangar_only.availability = Availability::MissionOnly;
    let hangar_only_roster = AirframeRoles::new(vec![hangar_only]).expect("a valid role");
    let assignment = forced(FIXED_WING, 41);
    assert_eq!(
        hangar_only_roster
            .resolve_launch(&garage, Some(&assignment))
            .err(),
        Some(LaunchAssignmentError::NotPilotable {
            airframe: airframe(FIXED_WING),
        })
    );

    let mut scoped = synthetic_autogyro_role();
    scoped.launch.mission_launchable = false;
    let scoped_roster = AirframeRoles::new(vec![scoped]).expect("a valid role");
    let scoped_assignment = forced(AUTOGYRO, 41);
    assert_eq!(
        scoped_roster
            .resolve_launch(&garage, Some(&scoped_assignment))
            .err(),
        Some(LaunchAssignmentError::NotMissionLaunchable {
            airframe: airframe(AUTOGYRO),
        })
    );

    // A mission-only airframe the hangar may not select is refused as a hangar
    // selection, never silently swapped for the garage plane.
    let mission_only_garage = OwnedLoadout {
        airframe: airframe(AUTOGYRO),
        session_generation: 41,
    };
    assert_eq!(
        roster.resolve_launch(&mission_only_garage, None).err(),
        Some(LaunchAssignmentError::NotHangarSelectable {
            airframe: airframe(AUTOGYRO),
        })
    );
}

/// Roster presence and ordinary menu availability are separate facts, and the
/// role boundary refuses an inconsistent declaration by name instead of
/// reconciling it. Nothing here claims an original roster: the declared roles
/// are synthetic and the exceptional rotor ratio stays an explicit unknown.
#[test]
fn accept_f25_a_roles_declare_availability_capability_and_unknowns() {
    let roster = declared_synthetic_roles();
    assert_eq!(roster.roles().len(), 2);

    let fixed: &AirframeRole = roster
        .role(&airframe(FIXED_WING))
        .expect("the fixed-wing role is in the roster");
    assert_eq!(fixed.validate(), Ok(()));
    assert_eq!(fixed.model_kind, "fixed_wing");
    assert!(!fixed.is_exceptional());
    assert!(fixed.rotor.is_none());
    assert!(fixed.launch.hangar_selectable);
    assert!(fixed.weapons.armed);

    let autogyro = roster
        .role(&airframe(AUTOGYRO))
        .expect("the exceptional role is in the roster");
    assert_eq!(autogyro.validate(), Ok(()));
    assert_eq!(autogyro.model_kind, "exceptional");
    assert!(autogyro.is_exceptional());
    assert_eq!(autogyro.availability, Availability::MissionOnly);
    assert!(!autogyro.weapons.armed);
    assert!(!autogyro.origin.is_original());
    assert_eq!(autogyro.origin, Origin::SyntheticFixture);
    assert_eq!(
        autogyro
            .rotor
            .as_ref()
            .expect("an exceptional role declares its rotor facts")
            .visual_radps_per_physical_radps
            .clone()
            .known(),
        None,
        "no original visual/physical rotor ratio was measured"
    );

    let mut mission_only_selectable = synthetic_autogyro_role();
    mission_only_selectable.launch.hangar_selectable = true;
    assert_eq!(
        mission_only_selectable.validate(),
        Err(AirframeRoleError::MissionOnlyButSelectable {
            airframe: airframe(AUTOGYRO)
        })
    );

    let mut rotor_on_a_wing = synthetic_fixed_wing_role();
    rotor_on_a_wing.rotor = Some(cs_content::airframe_roles::RotorRole {
        visual_radps_per_physical_radps: Resolved::unknown(
            claim("f25a.it.wing-rotor"),
            "a fixed wing has no rotor",
        )
        .expect("a reason is present"),
    });
    assert_eq!(
        rotor_on_a_wing.validate(),
        Err(AirframeRoleError::UnexpectedRotorRole)
    );

    let mut missing_rotor = synthetic_autogyro_role();
    missing_rotor.rotor = None;
    assert_eq!(
        missing_rotor.validate(),
        Err(AirframeRoleError::MissingRotorRole)
    );

    let mut invented_kind = synthetic_autogyro_role();
    invented_kind.model_kind = "gyrocopter".to_owned();
    assert_eq!(
        invented_kind.validate(),
        Err(AirframeRoleError::UnknownModelKind {
            value: "gyrocopter".to_owned()
        })
    );

    assert_eq!(
        AirframeRoles::new(vec![
            synthetic_fixed_wing_role(),
            synthetic_fixed_wing_role()
        ])
        .err(),
        Some(AirframeRoleError::DuplicateAirframe {
            airframe: airframe(FIXED_WING)
        })
    );
}
