# OrdinanceLayout integration status (#2066)

2026-10-24, integration step of #2066 after its split into #2068 (view),
#2069 (feed) and #2070 (routing). Implemented by `sonnet-1`. Static readings
only; nothing here is `verified_original`. No independent review yet.

## Met

* View: `ordinance_layout_view.rs` (shell window) and
  `mission_session/ordinance_screen.rs` (Flight Check) draw boxes, chosen rows,
  open lists, gun names (greyed for kind 5) and titles.
* Routing: `FC_B_CHANGEAMMO` / `FC_B_CHANGEAMMOW` (ZQ -1 / -2) open the screen;
  accept, Escape and cancel follow the script (see the routing finding).
* Feed: `ordinance_feed` builds guns, the 5 ammunition rows and the rocket rows
  from the engine table at `0x619efc`; `E 2034`, `E 2036`, `E 2037` edit and
  commit a working copy (`OrdinanceDraft`).

## Not met, stays unknown

* **The Instant Action draft is not wired.** Nothing builds a `PlaneRecord`
  from a `ScenarioActorSpec`, nor writes a committed record back into
  `SlotOrdnance` (`gun_ammo` = `+0x98`, `rockets` = `+0xa8`). Box counts
  (`+0x34` / `+0x38`) have no draft counterpart. Today only the Flight Check
  path commits, to `FlightCheckPhase::ordinance_record`.
* **The rocket gate** (`0x647b5c`, `0x647b68`, `0x647b6c`, `0x64b678`): the
  writers are unread, `ProgressGate::default()` stands in.
* No consumer of the committed record in flight (F27, F28).
