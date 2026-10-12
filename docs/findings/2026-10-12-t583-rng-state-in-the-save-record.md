# T583: the save record carries the RNG stream's own state

Date: 2026-10-12. Task #583 (`CS-TYPES-RNG-STATE`, follow-up of the F37-C
review). Test prefix `accept_t583_` in
`crates/cs_types/tests/accept_t583_splitmix64_state.rs` and
`crates/cs_script/tests/accept_t583_rng_state_record.rs`. Capability:
ordinary build/test only. No `verified_original` claim — the record and its
runtime are an engine design, not measured original behavior.

## What changed

- `cs_types::random::SplitMix64` gained `pub const fn state(&self) -> u64`
  and `pub const fn draws_since(&self, start: Self) -> u64`, plus a private
  `INCREMENT_INVERSE` derived by Newton–Raphson at compile time.
- `MissionStateSnapshot` now stores `rng_state` (the stream's own `u64`
  state) instead of `rng_draws`. `MissionState` no longer counts draws, the
  restore replay loop is gone, and restore is O(1).
- `MAX_RNG_REPLAY_DRAWS` → `MAX_RNG_STREAM_DRAWS` (same `1 << 22`) and
  `RestoreError::RngReplayTooLong` → `RestoreError::RngStateOutOfDomain
  { state, draws }`.
- `SNAPSHOT_VERSION` is 3: a version-2 record's field was a draw count, not
  a state, so it is refused rather than reinterpreted. `cs_sim`'s session and
  host records share the constant and bump with it.
- `MISSION_EVALUATOR_DOMAIN` is now `pub` so record-domain checks are
  observable in tests, matching the other domain constants.

## Why the restore is still checked, not trusted

`next_u64` adds `INCREMENT` to the state per draw, so the states a stream
visits from seed `s0` are exactly `s0 + k·INCREMENT` for `k = 0, 1, …`.
The increment is odd, hence a unit modulo 2^64, and the draw index a stored
state implies is recovered exactly and in constant time:

```text
k = (rng_state - s0) * INCREMENT_INVERSE   (mod 2^64)
```

That map is a bijection on `u64`, so every `u64` is *some* index — "on the
orbit" cannot be the check. The check is the bound: a state is in the
record's domain exactly when its implied index is at most
`MAX_RNG_STREAM_DRAWS`. For a honestly written record this is the same
refusal boundary as before (a session past the bound cannot restore,
exactly as a `rng_draws` past it could not). For a forged or foreign state —
another session's stream, another domain's — the implied index is
effectively uniform, so it is refused with probability ≈ `1 - 2^-42`.

Rejected alternative: storing `rng_state` *and* `rng_draws` and checking
them for consistency adds no refusal power — the fields are bijective, so a
forger writes the matching count — and keeps a redundant field in the record
forever.

## Acceptance mapping

- `SplitMix64::new(s.state())` continues the same stream bit-for-bit:
  `accept_t583_new_of_state_continues_the_same_stream_bit_for_bit` (several
  seeds and split points, plus a domain-separated stream);
  `accept_t583_draws_since_recovers_the_exact_draw_index` pins the inverse
  in both directions.
- Save/restore reproduces every later draw without a replay loop:
  `accept_f37_c_restore_continues_the_explicit_rng_stream` (updated to pin
  `record.rng_state ==` the stream's state after the draws taken) and
  `accept_t583_save_record_stores_and_continues_the_stream_state`; the F37-D
  corpus replay test still fails if the record forgets the stream position.
- An out-of-domain state is refused:
  `accept_f37_c_restore_refuses_a_foreign_or_incompatible_record` (bound,
  bound+1 and another session's state) and
  `accept_t583_restore_refuses_an_out_of_domain_rng_state`;
  `accept_t583_version_two_records_are_refused` pins the format bump.

## Not done here

The evaluator is still seeded from the session generation rather than the
run root seed — that is #581 (`F37-C-FU1`), deliberately untouched.
