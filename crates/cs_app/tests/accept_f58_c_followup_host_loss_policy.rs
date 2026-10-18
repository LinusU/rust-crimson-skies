//! Acceptance scenario F58-C follow-up: the match-level host-loss flow follows
//! the mode's [`HostLoss`] policy. Task test prefix:
//! `accept_f58_c_followup_host_loss_policy_`.
//!
//! `EndSession` ends the match once; `PauseThenEnd { ticks }` pauses for the
//! grace period, lets the host return and resumes inside it, and ends
//! irrevocably once it lapses. No original grace period is asserted: the
//! numbers are synthetic fixture data.

use cs_app::network::recovery::{HostLossAction, HostReturn, RecoveryFlow};
use cs_net::compat::PeerAllocator;
use cs_net::lobby::{HostLoss, LateJoin};
use cs_net::recovery::{
    RecoveryKind, RecoveryPolicy, RecoveryRefusal, RecoveryRequest, SessionGenerations, Settlement,
};
use cs_types::Tick;
use cs_types::net::SessionId;

const SESSION: u64 = 81;

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session")
}

fn policy(host_loss: HostLoss) -> RecoveryPolicy {
    RecoveryPolicy {
        late_join: LateJoin::Closed,
        match_running: true,
        authoritative_state: true,
        host_loss,
    }
}

fn reconnect() -> RecoveryRequest {
    RecoveryRequest {
        kind: RecoveryKind::Reconnect,
        resumed_from: session(),
        claimed: Vec::new(),
    }
}

fn recover_refusal(flow: &mut RecoveryFlow, host_loss: HostLoss) -> Option<RecoveryRefusal> {
    let mut generations = SessionGenerations::starting_at(SESSION + 1);
    let mut peers = PeerAllocator::new();
    flow.recover(
        reconnect(),
        &policy(host_loss),
        &mut generations,
        &mut peers,
    )
    .refusal()
}

#[test]
fn accept_f58_c_followup_host_loss_policy_end_session_ends_once_and_never_resumes() {
    let mut flow = RecoveryFlow::new(session()).with_host_loss_policy(HostLoss::EndSession);
    let first = flow.host_lost(Tick(10), None);
    assert_eq!(first.settlement, Settlement::Applied { tick: Tick(10) });
    assert_eq!(first.action, HostLossAction::ReturnToMenu);
    assert!(flow.is_ended());
    assert!(!flow.is_paused());

    let second = flow.host_lost(Tick(11), None);
    assert_eq!(second.settlement, Settlement::Duplicate { tick: Tick(10) });
    assert_eq!(flow.host_returned(Tick(11)), HostReturn::TooLate);
    assert_eq!(
        recover_refusal(&mut flow, HostLoss::EndSession),
        Some(RecoveryRefusal::MatchEnded)
    );
}

#[test]
fn accept_f58_c_followup_host_loss_policy_pause_then_end_resumes_inside_the_grace() {
    let grace = HostLoss::PauseThenEnd { ticks: 600 };
    let mut flow = RecoveryFlow::new(session()).with_host_loss_policy(grace);

    let first = flow.host_lost(Tick(100), None);
    assert_eq!(first.settlement, Settlement::Applied { tick: Tick(100) });
    assert_eq!(
        first.action,
        HostLossAction::PauseMatch { until: Tick(700) }
    );
    assert!(flow.is_paused());
    assert!(!flow.is_ended(), "a pause is not an end");

    // A repeated report inside the grace changes nothing, and polling before
    // the deadline ends nothing.
    let again = flow.host_lost(Tick(300), None);
    assert_eq!(again.settlement, Settlement::Duplicate { tick: Tick(100) });
    assert_eq!(
        again.action,
        HostLossAction::PauseMatch { until: Tick(700) }
    );
    assert!(flow.poll_host_loss(Tick(699), None).is_none());

    // A client may resume while paused.
    assert_eq!(recover_refusal(&mut flow, grace), None);

    // The host returns inside the grace: running again.
    assert_eq!(flow.host_returned(Tick(699)), HostReturn::Resumed);
    assert!(!flow.is_paused());
    assert_eq!(flow.host_returned(Tick(699)), HostReturn::NotPaused);
}

#[test]
fn accept_f58_c_followup_host_loss_policy_pause_then_end_ends_irrevocably_when_it_lapses() {
    let grace = HostLoss::PauseThenEnd { ticks: 600 };
    let mut flow = RecoveryFlow::new(session()).with_host_loss_policy(grace);
    let _ = flow.host_lost(Tick(100), None);

    // At the deadline the host's return is already too late, polled or not.
    assert_eq!(flow.host_returned(Tick(700)), HostReturn::TooLate);
    let ended = flow
        .poll_host_loss(Tick(700), None)
        .expect("the lapsed grace ends the match");
    assert_eq!(ended.settlement, Settlement::Applied { tick: Tick(700) });
    assert_eq!(ended.action, HostLossAction::ReturnToMenu);
    assert!(flow.is_ended());

    assert!(flow.poll_host_loss(Tick(701), None).is_none());
    assert_eq!(flow.host_returned(Tick(701)), HostReturn::TooLate);
    assert_eq!(
        flow.host_lost(Tick(702), None).settlement,
        Settlement::Duplicate { tick: Tick(700) }
    );
    assert_eq!(
        recover_refusal(&mut flow, grace),
        Some(RecoveryRefusal::MatchEnded)
    );
}

#[test]
fn accept_f58_c_followup_host_loss_policy_a_report_after_the_deadline_ends_it() {
    let mut flow =
        RecoveryFlow::new(session()).with_host_loss_policy(HostLoss::PauseThenEnd { ticks: 5 });
    let _ = flow.host_lost(Tick(0), None);
    let late = flow.host_lost(Tick(9), None);
    assert_eq!(late.action, HostLossAction::ReturnToMenu);
    assert!(flow.is_ended());

    let mut zero =
        RecoveryFlow::new(session()).with_host_loss_policy(HostLoss::PauseThenEnd { ticks: 0 });
    assert_eq!(
        zero.host_lost(Tick(3), None).action,
        HostLossAction::ReturnToMenu,
        "a zero grace period ends at once"
    );
}
