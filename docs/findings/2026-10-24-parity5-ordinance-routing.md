# Change Ammo opens the OrdinanceLayout screen from the Flight Check

2026-10-24, task #2070 (F2066-C, follow-up to #2062 / #2066). Implemented by
`sonnet-1`. Static readings only: nothing here is `verified_original`, the
original executable was not run. No independent review yet.

## What shipped

* `mission_session/ordinance_stage.rs`: `OrdinanceSetup` (read from the
  installation's `[@OrdinanceLayout@]`, its strings and the committed engine
  facts), `OrdinanceStage` (the screen model plus an `OrdinanceDraft`).
  `FC_B_CHANGEAMMO` (`ZQ -1`, `OrdinanceSide::Pilot`) and `FC_B_CHANGEAMMOW`
  (`ZQ -2`, `Wingmate`) open it through `FlightCheckPhase::press_hotspot`
  (`PhaseResponse::OrdinanceOpened`); `PreMissionStage::Ordinance` holds the
  mission while it shows. The front-end machine stays on `Screen::FlightCheck`
  (the layout names `FlightCheck` as the buttons' target; no machine row is
  involved).
* The feed is `ordinance_feed` (F2066-B) over the side's committed
  `PlaneRecord` (`PlaneRecord::fresh_campaign` until the first accept). The
  presses become stores on the draft: `E 2036` (`choose_ammo`), `E 2037`
  (`choose_rocket`); accept is `E 2034` (`commit`) and writes the committed
  record back; the next visit starts from it.
* Leaving: accept leaves because `XQ` (the script's `DKA`) is `0` here (the
  Flight Check sets `ZQ` only; `XQ` is set by the Campaign page and the debrief
  journal) — `callback A 12`. Cancel acts only with `XQ != 0`, so it does
  nothing from the Flight Check. Escape closes an open list, then leaves
  without committing (the phase's way-back key).
* `mission_session/ordinance_screen.rs` draws it with UI nodes over the Flight
  Check's cover (the shell window's sprite presenter, #2068, belongs to
  another app). The owner-command smoke (`smoke/ordinance.rs`) presses Change
  Ammo, photographs the screen and an opened ammunition list
  (`frame_olNNNNN_ordinance_layout.png`, `..._ordinance_list.png`) and leaves.
* `CHANGE_AMMO_UNAVAILABLE` now only describes a phase opened without the
  installation's layout (a developer composition).

## What stays unknown (affected content, resolving task)

* **The rocket gate.** `ProgressGate::default()` (nothing unlocked, progress
  `0`) stands in for the campaign cells `0x647b5c`, `0x647b68`, `0x647b6c`,
  `0x64b678`; with it the rocket lists offer three rows (table records 0, 1,
  11). Affected: the rocket lists of every real Flight Check. Resolved by
  reading the code that writes those cells.
* **A consumer of the committed record.** The mission start does not read
  `FlightCheckPhase::ordinance_record`; Fly Mission launches with the
  designed loadout. Affected: whether the player's chosen ammunition changes
  anything in flight (F27 gun ammunition, F28 rockets). The `SlotOrdnance`
  mapping in `2026-10-24-parity5-ordinance-feed.md` is the bridge.
* The wingmate's plane record is the fresh-campaign roster record, not the
  mission's authored wingmate; the plane-info line, the description texts,
  the plane diagrams and the drop-down art are not drawn (unread, as in #2062).
* The boxes' frames and row highlight are stand-in colours; text is the default
  face at the `[FONT]` size.
