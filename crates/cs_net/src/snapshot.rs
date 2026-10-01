//! The quantized server-snapshot schema and its declared budgets (F57-A).
//!
//! Spec: `specs/F57-networked-aircraft-prediction-interpolation-and-projectiles.md`,
//! stage `### F57-A`. Shared contracts: `docs/contracts/FLIGHT-PHYSICS.md`
//! (canonical space, SI units, radians, reject non-finite input where it enters)
//! and `docs/contracts/UI-NETWORK.md` ("Interpolation buffers separate actor
//! generations", "Motion snapshots are sequenced and may be dropped").
//!
//! # What this stage owns
//!
//! F54-A left [`crate::message::SnapshotFrame::payload`] an opaque bounded byte
//! string pending this stage. This module is the schema *inside* those bytes:
//! the per-actor record, the integer quantization of every numeric field with
//! its scale, width and half-step error budget, and the codec that both
//! [`Snapshot::encode`] and [`Snapshot::decode`] run. It is a pure typed
//! input/output boundary with a minimal synthetic fixture (the `synthetic_*`
//! functions at the end of this file). It contains no transport, no transport
//! codec, no interpolation buffer and no prediction — those are F54-B, F57-B
//! and F57-C.
//!
//! # Quantization is declared, not incidental
//!
//! Every numeric field goes through a [`Quantization`], which fixes the number
//! of integer steps per unit, the width and signedness of the stored integer
//! and the error budget that follows (`0.5` steps). Nothing is quantized by a
//! bare cast: [`Quantization::encode`] is the only way a real value becomes an
//! integer, it rejects non-finite input and it *refuses* a value outside the
//! declared range instead of saturating it into a plausible wrong number. The
//! whole declared table is reachable from code ([`SNAPSHOT_BUDGET`]), so a
//! consumer compares a measured error against a declared budget rather than
//! against a constant it happens to know.
//!
//! # Origin-relative coordinates need a shared epoch
//!
//! Positions are quantized *relative to the session's current world-origin
//! epoch*, because an absolute world coordinate loses precision far from the
//! origin. The epoch id travels in every snapshot ([`Snapshot::origin`]): both
//! ends must agree which frame the integers are measured from, and a receiver
//! with no anchor for an epoch has to refuse the records rather than place them
//! at a guessed zero. A rebase therefore *changes the frame*, not the
//! aircraft's world position, and cannot look like a velocity impulse — the
//! epoch id is what tells a receiver to convert.
//!
//! # What travels and what does not
//!
//! A record carries pose, velocities, the essential flight (throttle, engine
//! spool, remaining boost capacity), damage (remaining integrity, disabled
//! mounts) and weapon (rounds per bank, selected bank) channels, plus the
//! actor's generation and lifecycle. It carries no client authority: a snapshot
//! is a [`crate::message::ServerPayload`] value (F54-A), a record's actor must
//! belong to the session the packet was accepted for, and destruction
//! *semantics* stay with the damage domain and the reliable
//! [`crate::message::EventBody::ActorRemoved`] event — a snapshot may only say
//! an actor is already gone, never who destroyed it.
//!
//! All scales, caps and budgets here are newly authored engine design: no
//! original network budget, packet layout or replay behavior is known or
//! claimed.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::Tick;
use cs_types::net::{ActorId, SessionId};
use cs_types::space::Quaternion;

use crate::bounds::MAX_SNAPSHOT_BYTES;
use crate::message::SnapshotFrame;

// ---------------------------------------------------------------- caps ----

/// Most actor records one snapshot may carry.
///
/// The cap bounds a snapshot by construction rather than by the envelope alone:
/// [`MAX_ACTORS_PER_SNAPSHOT`] records of the fixed [`ACTOR_RECORD_BYTES`] must
/// fit inside the F54-A envelope cap [`MAX_SNAPSHOT_BYTES`] (asserted at compile
/// time below). A population larger than this is *refused* by
/// [`Snapshot::validate`], never silently truncated: splitting one tick's
/// population across several sequenced snapshots is F57-C's job.
pub const MAX_ACTORS_PER_SNAPSHOT: usize = 64;

/// Bytes of one actor record on the wire, fixed.
pub const ACTOR_RECORD_BYTES: usize = 66;

/// Bytes before the first actor record: schema version (1), origin epoch (4),
/// input acknowledgment (4) and the actor count (2).
pub const SNAPSHOT_HEADER_BYTES: usize = 11;

/// Bytes of the smallest-three rotation encoding: the dropped component's
/// two-bit index and its sign in one byte, then three signed 16-bit
/// components.
pub const ROTATION_BYTES: usize = 7;

/// The schema version this stage writes and the only version it reads.
pub const SNAPSHOT_SCHEMA_VERSION: u8 = 1;

/// The declared wire budgets are internally consistent: a full snapshot at the
/// actor cap still fits the envelope cap the F54-A vocabulary validates
/// against.
const _: () = assert!(
    SNAPSHOT_HEADER_BYTES + MAX_ACTORS_PER_SNAPSHOT * ACTOR_RECORD_BYTES <= MAX_SNAPSHOT_BYTES
);
const _: () = assert!(ACTOR_RECORD_BYTES <= MAX_SNAPSHOT_BYTES);
const _: () = assert!(ROTATION_BYTES < ACTOR_RECORD_BYTES);

// -------------------------------------------------------------- errors ----

/// Why a snapshot or one of its quantized fields was refused.
///
/// Every refusal names the field: an unexplained snapshot failure is
/// indistinguishable from a missing one.
#[derive(Clone, Debug, PartialEq)]
pub enum SnapshotError {
    /// The payload declares another schema version. This stage reads exactly
    /// one version; it does not guess at an unknown layout.
    SchemaVersion {
        /// The version the payload declares.
        found: u8,
        /// The version this stage reads.
        expected: u8,
    },
    /// The payload ended before a required field.
    Truncated {
        /// Bytes the field needs.
        need: usize,
        /// Bytes left in the payload.
        have: usize,
    },
    /// The payload carried bytes beyond the actor count it declares.
    TrailingBytes {
        /// The unaccounted byte count.
        len: usize,
    },
    /// A snapshot carried no actor record. An empty snapshot says nothing and
    /// is what a truncated payload looks like, so it is refused rather than
    /// read as "the world is empty" — which would retire every remote actor.
    Empty,
    /// A snapshot carried more records than [`MAX_ACTORS_PER_SNAPSHOT`].
    TooManyActors {
        /// The cap.
        max: usize,
        /// The offered count.
        len: usize,
    },
    /// A record's session epoch field was zero, which
    /// [`cs_types::net::SessionId`] never allows.
    InvalidSession {
        /// The zero field the payload carried.
        found: u64,
    },
    /// A record's actor belongs to another session generation.
    ForeignSession {
        /// The session the snapshot was accepted for.
        expected: SessionId,
        /// The actor that named another session.
        actor: ActorId,
    },
    /// A record's actor serial is zero. [`cs_types::net::ActorAllocator`]
    /// issues serials from 1, so serial 0 cannot name a live actor.
    ReservedSerial {
        /// The offending actor id.
        actor: ActorId,
    },
    /// The same actor appeared twice in one snapshot; there is no ordering
    /// rule that could pick between the two copies.
    DuplicateActor {
        /// The repeated actor.
        actor: ActorId,
    },
    /// A quantized field is not one this schema version defines.
    UnknownCode {
        /// The field whose code was not recognized.
        field: &'static str,
        /// The code the payload carried.
        value: u8,
    },
    /// A real value was non-finite or outside its declared quantization range,
    /// or a stored integer did not fit its declared width. Refused, never
    /// clamped.
    OutOfRange {
        /// The offending field.
        field: &'static str,
        /// The value offered or decoded.
        value: f64,
        /// The declared bound it broke, in the field's unit.
        max: f64,
    },
    /// The origin-epoch counter would wrap; an epoch must never name two
    /// different frames.
    EpochExhausted,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaVersion { found, expected } => write!(
                f,
                "snapshot schema version {found} is not the supported version {expected}"
            ),
            Self::Truncated { need, have } => {
                write!(f, "snapshot payload needs {need} more bytes, {have} left")
            }
            Self::TrailingBytes { len } => {
                write!(f, "snapshot payload has {len} bytes beyond its actor count")
            }
            Self::Empty => write!(f, "a snapshot must carry at least one actor record"),
            Self::TooManyActors { max, len } => {
                write!(f, "snapshot carries {len} actor records, max is {max}")
            }
            Self::InvalidSession { found } => {
                write!(
                    f,
                    "snapshot record carries session epoch {found}, which is never valid"
                )
            }
            Self::ForeignSession { expected, actor } => {
                write!(
                    f,
                    "snapshot record for {actor} does not belong to {expected}"
                )
            }
            Self::ReservedSerial { actor } => {
                write!(f, "actor serial 0 cannot name a live actor: {actor}")
            }
            Self::DuplicateActor { actor } => {
                write!(f, "snapshot carries {actor} more than once")
            }
            Self::UnknownCode { field, value } => {
                write!(f, "snapshot field {field} carries unknown code {value}")
            }
            Self::OutOfRange { field, value, max } => write!(
                f,
                "snapshot field {field} value {value} is outside the declared bound {max}"
            ),
            Self::EpochExhausted => write!(f, "snapshot origin epoch counter is exhausted"),
        }
    }
}

impl std::error::Error for SnapshotError {}

// ------------------------------------------------------ quantization ----

/// One declared quantization: how many integer steps make one unit, how wide
/// and signed the stored integer is, and therefore the error budget that
/// follows.
///
/// The error budget is a *half step* — `0.5 / steps_per_unit` in the field's
/// own unit — which is the largest amount a correctly rounded value can differ
/// from the real one. It is declared here rather than re-derived per consumer
/// so every consumer can name the budget it must stay inside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quantization {
    field: &'static str,
    unit: &'static str,
    steps_per_unit: f64,
    bits: u32,
    signed: bool,
}

impl Quantization {
    /// A signed quantization: `steps_per_unit` integer steps per unit, stored in
    /// `bits` bits of two's complement.
    pub const fn signed(
        field: &'static str,
        unit: &'static str,
        steps_per_unit: f64,
        bits: u32,
    ) -> Self {
        Self {
            field,
            unit,
            steps_per_unit,
            bits,
            signed: true,
        }
    }

    /// An unsigned quantization: `steps_per_unit` integer steps per unit in
    /// `bits` bits, with zero the smallest representable value.
    pub const fn unsigned(
        field: &'static str,
        unit: &'static str,
        steps_per_unit: f64,
        bits: u32,
    ) -> Self {
        Self {
            field,
            unit,
            steps_per_unit,
            bits,
            signed: false,
        }
    }

    /// The field name this budget applies to, as it appears in diagnostics.
    #[must_use]
    pub const fn field(self) -> &'static str {
        self.field
    }

    /// The unit the steps, the range and the error budget are counted in.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        self.unit
    }

    /// Integer steps per unit (SI `m`, `m/s`, `rad/s`, or a unit fraction).
    #[must_use]
    pub const fn steps_per_unit(self) -> f64 {
        self.steps_per_unit
    }

    /// Width of the stored integer, in bits. Only 16 and 32 bits are
    /// representable in a [`QuantizedVector`].
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.bits
    }

    /// Whether the stored integer is two's complement.
    #[must_use]
    pub const fn is_signed(self) -> bool {
        self.signed
    }

    /// The quantization step, in the field's unit.
    #[must_use]
    pub fn step(self) -> f64 {
        1.0 / self.steps_per_unit
    }

    /// The declared error budget: half a quantization step, in the field's
    /// unit.
    #[must_use]
    pub fn max_error(self) -> f64 {
        0.5 * self.step()
    }

    /// The largest value the declared width represents, in the field's unit.
    #[must_use]
    pub fn max_value(self) -> f64 {
        self.max_steps() / self.steps_per_unit
    }

    /// The smallest value the declared width represents, in the field's unit.
    #[must_use]
    pub fn min_value(self) -> f64 {
        self.min_steps() / self.steps_per_unit
    }

    /// Bytes one stored component occupies, from the declared width.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] for a width that is not 16 or 32 bits: a
    /// quantization whose width this schema cannot store is a declared budget
    /// the schema cannot honour, and has to be reported rather than truncated
    /// to a neighbouring width.
    pub fn storage_bytes(self) -> Result<usize, SnapshotError> {
        match self.bits {
            16 => Ok(2),
            32 => Ok(4),
            _ => Err(SnapshotError::OutOfRange {
                field: self.field,
                value: f64::from(self.bits),
                max: 32.0,
            }),
        }
    }

    /// Largest representable step count, inclusive.
    fn max_steps(self) -> f64 {
        if self.is_signed() {
            2_f64.powi(self.bits as i32 - 1) - 1.0
        } else {
            2_f64.powi(self.bits as i32) - 1.0
        }
    }

    /// Smallest representable step count, inclusive.
    fn min_steps(self) -> f64 {
        if self.is_signed() {
            -self.max_steps()
        } else {
            0.0
        }
    }

    /// Checks that the declaration itself is usable: a finite positive scale and
    /// a width this schema can store.
    ///
    /// The declared budgets are constants, so this runs on every
    /// [`Snapshot::validate`] rather than being a construction-time guarantee a
    /// later edit could quietly drop.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] naming the scale or the width that is not
    /// usable.
    pub fn validate(self) -> Result<(), SnapshotError> {
        if !self.steps_per_unit.is_finite() || self.steps_per_unit <= 0.0 {
            return Err(SnapshotError::OutOfRange {
                field: self.field,
                value: self.steps_per_unit,
                max: 0.0,
            });
        }
        self.storage_bytes()?;
        Ok(())
    }

    /// Quantizes one real value.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] for a non-finite value or one outside
    /// `[min_value, max_value]`. The value is refused, never saturated: a
    /// position or velocity beyond the declared range is a fact about the
    /// session that has to be reported, not a number to round into range.
    pub fn encode(self, value: f64) -> Result<i32, SnapshotError> {
        if !value.is_finite() {
            return Err(SnapshotError::OutOfRange {
                field: self.field,
                value,
                max: self.max_value(),
            });
        }
        let steps = (value * self.steps_per_unit).round();
        if steps < self.min_steps() || steps > self.max_steps() {
            return Err(SnapshotError::OutOfRange {
                field: self.field,
                value,
                max: self.max_value(),
            });
        }
        Ok(steps.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)
    }

    /// Dequantizes one stored integer.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] when `raw` does not fit the declared width
    /// and signedness, so a corrupt or foreign payload cannot decode into a
    /// plausible number.
    pub fn decode(self, raw: i32) -> Result<f64, SnapshotError> {
        if !self.fits(raw) {
            return Err(self.out_of(raw));
        }
        Ok(f64::from(raw) / self.steps_per_unit)
    }

    /// Whether a stored integer fits this declaration's width and signedness.
    #[must_use]
    pub fn fits(self, raw: i32) -> bool {
        let steps = f64::from(raw);
        steps >= self.min_steps() && steps <= self.max_steps()
    }

    /// The refusal this declaration reports for a stored integer it cannot hold.
    fn out_of(self, raw: i32) -> SnapshotError {
        SnapshotError::OutOfRange {
            field: self.field,
            value: f64::from(raw),
            max: self.max_value(),
        }
    }
}

/// Position: 1/64 m steps (15.6 mm) over the full signed 32-bit range — ±33 554
/// km from the origin epoch, far past any authored world — with a 7.8 mm error
/// budget.
pub const POSITION_QUANTIZATION: Quantization = Quantization::signed("position", "m", 64.0, 32);

/// World linear velocity: 0.05 m/s steps in 16 signed bits (±1638 m/s), error
/// budget 25 mm/s.
pub const LINEAR_VELOCITY_QUANTIZATION: Quantization =
    Quantization::signed("linear_velocity", "m/s", 20.0, 16);

/// Body angular velocity: 1/512 rad/s steps in 16 signed bits (±63.997 rad/s),
/// error budget 0.98 mrad/s.
pub const ANGULAR_VELOCITY_QUANTIZATION: Quantization =
    Quantization::signed("angular_velocity", "rad/s", 512.0, 16);

/// A unit-fraction channel (throttle, engine spool, remaining boost capacity):
/// 1/65535 steps in 16 unsigned bits, error budget 7.6e-6.
pub const UNIT_FRACTION_QUANTIZATION: Quantization =
    Quantization::unsigned("unit_fraction", "fraction", 65535.0, 16);

/// Remaining structural integrity in parts per thousand of the airframe's full
/// integrity: exact integer steps in 16 unsigned bits, error budget half a
/// permille.
pub const INTEGRITY_PERMILLE_QUANTIZATION: Quantization =
    Quantization::unsigned("integrity_permille", "per_mille", 1.0, 16);

/// Rounds remaining in one ammunition bank: exact integer counts in 16 unsigned
/// bits, error budget half a round — and exact for every in-range integer,
/// because the scale is one step per round.
pub const ROUNDS_QUANTIZATION: Quantization = Quantization::unsigned("rounds", "round", 1.0, 16);

/// Every quantization budget the snapshot schema declares, in field order, so a
/// consumer can audit the whole table instead of trusting the constants it
/// happens to use.
pub const SNAPSHOT_BUDGET: [Quantization; 6] = [
    POSITION_QUANTIZATION,
    LINEAR_VELOCITY_QUANTIZATION,
    ANGULAR_VELOCITY_QUANTIZATION,
    UNIT_FRACTION_QUANTIZATION,
    INTEGRITY_PERMILLE_QUANTIZATION,
    ROUNDS_QUANTIZATION,
];

/// Integer steps per unit in the smallest-three rotation encoding: the full
/// signed 16-bit range.
pub const ROTATION_COMPONENT_SCALE: f64 = 32767.0;

/// Declared per-component error of the rotation encoding: half a step.
pub const ROTATION_COMPONENT_ERROR: f64 = 0.5 / ROTATION_COMPONENT_SCALE;

/// Declared orientation error budget of the rotation encoding, in radians.
///
/// A first-order bound, derived rather than picked: each stored component is off
/// by at most [`ROTATION_COMPONENT_ERROR`] `e`, so the stored sum of squares is
/// off by at most `2e(|a|+|b|+|c|) <= 2e*sqrt(3)`, and the reconstructed dropped
/// component — whose magnitude is at least `1/sqrt(3)` — moves by at most `3e`.
/// The perturbation of the unit quaternion is therefore at most
/// `sqrt(3*e^2 + (3e)^2) = 3.47e`. A unit quaternion perturbed by `p` represents a
/// rotation that differs from the original by `2*|p|`, not `|p|`: for two unit
/// quaternions `|q - q'| = 2*sin(theta/4)`, so the rotation angle between them is
/// `theta ~= 2*|q - q'|`. The bound is therefore `2*3.47e = 6.93e`, and `8e` is
/// declared: enough headroom over that bound to cover the f32 world→local
/// narrowing an input rotation carries (`cs_types::space::
/// QUATERNION_LENGTH_TOLERANCE`, which `decode` normalizes away), while still far
/// too tight to hide a wrong dropped index or a lost sign, which are
/// order-one rotations.
///
/// Measured worst cases agree: a dense random sweep of unit rotations peaks at
/// about `2.8e`, and the structural worst case — a rotation whose four
/// components are near `1/2` in magnitude, so the dropped component is smallest
/// and its reconstruction is most amplified — peaks at about `6.93e`. The
/// acceptance test measures both and asserts they stay under this budget.
pub const ROTATION_ORIENTATION_ERROR_RAD: f64 = 8.0 * ROTATION_COMPONENT_ERROR;

/// A rotation as the three smallest components of its unit quaternion, plus the
/// index and sign of the component that was dropped.
///
/// The dropped component is the largest in magnitude, so it is at least
/// `1/sqrt(3)` and can be recovered from the unit-norm constraint — the
/// "smallest three" encoding compact quaternion formats use. The three stored
/// components are signed, so no component's sign is ever inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantizedRotation {
    index: u8,
    sign: u8,
    components: [i16; 3],
}

impl QuantizedRotation {
    /// Quantizes a unit rotation.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] when a component is non-finite, which a
    /// validated [`cs_types::space::Quaternion`] cannot be.
    pub fn encode(rotation: Quaternion) -> Result<Self, SnapshotError> {
        let all = rotation.components();
        let mut index = 0_usize;
        for candidate in 1..all.len() {
            if all[candidate].abs() > all[index].abs() {
                index = candidate;
            }
        }
        let sign = u8::from(all[index] < 0.0);
        let mut components = [0_i16; 3];
        let mut kept = 0_usize;
        for (slot, value) in all.iter().enumerate() {
            if slot == index {
                continue;
            }
            components[kept] = quantize_rotation_component(*value);
            kept += 1;
        }
        Ok(Self {
            index: index as u8,
            sign,
            components,
        })
    }

    /// Dequantizes back into a unit rotation.
    ///
    /// The rebuilt vector is normalized first, so the half-step error of the
    /// three stored components does not accumulate into a denormalized
    /// quaternion.
    ///
    /// # Errors
    ///
    /// [`cs_types::space::SpaceError`] when the reconstruction is not unit
    /// length, which can only happen for a rotation that was not built through
    /// [`cs_types::space::Quaternion::try_new`].
    pub fn decode(self) -> Result<Quaternion, cs_types::space::SpaceError> {
        let scale = ROTATION_COMPONENT_SCALE;
        let mut all = [0.0_f64; 4];
        let mut kept = 0_usize;
        for (slot, value) in all.iter_mut().enumerate() {
            if slot == usize::from(self.index) {
                continue;
            }
            *value = f64::from(self.components[kept]) / scale;
            kept += 1;
        }
        let squared: f64 = all.iter().map(|value| value * value).sum();
        let dropped = (1.0 - squared).max(0.0).sqrt();
        all[usize::from(self.index)] = if self.sign == 0 { dropped } else { -dropped };
        let norm = all.iter().map(|value| value * value).sum::<f64>().sqrt();
        let rebuilt = if norm > 0.0 {
            [all[0] / norm, all[1] / norm, all[2] / norm, all[3] / norm]
        } else {
            // Only a payload whose stored components are all zero reaches this,
            // and that is not a rotation at all: the identity is the honest
            // reading of "no stored component", not a division by zero.
            return Ok(Quaternion::IDENTITY);
        };
        Quaternion::try_new(rebuilt)
    }

    /// The index of the dropped component, `(x, y, z, w)` ordered.
    #[must_use]
    pub const fn dropped_index(self) -> u8 {
        self.index
    }

    /// The sign of the dropped component: `1` when it was negative.
    #[must_use]
    pub const fn dropped_sign(self) -> u8 {
        self.sign
    }

    /// The three stored components, in the order the dropped index leaves.
    #[must_use]
    pub const fn stored_components(self) -> [i16; 3] {
        self.components
    }

    /// Writes the fixed [`ROTATION_BYTES`] encoding, little-endian.
    fn write_to(self, out: &mut Vec<u8>) {
        out.push(self.index | (self.sign << 2));
        for component in self.components {
            out.extend_from_slice(&component.to_le_bytes());
        }
    }

    /// Reads the fixed [`ROTATION_BYTES`] encoding.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::Truncated`] when the payload ends inside the encoding.
    fn read_from(cursor: &mut Cursor<'_>) -> Result<Self, SnapshotError> {
        let head = cursor.u8()?;
        let mut components = [0_i16; 3];
        for slot in &mut components {
            *slot = cursor.i16()?;
        }
        Ok(Self {
            index: head & 0b11,
            sign: (head >> 2) & 0b1,
            components,
        })
    }
}

/// One stored rotation component: 16 signed bits at the declared scale. A unit
/// quaternion's components are inside `[-1, 1]`, so the only reachable clamp is
/// the exact endpoint.
fn quantize_rotation_component(value: f64) -> i16 {
    let steps = (value * ROTATION_COMPONENT_SCALE).round();
    if !steps.is_finite() {
        return 0;
    }
    steps.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

/// A quantized vector: three integer step counts in one declared quantization.
///
/// The step counts are held as `i32` and stored at the *declared* width of
/// their quantization, so a field declared as 16 bits occupies two bytes on
/// the wire and cannot smuggle a wider value past its budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantizedVector {
    steps: [i32; 3],
}

impl QuantizedVector {
    /// Quantizes `[x, y, z]` under `budget`.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] naming the first component `budget`
    /// refuses.
    pub fn quantize(budget: Quantization, value: [f64; 3]) -> Result<Self, SnapshotError> {
        Ok(Self {
            steps: [
                budget.encode(value[0])?,
                budget.encode(value[1])?,
                budget.encode(value[2])?,
            ],
        })
    }

    /// The raw step counts, in canonical `+X, +Y, +Z` order.
    #[must_use]
    pub const fn steps(self) -> [i32; 3] {
        self.steps
    }

    /// Dequantizes into real components under `budget`.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] when a stored step count does not fit the
    /// declared width.
    pub fn dequantize(self, budget: Quantization) -> Result<[f64; 3], SnapshotError> {
        Ok([
            budget.decode(self.steps[0])?,
            budget.decode(self.steps[1])?,
            budget.decode(self.steps[2])?,
        ])
    }

    /// Checks that every stored step count fits `budget`'s declared width.
    ///
    /// A record's fields are public and `QuantizedVector` is `Copy`, so a vector
    /// built under one budget can be assigned to a field declared with another.
    /// That has to be a named refusal at the schema boundary rather than a silent
    /// narrowing by [`Self::write_to`], or a record could travel with numbers
    /// that do not mean what its field declares.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] naming `budget`'s field for the first
    /// component that does not fit.
    pub fn check_width(self, budget: Quantization) -> Result<(), SnapshotError> {
        for step in self.steps {
            if !budget.fits(step) {
                return Err(budget.out_of(step));
            }
        }
        Ok(())
    }

    /// Bytes this vector occupies on the wire under `budget`.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] for a width this schema cannot store.
    pub fn wire_bytes(budget: Quantization) -> Result<usize, SnapshotError> {
        Ok(3 * budget.storage_bytes()?)
    }

    /// Writes the three step counts at `budget`'s declared width.
    fn write_to(self, out: &mut Vec<u8>, budget: Quantization) {
        for step in self.steps {
            if budget.bits() == 16 {
                let narrow = i16::try_from(step).unwrap_or(i16::MAX);
                out.extend_from_slice(&narrow.to_le_bytes());
            } else {
                out.extend_from_slice(&step.to_le_bytes());
            }
        }
    }

    /// Reads the three step counts at `budget`'s declared width.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::Truncated`] when the payload ends inside the vector.
    fn read_from(cursor: &mut Cursor<'_>, budget: Quantization) -> Result<Self, SnapshotError> {
        let mut steps = [0_i32; 3];
        for slot in &mut steps {
            *slot = if budget.bits() == 16 {
                i32::from(cursor.i16()?)
            } else {
                cursor.i32()?
            };
        }
        Ok(Self { steps })
    }
}

// --------------------------------------------------------- wire codes ----

/// What the server last did with an actor, as far as one record can report it.
///
/// Deliberately coarser than `cs_sim::damage::LifecycleKind`: a snapshot may
/// only say that an actor is gone, never why, who destroyed it or which
/// subsystem failed. Attribution and lifecycle semantics stay server-side and
/// travel as reliable events (F54-A), because a dropped sequenced snapshot must
/// never be the only witness of a destruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lifecycle {
    /// The actor exists and is being simulated.
    Alive,
    /// The actor was destroyed: a damage-domain event the server recorded.
    Destroyed,
    /// The actor's entities left the world.
    Despawned,
}

impl Lifecycle {
    /// Every kind, in a stable order.
    pub const ALL: &'static [Lifecycle] = &[Self::Alive, Self::Destroyed, Self::Despawned];

    /// The wire code this schema version uses.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Alive => 0,
            Self::Destroyed => 1,
            Self::Despawned => 2,
        }
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::Destroyed => "destroyed",
            Self::Despawned => "despawned",
        }
    }

    /// Whether the kind is terminal for the actor's record.
    #[must_use]
    pub const fn is_gone(self) -> bool {
        matches!(self, Self::Destroyed | Self::Despawned)
    }

    /// Reads a wire code.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::UnknownCode`] for a code this schema version does not
    /// define.
    pub fn from_code(value: u8) -> Result<Self, SnapshotError> {
        match value {
            0 => Ok(Self::Alive),
            1 => Ok(Self::Destroyed),
            2 => Ok(Self::Despawned),
            _ => Err(SnapshotError::UnknownCode {
                field: "lifecycle",
                value,
            }),
        }
    }
}

/// Who is steering the actor.
///
/// A designed vocabulary, not a measurement of any original control mode: the
/// original's own control modes are unmeasured, so this is the minimum a
/// snapshot needs to tell a pilot from a scripted aircraft.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlMode {
    /// A pilot, or a player-controlled aircraft.
    Manual,
    /// The mission program or an AI is steering.
    Autopilot,
    /// Nobody is steering; the airframe is coasting or falling.
    Uncontrolled,
}

impl ControlMode {
    /// Every mode, in a stable order.
    pub const ALL: &'static [ControlMode] = &[Self::Manual, Self::Autopilot, Self::Uncontrolled];

    /// The wire code this schema version uses.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Manual => 0,
            Self::Autopilot => 1,
            Self::Uncontrolled => 2,
        }
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Autopilot => "autopilot",
            Self::Uncontrolled => "uncontrolled",
        }
    }

    /// Reads a wire code.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::UnknownCode`] for a code this schema version does not
    /// define.
    pub fn from_code(value: u8) -> Result<Self, SnapshotError> {
        match value {
            0 => Ok(Self::Manual),
            1 => Ok(Self::Autopilot),
            2 => Ok(Self::Uncontrolled),
            _ => Err(SnapshotError::UnknownCode {
                field: "control_mode",
                value,
            }),
        }
    }
}

/// Which ammunition bank an actor's trigger is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bank {
    /// The primary bank.
    Primary,
    /// The secondary bank.
    Secondary,
}

impl Bank {
    /// Every bank, in a stable order.
    pub const ALL: &'static [Bank] = &[Self::Primary, Self::Secondary];

    /// The wire code this schema version uses.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Primary => 0,
            Self::Secondary => 1,
        }
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
        }
    }

    /// Reads a wire code.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::UnknownCode`] for a code this schema version does not
    /// define.
    pub fn from_code(value: u8) -> Result<Self, SnapshotError> {
        match value {
            0 => Ok(Self::Primary),
            1 => Ok(Self::Secondary),
            _ => Err(SnapshotError::UnknownCode {
                field: "selected_bank",
                value,
            }),
        }
    }
}

// ------------------------------------------------------ epoch + channels ----

/// The origin epoch a snapshot's positions are measured against.
///
/// The epoch is *shared*: both ends must name the same frame for the same world
/// origin, and a receiver with no anchor for an epoch refuses the records rather
/// than placing them at a guessed zero. Epochs increase and are never reused, so
/// a stale packet's epoch can always be told apart from the live one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OriginEpoch(pub u32);

impl OriginEpoch {
    /// The next epoch after this one.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::EpochExhausted`] when the counter would wrap; an epoch
    /// must never name two different frames.
    pub const fn next(self) -> Result<Self, SnapshotError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(SnapshotError::EpochExhausted),
        }
    }
}

impl fmt::Display for OriginEpoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "origin epoch {}", self.0)
    }
}

/// Encodes one unit fraction into [`UNIT_FRACTION_QUANTIZATION`] steps.
///
/// # Errors
///
/// [`SnapshotError::OutOfRange`] for a fraction outside `[0, 1]`.
fn encode_unit_fraction(field: &'static str, value: f64) -> Result<u16, SnapshotError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(SnapshotError::OutOfRange {
            field,
            value,
            max: 1.0,
        });
    }
    u16::try_from(UNIT_FRACTION_QUANTIZATION.encode(value)?).map_err(|_| {
        SnapshotError::OutOfRange {
            field,
            value,
            max: 1.0,
        }
    })
}

/// Dequantizes one unit-fraction channel value.
fn dequantize_unit_fraction(raw: u16) -> f64 {
    f64::from(raw) / UNIT_FRACTION_QUANTIZATION.steps_per_unit()
}

/// The essential flight state one record carries: throttle, engine spool and
/// the boost capacity that remains.
///
/// These are capacities and settings, not consumptions: a boost press the server
/// refused cannot spend this capacity, and a client's predicted boost never
/// writes it (UI-NETWORK ownership table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlightChannel {
    /// Commanded throttle, in [`UNIT_FRACTION_QUANTIZATION`] steps.
    pub throttle: u16,
    /// Engine spool, in [`UNIT_FRACTION_QUANTIZATION`] steps.
    pub engine_spool: u16,
    /// Boost capacity still available, in [`UNIT_FRACTION_QUANTIZATION`] steps.
    pub boost_capacity: u16,
}

impl FlightChannel {
    /// Builds the channel from real fractions in `[0, 1]`.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] for a fraction outside the unit range.
    pub fn from_fractions(
        throttle: f64,
        engine_spool: f64,
        boost_capacity: f64,
    ) -> Result<Self, SnapshotError> {
        Ok(Self {
            throttle: encode_unit_fraction("throttle", throttle)?,
            engine_spool: encode_unit_fraction("engine_spool", engine_spool)?,
            boost_capacity: encode_unit_fraction("boost_capacity", boost_capacity)?,
        })
    }

    /// The three fractions, dequantized: throttle, engine spool, boost
    /// capacity.
    #[must_use]
    pub fn fractions(self) -> [f64; 3] {
        [
            dequantize_unit_fraction(self.throttle),
            dequantize_unit_fraction(self.engine_spool),
            dequantize_unit_fraction(self.boost_capacity),
        ]
    }
}

/// The essential damage state one record carries: how much structure is left and
/// which mounts the damage resolver disabled.
///
/// Remaining integrity is a *summary* for presentation and targeting. It can
/// never award damage: the authoritative damage ledger is server-side and this
/// channel is read-only to a client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageChannel {
    /// Remaining structural integrity in parts per thousand of the airframe's
    /// full integrity, so `1000` is undamaged.
    pub integrity_permille: u16,
    /// Bit mask of the weapon mounts the damage resolver has disabled.
    pub disabled_mounts: u16,
}

impl DamageChannel {
    /// An undamaged airframe with every mount enabled.
    pub const PRISTINE: Self = Self {
        integrity_permille: 1000,
        disabled_mounts: 0,
    };

    /// Builds the channel from a remaining-integrity fraction.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] for a fraction outside `[0, 1]`.
    pub fn from_integrity_fraction(
        remaining: f64,
        disabled_mounts: u16,
    ) -> Result<Self, SnapshotError> {
        if !remaining.is_finite() || !(0.0..=1.0).contains(&remaining) {
            return Err(SnapshotError::OutOfRange {
                field: "integrity_permille",
                value: remaining,
                max: 1.0,
            });
        }
        let permille = INTEGRITY_PERMILLE_QUANTIZATION.encode(remaining * 1000.0)?;
        let permille = u16::try_from(permille).map_err(|_| SnapshotError::OutOfRange {
            field: "integrity_permille",
            value: f64::from(i32::MAX),
            max: 1000.0,
        })?;
        Ok(Self {
            integrity_permille: permille,
            disabled_mounts,
        })
    }

    /// The remaining integrity as a fraction of full.
    #[must_use]
    pub fn integrity_fraction(self) -> f64 {
        f64::from(self.integrity_permille) / 1000.0
    }
}

/// The essential weapon state one record carries: rounds per bank and which
/// bank is selected.
///
/// Ammunition is **authoritative**: these counters are the server's, and a
/// client's predicted shot changes nothing here (F57 sheet non-negotiable
/// behavior 3, UI-NETWORK ownership table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponChannel {
    /// Rounds in the primary bank.
    pub primary_rounds: u16,
    /// Rounds in the secondary bank.
    pub secondary_rounds: u16,
    /// Which bank the trigger is on.
    pub selected: Bank,
}

impl WeaponChannel {
    /// Builds the channel from exact round counts.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] for a count outside the declared 16-bit
    /// unsigned budget.
    pub fn from_rounds(
        primary_rounds: u16,
        secondary_rounds: u16,
        selected: Bank,
    ) -> Result<Self, SnapshotError> {
        ROUNDS_QUANTIZATION.encode(f64::from(primary_rounds))?;
        ROUNDS_QUANTIZATION.encode(f64::from(secondary_rounds))?;
        Ok(Self {
            primary_rounds,
            secondary_rounds,
            selected,
        })
    }

    /// The rounds in `bank`.
    #[must_use]
    pub const fn rounds(self, bank: Bank) -> u16 {
        match bank {
            Bank::Primary => self.primary_rounds,
            Bank::Secondary => self.secondary_rounds,
        }
    }
}

// ------------------------------------------------------------- records ----

/// One actor's snapshot record: the fixed [`ACTOR_RECORD_BYTES`] of quantized
/// state a sequenced snapshot publishes.
///
/// Every numeric field is an integer in its declared quantization, so no field
/// of a decoded record can be non-finite and two decoders of the same bytes
/// read the same numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorRecord {
    /// Which actor this record describes. Its session must be the one the
    /// snapshot was accepted for.
    pub actor: ActorId,
    /// The actor's generation. A generation change under the same id is a new
    /// actor: no state, buffer or history of the previous generation may be
    /// reused for it (UI-NETWORK, "Interpolation buffers separate actor
    /// generations").
    pub generation: u16,
    /// What the server last did with the actor.
    pub lifecycle: Lifecycle,
    /// Who is steering.
    pub control: ControlMode,
    /// Position relative to the snapshot's origin epoch, in
    /// [`POSITION_QUANTIZATION`] steps.
    pub position: QuantizedVector,
    /// Body-to-world orientation.
    pub rotation: QuantizedRotation,
    /// World-space linear velocity, in [`LINEAR_VELOCITY_QUANTIZATION`] steps.
    pub linear_velocity: QuantizedVector,
    /// Body-space angular velocity, in [`ANGULAR_VELOCITY_QUANTIZATION`] steps.
    pub angular_velocity: QuantizedVector,
    /// Throttle, engine spool and remaining boost capacity.
    pub flight: FlightChannel,
    /// Remaining integrity and disabled mounts.
    pub damage: DamageChannel,
    /// Rounds per bank and the selected bank.
    pub weapons: WeaponChannel,
}

impl ActorRecord {
    /// The byte width this record occupies on the wire.
    pub const WIRE_BYTES: usize = ACTOR_RECORD_BYTES;

    /// The record's position in meters relative to the snapshot's origin epoch.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] when a stored step count does not fit the
    /// declared position width.
    pub fn position_m(&self) -> Result<[f64; 3], SnapshotError> {
        self.position.dequantize(POSITION_QUANTIZATION)
    }

    /// The record's world linear velocity, in m/s.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] when a stored step count does not fit the
    /// declared velocity width.
    pub fn linear_velocity_mps(&self) -> Result<[f64; 3], SnapshotError> {
        self.linear_velocity
            .dequantize(LINEAR_VELOCITY_QUANTIZATION)
    }

    /// The record's body angular velocity, in rad/s.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`] when a stored step count does not fit the
    /// declared velocity width.
    pub fn angular_velocity_radps(&self) -> Result<[f64; 3], SnapshotError> {
        self.angular_velocity
            .dequantize(ANGULAR_VELOCITY_QUANTIZATION)
    }

    /// Writes this record's fixed-width encoding.
    fn write_to(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.actor.session.get().to_le_bytes());
        out.extend_from_slice(&self.actor.serial.to_le_bytes());
        out.extend_from_slice(&self.generation.to_le_bytes());
        out.push(self.lifecycle.code());
        out.push(self.control.code());
        self.position.write_to(out, POSITION_QUANTIZATION);
        self.rotation.write_to(out);
        self.linear_velocity
            .write_to(out, LINEAR_VELOCITY_QUANTIZATION);
        self.angular_velocity
            .write_to(out, ANGULAR_VELOCITY_QUANTIZATION);
        out.extend_from_slice(&self.flight.throttle.to_le_bytes());
        out.extend_from_slice(&self.flight.engine_spool.to_le_bytes());
        out.extend_from_slice(&self.flight.boost_capacity.to_le_bytes());
        out.extend_from_slice(&self.damage.integrity_permille.to_le_bytes());
        out.extend_from_slice(&self.damage.disabled_mounts.to_le_bytes());
        out.extend_from_slice(&self.weapons.primary_rounds.to_le_bytes());
        out.extend_from_slice(&self.weapons.secondary_rounds.to_le_bytes());
        out.push(self.weapons.selected.code());
    }

    /// Reads one record, checking every stored field against its declaration.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::Truncated`] for a short payload, and the identity, code
    /// and range refusals this schema applies to a record.
    fn read_from(cursor: &mut Cursor<'_>, session: SessionId) -> Result<Self, SnapshotError> {
        let session_field = cursor.u64()?;
        if session_field == 0 {
            return Err(SnapshotError::InvalidSession { found: 0 });
        }
        let Some(actor_session) = SessionId::new(session_field) else {
            return Err(SnapshotError::InvalidSession {
                found: session_field,
            });
        };
        let serial = cursor.u64()?;
        let actor = ActorId {
            session: actor_session,
            serial,
        };
        if actor.session != session {
            return Err(SnapshotError::ForeignSession {
                expected: session,
                actor,
            });
        }
        if actor.serial == 0 {
            return Err(SnapshotError::ReservedSerial { actor });
        }
        let generation = cursor.u16()?;
        if generation == 0 {
            return Err(SnapshotError::OutOfRange {
                field: "generation",
                value: 0.0,
                max: f64::from(u16::MAX),
            });
        }
        let lifecycle = Lifecycle::from_code(cursor.u8()?)?;
        let control = ControlMode::from_code(cursor.u8()?)?;
        let position = QuantizedVector::read_from(cursor, POSITION_QUANTIZATION)?;
        let rotation = QuantizedRotation::read_from(cursor)?;
        let linear_velocity = QuantizedVector::read_from(cursor, LINEAR_VELOCITY_QUANTIZATION)?;
        let angular_velocity = QuantizedVector::read_from(cursor, ANGULAR_VELOCITY_QUANTIZATION)?;
        let throttle = cursor.u16()?;
        let engine_spool = cursor.u16()?;
        let boost_capacity = cursor.u16()?;
        let integrity_permille = cursor.u16()?;
        let disabled_mounts = cursor.u16()?;
        let primary_rounds = cursor.u16()?;
        let secondary_rounds = cursor.u16()?;
        let selected = Bank::from_code(cursor.u8()?)?;
        // A "fraction" outside [0, 1] in a hostile or foreign payload must not
        // reach a consumer that trusts the channel's declaration.
        for (field, raw) in [
            ("throttle", throttle),
            ("engine_spool", engine_spool),
            ("boost_capacity", boost_capacity),
        ] {
            let fraction = dequantize_unit_fraction(raw);
            if !(0.0..=1.0).contains(&fraction) {
                return Err(SnapshotError::OutOfRange {
                    field,
                    value: fraction,
                    max: 1.0,
                });
            }
        }
        if integrity_permille > 1000 {
            return Err(SnapshotError::OutOfRange {
                field: "integrity_permille",
                value: f64::from(integrity_permille),
                max: 1000.0,
            });
        }
        ROUNDS_QUANTIZATION.encode(f64::from(primary_rounds))?;
        ROUNDS_QUANTIZATION.encode(f64::from(secondary_rounds))?;
        Ok(Self {
            actor,
            generation,
            lifecycle,
            control,
            position,
            rotation,
            linear_velocity,
            angular_velocity,
            flight: FlightChannel {
                throttle,
                engine_spool,
                boost_capacity,
            },
            damage: DamageChannel {
                integrity_permille,
                disabled_mounts,
            },
            weapons: WeaponChannel {
                primary_rounds,
                secondary_rounds,
                selected,
            },
        })
    }
}

// ------------------------------------------------------------ snapshot ----

/// One tick's quantized physics truth for a bounded set of actors.
///
/// `origin` is the epoch every position is relative to, `input_ack` is the
/// highest client input sequence the server consumed (so a client can stop
/// resending), and `actors` holds one [`ActorRecord`] per published actor.
/// There is no tick field here: the tick is the F54-A envelope's
/// [`crate::message::SnapshotFrame::tick`], stamped once and only once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// The origin epoch the positions are measured against.
    pub origin: OriginEpoch,
    /// The highest client input sequence the server consumed for this tick.
    pub input_ack: u32,
    /// The published actor records, in wire order.
    pub actors: Vec<ActorRecord>,
}

impl Snapshot {
    /// A snapshot for `actors` under `origin`, acknowledging `input_ack`.
    #[must_use]
    pub fn new(origin: OriginEpoch, input_ack: u32, actors: Vec<ActorRecord>) -> Self {
        Self {
            origin,
            input_ack,
            actors,
        }
    }

    /// The exact byte length [`Self::encode`] produces.
    #[must_use]
    pub const fn encoded_len(&self) -> usize {
        SNAPSHOT_HEADER_BYTES + self.actors.len() * ACTOR_RECORD_BYTES
    }

    /// The record for `actor`, if this snapshot carries one.
    #[must_use]
    pub fn actor(&self, actor: ActorId) -> Option<&ActorRecord> {
        self.actors.iter().find(|record| record.actor == actor)
    }

    /// Checks the whole snapshot against the declared schema and budgets.
    ///
    /// The declared budgets are re-checked here, so a snapshot is valid only
    /// while every quantization it depends on is a usable declaration. Each
    /// record is checked for session, serial and generation validity, integrity
    /// is bounded by its permille range, every stored position/velocity integer
    /// must fit the width its own field declares, and an actor may appear only
    /// once.
    ///
    /// # Errors
    ///
    /// The first [`SnapshotError`] the checks report: `Empty` or
    /// `TooManyActors` for the population bounds; `ForeignSession`,
    /// `ReservedSerial`, `DuplicateActor` or `OutOfRange` for a record.
    pub fn validate(&self, session: SessionId) -> Result<(), SnapshotError> {
        for budget in SNAPSHOT_BUDGET {
            budget.validate()?;
        }
        if self.actors.is_empty() {
            return Err(SnapshotError::Empty);
        }
        if self.actors.len() > MAX_ACTORS_PER_SNAPSHOT {
            return Err(SnapshotError::TooManyActors {
                max: MAX_ACTORS_PER_SNAPSHOT,
                len: self.actors.len(),
            });
        }
        let mut seen = BTreeSet::new();
        for record in &self.actors {
            if record.actor.session != session {
                return Err(SnapshotError::ForeignSession {
                    expected: session,
                    actor: record.actor,
                });
            }
            if record.actor.serial == 0 {
                return Err(SnapshotError::ReservedSerial {
                    actor: record.actor,
                });
            }
            if !seen.insert(record.actor) {
                return Err(SnapshotError::DuplicateActor {
                    actor: record.actor,
                });
            }
            if record.generation == 0 {
                return Err(SnapshotError::OutOfRange {
                    field: "generation",
                    value: 0.0,
                    max: f64::from(u16::MAX),
                });
            }
            if record.damage.integrity_permille > 1000 {
                return Err(SnapshotError::OutOfRange {
                    field: "integrity_permille",
                    value: f64::from(record.damage.integrity_permille),
                    max: 1000.0,
                });
            }
            // Every spatial field must hold integers its own declaration can
            // store, so a record assembled under a different budget is refused
            // here instead of being narrowed on the way to the wire.
            record.position.check_width(POSITION_QUANTIZATION)?;
            record
                .linear_velocity
                .check_width(LINEAR_VELOCITY_QUANTIZATION)?;
            record
                .angular_velocity
                .check_width(ANGULAR_VELOCITY_QUANTIZATION)?;
        }
        Ok(())
    }

    /// Encodes the snapshot into the bytes a
    /// [`crate::message::SnapshotFrame`] carries.
    ///
    /// # Errors
    ///
    /// Any [`SnapshotError`] [`Self::validate`] reports, plus a refusal when the
    /// encoded length would exceed the F54-A envelope cap
    /// [`MAX_SNAPSHOT_BYTES`]: the writer never emits a payload the envelope
    /// would reject.
    pub fn encode(&self, session: SessionId) -> Result<Vec<u8>, SnapshotError> {
        self.validate(session)?;
        let mut out = Vec::with_capacity(self.encoded_len());
        out.push(SNAPSHOT_SCHEMA_VERSION);
        out.extend_from_slice(&self.origin.0.to_le_bytes());
        out.extend_from_slice(&self.input_ack.to_le_bytes());
        let count = u16::try_from(self.actors.len()).map_err(|_| SnapshotError::TooManyActors {
            max: MAX_ACTORS_PER_SNAPSHOT,
            len: self.actors.len(),
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for record in &self.actors {
            record.write_to(&mut out);
        }
        if out.len() > MAX_SNAPSHOT_BYTES {
            return Err(SnapshotError::OutOfRange {
                field: "snapshot.payload",
                value: out.len() as f64,
                max: MAX_SNAPSHOT_BYTES as f64,
            });
        }
        Ok(out)
    }

    /// Decodes the bytes of one snapshot accepted for `session`.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::SchemaVersion`] for another schema version,
    /// `Truncated`/`TrailingBytes` for a payload that does not match its own
    /// declared count, and every refusal [`Self::validate`] reports.
    pub fn decode(payload: &[u8], session: SessionId) -> Result<Self, SnapshotError> {
        let mut cursor = Cursor::new(payload);
        let version = cursor.u8()?;
        if version != SNAPSHOT_SCHEMA_VERSION {
            return Err(SnapshotError::SchemaVersion {
                found: version,
                expected: SNAPSHOT_SCHEMA_VERSION,
            });
        }
        let origin = OriginEpoch(cursor.u32()?);
        let input_ack = cursor.u32()?;
        let count = usize::from(cursor.u16()?);
        if count == 0 {
            return Err(SnapshotError::Empty);
        }
        if count > MAX_ACTORS_PER_SNAPSHOT {
            return Err(SnapshotError::TooManyActors {
                max: MAX_ACTORS_PER_SNAPSHOT,
                len: count,
            });
        }
        let mut actors = Vec::with_capacity(count);
        for _ in 0..count {
            actors.push(ActorRecord::read_from(&mut cursor, session)?);
        }
        if cursor.remaining() != 0 {
            return Err(SnapshotError::TrailingBytes {
                len: cursor.remaining(),
            });
        }
        let snapshot = Self {
            origin,
            input_ack,
            actors,
        };
        snapshot.validate(session)?;
        Ok(snapshot)
    }

    /// Encodes and wraps into the F54-A sequenced snapshot envelope.
    ///
    /// # Errors
    ///
    /// Any [`SnapshotError`] [`Self::encode`] reports.
    pub fn into_frame(
        self,
        session: SessionId,
        tick: Tick,
    ) -> Result<SnapshotFrame, SnapshotError> {
        let payload = self.encode(session)?;
        Ok(SnapshotFrame { tick, payload })
    }

    /// Decodes the payload of an F54-A sequenced snapshot envelope.
    ///
    /// # Errors
    ///
    /// Any [`SnapshotError`] [`Self::decode`] reports.
    pub fn from_frame(frame: &SnapshotFrame, session: SessionId) -> Result<Self, SnapshotError> {
        Self::decode(&frame.payload, session)
    }
}

/// A bounds-checked reader over a snapshot payload: every read is either a
/// complete field or a named [`SnapshotError::Truncated`], so a short payload is
/// never read past its end.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, need: usize) -> Result<&'a [u8], SnapshotError> {
        let have = self.bytes.len() - self.at;
        if have < need {
            return Err(SnapshotError::Truncated { need, have });
        }
        let slice = &self.bytes[self.at..self.at + need];
        self.at += need;
        Ok(slice)
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn u8(&mut self) -> Result<u8, SnapshotError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, SnapshotError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, SnapshotError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, SnapshotError> {
        let bytes = self.take(8)?;
        let mut value = [0_u8; 8];
        value.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(value))
    }

    fn i16(&mut self) -> Result<i16, SnapshotError> {
        let bytes = self.take(2)?;
        Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn i32(&mut self) -> Result<i32, SnapshotError> {
        let bytes = self.take(4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

// ----------------------------------------------------- synthetic fixture ----

/// The origin epoch the synthetic snapshot fixture publishes under.
pub const SYNTHETIC_ORIGIN_EPOCH: OriginEpoch = OriginEpoch(1);

/// One synthetic actor record: an intact, level airframe at `offset_m` from the
/// origin epoch, flying forward at 30 m/s with a nonzero round count in both
/// ammunition banks.
///
/// Newly authored development content marked `SYNTHETIC`; it can never stand in
/// for a captured retail session.
#[must_use]
pub fn synthetic_actor_record(actor: ActorId, generation: u16, offset_m: [f64; 3]) -> ActorRecord {
    ActorRecord {
        actor,
        generation,
        lifecycle: Lifecycle::Alive,
        control: ControlMode::Manual,
        position: QuantizedVector::quantize(POSITION_QUANTIZATION, offset_m)
            .expect("the synthetic offset is inside the declared position range"),
        rotation: QuantizedRotation::encode(Quaternion::IDENTITY)
            .expect("the identity rotation is inside the declared rotation range"),
        linear_velocity: QuantizedVector::quantize(LINEAR_VELOCITY_QUANTIZATION, [0.0, 0.0, -30.0])
            .expect("30 m/s is inside the declared velocity range"),
        angular_velocity: QuantizedVector::quantize(ANGULAR_VELOCITY_QUANTIZATION, [0.0; 3])
            .expect("zero angular velocity is inside the declared range"),
        flight: FlightChannel::from_fractions(1.0, 1.0, 0.5)
            .expect("the synthetic fractions are inside the unit range"),
        damage: DamageChannel::PRISTINE,
        weapons: WeaponChannel {
            primary_rounds: 400,
            secondary_rounds: 64,
            selected: Bank::Primary,
        },
    }
}

/// The minimal synthetic snapshot: two actors under [`SYNTHETIC_ORIGIN_EPOCH`]
/// with client input sequence 7 acknowledged.
#[must_use]
pub fn synthetic_snapshot(first: ActorId, second: ActorId) -> Snapshot {
    Snapshot::new(
        SYNTHETIC_ORIGIN_EPOCH,
        7,
        vec![
            synthetic_actor_record(first, 1, [120.0, 40.0, -600.0]),
            synthetic_actor_record(second, 1, [-80.0, 55.0, -520.0]),
        ],
    )
}
