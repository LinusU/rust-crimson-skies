//! Acceptance scenario F16-F-SPEEDUP (task #722): **how single-player
//! speed-up reaches weapon cooldowns and objective timers**, decided and
//! implemented as the `SessionDriver` wiring.
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! follow-up stage of `### F16-D`, non-negotiable behavior 4. Shared
//! contract: `docs/contracts/FLIGHT-PHYSICS.md`. The decision and the
//! original addresses live in
//! `docs/findings/2026-10-06-f16-f-original-clock-pause-and-speed-up-policy.md`
//! (`speed-up.reaches-gameplay-timers`, updated by #722).
//!
//! What makes this scenario discriminating:
//!
//! * The original's speed-up is one 2× multiplier on the per-frame `game_dt`,
//!   and **everything** that reads it scales together: the vehicle clock
//!   behind weapon cooldowns and the mission update behind objective timers
//!   (owner notes on task #391). F16 non-negotiable 4 keeps speed-up as
//!   whole fixed ticks, so the project's equivalent is the
//!   [`SessionDriver`]: the frame's whole ticks are committed on the session
//!   clock and, when the 2× is requested, the very same whole ticks again —
//!   the total is then fed to the gameplay timeline.
//! * Every assertion below drives that production wiring. If the driver
//!   stops committing the extra ticks, stops feeding the timeline, or starts
//!   granting the multiplayer frame, the pinned behaviour fails.
//! * The refusal and pause boundaries are pinned alongside the new
//!   behaviour: `TimeError::NoSpeedUpAuthority` still refuses the multiplayer
//!   frame before anything mutates, and a paused frame commits nothing even
//!   when a speed-up was requested (AC04).

use std::time::Duration;

use cs_sim::time::{
    ORIGINAL_POLICY_TOLERANCE, OriginalClockPolicy, PolicyRelation, SessionDriver, SpeedUpRequest,
    TickRate, TimeError,
};

/// The project's designed fixed rate: 64 Hz is a designed development value,
/// not a measured original rate.
const TICK_HZ: u32 = 64;

fn rate() -> TickRate {
    TickRate::new(TICK_HZ).expect("64 Hz is a valid rate")
}

/// Periods long enough that the probes below never saturate a timer.
const COOLDOWN_TICKS: u64 = 256;
const OBJECTIVE_TICKS: u64 = 1_024;

/// A render frame of 100 ms: 6.4 ticks at 64 Hz, so the integer accumulator
/// has a carry to prove the doubling happens per frame, not from rounding.
const FRAME: Duration = Duration::from_millis(100);

fn relation_of(
    comparison: &cs_sim::time::OriginalPolicyComparison,
    field: &str,
) -> Option<PolicyRelation> {
    comparison
        .findings()
        .iter()
        .find(|finding| finding.field == field)
        .map(|finding| finding.relation)
}

/// AC3's decided behaviour, driven through the production `SessionDriver`:
/// a 2× single-player speed-up advances the weapon cooldown and the
/// objective timer by **twice** the ticks — per frame and in total — while
/// both timers and the two clocks stay in one lockstep tick count.
#[test]
fn accept_f16_f_two_x_speed_up_advances_cooldown_and_objective_by_twice_the_ticks() {
    let mut sped = SessionDriver::single_player(rate(), COOLDOWN_TICKS, OBJECTIVE_TICKS)
        .expect("periods are positive");
    let mut plain = SessionDriver::single_player(rate(), COOLDOWN_TICKS, OBJECTIVE_TICKS)
        .expect("periods are positive");

    let mut sped_total = 0u64;
    let mut plain_total = 0u64;
    for frame in 0..10 {
        let plain_ticks = plain
            .advance_frame(FRAME, SpeedUpRequest::Off)
            .expect("runs");
        let sped_ticks = sped.advance_frame(FRAME, SpeedUpRequest::On).expect("runs");
        plain_total += plain_ticks;
        sped_total += sped_ticks;
        assert_eq!(
            sped_ticks,
            plain_ticks * 2,
            "frame {frame}: a sped-up frame commits exactly twice the whole \
             ticks of the same plain frame, never a scaled dt"
        );
        assert_eq!(
            sped.gameplay_timeline().cooldown().elapsed_ticks(),
            sped_total,
            "frame {frame}: the weapon cooldown counted every committed tick"
        );
        assert_eq!(
            sped.gameplay_timeline().objective_timer().elapsed_ticks(),
            sped_total,
            "frame {frame}: the objective timer counted the same committed ticks"
        );
        assert_eq!(
            sped.gameplay_timeline().cooldown().remaining_ticks(),
            COOLDOWN_TICKS - sped_total,
        );
        assert_eq!(
            sped.gameplay_timeline().objective_timer().remaining_ticks(),
            OBJECTIVE_TICKS - sped_total,
        );
        assert_eq!(
            sped.session_clock().tick().0,
            sped_total,
            "the session clock is the one committing the ticks"
        );
        assert_eq!(
            sped.gameplay_timeline().tick().0,
            sped_total,
            "the gameplay timeline mirrors the session's committed time"
        );
        assert_eq!(
            sped_total,
            plain_total * 2,
            "in total the sped run is exactly twice the plain run at frame {frame}"
        );
    }
    assert_eq!(plain_total, 64, "1 s of wall time is 64 ticks at 64 Hz");
    assert_eq!(
        sped_total, 128,
        "1 s of wall time at 2x is 128 whole ticks at 64 Hz"
    );
}

/// AC2's multiplayer half: the same sped-up frame is refused by name before
/// anything mutates, and ordinary frames still run — the network gate the
/// original enforces (command 102 is off in network games) is this refusal.
#[test]
fn accept_f16_f_multiplayer_session_refuses_the_sped_up_frame() {
    let mut multi =
        SessionDriver::multiplayer(rate(), COOLDOWN_TICKS, OBJECTIVE_TICKS).expect("periods");

    assert_eq!(
        multi.advance_frame(Duration::from_secs(1), SpeedUpRequest::On),
        Err(TimeError::NoSpeedUpAuthority),
        "a multiplayer session grants no local speed-up authority"
    );
    assert_eq!(
        (
            multi.session_clock().tick().0,
            multi.gameplay_timeline().tick().0,
            multi.gameplay_timeline().cooldown().elapsed_ticks(),
            multi.gameplay_timeline().objective_timer().elapsed_ticks(),
        ),
        (0, 0, 0, 0),
        "a refused frame changes nothing anywhere"
    );

    // Ordinary frames are not refused: only the speed-up request is.
    assert_eq!(
        multi.advance_frame(Duration::from_secs(1), SpeedUpRequest::Off),
        Ok(64),
        "multiplayer still commits its ordinary wall-time ticks"
    );
    assert_eq!(
        multi.gameplay_timeline().cooldown().elapsed_ticks(),
        64,
        "ordinary frames reach the gameplay timers as before"
    );
}

/// AC04 through the driver: a paused frame commits nothing anywhere, whether
/// or not a speed-up was requested, and none of the paused time — doubled or
/// not — is banked for resume.
#[test]
fn accept_f16_f_paused_session_frame_commits_nothing_even_when_sped_up() {
    let mut driver = SessionDriver::single_player(rate(), COOLDOWN_TICKS, OBJECTIVE_TICKS)
        .expect("periods are positive");
    assert_eq!(driver.advance_frame(FRAME, SpeedUpRequest::Off), Ok(6));
    driver.set_paused(true);
    assert!(driver.is_paused());

    // 30 s of paused wall time, as one long frame and as short frames, with
    // the speed-up requested the whole time: all of it commits nothing.
    assert_eq!(
        driver.advance_frame(Duration::from_secs(30), SpeedUpRequest::On),
        Ok(0),
        "a paused frame commits no ticks, however long and however sped up"
    );
    for _ in 0..10 {
        assert_eq!(driver.advance_frame(FRAME, SpeedUpRequest::On), Ok(0));
    }
    assert_eq!(
        (
            driver.session_clock().tick().0,
            driver.gameplay_timeline().tick().0,
            driver.gameplay_timeline().cooldown().elapsed_ticks(),
            driver.gameplay_timeline().objective_timer().elapsed_ticks(),
        ),
        (6, 6, 6, 6),
        "pause advances neither the cooldown nor the objective timer"
    );

    driver.set_paused(false);
    assert_eq!(
        driver.advance_frame(Duration::from_secs(1), SpeedUpRequest::On),
        Ok(128),
        "resume banks nothing: one sped-up second is 128 ticks, not 30 s of them"
    );
    assert_eq!(
        driver.gameplay_timeline().cooldown().elapsed_ticks(),
        134,
        "the cooldown counted the 6 pre-pause ticks and the 128 resumed ones"
    );
    assert_eq!(
        driver.gameplay_timeline().objective_timer().elapsed_ticks(),
        134,
        "the objective timer moved by the same committed ticks"
    );
}

/// AC1/AC4: the comparison no longer reports a divergence for
/// `speed-up.reaches-gameplay-timers` — it **measures** the decided wiring at
/// the compared rate and agrees only when the production driver really
/// delivers the doubled ticks and really refuses the multiplayer frame.
#[test]
fn accept_f16_f_comparison_agrees_on_speed_up_reaching_gameplay_timers() {
    let comparison = OriginalClockPolicy::measured_original()
        .compare_project_clocks(rate(), ORIGINAL_POLICY_TOLERANCE);

    let finding = comparison
        .findings()
        .iter()
        .find(|finding| finding.field == "speed-up.reaches-gameplay-timers")
        .expect("the comparison reports the finding");
    assert_eq!(
        finding.relation,
        PolicyRelation::Agrees,
        "since #722 the session driver feeds the sped-up frame's committed \
         ticks to the gameplay timers, which is the original's behaviour in \
         whole ticks: {}",
        finding
    );
    assert!(
        finding.project.contains("#722"),
        "the finding must point at the task that decided the wiring: {}",
        finding.project
    );

    // The decided wiring changed exactly this one relation: the fixed-tick
    // mechanism divergence stays (F16 non-negotiable 4 decides it), and the
    // network gate agreement stays.
    assert_eq!(
        relation_of(&comparison, "speed-up.mechanism"),
        Some(PolicyRelation::Diverges),
        "the mechanism is still whole fixed ticks, never the original's \
         variable-dt multiplier"
    );
    assert_eq!(
        relation_of(&comparison, "speed-up.network-gate"),
        Some(PolicyRelation::Agrees)
    );
    let mut divergences: Vec<&'static str> = comparison
        .divergences()
        .iter()
        .map(|finding| finding.field)
        .collect();
    divergences.sort_unstable();
    assert_eq!(
        divergences,
        vec![
            "clock.frame-dt-cap",
            "clock.tick-source",
            "speed-up.mechanism"
        ],
        "speed-up.reaches-gameplay-timers left the divergence set when #722 \
         implemented the decided wiring; the other three are unchanged"
    );
    assert_eq!(
        comparison.claim(),
        cs_types::evidence::ClaimStatus::Inferred
    );
}
