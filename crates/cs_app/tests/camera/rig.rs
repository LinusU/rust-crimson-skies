//! F21-B: the cockpit, chase and look rigs.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-B`, non-negotiable behaviors 1 and 2. Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! These tests drive `cs_app::camera::CameraRig` over real declared records:
//! the modes come from `cs_content::cameras` and are lowered by
//! `cs_app::camera::lower_camera_modes`, so every eye position asserted here
//! is the production lowering applied to the production rig. The expected
//! numbers are derived by hand from the declared offsets and a single 90°
//! yaw, so a rig that invented an eye, a HUD anchor or a look pivot fails.
//!
//! The spyglass rig has its own file, `spyglass.rs`, because AC02 is about
//! the target changing underneath the camera; `smoothing.rs` covers F21
//! non-negotiable behavior 4.

use std::time::Duration;

use cs_app::camera::{
    CameraLowerError, CameraRig, LoweredCameraModes, RigError, RigInputs, SmoothingState, ViewRig,
    lower_camera_mode, lower_camera_modes,
};
use cs_app::origin::OriginChange;
use cs_content::cameras::{
    AspectRatio, BodyOffset, CameraModeError, CameraModeKind, CockpitBindingSource,
    CockpitViewpoint, DeclaredCameraMode, DeclaredCameraModes, DeclaredPlacement, LookLimits,
    Magnification, declared_synthetic_camera_modes, owns_camera_modes,
};
use cs_sim::damage::ActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, Radians, UnitVec3};

use crate::common::{
    aircraft_pose, assert_close, assert_close_position, body_offset, claim, declared_mode,
    declared_mode_with, declared_set, fixture_rig, known, known_look_limits, look, lowered_set,
    placement, projection_for, world,
};

/// The fixture airframe's pilot: 1.2 m above and 1.5 m forward of the body
/// origin.
const COCKPIT_UP_M: f64 = 1.2;
const COCKPIT_FORWARD_M: f64 = -1.5;
/// The fixture airframe's chase view: 3 m above and 12 m behind the origin.
const CHASE_UP_M: f64 = 3.0;
const CHASE_BACK_M: f64 = -12.0;

/// A +90° yaw about the canonical up axis.
///
/// +90° about `+Y` turns the canonical forward `-Z` into `-X` (right-hand
/// rule), and turns the body offset `(right, up, forward)` into
/// `(forward, up, -right)` — which is what makes the expected eye positions
/// below exact hand-computed numbers rather than a second implementation of
/// the rotation.
fn yawed_right() -> Quaternion {
    Quaternion::from_axis_angle(UnitVec3::UP, Radians(std::f64::consts::FRAC_PI_2))
        .expect("a right-angle yaw is a usable rotation")
}

fn session() -> SessionId {
    SessionId::new(7).expect("a nonzero session generation")
}

fn observer(serial: u64) -> ActorId {
    ActorId {
        session: session(),
        serial,
    }
}

fn subject() -> ActorId {
    observer(1)
}

/// One frame of inputs at the given pose, with the rig's own wall time.
fn inputs(aircraft: cs_app::camera::CameraPose) -> RigInputs<'static> {
    RigInputs {
        at: Tick(10),
        subject: subject(),
        aircraft,
        aspect: AspectRatio::SIXTEEN_NINE,
        elapsed: Duration::from_millis(16),
        look: None,
        spyglass: None,
        origin_change: OriginChange::Rebase,
    }
}

/// A fixture rig whose smoothing is effectively off.
///
/// At this response rate a 16 ms frame has `1 − exp(−k·dt)` equal to exactly 1
/// in f64, so a frame *is* the desired pose. The rig tests use it where they
/// assert an orientation: at a realistic rate the camera is still easing
/// toward a pose that moved, which is correct behaviour and would make the
/// assertion measure the smoothing law instead of the look.
fn instant_rig() -> CameraRig {
    CameraRig::with_response(
        lower_camera_modes(&declared_synthetic_camera_modes()).expect("the fixture lowers"),
        1.0e9,
    )
    .expect("a finite response rate")
}

/// F21 non-negotiable behavior 1: the cockpit viewpoint comes from verified
/// model/config bindings, and a HUD-only synthetic camera is not a
/// replacement for every original cockpit.
///
/// The failure this discriminates: a rig that places the eye from its own
/// constant — a fixed "cockpit" point, or nothing at all with the HUD doing
/// the drawing — passes every other test in this file and shows the pilot the
/// wrong place.
#[test]
fn accept_f21_b_cockpit_eye_comes_from_the_declared_binding_and_names_it() {
    let mut rig = fixture_rig();
    rig.set_rig(ViewRig::Cockpit)
        .expect("the cockpit rig exists");

    // 90° of yaw, so the declared body offset becomes an exact world offset:
    // `(0, 1.2, -1.5)` rotated right is `(-1.5, 1.2, 0)`.
    let aircraft = aircraft_pose([1_000.0, -250.0, 500.0], yawed_right());
    let frame = rig
        .resolve(&inputs(aircraft))
        .expect("the cockpit resolves");

    assert_eq!(frame.rig, ViewRig::Cockpit);
    assert_eq!(frame.mode, CameraModeKind::Cockpit);
    assert_close_position(
        frame.pose.position(),
        [1_000.0 - 1.5, -250.0 + COCKPIT_UP_M, 500.0],
        1e-9,
        "the cockpit eye sits at the declared binding offset",
    );
    assert_ne!(
        frame.pose.position(),
        aircraft.position(),
        "the cockpit eye is not the body origin: that would be a HUD-only camera"
    );

    // The eye's rotation is the aircraft's attitude with the pilot's declared
    // head turn, which for this fixture is the identity turn.
    let basis = frame.pose.basis().expect("a usable basis");
    assert_close_position(
        world(basis.forward().to_array()),
        [-1.0, 0.0, 0.0],
        1e-12,
        "the cockpit looks where the aircraft looks, yawed right",
    );

    // The frustum is the cockpit mode's own, aspect-correct one.
    assert_close(
        frame.projection.vertical_fov().0,
        Radians(60.0_f64.to_radians()).0,
        1e-12,
        "the cockpit frame uses the declared cockpit field of view",
    );
    assert_eq!(
        frame.projection.near_m().0,
        0.1,
        "and the declared near plane"
    );
    assert_eq!(frame.magnification.value(), 1.0);
    assert_eq!(frame.aspect, AspectRatio::SIXTEEN_NINE);

    // Provenance: the rig names the binding its eye came from, and the set's
    // origin says the binding is a fixture and not an original measurement.
    assert_eq!(
        rig.cockpit_binding(),
        Some(&CockpitBindingSource::ModelNode {
            node: "synthetic.pilot_eye".to_owned(),
        }),
        "the frame's eye is bound to a named model node"
    );
    assert_eq!(
        rig.origin(),
        &Origin::SyntheticFixture,
        "the fixture binding is development content, never a verified original one"
    );

    // A *different* declared binding moves the eye, which is what "comes from
    // the binding" means operationally: the rig reads the record, it does not
    // remember a constant.
    let tall = DeclaredPlacement::at_cockpit(
        CockpitViewpoint::try_new(
            CockpitBindingSource::ConfigKey {
                key: "synthetic.tall_eye".to_owned(),
            },
            body_offset(0.0, 2.5, -2.0),
            known(Radians(0.0)),
            known(Radians(0.0)),
        )
        .expect("the tall viewpoint is valid"),
    );
    let tall_modes = lowered_set(
        vec![declared_mode_with(
            CameraModeKind::Cockpit,
            tall.clone(),
            false,
        )],
        CameraModeKind::Cockpit,
    );
    let mut tall_rig = CameraRig::new(tall_modes).expect("the tall set has a rig");
    let tall_frame = tall_rig
        .resolve(&inputs(aircraft_pose(
            [0.0, 0.0, 0.0],
            Quaternion::IDENTITY,
        )))
        .expect("the tall cockpit resolves");
    assert_close_position(
        tall_frame.pose.position(),
        [0.0, 2.5, -2.0],
        1e-12,
        "a second declared binding places a second eye",
    );
    assert_eq!(
        tall_rig.cockpit_binding(),
        Some(&CockpitBindingSource::ConfigKey {
            key: "synthetic.tall_eye".to_owned(),
        }),
        "and the rig names that binding instead"
    );
}

/// F21 non-negotiable behavior 1, enforced by the record rather than by the
/// rig: a cockpit mode without a binding and a non-cockpit mode with one are
/// both declaration errors, so no session can be given an "aircraft with a
/// cockpit" that has nothing to look from.
#[test]
fn accept_f21_b_a_cockpit_mode_needs_a_binding_and_no_other_mode_may_claim_one() {
    let no_binding = DeclaredCameraMode::try_new(
        CameraModeKind::Cockpit,
        projection_for(CameraModeKind::Cockpit),
        known(Magnification::ONE),
        known(false),
        DeclaredPlacement::BodyOffset(BodyOffset::ZERO),
        known_look_limits(),
    );
    assert_eq!(
        no_binding,
        Err(CameraModeError::CockpitViewpointRequired {
            kind: CameraModeKind::Cockpit,
        }),
        "a body offset is not a cockpit viewpoint"
    );

    for kind in [
        CameraModeKind::External,
        CameraModeKind::Spyglass,
        CameraModeKind::AuthoredSequence,
    ] {
        let claimed = DeclaredCameraMode::try_new(
            kind,
            projection_for(kind),
            known(Magnification::ONE),
            known(false),
            placement(CameraModeKind::Cockpit),
            known_look_limits(),
        );
        assert_eq!(
            claimed,
            Err(CameraModeError::UnexpectedCockpitViewpoint { kind }),
            "the {kind} mode must not claim the cockpit binding"
        );
    }

    // An airframe with no cockpit binding is expressible, and it simply has no
    // cockpit mode: the set's default moves to the view it does have.
    let external_only = declared_set(
        vec![declared_mode(CameraModeKind::External)],
        CameraModeKind::External,
    );
    assert!(
        external_only.get(CameraModeKind::Cockpit).is_none(),
        "an aircraft with no binding declares no cockpit view at all"
    );
    let mut rig = CameraRig::new(lower_camera_modes(&external_only).expect("the set lowers"))
        .expect("the external default has a rig");
    assert_eq!(
        rig.set_rig(ViewRig::Cockpit),
        Err(RigError::ModeNotDeclared {
            rig: ViewRig::Cockpit,
            kind: CameraModeKind::Cockpit,
        }),
        "and asking for the cockpit it does not have is refused by name"
    );
    assert_eq!(rig.cockpit_binding(), None);
}

/// The chase view sits at the declared body offset and looks along the
/// aircraft's own axes — no pivot, no invented look-at target.
///
/// The failure this discriminates: a chase camera that rotates to keep a fixed
/// world point in view, or one that hangs off the aircraft origin, is a
/// different view that happens to be called "external".
#[test]
fn accept_f21_b_chase_view_sits_at_the_declared_body_offset_and_follows_the_aircraft() {
    let mut rig = fixture_rig();
    rig.set_rig(ViewRig::Chase).expect("the chase rig exists");

    let aircraft = aircraft_pose([0.0, 0.0, 0.0], yawed_right());
    let frame = rig.resolve(&inputs(aircraft)).expect("the chase resolves");

    assert_eq!(frame.rig, ViewRig::Chase);
    assert_eq!(frame.mode, CameraModeKind::External);
    assert_close_position(
        frame.pose.position(),
        [CHASE_BACK_M, CHASE_UP_M, 0.0],
        1e-9,
        "the chase eye is the declared offset turned with the aircraft",
    );

    let basis = frame.pose.basis().expect("a usable basis");
    assert_close_position(
        world(basis.forward().to_array()),
        [-1.0, 0.0, 0.0],
        1e-12,
        "the chase view looks along the aircraft's own forward axis",
    );
    assert_close_position(
        world(basis.up().to_array()),
        [0.0, 1.0, 0.0],
        1e-12,
        "and keeps the canonical up, with no roll",
    );
    assert_eq!(frame.magnification.value(), 1.0);
    assert_eq!(
        rig.cockpit_binding(),
        None,
        "the chase view claims no binding"
    );

    // Framing composes: an invariant world point keeps a viewport coordinate
    // that comes from the chase mode's own frustum, not the cockpit's.
    let framing = frame
        .framing_of(world([CHASE_BACK_M - 40.0, CHASE_UP_M, 0.0]))
        .expect("the point is in front of the chase eye");
    assert_close(
        framing.y(),
        0.0,
        1e-12,
        "a point on the view axis is centred",
    );
    assert_close(framing.x(), 0.0, 1e-12, "on both axes");
}

/// Free look turns the view without moving the eye, and it is clamped to the
/// mode's declared limits — the frame reports the turn that was applied, so a
/// consumer cannot act on one that was not.
///
/// The failure this discriminates: an unclamped look rotates the camera past
/// vertical, where the up axis has no right axis left; a clamped-but-unreported
/// one makes the HUD and the camera disagree about where the pilot is looking.
#[test]
fn accept_f21_b_free_look_turns_the_view_only_and_is_clamped_to_the_declared_limits() {
    let mut rig = instant_rig();
    rig.set_rig(ViewRig::Cockpit)
        .expect("the cockpit rig exists");
    let cockpit = rig
        .resolve(&inputs(aircraft_pose(
            [0.0, 0.0, 0.0],
            Quaternion::IDENTITY,
        )))
        .expect("the cockpit resolves");

    // A 90° glance to the left is inside the declared limits, so it is applied
    // whole: yaw is positive toward -X, so forward becomes -X.
    rig.set_rig(ViewRig::Look).expect("the look rig exists");
    let mut looked = inputs(aircraft_pose([0.0, 0.0, 0.0], Quaternion::IDENTITY));
    looked.look = Some(look(std::f64::consts::FRAC_PI_2, 0.0));
    let glanced = rig.resolve(&looked).expect("the look resolves");
    assert_eq!(glanced.rig, ViewRig::Look);
    assert_eq!(
        glanced.mode,
        CameraModeKind::Cockpit,
        "a look keeps the view it is looking out of"
    );
    assert_eq!(
        glanced.pose.position(),
        cockpit.pose.position(),
        "a look turns the view; it does not move the eye"
    );
    assert_close_position(
        world(glanced.pose.basis().expect("a basis").forward().to_array()),
        [-1.0, 0.0, 0.0],
        1e-12,
        "the looked view points where the glance points",
    );

    // Beyond the declared limits the offset is clamped, and the clamped value
    // is what the frame reports.
    let mut overboard = inputs(aircraft_pose([0.0, 0.0, 0.0], Quaternion::IDENTITY));
    overboard.look = Some(look(3.0, 0.5));
    let clamped = rig.resolve(&overboard).expect("the clamped look resolves");
    let limits = LookLimits::new(
        Radians(120.0_f64.to_radians()),
        Radians(60.0_f64.to_radians()),
    )
    .expect("the fixture limits are in range");
    assert_eq!(
        clamped.look,
        Some(look(120.0_f64.to_radians(), 0.5).clamped(limits)),
        "the frame reports the clamped turn, not the requested one"
    );
    assert_eq!(
        clamped.look.map(cs_app::camera::LookOffset::yaw),
        Some(Radians(120.0_f64.to_radians())),
        "yaw is held at the declared limit"
    );
    assert_eq!(
        clamped.look.map(cs_app::camera::LookOffset::pitch),
        Some(Radians(0.5)),
        "and a pitch inside the limits is left alone"
    );

    // A look with no input is an identity turn, reported as one.
    let mut released = inputs(aircraft_pose([0.0, 0.0, 0.0], Quaternion::IDENTITY));
    released.look = None;
    let released_frame = rig.resolve(&released).expect("the released look resolves");
    assert_eq!(
        released_frame.look,
        Some(cs_app::camera::LookOffset::IDENTITY)
    );
    assert_close_position(
        world(
            released_frame
                .pose
                .basis()
                .expect("a basis")
                .forward()
                .to_array(),
        ),
        [0.0, 0.0, -1.0],
        1e-12,
        "and the view is back along the aircraft's own axis",
    );
}

/// The spyglass aims at the selection, so it does not free-look; a rig
/// selection for a mode its owner did not declare, and a default with no rig
/// in this stage, are both refused by name.
#[test]
fn accept_f21_b_the_spyglass_does_not_free_look_and_undeclared_modes_refuse() {
    let mut rig = fixture_rig();
    rig.set_rig(ViewRig::Spyglass)
        .expect("the spyglass rig exists");
    assert_eq!(
        rig.set_rig(ViewRig::Look),
        Err(RigError::LookNotAvailable {
            kind: CameraModeKind::Spyglass,
        }),
        "a look would point the view away from the thing it magnifies"
    );
    assert_eq!(
        rig.set_rig(ViewRig::Spyglass),
        Ok(()),
        "re-selecting the rig it is already in is not an error"
    );

    let cockpit_only = lowered_set(
        vec![declared_mode(CameraModeKind::Cockpit)],
        CameraModeKind::Cockpit,
    );
    let mut limited = CameraRig::new(cockpit_only).expect("the cockpit default has a rig");
    assert_eq!(
        limited.set_rig(ViewRig::Chase),
        Err(RigError::ModeNotDeclared {
            rig: ViewRig::Chase,
            kind: CameraModeKind::External,
        })
    );
    assert_eq!(
        limited.set_rig(ViewRig::Spyglass),
        Err(RigError::ModeNotDeclared {
            rig: ViewRig::Spyglass,
            kind: CameraModeKind::Spyglass,
        })
    );
    assert_eq!(
        limited.rig(),
        ViewRig::Cockpit,
        "a refused change moves nothing"
    );

    // An authored camera sequence has no rig in this stage.
    let scripted = CameraRig::new(lowered_set(
        vec![declared_mode(CameraModeKind::AuthoredSequence)],
        CameraModeKind::AuthoredSequence,
    ));
    assert_eq!(
        scripted,
        Err(RigError::NoRigForMode {
            kind: CameraModeKind::AuthoredSequence,
        }),
        "a scripted default is F21-C's, not an invented chase view"
    );
}

/// The lowering boundary keeps refusing: an unknown look limit and an unknown
/// cockpit head orientation are both refused by field name, so no session runs
/// a rig under a guessed clamp or a guessed eye.
#[test]
fn accept_f21_b_unknown_look_limits_and_cockpit_orientation_refuse_to_lower() {
    let unknown_limits = DeclaredCameraMode::try_new(
        CameraModeKind::Cockpit,
        projection_for(CameraModeKind::Cockpit),
        known(Magnification::ONE),
        known(false),
        placement(CameraModeKind::Cockpit),
        Resolved::unknown(claim(), "the original look range is unmeasured").expect("a reason"),
    )
    .expect("an unknown is valid declared content");
    assert_eq!(
        lower_camera_mode(&unknown_limits),
        Err(CameraLowerError::UnknownField {
            mode: CameraModeKind::Cockpit,
            field: "look_limits",
            claim_id: claim(),
            reason: "the original look range is unmeasured".to_owned(),
        })
    );

    let unknown_eye = DeclaredCameraMode::try_new(
        CameraModeKind::Cockpit,
        projection_for(CameraModeKind::Cockpit),
        known(Magnification::ONE),
        known(false),
        DeclaredPlacement::at_cockpit(
            CockpitViewpoint::try_new(
                CockpitBindingSource::ModelNode {
                    node: "synthetic.unmeasured_eye".to_owned(),
                },
                BodyOffset::ZERO,
                Resolved::unknown(claim(), "no head yaw was read").expect("a reason"),
                known(Radians(0.0)),
            )
            .expect("an unknown angle is valid declared content"),
        ),
        known_look_limits(),
    )
    .expect("an unknown is valid declared content");
    assert_eq!(
        lower_camera_mode(&unknown_eye),
        Err(CameraLowerError::UnknownField {
            mode: CameraModeKind::Cockpit,
            field: "cockpit_viewpoint.yaw",
            claim_id: claim(),
            reason: "no head yaw was read".to_owned(),
        })
    );
}

/// The whole declared set still lowers for every owner kind that may own one,
/// placement and origin included — the F21-B fields did not narrow the F21-A
/// owner vocabulary.
#[test]
fn accept_f21_b_every_owner_kind_lowers_with_its_placement_and_origin() {
    let fixture = declared_synthetic_camera_modes();
    let owners: Vec<ContentId> = ContentKind::ALL
        .iter()
        .copied()
        .filter(|kind| owns_camera_modes(*kind))
        .map(|kind| ContentId::from_source(kind, "synthetic.camera-owner").expect("a valid id"))
        .collect();
    assert_eq!(owners.len(), 4, "the decided owner vocabulary");

    for owner in owners {
        let set = DeclaredCameraModes::try_new(
            owner.clone(),
            fixture.origin().clone(),
            fixture.default_mode(),
            fixture.modes().to_vec(),
            Provenance::designed(claim()),
        )
        .expect("a decided owner is accepted");
        let lowered: LoweredCameraModes = lower_camera_modes(&set).expect("the set lowers");
        assert_eq!(lowered.origin(), set.origin());
        let mut rig = CameraRig::new(lowered).expect("the default has a rig");
        assert_eq!(rig.origin(), set.origin());
        assert_eq!(rig.mode(), CameraModeKind::Cockpit);
        assert!(
            rig.cockpit_binding().is_some(),
            "{owner}: the lowered cockpit keeps its binding"
        );
        // And the rig actually runs for this owner: the eye comes from the
        // declared binding, at the declared offset.
        let frame = rig
            .resolve(&inputs(aircraft_pose(
                [0.0, 0.0, 0.0],
                Quaternion::IDENTITY,
            )))
            .expect("the rig resolves");
        assert_close_position(
            frame.pose.position(),
            [0.0, COCKPIT_UP_M, COCKPIT_FORWARD_M],
            1e-12,
            &format!("{owner}: the cockpit eye"),
        );
        assert_eq!(frame.smoothing, SmoothingState::Reseated);
    }
}
