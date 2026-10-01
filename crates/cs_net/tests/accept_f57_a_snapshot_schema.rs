//! F57-A acceptance: the snapshot schema, its quantization budgets and its
//! refusals.
//!
//! Minimum scenario (sheet, `### F57-A`): "Inject latency/loss/reordering; no
//! duplicate destruction or permanent ghost aircraft." The transport-level part
//! of that scenario is covered here — a snapshot must survive reordering and
//! loss without its numbers, its identity or its lifecycle becoming something a
//! receiver can misread — and the end-to-end run is in
//! `crates/cs_app/tests/accept_f57_a_latency_loss_and_reordering.rs`, which
//! drives this schema over a lossy, delayed, reordered link.
//!
//! Everything here calls production code in `cs_net::snapshot`. The file pins
//! the properties the later stages depend on: the declared budgets are explicit
//! and *satisfied*, the codec is exact except inside the declared error, and a
//! malformed or foreign payload is refused by name instead of guessed at.

use cs_net::bounds::MAX_SNAPSHOT_BYTES;
use cs_net::message::{ServerMessage, ServerPayload};
use cs_net::snapshot::{
    ACTOR_RECORD_BYTES, ANGULAR_VELOCITY_QUANTIZATION, Bank, ControlMode, DamageChannel,
    INTEGRITY_PERMILLE_QUANTIZATION, LINEAR_VELOCITY_QUANTIZATION, Lifecycle,
    MAX_ACTORS_PER_SNAPSHOT, OriginEpoch, POSITION_QUANTIZATION, Quantization, ROTATION_BYTES,
    ROTATION_ORIENTATION_ERROR_RAD, ROUNDS_QUANTIZATION, SNAPSHOT_BUDGET, SNAPSHOT_HEADER_BYTES,
    SNAPSHOT_SCHEMA_VERSION, Snapshot, SnapshotError, UNIT_FRACTION_QUANTIZATION,
    synthetic_actor_record, synthetic_snapshot,
};
use cs_types::Tick;
use cs_types::net::{ActorAllocator, ActorId, SessionId};
use cs_types::random::SplitMix64;
use cs_types::space::{Quaternion, Radians, UnitVec3};

const SESSION: SessionId = match SessionId::new(11) {
    Some(id) => id,
    None => unreachable!(),
};

/// Three distinct actors in `SESSION`, allocated through the real allocator.
fn three_actors() -> [ActorId; 3] {
    let mut allocator = ActorAllocator::new(SESSION);
    [
        allocator.allocate().expect("serial space"),
        allocator.allocate().expect("serial space"),
        allocator.allocate().expect("serial space"),
    ]
}

/// The largest angle, in radians, by which two unit quaternions disagree.
///
/// Two unit quaternions whose dot product is `c` represent rotations that differ
/// by exactly `2*acos(|c|)`: `|q1 - q2| = 2*sin(theta/4)`, so the rotation angle
/// `theta` is *twice* the chord, not the chord. Both arguments are normalized
/// first because
/// [`Quaternion::try_new`](cs_types::space::Quaternion) accepts a rotation within
/// `QUATERNION_LENGTH_TOLERANCE` of unit length, while `decode` returns a
/// strictly unit rotation — measuring against a denormalized input would report
/// the input's own error as the encoding's.
fn orientation_error_rad(a: Quaternion, b: Quaternion) -> f64 {
    let [ax, ay, az, aw] = normalized_components(a);
    let [bx, by, bz, bw] = normalized_components(b);
    let dot = (ax * bx + ay * by + az * bz + aw * bw).abs();
    2.0 * dot.clamp(-1.0, 1.0).min(1.0).acos()
}

/// The components of `rotation`, scaled to exactly unit length.
fn normalized_components(rotation: Quaternion) -> [f64; 4] {
    let [x, y, z, w] = rotation.components();
    let length = (x * x + y * y + z * z + w * w).sqrt();
    if length > 0.0 {
        [x / length, y / length, z / length, w / length]
    } else {
        [x, y, z, w]
    }
}

/// The structural worst cases of the smallest-three encoding: a rotation whose
/// four components are near `1/2` in magnitude, so the dropped component is the
/// smallest one and its reconstruction from the unit-norm constraint is the most
/// amplified. These are the rotations that decide the declared budget; a sweep of
/// well-separated rotations alone never reaches them.
fn near_equal_component_rotations() -> Vec<Quaternion> {
    let mut rotations = Vec::new();
    // All sixteen sign patterns of (+-0.5, +-0.5, +-0.5, +-0.5): the components
    // are exactly equal in magnitude, so whichever one is dropped leaves the
    // other three at their worst ratio to it.
    for bits in 0_u8..16 {
        rotations.push(
            Quaternion::try_new([
                if bits & 1 == 0 { 0.5 } else { -0.5 },
                if bits & 2 == 0 { 0.5 } else { -0.5 },
                if bits & 4 == 0 { 0.5 } else { -0.5 },
                if bits & 8 == 0 { 0.5 } else { -0.5 },
            ])
            .expect("exactly unit"),
        );
    }
    // The same structure reached continuously: every rotation about the diagonal
    // (1,1,1)/sqrt(3) axis passes through a state where three components are
    // equal, which is where the dropped component is smallest.
    let diagonal = UnitVec3::try_new([
        1.0 / 3.0_f64.sqrt(),
        1.0 / 3.0_f64.sqrt(),
        1.0 / 3.0_f64.sqrt(),
    ])
    .expect("unit");
    let steps = 720;
    for step in 0..steps {
        let angle = std::f64::consts::TAU * step as f64 / steps as f64;
        rotations.push(Quaternion::from_axis_angle(diagonal, Radians(angle)).expect("unit"));
    }
    rotations
}

/// The declared budgets are explicit: every entry names its field, unit, scale
/// and width, and the error budget is exactly half a step — not a number a test
/// re-derives for itself.
#[test]
fn accept_f57_a_declared_budgets_are_explicit_and_half_a_step() {
    assert_eq!(SNAPSHOT_BUDGET.len(), 6);
    for budget in SNAPSHOT_BUDGET {
        assert_eq!(
            budget.validate(),
            Ok(()),
            "every declared budget must be a usable declaration: {budget:?}"
        );
        assert!(!budget.field().is_empty(), "a budget must name its field");
        assert!(!budget.unit().is_empty(), "a budget must name its unit");
        assert!(
            (budget.max_error() - 0.5 * budget.step()).abs() < f64::EPSILON,
            "the error budget must be half a quantization step"
        );
        assert!(
            budget.steps_per_unit() > 0.0 && budget.step() > 0.0,
            "a declared scale must be a positive step"
        );
        assert!(
            budget.max_value() > budget.min_value(),
            "a declared width must cover a positive range"
        );
    }
    // The scales are the ones the field docs declare, in each field's own unit.
    assert_eq!(POSITION_QUANTIZATION.steps_per_unit(), 64.0);
    assert_eq!(POSITION_QUANTIZATION.max_error(), 1.0 / 128.0);
    assert_eq!(LINEAR_VELOCITY_QUANTIZATION.steps_per_unit(), 20.0);
    assert_eq!(ANGULAR_VELOCITY_QUANTIZATION.steps_per_unit(), 512.0);
    assert_eq!(UNIT_FRACTION_QUANTIZATION.max_value(), 65535.0 / 65535.0);
    assert_eq!(INTEGRITY_PERMILLE_QUANTIZATION.max_error(), 0.5);
    assert_eq!(ROUNDS_QUANTIZATION.max_error(), 0.5);

    // Position has to be able to carry a far-from-origin world coordinate, which
    // is the whole reason positions are epoch-relative.
    assert!(
        POSITION_QUANTIZATION.max_value() > 1.0e6,
        "position range must cover a world-scale distance from the origin epoch"
    );
    // And the position budget has to be finer than the smallest motion one tick
    // of flight can produce, or the round trip would be visible.
    assert!(
        POSITION_QUANTIZATION.max_error() < 0.05,
        "position error budget must be below 5 cm"
    );
}

/// A value outside the declared width is refused, not saturated; and a
/// quantizer that cannot store its declared width is itself refused, so a
/// declared budget can never disagree with the bytes on the wire.
#[test]
fn accept_f57_a_out_of_range_values_are_refused_not_saturated() {
    // One step past the declared position range.
    let beyond = POSITION_QUANTIZATION.max_value() + POSITION_QUANTIZATION.step();
    assert!(
        POSITION_QUANTIZATION.encode(beyond).is_err(),
        "a position past the declared range must be refused, not clamped"
    );
    assert!(
        POSITION_QUANTIZATION
            .encode(POSITION_QUANTIZATION.max_value())
            .is_ok()
    );
    assert!(
        POSITION_QUANTIZATION.encode(f64::NAN).is_err(),
        "a non-finite value must be refused at the boundary"
    );
    assert!(POSITION_QUANTIZATION.encode(f64::INFINITY).is_err());

    // A declared width this schema cannot store is a refusal, not a silently
    // narrowed field.
    let unwritable = Quantization::signed("hypothetical", "m", 64.0, 24);
    assert!(matches!(
        unwritable.validate(),
        Err(SnapshotError::OutOfRange {
            field: "hypothetical",
            ..
        })
    ));
    assert_eq!(
        unwritable.storage_bytes(),
        Err(SnapshotError::OutOfRange {
            field: "hypothetical",
            value: 24.0,
            max: 32.0,
        })
    );

    // A stored integer that does not fit its declared width cannot decode.
    assert!(
        LINEAR_VELOCITY_QUANTIZATION.decode(40_000).is_err(),
        "a stored step count beyond the declared 16-bit width must not decode"
    );
    assert_eq!(
        LINEAR_VELOCITY_QUANTIZATION.decode(-32_767),
        Ok(-1638.35_f64)
    );
}

/// Every numeric field of a record survives the codec inside its declared error
/// budget, over a deterministic sweep that includes the extremes of each
/// declared range. This is the property that makes the budgets claims rather
/// than comments: if a quantizer were wrong (a wrong scale, a swapped axis, a
/// lost sign) the measured error would exceed the declared budget and fail here.
#[test]
fn accept_f57_a_every_field_round_trips_inside_its_declared_error_budget() {
    let mut rng = SplitMix64::new(0x00F5_7A00_0001_0001);
    let mut worst = [0.0_f64; 5];
    for sample in 0..256 {
        let actor = ActorId {
            session: SESSION,
            serial: sample as u64 + 1,
        };
        // Sweep each component across the declared range, so the extremes are
        // exercised and not just values near zero.
        let mut spread = |magnitude: f64| -> f64 {
            let unit = cs_types::random::unit_f64(rng.next_u64());
            let sign = if (rng.next_u64() & 1) == 1 { -1.0 } else { 1.0 };
            sign * magnitude * unit
        };
        let position = [spread(12_000.0), spread(12_000.0), spread(12_000.0)];
        let linear = [spread(400.0), spread(400.0), spread(400.0)];
        let angular = [spread(6.0), spread(6.0), spread(6.0)];
        let throttle = cs_types::random::unit_f64(rng.next_u64());
        let spool = cs_types::random::unit_f64(rng.next_u64());
        let boost = cs_types::random::unit_f64(rng.next_u64());
        let integrity = cs_types::random::unit_f64(rng.next_u64());
        let rounds = u16::try_from(rng.next_u64() % u64::from(u16::MAX)).expect("mod fits");
        let bank = if (rng.next_u64() & 1) == 1 {
            Bank::Secondary
        } else {
            Bank::Primary
        };
        let axis = [
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("unit"),
            UnitVec3::try_new([0.0, 1.0, 0.0]).expect("unit"),
            UnitVec3::try_new([0.0, 0.0, 1.0]).expect("unit"),
            UnitVec3::try_new([-0.3 / 0.989_949_49, 0.5 / 0.989_949_49, -0.8 / 0.989_949_49])
                .expect("unit"),
        ][sample % 4];
        let angle = Radians(cs_types::random::unit_f64(rng.next_u64()) * 7.0 - 3.5);
        let orientation =
            Quaternion::from_axis_angle(axis, angle).expect("unit-length axis and finite angle");

        let mut record = synthetic_actor_record(actor, 1, position);
        record.position =
            cs_net::snapshot::QuantizedVector::quantize(POSITION_QUANTIZATION, position)
                .expect("the swept position is inside the declared range");
        record.rotation = cs_net::snapshot::QuantizedRotation::encode(orientation)
            .expect("a unit rotation is inside the declared range");
        record.linear_velocity =
            cs_net::snapshot::QuantizedVector::quantize(LINEAR_VELOCITY_QUANTIZATION, linear)
                .expect("the swept velocity is inside the declared range");
        record.angular_velocity =
            cs_net::snapshot::QuantizedVector::quantize(ANGULAR_VELOCITY_QUANTIZATION, angular)
                .expect("the swept angular velocity is inside the declared range");
        record.flight = cs_net::snapshot::FlightChannel::from_fractions(throttle, spool, boost)
            .expect("swept fractions are unit values");
        record.damage = DamageChannel::from_integrity_fraction(integrity, u16::MAX)
            .expect("swept integrity is a fraction");
        record.weapons = cs_net::snapshot::WeaponChannel::from_rounds(rounds, rounds, bank)
            .expect("swept rounds are in range");
        record.control = ControlMode::ALL[sample % ControlMode::ALL.len()];
        record.lifecycle = Lifecycle::ALL[sample % Lifecycle::ALL.len()];

        let snapshot = Snapshot::new(OriginEpoch(4), sample as u32, vec![record]);
        let payload = snapshot.encode(SESSION).expect("the swept record encodes");
        let decoded = Snapshot::decode(&payload, SESSION).expect("the swept record decodes");
        let got = decoded.actor(actor).expect("the record is present");

        let got_position = got.position_m().expect("in range");
        let got_linear = got.linear_velocity_mps().expect("in range");
        let got_angular = got.angular_velocity_radps().expect("in range");
        for axis in 0..3 {
            let error = (position[axis] - got_position[axis]).abs();
            assert!(
                error <= POSITION_QUANTIZATION.max_error(),
                "position axis {axis} lost {error} m, budget is {}",
                POSITION_QUANTIZATION.max_error()
            );
            worst[0] = worst[0].max(error);

            let error = (linear[axis] - got_linear[axis]).abs();
            assert!(
                error <= LINEAR_VELOCITY_QUANTIZATION.max_error(),
                "linear velocity axis {axis} lost {error} m/s, budget is {}",
                LINEAR_VELOCITY_QUANTIZATION.max_error()
            );
            worst[1] = worst[1].max(error);

            let error = (angular[axis] - got_angular[axis]).abs();
            assert!(
                error <= ANGULAR_VELOCITY_QUANTIZATION.max_error(),
                "angular velocity axis {axis} lost {error} rad/s, budget is {}",
                ANGULAR_VELOCITY_QUANTIZATION.max_error()
            );
            worst[2] = worst[2].max(error);
        }
        let got_rotation = got.rotation.decode().expect("the rotation decodes");
        let rotation_error = orientation_error_rad(orientation, got_rotation);
        assert!(
            rotation_error <= ROTATION_ORIENTATION_ERROR_RAD,
            "rotation drifted {rotation_error} rad, declared budget is {ROTATION_ORIENTATION_ERROR_RAD}"
        );
        worst[3] = worst[3].max(rotation_error);

        for (index, (want, have)) in [throttle, spool, boost]
            .into_iter()
            .zip(got.flight.fractions())
            .enumerate()
        {
            let error = (want - have).abs();
            assert!(
                error <= UNIT_FRACTION_QUANTIZATION.max_error(),
                "flight channel {index} lost {error}, budget is {}",
                UNIT_FRACTION_QUANTIZATION.max_error()
            );
            worst[4] = worst[4].max(error);
        }
        assert!(
            (integrity - got.damage.integrity_fraction()).abs()
                <= INTEGRITY_PERMILLE_QUANTIZATION.max_error() / 1000.0
        );
        assert_eq!(got.weapons.rounds(bank), rounds);
        assert_eq!(got.weapons.selected, bank);
        assert_eq!(got.control, record.control);
        assert_eq!(got.lifecycle, record.lifecycle);
        assert_eq!(got.generation, 1);
        assert_eq!(decoded.origin, OriginEpoch(4));
        assert_eq!(decoded.input_ack, sample as u32);
    }

    // The budgets are not vacuous: over the sweep the quantization really does
    // spend most of a step, so a test that only asserted "error <= budget"
    // could not be passing against a quantizer that dropped precision entirely.
    assert!(
        worst[0] > POSITION_QUANTIZATION.max_error() / 2.0,
        "position error should approach its budget, measured worst was {}",
        worst[0]
    );
    assert!(
        worst[3] > 0.0 && worst[3] <= ROTATION_ORIENTATION_ERROR_RAD,
        "rotation error should be nonzero and inside its budget, measured worst was {}",
        worst[3]
    );
}

/// The declared orientation budget is an actual upper bound: it holds for a dense
/// random sweep *and* for the structural worst cases of the smallest-three
/// encoding, where the dropped component is smallest and its reconstruction from
/// the unit-norm constraint is most amplified.
///
/// The random sweep alone peaks well below the budget, so a bound that only the
/// random sweep can see would be an unproven one.
#[test]
fn accept_f57_a_the_rotation_budget_bounds_the_structural_worst_cases() {
    let mut rng = SplitMix64::new(0x0000_1200_5EED_0001);
    let mut worst_random = 0.0_f64;
    for _ in 0..4096 {
        let u = |rng: &mut SplitMix64| cs_types::random::unit_f64(rng.next_u64());
        let Ok(axis) = UnitVec3::try_new([
            u(&mut rng) * 2.0 - 1.0,
            u(&mut rng) * 2.0 - 1.0,
            u(&mut rng) * 2.0 - 1.0,
        ]) else {
            continue;
        };
        let rotation =
            Quaternion::from_axis_angle(axis, Radians(u(&mut rng) * std::f64::consts::TAU))
                .expect("unit-length axis and finite angle");
        worst_random = worst_random.max(rotation_error_within_budget(rotation));
    }
    let worst_structural = near_equal_component_rotations()
        .into_iter()
        .map(rotation_error_within_budget)
        .fold(0.0_f64, f64::max);
    // Both families stay inside the declared budget, and the structural family is
    // the one that comes close to it: a budget only the easy family can meet
    // would be unproven.
    assert!(
        worst_structural > worst_random,
        "the structural worst cases must be the binding ones, measured random {worst_random} vs structural {worst_structural}"
    );
    assert!(
        worst_structural > 0.75 * ROTATION_ORIENTATION_ERROR_RAD,
        "the structural worst case must approach the declared budget, measured {worst_structural} of {ROTATION_ORIENTATION_ERROR_RAD}"
    );
}

/// Encodes and decodes `rotation`, asserting the round trip stays inside the
/// declared budget and returning the measured error.
fn rotation_error_within_budget(rotation: Quaternion) -> f64 {
    let encoded = cs_net::snapshot::QuantizedRotation::encode(rotation).expect("unit");
    let decoded = encoded.decode().expect("decodes to unit length");
    let error = orientation_error_rad(rotation, decoded);
    assert!(
        error <= ROTATION_ORIENTATION_ERROR_RAD,
        "rotation drifted {error} rad, declared budget is {ROTATION_ORIENTATION_ERROR_RAD}"
    );
    error
}

/// The smallest-three rotation encoding keeps its index/sign discipline: a
/// dropped component is always the largest, its sign survives, and a corrupted
/// index or sign is detectable rather than silently producing a plausible
/// rotation.
#[test]
fn accept_f57_a_rotation_encoding_drops_the_largest_component_and_keeps_its_sign() {
    for rotation in [
        Quaternion::IDENTITY,
        Quaternion::try_new([-1.0, 0.0, 0.0, 0.0]).expect("unit"),
        Quaternion::try_new([0.0, -0.6, 0.0, 0.8]).expect("unit"),
        Quaternion::from_axis_angle(UnitVec3::UP, Radians(1.2345)).expect("unit"),
        Quaternion::from_axis_angle(
            UnitVec3::try_new([
                1.0 / 3.0_f64.sqrt(),
                1.0 / 3.0_f64.sqrt(),
                1.0 / 3.0_f64.sqrt(),
            ])
            .expect("unit"),
            Radians(2.1),
        )
        .expect("unit"),
    ] {
        let encoded = cs_net::snapshot::QuantizedRotation::encode(rotation).expect("unit");
        let original = rotation.components();
        let index = usize::from(encoded.dropped_index());
        let largest = (0..4).map(|slot| (original[slot].abs(), slot)).fold(
            (0.0_f64, 0_usize),
            |best, (value, slot)| {
                if value > best.0 { (value, slot) } else { best }
            },
        );
        assert_eq!(
            largest.1, index,
            "the dropped component must be a largest one"
        );
        assert_eq!(
            encoded.dropped_sign(),
            u8::from(original[index] < 0.0),
            "the dropped component's sign must survive"
        );
        let decoded = encoded.decode().expect("decodes to unit length");
        let error = orientation_error_rad(rotation, decoded);
        assert!(
            error <= ROTATION_ORIENTATION_ERROR_RAD,
            "rotation drifted {error} rad, budget is {ROTATION_ORIENTATION_ERROR_RAD}"
        );
    }
    // A corrupted sign byte on the wire is a different rotation, not a silently
    // equivalent one: the encoding is read back out of a real payload, so this
    // exercises the codec rather than a private constructor.
    let identity = cs_net::snapshot::QuantizedRotation::encode(Quaternion::IDENTITY)
        .expect("the identity encodes");
    let mut payload = synthetic_snapshot(
        ActorId {
            session: SESSION,
            serial: 1,
        },
        ActorId {
            session: SESSION,
            serial: 2,
        },
    )
    .encode(SESSION)
    .expect("the fixture encodes");
    let sign_byte = SNAPSHOT_HEADER_BYTES + 8 + 8 + 2 + 1 + 1 + 12;
    payload[sign_byte] |= 0b100;
    let decoded = Snapshot::decode(&payload, SESSION).expect("the record still decodes");
    let record = decoded
        .actor(ActorId {
            session: SESSION,
            serial: 1,
        })
        .expect("the record is present");
    assert_ne!(
        record.rotation.decode().expect("unit"),
        Quaternion::IDENTITY,
        "flipping the dropped component's sign must not reproduce the same rotation"
    );
    assert_eq!(record.rotation.dropped_index(), identity.dropped_index());
}

/// A snapshot refuses what it cannot interpret: a foreign session epoch, the
/// reserved serial zero, a duplicate actor, an empty payload and an unknown
/// code. Each refusal is exact, so a caller can tell which invariant broke.
#[test]
fn accept_f57_a_identity_and_population_refusals_are_exact() {
    let [first, second, _] = three_actors();

    // An empty snapshot says nothing; reading it as "the world is empty" would
    // retire every remote actor.
    assert_eq!(
        Snapshot::new(OriginEpoch(1), 0, Vec::new()).validate(SESSION),
        Err(SnapshotError::Empty)
    );
    // Even a payload that declares zero records is refused before allocation.
    let mut empty_payload = vec![SNAPSHOT_SCHEMA_VERSION];
    empty_payload.extend_from_slice(&1_u32.to_le_bytes());
    empty_payload.extend_from_slice(&0_u32.to_le_bytes());
    empty_payload.extend_from_slice(&0_u16.to_le_bytes());
    assert_eq!(
        Snapshot::decode(&empty_payload, SESSION),
        Err(SnapshotError::Empty)
    );

    // A record for another session epoch.
    let foreign = SessionId::new(99).expect("nonzero");
    let mut record = synthetic_actor_record(
        ActorId {
            session: foreign,
            serial: 4,
        },
        1,
        [0.0, 0.0, 0.0],
    );
    record.actor = ActorId {
        session: foreign,
        serial: 4,
    };
    assert_eq!(
        Snapshot::new(OriginEpoch(1), 0, vec![record]).validate(SESSION),
        Err(SnapshotError::ForeignSession {
            expected: SESSION,
            actor: ActorId {
                session: foreign,
                serial: 4,
            },
        })
    );

    // The reserved serial zero cannot name a live actor.
    let mut reserved = synthetic_actor_record(first, 1, [0.0; 3]);
    reserved.actor = ActorId {
        session: SESSION,
        serial: 0,
    };
    assert_eq!(
        Snapshot::new(OriginEpoch(1), 0, vec![reserved]).validate(SESSION),
        Err(SnapshotError::ReservedSerial {
            actor: ActorId {
                session: SESSION,
                serial: 0,
            },
        })
    );

    // Generation zero is refused as well: "generation not carried" must be
    // distinguishable from a live generation.
    let mut generation_zero = synthetic_actor_record(first, 1, [0.0; 3]);
    generation_zero.generation = 0;
    assert_eq!(
        Snapshot::new(OriginEpoch(1), 0, vec![generation_zero]).validate(SESSION),
        Err(SnapshotError::OutOfRange {
            field: "generation",
            value: 0.0,
            max: f64::from(u16::MAX),
        })
    );

    // The same actor twice in one snapshot has no ordering rule to pick between.
    assert_eq!(
        Snapshot::new(
            OriginEpoch(1),
            0,
            vec![
                synthetic_actor_record(first, 1, [1.0, 0.0, 0.0]),
                synthetic_actor_record(first, 1, [2.0, 0.0, 0.0]),
            ]
        )
        .validate(SESSION),
        Err(SnapshotError::DuplicateActor { actor: first })
    );
    let _ = second;
}

/// A payload that is corrupt, truncated, from another schema version or claims
/// more actors than the cap is refused by name; nothing is read past the end and
/// nothing is guessed.
#[test]
fn accept_f57_a_corrupt_and_foreign_payloads_are_refused_by_name() {
    let [first, second, _] = three_actors();
    let snapshot = synthetic_snapshot(first, second);
    let payload = snapshot.encode(SESSION).expect("the fixture encodes");

    // Another schema version is not this stage's layout.
    let mut version = payload.clone();
    version[0] = SNAPSHOT_SCHEMA_VERSION.wrapping_add(1);
    assert_eq!(
        Snapshot::decode(&version, SESSION),
        Err(SnapshotError::SchemaVersion {
            found: SNAPSHOT_SCHEMA_VERSION.wrapping_add(1),
            expected: SNAPSHOT_SCHEMA_VERSION,
        })
    );

    // Every truncation is a named short read, never a partial parse.
    for cut in 1..payload.len() {
        let Err(error) = Snapshot::decode(&payload[..cut], SESSION) else {
            panic!("a payload cut to {cut} bytes must not decode");
        };
        assert!(
            matches!(
                error,
                SnapshotError::Truncated { .. }
                    | SnapshotError::Empty
                    | SnapshotError::TooManyActors { .. }
            ),
            "truncation to {cut} bytes reported {error}, which is not a short read"
        );
    }

    // Trailing bytes beyond the declared count are refused, not ignored.
    let mut trailing = payload.clone();
    trailing.push(0);
    assert_eq!(
        Snapshot::decode(&trailing, SESSION),
        Err(SnapshotError::TrailingBytes { len: 1 })
    );

    // A count beyond the declared actor cap is refused before any allocation.
    let mut over_cap = payload.clone();
    let cap = u16::try_from(MAX_ACTORS_PER_SNAPSHOT + 1).expect("cap fits u16");
    over_cap[9..11].copy_from_slice(&cap.to_le_bytes());
    assert_eq!(
        Snapshot::decode(&over_cap, SESSION),
        Err(SnapshotError::TooManyActors {
            max: MAX_ACTORS_PER_SNAPSHOT,
            len: MAX_ACTORS_PER_SNAPSHOT + 1,
        })
    );

    // An unknown lifecycle code, integrity above its permille range, and a
    // session field of zero are each refused by field name.
    let lifecycle_offset = SNAPSHOT_HEADER_BYTES + 8 + 8 + 2;
    for (offset, value, field) in [
        (lifecycle_offset, 9_u8, "lifecycle"),
        (lifecycle_offset, 7_u8, "lifecycle"),
        (lifecycle_offset + 1, 5_u8, "control_mode"),
        (
            SNAPSHOT_HEADER_BYTES + 8 + 8 + 2 + 1 + 1 + 12 + ROTATION_BYTES + 6 + 6 + 6,
            0xFF_u8,
            "integrity_permille",
        ),
    ] {
        let mut corrupt = payload.clone();
        corrupt[offset] = value;
        let decoded = Snapshot::decode(&corrupt, SESSION);
        if field == "integrity_permille" {
            assert!(
                matches!(
                    decoded,
                    Err(SnapshotError::OutOfRange {
                        field: "integrity_permille",
                        ..
                    })
                ),
                "a permille value above 1000 must be refused, got {decoded:?}"
            );
        } else {
            assert!(
                matches!(decoded, Err(SnapshotError::UnknownCode { field: got, .. }) if got == field),
                "an unknown {field} code must be refused by name, got {decoded:?}"
            );
        }
    }
    let mut zero_session = payload.clone();
    zero_session[SNAPSHOT_HEADER_BYTES..SNAPSHOT_HEADER_BYTES + 8]
        .copy_from_slice(&0_u64.to_le_bytes());
    assert_eq!(
        Snapshot::decode(&zero_session, SESSION),
        Err(SnapshotError::InvalidSession { found: 0 })
    );
}

/// The declared widths and caps are the schema's own: the measured encoded size
/// equals the declared constant, a full snapshot at the cap fits the F54-A
/// envelope cap, and the payload that travels inside an envelope is exactly
/// what the envelope validates.
#[test]
fn accept_f57_a_encoded_size_matches_the_declared_layout_and_the_envelope_cap() {
    let [first, second, _] = three_actors();
    let snapshot = synthetic_snapshot(first, second);
    let payload = snapshot.encode(SESSION).expect("the fixture encodes");
    assert_eq!(
        payload.len(),
        SNAPSHOT_HEADER_BYTES + 2 * ACTOR_RECORD_BYTES
    );
    assert_eq!(snapshot.encoded_len(), payload.len());
    assert_eq!(ROTATION_BYTES, 7);

    // A snapshot at the actor cap is inside the envelope cap; one record more is
    // refused rather than truncated.
    let mut allocator = ActorAllocator::new(SESSION);
    let mut records = Vec::with_capacity(MAX_ACTORS_PER_SNAPSHOT + 1);
    for index in 0..=MAX_ACTORS_PER_SNAPSHOT {
        let actor = allocator.allocate().expect("serial space");
        records.push(synthetic_actor_record(actor, 1, [index as f64, 0.0, 0.0]));
    }
    let over = Snapshot::new(OriginEpoch(1), 0, records.clone());
    assert_eq!(
        over.validate(SESSION),
        Err(SnapshotError::TooManyActors {
            max: MAX_ACTORS_PER_SNAPSHOT,
            len: MAX_ACTORS_PER_SNAPSHOT + 1,
        })
    );
    records.pop();
    let at_cap = Snapshot::new(OriginEpoch(1), 0, records);
    let payload = at_cap.encode(SESSION).expect("at-cap snapshot encodes");
    assert!(payload.len() <= MAX_SNAPSHOT_BYTES);
    assert_eq!(Snapshot::decode(&payload, SESSION), Ok(at_cap));

    // The bytes that travel are the F54-A envelope's payload, validated by the
    // F54-A envelope: the schema does not define a second, larger packet.
    let frame = snapshot
        .clone()
        .into_frame(SESSION, Tick(31))
        .expect("the fixture wraps");
    let envelope = ServerMessage {
        header: cs_net::message::MessageHeader {
            session: SESSION,
            sequence: 3,
        },
        payload: ServerPayload::Snapshot(frame.clone()),
    };
    assert_eq!(envelope.validate(), Ok(()));
    assert_eq!(
        Snapshot::from_frame(&frame, SESSION),
        Ok(snapshot),
        "the envelope's payload must decode back to the snapshot that produced it"
    );
}

/// A record assembled under a different budget than the field it is assigned to
/// is refused by name, never narrowed: the wire width a field declares is the
/// width its integers have to fit, and a silently truncated step count would
/// travel as a plausible different position or velocity.
#[test]
fn accept_f57_a_a_record_whose_stored_integers_exceed_its_field_width_is_refused() {
    let [first, second, _] = three_actors();
    let mut record = synthetic_actor_record(first, 1, [1.0, 2.0, 3.0]);

    // A velocity field carrying integers quantized for the 32-bit position budget
    // is refused by name, per field, rather than narrowed to 16 bits on the way to
    // the wire.
    record.linear_velocity =
        cs_net::snapshot::QuantizedVector::quantize(POSITION_QUANTIZATION, [1.0e6, 0.0, 0.0])
            .expect("a million meters is inside the declared position range");
    assert_eq!(
        Snapshot::new(OriginEpoch(1), 0, vec![record]).validate(SESSION),
        Err(SnapshotError::OutOfRange {
            field: "linear_velocity",
            value: 1.0e6 * 64.0,
            max: LINEAR_VELOCITY_QUANTIZATION.max_value(),
        })
    );
    record.linear_velocity =
        cs_net::snapshot::QuantizedVector::quantize(LINEAR_VELOCITY_QUANTIZATION, [30.0; 3])
            .expect("30 m/s is inside the declared velocity range");
    record.angular_velocity =
        cs_net::snapshot::QuantizedVector::quantize(POSITION_QUANTIZATION, [-1.0e6, 0.0, 0.0])
            .expect("a million meters is inside the declared position range");
    assert_eq!(
        Snapshot::new(OriginEpoch(1), 0, vec![record]).validate(SESSION),
        Err(SnapshotError::OutOfRange {
            field: "angular_velocity",
            value: -1.0e6 * 64.0,
            max: ANGULAR_VELOCITY_QUANTIZATION.max_value(),
        })
    );

    // A record built the declared way passes, so the check is not vacuous, and
    // the position field itself still carries its full 32-bit range.
    let _ = second;
    record.angular_velocity =
        cs_net::snapshot::QuantizedVector::quantize(ANGULAR_VELOCITY_QUANTIZATION, [1.0; 3])
            .expect("1 rad/s is inside the declared angular velocity range");
    assert_eq!(
        Snapshot::new(OriginEpoch(1), 0, vec![record]).validate(SESSION),
        Ok(())
    );
    record.position = cs_net::snapshot::QuantizedVector::quantize(
        POSITION_QUANTIZATION,
        [POSITION_QUANTIZATION.max_value(), 0.0, 0.0],
    )
    .expect("the declared position maximum is inside the declared position range");
    assert_eq!(
        Snapshot::new(OriginEpoch(1), 0, vec![record]).validate(SESSION),
        Ok(())
    );
}

/// The wire codes are a closed, round-tripping set: every variant encodes to a
/// code this version reads back, and a code outside the set is refused rather
/// than mapped onto a plausible neighbour.
#[test]
fn accept_f57_a_wire_codes_round_trip_and_reject_unknown_values() {
    for (index, lifecycle) in Lifecycle::ALL.iter().enumerate() {
        assert_eq!(
            index as u8,
            lifecycle.code(),
            "wire codes are declared in order"
        );
        assert_eq!(Lifecycle::from_code(lifecycle.code()), Ok(*lifecycle));
    }
    for mode in ControlMode::ALL {
        assert_eq!(ControlMode::from_code(mode.code()), Ok(*mode));
    }
    for bank in Bank::ALL {
        assert_eq!(Bank::from_code(bank.code()), Ok(*bank));
    }
    for code in [3_u8, 4, 200, 255] {
        assert!(Lifecycle::from_code(code).is_err());
        assert!(ControlMode::from_code(code).is_err());
        assert!(Bank::from_code(code).is_err());
    }
    // The stable labels exist for reports and are distinct per variant.
    let labels: std::collections::BTreeSet<&str> =
        Lifecycle::ALL.iter().map(|kind| kind.label()).collect();
    assert_eq!(labels.len(), Lifecycle::ALL.len());
}

/// The synthetic fixture is marked synthetic development content and is
/// self-consistent: it validates, encodes, decodes and carries two distinct
/// actors, both at generation 1.
#[test]
fn accept_f57_a_synthetic_fixture_is_self_consistent() {
    let [first, second, third] = three_actors();
    let snapshot = synthetic_snapshot(first, second);
    assert_eq!(snapshot.validate(SESSION), Ok(()));
    assert_eq!(snapshot.actors.len(), 2);
    assert_ne!(first, second);
    assert!(third != first && third != second);
    assert_eq!(snapshot.origin, OriginEpoch(1));
    assert_eq!(snapshot.input_ack, 7);
    for actor in [first, second] {
        let record = snapshot
            .actor(actor)
            .expect("the fixture carries the actor");
        assert_eq!(record.generation, 1);
        assert_eq!(record.lifecycle, Lifecycle::Alive);
        assert_eq!(record.control, ControlMode::Manual);
        assert!(record.weapons.primary_rounds > 0);
    }
    let payload = snapshot.encode(SESSION).expect("the fixture encodes");
    assert_eq!(Snapshot::decode(&payload, SESSION), Ok(snapshot));
}
