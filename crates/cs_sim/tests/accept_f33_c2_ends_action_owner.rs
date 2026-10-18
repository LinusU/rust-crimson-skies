//! F33-C2: `LifecycleKind::ends_action` is the one owner of the designed
//! "which transitions end an actor's ability to act" split.

use cs_sim::allies::AllyStatus;
use cs_sim::damage::LifecycleKind;

#[test]
fn accept_f33_c2_predicate_set_is_destroyed_despawned_mission_removed() {
    let ending: Vec<LifecycleKind> = LifecycleKind::ALL
        .iter()
        .copied()
        .filter(|kind| kind.ends_action())
        .collect();
    assert_eq!(
        ending,
        vec![
            LifecycleKind::Destroyed,
            LifecycleKind::Despawned,
            LifecycleKind::MissionRemoved
        ]
    );
}

#[test]
fn accept_f33_c2_capture_and_bailout_do_not_end_action() {
    assert!(!LifecycleKind::OwnershipCaptured.ends_action());
    assert!(!LifecycleKind::PilotBailout.ends_action());
    assert!(!AllyStatus::Captured.ends_action());
    assert!(!AllyStatus::BailedOut.ends_action());
    assert!(!AllyStatus::Active.ends_action());
}

#[test]
fn accept_f33_c2_ally_status_agrees_with_predicate() {
    for (status, kind) in [
        (AllyStatus::Destroyed, LifecycleKind::Destroyed),
        (AllyStatus::Despawned, LifecycleKind::Despawned),
        (AllyStatus::MissionRemoved, LifecycleKind::MissionRemoved),
        (AllyStatus::Captured, LifecycleKind::OwnershipCaptured),
        (AllyStatus::BailedOut, LifecycleKind::PilotBailout),
    ] {
        assert_eq!(status.ends_action(), kind.ends_action(), "{status}");
    }
}
