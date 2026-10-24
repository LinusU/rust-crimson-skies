# PARITY3-PAUSE-MAP-GAP.03: pause-map note rows and flag faces (#1986)

Date: 2026-10-24. Implementer: `claude-1`; no independent review yet. Provenance `static`;
nothing here is `verified_original`. Evidence is the owner's Esc-screen frame (Drydock, retail
1.02, 800x600; private, never committed). The mission state at capture time was not recorded.

## What the frame shows

* The note lists all four objectives: "1) Find the main treasure site.", "2) Drop off Jack at the
  treasure site.", "3) Guard the treasure site until reinforcements arrive.", "4) Dock with the
  PANDORA." No check mark.
* Four flags at the `loading_c61` `OBJPINn` positions (506,149), (396,254), (181,244),
  (106,319): the first three are the "?" flag (`pin6`), the fourth is the "4" flag (`pin4`).
  Zoomed in, the top-right one is a "?" too, not blank. The positions equal the authored ones.
* The green aircraft icon is on the east coast, where the flight starts.

## Readings

1. The note lists every row, woken or not (`PauseObjectiveRow::rows_of`, the screen's list).
   Earlier, only revealed rows were listed, so a fresh flight showed one line.
2. A pin keeps its authored bitmap until its objective is completed
   (`pin_flag_bitmap`). The earlier reading showed the HUD's current objective's pin as its
   number; the frame refutes that for a player at the start (pin 1 is "?"). That *completion*
   switches "?" to the number is a reading, not in the frame: no capture of a completed
   objective exists.

## Unknown

The objective state at capture (the frame is consistent with the start of the flight), whether
the original numbers a pin on completion, on waking or on another event, and where the check
mark of a completed row sits.

## Evidence

Headless `accept_parity3_pause_map_` tests in `cs_app`; smoke frame cited in the review notes.
