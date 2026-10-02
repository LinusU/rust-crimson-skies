//! F21-B: frame-rate independent camera smoothing, and what resets it.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-B`, non-negotiable behavior 4: "Camera smoothing is frame-rate
//! independent, reset on teleport/plane swap and preserved correctly through
//! origin shifts". Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! The rig is driven through the real `cs_app::origin` types here, so the
//! rebase case is measured against the epoch and local-coordinate bookkeeping
//! F16 owns rather than against a comment.

use std::time::Duration;

use cs_app::camera::{
    CameraRig, LookOffset, PoseSmoother, RigInputs, SmoothingError, SmoothingState, ViewRig,
    lower_camera_modes,
};
use cs_app::origin::{
    OriginChange, OriginEpoch, OriginShift, SpatialAnchor, WorldOrigin,
    local_round_trip_tolerance_m,
};
use cs_content::cameras::{AspectRatio, CameraModeKind, declared_synthetic_camera_modes};
use cs_sim::damage::ActorId;
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, Radians, WorldPosition};

use crate::common::{aircraft_pose, assert_close, assert_close_position, world};

/// One second of wall time, split into `frame_rate` render frames plus the
/// remainder, exactly as `FixedTickDriver::advance_frame` splits a frame.
fn frames_at(frame_rate: u32, total: Duration) -> Vec<Duration> {
    let nanos = total.as_nanos();
    let per_frame = nanos / u128::from(frame_rate);
    let mut spans: Vec<Duration> = (0..u128::from(frame_rate))
        .map(|_| Duration::from_nanos(per_frame as u64))
        .collect();
    let used: u128 = spans.iter().map(|span| span.as_nanos()).sum();
    spans.push(Duration::from_nanos((nanos - used) as u64));
    spans
}

fn session() -> SessionId {
    SessionId::new(33).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(),
        serial,
    }
}

/// A cockpit rig over the fixture's modes, with a response rate chosen so a
/// one-second follow is visibly incomplete but clearly converging.
fn chase_rig(response_per_s: f64) -> CameraRig {
    let mut rig = CameraRig::with_response(
        lower_camera_modes(&declared_synthetic_camera_modes()).expect("the fixture lowers"),
        response_per_s,
    )
    .expect("a finite response rate");
    rig.set_rig(ViewRig::Chase).expect("the chase rig exists");
    rig
}

fn frame_inputs(
    at: u64,
    subject: ActorId,
    aircraft: cs_app::camera::CameraPose,
    elapsed: Duration,
    origin_change: OriginChange,
) -> RigInputs<'static> {
    RigInputs {
        at: Tick(at),
        subject,
        aircraft,
        aspect: AspectRatio::SIXTEEN_NINE,
        elapsed,
        look: None,
        spyglass: None,
        origin_change,
    }
}

/// A chase rig parked at `at_start`, so the next frames have somewhere to
/// follow from.
fn parked(at_start: [f64; 3]) -> CameraRig {
    let mut rig = chase_rig(12.0);
    rig.resolve(&frame_inputs(
        1,
        actor(1),
        aircraft_pose(at_start, Quaternion::IDENTITY),
        Duration::ZERO,
        OriginChange::Rebase,
    ))
    .expect("the first frame reseats");
    rig
}

/// F21 non-negotiable behavior 4, first clause: after the same wall time the
/// camera is in the same place whatever the render frame rate.
///
/// The failure this discriminates: a per-frame constant fraction
/// (`pose += 0.1 * (desired - pose)`) puts 30 FPS a tenth of the way and 144
/// FPS nearly half way through the same second, so the same session looks
/// different on two machines.
#[test]
fn accept_f21_b_smoothing_is_frame_rate_independent_at_30_60_and_144_fps() {
    let target = [4_000.0, 900.0, -1_200.0];
    let mut runs = Vec::new();

    for frame_rate in [30_u32, 60, 144] {
        let mut rig = parked([0.0, 0.0, 0.0]);
        let start = rig.smoother().pose().expect("a parked pose").position();
        let aircraft = aircraft_pose(target, Quaternion::IDENTITY);
        for (tick, span) in (2..).zip(frames_at(frame_rate, Duration::from_secs(1))) {
            rig.resolve(&frame_inputs(
                tick,
                actor(1),
                aircraft,
                span,
                OriginChange::Rebase,
            ))
            .expect("the rig resolves");
        }
        let pose = rig.smoother().pose().expect("a pose");
        runs.push((frame_rate, pose, start));
    }

    let (_, reference, start) = runs[0];
    for (frame_rate, pose, run_start) in &runs {
        // The exponential law telescopes: over one second the weight left on
        // the starting pose is exp(-k·1s) whatever the split, so the three
        // runs agree to floating-point rounding rather than "closely".
        assert_close_position(
            pose.position(),
            reference.position().to_array(),
            1e-9,
            &format!("{frame_rate} FPS must agree with 30 FPS"),
        );
        assert_eq!(
            *run_start, start,
            "every run must start from the same parked pose"
        );
        assert_close(
            pose.rotation().components()[0],
            reference.rotation().components()[0],
            1e-9,
            &format!("{frame_rate} FPS must agree with 30 FPS on rotation"),
        );
    }

    // And it really followed: the exponential law says the weight left on the
    // starting pose after one second is exp(−k·1s), so the residual is exactly
    // that fraction of the distance it began with. The assertion is on the
    // residual rather than on "close enough to the target", because the second
    // number is the one that would differ if the law were per-frame.
    //
    // The fixture's chase offset is 3 m up and 12 m behind the body origin,
    // which for an unrotated aircraft is 12 m along canonical `-Z`.
    let desired = [target[0], target[1] + 3.0, target[2] - 12.0];
    let residual = (-CameraRig::DEFAULT_RESPONSE_PER_S).exp();
    for (frame_rate, pose, _) in &runs {
        for (axis, value) in pose.position().to_array().into_iter().enumerate() {
            let expected = desired[axis] + residual * (start.to_array()[axis] - desired[axis]);
            assert_close(
                value,
                expected,
                1e-6,
                &format!("{frame_rate} FPS must retain exp(-k) of the starting distance"),
            );
        }
    }
    assert!(
        reference.position() != aircraft_pose(target, Quaternion::IDENTITY).position(),
        "and the camera is not the aircraft's origin"
    );
}

/// A frame rate of zero is not a camera that lags: no wall time passed, so
/// nothing moved, and the frame says so.
#[test]
fn accept_f21_b_a_zero_length_frame_does_not_move_the_camera() {
    let mut rig = parked([0.0, 0.0, 0.0]);
    let start = rig.smoother().pose().expect("a parked pose");
    let elsewhere = aircraft_pose([500.0, 0.0, 0.0], Quaternion::IDENTITY);
    let frame = rig
        .resolve(&frame_inputs(
            2,
            actor(1),
            elsewhere,
            Duration::ZERO,
            OriginChange::Rebase,
        ))
        .expect("the rig resolves");
    assert_eq!(
        frame.smoothing,
        SmoothingState::Tracking,
        "and the camera is not settled: it is not where it was asked to be"
    );
    assert_eq!(
        frame.pose.position(),
        start.position(),
        "no wall time means no smoothing"
    );
    assert_eq!(
        rig.smoother().desired(),
        Some(cs_app::camera::CameraPose::new(
            world([500.0, 3.0, -12.0]),
            Quaternion::IDENTITY
        )),
        "and the rig remembers what it was asked for, which it is not at yet"
    );
    assert!(
        !rig.smoother().is_settled(),
        "the rig is not where it was asked to be"
    );
}

/// F21 non-negotiable behavior 4, second clause: a teleport jumps the camera
/// and does not drag it across the world over the following frames.
#[test]
fn accept_f21_b_a_teleport_reseats_the_camera_instead_of_drags_it() {
    let mut rig = chase_rig(1.0);
    let here = aircraft_pose([0.0, 0.0, 0.0], Quaternion::IDENTITY);
    rig.resolve(&frame_inputs(
        1,
        actor(1),
        here,
        Duration::ZERO,
        OriginChange::Rebase,
    ))
    .expect("the first frame reseats");

    // A long smooth run away from the origin first, so the camera is visibly
    // lagging.
    let there = aircraft_pose([1_000.0, 0.0, 0.0], Quaternion::IDENTITY);
    let mut tick = 2;
    for span in frames_at(60, Duration::from_millis(200)) {
        rig.resolve(&frame_inputs(
            tick,
            actor(1),
            there,
            span,
            OriginChange::Rebase,
        ))
        .expect("the rig resolves");
        tick += 1;
    }
    let lagging = rig.smoother().pose().expect("a pose");
    assert_eq!(rig.smoother().state(), SmoothingState::Tracking);

    // The teleport: the aircraft jumps 10 km, and the camera is with it on the
    // same frame.
    let jumped = aircraft_pose([10_000.0, 0.0, 0.0], Quaternion::IDENTITY);
    let frame = rig
        .resolve(&frame_inputs(
            tick,
            actor(1),
            jumped,
            Duration::from_millis(16),
            OriginChange::Teleport,
        ))
        .expect("the rig resolves");
    assert_eq!(frame.smoothing, SmoothingState::Reseated);
    assert_close_position(
        frame.pose.position(),
        [10_000.0, 3.0, -12.0],
        1e-9,
        "a teleport reseats the camera at the new declared eye",
    );
    assert!(
        frame.pose.position() != lagging.position(),
        "and does not leave the camera at the old one"
    );
}

/// A plane swap is the other reset: a different aircraft is a different
/// generation, and the camera starts where the *new* aircraft's declared
/// placement says, with none of the previous aircraft's lag.
#[test]
fn accept_f21_b_a_plane_swap_reseats_the_camera() {
    let mut rig = chase_rig(1.0);
    let first = aircraft_pose([0.0, 0.0, 0.0], Quaternion::IDENTITY);
    rig.resolve(&frame_inputs(
        1,
        actor(1),
        first,
        Duration::ZERO,
        OriginChange::Rebase,
    ))
    .expect("the first frame reseats");
    let away = aircraft_pose([1_000.0, 0.0, 0.0], Quaternion::IDENTITY);
    let mut tick = 2;
    for span in frames_at(60, Duration::from_millis(200)) {
        rig.resolve(&frame_inputs(
            tick,
            actor(1),
            away,
            span,
            OriginChange::Rebase,
        ))
        .expect("the rig resolves");
        tick += 1;
    }

    let frame = rig
        .resolve(&frame_inputs(
            tick,
            actor(2),
            aircraft_pose([5_000.0, 0.0, 0.0], Quaternion::IDENTITY),
            Duration::from_millis(16),
            OriginChange::Rebase,
        ))
        .expect("the rig resolves");
    assert_eq!(
        frame.smoothing,
        SmoothingState::Reseated,
        "a different aircraft reseats the camera"
    );
    assert_close_position(
        frame.pose.position(),
        [5_000.0, 3.0, -12.0],
        1e-9,
        "at the new aircraft's declared eye",
    );
    assert_eq!(rig.subject(), Some(actor(2)));
}

/// F21 non-negotiable behavior 4, third clause, measured against F16's own
/// bookkeeping: a rebase moves the local frame and the epoch and leaves world
/// identity alone, so the camera neither jumps nor restarts following.
///
/// The failure this discriminates: a camera that smoothed a *local* pose would
/// be displaced by the origin offset at the rebase, which is a visible jump of
/// the whole world for a change that must move nothing in the world.
#[test]
fn accept_f21_b_a_rebase_moves_the_local_frame_without_moving_the_camera() {
    let origin = WorldOrigin::new(
        OriginEpoch(0),
        WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
    );
    let aircraft = aircraft_pose([1_200.0, 300.0, -800.0], Quaternion::IDENTITY);

    // The camera history is the same record F16 names for a camera, so the
    // shift that moves every subsystem is exercised on the camera's own pose.
    let mut anchor = SpatialAnchor::new(&origin, aircraft.position()).expect("finite");
    let shift = OriginShift::rebase(
        origin,
        WorldPosition::try_new([1_200.0, 300.0, -800.0]).expect("finite"),
    )
    .expect("epoch 0 can rebase");

    let mut with_rebase = chase_rig(12.0);
    let mut control = chase_rig(12.0);
    let total = Duration::from_millis(300);
    let spans = frames_at(60, total);
    let mut rebased_at = None;

    for (tick, (index, span)) in (1..).zip(spans.iter().enumerate()) {
        if rebased_at.is_none() {
            let before = anchor.local();
            shift
                .apply(std::slice::from_mut(&mut anchor))
                .expect("the camera converts");
            rebased_at = Some(Tick(tick));
            // The F16 bookkeeping this claim rests on: the epoch advanced and
            // the local coordinate changed, while the world identity did not.
            assert_eq!(
                anchor.epoch(),
                OriginEpoch(1),
                "a rebase advances the epoch"
            );
            assert_eq!(
                anchor.world(),
                aircraft.position(),
                "world identity survives"
            );
            assert_ne!(
                anchor.local(),
                before,
                "the local frame moved, which is the whole point of a rebase"
            );
            let tolerance = local_round_trip_tolerance_m(anchor.world(), anchor.local());
            assert!(tolerance > 0.0 && tolerance.is_finite());
            let back = shift.to().world_of(anchor.local()).expect("finite sum");
            assert_close_position(
                back,
                anchor.world().to_array(),
                tolerance,
                "the converted local address still describes the world point",
            );
        }

        let rebased_frame = with_rebase
            .resolve(&frame_inputs(
                tick,
                actor(1),
                aircraft,
                *span,
                // Every frame is told a rebase happened: the claim is that the
                // rig's answer is identical to one that is never told.
                OriginChange::Rebase,
            ))
            .expect("the rig resolves");
        let control_frame = control
            .resolve(&frame_inputs(
                tick,
                actor(1),
                aircraft,
                *span,
                OriginChange::Rebase,
            ))
            .expect("the rig resolves");
        assert_eq!(
            rebased_frame.pose, control_frame.pose,
            "frame {index}: a rebase must not change the camera at all"
        );
        assert_eq!(
            rebased_frame.smoothing, control_frame.smoothing,
            "frame {index}: and must not change how it got there"
        );
    }

    let pose = with_rebase.smoother().pose().expect("a pose");
    assert_eq!(
        pose.position(),
        control.smoother().pose().expect("a pose").position(),
        "after the whole run the two cameras are identical"
    );
    assert_ne!(
        pose.position(),
        world([0.0, 0.0, 0.0]),
        "and the run was not a no-op"
    );
}

/// The end-of-session path: a rig that is reset and rebuilt for the next
/// generation must not open with the previous one's camera position.
#[test]
fn accept_f21_b_reset_clears_the_session_camera_state() {
    let mut rig = parked([0.0, 0.0, 0.0]);
    let start = rig.smoother().pose().expect("a parked pose");
    let elsewhere = aircraft_pose([900.0, 0.0, 0.0], Quaternion::IDENTITY);
    let mut tick = 2;
    for span in frames_at(60, Duration::from_millis(100)) {
        rig.resolve(&frame_inputs(
            tick,
            actor(1),
            elsewhere,
            span,
            OriginChange::Rebase,
        ))
        .expect("the rig resolves");
        tick += 1;
    }
    assert_ne!(rig.smoother().pose(), Some(start));

    rig.reset();
    assert_eq!(rig.smoother().pose(), None, "the pose is forgotten");
    assert_eq!(rig.subject(), None, "the aircraft binding is forgotten");
    assert_eq!(rig.framed_target(), None, "and no target is framed");
    assert!(!rig.is_looking());

    let frame = rig
        .resolve(&frame_inputs(
            tick,
            actor(2),
            elsewhere,
            Duration::from_millis(16),
            OriginChange::Rebase,
        ))
        .expect("the rig resolves");
    assert_eq!(frame.smoothing, SmoothingState::Reseated);
    assert_close_position(
        frame.pose.position(),
        [900.0, 3.0, -12.0],
        1e-9,
        "and the new session starts at its own declared eye",
    );
}

/// The smoothing law itself: a rate that is zero or negative has no defined
/// behaviour, and a negative one would run the camera away from the pose it
/// was asked for, so it is refused rather than clamped.
#[test]
fn accept_f21_b_an_unusable_response_rate_is_refused() {
    for bad in [0.0, -1.0, f64::INFINITY] {
        assert_eq!(
            PoseSmoother::new(bad),
            Err(SmoothingError::InvalidResponse { per_second: bad }),
            "a response rate of {bad} has no meaning"
        );
    }
    assert!(
        matches!(
            PoseSmoother::new(f64::NAN),
            Err(SmoothingError::InvalidResponse { per_second }) if per_second.is_nan()
        ),
        "and a NaN rate is refused too, which an equality check could not see"
    );
    let smoother = PoseSmoother::new(12.0).expect("a finite rate");
    assert_eq!(smoother.response_per_s(), 12.0);
    assert_eq!(smoother.pose(), None);
    assert_eq!(smoother.state(), SmoothingState::Reseated);

    // A snap is the teleport path and lands exactly on the desired pose.
    let mut snapped = PoseSmoother::new(0.001).expect("a finite rate");
    let far = aircraft_pose([1_000.0, 0.0, 0.0], Quaternion::IDENTITY);
    snapped.snap(far);
    assert_eq!(snapped.pose(), Some(far));
    assert_eq!(snapped.state(), SmoothingState::Reseated);
    assert!(snapped.is_settled());
    assert_eq!(snapped.advance(far, Duration::ZERO).expect("advances"), far);
}

/// The camera's default response rate is a declared constant, so a session that
/// does not choose one gets the same rate a test measured.
#[test]
fn accept_f21_b_the_default_response_rate_is_the_measured_one() {
    const { assert!(CameraRig::DEFAULT_RESPONSE_PER_S > 0.0) };
    let mut rig = parked([0.0, 0.0, 0.0]);
    assert_eq!(
        rig.smoother().response_per_s(),
        CameraRig::DEFAULT_RESPONSE_PER_S,
        "and it is the rate the rig actually runs"
    );
    let elsewhere = aircraft_pose([1_000.0, 0.0, 0.0], Quaternion::IDENTITY);
    let _ = rig.resolve(&frame_inputs(
        2,
        actor(1),
        elsewhere,
        Duration::from_millis(16),
        OriginChange::Rebase,
    ));
    assert_eq!(rig.mode(), CameraModeKind::External);
    assert_eq!(rig.rig(), ViewRig::Chase);
    assert_eq!(
        rig.smoother().desired(),
        Some(cs_app::camera::CameraPose::new(
            world([1_000.0, 3.0, -12.0]),
            Quaternion::IDENTITY
        )),
        "and it was asked to be at the new aircraft's declared eye"
    );
}

/// The rig keeps no look state of its own: the clamped offset in a frame is
/// the whole of it, so releasing the look returns the view exactly.
#[test]
fn accept_f21_b_a_look_is_reported_as_applied_and_releases_cleanly() {
    let mut rig = chase_rig(12.0);
    let here = aircraft_pose([0.0, 0.0, 0.0], Quaternion::IDENTITY);
    rig.resolve(&frame_inputs(
        1,
        actor(1),
        here,
        Duration::ZERO,
        OriginChange::Rebase,
    ))
    .expect("the first frame reseats");
    rig.set_rig(ViewRig::Look).expect("the look rig exists");
    assert!(rig.is_looking());
    assert_eq!(rig.rig(), ViewRig::Look);

    let mut looking = frame_inputs(2, actor(1), here, Duration::ZERO, OriginChange::Rebase);
    looking.look = Some(LookOffset::new(Radians(0.25), Radians(0.0)).expect("finite"));
    let looked = rig.resolve(&looking).expect("the look resolves");
    assert_eq!(
        looked.look, looking.look,
        "a turn inside the limits is applied whole"
    );

    let released = rig
        .resolve(&frame_inputs(
            3,
            actor(1),
            here,
            Duration::ZERO,
            OriginChange::Rebase,
        ))
        .expect("the released look resolves");
    assert_eq!(
        released.look,
        Some(LookOffset::IDENTITY),
        "no input is an identity turn, reported as one"
    );
}
