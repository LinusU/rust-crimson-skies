# M01 wingman stick law ported; Betty does not hold station with it (#2263)

Date: 2026-11-02. Task #2263, follow-up of #2262
(`2026-11-02-m01-wingman-hillside-divergence.md`). Implementer: `sonnet-1`;
not independently reviewed. Static claims are `static`, read from the owner
image with `llvm-objdump -d -M intel`; the original executable was not run and
nothing is `verified_original`. Flight numbers are port behaviour only.

## What is ported

`cs_app::mission_ai::wingman_stick` is `0x41b560`'s stick half
(`0x41bc08`..`0x41be47`), both `bool` arguments clear, as decoded in
`2026-10-18-m01-wingman-player-leader.md` §4 and re-read here against the
bytes (the branch conditions, the `by` sign flip of the fine branch, and the
exact wings-level rule, which §4 only summarised):

* wings-level (`0x41bd98`..): `A100 = 0.2 * (row1.x < 0 ? (row0.y < 0 ? +1 : -1) : -row0.y)`
  with `row0.y = [V+0x184]`, `row1.x = [V+0x190]` (the vehicle's own basis rows
  at `V+0x180/0x18c/0x198`; the same rows project the goal delta into
  `(bx, by, bz)`).
* The words become `FlightInput` as `roll = -A100`, `pitch = A108`,
  `yaw = A10c` (`StickWords::flight_input`), per the measured force-block reads
  of `+0x114/+0x11c/+0x120` (roll, pitch, yaw; #2262). The **sign** of each is
  a designed reading: with own rows read as right/up/nose, the law is only
  self-consistent (target above-right gets `A100 = -bx`; the wings-level
  assist pushes `-row0.y`; a low right wing has `row0.y < 0`) if a positive
  `+0x100` is a roll to the left, so the port negates it. Pitch and yaw pass
  through.

It is selected per aircraft by `MissionAircraftAi::original_stick`, **off by
default**, and only for the engaged station-flying branch (the unengaged
heading hold keeps `pursuit_command`).

## Not ported, and why (all stay unknown)

| unknown | effect |
| --- | --- |
| pilot gains `V+0x8c4/0x8c8/0x8cc` and limits `V+0x8d0/0x8d4/0x8d8` | copied at spawn (`0x47d1fb`..`0x47d2ef`) from per-spawn words `ebx+0xa8..0xb8` when not `-1.0`, else from the definition words `def+0x264..0x274`; the `wingman`/`devastator` vehicle.zrd records carry no such field (`dynamics` has only torques), so the values come from a default the evidence has not given. The port flies gain 1, limit 1. **This is the dominant unknown.** |
| `def+0x260` (fine-aim limit) | with it below 1 the fine branch never runs ahead (normalised `m = 1`); flown as 0 (`STICK_FINE_AIM_LIMIT`) and, as an experiment, 1 |
| goal-`y` clamp (`+0x314`, `[0x71c3f0]`), 80 m push, leader row signs | as in #2262 |
| `0x64ee4c` (gain/limit bonus byte) | flown as clear |

## Hillside re-flight (retail M01, AI blow on, 120 Hz, 45 s)

`accept_m01_wingman_stick_betty_over_the_hillside` (ignored, retail), spawn
gap 118 m:

| `def+0x260` reading | closest | worst after 30 s | final |
| --- | --- | --- | --- |
| 0 (coarse branch; the committed default) | 119 m | 4155 m | 4155 m |
| 1 (fine branch ahead; experiment, not committed) | 119 m | 866 m | 804 m |

**Betty does not hold station with the ported law at gain 1 / limit 1, and
never closes**: the designed law reached 22 m with her blow off and 36 m with
it on. The trace shows why: the coarse branch writes `A108 = by` whenever
`|bx| < 0.06`, and `by` is the *normalised* vertical aim, so any vertical
error (30 m at 200 m range) is a full-scale pitch word; with gain 1 she
loops over the slot and flies away. The bang-bang law only makes sense at the
original's smaller pilot gains/limits, which are unmeasured. This says
nothing about the original wingman either way; it says the port cannot test
the claim until `def+0x264..0x274` (or the spawn words `ebx+0xa8..`) are read.

No port constant was tuned to hide this, and the M01 pins
(`m01_wingman_formation`, `m01_scenery_colliders`, `m01_free_flight`) stay on
the designed follower and stay green.

## Resolving step

Read the writers of `def+0x260..0x274` (the vehicle-definition constructor
near `0x45c0d4`, which zero-fills `+0x26c..0x274`, and whatever fills
`0x264/0x268`) and the spawn record words `+0xa8..0xb8`; then set the
gains/limits, flip `original_stick` on for mode-4 followers, and re-run the
hillside flight.
