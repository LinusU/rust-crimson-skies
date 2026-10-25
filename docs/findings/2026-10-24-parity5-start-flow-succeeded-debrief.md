# Start flow: the Mission Succeeded debrief (#1972)

Date: 2026-10-24. Implementer `sonnet-2`; no independent review yet. Nothing
here is `verified_original`.

## What now happens

* In a run the start flow handed over (`--from-start-flow`), a settled
  **Success** shows the ScrapBook debrief screen instead of ending the process.
  The caption is the string image's `IDS_SB_SUCCESS` (`Mission Completed`); the
  rest is the failure screen's layout and rows (`DebriefScene::build`,
  `facts.succeeded`).
* Its **Return to Cabin** is the failure screen's: it records `MissionLeft` and
  the front end reopens on the cabin through the F45 machine (see
  `2026-10-22-parity5-start-flow-03-return-to-cabin.md`).
* Without `--from-start-flow` (`cs --mission`) a win keeps its exit code 0 and
  no screen, as before. An extraction is unchanged.

## Recorded unknown (rule 4)

* The Mission Succeeded frame has not been photographed from the original. The
  scene lists `SB_T_COMPLETETITLE` among `DebriefScene::unknowns` for both
  variants: whether the stat rows, the card or the buttons differ is unknown.
* Cash Earned stays unknown (no cash rule is measured), as for the failure.

## Not done

* **No smoke frame beside the original.** There is no original Mission
  Succeeded frame to compare with, and the owner-command smoke does not win M01
  by itself, so no windowed screenshot of the won screen was taken. The
  headless acceptance tests (`accept_parity5_start_flow_succeeded_`) cover the
  caption, the unknowns, the press and the exit. Needs the owner's frame and a
  windowed win.

## Smoke checks (2026-10-25)

`--smoke-success --from-start-flow` now judges `success_debrief_drawn` (the
debrief was up for the settled frame, off-black floor) and
`success_returns_to_cabin` (Return to Cabin answers `ReturnToCabin` through the
screen's own press; not applied, as that ends the process before the report).
Both apply only in a start-flow run. Retail run, 700 s budget: 5210 frames,
13 checks, no failures; settled frame 744 per mille off black, showing
"Mission Completed", Run Time 1:24, Gun Hit Ratio 100%, Overall Planes Downed 4
and Cash Earned unreadable (unknown). `MissionExitHeld` stays: a win without the
start flow still ends the process and the plain smoke needs the hold. Frames are
in `private/evidence/1972/` (not committed). No original frame to compare (#1655).
