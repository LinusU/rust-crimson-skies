# M01-PLAY-TRIGGERS-WINDOWED: the driver exists, the windowed run is not yet recorded

Date: 2026-10-15. Task: `M01-PLAY-TRIGGERS-WINDOWED` (Rally #1486), the
windowed half of #1439. Implementer: claude-1. Nothing here is
`verified_original`.

## What this slice adds

`crates/cs_app/src/mission_session/scenario.rs`: a capture **scenario**
(start override, trigger watch) read from `CS_MISSION_START_M`,
`CS_MISSION_START_HEADING_RAD`, `CS_MISSION_WATCH_BLOCK` and
`CS_MISSION_WATCH_MAX_FRAMES`, documented in `docs/PLAYTEST.md`. Wiring:
`run_windowed` applies the start override to the stage's start pose, the
capture plugin installs `watch_trigger` after `mission_host_tick` (one
rendered frame runs two fixed ticks, so the completion must be read on its own
tick), and `finish` refuses a watched run whose block never completed.

Inputs that are missing, half-stated, malformed or asked of a non-capture run
are `CaptureRefusal::Scenario`. CI covers this without `CS_GAME_DIR`
(`accept_m01_play_triggers_windowed_*`).

## What is unmet

The windowed run itself has **not** been made. This session had
`CS_CAPABILITIES=retail,gpu,audio` and `CS_GAME_DIR`, but `CS_ENGINE_IMAGE`
was unset, and the mission launch needs the owner-supplied decrypted
executable for the player's airframe and start pose. AGENTS.md forbids reading
it unless that variable names it. So no `acceptance.json`, no
`docs/findings/evidence/M01-PLAY-TRIGGERS-WINDOWED.json`, and the windowed
criterion of #1439 stays unmet. The evidence harness (modelled on
`crates/cs_app/tests/evidence_report_m01_play_weather.rs`) is also not
written: its assertions on the `trigger` record should be written against a
real trace.

Suggested placement for the run (computed, not measured in a run): 1 300 m
from the anchor on the line toward the measured start, at the measured start
altitude, heading `atan2(-dx, -dz)` toward the anchor (`-Z` nose under `Ry`).
For the anchor at `[-5972, 1460, -7680]` that is
`CS_MISSION_START_M=-5414.8,1318,-8854.5`
`CS_MISSION_START_HEADING_RAD=2.6986`. Whether the body holds altitude and
speed over the ~1 300 m at the law's behaviour without input is unmeasured.
