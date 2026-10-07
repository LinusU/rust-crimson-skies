//! F46-B: the HUD frame's gauge clusters read the session's own authorities
//! for the bound actor — never a caller-asserted copy, never a foreign
//! generation, and "empty" is a gauge state, not a selection change.

use cs_app::ui::hud::{Hud, HudError, HudSources};
use cs_content::hud::HudPolicy;
use cs_content::ordnance::{declared_synthetic_direct, declared_synthetic_nitro};
use cs_content::weapons::DeclaredGunMountKind;
use cs_sim::damage::{DamageNodeKind, PartState};
use cs_sim::weapons::{GunBank, GunRate, SYNTHETIC_TICKS_BETWEEN_SHOTS};

use crate::{
    NOSE_MOUNT, WING_MOUNT, actor, damage_for, key, level_sample, ordnance_for, session,
    weapon_session,
};

fn rate() -> GunRate {
    GunRate::try_new(SYNTHETIC_TICKS_BETWEEN_SHOTS).expect("the fixture rate is valid")
}

fn weapons_only(weapons: &cs_app::weapons::WeaponSession) -> HudSources<'_> {
    HudSources {
        weapons: Some(weapons),
        ..HudSources::default()
    }
}

/// The minimum scenario, AC02: the selected bank fires its last round through
/// the real `WeaponSession`, and the very next frame reads `empty` while the
/// mount stays selected — the gauge observes the authority, it never owns the
/// selection.
#[test]
fn accept_f46_b_zero_ammunition_is_a_gauge_state_not_a_selection_change() {
    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(1), actor(1, 1)).unwrap();
    let mut weapons = weapon_session(
        1,
        1,
        &[(NOSE_MOUNT, DeclaredGunMountKind::Nose)],
        &[NOSE_MOUNT],
        1,
    );
    let nose = key(NOSE_MOUNT);

    let gauge = hud
        .frame(&level_sample(1, 1), &weapons_only(&weapons))
        .unwrap()
        .weapons
        .expect("the registered arsenal is a gauge");
    assert!(!gauge.empty && gauge.has_selection);
    assert_eq!(gauge.selected_rounds, 1);

    weapons
        .state_mut(&actor(1, 1))
        .expect("registered")
        .consume_round(&nose, rate())
        .expect("a loaded mount fires");
    let frame = hud
        .frame(&level_sample(1, 1), &weapons_only(&weapons))
        .unwrap();
    let gauge = frame.weapons.expect("the arsenal is still a gauge");
    assert!(gauge.empty, "a dry bank reads empty on the next frame");
    assert_eq!(gauge.selected_rounds, 0);
    assert!(
        gauge.has_selection,
        "empty is a gauge state, never a selection change"
    );
    let mount = gauge
        .guns
        .iter()
        .find(|gun| gun.mount == nose)
        .expect("the mounted gun is a row");
    assert!(mount.selected, "the dry mount stays selected");
    assert_eq!(mount.rounds, 0);
    // The authority's selection is equally unchanged.
    assert!(
        weapons
            .state(&actor(1, 1))
            .unwrap()
            .selected()
            .contains(&nose)
    );
}

/// One row per mounted gun: the selected bank's aggregate rounds, the
/// ammunition type the selected mounts load, and a disabled mount that stays
/// a row.
#[test]
fn accept_f46_b_gauge_rows_are_the_authoritys_own_mount_state() {
    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(1), actor(1, 1)).unwrap();
    let mut weapons = weapon_session(
        1,
        1,
        &[
            (NOSE_MOUNT, DeclaredGunMountKind::Nose),
            (WING_MOUNT, DeclaredGunMountKind::WingLeft),
        ],
        &[NOSE_MOUNT],
        3,
    );
    let (nose, wing) = (key(NOSE_MOUNT), key(WING_MOUNT));

    let gauge = hud
        .frame(&level_sample(1, 1), &weapons_only(&weapons))
        .unwrap()
        .weapons
        .expect("the registered arsenal is a gauge");
    assert_eq!(gauge.guns.len(), 2, "every mounted gun is a row");
    let row = |mount| gauge.guns.iter().find(|gun| gun.mount == mount).unwrap();
    assert!(row(nose.clone()).selected && row(nose.clone()).rounds == 3);
    assert!(!row(wing.clone()).selected && row(wing.clone()).rounds == 3);
    assert_eq!(gauge.selected_rounds, 3, "the selected bank's rounds");
    let selected_ammunition = weapons
        .cadence()
        .resolver()
        .definitions(&actor(1, 1))
        .iter()
        .find(|definition| definition.mount() == &nose)
        .expect("the nose definition")
        .ammunition()
        .clone();
    assert_eq!(gauge.ammunition, vec![selected_ammunition]);

    // A destroyed mount node disables its gun; the row stays.
    weapons
        .state_mut(&actor(1, 1))
        .expect("registered")
        .disable(&wing);
    let gauge = hud
        .frame(&level_sample(1, 1), &weapons_only(&weapons))
        .unwrap()
        .weapons
        .unwrap();
    assert_eq!(gauge.guns.len(), 2);
    assert!(
        gauge
            .guns
            .iter()
            .any(|gun| gun.mount == wing && gun.disabled)
    );
    assert!(!gauge.empty, "the selected mount still has rounds");
}

/// An unselected bank is not "empty", and an absent arsenal is no gauge.
#[test]
fn accept_f46_b_empty_requires_a_selected_bank() {
    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(1), actor(1, 1)).unwrap();

    // Selection cleared inside the authority: nothing selected is not empty.
    let mut weapons = weapon_session(
        1,
        1,
        &[(NOSE_MOUNT, DeclaredGunMountKind::Nose)],
        &[NOSE_MOUNT],
        5,
    );
    weapons
        .state_mut(&actor(1, 1))
        .expect("registered")
        .select(GunBank::default());
    let gauge = hud
        .frame(&level_sample(1, 1), &weapons_only(&weapons))
        .unwrap()
        .weapons
        .expect("the registered arsenal is a gauge");
    assert!(!gauge.has_selection && !gauge.empty);
    // A serial the authority never registered has no gauge at all.
    let mut other = Hud::new(HudPolicy::designed()).unwrap();
    other.bind(session(1), actor(1, 9)).unwrap();
    let frame = other
        .frame(&level_sample(1, 9), &weapons_only(&weapons))
        .unwrap();
    assert!(frame.weapons.is_none());
}

/// A gauge can only describe the bound actor: a source stamped for another
/// session generation is refused, never read as an empty gauge.
#[test]
fn accept_f46_b_foreign_generation_authorities_are_refused() {
    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(1), actor(1, 1)).unwrap();
    let weapons = weapon_session(
        5,
        1,
        &[(NOSE_MOUNT, DeclaredGunMountKind::Nose)],
        &[NOSE_MOUNT],
        7,
    );
    let err = hud
        .frame(&level_sample(1, 1), &weapons_only(&weapons))
        .unwrap_err();
    assert!(
        matches!(
            err,
            HudError::ForeignAuthority {
                source: "weapons",
                expected,
                found: 5,
            } if expected == session(1)
        ),
        "{err}"
    );

    let damage = damage_for(5, 1);
    let err = hud
        .frame(
            &level_sample(1, 1),
            &HudSources {
                damage: Some(&damage),
                ..HudSources::default()
            },
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            HudError::ForeignAuthority {
                source: "damage",
                found: 5,
                ..
            }
        ),
        "{err}"
    );

    let ordnance = ordnance_for(5, 1, &[declared_synthetic_direct()]);
    let err = hud
        .frame(
            &level_sample(1, 1),
            &HudSources {
                ordnance: Some(&ordnance),
                ..HudSources::default()
            },
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            HudError::ForeignAuthority {
                source: "ordnance",
                found: 5,
                ..
            }
        ),
        "{err}"
    );
}

/// The airframe panel is the damage graph's own nodes with their live part
/// state — `Intact` here because the fixture graph starts intact — and an
/// unregistered actor has no panel at all.
#[test]
fn accept_f46_b_airframe_panel_is_the_graphs_own_parts() {
    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(1), actor(1, 1)).unwrap();
    let damage = damage_for(1, 1);

    let frame = hud
        .frame(
            &level_sample(1, 1),
            &HudSources {
                damage: Some(&damage),
                ..HudSources::default()
            },
        )
        .unwrap();
    let zones = frame.airframe.expect("a registered airframe is a panel");
    assert_eq!(zones.len(), 4, "the synthetic airframe's four nodes");
    assert!(
        zones.iter().all(|zone| zone.state == PartState::Intact),
        "an intact graph reports intact parts"
    );
    assert!(
        zones
            .iter()
            .any(|zone| zone.kind == DamageNodeKind::WeaponMount),
        "the mount node is a zone"
    );

    // No authority installed: no panel, no invented rows.
    let bare = hud
        .frame(&level_sample(1, 1), &HudSources::default())
        .unwrap();
    assert!(bare.airframe.is_none() && bare.weapons.is_none() && bare.ordnance.is_none());
}

/// The launcher cluster reports the session's launchable components for the
/// bound actor; a booster is driven, not launched, so it is never a row.
#[test]
fn accept_f46_b_ordnance_rows_are_the_sessions_launchable_components() {
    let mut hud = Hud::new(HudPolicy::designed()).unwrap();
    hud.bind(session(1), actor(1, 1)).unwrap();
    let ordnance = ordnance_for(
        1,
        1,
        &[declared_synthetic_direct(), declared_synthetic_nitro()],
    );

    let frame = hud
        .frame(
            &level_sample(1, 1),
            &HudSources {
                ordnance: Some(&ordnance),
                ..HudSources::default()
            },
        )
        .unwrap();
    let rows = frame.ordnance.expect("a registered launcher is a cluster");
    assert_eq!(rows.len(), 1, "the booster is driven, not launched");
    assert_eq!(
        rows[0].mount,
        cs_types::content::DamageNodeKey::new(
            cs_content::ordnance::DECLARED_SYNTHETIC_LAUNCHER_MOUNT
        )
        .expect("the fixture mount key is valid")
    );
    // Another serial of the same session has no components: no cluster.
    let mut other = Hud::new(HudPolicy::designed()).unwrap();
    other.bind(session(1), actor(1, 2)).unwrap();
    let frame = other
        .frame(
            &level_sample(1, 2),
            &HudSources {
                ordnance: Some(&ordnance),
                ..HudSources::default()
            },
        )
        .unwrap();
    assert!(frame.ordnance.is_none());
}
