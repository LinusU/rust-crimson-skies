# PARITY-M01-PLAYER-VIEW: the integrated player view at the owner-capture seconds

Date: 2026-10-19. Task: #1644, the integration step over its three approved
slices — A #1666 (chase camera preset), B #1667 (`prop1` blur disc), C #1668
(damage-driven wing fire). Visual targets: the owner captures
`orig25.png` (intact, +25 s) and `orig70.png` (port wingtip fire, dark smoke,
per-region damage gauges, +70 s) in the gitignored `private/ref-drydock/m01/`;
never committed. Capabilities used: `retail`, `gpu`, `audio`.

## What the integration is

The slices each proved a mechanism. This step proves them as one view on the
production M01 composition — what the owner-command smoke frames:

- The chase camera is the A slice's **designed capture fit**
  (`CHASE_ASTERN_M = 10.1`, `CHASE_ABOVE_M = 2.1`; fitted so the measured
  lower-wing span ≈ 0.918 of image height and the fuselage/spinner drop ≈
  0.203 of image height, designed tolerance 0.02). The original's chase
  distance, height, FOV, smoothing and horizon-level camera-up stay
  **unknown** — the fit is the target, not a recovered value (#541 names the
  up-axis follow-up).
- The propeller is the B slice's measured `prop1` disc (node slot 211, mesh
  108, `rotorblur.tif`, radius 1.666 m), shown while the engine runs.
- The wing fire is the C slice's measured `wing_flare1`/`wing_flare2` quads
  (slots 208/209), gated per wing on the session `DamageResolver`'s own
  `PartState` — no parallel damage state. With no player damage graph the
  records read `Unknown`, the quads stay `Hidden` and the gauge stays green:
  the intact presentation `orig25.png` shows.

`accept_parity_m01_player_view_retail_m01_composes_the_integrated_player_view`
composes real M01 headless and asserts all three on the one player body at
one resolved frame: the eye at `pos + rot·(0, 2.1, 10.1)`, the disc a child
of the body and drawn, both quads bound to the player actor's
`leftwingdamage`/`rightwingdamage` nodes.

## The parity frames — how the comparison is produced

The no-teleport smoke (`--smoke-no-teleport`) is the only run that never
moves the player (`report.json`: `"mode":"no_teleport"`,
`"player_teleported":false`, `placements:[]`), so it is where the task's
"same simulated time" frames come from. It now photographs the owner-capture
seconds:

| our frame | requested at | compared against |
| --- | --- | --- |
| `frame_01500_flight_25s.png` | t+25 s | `orig25.png` |
| `frame_01801_flight.png` | just after the last wingman sample (kept) | — |
| `frame_04200_flight_70s.png` | t+70 s | `orig70.png` |

The run ends after the t+70 s frame (~70 simulated seconds; `--smoke-seconds`
is a minimum, not a cap). `cs_xtask parity` letterboxes both sides to the
original's 1024×768, pairs each capture with our nearest frame within 1 s
and writes `private/parity-m01/sheet/{sheet.png,report.json,report.md}` —
its report always says "visual comparison needed"; the numbers are a reading
aid, never a parity verdict.

## What the runs showed (this branch's head)

Owner-command scripted smoke
`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY-M01-PLAYER-VIEW`: 21 checks, 0 failed, 28
framebuffers — through the launch cinematic (4818/4818 ticks, handed over,
0.000 m drift, its bars and HUD pixels judged as expected) and the whole
scripted probe/gun/rocket sequence.

No-teleport smoke into `private/evidence/PARITY-M01-PLAYER-VIEW/no-teleport`:
2 checks, 0 failed, 6 framebuffers — `wingmen_near_player` pass (every
wingman within 500 m at t+30 s; distances t+1..30 s: 196, 221, 178, 62,
27 m), every requested framebuffer delivered, 4203 frames.

The parity run paired both captures at Δt = 0.00 s:

- **`flight_25s` vs `orig25`**: our Devastator sits astern-and-above at the
  fitted offset, intact — the same presentation the capture shows: red
  wings spanning the designed fraction of the frame, the `prop1` disc over
  the spinner, no fire, green gauges. HUD vocabulary differs by design
  (ours is the designed gauge cluster and the targeting labels the original
  does not show in this capture); the sky/sea/fog come from the record's
  own weather binds where known.
- **`flight_70s` vs `orig70`**: ours is the same fitted view at the same
  simulated second, still **intact** — the honest limit below — flying
  alongside the Pioneer cargo zeppelin at 1305 ft. The capture's aircraft
  burns from the port wingtip with a dark trail and red/yellow per-region
  gauges. Our run cannot reach that presentation: the M01 player actor
  registers no damage graph, so nothing ever damages it in play and the
  `wing_flare` quads stay `Hidden` by their own gate. The damaged path is
  exercised only by the `accept_` tests' synthetic graph (each wing
  answering its own node), never by a windowed run — resolving task #1699.
  Nor is the damaged look the original's yet: the sprite trail, `fire_lt`
  anchor and per-region gauge cycles are unimplemented (#1700/#1701).

## Measured, designed, unknown

- **Measured** (from the original's data, cited in the slice findings): the
  `prop1` disc mesh/material/radius and its `running`-flag visibility; the
  `wing_flare` quad nodes, positions and material; the wing damage node
  names; the player's `pdevastator` airframe and `player_pfighter`/`healthy`
  subtree; the M01 start pose.
- **Designed** (labelled, never claimed measured): the chase offset
  (10.1 m astern, 2.1 m up), its FOV/smoothing defaults, the 0.02 image-height
  fit tolerance; the HUD damage-gauge tint placeholder; the smoke check
  floors.
- **Unknown** (recorded, not guessed): the original chase distance/height/
  FOV/smoothing and horizon-level camera-up (#541); the original's per-part
  damage thresholds and the M01 player's damage graph (#1699); the trail
  emitter parameters and `fire_lt` anchor (#1700); the per-region gauge
  cycle wiring (#1701); `prop1b` and the RPM/spool threshold.

## Evidence

- Acceptance harness:
  `crates/cs_app/tests/evidence_report_parity_m01_player_view.rs`; committed
  report `docs/findings/evidence/PARITY-M01-PLAYER-VIEW.json`, validated by
  `tools/validate_evidence.py` (structure and artifact hashes; `--require-pass`
  reports the carried follow-up unknowns as unresolved issues by design).
- Private artifacts: `private/evidence/PARITY-M01-PLAYER-VIEW/` (scripted
  run's 28 PNGs + `report.json`), `…/no-teleport/` (6 PNGs + `report.json`),
  `private/parity-m01/sheet/` (contact sheet and reports; owner captures stay
  in `private/ref-drydock/m01/`, gitignored).
