use cs_app::ui::hud::{Hud, HudError, WeaponSample};
use cs_content::hud::HudPolicy;

use crate::{actor, level_sample, session, weapon_id};

#[test]
fn accept_f46_a_empty_ammunition_is_a_gauge_state_not_a_selection_change() {
    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(1), actor(1, 1)).unwrap();
    let mut s = level_sample(1, 1);
    let full = hud.project(&s).unwrap();
    assert!(!full.weapon.empty);

    s.weapon.ammunition = 0;
    let empty = hud.project(&s).unwrap();
    assert!(empty.weapon.empty, "same sample tick shows empty");
    assert_eq!(empty.weapon.selected, Some(weapon_id("synthetic-gun")));

    s.weapon = WeaponSample {
        selected: None,
        ammunition: 0,
    };
    assert!(
        !hud.project(&s).unwrap().weapon.empty,
        "nothing selected is not empty"
    );
}

#[test]
fn accept_f46_a_swap_rebinds_and_refuses_the_old_aircrafts_samples() {
    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    assert_eq!(
        hud.project(&level_sample(1, 1)).unwrap_err(),
        HudError::Unbound
    );

    hud.bind(session(1), actor(1, 1)).unwrap();
    let mut low = level_sample(1, 1);
    low.height_m = 10.0;
    assert!(hud.project(&low).unwrap().low_altitude_warning);

    // Swap to another aircraft with a different weapon.
    hud.bind(session(1), actor(1, 2)).unwrap();
    assert_eq!(hud.bound(), Some((session(1), actor(1, 2))));
    let stale = hud.project(&low).unwrap_err();
    assert!(matches!(stale, HudError::Stale { .. }), "{stale}");

    let mut next = level_sample(1, 2);
    next.weapon.selected = Some(weapon_id("synthetic-rocket"));
    next.weapon.ammunition = 4;
    let out = hud.project(&next).unwrap();
    assert!(!out.low_altitude_warning, "warning does not carry over");
    assert_eq!(out.weapon.selected, Some(weapon_id("synthetic-rocket")));
    assert_eq!(out.weapon.ammunition, 4);
    assert_eq!(out.actor, actor(1, 2));
}

#[test]
fn accept_f46_a_new_session_generation_is_stale_for_the_same_serial() {
    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(2), actor(2, 1)).unwrap();
    // A retry made session 3; a late sample of session 2's serial 1 is refused.
    let err = hud.project(&level_sample(3, 1)).unwrap_err();
    assert!(matches!(err, HudError::Stale { .. }));
    assert_eq!(
        hud.bind(session(3), actor(2, 1)).unwrap_err(),
        HudError::ActorSessionMismatch
    );
}
