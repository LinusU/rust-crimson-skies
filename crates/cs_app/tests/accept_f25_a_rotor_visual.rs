//! Acceptance scenario F25-A for the rotor's visual binding: which node spins,
//! how it is drawn from the authoritative physical rate, and the fact that a
//! render frame cannot drive the simulation.
//!
//! Spec: `specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! stage `### F25-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! section "Boost and special models". Task test prefix: `accept_f25_a_`.
//!
//! These tests use only the public API of [`cs_app::airframe_visual`] and the
//! `cs_sim` rotor types it consumes, so they fail to compile if the binding is
//! removed and fail at run time if a rotor can bind a foreign node or if
//! sampling the picture moves the simulation.

use cs_app::airframe_visual::{AirframeVisual, AirframeVisualError, RotorVisualBinding};
use cs_content::scene::SceneNodeId;
use cs_sim::flight::{
    RotorVisualSample, SYNTHETIC_TICK_DT_S, synthetic_rotor_drive, synthetic_rotor_mapping,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid content id")
}

fn node(key: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(cid(ContentKind::SceneNode, key)).expect("a scene node id")
}

fn autogyro_visual() -> AirframeVisual {
    AirframeVisual::new(
        cid(ContentKind::Airframe, "fixture.synthetic-autogyro"),
        cid(ContentKind::InstallFile, "planes"),
        node("planes.autogyro"),
    )
    .expect("a valid airframe visual")
}

/// A rotor binds by content id inside the airframe's own container. A node from
/// another tree and a repeated node are refused by name, so the drawn phase can
/// never depend on a foreign tree or on lookup order.
#[test]
fn accept_f25_a_rotor_visual_binds_a_node_under_the_airframe_container() {
    let mut visual = autogyro_visual();
    assert!(visual.rotors().is_empty());

    let mapping = synthetic_rotor_mapping();
    let rotor_node = node("planes.autogyro.rotor_main");
    let binding = RotorVisualBinding::new(rotor_node.clone(), mapping.clone());
    assert_eq!(binding.node(), &rotor_node);
    assert_eq!(binding.mapping(), &mapping);

    visual
        .bind_rotor(binding.clone())
        .expect("a rotor under the airframe's own container binds");
    assert_eq!(visual.rotors(), std::slice::from_ref(&binding));
    assert_eq!(visual.rotor(&rotor_node), Some(&binding));

    assert_eq!(
        visual.bind_rotor(binding.clone()),
        Err(AirframeVisualError::DuplicateRotorNode {
            node: "planes.autogyro.rotor_main".to_owned()
        })
    );

    assert_eq!(
        visual.bind_rotor(RotorVisualBinding::new(
            node("gamez.autogyro.rotor_main"),
            mapping.clone()
        )),
        Err(AirframeVisualError::RotorNodeOutsideContainer {
            container: "planes".to_owned(),
            node: "gamez.autogyro.rotor_main".to_owned()
        })
    );
    assert_eq!(
        visual.rotors().len(),
        1,
        "a refused binding changes nothing"
    );

    // A second, distinct rotor of the same airframe binds alongside the first.
    let tail = RotorVisualBinding::new(node("planes.autogyro.rotor_tail"), mapping);
    visual
        .bind_rotor(tail.clone())
        .expect("a second rotor binds");
    assert_eq!(visual.rotor(&tail.node().clone()), Some(&tail));
    assert_eq!(visual.rotors().len(), 2);
}

/// The declared mapping drives the drawn rate, and sampling the picture at one
/// frame per simulation tick or sixty frames per simulation tick leaves the
/// simulation and the drawn phase identical: rotor animation never becomes the
/// source of physics dt.
#[test]
fn accept_f25_a_rotor_visual_sampling_cannot_drive_physics_dt() {
    let binding = RotorVisualBinding::new(
        node("planes.autogyro.rotor_main"),
        synthetic_rotor_mapping(),
    );

    let draw = |frames_per_tick: u32| {
        let mut drive = synthetic_rotor_drive();
        let mut sample = RotorVisualSample::at_rest();
        let frame_dt = SYNTHETIC_TICK_DT_S / f64::from(frames_per_tick);
        for tick in 1..=120u64 {
            drive
                .advance_tick(40.0, 48.0, Tick(tick), SYNTHETIC_TICK_DT_S)
                .expect("each tick is newer than the last");
            for _ in 0..frames_per_tick {
                sample = binding
                    .sample(&drive, sample, frame_dt)
                    .expect("the drawn sample is finite");
            }
        }
        (drive, sample)
    };

    let (slow_drive, slow) = draw(1);
    let (fast_drive, fast) = draw(60);

    assert!(
        (slow_drive.physical_speed_radps() - fast_drive.physical_speed_radps()).abs()
            < f64::EPSILON,
        "the authoritative rate is a fixed-tick quantity"
    );
    assert!((slow_drive.physical_speed_radps() - 40.0).abs() < 1e-9);
    assert_eq!(slow.physical_speed_radps, fast.physical_speed_radps);
    assert!(
        (slow.phase_rad - fast.phase_rad).abs() < 1e-9,
        "both draws span one second of render time"
    );
    assert!((0.0..std::f64::consts::TAU).contains(&slow.phase_rad));
    assert_eq!(
        slow.visual_speed_radps(),
        Some(40.0 * 1.5),
        "the drawn rate follows the declared mapping, not the physical rate"
    );
    assert_ne!(
        slow.visual_speed_radps(),
        Some(slow.physical_speed_radps),
        "physical and visual rotor speeds may differ"
    );

    // Sampling is pure: the same drive and the same previous phase give the same
    // sample.
    let once = binding
        .sample(&slow_drive, RotorVisualSample::at_rest(), 0.01)
        .expect("the drawn sample is finite");
    let twice = binding
        .sample(&slow_drive, RotorVisualSample::at_rest(), 0.01)
        .expect("the drawn sample is finite");
    assert_eq!(once, twice);
    assert_eq!(slow_drive.physical_speed_radps(), 40.0);
}
