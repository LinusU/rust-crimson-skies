# F16-F-CAP: the fixed-step accumulator adopts the original's 0.125 s frame-dt cap

Date: 2026-10-14. Task: **#721** "Decide whether the fixed-step accumulator
needs the original 0.125 s frame-dt cap" (`F16-F-CAP`, filed by F16-F /
#391 — see
`docs/findings/2026-10-06-f16-f-original-clock-pause-and-speed-up-policy.md`).
Feature sheet:
`specs/F16-coordinates-units-origin-management-and-clocks.md`. Shared
contract: `docs/contracts/FLIGHT-PHYSICS.md`. Required capability: ordinary
build/test — this stage reads no original data at run time, so no
`private/evidence/` report is produced.

## The decision

**Adopted.** The project now clamps one rendered frame's wall-time delta to
the measured original bound:

- The value is **`MAX_FRAME_DT = Duration::from_millis(125)`** (125 ms,
  125 000 000 ns), declared once in `crates/cs_sim/src/time.rs` and shared
  with `OriginalClockPolicy::measured_original().max_frame_dt`.
- The enforcement point is **`SimClock::advance`**: the clamped `elapsed`
  enters the nanosecond accumulator, the cap's worth of ticks commits, and
  **the excess is dropped, never banked** — nothing is deferred to a later
  frame, and the sub-tick carry is the only state that persists.
- Every clock domain inherits the bound because every producer path —
  `FixedTickDriver`, `GameplayTimeline`, `VisibilityTimeline`, the input
  pump and replay clocks — delivers one frame's delta through
  `SimClock::advance`. No per-consumer cap exists or is needed.
- Ordering: a paused `PausePolicy::Freeze` clock still returns zero
  *before* the clamp is reached, so pausing can never interact with the
  cap; the clamp applies to the delivered delta, after which the existing
  overflow check is unchanged and atomic (a refused advance mutates
  nothing).

## What the original does, and why it settles the question

From the static analysis F16-F recorded (owner-supplied decrypted
executable, sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`):

- The original frame clock at **`0x59c0c0`** produces a scaled `game_dt`
  per frame and clamps it to `[0, 0.125 s]` **after** scaling.
- Clamp enable flag **`0x63b160`**, default 1; clamp maximum
  **`0x63b158`** = 125 ms; clamp minimum **`0x63b15c`** = 0.

Consequence: in the original, a stalled frame advances at most 125 ms of
game time. The project's previous behaviour — `SimClock::advance` accepted
any `Duration` and committed all of its whole ticks — let one stalled frame
advance unbounded game time, so a hitch at 1 fps would simulate a full
second while the original simulates an eighth of it. That is a behavioural
divergence on a path real runs hit (any frame slower than 8 fps), not a
cosmetic one, and the remedy is one line at the point where all producers
already converge. Adoption costs nothing the project wanted: under F16
non-negotiable 3 the tick is still integer and fixed-dt, and under AC03 the
same wall time still produces the same ticks at every delivery rate whose
frames fit under the cap.

## What changes, concretely

- `clock.frame-dt-cap` now reports **`agrees`** in
  `OriginalClockPolicy::compare_project_clocks` (previously `diverges`);
  the comparison counts move to 7 agrees / 3 diverges / 4 not-modeled, and
  `accept_f16_f_comparison_pins_the_measured_policies_against_the_project_clocks`
  pins the new split.
- The distinction the tests draw: **a single stalled frame** commits the
  cap's ticks (8 at 64 Hz) and banks nothing, while **a long span delivered
  as ordinary frames** — each at or under 125 ms — still converts all of
  its wall time. A render loop that stays responsive keeps exact
  accumulation; only a frame that genuinely stalls loses time, exactly as
  the original does.
- Behaviour below the cap threshold is pinned rather than smoothed over: at
  1 fps a 1 s frame commits 8 ticks; at 7 fps seven 142 857 142 ns frames
  each clamp to the cap and the remainder frame adds nothing, so a second
  commits 56 ticks.
- Existing `accept_f16_a_`, `accept_f16_d_` and `accept_f16_f_` tests were
  **extended, not weakened**: assertions that meant "deliver N seconds of
  wall time" now deliver it as consecutive frames at or under the cap (the
  way a real frame loop delivers it) and keep their original expectations;
  new assertions pin the capped single-frame counts, the dropped-excess
  semantics, and the driver-level bound
  (`accept_f16_f_cap_a_stalled_frame_advances_exactly_the_cap`,
  `accept_f16_f_cap_a_frame_within_the_cap_is_untouched`,
  `accept_f16_c_stalled_frame_commits_at_most_the_cap`, plus the capped
  halves of the frame-split and pause-probe tests).

## What this does not decide

- The clamp's **minimum** (`0x63b15c` = 0) needs no enforcement: a
  `Duration` cannot be negative, so the lower bound is structural.
- **#722 (`F16-F-SPEEDUP`)** stays open: how single-player speed-up's extra
  fixed ticks reach the gameplay timers is a separate decision. The cap
  constrains one frame's *input*; it says nothing about `advance_fixed_ticks`.
- Claim status stays **`inferred`** (`ORIGINAL_CLOCK_POLICY_STATUS`): this
  is static code evidence, not a runtime observation, and nothing here
  self-awards `verified_original`. What only a run could give — measured
  frame pacing and the real distribution of frame deltas — remains
  unmeasured.

## Alternatives considered

- **Record the divergence as accepted.** Rejected: the divergence is not a
  designed policy choice like the fixed dt or the fixed-tick speed-up
  mechanism (those are F16 non-negotiables); it is an unowned gap between
  "the original bounds catch-up per frame" and "we bound nothing", and it
  is reachable by any machine that drops below 8 fps for one frame.
- **Cap in the session driver or each consumer** instead of `SimClock`.
  Rejected: the original clamps the *frame clock's* dt before anyone
  consumes it, so the bound belongs at the accumulator, one implementation
  shared by every domain; five consumer-side caps would risk drifting
  apart and would leave direct `SimClock` callers (input pump, replay)
  uncapped.
- **Bank the excess** into later frames. Rejected: the original drops it —
  the clamp is a per-frame bound on *converted* time, not a deferred debt —
  and banking would resurrect the unbounded catch-up burst one frame later.

## Provenance

Addresses and behaviour above are the static-analysis record carried over
from the F16-F entry; the image itself, disassembly and decompiled code are
never committed. See the F16-F entry's provenance section for the image
sha256, the owner notes behind the analysis and the `code-derived →
inferred` claim rule this entry inherits.
