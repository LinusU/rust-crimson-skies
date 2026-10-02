# F60-A: benchmark scenarios, budgets and the soak contract

Code: `crates/cs_app/src/diagnostics/` (`scenario.rs`, `soak.rs`). Tests:
`crates/cs_app/tests/accept_f60_a_benchmark_contracts.rs`. Everything here is
authored design; nothing was measured on any machine or on the original game.

## What this stage fixes

- Scenarios: worst-case campaign battle, dense world, smoke/transparent
  effects, many projectiles, full multiplayer load, cold load, warm load, soak.
  Collision, AI, audio, mission content and rendering are required in every
  one (networking in multiplayer); a run with one disabled is an error.
- Budgets are bound to a hardware profile (platform, resolution, quality). A
  run on another profile is an error. Only the designed targets exist: frame
  p95 <= 16,667 us (60 FPS) and simulation tick p99 <= 8,333 us (120 Hz) at
  1080p. Frame p99, load, peak memory and cache size are `Unset`, and an unset
  limit is `Unevaluated`, never a pass (sheet: set budgets after measuring a
  fixed baseline).
- Soak (AC01): ten 6-minute cycles (3 min mission, 2 min AI, 1 min menu) =
  60 minutes = 432,000 ticks, one sample per cycle at the menu boundary. Leaks
  are compared with the last warm-up sample; memory trend is an exact integer
  least-squares slope over post-warm-up samples, bounded at 16 MiB/h (designed
  value, to be replaced from a baseline).

## Unknowns

- **Reference machine:** the owner's machine (CPU, GPU, RAM, OS) is not
  recorded, so `HardwareProfile::baseline_measured` is false everywhere. Needs
  an owner-supplied profile before F60-B/D can set measured budgets. The exact
  fields the owner must supply and how they are consumed are recorded in
  `2026-10-02-f60-reference-machine-and-ia-reading.md` (#469).
- **"IA" in AC01:** confirmed to be **Instant Action** (F49), not an AI
  engagement. The soak's middle phase is an Instant Action mission; the
  previous "AI engagement" reading was a misread and `SoakPhase` now names it
  `InstantAction` (#469, see the reference-machine finding).
- **Memory source:** which counter feeds `resident_bytes` (process RSS vs
  allocator) is decided by F60-B instrumentation.
- Platform smoke (AC03) and device-loss/focus/resize/sleep (AC04) are F60-C/D.
