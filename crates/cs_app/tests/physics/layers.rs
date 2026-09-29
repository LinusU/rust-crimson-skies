//! The declared collision layers and the sensor/damage boundary (F23-A
//! non-negotiable behaviors 2 and 3).

use cs_sim::collision::{
    CollisionLayer, CollisionLayers, ContactKind, ShapeClass, classify_contact,
};

/// Every declared layer has a unique bit and a stable, unique label.
///
/// Observable failure if a layer is removed or its bit collides with another:
/// the union no longer covers every layer, or a duplicate is found.
#[test]
fn accept_f23_a_declared_layers_are_complete_and_uniquely_bitted() {
    let mut bits = 0_u8;
    for layer in CollisionLayer::ALL {
        let bit = layer.bit();
        assert_eq!(bit & bits, 0, "layer {layer} reuses bit {bit:#04x}");
        bits |= bit;
        assert_eq!(
            CollisionLayer::from_label(layer.label()),
            Some(layer),
            "layer {} must round-trip through its label",
            layer.label()
        );
    }

    assert_eq!(bits, CollisionLayers::ALL.bits());
    assert_eq!(
        CollisionLayers::ALL.bits().count_ones(),
        CollisionLayer::ALL.len() as u32,
        "every declared layer must own exactly one bit"
    );
    assert_eq!(
        CollisionLayer::from_label("not_a_layer"),
        None,
        "an unknown label must not resolve"
    );
}

/// Set algebra masks unknown bits and composes correctly.
#[test]
fn accept_f23_a_collision_layer_sets_mask_and_compose() {
    assert!(CollisionLayers::NONE.is_empty());
    assert_eq!(
        CollisionLayers::from_bits(0xff).bits(),
        CollisionLayers::ALL.bits()
    );

    let set = CollisionLayers::from(CollisionLayer::Aircraft).with(CollisionLayer::Trigger);
    assert!(set.contains(CollisionLayer::Aircraft));
    assert!(set.contains(CollisionLayer::Trigger));
    assert!(!set.contains(CollisionLayer::Camera));
    assert!(!set.is_empty());
    assert!(set.intersects(CollisionLayers::from(CollisionLayer::Trigger)));
    assert!(!set.intersects(CollisionLayers::from(CollisionLayer::Camera)));
}

/// The designed matrix is symmetric and keeps the camera layer inert.
#[test]
fn accept_f23_a_designed_matrix_is_symmetric_and_camera_is_inert() {
    for a in CollisionLayer::ALL {
        for b in CollisionLayer::ALL {
            assert_eq!(
                a.designed_collides_with(b),
                b.designed_collides_with(a),
                "the matrix must be symmetric for {a} and {b}"
            );
            if a == CollisionLayer::Camera || b == CollisionLayer::Camera {
                assert!(
                    !a.designed_collides_with(b),
                    "the camera query filter must never generate contacts ({a}, {b})"
                );
            }
        }
    }

    assert!(CollisionLayer::Aircraft.designed_collides_with(CollisionLayer::StaticWorld));
    assert!(CollisionLayer::Projectile.designed_collides_with(CollisionLayer::Aircraft));
    assert!(CollisionLayer::Trigger.designed_collides_with(CollisionLayer::Aircraft));
    assert!(!CollisionLayer::Trigger.designed_collides_with(CollisionLayer::Trigger));
}

/// A sensor on either side of an interacting pair is a sensor overlap, never a
/// solid contact: sensor overlap is not damage by itself.
///
/// Observable failure if the classifier reports `SolidContact` for a sensor
/// pair, or an overlap for layers that do not interact.
#[test]
fn accept_f23_a_sensor_overlap_is_never_a_solid_contact() {
    for a in CollisionLayer::ALL {
        for b in CollisionLayer::ALL {
            let solid = classify_contact(a, b, ShapeClass::Solid, ShapeClass::Solid);
            let sensor = classify_contact(a, b, ShapeClass::Sensor, ShapeClass::Sensor);
            let mixed = classify_contact(a, b, ShapeClass::Sensor, ShapeClass::Solid);

            if a.designed_collides_with(b) {
                assert_eq!(solid, ContactKind::SolidContact, "{a} vs {b}");
                assert!(
                    sensor.is_sensor_overlap() && !sensor.is_solid_contact(),
                    "{a} vs {b} sensor overlap must not be a solid contact"
                );
            } else {
                assert_eq!(solid, ContactKind::Ignored, "{a} vs {b}");
                assert_eq!(sensor, ContactKind::Ignored, "{a} vs {b}");
            }
            assert_eq!(
                mixed, sensor,
                "a sensor on only one side classifies like a full sensor pair ({a} vs {b})"
            );
        }
    }
}

/// Fast projectiles and narrow triggers demand swept/continuous detection;
/// the other layers do not carry the requirement.
#[test]
fn accept_f23_a_projectiles_and_triggers_require_continuous_detection() {
    assert!(CollisionLayer::Projectile.requires_continuous_detection());
    assert!(CollisionLayer::Trigger.requires_continuous_detection());

    for layer in [
        CollisionLayer::Aircraft,
        CollisionLayer::StaticWorld,
        CollisionLayer::Debris,
        CollisionLayer::Camera,
    ] {
        assert!(
            !layer.requires_continuous_detection(),
            "{layer} must not be forced into swept detection"
        );
    }
}
