# PARITY5-START-FLOW.04: the owner-command start-flow smoke (#1832)

Date: 2026-10-23. Part of #1747. Implementer and reviewer `swe2-max-1`
(same agent name as the implementation session; fresh review context — a
same-instance review, not independent evidence). Nothing here is
`verified_original`; no original executable ran. Evidence report:
`docs/findings/evidence/PARITY5-START-FLOW.04.json`, artifacts under
`private/evidence/PARITY5-START-FLOW.04/`.

## What ran

```sh
CS_CAPABILITIES=retail,gpu,audio \
target/release/cs --cs-path $CS_GAME_DIR \
  --smoke-seconds 60 --capture-dir private/evidence/PARITY5-START-FLOW.04
```

The bare `--cs-path --smoke-seconds` command scripts the whole start flow
through the same windows, input path and renderer a player drives, as a
chain of four process images (winit allows one event loop per process).
Every leg passed:

* `front-end/` — the `StartFlow` smoke scene: `mainmenu.png`, `campaign.png`
  (after the Campaign press), `pilot.png` (the typed new pilot "Eve" on the
  page's own edit box), `intro.png` (the intro movie, then Esc-skipped),
  `cabin.png`, then Next Mission's hand-over to `--mission M01
  --smoke-start-flow`. Report `kind: front-end-shell-smoke-report`, all
  required frames and the MainMenu → Campaign → PassengerCabin visit order
  present, `passed: true`.
* `mission/` — one `MissionSmokeMode::StartFlow` leg: the ten briefing
  pages (`frame_br00..br09`), the Flight Check and its bare pane
  (`frame_fc00020`, `frame_fc00028`), the launch cinematic
  (`frame_00005_cinematic_start` … `frame_02417_cinematic_handover`), the
  first-control frame (`frame_02411_first_control`), a short controlled
  flight (`frame_02568_flight`), then Esc's pause map
  (`frame_02577_pause_map`) and its authored **Quit** click. Report
  `kind: mission-smoke-report`, `mode: start_flow`, 20 checks all passing,
  `passed: true`, exit `abandoned` → `--resume-start-flow abandoned`.
* `resumed/` — the `StartFlowResumed` scene: `cabin.png` again, then Next
  Mission's second hand-over. `passed: true`.
* `replay/` — the mission leg once more; it is never handed back, so its
  Quit ends the process and the chain. `mode: start_flow`,
  `start_flow_mission_left` recorded not-applicable. `passed: true`.

## Frames beside the owner's Drydock captures

Side-by-side reading of this machine's frames against the owner's original
captures (git-ignored under `private/ref-drydock/`):

| ours | original | reading |
| --- | --- | --- |
| `front-end/mainmenu.png` | `menu/original.png` | same logo, same backdrop art, same four-button column in the same order and palette |
| `front-end/pilot.png` | `menu/original-pilot.png` | same pilot-page layout; ours carries the typed name in the edit box |
| `mission/frame_00062_scene_01s.png` | `m01/orig25.png` | same M01 opening world: terrain, water, sky and the flight's authored pose |
| `mission/frame_02411_first_control.png` | `m01/orig70.png` | same in-flight composition with the HUD up |

The comparisons are the reviewer's reading of two stills, not a metric:
the capture resolutions and cameras are close but not claimed identical,
and no original run produced reference frames of the briefing, Flight Check
or pause-map screens. `human_review` remains the owner's.

## Limits, recorded rather than guessed

* **Both smokes keep their manual 1/60 s clocks**; wall time is not
  simulated time.
* **Pilots are sandboxed.** The typed pilot lives under
  `front-end/profiles/` inside the capture directory; the production
  profile tree is never opened, and nothing is written under
  `$CS_GAME_DIR`.
* **Per-pilot state does not cross the chain's process images** — the
  pilot, the completed count and the mission record do not survive the
  return (recorded, not performed; the #1831 finding).
* **Pause → Quit → cabin is the state table's design**; the original's
  Quit handler is unread
  (`2026-10-22-parity5-start-flow-transitions.md`).
* A leg that never reaches a required screen fails with a named check —
  `briefing_screen_drawn` names a mission leg whose briefing never ran,
  and the front-end leg's required labels (`mainmenu`, `campaign`,
  `pilot`, `intro`, `cabin`) fail naming the frame that never arrived.

## Review fixes in this branch

* `start_flow::evaluate` now emits the failing `briefing_screen_drawn`
  check when the leg collected no briefing metrics — previously a stuck
  leg failed checks that never named "briefing".
* The first leg's required-frame list gained `pilot`, which the script
  requested but nothing required.
* `build_report` labels the mission legs `"mode":"start_flow"`; they were
  serialized `"scripted"`.
* The windowed acceptance test honours `CS_START_FLOW_SMOKE_OUT`, so the
  evidence harness can collect the chain's captures under
  `private/evidence/PARITY5-START-FLOW.04/`.
