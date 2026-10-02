//! F21-C: the deterministic capture request, the pinned projection and the
//! override report.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-C`, non-negotiable behavior 5: "Screenshot CLI accepts a
//! reproducible world pose, mission id, tick and deterministic settings;
//! report all overrides." Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! The four inputs behavior 5 names are asserted individually — a capture
//! that cannot name its mission, its tick, its pose or its settings is not a
//! reproducible capture — and the projection pin is asserted against the
//! frame's own lowered policy rather than against a formula written here, so
//! the test measures the bridge instead of restating it.

use cs_app::camera::{
    CameraPose, CaptureError, CaptureOverride, CaptureRequest, CaptureTarget, MagnificationPin,
    ProjectionPinError, ViewRig, pin,
};
use cs_app::render::capture::ComparisonSettings;
use cs_content::cameras::{AspectFraming, AspectRatio, FovAxis, Magnification, ProjectionPolicy};
use cs_types::Tick;
use cs_types::content::ContentKind;
use cs_types::space::{Meters, Quaternion, Radians};

use crate::common::{
    aircraft_pose, assert_close, declared_mode, known, lowered_set, mission, world,
};

/// The spyglass mode of the synthetic fixture: 20° vertical at 4:3, 1 m to
/// 20 km, magnified 4x.
fn spyglass_projection() -> cs_app::camera::LoweredProjection {
    let modes = lowered_set(
        vec![
            declared_mode(cs_content::cameras::CameraModeKind::Cockpit),
            declared_mode(cs_content::cameras::CameraModeKind::External),
            declared_mode(cs_content::cameras::CameraModeKind::Spyglass),
        ],
        cs_content::cameras::CameraModeKind::Cockpit,
    );
    modes
        .get(cs_content::cameras::CameraModeKind::Spyglass)
        .expect("the spyglass mode is declared")
        .projection()
}

/// The cockpit mode's projection: 60° vertical at 4:3, 0.1 m to 10 km, not
/// magnified.
fn cockpit_projection() -> cs_app::camera::LoweredProjection {
    let modes = lowered_set(
        vec![
            declared_mode(cs_content::cameras::CameraModeKind::Cockpit),
            declared_mode(cs_content::cameras::CameraModeKind::Spyglass),
        ],
        cs_content::cameras::CameraModeKind::Cockpit,
    );
    modes
        .get(cs_content::cameras::CameraModeKind::Cockpit)
        .expect("the cockpit mode is declared")
        .projection()
}

#[test]
fn accept_f21_c_a_capture_names_its_mission_and_refuses_one_that_is_not() {
    let request = CaptureRequest::new(mission("m01"), Tick(900)).expect("a mission id is named");
    assert_eq!(request.mission(), &mission("m01"));
    assert_eq!(request.mission().kind(), ContentKind::Mission);
    assert_eq!(request.tick(), Tick(900));
    assert_eq!(
        request.pose(),
        None,
        "a bare capture pins no pose, and a pose it did not name is one it \
         must not invent"
    );

    // An airframe id is a stable typed key too, but a capture of an airframe is
    // not a capture of a mission: the mission is the world, the rules and the
    // content set a frame was drawn in.
    let airframe = cs_types::content::ContentId::from_source(
        ContentKind::Airframe,
        crate::common::owner_subject()
            .as_str()
            .rsplit('/')
            .next()
            .expect("a key"),
    )
    .expect("a valid airframe id");
    assert_eq!(
        CaptureRequest::new(airframe.clone(), Tick(1)),
        Err(CaptureError::NotAMission { id: airframe })
    );
}

#[test]
fn accept_f21_c_a_capture_refuses_comparison_settings_that_are_not_the_fixed_set() {
    let pinned = ComparisonSettings::comparison();
    assert!(pinned.is_fixed(), "the comparison set is the fixed one");

    let request =
        CaptureRequest::with_rig(mission("m01"), Tick(12), ViewRig::Cockpit, Some(pinned))
            .expect("the fixed set is accepted");
    assert_eq!(request.settings(), Some(pinned));
    assert_eq!(request.rig(), Some(ViewRig::Cockpit));

    // An unpinned exposure is exactly what F17's own capture refuses, and a
    // comparison between two frames rendered under different exposures compares
    // nothing, so the camera boundary refuses it too rather than passing it on.
    let loose = ComparisonSettings::with_exposure(1.5);
    assert!(!loose.is_fixed());
    assert_eq!(
        CaptureRequest::with_rig(mission("m01"), Tick(12), ViewRig::Cockpit, Some(loose)),
        Err(CaptureError::SettingsNotFixed {
            exposure_bits: 1.5_f32.to_bits()
        }),
        "the refusal names the exact exposure it rejected"
    );
    for loose in [
        ComparisonSettings::with_msaa_samples(4),
        ComparisonSettings::with_shadows(true),
    ] {
        assert!(
            CaptureRequest::with_rig(mission("m01"), Tick(12), ViewRig::Cockpit, Some(loose))
                .is_err(),
            "an enhanced presentation is not a fixed comparison"
        );
    }
}

#[test]
fn accept_f21_c_the_pinned_capture_projection_is_derived_from_the_lowered_policy() {
    let policy = spyglass_projection();

    // The pin is the policy's own vertical field of view at the capture's
    // aspect, narrowed to `f32` — not a second, independently authored
    // frustum. `PreserveVertical` keeps it constant across aspects, so the
    // three pins differ only in their aspect.
    for aspect in [
        AspectRatio::FOUR_THREE,
        AspectRatio::SIXTEEN_NINE,
        AspectRatio::ULTRAWIDE_64_27,
    ] {
        let pinned = pin(policy, aspect, Magnification::ONE).expect("the policy pins");
        assert_close(
            pinned.declared_vertical_fov(),
            policy.vertical_fov_at(aspect).0,
            1e-12,
            "the declared field of view is the policy's own",
        );
        assert_close(
            f64::from(pinned.pinned().fov_y_radians()),
            pinned.declared_vertical_fov(),
            1e-6,
            "the f32 pin is the declared value, narrowed",
        );
        // The aspect narrows the same way: 16:9 has no exact f32 either, and a
        // capture that rounded it would draw at a slightly different viewport.
        assert_close(
            f64::from(pinned.pinned().aspect_ratio()),
            aspect.value(),
            1e-6,
            "the pinned aspect is the declared one, narrowed",
        );
        assert_eq!(f64::from(pinned.pinned().near_m()), 1.0);
        assert_eq!(f64::from(pinned.pinned().far_m()), 20_000.0);
    }

    // The narrowing is kept visible rather than hidden behind the cast: every
    // field reports both ends, and a consumer can compare them.
    let pinned = pin(policy, AspectRatio::SIXTEEN_NINE, Magnification::ONE).expect("pins");
    let fields: Vec<&str> = pinned
        .narrowings()
        .iter()
        .map(|narrowing| narrowing.field)
        .collect();
    assert_eq!(fields, ["vertical_fov", "aspect", "near_m", "far_m"]);
    for narrowing in pinned.narrowings() {
        // Each field lands on the f32 nearest its exact value: at most half an
        // ulp away, which is the strongest statement a narrowing can make and
        // the one a consumer needs in order to trust the pinned record.
        let magnitude = narrowing.exact.abs().max(f64::MIN_POSITIVE);
        assert!(
            narrowing.drift().abs() <= f32::EPSILON as f64 * magnitude / 2.0,
            "{} drifted by {}, more than half an ulp",
            narrowing.field,
            narrowing.drift()
        );
        assert!(narrowing.drift().is_finite());
    }
    assert!(
        pinned.max_drift() > 0.0,
        "the aspect is not exactly representable, and the report says so"
    );
}

#[test]
fn accept_f21_c_a_capture_folds_the_declared_magnification_into_the_pinned_frustum() {
    let policy = spyglass_projection();
    let plain = pin(policy, AspectRatio::FOUR_THREE, Magnification::ONE).expect("pins");
    let magnified = pin(
        policy,
        AspectRatio::FOUR_THREE,
        Magnification::new(4.0).expect("a positive factor"),
    )
    .expect("pins");

    // The mechanism is a narrower vertical field of view, and the arithmetic is
    // the half-angle tangent divided by the factor — the standard magnified
    // sight, and the only one of the three candidate mechanisms this stage can
    // justify without original data.
    let expected = 2.0 * ((plain.declared_vertical_fov() * 0.5).tan() / 4.0).atan();
    assert_close(
        magnified.pinned_vertical_fov().exact,
        expected,
        1e-12,
        "four times the factor, a quarter of the half-angle tangent",
    );
    assert_close(
        magnified.declared_vertical_fov(),
        plain.declared_vertical_fov(),
        1e-12,
        "the declared policy is unchanged; the fold happened in the pin",
    );
    assert!(
        magnified.pinned().fov_y_radians() < plain.pinned().fov_y_radians(),
        "a magnified capture draws a narrower frustum"
    );
    assert_eq!(
        magnified.magnification(),
        MagnificationPin {
            factor: 4.0,
            folded_into_fov: true
        }
    );
    // The near and far planes are the mode's own, not the cockpit's: the
    // spyglass "obeys its own near/far rendering requirements" through a
    // capture exactly as it does through a live frame.
    assert_eq!(magnified.pinned().near_m(), 1.0);
    assert_eq!(magnified.pinned().far_m(), 20_000.0);

    // A mode that magnifies nothing reports that nothing was folded, rather
    // than claiming a narrowing that did not happen.
    let plain_pin = pin(
        cockpit_projection(),
        AspectRatio::FOUR_THREE,
        Magnification::ONE,
    )
    .expect("pins");
    assert_eq!(
        plain_pin.magnification(),
        MagnificationPin {
            factor: 1.0,
            folded_into_fov: false
        }
    );
}

#[test]
fn accept_f21_c_a_projection_with_no_f32_near_it_refuses_instead_of_pinning_an_infinity() {
    // A far plane beyond `f32::MAX` is finite as `f64` and has no `f32` near it.
    // Casting it would pin an infinity, and a capture that claims to reproduce a
    // projection while carrying one is a lie, so the boundary refuses by name.
    let huge = exotic_projection(0.1, 1.0e300);
    assert_eq!(
        pin(huge, AspectRatio::FOUR_THREE, Magnification::ONE),
        Err(ProjectionPinError::Unrepresentable {
            field: "far_m",
            exact: 1.0e300
        }),
        "the refusal names the field that has no f32 near it"
    );

    // A near plane below the smallest positive `f32` would pin a zero, which
    // the `f32` record then refuses for its own reasons — the other half of the
    // same boundary, checked here so both sides of the cast are covered.
    let tiny = exotic_projection(1.0e-300, 10_000.0);
    assert_eq!(
        pin(tiny, AspectRatio::FOUR_THREE, Magnification::ONE),
        Err(ProjectionPinError::Unrepresentable {
            field: "near_m",
            exact: 1.0e-300
        })
    );
}

/// Lowers a declared policy with the given clipping planes, through the real
/// content record and the real lowering boundary.
///
/// The values are extreme but legal: a mission may declare a far plane in the
/// hundreds of kilometres and a near plane far below a millimetre, and both are
/// representable in the canonical `f64` world this project owns.
fn exotic_projection(near_m: f64, far_m: f64) -> cs_app::camera::LoweredProjection {
    cs_app::camera::lower_projection(&ProjectionPolicy {
        fov: known(Radians(std::f64::consts::FRAC_PI_3)),
        fov_axis: known(FovAxis::Vertical),
        reference_aspect: known(AspectRatio::FOUR_THREE),
        framing: known(AspectFraming::PreserveVertical),
        near_m: known(Meters(near_m)),
        far_m: known(Meters(far_m)),
    })
    .expect("the declared policy is valid as a policy")
}

#[test]
fn accept_f21_c_the_override_report_lists_every_override_a_capture_applied() {
    let pose = aircraft_pose([12.0, 34.0, -56.0], Quaternion::IDENTITY);
    let settings = ComparisonSettings::comparison();
    let request = CaptureRequest::build(
        mission("m01"),
        Tick(77),
        Some(ViewRig::Spyglass),
        Some(pose),
        Some(AspectRatio::SIXTEEN_NINE),
        Some(settings),
    )
    .expect("every input names a valid value");

    let target = CaptureTarget {
        rig: Some(ViewRig::Spyglass),
        pose,
        projection: spyglass_projection(),
        aspect: AspectRatio::SIXTEEN_NINE,
        magnification: Magnification::new(4.0).expect("a positive factor"),
    };
    let report = request
        .apply(&target, ViewRig::Cockpit)
        .expect("the frame's projection pins at this aspect");

    // "Report all overrides" means the list is complete and ordered, and each
    // entry names what was asked for *and* what the engine used.
    let fields: Vec<&str> = report
        .overrides()
        .iter()
        .map(|entry| entry.field())
        .collect();
    assert_eq!(
        fields,
        [
            "rig",
            "world_pose",
            "aspect",
            "settings",
            "smoothing",
            "magnification",
            "projection"
        ],
        "every override is reported, in report order"
    );
    assert_eq!(
        report.override_of("rig"),
        Some(&CaptureOverride::Rig {
            requested: ViewRig::Spyglass,
            restored: ViewRig::Cockpit
        }),
        "a teardown is as visible as the override"
    );
    assert_eq!(
        report.override_of("world_pose"),
        Some(&CaptureOverride::WorldPose {
            requested: pose,
            effective: pose
        })
    );
    assert_eq!(
        report.override_of("aspect"),
        Some(&CaptureOverride::Aspect {
            requested: AspectRatio::SIXTEEN_NINE,
            effective: AspectRatio::SIXTEEN_NINE
        })
    );
    assert_eq!(
        report.override_of("settings"),
        Some(&CaptureOverride::Settings { settings })
    );
    assert_eq!(
        report.override_of("smoothing"),
        Some(&CaptureOverride::Smoothing { bypassed: true }),
        "a pinned pose is held exactly, and the report says so"
    );
    assert_eq!(
        report.override_of("magnification"),
        Some(&CaptureOverride::Magnification {
            factor: 4.0,
            folded_into_fov: true
        })
    );
    assert!(matches!(
        report.override_of("projection"),
        Some(CaptureOverride::Projection { .. })
    ));
    assert_eq!(report.mission(), &mission("m01"));
    assert_eq!(report.tick(), Tick(77));
    assert_eq!(report.rig(), Some(ViewRig::Spyglass));
    assert_eq!(report.pose(), pose);
    assert_eq!(report.aspect(), AspectRatio::SIXTEEN_NINE);
    assert_eq!(report.request(), &request);
    for entry in report.overrides() {
        assert!(!entry.to_string().is_empty(), "{entry} is reportable");
    }

    // A capture that pins nothing still reports the two things the engine
    // always had to state, so "no overrides" and "not reported" cannot look the
    // same.
    let bare = CaptureRequest::new(mission("m01"), Tick(77)).expect("valid");
    let bare_report = bare
        .apply(
            &CaptureTarget {
                rig: Some(ViewRig::Cockpit),
                pose: CameraPose::new(world([0.0, 0.0, 0.0]), Quaternion::IDENTITY),
                projection: cockpit_projection(),
                aspect: AspectRatio::FOUR_THREE,
                magnification: Magnification::ONE,
            },
            ViewRig::Cockpit,
        )
        .expect("pins");
    let fields: Vec<&str> = bare_report
        .overrides()
        .iter()
        .map(|entry| entry.field())
        .collect();
    assert_eq!(fields, ["smoothing", "magnification", "projection"]);
    assert_eq!(
        bare_report.override_of("smoothing"),
        Some(&CaptureOverride::Smoothing { bypassed: false }),
        "nothing pinned, so the live camera's smoothing stands"
    );
    assert!(bare_report.override_of("world_pose").is_none());
}
