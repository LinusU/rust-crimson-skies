//! Deterministic randomness for run seeds (F00-SEED, contract
//! `docs/contracts/CLI-EVIDENCE.md`, section "`--seed`").
//!
//! Everything here is integer arithmetic in `std` only: `cs_types` stays
//! dependency-free by contract (`docs/01-ARCHITECTURE.md`), and the values a
//! root seed produces must be bit-identical on every platform, so the
//! conversion to a unit float is written as an exact integer-to-float scale
//! rather than as a division by a random-looking literal.

/// The SplitMix64 generator of Sebastiano Vigna's public-domain reference
/// implementation (<https://prng.di.unimi.it/splitmix64.c>), a fixed-increment
/// version of Java 8's `SplittableRandom` step.
///
/// The state is a single `u64` advanced by the golden-ratio increment and
/// finalised by two xor-shift/multiply rounds; every operation is a wrapping
/// integer operation, so the sequence depends only on the seed, never on the
/// platform, the optimisation level or a floating-point environment.
///
/// A [`SplitMix64`] is *not* shared between consumers: each consumer derives
/// its own stream with [`SplitMix64::for_domain`], so adding a consumer never
/// shifts the values another consumer already observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Golden-ratio increment `0x9e3779b97f4a7c15` of the reference
    /// implementation.
    const INCREMENT: u64 = 0x9E37_79B9_7F4A_7C15;
    /// First finaliser constant `0xbf58476d1ce4e5b9`.
    const FINALISER_1: u64 = 0xBF58_476D_1CE4_E5B9;
    /// Second finaliser constant `0x94d049bb133111eb`.
    const FINALISER_2: u64 = 0x94D0_49BB_1331_11EB;

    /// The multiplicative inverse of [`Self::INCREMENT`] modulo `2^64`: for
    /// every `x`, `x * INCREMENT * INCREMENT_INVERSE == x`. It exists because
    /// the increment is odd, and it is what [`Self::draws_since`] uses to
    /// recover a draw index in one multiplication instead of a replay.
    ///
    /// Derived by Newton–Raphson at compile time — `x ← x·(2 − a·x)` doubles
    /// the number of correct low bits each round, and an odd seed is already
    /// exact in 3 bits, so six rounds cover all 64.
    const INCREMENT_INVERSE: u64 = {
        let mut x = Self::INCREMENT;
        let mut round = 0;
        while round < 6 {
            x = x.wrapping_mul(2u64.wrapping_sub(Self::INCREMENT.wrapping_mul(x)));
            round += 1;
        }
        x
    };

    /// Creates a generator whose state is exactly `state`.
    ///
    /// The next draw is the SplitMix64 output of `state` advanced once, which
    /// is what [`SplitMix64::for_domain`] uses to build a domain-separated
    /// stream.
    pub const fn new(state: u64) -> Self {
        Self { state }
    }

    /// The generator's state.
    ///
    /// The state is everything a draw depends on — the output is a pure
    /// function of it — so `SplitMix64::new(g.state())` resumes the stream
    /// exactly where `g` left it. A save record can therefore store one
    /// `u64` instead of a draw count to replay.
    pub const fn state(&self) -> u64 {
        self.state
    }

    /// The draw index of `self` relative to `start`: the count `k` such that
    /// `self` is the state the stream holds `k` draws after `start`.
    ///
    /// [`Self::next_u64`] adds [`Self::INCREMENT`] to the state on every draw,
    /// so the states a stream visits from `start` are exactly
    /// `start.state + k * INCREMENT` for `k = 0, 1, …`, and multiplying the
    /// difference by [`Self::INCREMENT_INVERSE`] recovers `k` exactly. The map
    /// is a bijection on `u64`, so *every* pair of states has a defined
    /// index — a state that is not actually `k` draws ahead of `start` still
    /// returns the `k` it would be at. A caller that needs "did this stream
    /// produce that state" must bound the returned count itself.
    pub const fn draws_since(&self, start: Self) -> u64 {
        self.state
            .wrapping_sub(start.state)
            .wrapping_mul(Self::INCREMENT_INVERSE)
    }

    /// The stream a consumer draws from when the run's root seed is
    /// `root_seed` and the consumer's domain constant is `domain`.
    ///
    /// The contract fixes the recipe: the stream seed is the SplitMix64 output
    /// of `root_seed ^ DOMAIN`, and that output seeds the generator the
    /// consumer actually draws from. Two different domains therefore produce
    /// two independent streams from one root seed, and a new consumer can be
    /// added later without moving any value an existing consumer already
    /// produced.
    pub fn for_domain(root_seed: u64, domain: u64) -> Self {
        Self::new(Self::new(root_seed ^ domain).next_u64())
    }

    /// The next 64 bits of the stream.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(Self::INCREMENT);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(Self::FINALISER_1);
        value = (value ^ (value >> 27)).wrapping_mul(Self::FINALISER_2);
        value ^ (value >> 31)
    }

    /// The next draw as a `f64` in `[0, 1)`; see [`unit_f64`].
    pub fn unit_f64(&mut self) -> f64 {
        unit_f64(self.next_u64())
    }
}

/// Converts one 64-bit draw to a `f64` in `[0, 1)` exactly as the contract
/// fixes: `(x >> 11) as f64 * 2^-53`.
///
/// The result of `x >> 11` is an integer below `2^53`, so scaling it by the
/// power of two `2^-53` is exact integer-to-float arithmetic: the value is the
/// nearest double to `floor(x / 2^11) / 2^53` on every platform, in particular
/// `0.0` for `0` and `1.0 - 2^-53` for `u64::MAX`. The top 53 bits carry the
/// value, so no low-order rounding differences can creep in either.
pub fn unit_f64(draw: u64) -> f64 {
    /// `2^-53`, the exact double the contract scales the 53-bit fraction by.
    const TWO_POW_MINUS_53: f64 = 1.0 / 9_007_199_254_740_992.0;
    ((draw >> 11) as f64) * TWO_POW_MINUS_53
}

/// The domain constant of the synthetic body stream: the first and, for now,
/// only consumer of a run's root seed.
///
/// Chosen once and documented here; it is an arbitrary but fixed `u64`
/// (the big-endian ASCII bytes `"SYNTH_BO"`), not a tuning value and not
/// original-game data. Every future consumer of `--seed` adds its own
/// documented constant instead of reusing this one, so its values can never
/// move under the synthetic body stream.
pub const SYNTHETIC_BODY_DOMAIN: u64 = 0x5359_4E54_485F_424F;
