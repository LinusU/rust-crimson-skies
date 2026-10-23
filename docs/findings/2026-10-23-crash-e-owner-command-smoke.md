# #1857 PARITY4-CRASH-E: the crash phase of the owner-command smoke

Date: 2026-10-23. Implementer: `sonnet-2`. Nothing here is `verified_original`; a scripted run is never
`human_play`.

## What is in the build

`cs --cs-path <retail> --mission M01 --smoke-seconds 60 --smoke-crash --capture-dir <dir>`
(`docs/PLAYTEST.md`, "The crash mode"). After the briefing, Flight Check and launch cinematic it places the
player 400 m above the sea ahead of it with the nose 45 degrees down (a test placement, in `report.json`
`placements`), holds the Up arrow, and waits for the production crash flow: a `CrashSequence`, the contact
frame, the detached camera frame with `Fatal Crash!`, a key press, the Mission Failed screen. The pure judging
(`smoke/crash.rs::evaluate`) is covered by `accept_crash_e_` (4 tests, `tests/campaign/accept_crash_e_smoke.rs`).

## What the retail run shows (after #1983 judged the world's solid colliders)

Run on the rebased branch (main at 72c928396) with `CS_GAME_DIR` and `CS_ENGINE_IMAGE` set:
`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 60 --smoke-crash --capture-dir private/evidence/PARITY4-CRASH-E`,
exit 0, 8 of 8 checks passing, 16 framebuffers. The dive is placed at frame 2471 from (-1469.6, 400, -1864.9)
with Up held; the production terrain-contact rule judged 9 contacts and ended the flight at frame 2777 (tick
5549, severity 0.178) at (-1510.9, 2.7, -1913.5). The frames, read by eye:

* `frame_cr02782_crash_contact.png`: the aircraft seen from above with a smoke trail, orange fire puffs and
  yellow sparks beside the right wing, and the blue `Fatal Crash!` text at the left edge.
* `frame_cr02837_crash_camera.png`: the detached camera over the aircraft with `Fatal Crash!` still shown and
  the smoke drifting away.
* `frame_cr02888_failed.png`: the scrapbook debrief, `Mission Failed` / `Mission Results`, Run Time 0:46,
  Gun Hit Ratio 0%, Cash Earned unknown, Overall Planes Downed 0.

## Unmet

* The crash is into the **sea** near the start, not the jungle plateau the task names: the contact position is
  2.7 m above the origin of the sea, and both crash frames show water. Placing the dive over the plateau needs
  its measured coordinates; none are recorded, so none were guessed.
* No Drydock crash frames are on this machine (`private/ref-drydock/m01` holds only `orig25.png`,
  `orig70.png`), so no frame could be put beside an original. The owner's captures are needed for the
  comparison.
* The `Fatal Crash!` text sits at the left edge in blue; whether that is the original's place and colour is
  not established here.
