//! Acceptance scenarios F41-B: spatial placement of emitters.
//! Task test prefix: `accept_f41_b_`. Designed law, synthetic geometry.

use cs_sim::audio_events::{Listener, SpatialError, SpatialPolicy, spatialize};

fn listener() -> Listener {
    Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("valid")
}

fn policy() -> SpatialPolicy {
    SpatialPolicy::try_new(10.0, 100.0).expect("valid")
}

#[test]
fn accept_f41_b_emitter_pans_toward_its_side_and_attenuates_with_distance() {
    let right = spatialize(&policy(), &listener(), [20.0, 0.0, 0.0]).unwrap();
    let left = spatialize(&policy(), &listener(), [-20.0, 0.0, 0.0]).unwrap();
    assert_eq!(right.pan, 1.0);
    assert_eq!(left.pan, -1.0);
    assert!((right.gain - 0.5).abs() < 1e-12);
    let near = spatialize(&policy(), &listener(), [0.0, 5.0, 0.0]).unwrap();
    assert_eq!((near.gain, near.pan), (1.0, 0.0));
    let far = spatialize(&policy(), &listener(), [0.0, 0.0, 100.0]).unwrap();
    assert_eq!(far.gain, 0.0);
    let centred = spatialize(&policy(), &listener(), [0.0, 0.0, 0.0]).unwrap();
    assert_eq!((centred.gain, centred.pan), (1.0, 0.0));
}

#[test]
fn accept_f41_b_corrupt_spatial_inputs_are_refused() {
    assert!(matches!(
        SpatialPolicy::try_new(0.0, 5.0),
        Err(SpatialError::BadReferenceDistance { .. })
    ));
    assert!(matches!(
        SpatialPolicy::try_new(5.0, 5.0),
        Err(SpatialError::BadMaxDistance { .. })
    ));
    assert_eq!(
        Listener::try_new([0.0; 3], [2.0, 0.0, 0.0]),
        Err(SpatialError::NonUnitRightAxis)
    );
    assert_eq!(
        spatialize(&policy(), &listener(), [f64::NAN, 0.0, 0.0]),
        Err(SpatialError::NonFinite)
    );
}
