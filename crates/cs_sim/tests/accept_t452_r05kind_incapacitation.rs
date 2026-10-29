//! T452-R05-STATUS-KIND (#2172): the pilot-incapacitation status a measured
//! `SONIC`/`FLASH` hit applies, through production code.

use cs_sim::damage::ActorId;
use cs_sim::time::TickRate;
use cs_sim::weapons::incapacitation::{
    CONTROL_WORD_COUNT, ExposureBit, ExposureGeometry, ExposureOutcome, INCAPACITATED_MODE,
    IncapacitationVictim, apply_exposure, effective_controls, incapacitated_mode, measure_exposure,
};
use cs_sim::weapons::ordnance::{
    OrdnanceId, StatusEffectKind, StatusEffectLedger, StatusEffectTarget,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

const TICKS_PER_SECOND: u32 = 30;

fn session() -> SessionId {
    SessionId::new(7).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(),
        serial,
    }
}

fn source() -> OrdnanceId {
    OrdnanceId::try_new(
        ContentId::from_source(ContentKind::Weapon, "synthetic.sonic").expect("valid id"),
    )
    .expect("the weapon namespace is correct")
}

fn rate() -> TickRate {
    TickRate::new(TICKS_PER_SECOND).expect("a nonzero rate")
}

fn npc() -> IncapacitationVictim {
    IncapacitationVictim {
        is_player: false,
        exempt: false,
        busy: false,
        state_word: 0,
    }
}

/// Squared distance for a ratio of exactly `ratio` over a 100 m proximity.
fn at_ratio(ratio: f64, facing_dot: f64) -> ExposureGeometry {
    ExposureGeometry {
        distance_squared_m2: ratio * 100.0 * 100.0,
        impact_proximity_m: 100.0,
        facing_dot,
    }
}

#[test]
fn accept_t452_r05kind_the_variant_has_a_stable_surface() {
    assert!(StatusEffectKind::ALL.contains(&StatusEffectKind::Incapacitation));
    assert_eq!(StatusEffectKind::Incapacitation.label(), "incapacitation");
    assert_eq!(
        StatusEffectKind::Incapacitation.to_string(),
        "incapacitation"
    );
    assert!(StatusEffectKind::Incapacitation.is_degrading());
}

#[test]
fn accept_t452_r05kind_duration_is_intensity_times_five_seconds_by_ratio() {
    let full = measure_exposure(ExposureBit::Sonic, &at_ratio(0.6, 0.0)).unwrap();
    assert!((full.intensity - 1.0).abs() < 1e-12);
    assert!((full.duration_s - 5.0).abs() < 1e-12);
    // ratio 0.8: 1 - 2.5 * 0.2 = 0.5.
    let half = measure_exposure(ExposureBit::Sonic, &at_ratio(0.8, 0.0)).unwrap();
    assert!((half.intensity - 0.5).abs() < 1e-12);
    assert!((half.duration_s - 2.5).abs() < 1e-12);
    // ratio 1.0 and beyond: nothing.
    assert_eq!(
        measure_exposure(ExposureBit::Sonic, &at_ratio(1.0, 0.0))
            .unwrap()
            .intensity,
        0.0
    );
    assert_eq!(
        measure_exposure(ExposureBit::Sonic, &at_ratio(3.0, 0.0))
            .unwrap()
            .duration_s,
        0.0
    );
}

#[test]
fn accept_t452_r05kind_flash_is_scaled_by_facing_and_sonic_is_not() {
    // Facing dot 0.25 -> 2 * 0.25 = 0.5; behind the victim -> nothing.
    let flash = measure_exposure(ExposureBit::Flash, &at_ratio(0.6, 0.25)).unwrap();
    assert!((flash.duration_s - 2.5).abs() < 1e-12);
    let behind = measure_exposure(ExposureBit::Flash, &at_ratio(0.6, -0.5)).unwrap();
    assert_eq!(behind.intensity, 0.0);
    let past_half = measure_exposure(ExposureBit::Flash, &at_ratio(0.6, 0.9)).unwrap();
    assert!((past_half.duration_s - 5.0).abs() < 1e-12);
    let sonic = measure_exposure(ExposureBit::Sonic, &at_ratio(0.6, -0.5)).unwrap();
    assert!((sonic.duration_s - 5.0).abs() < 1e-12);
}

#[test]
fn accept_t452_r05kind_a_struck_actor_is_incapacitated_until_the_clock_reaches_the_deadline() {
    let mut ledger = StatusEffectLedger::new(session(), Tick(100));
    let victim = actor(3);
    let outcome = apply_exposure(
        &mut ledger,
        session(),
        Tick(100),
        rate(),
        victim,
        &source(),
        ExposureBit::Sonic,
        &at_ratio(0.8, 0.0),
        &npc(),
    )
    .unwrap();
    // 2.5 s at 30 Hz is 75 ticks.
    let ExposureOutcome::Incapacitated {
        ticks, expires_at, ..
    } = outcome
    else {
        panic!("a non-player in range is incapacitated, got {outcome:?}");
    };
    assert_eq!(ticks, 75);
    assert_eq!(expires_at, Tick(175));

    assert_eq!(
        incapacitated_mode(&ledger, victim),
        Some(INCAPACITATED_MODE)
    );
    let requested = [1.0; CONTROL_WORD_COUNT];
    assert_eq!(
        effective_controls(&ledger, victim, requested),
        [0.0; CONTROL_WORD_COUNT]
    );
    // Another actor is untouched.
    assert_eq!(effective_controls(&ledger, actor(4), requested), requested);

    // Still incapacitated on the last tick before the deadline...
    let expired = ledger.advance_to(session(), Tick(174)).unwrap();
    assert!(expired.is_empty());
    assert!(incapacitated_mode(&ledger, victim).is_some());
    // ...and recovered on the deadline tick itself (clock >= deadline).
    let expired = ledger.advance_to(session(), Tick(175)).unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].kind, StatusEffectKind::Incapacitation);
    assert_eq!(expired[0].expired_at, Tick(175));
    assert_eq!(incapacitated_mode(&ledger, victim), None);
    assert_eq!(effective_controls(&ledger, victim, requested), requested);
}

#[test]
fn accept_t452_r05kind_a_duration_between_ticks_rounds_up_to_the_reaching_tick() {
    let mut ledger = StatusEffectLedger::new(session(), Tick(0));
    // ratio 0.7: intensity 0.75, 3.75 s * 30 = 112.5 ticks -> 113.
    let outcome = apply_exposure(
        &mut ledger,
        session(),
        Tick(0),
        rate(),
        actor(1),
        &source(),
        ExposureBit::Sonic,
        &at_ratio(0.7, 0.0),
        &npc(),
    )
    .unwrap();
    let ExposureOutcome::Incapacitated { ticks, .. } = outcome else {
        panic!("expected incapacitation");
    };
    assert_eq!(ticks, 113);
}

#[test]
fn accept_t452_r05kind_the_player_is_not_control_incapacitated() {
    let mut ledger = StatusEffectLedger::new(session(), Tick(0));
    let player = actor(0);
    let victim = IncapacitationVictim {
        is_player: true,
        ..npc()
    };
    let outcome = apply_exposure(
        &mut ledger,
        session(),
        Tick(0),
        rate(),
        player,
        &source(),
        ExposureBit::Flash,
        &at_ratio(0.5, 1.0),
        &victim,
    )
    .unwrap();
    let ExposureOutcome::PlayerScreenAudio(exposure) = outcome else {
        panic!("the player gets a screen/audio exposure, got {outcome:?}");
    };
    assert!((exposure.duration_s - 5.0).abs() < 1e-12);
    assert!(ledger.is_empty());
    assert_eq!(incapacitated_mode(&ledger, player), None);
    let requested = [0.5; CONTROL_WORD_COUNT];
    assert_eq!(effective_controls(&ledger, player, requested), requested);
}

#[test]
fn accept_t452_r05kind_the_gate_refuses_exempt_busy_and_wrong_state_victims() {
    for (victim, expect_exempt) in [
        (
            IncapacitationVictim {
                exempt: true,
                ..npc()
            },
            true,
        ),
        (
            IncapacitationVictim {
                busy: true,
                ..npc()
            },
            false,
        ),
        (
            IncapacitationVictim {
                state_word: 2,
                ..npc()
            },
            false,
        ),
    ] {
        let mut ledger = StatusEffectLedger::new(session(), Tick(0));
        let outcome = apply_exposure(
            &mut ledger,
            session(),
            Tick(0),
            rate(),
            actor(2),
            &source(),
            ExposureBit::Sonic,
            &at_ratio(0.5, 0.0),
            &victim,
        )
        .unwrap();
        let expected = if expect_exempt {
            ExposureOutcome::Exempt
        } else {
            ExposureOutcome::Gated
        };
        assert_eq!(outcome, expected);
        assert!(ledger.is_empty());
    }
    // State 4 (already incapacitated) is admitted.
    assert!(
        IncapacitationVictim {
            state_word: 4,
            ..npc()
        }
        .admits_incapacitation()
    );
}

#[test]
fn accept_t452_r05kind_a_second_hit_replaces_the_deadline_rather_than_stacking() {
    let mut ledger = StatusEffectLedger::new(session(), Tick(0));
    let victim = actor(5);
    for (at, ratio) in [(0_u64, 0.5), (30, 0.8)] {
        ledger.advance_to(session(), Tick(at)).unwrap();
        apply_exposure(
            &mut ledger,
            session(),
            Tick(at),
            rate(),
            victim,
            &source(),
            ExposureBit::Sonic,
            &at_ratio(ratio, 0.0),
            &npc(),
        )
        .unwrap();
    }
    let target = StatusEffectTarget::whole_actor(victim);
    assert_eq!(ledger.effects_on(&target).len(), 1);
    // The second hit: tick 30 + 75 = 105, shorter than the first's 150.
    ledger.advance_to(session(), Tick(104)).unwrap();
    assert!(ledger.is_under(&target, StatusEffectKind::Incapacitation));
    ledger.advance_to(session(), Tick(105)).unwrap();
    assert!(!ledger.is_under(&target, StatusEffectKind::Incapacitation));
}

#[test]
fn accept_t452_r05kind_a_hit_outside_the_falloff_applies_nothing() {
    let mut ledger = StatusEffectLedger::new(session(), Tick(0));
    let outcome = apply_exposure(
        &mut ledger,
        session(),
        Tick(0),
        rate(),
        actor(2),
        &source(),
        ExposureBit::Sonic,
        &at_ratio(1.5, 0.0),
        &npc(),
    )
    .unwrap();
    assert_eq!(outcome, ExposureOutcome::NoEffect);
    assert!(ledger.is_empty());
}
