//! The animation `HEALTH` consumer: a weapon hit on a target lowers its
//! animation record's health pool and a pool at or below zero starts the
//! record (M01-HYDROGEN-FIRE-HOOK, #2088).
//!
//! # What is measured
//!
//! `docs/findings/2026-10-24-m01-animation-health-consumer.md`, rows
//! `[[animation_health_fact]]` of `data/owner-static/engine-facts.toml`
//! (provenance `static`). An `ANIMATION_DEFINITION` that states no
//! `ACTIVATION` is activation 0, the weapon-hit mode, and registers handler
//! `0x4e7220` on its target node. For every weapon hit on that node the
//! handler takes the health channel of the hit's damage pair, subtracts the
//! definition's `ARMOR`, lowers the pool (the record's `HEALTH`) by the
//! result when it is positive, and, once the pool is at or below zero, starts
//! the record's default sequence at zero.
//!
//! # What this consumer does, and what stays open
//!
//! * The pool starts at the record's `max_health` and takes the
//!   [`DamageChannel::Internal`] amount of every hit on the target's damage
//!   node: the same `HEALTH_DAMAGE` unit the engine's pair carries.
//! * `ARMOR` is taken as 0: the zeroed allocation default. The definition
//!   reader does not surface an `ARMOR` field, so a definition that states
//!   one is not told apart (the M01 tank definition states none).
//! * The record starts **once**, on the hit that takes the pool to zero. The
//!   engine calls its starter again on every later hit; whether that starter
//!   restarts a finished record is not traced (`0x4ed8c0` refuses several
//!   instance kinds), so no restart is invented.
//! * `DAMAGE_SEQUENCE` (the hit-reaction start `0x4e71e0`), the collide
//!   activations and the weapon flag `FREEZE` filter are not modelled.
//! * Only the mission's bound ground targets are armed; the zeppelin part
//!   records that share the shape are a separate consumer.

use cs_sim::damage::{DamageChannel, HitEvent};
use cs_types::Tick;

use crate::animation::mission::{AnimationNameIndex, NamedAnimationResolution};
use crate::animation::survey::CarrierKind;
use crate::mission_animations::{ActivationOutcome, MissionAnimationPlayer};

/// The claim the hit-to-health coupling is recorded under.
pub const ANIMATION_HEALTH_CLAIM: &str = "m01-anim.health-hit-consumer-static";

/// The activation byte of the weapon-hit mode (a definition with no
/// `ACTIVATION`).
pub const WEAPON_HIT_ACTIVATION: u8 = 0;

/// One armed health pool: a record, the node it watches and what is left.
#[derive(Clone, Debug, PartialEq)]
pub struct HealthPool {
    animation: String,
    node: String,
    maximum: f32,
    remaining: f32,
    started: bool,
}

impl HealthPool {
    /// The record the pool starts.
    #[must_use]
    pub fn animation(&self) -> &str {
        &self.animation
    }

    /// The target node whose hits it counts.
    #[must_use]
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The record's `HEALTH`.
    #[must_use]
    pub const fn maximum(&self) -> f32 {
        self.maximum
    }

    /// What is left of it; may go below zero (the engine does not clamp).
    #[must_use]
    pub const fn remaining(&self) -> f32 {
        self.remaining
    }
}

/// One record this consumer started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthStart {
    /// The record started.
    pub animation: String,
    /// The target node whose hit emptied the pool.
    pub node: String,
    /// The tick the start was made on.
    pub at: Tick,
    /// What the player answered.
    pub outcome: ActivationOutcome,
}

/// The weapon-hit health pools of a mission's animation records.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationHealthLedger {
    pools: Vec<HealthPool>,
    starts: Vec<HealthStart>,
}

impl AnimationHealthLedger {
    /// Arms a pool for every record of the mission carrier whose activation is
    /// the weapon-hit mode, whose `HEALTH` is positive and whose object is one
    /// of `targets` (the bound ground targets' node names).
    #[must_use]
    pub fn arm<'a>(names: &AnimationNameIndex, targets: impl IntoIterator<Item = &'a str>) -> Self {
        let targets: Vec<&str> = targets.into_iter().collect();
        let mut pools = Vec::new();
        for carrier in names.carriers() {
            if carrier.kind() != CarrierKind::Mission {
                continue;
            }
            let Some(table) = carrier.names() else {
                continue;
            };
            for index in 0..table.count {
                let Some(facts) = carrier.record(index) else {
                    continue;
                };
                let health = facts.max_health();
                if facts.activation() != WEAPON_HIT_ACTIVATION
                    || !health.is_finite()
                    || health <= 0.0
                    || !targets.contains(&facts.object_name())
                {
                    continue;
                }
                pools.push(HealthPool {
                    animation: facts.anim_name().to_owned(),
                    node: facts.object_name().to_owned(),
                    maximum: health,
                    remaining: health,
                    started: false,
                });
            }
        }
        Self {
            pools,
            starts: Vec::new(),
        }
    }

    /// Every armed pool, in carrier order.
    #[must_use]
    pub fn pools(&self) -> &[HealthPool] {
        &self.pools
    }

    /// Every record this ledger started, in order.
    #[must_use]
    pub fn starts(&self) -> &[HealthStart] {
        &self.starts
    }

    /// Counts `hits` against the pools and starts every record whose pool
    /// reached zero, through `player` at `tick`. Answers the starts made.
    pub fn consume(
        &mut self,
        hits: &[HitEvent],
        names: &AnimationNameIndex,
        player: &mut MissionAnimationPlayer,
        tick: Tick,
    ) -> Vec<HealthStart> {
        let mut made = Vec::new();
        for hit in hits {
            if hit.channel != DamageChannel::Internal {
                continue;
            }
            for pool in self
                .pools
                .iter_mut()
                .filter(|pool| pool.node == hit.node.as_str())
            {
                // `4e724a`: the counted amount is the health channel less the
                // armor (taken as 0); only a positive amount lowers the pool.
                let amount = hit.damage as f32;
                if amount > 0.0 {
                    pool.remaining -= amount;
                }
                if pool.remaining > 0.0 || pool.started {
                    continue;
                }
                pool.started = true;
                let outcome = match names.resolve(&pool.animation) {
                    NamedAnimationResolution::Bound(row) => player.activate(tick, &row),
                    NamedAnimationResolution::Unbound { .. } => ActivationOutcome::Refused,
                };
                made.push(HealthStart {
                    animation: pool.animation.clone(),
                    node: pool.node.clone(),
                    at: tick,
                    outcome,
                });
            }
        }
        self.starts.extend(made.iter().cloned());
        made
    }
}
