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
