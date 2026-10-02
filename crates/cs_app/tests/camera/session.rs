//! F21-C: the camera session, and AC03 — "swap aircraft during a scripted
//! capture and verify the camera binds to the new player body".
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-C`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! These tests drive `cs_app::camera::session` over the real F21-A records and
//! the real F21-B rigs: a scripted camera places its eye through the same
//! `rig::oriented_pose` the rigs use, a capture pins the projection derived from
//! the frame's own lowered policy, and the player's frames come out of a real
//! `CameraRig`. Nothing here re-implements a camera.
//!
//! What the scenario asserts is the *seam*, not a rig: an `ActorId` is
//! generation-qualified, so an aircraft swap is a different camera binding, and
//! the session is what turns that into a named rebound, a reseat and a capture
//! that draws through the new body.

use std::time::Duration;

use cs_app::camera::{
    BodyPose, CameraAuthority, CameraEvent, CameraSession, CaptureError, CaptureOverride,
    CaptureRequest, RigAimError, RigError, ScriptCameraRequest, ScriptEndReason, ScriptSubject,
    SessionError, SessionFrameInputs, ViewRig,
};
use cs_app::origin::OriginChange;
use cs_app::targeting::{SpyglassReadout, SpyglassTarget};
use cs_content::cameras::{AspectRatio, CameraModeKind};
use cs_sim::targeting::{Allegiance, TargetClass};
use cs_types::Tick;
use cs_types::space::{Meters, Quaternion};

use crate::common::{
    actor, aircraft_pose, assert_close, assert_close_position, authored_session, camera_track,
    mission, origin_pose, spyglass_authored_session, world,
};

/// The frame rate the scenario runs at. A fixed one keeps the smoothing law out
/// of the assertions: what is under test is the *binding*, not the lag.
const FRAME: Duration = Duration::from_millis(16);

/// A request that frames the player's aircraft over ticks 10..40.
fn follow_request() -> ScriptCameraRequest {
    ScriptCameraRequest::follows(
        camera_track("synthetic.intro.pan"),
        Tick(10),
        Tick(40),
        ScriptSubject::Player,
    )
    .expect("the designed request is valid")
}

/// The bodies a frame publishes: the player body and, optionally, another one.
fn bodies(player: u64, player_at: [f64; 3], extra: Option<(u64, [f64; 3])>) -> Vec<BodyPose> {
    let mut published = vec![BodyPose::new(
        actor(player),
        aircraft_pose(player_at, Quaternion::IDENTITY),
    )];
    if let Some((serial, position)) = extra {
        published.push(BodyPose::new(
            actor(serial),
            aircraft_pose(position, Quaternion::IDENTITY),
        ));
    }
    published
}

/// One frame's inputs with the F21-C producer fields filled in.
fn inputs<'a>(
    at: u64,
    player: u64,
    bodies: &'a [BodyPose],
    spyglass: Option<&'a SpyglassReadout>,
) -> SessionFrameInputs<'a> {
    SessionFrameInputs {
        at: Tick(at),
        player: actor(player),
        bodies,
        aspect: AspectRatio::FOUR_THREE,
        elapsed: FRAME,
        look: None,
        spyglass,
        origin_change: OriginChange::Rebase,
    }
}

/// A published spyglass target at `position`, which is how a producer's own
/// published view says "this is what the spyglass would magnify".
fn target_on(actor: cs_sim::damage::ActorId, position: [f64; 3]) -> SpyglassTarget {
    SpyglassTarget {
        actor,
        class: TargetClass::Aircraft,
        allegiance: Some(Allegiance::Hostile),
        hostile: true,
        threatening: false,
        objective: false,
        position: world(position),
        distance: Meters(0.0),
    }
}

/// AC03: the player's aircraft is swapped in the middle of a scripted capture.
///
/// This is the stage's minimum scenario, and it has two halves that must both
/// hold or the scenario is vacuous:
///
/// 1. a script that frames **the player role** re-resolves its subject every
///    frame, so the swap rebinds the camera in the same frame, names the
///    rebound, and reseats instead of dragging the camera across the world;
/// 2. the capture taken afterwards draws through the **new** body, with a
///    report that names the pose, aspect and frustum it pinned.
#[test]
fn accept_f21_c_swapping_aircraft_during_a_scripted_capture_rebinds_to_the_new_body() {
    let mut session = authored_session();
    session
        .request_script(follow_request())
        .expect("the set declares an authored sequence");
    // A capture of the mission, at a pose and aspect of its own, taken five
    // frames after the swap.
    let capture_pose = aircraft_pose([500.0, 40.0, -900.0], Quaternion::IDENTITY);
    let capture = CaptureRequest::build(
        mission("m01"),
        Tick(30),
        None,
        Some(capture_pose),
        Some(AspectRatio::SIXTEEN_NINE),
        Some(cs_app::render::capture::ComparisonSettings::comparison()),
    )
    .expect("every input names a valid value");
    session.apply_capture(capture).expect("installed");

    // The first body, at the origin. The scripted camera frames it and the
    // first frame reports that the script started.
    let first = bodies(1, [0.0, 0.0, 0.0], None);
    let frame = session
        .frame(&inputs(20, 1, &first, None))
        .expect("a scripted frame");
    assert_eq!(
        frame.authority,
        CameraAuthority::Script {
            camera: camera_track("synthetic.intro.pan"),
            subject: Some(actor(1))
        },
        "the script is driving and it says which camera"
    );
    assert!(frame.is_scripted());
    assert!(
        frame.rig.is_none(),
        "a scripted frame is not a rig frame, and a consumer cannot mistake it"
    );
    assert!(frame.carries(&CameraEvent::ScriptStarted {
        camera: camera_track("synthetic.intro.pan"),
        subject: Some(actor(1))
    }));
    assert_eq!(frame.view.subject, Some(actor(1)));
    assert_eq!(frame.view.mode, CameraModeKind::AuthoredSequence);

    // The swap: the player now flies body 2, far from body 1, and body 1 is
    // gone from the frame. An `ActorId` is generation-qualified, so this is a
    // different camera binding and not a moved one.
    let second = bodies(2, [4_000.0, 0.0, 0.0], None);
    let rebound = session
        .frame(&inputs(25, 2, &second, None))
        .expect("the swap frame");
    assert_eq!(
        rebound.view.subject,
        Some(actor(2)),
        "the camera is bound to the body the player flies now"
    );
    assert!(
        rebound.carries(&CameraEvent::SubjectRebound {
            from: actor(1),
            to: actor(2)
        }),
        "the swap is named, not inferred: {:?}",
        rebound.events
    );
    assert!(
        rebound.view.smoothing.is_settled(),
        "a rebound reseats rather than dragging the camera across the world"
    );
    // The camera really moved to the new body: the scripted placement is the
    // declared one (3 m up, 12 m astern of a body facing -Z), so the eye sits
    // at the new body's position with that offset.
    assert_close_position(
        rebound.view.pose.position(),
        [4_000.0, 3.0, 12.0],
        1e-9,
        "the scripted eye is the declared offset from the new body",
    );

    // The capture, on its own tick, through the new body.
    let taken = session
        .frame(&inputs(30, 2, &second, None))
        .expect("the capture frame");
    let report = taken.capture.as_ref().expect("the capture was taken");
    assert_eq!(report.tick(), Tick(30));
    assert_eq!(report.rig(), None, "a scripted frame is not a player view");
    assert_eq!(
        report.pose(),
        capture_pose,
        "the report names the exact pose it drew from"
    );
    assert_eq!(report.aspect(), AspectRatio::SIXTEEN_NINE);
    assert!(
        report.override_of("world_pose").is_some(),
        "the pinned pose is reported"
    );
    // And the frame after it is an ordinary scripted frame again: the capture
    // was one frame, not a mode.
    let after = session
        .frame(&inputs(31, 2, &second, None))
        .expect("the frame after the capture");
    assert!(after.capture.is_none());
    assert_eq!(after.view.subject, Some(actor(2)));
    assert_close_position(
        after.view.pose.position(),
        [4_000.0, 3.0, 12.0],
        1e-9,
        "the capture did not leave the live camera pinned",
    );
}

#[test]
fn accept_f21_c_a_script_naming_the_destroyed_body_hands_the_camera_back_in_the_same_frame() {
    let mut session = authored_session();
    let request = ScriptCameraRequest::follows(
        camera_track("synthetic.intro.wingman"),
        Tick(0),
        Tick(100),
        // A specific body, not the player role: this shot does not follow
        // whoever is flying, it follows one actor.
        ScriptSubject::Actor(actor(9)),
    )
    .expect("valid");
    session.request_script(request).expect("installed");

    // Wingman 9 is in the frame; the script frames it.
    let with_wingman = bodies(1, [0.0, 0.0, 0.0], Some((9, [600.0, 0.0, 0.0])));
    let frame = session
        .frame(&inputs(5, 1, &with_wingman, None))
        .expect("a scripted frame");
    assert_eq!(frame.authority.subject(), Some(actor(9)));
    assert_close_position(
        frame.view.pose.position(),
        [600.0, 3.0, 12.0],
        1e-9,
        "the eye is the declared offset from the wingman",
    );

    // Wingman 9 is destroyed: it leaves the frame and never comes back. The
    // camera must not keep drawing as though it could. The frame that notices
    // is the frame that reports the end *and* hands the camera back.
    let alone = bodies(1, [0.0, 0.0, 0.0], None);
    let after = session
        .frame(&inputs(6, 1, &alone, None))
        .expect("the frame the destruction is noticed on");
    assert_eq!(
        after.authority,
        CameraAuthority::Player {
            rig: ViewRig::Cockpit
        },
        "the player takes the camera back on the same frame"
    );
    assert!(
        after.carries(&CameraEvent::ScriptEnded {
            camera: camera_track("synthetic.intro.wingman"),
            reason: ScriptEndReason::SubjectGone { actor: actor(9) }
        }),
        "the end is reported with its reason: {:?}",
        after.events
    );
    assert!(
        session.script().is_none(),
        "the script is not left running on a body that is gone"
    );
    assert_eq!(after.view.subject, Some(actor(1)));
    assert!(
        after.rig.is_some(),
        "the player's own frame carries the rig detail"
    );

    // And the next frame is ordinary: no lingering follow, no re-bind.
    let next = session
        .frame(&inputs(7, 1, &alone, None))
        .expect("the frame after");
    assert!(next.events.is_empty(), "{:?}", next.events);
    assert_eq!(
        next.authority,
        CameraAuthority::Player {
            rig: ViewRig::Cockpit
        }
    );
}

#[test]
fn accept_f21_c_a_script_span_ends_by_itself_and_returns_the_camera_with_a_reason() {
    let mut session = authored_session();
    session
        .request_script(
            ScriptCameraRequest::follows(
                camera_track("synthetic.intro.pan"),
                Tick(10),
                Tick(12),
                ScriptSubject::Player,
            )
            .expect("valid"),
        )
        .expect("installed");

    let published = bodies(1, [0.0, 0.0, 0.0], None);
    let scripted = session
        .frame(&inputs(11, 1, &published, None))
        .expect("inside the span");
    assert!(scripted.is_scripted());

    // The frame at `until` is half-open: the span is over, so the camera has
    // been handed back and the frame says so and why.
    let ended = session
        .frame(&inputs(12, 1, &published, None))
        .expect("the frame after the span");
    assert_eq!(
        ended.authority,
        CameraAuthority::Player {
            rig: ViewRig::Cockpit
        }
    );
    assert!(ended.carries(&CameraEvent::ScriptEnded {
        camera: camera_track("synthetic.intro.pan"),
        reason: ScriptEndReason::SpanEnded { at: Tick(12) }
    }));
    assert!(session.script().is_none());
}

#[test]
fn accept_f21_c_a_pinned_script_pose_is_held_exactly_and_binds_to_no_body() {
    let mut session = authored_session();
    let pose = aircraft_pose([1_000.0, 250.0, -2_000.0], Quaternion::IDENTITY);
    session
        .request_script(
            ScriptCameraRequest::pinned(
                camera_track("synthetic.intro.hold"),
                Tick(0),
                Tick(5),
                pose,
            )
            .expect("valid"),
        )
        .expect("installed");

    let published = bodies(1, [0.0, 0.0, 0.0], None);
    let frame = session
        .frame(&inputs(1, 1, &published, None))
        .expect("a pinned frame");
    assert_eq!(
        frame.authority.subject(),
        None,
        "a pinned pose has no subject"
    );
    assert_eq!(
        frame.view.pose, pose,
        "a pinned pose is held exactly, with no lag"
    );
    assert!(frame.view.smoothing.is_settled());

    // A body moving under a pinned camera does not move the camera.
    let moved = bodies(1, [9_000.0, 0.0, 0.0], None);
    let still = session
        .frame(&inputs(2, 1, &moved, None))
        .expect("a pinned frame");
    assert_eq!(
        still.view.pose, pose,
        "the script owns the camera, not the aircraft"
    );
}

#[test]
fn accept_f21_c_a_capture_applies_to_exactly_its_tick_and_a_missed_one_is_retired() {
    let mut session = authored_session();
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    let pinned = aircraft_pose([10.0, 20.0, -30.0], Quaternion::IDENTITY);
    session
        .apply_capture(
            CaptureRequest::build(mission("m01"), Tick(3), None, Some(pinned), None, None)
                .expect("valid"),
        )
        .expect("installed");

    // Before its tick: the camera is the live camera and nothing is reported.
    let before = session
        .frame(&inputs(2, 1, &published, None))
        .expect("a frame before the capture");
    assert!(before.capture.is_none());
    assert!(
        !before
            .view
            .pose
            .position()
            .to_array()
            .eq(&[10.0, 20.0, -30.0])
    );

    // On its tick: the pinned pose, and the report says the smoothing was
    // bypassed and the pose held exactly.
    let on_tick = session
        .frame(&inputs(3, 1, &published, None))
        .expect("the capture frame");
    let report = on_tick.capture.as_ref().expect("taken");
    assert_eq!(report.pose(), pinned);
    assert_eq!(
        report.override_of("smoothing"),
        Some(&CaptureOverride::Smoothing { bypassed: true })
    );
    assert_eq!(on_tick.view.pose, pinned);
    assert_eq!(on_tick.rig.expect("a rig frame").pose, pinned);

    // A capture is one frame: the next one is the live camera again, carrying on
    // from where it was rather than from the pinned pose, and nothing is
    // reported — a capture that lingered would have to be retired as missed.
    let after = session
        .frame(&inputs(4, 1, &published, None))
        .expect("the frame after the capture");
    assert!(after.capture.is_none());
    assert!(
        after.events.is_empty(),
        "a capture that was taken is not also missed: {:?}",
        after.events
    );
    assert!(session.capture().is_none());

    // A capture whose tick has already passed is retired with an event, not
    // held forever against every later mission.
    session
        .apply_capture(CaptureRequest::new(mission("m01"), Tick(2)).expect("valid"))
        .expect("installed for a frame already gone");
    let missed = session
        .frame(&inputs(50, 1, &published, None))
        .expect("the frame that finds the capture late");
    assert!(missed.capture.is_none());
    assert!(missed.carries(&CameraEvent::CaptureMissed {
        requested: Tick(2),
        at: Tick(50)
    }));
    assert!(session.capture().is_none());
}

#[test]
fn accept_f21_c_a_capture_that_pins_a_view_switches_it_for_one_frame_and_restores_it() {
    let mut session = authored_session();
    // The session starts in the cockpit. A capture that pins the chase view
    // must draw through the chase view and hand the cockpit back afterwards.
    assert_eq!(session.view_rig(), ViewRig::Cockpit);
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    session
        .apply_capture(
            CaptureRequest::with_rig(
                mission("m01"),
                Tick(1),
                ViewRig::Chase,
                Some(cs_app::render::capture::ComparisonSettings::comparison()),
            )
            .expect("valid"),
        )
        .expect("the chase view is declared");

    let taken = session
        .frame(&inputs(1, 1, &published, None))
        .expect("the capture frame");
    assert_eq!(
        taken.authority,
        CameraAuthority::Player {
            rig: ViewRig::Chase
        },
        "the capture drew through the view it pinned"
    );
    let report = taken.capture.as_ref().expect("taken");
    assert_eq!(report.rig(), Some(ViewRig::Chase));
    assert_eq!(
        report.override_of("rig"),
        Some(&CaptureOverride::Rig {
            requested: ViewRig::Chase,
            restored: ViewRig::Cockpit
        }),
        "the report names the view it went back to"
    );

    // Restored on the very next frame.
    let after = session
        .frame(&inputs(2, 1, &published, None))
        .expect("the frame after");
    assert_eq!(
        after.authority,
        CameraAuthority::Player {
            rig: ViewRig::Cockpit
        }
    );
    assert_eq!(session.view_rig(), ViewRig::Cockpit);
}

#[test]
fn accept_f21_c_a_capture_pinning_an_undeclared_view_is_refused_at_the_boundary() {
    let mut session = authored_session();
    // The session has no spyglass view, so a capture that pins one is refused
    // when it is installed — before the frame it would have been drawn on — and
    // nothing is left half-installed.
    let refusal = session.apply_capture(
        CaptureRequest::with_rig(mission("m01"), Tick(1), ViewRig::Spyglass, None).expect("valid"),
    );
    assert_eq!(
        refusal,
        Err(CaptureError::UndeclaredRig {
            rig: ViewRig::Spyglass,
            kind: "spyglass"
        }),
        "the refusal names the view and the mode kind it needed"
    );
    assert!(session.capture().is_none());
    assert_eq!(session.view_rig(), ViewRig::Cockpit);

    // A second pending capture is refused, and the first stays pending.
    session
        .apply_capture(CaptureRequest::new(mission("m01"), Tick(5)).expect("valid"))
        .expect("installed");
    assert_eq!(
        session.apply_capture(CaptureRequest::new(mission("m01"), Tick(6)).expect("valid")),
        Err(CaptureError::AlreadyPending {
            requested: Tick(6),
            pending: Tick(5)
        })
    );
    assert_eq!(session.capture().map(|c| c.tick()), Some(Tick(5)));
}

#[test]
fn accept_f21_c_a_frame_refusal_tears_down_nothing_and_the_next_frame_retries() {
    let mut session = authored_session();
    session.request_script(follow_request()).expect("installed");
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    // Establish a scripted frame first, so there is state that could be lost.
    let _ = session
        .frame(&inputs(20, 1, &published, None))
        .expect("a scripted frame");

    // A frame whose producer published no pose for the body the player claims
    // to fly cannot be drawn: the camera would be placed with no authority
    // behind it. It is a producer that has not finished the frame, not a death,
    // so it must not be mistaken for one.
    let empty: Vec<BodyPose> = Vec::new();
    let refusal = session
        .frame(&inputs(21, 1, &empty, None))
        .expect_err("no pose for the player body");
    assert_eq!(refusal, SessionError::PlayerPoseMissing { actor: actor(1) });
    assert!(refusal.to_string().contains("published no pose"));

    // Nothing was torn down: the script is still installed, and the next frame
    // with a published pose draws normally.
    assert!(
        session.script().is_some(),
        "a frame error is not a teardown"
    );
    let recovered = session
        .frame(&inputs(22, 1, &published, None))
        .expect("the next frame retries with the same code");
    assert!(recovered.is_scripted());
    assert_eq!(recovered.view.subject, Some(actor(1)));
}

#[test]
fn accept_f21_c_a_frame_refusal_still_reports_the_rebound_the_swap_caused() {
    // A frame can be refused *after* the session has already seen something the
    // consumer must learn about. Here the player swaps aircraft and the rig then
    // refuses the frame, because the target targeting published sits on the
    // camera's own eye and cannot be aimed at (F21-B's rule, owned by the rig).
    // The rebound is a fact about the world that has already happened and the
    // session has already recorded it; dropping it with the refused frame would
    // leave a consumer that skipped a frame — a load spike, a paused renderer —
    // with a camera bound to a body it was never told about.
    let mut session = spyglass_authored_session();
    session
        .select_rig(ViewRig::Spyglass)
        .expect("the set declares a spyglass");
    let first = bodies(1, [0.0, 0.0, 0.0], None);
    let clear = SpyglassReadout {
        at: Tick(5),
        target: None,
        cleared: None,
    };
    session
        .frame(&inputs(5, 1, &first, Some(&clear)))
        .expect("a spyglass frame with nothing selected");

    // The swap, on a frame the rig refuses: the new body's eye is at the body's
    // own position and the published target sits exactly there.
    let second = bodies(2, [4_000.0, 0.0, 0.0], None);
    let on_the_eye = SpyglassReadout {
        at: Tick(6),
        target: Some(target_on(actor(9), [4_000.0, 0.0, 0.0])),
        cleared: None,
    };
    let refusal = session
        .frame(&inputs(6, 2, &second, Some(&on_the_eye)))
        .expect_err("a target on the camera's own eye cannot be aimed at");
    assert_eq!(
        refusal,
        SessionError::Rig(RigError::UnaimableTarget {
            actor: actor(9),
            reason: RigAimError::CoincidentWithCamera,
        }),
        "the refusal is the rig's own and it tears down nothing"
    );

    // The rebound is not lost with the refused frame: the session has already
    // moved its binding, so it cannot report it again later.
    let recovered = session
        .frame(&inputs(7, 2, &second, None))
        .expect("the next frame retries with the same code");
    assert_eq!(recovered.view.subject, Some(actor(2)));
    assert!(
        recovered.carries(&CameraEvent::SubjectRebound {
            from: actor(1),
            to: actor(2)
        }),
        "the swap the refused frame had already seen is reported once: {:?}",
        recovered.events
    );
    let after = session
        .frame(&inputs(8, 2, &second, None))
        .expect("the frame after");
    assert!(after.events.is_empty(), "and only once: {:?}", after.events);
}

#[test]
fn accept_f21_c_a_script_that_ends_on_a_refused_frame_is_still_reported() {
    // The second way a frame can lose an event: the script's span runs out, the
    // session ends it and hands the camera back — and *then* the player's rig
    // refuses the frame, because the producer published no pose for the body it
    // says the player flies. The end has already happened and the script is
    // already gone; if the event died with the refused frame, no consumer would
    // ever learn that the camera came back.
    let mut session = authored_session();
    session.request_script(follow_request()).expect("installed");
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    session
        .frame(&inputs(39, 1, &published, None))
        .expect("the last scripted frame");

    let empty: Vec<BodyPose> = Vec::new();
    let refusal = session
        .frame(&inputs(40, 1, &empty, None))
        .expect_err("no pose for the player body");
    assert_eq!(refusal, SessionError::PlayerPoseMissing { actor: actor(1) });
    assert!(
        session.script().is_none(),
        "the span is over whatever the frame could draw"
    );

    let after = session
        .frame(&inputs(41, 1, &published, None))
        .expect("the next frame");
    assert_eq!(
        after.authority,
        CameraAuthority::Player {
            rig: ViewRig::Cockpit
        }
    );
    assert!(
        after.carries(&CameraEvent::ScriptEnded {
            camera: camera_track("synthetic.intro.pan"),
            reason: ScriptEndReason::SpanEnded { at: Tick(40) },
        }),
        "the end is reported on the first frame that can report it, and it \
         names the tick that discovered it: {:?}",
        after.events
    );
    let next = session
        .frame(&inputs(42, 1, &published, None))
        .expect("the frame after");
    assert!(next.events.is_empty(), "{:?}", next.events);
}

#[test]
fn accept_f21_c_releasing_a_running_script_reports_the_end_it_caused() {
    // The teardown path a producer uses mid-cinematic. It is reported on the
    // next frame, because a camera that quietly stops being a script is exactly
    // the silent handover this seam exists to rule out.
    let mut session = authored_session();
    session.request_script(follow_request()).expect("installed");
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    session
        .frame(&inputs(20, 1, &published, None))
        .expect("a scripted frame");

    let released = session.release_script().expect("something was released");
    assert_eq!(
        released,
        follow_request(),
        "the release hands the request back"
    );
    let after = session
        .frame(&inputs(21, 1, &published, None))
        .expect("the frame after the release");
    assert_eq!(
        after.authority,
        CameraAuthority::Player {
            rig: ViewRig::Cockpit
        }
    );
    assert!(
        after.carries(&CameraEvent::ScriptEnded {
            camera: camera_track("synthetic.intro.pan"),
            reason: ScriptEndReason::Released,
        }),
        "a release is an end like any other: {:?}",
        after.events
    );
    let next = session
        .frame(&inputs(22, 1, &published, None))
        .expect("the frame after");
    assert!(next.events.is_empty(), "{:?}", next.events);

    // A request installed and released before it ever drew is not an end of
    // anything: it never drove the camera, so there is no handover to report and
    // `ScriptStarted` is not left unmatched.
    let mut quiet = authored_session();
    quiet.request_script(follow_request()).expect("installed");
    assert!(quiet.release_script().is_some());
    let frame = quiet
        .frame(&inputs(20, 1, &published, None))
        .expect("an ordinary player frame");
    assert!(
        frame.events.is_empty(),
        "a script that never drove has no end to report: {:?}",
        frame.events
    );
}

#[test]
fn accept_f21_c_the_session_writes_nothing_back_to_what_it_read() {
    // The camera is a consumer: the frame inputs are values, and drawing a
    // frame — scripted or captured — changes none of them. The published
    // spyglass view in particular is read through a borrow, so a rig that
    // "helped" by updating it would be changing targeting's output.
    let mut session = authored_session();
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    let readout = cs_app::targeting::SpyglassReadout {
        at: Tick(1),
        target: None,
        cleared: None,
    };
    let before_bodies = published.clone();
    let before_readout = readout.clone();

    session
        .request_script(
            ScriptCameraRequest::pinned(
                camera_track("synthetic.intro.hold"),
                Tick(0),
                Tick(10),
                origin_pose(),
            )
            .expect("valid"),
        )
        .expect("installed");
    let _ = session
        .frame(&inputs(1, 1, &published, Some(&readout)))
        .expect("a frame");

    assert_eq!(published, before_bodies, "the input bodies are unchanged");
    assert_eq!(
        readout, before_readout,
        "the published spyglass view is unchanged"
    );
    // And the session exposes no handle into either: it is a value in, a value
    // out.
    let _ = session.clone().frame(&inputs(2, 1, &published, None));
}

#[test]
fn accept_f21_c_a_session_reset_ends_the_script_and_drops_the_pending_capture() {
    let mut session: CameraSession = authored_session();
    session.request_script(follow_request()).expect("installed");
    session
        .apply_capture(CaptureRequest::new(mission("m01"), Tick(900)).expect("valid"))
        .expect("installed");
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    let _ = session
        .frame(&inputs(20, 1, &published, None))
        .expect("a scripted frame");

    // A new session generation must not open on the previous pilot's camera, a
    // script from a mission that has ended, or a capture for a tick in a
    // mission that is no longer running — and the teardown names both what it
    // forced and what it dropped, because a teardown nobody can see is not a
    // teardown.
    let events = session.reset();
    assert_eq!(
        events,
        vec![
            CameraEvent::ScriptEnded {
                camera: camera_track("synthetic.intro.pan"),
                reason: ScriptEndReason::SessionEnded
            },
            CameraEvent::CaptureDiscarded {
                requested: Tick(900)
            },
        ],
        "the capture is named too: it is a promise the producer still believes \
         in and this teardown will never keep"
    );
    assert!(session.script().is_none());
    assert!(session.capture().is_none());
    assert!(
        session.bound().is_none(),
        "the camera is bound to nothing yet"
    );
    assert_eq!(session.rig().subject(), None);

    // The first frame of the new generation is an ordinary player frame that
    // reseats, because the smoother was cleared with the rest.
    let fresh = session
        .frame(&inputs(1, 1, &published, None))
        .expect("the new generation's first frame");
    assert_eq!(
        fresh.authority,
        CameraAuthority::Player {
            rig: ViewRig::Cockpit
        }
    );
    assert!(fresh.events.is_empty());
}

#[test]
fn accept_f21_c_the_player_can_switch_views_through_the_session() {
    // The ordinary producer path into F21-B's rigs: the session owns the rig
    // and forwards the selection, refusing one the mode set does not declare.
    let mut session = authored_session();
    session
        .select_rig(ViewRig::Chase)
        .expect("the chase view is declared");
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    let frame = session
        .frame(&inputs(1, 1, &published, None))
        .expect("a chase frame");
    assert_eq!(
        frame.authority,
        CameraAuthority::Player {
            rig: ViewRig::Chase
        }
    );
    // A free look is the rig that is already up, turned.
    session
        .select_rig(ViewRig::Look)
        .expect("look is available here");
    assert!(session.rig().is_looking());
    // There is no spyglass in this set, so it refuses by name — and a refused
    // selection changes nothing, which is the state the session was in before.
    let looking_before = session.rig().is_looking();
    assert!(session.select_rig(ViewRig::Spyglass).is_err());
    assert_eq!(
        session.rig().is_looking(),
        looking_before,
        "a refused selection changed nothing"
    );
    assert_eq!(
        session.view_rig(),
        ViewRig::Look,
        "and the view is still the one that was up"
    );
}

#[test]
fn accept_f21_c_a_scripted_camera_uses_the_declared_authored_sequence_projection() {
    // A scripted camera must not invent a frustum: it runs under the owner's
    // declared authored-sequence mode, so its projection and magnification are
    // the declared ones and a report can name them.
    let mut session = authored_session();
    session.request_script(follow_request()).expect("installed");
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    let frame = session
        .frame(&inputs(20, 1, &published, None))
        .expect("a scripted frame");
    let declared = session
        .rig()
        .modes()
        .get(CameraModeKind::AuthoredSequence)
        .expect("the set declares one");
    assert_eq!(frame.view.projection, declared.projection());
    assert_eq!(frame.view.magnification, declared.magnification());
    assert_eq!(frame.view.aspect, AspectRatio::FOUR_THREE);
    assert_eq!(frame.view.mode, CameraModeKind::AuthoredSequence);
}

#[test]
fn accept_f21_c_a_scripted_camera_follows_its_declared_body_placement() {
    // The scripted eye comes from the *same* declared placement convention the
    // rigs use, through the same function: 3 m up and 12 m astern of a body
    // facing canonical forward. If the sign were flipped the eye would sit in
    // front of a body flying at it, framing empty sky — the exact defect the
    // F21-B review caught in the chase rig.
    let mut session = authored_session();
    session.request_script(follow_request()).expect("installed");
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    let frame = session
        .frame(&inputs(20, 1, &published, None))
        .expect("a scripted frame");
    assert_close_position(
        frame.view.pose.position(),
        [0.0, 3.0, 12.0],
        1e-9,
        "the scripted eye is astern and above",
    );
    // The body is in front of that eye, which is what makes it a chase framing
    // rather than a view of empty sky: `framing_of` refuses a point at or
    // behind the eye, so returning at all is the front-facing claim. It sits
    // below the centre because the eye is 3 m above a level view — the exact
    // offset the declared placement asks for, measured through the declared
    // frustum rather than asserted as "somewhere on screen".
    let half_tangent = (frame
        .view
        .projection
        .vertical_fov_at(AspectRatio::FOUR_THREE)
        .0
        * 0.5)
        .tan();
    let framing = frame
        .view
        .framing_of(world([0.0, 0.0, 0.0]))
        .expect("the body is in front of the scripted eye");
    assert_close(framing.x(), 0.0, 1e-12, "centred across the view");
    assert_close(
        framing.y(),
        -(3.0 / 12.0) / half_tangent,
        1e-9,
        "3 m below a level eye 12 m astern, through the declared frustum",
    );
    assert!(
        framing.is_inside(),
        "the framed body is inside the viewport, {framing}"
    );
}

#[test]
fn accept_f21_c_a_pending_script_keeps_the_player_camera_until_its_span_opens() {
    // A request installed before its span is pending, not active: the player
    // keeps the camera and nothing is reported until the script's first frame.
    let mut session = authored_session();
    session
        .request_script(
            ScriptCameraRequest::follows(
                camera_track("synthetic.intro.pan"),
                Tick(100),
                Tick(200),
                ScriptSubject::Player,
            )
            .expect("valid"),
        )
        .expect("installed");
    let published = bodies(1, [0.0, 0.0, 0.0], None);

    let early = session
        .frame(&inputs(50, 1, &published, None))
        .expect("a frame before the span");
    assert_eq!(
        early.authority,
        CameraAuthority::Player {
            rig: ViewRig::Cockpit
        }
    );
    assert!(
        early.events.is_empty(),
        "a pending script reports nothing yet"
    );
    assert!(session.script().is_some(), "but it is installed");

    let started = session
        .frame(&inputs(100, 1, &published, None))
        .expect("the first scripted frame");
    assert!(started.is_scripted());
    assert!(started.carries(&CameraEvent::ScriptStarted {
        camera: camera_track("synthetic.intro.pan"),
        subject: Some(actor(1))
    }));
}

#[test]
fn accept_f21_c_a_frame_respects_the_capture_aspect_and_projection_pin() {
    // The capture's aspect override flows into the frame the consumer draws,
    // not just into the report: the frame is framed at what the capture pinned.
    let mut session = authored_session();
    let published = bodies(1, [0.0, 0.0, 0.0], None);
    session
        .apply_capture(
            CaptureRequest::build(
                mission("m01"),
                Tick(1),
                None,
                None,
                Some(AspectRatio::SIXTEEN_NINE),
                None,
            )
            .expect("valid"),
        )
        .expect("installed");
    let frame = session
        .frame(&inputs(1, 1, &published, None))
        .expect("the capture frame");
    assert_eq!(frame.view.aspect, AspectRatio::SIXTEEN_NINE);
    let report = frame.capture.as_ref().expect("taken");
    assert_eq!(report.aspect(), AspectRatio::SIXTEEN_NINE);
    assert_close(
        f64::from(report.pinned().pinned().aspect_ratio()),
        AspectRatio::SIXTEEN_NINE.value(),
        1e-6,
        "the pinned aspect narrows the declared one",
    );
    // The report names the mission it is of, so an evidence run can say what it
    // captured without re-deriving it. This request pinned no pose, so the
    // report must say that too rather than leaving a reader to assume one.
    assert_eq!(report.mission(), &mission("m01"));
    assert_eq!(report.request().pose(), None);
    assert_eq!(
        report.override_of("world_pose"),
        None,
        "nothing was pinned, so nothing is reported as pinned"
    );
}
