# PARITY5-START-FLOW.03: back from a mission to the cabin (#1831)

Date: 2026-10-22. Part of #1747. Implementer `claude-2/claude-1`; no
independent review yet. Nothing here is `verified_original`; the transitions
are the static readings of
`2026-10-22-parity5-start-flow-transitions.md`.

## What now happens

* `cs --cs-path <retail>` hands the mission over as
  `--mission M01 --from-start-flow` (same `--mission` command as before).
* In such a run, the failure debrief's **Return to Cabin** (`SB_B_RETURNPC`)
  and the Esc pause map's **Quit** record `MissionLeft` and end the window;
  `run_windowed` tears the world down and reports the `MissionEnd`.
* `launch_mission` then replaces the process with
  `cs --cs-path <retail> --resume-start-flow cabin|abandoned`. A new process
  image because winit allows one event loop per process.
* `StartFlow::after_mission` replays the table's walk
  (`MainMenu → ProfileSelect → Cabin → Briefing → FlightCheck → Loading →
  Flight`, then `Results → Cabin` or `Pause → Cabin`), so the world the flight
  acquired is released by the table's own effects, and the window opens on the
  `PassengerCabin` layout section.
* From the cabin, **Next Mission** hands the mission over again (a fresh
  process, a fresh world and session generation) and **Return to Main Menu**
  opens the main menu. **Replay Mission** on the debrief is unchanged: it
  restarts in place under a new session generation (#1656), which the
  original does too (`A 1, 0`, no briefing).
* Without `--from-start-flow` (`cs --mission`) nothing changes.

## Limits, recorded rather than guessed

* The cabin is the layout's buttons only; the hub (photo, pins, pilot data) is
  #1840. The pilot-selection page is #1835, so the walk takes the table's
  `ProfileSelect` rows itself (`ContinueProfile`, `ConfirmProfile`).
* Launch row guard: the Flight Check that launched the flight is the mission
  session's, so the machine's own loadout draft is empty and the walk skips the
  `ValidLoadout` guard (`FrontEnd::apply_replayed`).
* **Pause → Quit → cabin is the table's design.** The original's Quit handler
  is unread (see the transitions finding); `MAINMENU_BTN` is only a hint.
* **Per-pilot state is not saved.** The `Status.dat` / `Mission.*` layouts are
  unknown (transitions finding), no pilot profile exists in this flow yet
  (#1835), and a process image carries no state: the pilot, the completed
  count and the debrief record do not survive the return. The requests the
  walk asks for (`OpenProfile`, `ApplyOutcome`) are recorded, not performed.
* Mission Succeeded has no screen yet (#1655 is failure-only), so a won mission
  still ends the process.
* **Not run windowed.** The exec chain (front end → mission → front end) was
  not observed in a window here: the headless tests cover the debrief press,
  the command lines and the machine walk, and the retail test covers the cabin
  section, not the pause Quit press or the process hand-offs.
