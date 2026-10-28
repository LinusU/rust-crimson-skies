//! Task T452-REG-BEEPER-SEEKER (#2178): the measured `BEEPER_SEEKER` retargeting
//! in the runtime guidance vocabulary.
//!
//! `wep_11` declares `BEEPER_SEEKER`, which the engine turns into a per-step
//! callback (`0x441780`, installed at `0x441877`, called at `0x5af745`) that
//! writes the best live beeper into the projectile's target slot or zeroes it
//! (T452-R.02 §1, T2116). These tests take a measured record declaring it
//! through the production lowering and fly it against a live marker ledger.
//! Synthetic records: they prove the rule, not the retail record's values.

use cs_app::ordnance::{lower_ordnance, measured_ordnance_declaration};
use cs_content::ordnance::{
    OriginalStatusEffectDeclaration, T452_R02_BEEPER_SEEKER, measure_component_behavior,
};
use cs_sim::weapons::{
    GuidanceRule, GuidanceUpdate, MountTransform, OrdnanceComponent, OrdnanceRuntime, ProjectileId,
    StatusEffectTarget, TargetObservation,
};
use cs_types::Tick;
use cs_types::space::{UnitVec3, WorldPosition};

use super::accept_t452_register::{
    PRODUCER, SESSION, actor, guided_ammo, guided_media, guided_record, guided_row, provenance,
    rate, session_id,
};

fn component(record: &str, status: OriginalStatusEffectDeclaration) -> OrdnanceComponent {
    let mut row = guided_row();
    row.record = record.to_owned();
    row.statuses = vec![status];
    let mut weapon = guided_record();
    weapon.record = record.to_owned();
    let mut media = guided_media();
    media.record = record.to_owned();
    let mut ammo = guided_ammo();
    ammo.record = record.to_owned();
    let declared = measured_ordnance_declaration(&row, &weapon, &media, &ammo, &provenance(), 30)
        .expect("the synthetic record's values are all expressible");
    lower_ordnance(&declared).expect("the measured record lowers")
}

fn seeker() -> OrdnanceComponent {
    component(
        "wep_11",
        OriginalStatusEffectDeclaration {
            key: "BEEPER_SEEKER",
            duration_s: None,
            duration_unit: None,
        },
    )
}

/// The beeper emitter: a measured `BEEPER` marker of 20 s (600 ticks).
fn emitter() -> OrdnanceComponent {
    component(
        "wep_10",
        OriginalStatusEffectDeclaration {
            key: "BEEPER",
            duration_s: Some(20.0),
            duration_unit: Some("TIME".to_owned()),
        },
    )
}

fn projectile(serial: u64) -> ProjectileId {
    ProjectileId {
        session: SESSION,
        serial,
    }
}

fn transform() -> MountTransform {
    MountTransform::try_new(
        WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("a unit vector"),
        [0.0; 3],
    )
    .expect("the fixture mount is valid")
}

fn marker_on(runtime: &mut OrdnanceRuntime, emitter_item: ProjectileId, victim: u64, at: u64) {
    runtime
        .advance_status(session_id(), Tick(at))
        .expect("the ledger moves forward");
    runtime
        .apply_statuses(
            session_id(),
            Tick(at),
            &emitter_item,
            StatusEffectTarget::whole_actor(actor(victim)),
        )
        .expect("the emitter's measured marker applies");
}

/// The record's `BEEPER_SEEKER` lowers to the retargeting rule, and the claim
/// that kept it off the runtime is no longer named by the measured behavior.
#[test]
fn accept_t452_beeper_the_measured_seeker_lowers_to_the_retargeting_rule() {
    let component = seeker();
    let projectile = component.as_projectile().expect("a projectile");
    assert_eq!(projectile.guidance(), GuidanceRule::MarkerSeeker);
    assert!(projectile.guidance().is_targeted());
    assert_eq!(projectile.guidance().lost_target(), None);

    let mut row = guided_row();
    row.record = "wep_11".to_owned();
    row.statuses = vec![OriginalStatusEffectDeclaration {
        key: "BEEPER_SEEKER",
        duration_s: None,
        duration_unit: None,
    }];
    let behavior = measure_component_behavior(&row);
    assert!(!behavior.names_claim(T452_R02_BEEPER_SEEKER));
}

/// Each step the seeker takes the newest live marker, switches when a newer
/// one appears, falls back when markers expire, and flies unguided when none
/// is live.
#[test]
fn accept_t452_beeper_the_seeker_takes_the_newest_live_marker_each_step() {
    let mut runtime = OrdnanceRuntime::new(session_id(), Tick(0), rate(), PRODUCER);
    let emitter_item = projectile(1);
    let seeker_item = projectile(2);
    let emitter_component = emitter();
    let seeker_component = seeker();
    runtime
        .launch(
            actor(1),
            emitter_item,
            emitter_component.as_projectile().expect("a projectile"),
            &transform(),
            None,
            [0.0; 3],
        )
        .expect("the emitter launches");
    runtime
        .launch(
            actor(1),
            seeker_item,
            seeker_component.as_projectile().expect("a projectile"),
            &transform(),
            // Even a target named at launch is overwritten by the callback.
            Some(actor(9)),
            [0.0; 3],
        )
        .expect("the seeker launches");
    let target_of = |runtime: &OrdnanceRuntime| {
        runtime
            .guidance()
            .tracker(&seeker_item)
            .expect("the seeker has a tracker")
            .target()
    };
    let step = |runtime: &mut OrdnanceRuntime, at: u64| {
        runtime.retarget_marker_seekers(session_id());
        runtime.guidance_tick(
            session_id(),
            TargetObservation {
                tick: Tick(at),
                target: None,
                lost: None,
            },
        )
    };

    // No marker is live: no target and unguided flight, not a lost target.
    let tick = step(&mut runtime, 0);
    assert_eq!(target_of(&runtime), None);
    assert_eq!(tick.updates[&seeker_item], GuidanceUpdate::Unguided);

    marker_on(&mut runtime, emitter_item, 5, 1);
    step(&mut runtime, 1);
    assert_eq!(target_of(&runtime), Some(actor(5)));

    marker_on(&mut runtime, emitter_item, 6, 3);
    let tick = step(&mut runtime, 3);
    assert_eq!(
        target_of(&runtime),
        Some(actor(6)),
        "the newest marker wins"
    );
    assert_eq!(
        tick.updates[&seeker_item],
        GuidanceUpdate::Tracked { target: actor(6) }
    );

    marker_on(&mut runtime, emitter_item, 7, 10);
    step(&mut runtime, 10);
    assert_eq!(target_of(&runtime), Some(actor(7)));

    // Markers last 600 ticks: 6's ends at 603 while 7's (610) still stands, and
    // the loss of an older marker is no news to a seeker on a newer one.
    runtime
        .advance_status(session_id(), Tick(604))
        .expect("the ledger moves forward");
    step(&mut runtime, 604);
    assert_eq!(target_of(&runtime), Some(actor(7)));

    // The last marker ends: the seeker is not terminal, it flies unguided.
    runtime
        .advance_status(session_id(), Tick(611))
        .expect("the ledger moves forward");
    let tick = step(&mut runtime, 611);
    assert_eq!(target_of(&runtime), None);
    assert_eq!(tick.updates[&seeker_item], GuidanceUpdate::Unguided);

    // ... and a marker applied afterwards is taken again, because loss here is
    // not terminal (T2116).
    marker_on(&mut runtime, emitter_item, 8, 620);
    step(&mut runtime, 620);
    assert_eq!(target_of(&runtime), Some(actor(8)));
}

/// F28 AC02 stays green: the seeker's target is lost on destruction of the
/// tracked actor and on a session change, and a session change is final.
#[test]
fn accept_t452_beeper_a_session_change_ends_the_seeking() {
    let mut runtime = OrdnanceRuntime::new(session_id(), Tick(0), rate(), PRODUCER);
    let emitter_item = projectile(1);
    let seeker_item = projectile(2);
    let emitter_component = emitter();
    let seeker_component = seeker();
    for (item, component) in [
        (emitter_item, &emitter_component),
        (seeker_item, &seeker_component),
    ] {
        runtime
            .launch(
                actor(1),
                item,
                component.as_projectile().expect("a projectile"),
                &transform(),
                None,
                [0.0; 3],
            )
            .expect("the fixture launches");
    }
    marker_on(&mut runtime, emitter_item, 5, 1);
    runtime.retarget_marker_seekers(session_id());
    let target = || {
        runtime
            .guidance()
            .tracker(&seeker_item)
            .expect("a tracker")
            .target()
    };
    assert_eq!(target(), Some(actor(5)));

    // The tracked actor is destroyed: the seeker drops it this step.
    runtime.guidance_tick(
        session_id(),
        TargetObservation::destroyed(actor(5), Tick(2)),
    );
    assert_eq!(
        runtime
            .guidance()
            .tracker(&seeker_item)
            .expect("a tracker")
            .target(),
        None
    );

    // A different session generation loses it for good, markers or not.
    runtime.retarget_marker_seekers(session_id());
    let other = cs_types::net::SessionId::new(SESSION + 1).expect("nonzero");
    runtime.guidance_tick(other, TargetObservation::alive(actor(5), Tick(3)));
    runtime.retarget_marker_seekers(session_id());
    let tracker = runtime.guidance().tracker(&seeker_item).expect("a tracker");
    assert_eq!(tracker.target(), None);
    assert!(tracker.lost().is_some());
}
