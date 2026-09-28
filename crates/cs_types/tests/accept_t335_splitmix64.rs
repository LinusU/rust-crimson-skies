//! Acceptance tests for F00-SEED: the dependency-free SplitMix64 generator,
//! its domain separation and `SyntheticBodySpec::seeded` in `cs_types`.
//!
//! The generator is checked against **published reference values** — the first
//! five outputs for seed `1234567` and the 100 000-draw histogram for seed
//! `987654321` that Rosetta Code's *Pseudo-random numbers/Splitmix64* task
//! records for the public-domain C reference implementation of Sebastiano
//! Vigna (<https://prng.di.unimi.it/splitmix64.c>). Those vectors are
//! independent of this repository: a generator that "looks random" but is not
//! SplitMix64 fails here, and so does one with a wrong increment, a wrong
//! finaliser or a reordered step.
//!
//! The seeded fixture is checked for what the contract
//! (`docs/contracts/CLI-EVIDENCE.md`, section "`--seed`") allows it to touch:
//! the two lateral position and two lateral velocity components, nothing else.

use cs_types::random::{SYNTHETIC_BODY_DOMAIN, SplitMix64, unit_f64};
use cs_types::{BodyKind, SyntheticBodySpec};

/// The canonical fixture a seeded spec is derived from.
fn canonical() -> SyntheticBodySpec {
    SyntheticBodySpec::falling_box(BodyKind::Dynamic)
}

/// Published reference: the first five SplitMix64 outputs of seed `1234567`,
/// recorded on Rosetta Code's *Pseudo-random numbers/Splitmix64* task for the
/// C reference implementation (and reproduced by the Ada, C++, Go, Haskell,
/// Java, Julia, Perl, Python and Rust entries there).
#[test]
fn accept_t335_splitmix64_matches_published_reference_values() {
    let mut generator = SplitMix64::new(1_234_567);
    let expected = [
        6_457_827_717_110_365_317,
        3_203_168_211_198_807_973,
        9_817_491_932_198_370_423,
        4_593_380_528_125_082_431,
        16_408_922_859_458_223_821,
    ];

    for (index, want) in expected.into_iter().enumerate() {
        let got = generator.next_u64();
        assert_eq!(
            got, want,
            "draw {index} of SplitMix64 seeded with 1234567 must match the \
             published reference value"
        );
    }

    // The second published vector: 100 000 draws of `next_int() / 2^64`
    // bucketed by `floor(value * 5)` must reproduce the reference histogram
    // exactly (0: 20027, 1: 19892, 2: 20073, 3: 19978, 4: 20030).
    let mut generator = SplitMix64::new(987_654_321);
    let mut counts = [0usize; 5];
    for _ in 0..100_000 {
        let value = (generator.next_u64() as f64) / 18_446_744_073_709_551_616.0;
        let bucket = (value * 5.0) as usize;
        counts[bucket] += 1;
    }
    assert_eq!(
        counts,
        [20_027, 19_892, 20_073, 19_978, 20_030],
        "the published SplitMix64 histogram for seed 987654321 must be \
         reproduced draw for draw"
    );
}

/// The contract's exact unit-float conversion: `(x >> 11) as f64 * 2^-53`.
/// These are the values the arithmetic pins down, so an implementation that
/// divides by `2^64`, by `2^64 - 1` or that keeps the low 11 bits fails.
#[test]
fn accept_t335_unit_float_conversion_is_the_contract_formula() {
    assert_eq!(unit_f64(0), 0.0, "zero draws to zero");
    assert_eq!(
        unit_f64(1 << 11),
        2.0f64.powi(-53),
        "one unit is exactly 2^-53"
    );
    assert_eq!(
        unit_f64(u64::MAX),
        1.0 - 2.0f64.powi(-53),
        "the largest draw stays below one, at exactly 1 - 2^-53"
    );

    // The low 11 bits carry no value: draws that differ only below bit 53
    // must convert to the same float.
    assert_eq!(unit_f64(0b1011), unit_f64(0));

    // Monotonic in the draw: a larger integer never yields a smaller unit.
    let mut previous = -1.0f64;
    for draw in (0..64).map(|index| index * (u64::MAX / 63)) {
        let unit = unit_f64(draw);
        assert!(
            unit >= previous,
            "unit floats must be monotonic in the draw, {draw} broke it"
        );
        assert!((0.0..1.0).contains(&unit));
        previous = unit;
    }
}

/// Domain separation: one root seed feeds independent streams, and adding a
/// consumer (a different domain constant) can never shift the stream the
/// synthetic body already draws from.
#[test]
fn accept_t335_domain_separation_keeps_streams_independent() {
    let root = 0x0123_4567_89AB_CDEFu64;
    const OTHER_DOMAIN: u64 = 0x4F54_4845_525F_444F; // "OTHER_DO"

    let body_a = {
        let mut stream = SplitMix64::for_domain(root, SYNTHETIC_BODY_DOMAIN);
        [stream.next_u64(), stream.next_u64(), stream.next_u64()]
    };
    let body_b = {
        let mut stream = SplitMix64::for_domain(root, SYNTHETIC_BODY_DOMAIN);
        [stream.next_u64(), stream.next_u64(), stream.next_u64()]
    };
    assert_eq!(
        body_a, body_b,
        "the same root seed and domain must reproduce the same stream"
    );

    let other = {
        let mut stream = SplitMix64::for_domain(root, OTHER_DOMAIN);
        [stream.next_u64(), stream.next_u64(), stream.next_u64()]
    };
    assert_ne!(
        body_a, other,
        "a different domain constant must produce a different stream"
    );

    // A neighbouring root seed must not collide with this one either.
    let neighbour = {
        let mut stream = SplitMix64::for_domain(root ^ 1, SYNTHETIC_BODY_DOMAIN);
        [stream.next_u64(), stream.next_u64(), stream.next_u64()]
    };
    assert_ne!(
        body_a, neighbour,
        "different root seeds must produce different streams"
    );
}

/// `SyntheticBodySpec::seeded` moves the lateral position by at most 2 m and
/// the lateral velocity by at most 4 m/s, leaves every other field at the
/// fixture value, and always produces a spec that passes `validate`.
#[test]
fn accept_t335_seeded_spec_varies_only_the_lateral_components() {
    let base = canonical();

    for root_seed in [0u64, 1, 2, 42, 987_654_321, u64::MAX] {
        let seeded = base.seeded(root_seed);
        assert_eq!(
            seeded.validate(),
            Ok(()),
            "seed {root_seed}: a seeded spec must always validate"
        );
        assert_eq!(
            seeded.kind, base.kind,
            "seed {root_seed}: the body kind must not move"
        );
        assert_eq!(
            seeded.position_m[1], base.position_m[1],
            "seed {root_seed}: the drop height must not move"
        );
        assert_eq!(
            seeded.linear_velocity_m_s[1], base.linear_velocity_m_s[1],
            "seed {root_seed}: the vertical velocity must not move"
        );
        assert_eq!(
            seeded.half_extents_m, base.half_extents_m,
            "seed {root_seed}: the extents must not move"
        );

        for axis in [0usize, 2] {
            let offset = seeded.position_m[axis] - base.position_m[axis];
            assert!(
                (-2.0..=2.0).contains(&offset),
                "seed {root_seed}: position_m[{axis}] offset {offset} must be \
                 within +/- 2 m"
            );
            let offset = seeded.linear_velocity_m_s[axis] - base.linear_velocity_m_s[axis];
            assert!(
                (-4.0..=4.0).contains(&offset),
                "seed {root_seed}: linear_velocity_m_s[{axis}] offset {offset} \
                 must be within +/- 4 m/s"
            );
        }
    }
}

/// Different root seeds must move the body differently, and a seed must never
/// reproduce the canonical fixture by accident more often than chance allows:
/// an implementation that accepts the seed and then ignores it fails here.
#[test]
fn accept_t335_seeded_specs_differ_between_root_seeds() {
    let base = canonical();
    let mut seen = vec![base];
    for root_seed in 0..64u64 {
        let seeded = base.seeded(root_seed);
        assert_ne!(
            seeded, base,
            "seed {root_seed} must move the body away from the canonical fixture"
        );
        assert!(
            !seen.contains(&seeded),
            "seed {root_seed} must not collide with an earlier seeded spec"
        );
        seen.push(seeded);
    }
}
