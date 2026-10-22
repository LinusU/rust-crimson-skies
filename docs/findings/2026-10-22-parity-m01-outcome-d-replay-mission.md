# PARITY-M01-OUTCOME-D: Replay Mission restarts M01 (#1656)

Date: 2026-10-22. Implementer: `claude-2` (Rally); no independent review yet.
Capabilities: `retail`, `gpu`, `audio`. Nothing here is `verified_original`; no
original executable was run.

## What it does

Pressing `SB_B_REPLAY` on the debrief (`DebriefScreen::press_hotspot` returns
`DebriefResponse::Replay`; a window click goes through the same path) drops the
`DebriefScreen` (which releases the held clock and brings the HUD back) and
raises `MissionHostRestartRequest`. The composed `mission_host_restart` then
runs `MissionHost::restart`, the code the playtest's `R` already takes: a fresh
session generation and session id, a rebuilt objective/control/world-actor
session, a reloaded world, a respawned player, the settled terminal cleared.
The gun tally and score ledger are keyed by session, so nothing is carried over.

## Recorded unknowns

* The restart goes straight back into flight. Whether the original's Replay
  shows the briefing / Flight Check / launch cinematic again is not measured
  (`2026-10-22-parity5-start-flow-transitions.md` records the measured
  transitions only for the first start); the first run's cinematic is not replayed.
* View All Missions (`ScrapBook_TOC`) and Return to Cabin (#1749) still report
  that they are unavailable.

## Evidence

`cargo test --locked -p cs_app --test cs_app_tests_a accept_parity_m01_outcome_d_
-- --include-ignored`: 2 tests (fail the run, press Replay, assert the new
generation reached `mission_host_restart`, terminal cleared, no exit, no carried
tally; a press with no screen does nothing).

Owner-command smoke `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 60
--smoke-failure --capture-dir private/evidence/PARITY-M01-OUTCOME-D`: exit 0,
7 checks, 0 failed; Replay pressed at frame 2504, generation 1 -> 2, 999 per
mille of the restarted frame off black, no `AppExit`. Frames (private):
`frame_db02492_debrief.png`, `frame_db02594_replayed.png` (the cockpit view with
HUD back over the island).
