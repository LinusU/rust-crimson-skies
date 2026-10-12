//! T583 acceptance (task `CS-TYPES-RNG-STATE`): `SplitMix64::state` and
//! `SplitMix64::draws_since`.
//!
//! A save record stores the stream's state, so `SplitMix64::new(g.state())`
//! must resume the same stream bit-for-bit — that is what lets a record carry
//! one `u64` instead of a draw count to replay. `draws_since` recovers the
//! draw index of a state relative to a start state exactly, which is what a
//! restore checks instead of trusting the stored state.

use cs_types::random::SplitMix64;

/// `SplitMix64::new(s.state())` continues the same stream bit-for-bit:
/// a stream saved after `before` draws and resumed from its stored state
/// produces the identical later draws.
#[test]
fn accept_t583_new_of_state_continues_the_same_stream_bit_for_bit() {
    for seed in [0, 1, 0x5EED_5EED_5EED_5EED, u64::MAX] {
        for before in [0u64, 1, 7, 64, 1024] {
            let mut stream = SplitMix64::new(seed);
            for _ in 0..before {
                stream.next_u64();
            }
            let mut resumed = SplitMix64::new(stream.state());
            for draw in 0..5 {
                assert_eq!(
                    resumed.next_u64(),
                    stream.next_u64(),
                    "seed {seed:#x}, resumed after {before} draws: draw {draw} diverged"
                );
            }
        }
    }

    // The same through a domain-separated stream, which is how a save record
    // reaches the state: for_domain's output is the stored start.
    let start = SplitMix64::for_domain(0xC510_0000_0000_0001, 0x4D53_4E5F_4556_414C);
    let mut stream = start;
    for _ in 0..3 {
        stream.next_u64();
    }
    let mut resumed = SplitMix64::new(stream.state());
    for _ in 0..5 {
        assert_eq!(resumed.next_u64(), stream.next_u64());
    }
}

/// `draws_since` recovers the exact index a state holds on the stream: the
/// state after `k` draws is at index `k`, and the inverse is total — a state
/// behind the start is one draw *shy* of a full cycle, index `u64::MAX`.
#[test]
fn accept_t583_draws_since_recovers_the_exact_draw_index() {
    for seed in [0, 0xDEAD_BEEF_CAFE_F00D, u64::MAX] {
        let start = SplitMix64::new(seed);
        let mut stream = start;
        assert_eq!(stream.draws_since(start), 0, "the start is index zero");
        for k in 1..=1000u64 {
            stream.next_u64();
            assert_eq!(
                stream.draws_since(start),
                k,
                "seed {seed:#x}: state after {k} draws must index {k}"
            );
        }
        // The index is exact mod 2^64 in both directions: 1000 draws in,
        // `start` is the state a full cycle minus 1000 draws reaches — index
        // `u64::MAX - 999` — and one draw in it is index `u64::MAX`. The map
        // is a bijection on u64, so a caller checking "is this state on my
        // stream's horizon" must bound the index itself.
        assert_eq!(start.draws_since(stream), u64::MAX - 999);
        let mut one = start;
        one.next_u64();
        assert_eq!(start.draws_since(one), u64::MAX);
    }
}
