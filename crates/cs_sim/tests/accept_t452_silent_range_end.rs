//! Task T452-REG-SILENT-EXPIRY (#2175): the measured end of flight at the end
//! of `RANGE`.
//!
//! `docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md` §3
//! measured that the original's projectile step ends a flight at
//! `0x5b01ec`/`0x5b0318` by marking the item dead with **no detonation** when
//! neither `LOCK_ON` nor `DETONATE_AT_RANGE` is declared. [`FuseRule`] could
//! only impact, proximity or *detonate on a clock*, so the measured end had no
//! runtime variant. This module drives the state and the runtime, not a copy of
//! them: every assertion calls production code.
//!
//! No test awards `verified_original`: the measurement is static analysis of the
//! owner's decrypted image, recorded in the finding above.

use std::collections::BTreeSet;

use cs_sim::damage::ActorId;
use cs_sim::time::TickRate;
use cs_sim::weapons::ordnance::{
    ArmingRule, EquipmentRules, FuseDecision, FuseInert, FuseRule, FuseTrigger, GuidanceRule,
    OrdnanceDefinitionError, OrdnanceFamily, OrdnanceRuntime, OrdnanceState, ProjectileOrdnance,
    StackLoad, synthetic_channels, synthetic_launch_geometry, synthetic_media, synthetic_ordnance,
};
use cs_sim::weapons::{MountTransform, ProjectileId, ProjectileSegment, SweptHit};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 91;
const PRODUCER: u32 = 92;
const TICKS_PER_SECOND: u32 = 30;

/// The synthetic component's launch speed is `210` m/s at `30` Hz, so one tick
/// of flight is exactly `7` m — the fixture's range bound is a whole number of
/// ticks, which is what makes the precedence assertions exact.
const METRES_PER_TICK: f64 = 210.0 / 30.0;

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(),
        serial,
    }
}

fn projectile(serial: u64) -> ProjectileId {
    ProjectileId {
        session: SESSION,
        serial,
    }
}

fn rate() -> TickRate {
    TickRate::new(TICKS_PER_SECOND).expect("the test rate is nonzero")
}

fn tick_seconds() -> f64 {
    1.0 / f64::from(TICKS_PER_SECOND)
}

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

/// One tick's swept segment along -Z, the canonical forward.
fn segment(from_z: f64, to_z: f64) -> ProjectileSegment {
    ProjectileSegment {
        projectile: projectile(1),
        previous: position([0.0, 0.0, from_z]),
        current: position([0.0, 0.0, to_z]),
    }
}

fn transform() -> MountTransform {
    MountTransform::try_new(
        position([0.0, 0.0, 0.0]),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("the canonical forward is a unit vector"),
        [0.0, 0.0, 0.0],
    )
    .expect("the fixture mount is valid")
}

/// An in-flight state with the measured range end as its fuse.
fn range_bounded(range_m: f64, lifetime_ticks: u64) -> OrdnanceState {
    let definition = area_denial_ending_at(range_m, lifetime_ticks);
    OrdnanceState::launch(
        projectile(1),
        definition.ordnance().clone(),
        definition.family(),
        definition.arming(),
        definition.fuse(),
        definition.lifetime_ticks(),
        Tick(0),
    )
}

/// An area-denial component whose measured end of flight is the record's
/// `RANGE`: unguided, so the family check passes, and ending on its own rule
/// rather than on contact.
fn area_denial_ending_at(range_m: f64, lifetime_ticks: u64) -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance("synthetic.range_ending_area_denial"),
        OrdnanceFamily::AreaDenialEngine,
        synthetic_launch_geometry(),
        StackLoad::try_new(6, 0.0).expect("a six-round stack is valid"),
        ArmingRule::AfterTicks(0),
        FuseRule::ExpiresAtRange { range_m },
        GuidanceRule::Unguided,
        lifetime_ticks,
        None,
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the range-ending area-denial fixture is coherent")
}

// ------------------------------------------------------------------- tests ---

/// The measured end consumes the item at its `RANGE` without detonating, and
/// the end is not a detonation: no trigger, no area, no damage routing.
#[test]
fn accept_t452_silent_the_measured_range_end_consumes_the_item_without_detonating() {
    let mut state = range_bounded(20.0, 100);

    // Below the bound the fuse has done nothing: this is not a detonation and
    // not an expiry.
    state.advance(&segment(0.0, -10.0));
    assert_eq!(
        state.fuse_decision(&segment(0.0, -10.0), &[], &[]),
        FuseDecision::Inert(FuseInert::NothingTriggered)
    );
    assert!(!state.is_triggered());

    // At the bound the item is consumed.
    state.advance(&segment(-10.0, -20.0));
    let decision = state.fuse_decision(&segment(-10.0, -20.0), &[], &[]);
    assert_eq!(
        decision,
        FuseDecision::Consumed { range_m: 20.0 },
        "the item ends at the record's RANGE"
    );
    assert_eq!(decision.consumed_range_m(), Some(20.0));
    assert!(decision.has_ended());
    assert!(
        decision.trigger().is_none(),
        "a consumption is not a detonation"
    );
    assert_eq!(state.consumed_range_m(), Some(20.0));
    assert!(state.is_consumed());
    assert!(
        !state.is_triggered(),
        "nothing detonated: the measured end is silent"
    );
    assert_eq!(state.triggered(), None);

    // One end per item: a second call reports the same consumption rather than
    // inventing a detonation, and the ended item stops counting distance.
    assert_eq!(
        state.fuse_decision(&segment(-20.0, -30.0), &[], &[]),
        FuseDecision::Consumed { range_m: 20.0 }
    );
    let travelled = state.travelled_m();
    state.advance(&segment(-20.0, -30.0));
    assert_eq!(state.travelled_m(), travelled);
}

/// A swept contact still detonates a range-bounded item: the measurement covers
/// the end of `RANGE`, and nothing measured says a collision cannot detonate
/// these records. The contact is therefore decided first.
#[test]
fn accept_t452_silent_a_contact_still_detonates_a_range_bounded_item() {
    let mut state = range_bounded(20.0, 100);
    state.advance(&segment(0.0, -20.0));
    let hit = SweptHit {
        projectile: projectile(1),
        target: actor(2),
        time_of_impact: 0.5,
    };
    let decision = state.fuse_decision(&segment(0.0, -20.0), &[], &[hit]);
    assert_eq!(
        decision,
        FuseDecision::Triggered(FuseTrigger::Impact {
            target: actor(2),
            time: 0.5,
        }),
        "the contact wins, and the range end does not hide it"
    );
    assert!(state.is_triggered());
    assert!(
        !state.is_consumed(),
        "a detonation is not a silent consumption"
    );
}

/// The measured range end is reported instead of the designed lifetime cap it
/// was derived from, when both boundaries fall on the same tick — and the item
/// is removed then, not later.
#[test]
fn accept_t452_silent_the_range_end_is_reported_instead_of_the_lifetime_cap() {
    // 28 m is exactly four ticks at 7 m/tick, and the cap is those four ticks:
    // the same boundary the measured lowering derives from RANGE.
    let definition = area_denial_ending_at(4.0 * METRES_PER_TICK, 4);
    assert_eq!(
        definition.fuse(),
        FuseRule::ExpiresAtRange {
            range_m: 4.0 * METRES_PER_TICK
        }
    );
    let mut runtime = OrdnanceRuntime::new(session(), Tick(0), rate(), PRODUCER);
    runtime
        .launch(
            actor(1),
            projectile(1),
            &definition,
            &transform(),
            None,
            [0.0; 3],
        )
        .expect("the fixture launches");

    let mut consumptions = Vec::new();
    for _ in 0..4 {
        let advanced = runtime
            .advance(tick_seconds(), [0.0; 3])
            .expect("the fixture advances");
        assert!(
            advanced.expired.is_empty(),
            "the measured end is not the lifetime cap: {:?}",
            advanced.expired
        );
        assert!(advanced.triggered.is_empty(), "nothing detonated");
        match runtime
            .decide(&projectile(1), &[], &[])
            .expect("the live item decides")
        {
            FuseDecision::Consumed { range_m } => consumptions.push(range_m),
            FuseDecision::Inert(_) => {}
            other => panic!("unexpected decision {other:?}"),
        }
    }

    assert_eq!(
        consumptions,
        vec![4.0 * METRES_PER_TICK],
        "the item is consumed once, on the tick the path reaches RANGE"
    );
    let live = runtime
        .get(&projectile(1))
        .expect("the runtime still holds it");
    assert_eq!(live.state().consumed_range_m(), Some(4.0 * METRES_PER_TICK));
    // The session removes a consumed item; the runtime's removal is idempotent.
    runtime.remove(&projectile(1));
    assert!(runtime.get(&projectile(1)).is_none());
}

/// A range bound that is zero, negative or non-finite is refused rather than
/// silently turned into "consumed at once".
#[test]
fn accept_t452_silent_the_runtime_refuses_a_range_bound_that_cannot_be_flown() {
    let refused = |range_m: f64| {
        ProjectileOrdnance::try_new(
            synthetic_ordnance("synthetic.range_ending_area_denial"),
            OrdnanceFamily::AreaDenialEngine,
            synthetic_launch_geometry(),
            StackLoad::try_new(6, 0.0).expect("a six-round stack is valid"),
            ArmingRule::AfterTicks(0),
            FuseRule::ExpiresAtRange { range_m },
            GuidanceRule::Unguided,
            100,
            None,
            synthetic_channels(),
            Vec::new(),
            synthetic_media(),
            EquipmentRules::new(None, BTreeSet::new()),
        )
        .expect_err("an unflown range bound is refused")
    };
    assert_eq!(
        refused(0.0),
        OrdnanceDefinitionError::NonPositiveFuseRange { range_m: 0.0 }
    );
    assert!(matches!(
        refused(-5.0),
        OrdnanceDefinitionError::NonPositiveFuseRange { .. }
    ));
    assert_eq!(
        refused(f64::NAN),
        OrdnanceDefinitionError::NonFiniteFuseRange
    );
}

/// An area-denial component may end on its measured `RANGE`, and still may not
/// be impact-fused: the family's own rule is unchanged, only the vocabulary for
/// the end it does have has grown.
#[test]
fn accept_t452_silent_an_area_denial_component_end_on_range_but_not_on_contact() {
    let ending = area_denial_ending_at(500.0, 126);
    assert_eq!(
        ending.fuse(),
        FuseRule::ExpiresAtRange { range_m: 500.0 },
        "the measured end is the record's RANGE"
    );
    assert_eq!(ending.family(), OrdnanceFamily::AreaDenialEngine);

    let refused = ProjectileOrdnance::try_new(
        synthetic_ordnance("synthetic.impact_ending_area_denial"),
        OrdnanceFamily::AreaDenialEngine,
        synthetic_launch_geometry(),
        StackLoad::try_new(6, 0.0).expect("a six-round stack is valid"),
        ArmingRule::AfterTicks(0),
        FuseRule::Impact,
        GuidanceRule::Unguided,
        100,
        None,
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect_err("an area-denial component still may not end on contact");
    assert!(matches!(
        refused,
        OrdnanceDefinitionError::IncoherentFamily {
            family: OrdnanceFamily::AreaDenialEngine,
            field: "fuse",
            ..
        }
    ));
}
