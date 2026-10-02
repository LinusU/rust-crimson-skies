//! F21-C: the scripted camera request and its refusals.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-C`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! These tests drive `cs_app::camera::script` (the request record) and
//! `cs_app::camera::session` (the install boundary). They assert the two
//! disciplines a scripted camera has to keep: a refused request changes
//! nothing so the producer can retry it, and a script runs over a **bounded,
//! half-open** span of ticks so two shots can never both claim a frame.

use cs_app::camera::{
    CameraEvent, ScriptCameraError, ScriptCameraRequest, ScriptEndReason, ScriptSubject,
    ScriptedShot,
};
use cs_content::cameras::CameraModeKind;
use cs_types::Tick;
use cs_types::content::ContentKind;

use crate::common::{actor, authored_session, camera_track, fixture_session, mission};

/// A request over ticks 10..20 that frames the player.
///
/// The span is half-open, so a frame at 20 belongs to whatever comes next.
fn request() -> ScriptCameraRequest {
    ScriptCameraRequest::follows(
        camera_track("synthetic.intro.pan"),
        Tick(10),
        Tick(20),
        ScriptSubject::Player,
    )
    .expect("the designed request is valid")
}

#[test]
fn accept_f21_c_a_script_request_needs_a_camera_track_id_and_a_non_empty_span() {
    // A mission id is a stable typed key for a mission. Putting one where the
    // camera's identity belongs would make one id address two unrelated
    // records, which is exactly what the `UI-NETWORK` id rule forbids.
    let wrong_kind =
        ScriptCameraRequest::follows(mission("m01"), Tick(10), Tick(20), ScriptSubject::Player);
    assert_eq!(
        wrong_kind,
        Err(ScriptCameraError::NotACameraTrack { id: mission("m01") }),
        "a mission id is not an authored camera"
    );
    assert_eq!(mission("m01").kind(), ContentKind::Mission);
    assert_eq!(
        camera_track("synthetic.intro.pan").kind(),
        ContentKind::CameraTrack
    );

    // A span that covers no tick is a camera that never draws anything, which
    // is a typo, not a request.
    let empty = ScriptCameraRequest::follows(
        camera_track("synthetic.intro.pan"),
        Tick(20),
        Tick(20),
        ScriptSubject::Player,
    );
    assert_eq!(
        empty,
        Err(ScriptCameraError::EmptySpan {
            from: Tick(20),
            until: Tick(20)
        })
    );
    let backwards = ScriptCameraRequest::follows(
        camera_track("synthetic.intro.pan"),
        Tick(20),
        Tick(10),
        ScriptSubject::Player,
    );
    assert_eq!(
        backwards,
        Err(ScriptCameraError::EmptySpan {
            from: Tick(20),
            until: Tick(10)
        })
    );

    // Both refusals are values, so a caller can print them; and the messages
    // name what was wrong rather than "invalid request".
    assert!(
        wrong_kind
            .unwrap_err()
            .to_string()
            .contains("authored camera track")
    );
    assert!(empty.unwrap_err().to_string().contains("covers none"));
}

#[test]
fn accept_f21_c_a_script_span_is_half_open_so_two_shots_never_claim_one_frame() {
    let request = request();
    assert!(!request.covers(Tick(9)), "before the span");
    assert!(request.covers(Tick(10)), "at the first tick");
    assert!(request.covers(Tick(19)), "at the last tick");
    assert!(
        !request.covers(Tick(20)),
        "the frame at `until` belongs to the next shot"
    );
    assert_eq!(request.from(), Tick(10));
    assert_eq!(request.until(), Tick(20));
    assert_eq!(request.camera(), &camera_track("synthetic.intro.pan"));
    assert_eq!(
        request.subject_of(actor(1)),
        Some(actor(1)),
        "the player role resolves to whoever the producer says is flying"
    );
}

#[test]
fn accept_f21_c_a_mode_set_with_no_authored_sequence_refuses_a_script() {
    // The synthetic fixture declares a cockpit, an external view and a
    // spyglass and nothing else. F21-B left this refusal in place by name, and
    // F21-C is where it is spent: a scripted camera with no declared
    // projection, placement or magnification must not silently behave like the
    // chase view.
    let mut session = fixture_session();
    assert!(
        session
            .rig()
            .modes()
            .get(CameraModeKind::AuthoredSequence)
            .is_none(),
        "the fixture declares no authored sequence, which is the case under test"
    );
    let refusal = session
        .request_script(request())
        .expect_err("no authored mode");
    assert_eq!(
        refusal,
        ScriptCameraError::NoAuthoredMode {
            camera: camera_track("synthetic.intro.pan")
        }
    );
    assert!(
        session.script().is_none(),
        "a refused request installed nothing"
    );

    // The same request installs against a set that declares one, which is what
    // makes the refusal about the *set* and not about the request.
    let mut ready = authored_session();
    ready
        .request_script(request())
        .expect("the set declares one");
    assert_eq!(ready.script(), Some(&request()));
}

#[test]
fn accept_f21_c_a_second_script_is_refused_while_one_drives_and_installs_after_teardown() {
    let mut session = authored_session();
    session.request_script(request()).expect("installed");

    let second = ScriptCameraRequest::pinned(
        camera_track("synthetic.intro.close"),
        Tick(11),
        Tick(19),
        crate::common::origin_pose(),
    )
    .expect("valid");
    assert_eq!(
        session.request_script(second.clone()),
        Err(ScriptCameraError::AlreadyActive {
            active: camera_track("synthetic.intro.pan"),
            requested: camera_track("synthetic.intro.close"),
        })
    );
    assert_eq!(
        session.script(),
        Some(&request()),
        "the refused request did not disturb the running one"
    );

    // Teardown: the released request is handed back, and the next one installs.
    let released = session.release_script().expect("something was released");
    assert_eq!(released, request());
    assert!(session.script().is_none());
    session
        .request_script(second.clone())
        .expect("the camera is free again");
    assert_eq!(session.script(), Some(&second));

    // Releasing nothing is not an error.
    let released = session.release_script().expect("released");
    assert_eq!(released, second);
    assert_eq!(session.release_script(), None);
}

#[test]
fn accept_f21_c_a_capture_that_pins_a_view_and_a_script_cannot_both_claim_a_frame() {
    let mut session = authored_session();
    session
        .request_script(request())
        .expect("the script is installed");

    // A capture for a tick *inside* the script's span that also names a view
    // makes two claims on one frame. Refused, and the script keeps running.
    let inside = cs_app::camera::CaptureRequest::with_rig(
        mission("m01"),
        Tick(15),
        cs_app::camera::ViewRig::Chase,
        None,
    )
    .expect("the capture request is valid");
    assert_eq!(
        session.apply_capture(inside),
        Err(cs_app::camera::CaptureError::ScriptCoversTick {
            camera: camera_track("synthetic.intro.pan"),
            tick: Tick(15)
        })
    );
    assert_eq!(session.script(), Some(&request()));

    // A capture for a tick outside the span coexists: one of them applies on
    // its own frame.
    let outside = cs_app::camera::CaptureRequest::with_rig(
        mission("m01"),
        Tick(30),
        cs_app::camera::ViewRig::Chase,
        None,
    )
    .expect("the capture request is valid");
    session.apply_capture(outside).expect("no overlap");
    assert_eq!(session.capture().map(|c| c.tick()), Some(Tick(30)));

    // And the other order is refused too: a script that would cover a pending
    // view-pinning capture's tick cannot be installed.
    let mut other = authored_session();
    other
        .apply_capture(
            cs_app::camera::CaptureRequest::with_rig(
                mission("m01"),
                Tick(15),
                cs_app::camera::ViewRig::Chase,
                None,
            )
            .expect("valid"),
        )
        .expect("installed with no script running");
    let overlapping = ScriptCameraRequest::follows(
        camera_track("synthetic.intro.pan"),
        Tick(10),
        Tick(20),
        ScriptSubject::Player,
    )
    .expect("valid");
    assert_eq!(
        other.request_script(overlapping),
        Err(ScriptCameraError::CapturePinsView { tick: Tick(15) })
    );
    assert!(other.script().is_none());
}

#[test]
fn accept_f21_c_a_script_can_hold_a_pinned_pose_which_binds_to_no_body() {
    let pinned = ScriptCameraRequest::pinned(
        camera_track("synthetic.intro.hold"),
        Tick(0),
        Tick(5),
        world_pose(),
    )
    .expect("valid");
    assert_eq!(pinned.shot(), ScriptedShot::Pinned { pose: world_pose() });
    assert_eq!(
        pinned.subject_of(actor(1)),
        None,
        "a pinned pose has no subject, and a consumer that asked must not be \
         handed one"
    );
    assert_eq!(
        ScriptSubject::Player.resolve(actor(7)),
        actor(7),
        "the player role follows whoever is flying"
    );
    assert_eq!(ScriptSubject::Actor(actor(3)).resolve(actor(7)), actor(3));
    assert!(ScriptSubject::Player.is_role());
    assert!(!ScriptSubject::Actor(actor(3)).is_role());
}

#[test]
fn accept_f21_c_a_script_teardown_ends_it_visibly_and_leaves_nothing_running() {
    let mut session = authored_session();
    session.request_script(request()).expect("installed");
    session.release_script();

    // Every end has a name a consumer can print, so a report is readable
    // without matching on the enum's shape.
    let reasons = [
        ScriptEndReason::SpanEnded { at: Tick(20) },
        ScriptEndReason::SubjectGone { actor: actor(3) },
        ScriptEndReason::Released,
        ScriptEndReason::SessionEnded,
    ];
    for reason in reasons {
        assert!(!reason.to_string().is_empty(), "{reason} is reportable");
    }

    // A session generation that ends with a script still running names it,
    // because the next generation must not inherit the old pilot's camera.
    session
        .request_script(request())
        .expect("installed for the teardown");
    let events = session.reset();
    assert_eq!(
        events,
        vec![CameraEvent::ScriptEnded {
            camera: camera_track("synthetic.intro.pan"),
            reason: ScriptEndReason::SessionEnded,
        }],
        "a teardown that reports nothing is a teardown nobody can see"
    );
    assert!(
        session.script().is_none(),
        "the next session generation opens with no script"
    );
    assert!(
        session.reset().is_empty(),
        "tearing down twice reports nothing"
    );
}

fn world_pose() -> cs_app::camera::CameraPose {
    crate::common::aircraft_pose([100.0, 20.0, -300.0], cs_types::space::Quaternion::IDENTITY)
}
