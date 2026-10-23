# PARITY3-PAUSE-MAP-SCREEN.02 integration check

Task #1817 is the integration step of the split into #1873-#1877. All five subtasks are done; nothing was left to implement.

## Check (main at 5c683a482, `CS_GAME_DIR` set, capabilities `retail,gpu,audio`)

`cargo test --locked -p cs_app -- accept_parity3_pause_map_02 --include-ignored` ran 17 tests, 17 passed (exit 0).

The tests cover:

- Esc toggles the pause map and the clock does not advance while it is open.
- Resume closes the map and the clock runs again.
- Restart restarts the mission under a new generation; Resume does not restart.
- Quit abandons the mission cleanly.
- Preferences opens the settings overlay (rows at authored positions) and Return goes back to the map, including a retail M01 round trip.
- A newly revealed objective changes its pin from '?' to its number.
- The objectives note lists the revealed objectives only.
- The player aircraft icon follows live position and heading.

## Esc controls panel

The controls text panel on Esc is replaced by `pause_map`. `original_keys.rs` keeps only the F1 help.

## Known leftover (not changed here)

The dev-only playtest HUD still prints `PAUSED (reason) — Esc to resume` at `crates/cs_app/src/playtest/visuals.rs:330`. It is outside this task's owner paths, so it is left as is.

## Not covered

The owner-command smoke frame of the pause screen beside the original belongs to #1818.
