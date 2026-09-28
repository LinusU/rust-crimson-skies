# Rally task #335: what `--seed` could select for the headless synthetic run

Date: 2026-09-23. Task: #335 "Define what --seed selects for the headless
synthetic run before accepting it" (contract `docs/contracts/CLI-EVIDENCE.md`,
spec `specs/F00-workspace-toolchain-and-first-executable.md`). Capabilities
used: ordinary build/test only; no original data involved.

## Status: ratified by the owner and implemented (2026-09-28)

The owner ruled on 2026-09-25 (`docs/contracts/CLI-EVIDENCE.md`, section
"`--seed`", main 9e7a2a8): the proposal below is **approved with
amendments**, and the contract is now the specification the implementation
follows. Where the two differ, the contract wins:

1. `--seed` is the run's **root seed**; each consumer draws a
   domain-separated SplitMix64 stream — the stream seed is the SplitMix64
   output of `root_seed ^ DOMAIN`, with one documented `u64` domain constant
   per consumer. The synthetic body is the first domain
   (`cs_types::random::SYNTHETIC_BODY_DOMAIN`), so later consumers can be
   added without shifting this stream.
2. Unit floats use `(x >> 11) as f64 * 2^-53`
   (`cs_types::random::unit_f64`), which is exact integer-to-float
   arithmetic and therefore bit-stable on every platform. The proposal left
   the float conversion unspecified.
3. `--seed` without `--synthetic` is `Invalid` for now: only the synthetic
   headless scene consumes a root seed until another stage specifies its
   consumer.

Everything else in the proposal stands: an optional flag, `None` = the
unchanged canonical fixture, lateral bounds of ±2 m and ±4 m/s, `"seed"` in
the trace header (including `"seed": null` for an unseeded run), the
`--ticks`-style CLI errors and the test plan. The implementation and its
`accept_t335_*` tests landed on this task's branch on 2026-09-28; the
sections below are kept as the record of the gap and the design reasoning
that produced the ruling.

## Status at the time of writing: blocked on the owner decision this task exists to collect

The task's own acceptance requires the semantics to be "written down in a
spec/contract update by the owner", and `specs/` plus `docs/contracts/` are
protected paths an agent branch cannot touch. This finding records the
evidence and one concrete proposal the owner can ratify, amend or replace.
Nothing here is implemented: `--seed` still exits 2.

## Evidence for the gap

- `docs/contracts/CLI-EVIDENCE.md` shows
  `cs --synthetic --headless --ticks 600 --seed 1 --trace <file>`, but no
  text in `specs/`, `missions/` or `docs/` defines what the seed selects.
  A repository-wide search finds only unrelated uses: the fixed-clock
  baseline seed in `cs_app::synthetic`, fuzzing seeds in F03/F62, and
  future weather/AI/difficulty seeds in F19/F26/F31/F49.
- `cs_app::cli::parse` therefore classifies `--seed` as an unknown argument
  → `CliRequest::Unsupported` → exit 2. That is deliberate: a flag that
  reads a value and changes nothing is a no-op stub, which the contract
  forbids ("never return zero after only logging a failure"; tests must not
  assert success from stubbed behavior).
- The scene the flag would steer is already deterministic end to end:
  `SyntheticBodySpec::falling_box` is authored constants, `TICK_HZ` makes
  the manual frame delta and the fixed timestep the identical `Duration`,
  and the real clock's baseline is seeded at startup
  (`docs/findings/2026-09-23-t334-first-frame-fixed-step.md`), so today a
  trace is a pure function of `ticks` alone.

## Design space

Within this stage the seed can only honestly vary the synthetic body's
initial conditions — `position_m`, `linear_velocity_m_s`, `half_extents_m`,
`kind` in `cs_types::SyntheticBodySpec`. Weather, AI and traffic are named
as future consumers but do not exist yet, so their seed streams are out of
scope for a decision that must be observable now. Varying the drop height or
the extents would change the experiment itself; varying lateral position and
horizontal velocity keeps "one box falling under gravity" while making the
seed observable in the trace from tick 0.

## Proposal for owner ratification

1. `--seed <u64>` is **optional** on the synthetic headless request. Typed
   input: `SyntheticRequest.seed: Option<u64>`; `None` keeps today's
   canonical fixture so existing traces stay byte-identical. A required or
   defaulted seed would either silently change the default trace or need a
   seed value that maps to the canonical fixture — a hash-style generator
   cannot promise that.
2. `cs_types` gains a pure, total, documented generator — e.g.
   `SyntheticBodySpec::seeded(self, seed: u64) -> Self` built on SplitMix64 —
   mapping the seed to bounded lateral offsets, e.g.
   `position_m[0]/[2] += u ∈ [-2, +2] m` and
   `linear_velocity_m_s[0]/[2] += u ∈ [-4, +4] m/s`. Drop height, vertical
   velocity, extents and kind stay at the fixture values, and the output
   always passes `SyntheticBodySpec::validate`. SplitMix64 is ~15 lines of
   integer arithmetic, dependency-free (cs_types must stay so) and bit-stable
   across platforms; later subsystems should draw domain-separated streams
   from the same root seed rather than sharing this sequence.
3. `run_synthetic` seeds the spec when `seed` is `Some`, and the trace
   header gains `"seed":<u64>` or `null`, so a trace proves which seed — if
   any — produced it.
4. CLI behavior mirrors `--ticks`: a missing value is `Unsupported`
   (structurally incomplete), a non-u64 value is `Invalid` naming `--seed`,
   a duplicate flag's later value wins for consistency, `--seed` without
   `--synthetic` is `Invalid`, and `--help` documents the flag.
5. Tests under the implementing stage's prefix: `Some(1)` parses; `--seed
   abc` and non-synthetic `--seed` are `Invalid`; the same seed twice yields
   identical trace bytes; two different seeds yield a different tick-0
   sample and a diverging trace — so an implementation that parses but
   ignores the seed fails; the header carries the seed.

## Decision points the owner must rule on

- Optional flag vs. required/default seed — and if a default, what the
  default run produces relative to today's canonical fixture.
- Which fields the seed perturbs and within which bounds (the concrete
  numbers above are a proposal, not original data — none exists to match).
- Generator choice: SplitMix64 proposed; a vetted rand-style crate would
  add a dependency to `cs_types`, which is deliberately dependency-free.
- Whether `--seed` is documented as synthetic-only for now or reserved
  globally (input replays will need a seed record later).

## What stays unclaimed

No original-game RNG semantics are asserted anywhere here: the synthetic
scene is authored development content and the seed is an input to our own
tooling. Any relationship to retail randomness is a future research
question, not this task.
