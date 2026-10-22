//! The flak rocket `wep_27` in the weapon runtime (Rally #1884, FLAK-C2).
//!
//! The rule is the one measured in
//! `docs/findings/2026-10-21-flak-a-turret-update-law.md` ("The flak rocket's
//! burst") and `docs/findings/2026-10-22-flak-c1-burst-presentation-and-descriptor-flag.md`
//! (descriptor flag, range end, `IMPACT` row). Provenance `static`, nothing
//! here is `verified_original`.
//!
//! * [`FlakRecord`] is read from the retail `weapons.zrd` record; every number
//!   the round uses (`VELOCITY`, `RANGE`, `DETONATION_DISTANCE`,
//!   `IMPACT_PROXIMITY`, damage) comes from it and a missing one refuses.
//!   No descriptor bit is assumed: `CANNON` (descriptor flag `0x40`) is read
//!   as the presence of the key, and so are `EXPIRES` and
//!   `DETONATE_AT_RANGE`, the two keys the range-end table of FLAK-C1 reads.
//! * [`FlakRound::step`] flies the round and returns an [`OrdnanceBurst`]:
//!   at the end of `RANGE` per [`range_end`], or below `RANGE` as soon as a
//!   `LOCK_ON` round holding a target is within `DETONATION_DISTANCE` of it
//!   (squared-distance test, burst at the round's own position). The target
//!   reaches a `LOCK_ON` round through the lock globals, not through
//!   `CANNON`, so a record without `CANNON` still carries it.
//!
//! Not modelled, because the original's law is not measured: `ACCELERATION`
//! (the round flies at `VELOCITY` from the muzzle), `TURN_RATE` (no homing),
//! the `entity+0x678` factor on `IMPACT_PROXIMITY` (the burst reports the
//! record's radius unscaled), the order of the proximity test against the
//! move within one tick, and what the entity pool does with a silent end.
//! The presentation (`flak_effect`, sounds) belongs to FLAK-C3/C4.

use std::fmt;
use std::path::Path;

use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::Tick;

use super::gun_sound::{READER_ARCHIVE, WEAPONS_MEMBER};
use super::launch_sound::reader_member;

/// The weapon record of the turret's flak rocket.
pub const FLAK_RECORD: &str = "wep_27";

/// The `IMPACT` row a rocket burst starts: index 0, `default` (FLAK-C1).
pub const BURST_IMPACT_ROW: usize = 0;

/// Why [`FlakRecord`] could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlakRecordError {
    /// The archive could not be read or decoded.
    Read(String),
    /// The record or one of its numbers is absent or not a positive number.
    Unmeasured(String),
}

impl fmt::Display for FlakRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "flak record unreadable: {reason}"),
            Self::Unmeasured(reason) => write!(f, "flak record unmeasured: {reason}"),
        }
    }
}

impl std::error::Error for FlakRecordError {}

/// What the round does at `travelled >= RANGE` (FLAK-C1 table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeEnd {
    /// The round bursts (`0x5b02ba`).
    Burst,
    /// The round ends with no impact call (`rocket.silent_end`).
    SilentEnd,
}

/// The range-end law: without `LOCK_ON` only `DETONATE_AT_RANGE` bursts; with
/// `LOCK_ON` the round bursts unless it has `EXPIRES` and no
/// `DETONATE_AT_RANGE`.
#[must_use]
pub const fn range_end(lock_on: bool, expires: bool, detonate_at_range: bool) -> RangeEnd {
    let burst = if lock_on {
        !expires || detonate_at_range
    } else {
        detonate_at_range
    };
    if burst {
        RangeEnd::Burst
    } else {
        RangeEnd::SilentEnd
    }
}

/// The numbers and flags of the flak record.
#[derive(Clone, Debug, PartialEq)]
pub struct FlakRecord {
    /// `VELOCITY`, metres per second.
    pub velocity_mps: f64,
    /// `RANGE`, metres.
    pub range_m: f64,
    /// `DETONATION_DISTANCE`, metres: the proximity trigger.
    pub detonation_distance_m: f64,
    /// `IMPACT_PROXIMITY`, metres: the splash radius at the burst.
    pub impact_proximity_m: f64,
    /// `ARMOR_DAMAGE` at the burst centre.
    pub armor_damage: f64,
    /// `HEALTH_DAMAGE` at the burst centre.
    pub health_damage: f64,
    /// The record has a `LOCK_ON` key (weapon flag `0x8000`).
    pub lock_on: bool,
    /// The record has an `EXPIRES` key (weapon flag `0x80`).
    pub expires: bool,
    /// The record has a `DETONATE_AT_RANGE` key (weapon flag `0x2000000`).
    pub detonate_at_range: bool,
    /// The record has a `CANNON` key (descriptor flag `0x40`).
    pub cannon: bool,
}

fn record_body<'a>(document: &'a ZrdValue, name: &str) -> Option<&'a [ZrdValue]> {
    let children = document.as_list()?;
    for (index, child) in children.iter().enumerate() {
        if child.as_text() == Some(name)
            && let Some(body) = children.get(index + 1).and_then(ZrdValue::as_list)
        {
            return Some(body);
        }
    }
    children.iter().find_map(|child| record_body(child, name))
}

fn first_number(node: &ZrdValue) -> Option<f64> {
    match node {
        ZrdValue::Float(n) => Some(f64::from(*n)),
        ZrdValue::Int(n) => Some(f64::from(*n)),
        ZrdValue::List(children) => children.first().and_then(first_number),
        ZrdValue::Text(_) => None,
    }
}

impl FlakRecord {
    /// Reads the record from a decoded `weapons.zrd`.
    ///
    /// # Errors
    ///
    /// [`FlakRecordError::Unmeasured`] when the record or a number is absent
    /// or not positive.
    pub fn from_document(weapons: &ZrdValue) -> Result<Self, FlakRecordError> {
        let body = record_body(weapons, FLAK_RECORD).ok_or_else(|| {
            FlakRecordError::Unmeasured(format!("weapons.zrd has no record {FLAK_RECORD}"))
        })?;
        let has = |key: &str| body.chunks(2).any(|pair| pair[0].as_text() == Some(key));
        let number = |key: &str| {
            body.chunks(2)
                .find(|pair| pair[0].as_text() == Some(key))
                .and_then(|pair| pair.get(1))
                .and_then(first_number)
                .filter(|n| n.is_finite() && *n > 0.0)
                .ok_or_else(|| {
                    FlakRecordError::Unmeasured(format!(
                        "{FLAK_RECORD} {key} is absent or not a positive number"
                    ))
                })
        };
        Ok(Self {
            velocity_mps: number("VELOCITY")?,
            range_m: number("RANGE")?,
            detonation_distance_m: number("DETONATION_DISTANCE")?,
            impact_proximity_m: number("IMPACT_PROXIMITY")?,
            armor_damage: number("ARMOR_DAMAGE")?,
            health_damage: number("HEALTH_DAMAGE")?,
            lock_on: has("LOCK_ON"),
            expires: has("EXPIRES"),
            detonate_at_range: has("DETONATE_AT_RANGE"),
            cannon: has("CANNON"),
        })
    }

    /// Reads the record from the installation's `weapons.zrd`.
    ///
    /// # Errors
    ///
    /// [`FlakRecordError`] when the archive is unreadable or the record lacks
    /// a number.
    pub fn read(install_root: &Path) -> Result<Self, FlakRecordError> {
        let bytes = reader_member(install_root, READER_ARCHIVE, WEAPONS_MEMBER)
            .map_err(FlakRecordError::Read)?;
        let weapons = decode_zrd(&bytes)
            .map_err(|error| FlakRecordError::Read(format!("{WEAPONS_MEMBER}: {error}")))?;
        Self::from_document(&weapons)
    }
}

/// Why a flak round burst.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurstCause {
    /// Within `DETONATION_DISTANCE` of the held target.
    Proximity,
    /// `travelled >= RANGE` with [`RangeEnd::Burst`].
    RangeEnd,
}

/// One burst: where, why, and the splash the record declares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrdnanceBurst {
    /// The tick the burst happened on.
    pub at: Tick,
    /// The burst point, metres, the round's own position.
    pub position_m: [f64; 3],
    /// Why it burst.
    pub cause: BurstCause,
    /// `IMPACT_PROXIMITY`, metres, unscaled (the `entity+0x678` factor is
    /// unmeasured).
    pub splash_radius_m: f64,
    /// `ARMOR_DAMAGE` at the centre.
    pub armor_damage: f64,
    /// `HEALTH_DAMAGE` at the centre.
    pub health_damage: f64,
    /// The `IMPACT` row to start: [`BURST_IMPACT_ROW`].
    pub impact_row: usize,
}

/// What one tick did to a round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlakStep {
    /// Still flying.
    Flying,
    /// Burst this tick.
    Burst(OrdnanceBurst),
    /// Ended at `RANGE` without an impact call.
    SilentEnd,
}

/// Why a round could not be fired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlakFireRefusal {
    /// The muzzle position or direction is not finite, or the direction is
    /// the zero vector.
    BadAim,
}

/// A flak round in flight.
#[derive(Clone, Debug, PartialEq)]
pub struct FlakRound {
    record: FlakRecord,
    position_m: [f64; 3],
    direction: [f64; 3],
    travelled_m: f64,
    finished: bool,
}

impl FlakRound {
    /// Fires a round from `muzzle_m` along `direction` (any length).
    ///
    /// # Errors
    ///
    /// [`FlakFireRefusal::BadAim`] for a non-finite or zero aim.
    pub fn fire(
        record: &FlakRecord,
        muzzle_m: [f64; 3],
        direction: [f64; 3],
    ) -> Result<Self, FlakFireRefusal> {
        let length = direction.iter().map(|c| c * c).sum::<f64>().sqrt();
        if !length.is_finite() || length <= 0.0 || muzzle_m.iter().any(|c| !c.is_finite()) {
            return Err(FlakFireRefusal::BadAim);
        }
        Ok(Self {
            record: record.clone(),
            position_m: muzzle_m,
            direction: direction.map(|c| c / length),
            travelled_m: 0.0,
            finished: false,
        })
    }

    /// The round's position, metres.
    #[must_use]
    pub const fn position_m(&self) -> [f64; 3] {
        self.position_m
    }

    /// Whether the round has burst or ended.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.finished
    }

    /// Flies the round `dt_s` seconds at `tick`, with the target the lock
    /// globals hold (`None` when no target is held).
    pub fn step(&mut self, tick: Tick, dt_s: f64, held_target_m: Option<[f64; 3]>) -> FlakStep {
        if self.finished {
            return FlakStep::SilentEnd;
        }
        let distance = self.record.velocity_mps * dt_s;
        for (axis, component) in self.position_m.iter_mut().zip(self.direction) {
            *axis += component * distance;
        }
        self.travelled_m += distance;
        let burst = |cause| {
            FlakStep::Burst(OrdnanceBurst {
                at: tick,
                position_m: self.position_m,
                cause,
                splash_radius_m: self.record.impact_proximity_m,
                armor_damage: self.record.armor_damage,
                health_damage: self.record.health_damage,
                impact_row: BURST_IMPACT_ROW,
            })
        };
        let outcome = if self.travelled_m >= self.record.range_m {
            match range_end(
                self.record.lock_on,
                self.record.expires,
                self.record.detonate_at_range,
            ) {
                RangeEnd::Burst => burst(BurstCause::RangeEnd),
                RangeEnd::SilentEnd => FlakStep::SilentEnd,
            }
        } else if self.record.lock_on
            && let Some(target) = held_target_m
            && squared_distance(self.position_m, target)
                <= self.record.detonation_distance_m * self.record.detonation_distance_m
        {
            burst(BurstCause::Proximity)
        } else {
            FlakStep::Flying
        };
        self.finished = outcome != FlakStep::Flying;
        outcome
    }
}

fn squared_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> ZrdValue {
        ZrdValue::Text(s.to_owned())
    }

    fn number(key: &str, value: f32) -> [ZrdValue; 2] {
        [text(key), ZrdValue::List(vec![ZrdValue::Float(value)])]
    }

    fn flag(key: &str) -> [ZrdValue; 2] {
        [text(key), ZrdValue::List(Vec::new())]
    }

    /// A document with the numbers of `wep_27` and the given extra flags.
    fn document(extra: &[&str]) -> ZrdValue {
        let mut body = Vec::new();
        for (key, value) in [
            ("VELOCITY", 850.0),
            ("RANGE", 900.0),
            ("DETONATION_DISTANCE", 20.0),
            ("IMPACT_PROXIMITY", 22.0),
            ("ARMOR_DAMAGE", 10.0),
            ("HEALTH_DAMAGE", 10.0),
        ] {
            body.extend(number(key, value));
        }
        body.extend(flag("LOCK_ON"));
        body.extend(flag("ROCKET"));
        for key in extra {
            body.extend(flag(key));
        }
        ZrdValue::List(vec![ZrdValue::List(vec![
            text(FLAK_RECORD),
            ZrdValue::List(body),
        ])])
    }

    fn record(extra: &[&str]) -> FlakRecord {
        FlakRecord::from_document(&document(extra)).expect("record")
    }

    const DT: f64 = 1.0 / 60.0;

    #[test]
    fn accept_flak_c2_record_numbers_and_flags_come_from_the_document() {
        let record = record(&[]);
        assert_eq!(record.velocity_mps, 850.0);
        assert_eq!(record.range_m, 900.0);
        assert_eq!(record.detonation_distance_m, 20.0);
        assert_eq!(record.impact_proximity_m, 22.0);
        assert!(record.lock_on && !record.expires && !record.cannon);
        assert!(super::FlakRecord::from_document(&ZrdValue::List(Vec::new())).is_err());
        let cannon = FlakRecord::from_document(&document(&["CANNON"])).expect("record");
        assert!(cannon.cannon);
    }

    #[test]
    fn accept_flak_c2_bursts_at_the_detonation_distance_from_a_held_target() {
        let record = record(&[]);
        let target = [500.0, 0.0, 0.0];
        let mut round = FlakRound::fire(&record, [0.0; 3], [1.0, 0.0, 0.0]).expect("fires");
        let mut burst = None;
        for n in 1..=60 {
            if let FlakStep::Burst(b) = round.step(Tick(n), DT, Some(target)) {
                burst = Some(b);
                break;
            }
        }
        let burst = burst.expect("bursts before range");
        assert_eq!(burst.cause, BurstCause::Proximity);
        let gap = squared_distance(burst.position_m, target).sqrt();
        assert!(gap <= 20.0 && gap > 20.0 - 850.0 * DT, "gap {gap}");
        assert_eq!(burst.splash_radius_m, 22.0);
        assert_eq!(burst.impact_row, 0);
        assert!(round.is_finished());
        assert_eq!(round.step(Tick(99), DT, Some(target)), FlakStep::SilentEnd);
    }

    #[test]
    fn accept_flak_c2_without_a_target_it_bursts_at_range() {
        let record = record(&[]);
        let mut round = FlakRound::fire(&record, [0.0; 3], [0.0, 3.0, 0.0]).expect("fires");
        let mut tick = 0;
        let burst = loop {
            tick += 1;
            match round.step(Tick(tick), DT, None) {
                FlakStep::Flying => {}
                FlakStep::Burst(b) => break b,
                FlakStep::SilentEnd => panic!("LOCK_ON without EXPIRES bursts"),
            }
        };
        assert_eq!(burst.cause, BurstCause::RangeEnd);
        assert!(burst.position_m[1] >= 900.0 && burst.position_m[1] < 900.0 + 850.0 * DT);
        assert_eq!(burst.position_m[0], 0.0);
    }

    #[test]
    fn accept_flak_c2_range_end_table() {
        use RangeEnd::{Burst, SilentEnd};
        assert_eq!(range_end(false, false, true), Burst);
        assert_eq!(range_end(false, true, false), SilentEnd);
        assert_eq!(range_end(false, false, false), SilentEnd);
        assert_eq!(range_end(true, false, false), Burst);
        assert_eq!(range_end(true, true, true), Burst);
        assert_eq!(range_end(true, true, false), SilentEnd);
        let record = record(&["EXPIRES"]);
        let mut round = FlakRound::fire(&record, [0.0; 3], [1.0, 0.0, 0.0]).expect("fires");
        let ended = (1..)
            .map(|n| round.step(Tick(n), DT, None))
            .find(|step| *step != FlakStep::Flying);
        assert_eq!(ended, Some(FlakStep::SilentEnd));
    }

    #[test]
    fn accept_flak_c2_a_bad_aim_is_refused() {
        let record = record(&[]);
        assert_eq!(
            FlakRound::fire(&record, [0.0; 3], [0.0; 3]),
            Err(FlakFireRefusal::BadAim)
        );
        assert!(FlakRound::fire(&record, [f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0]).is_err());
    }

    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_flak_c2_retail_wep_27_is_the_measured_rocket() {
        let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set");
        let record = FlakRecord::read(Path::new(&root)).expect("retail wep_27");
        assert_eq!(record.velocity_mps, 850.0);
        assert_eq!(record.range_m, 900.0);
        assert_eq!(record.detonation_distance_m, 20.0);
        assert_eq!(record.impact_proximity_m, 22.0);
        assert_eq!(record.armor_damage, 10.0);
        assert!(record.lock_on);
        assert!(!record.cannon && !record.expires && !record.detonate_at_range);
        let mut round = FlakRound::fire(&record, [0.0; 3], [0.0, 0.0, 1.0]).expect("fires");
        let target = [0.0, 0.0, 300.0];
        let burst = (1..)
            .find_map(|n| match round.step(Tick(n), DT, Some(target)) {
                FlakStep::Burst(b) => Some(b),
                _ => None,
            })
            .expect("bursts");
        assert_eq!(burst.cause, BurstCause::Proximity);
        assert!(squared_distance(burst.position_m, target).sqrt() <= 20.0);
    }
}
