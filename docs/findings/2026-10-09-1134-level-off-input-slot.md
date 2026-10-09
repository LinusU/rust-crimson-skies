# #1134 — the original Level-Off assist gets an input-layer slot and is wired into the retail playtest

Date: 2026-10-09 · Task: Rally #1134 `FLIGHT-ORIGINAL-LEVELOFF-INPUT` · Branch
`rally/1134-give-the-original-level-off-assist-comma` · Capability: `retail`,
`synthetic` · Claim: `implemented`.

## What was asked and why

Task #797 flew the retail playtest on the original's statically recovered
fixed-wing law (`OWNER-STATIC-2026-10-08`) and found the law's Level-Off
assist (command 47, the original's Shift+L) fully present in the recovered
torque path but unreachable: `cs_types::input::FlightCommand` had no slot for
it, so the retail playtest could not bind it and `OriginalState.level_off`
stayed `false` for every run. #797 recorded that as its evidence unknown
`level_off_command_47` and filed this follow-up instead of widening its own
owner paths.

## What changed

* **`cs_types::input`** — a new `FlightCommand::LevelOff` edge command
  (label `level_off`, appended at the end of `FlightCommand::ALL` so the
  `cs_net` wire tags of every existing command are unchanged), a new `Key::L`
  keyboard key (label `l`), and the F22 designed-default action map binds
  `Key::L` → `LevelOff`. The binding comment says what it is: the map has no
  chord sources, so the original's Shift+L chord is not reproduced — the
  letter alone carries the command here, a designed slot for the original
  command.
* **`cs_app::input::platform`** — `key_from_bevy` lowers `KeyCode::KeyL`
  onto the new engine key.
* **`cs_app::playtest`** — `apply_flight_command` counts the
  `FlightCommand::LevelOff` edges the frame's `PlatformFrameReport` delivered
  and flips `OriginalState.level_off` once per delivered press (odd count in
  the frame toggles, so two taps in one frame cancel and a paused or
  unfocused session, which runs no input boundary and so delivers nothing,
  cannot flip it). The playtest's `R` reset, throttle-step and pause bindings
  are untouched: `L` was unbound before, and the existing
  `accept_playtest_fly_*` / `accept_playtest_retail_launch_*` tests stayed
  green unchanged.
* **`cs_app::playtest::retail`** — the `playtest flight` JSON statement now
  carries `"level_off_toggle_wired":true,"level_off_binding":"key.l"` instead
  of `false` plus the resolving-task note.
* **`cs_content` F22-H comparison table** — the new command had to be
  classified against the original (the F22-H coverage test demands every
  declared command be classified). The original's `strings.dll` ships the
  measured command label **`MSG_LEVEL_TOG` (11061)** for it, so `level_off` is
  `observed`, not absent; `MSG_LEVEL_TOG` moved out of
  `ORIGINAL_ONLY_COMMANDS` (33 → 32) into the cited set (32 → 33). The 65
  measured labels still partition exactly, and the machine-checked test
  enforces it.
* **Docs** — `docs/PLAYTEST.md` and `docs/PLAYTEST-RETAIL.md` now describe the
  wired toggle (bound to `L`, why the chord is not Shift+L) instead of saying
  the input layer has no slot; the on-screen controls reminder gained `L`.

## The tests and what they measured

Prefix `accept_flight_original_levelop_`, three tests:

* `accept_flight_original_levelop_torque_path_runs_only_when_on_and_hands_off`
  (`cs_sim`, unignored, runs in CI): from one 30-degree-banked state at
  cruise, the hands-off on/off steps report **different** `world_torque` (the
  assist contributes), the roll-deflected on/off steps report **bit-identical**
  torque *and* state (the gate suppresses the assist entirely), and after
  three simulated seconds hands-off the on case banks **1.48°** while the off
  case still banks **28.72°** (start 28.65°). The gate is the law's own
  `level_off && roll == 0 && pitch == 0` condition, unchanged by this task.
* `accept_flight_original_levelop_input_slot_and_designed_bindings`
  (`cs_app`, unignored, runs in CI): the vocabulary slot exists and is an
  edge, `key_from_bevy(KeyCode::KeyL)` lowers onto it, the playtest map
  resolves `L` → `LevelOff` in flight context while `Left Shift` remains
  `ThrottleStepUp`, `R` remains unbound (reset meta key) and text entry still
  gates the command; a real `InputSession` pumped with a keyboard report that
  holds `L` delivers exactly one Level-Off edge and the release delivers
  nothing.
* `accept_flight_original_levelop_bound_key_toggles_the_retail_level_off`
  (`cs_app`, `#[ignore = "requires CS_GAME_DIR"]`, run locally): over the
  installation, one press of `L` flips `OriginalState.level_off` on the retail
  playtest body, the second flips it back, `R` reset returns to off, and the
  `playtest flight` statement reports `level_off_toggle_wired:true`. The
  assist's effect on the retail body: a 30° bank hands-off with the toggle on
  rolls to **1.453°** in 3 s; the same bank with it off stays at **29.78°**;
  with it on but `E` (roll) held the bank stays at **28.42°** — the gate
  suppresses the assist and the stick banks the aircraft instead.

The task selection `cargo test --workspace --locked --
accept_flight_original_levelop_ --include-ignored` discovered and executed all
three, all green, with `CS_GAME_DIR` set. The full workspace suite, `cargo fmt
--all -- --check` and `cargo clippy --workspace --all-targets --all-features
--locked -- -D warnings` are green too.

## Evidence

* `docs/findings/evidence/T1134.json` — this task's report
  (`private/evidence/T1134/` holds the run artifacts: the acceptance log and
  `playtest-flight.json`, the production flight import whose statement now
  reports the toggle wired). Two unknowns, both honest: the playtest binds the
  bare letter because the action map has no chord sources (the original's own
  default binding remains unmeasured native data), and the assist's rate is
  still static evidence uncalibrated against an original run (#358).
* `docs/findings/evidence/T797.json` — regenerated on this branch with
  `level_off_command_47` **resolved and removed** (its harness
  `evidence_report_flight_original_playtest.rs` no longer carries the entry;
  `#1135` had already resolved `scripted_smoke_path` the same way, so the
  regenerated report carries the three still-open unknowns —
  `player_spawn_speed`, `roll_axis_sign` and `law_calibration`). The T797
  acceptance suite was re-run green over the installation to produce the new
  log.

Nothing here is `verified_original`: the law and its parameters remain static
evidence under `OWNER-STATIC-2026-10-08`, still uncalibrated against an
original run (#358), and the original's own default key bindings remain
unmeasured (F22-H unknown #1). What this task adds is a wired, tested input
slot — the playtest can now exercise the assist — and says so.
