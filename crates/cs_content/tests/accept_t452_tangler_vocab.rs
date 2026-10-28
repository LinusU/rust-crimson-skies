//! Task #2185 (T452-REG-TANGLER-VOCAB): the persistent tangler field and its
//! timed force-suppressed status. Task test prefix: `accept_t452_tangler_vocab_`.
//!
//! Measured by #2179 (`docs/findings/2026-10-28-t452-tangler-effect.md`); static
//! analysis only, never original-run behavior.

use cs_content::ordnance::{
    DeclaredOrdnanceFamily, MeasuredAreaSource, OriginalAreaDeclaration, OriginalClusterShape,
    OriginalComponentLaunch, OriginalDamageDeclaration, OriginalLifetime, OriginalMountDeclaration,
    OriginalStatusEffectDeclaration, T452_REG_TANGLER_FORCE_SCALAR, TANGLE_STATUS_NAME,
    TangleCountdown, TangleReHit, lower_tangler_field,
};

fn row(key: &'static str, time: Option<f64>) -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: "wep_12".to_owned(),
        family: DeclaredOrdnanceFamily::AreaDenialEngine,
        mount: OriginalMountDeclaration::RocketSlot { slots: 8 },
        velocity: None,
        fire_rate: None,
        proximity_trigger_m: None,
        lifetime: OriginalLifetime::Undeclared,
        area: OriginalAreaDeclaration::default(),
        damage: OriginalDamageDeclaration::default(),
        statuses: vec![OriginalStatusEffectDeclaration {
            key,
            duration_s: time,
            duration_unit: time.map(|_| "TIME".to_owned()),
        }],
        cluster: OriginalClusterShape::Undeclared,
        unknowns: Vec::new(),
    }
}

#[test]
fn accept_t452_tangler_vocab_wep_12_lowers_to_the_measured_field() {
    let field = lower_tangler_field(&row("TANGLER", Some(2.0)), None).expect("a TANGLER record");
    assert_eq!(field.record, "wep_12");
    assert_eq!(field.radius_m, 10.0);
    assert_eq!(
        field.radius_source,
        MeasuredAreaSource::ImageDefault { value: 10.0 }
    );
    assert_eq!(field.lifetime_s, 2.0);
    assert_eq!(
        field.lifetime_source,
        MeasuredAreaSource::RecordKey { key: "TIME" }
    );
    assert!(field.created_at_fire_time && field.reapplied_every_tick && field.includes_owner);
    assert_eq!(field.status.name, TANGLE_STATUS_NAME);
    assert_eq!(field.status.claim, T452_REG_TANGLER_FORCE_SCALAR);
    assert_eq!(field.status.on_rehit, TangleReHit::RaiseToMax);
    assert_eq!(field.status.countdown, TangleCountdown::FrameTime);
    // Strictly inside the squared radius.
    assert!(field.contains(99.9));
    assert!(!field.contains(100.0));
}

#[test]
fn accept_t452_tangler_vocab_duration_has_a_two_second_floor() {
    let status = lower_tangler_field(&row("TANGLER", Some(2.0)), None)
        .expect("field")
        .status;
    assert_eq!(status.duration_for(0.0), 10.0);
    assert_eq!(status.duration_for(5.0), 5.0);
    // Above the floor only for d2 < 8.
    assert_eq!(status.duration_for(8.0), 2.0);
    assert_eq!(status.duration_for(50.0), 2.0);
    assert_eq!(status.duration_for(99.0), 2.0);
}

#[test]
fn accept_t452_tangler_vocab_a_rehit_raises_but_never_shortens() {
    let status = lower_tangler_field(&row("TANGLER", Some(2.0)), None)
        .expect("field")
        .status;
    assert_eq!(status.after_hit(0.0, 5.0), 5.0);
    assert_eq!(status.after_hit(9.0, 5.0), 9.0);
    assert_eq!(status.after_hit(3.0, 0.0), 10.0);
}

#[test]
fn accept_t452_tangler_vocab_time_and_radius_change_the_declaration() {
    let field = lower_tangler_field(&row("TANGLER", Some(3.5)), Some(20.0)).expect("field");
    assert_eq!(field.lifetime_s, 3.5);
    assert_eq!(field.radius_m, 20.0);
    assert_eq!(
        field.radius_source,
        MeasuredAreaSource::RecordKey { key: "RADIUS" }
    );
    assert!(field.contains(399.0));
    assert!(!field.contains(401.0));
    // The divisor is the unsquared radius: d2 = 10 on RADIUS 20 gives 5 s.
    assert_eq!(field.status.duration_for(10.0), 5.0);

    let defaulted = lower_tangler_field(&row("TANGLER", None), None).expect("field");
    assert_eq!(defaulted.lifetime_s, 5.0);
    assert_eq!(
        defaulted.lifetime_source,
        MeasuredAreaSource::ImageDefault { value: 5.0 }
    );
}

#[test]
fn accept_t452_tangler_vocab_other_records_lower_to_nothing() {
    assert!(lower_tangler_field(&row("SMOKE_SCREEN", Some(8.0)), None).is_none());
}
