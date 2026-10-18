# F45-B.2: the handlers of the front-end buttons that start no screen script

Date: 2026-10-18. Task: #1456 / F45-B.2 (follow-up of #742,
`2026-10-15-f45-b-1-original-layout.md`). Capability used: **`retail`**
(read-only `$CS_GAME_DIR`). The original executable was not run, so nothing
here is `verified_original`. Implementer: `sonnet-2`; no independent review yet.

## What the scripts are

`GOSDATA/ASSETS/crimson.rof` holds 61 plain-text `ASSETS/SCRIPTS/*.SCRIPT`
members (read with `cs-inspect rof --member … --export-dir`, kept in the
ignored `private/`). Control and variable names inside them are obfuscated
(`QW`, `VW`, `$$KW$$`). A screen script creates its controls in `gui_create`
(`object VW = @ctl@BE` then `VW.YC = "mm_b_quit"`, where `YC` is the
`LAYOUT.CSV` row id) and answers button presses in `gui_mailbox` as
`case VW:` arms. Observed shapes: `case A:` / `case B:` stacked over one arm;
`case VW` without a colon; arms ended by `break` or by the closing brace.
The message number of a press (`10003`) is seen guarding the switch in the
main menu and flight check; the reader does not check it.

## What the loader does now (`original.rs`)

`ScreenHandlers::parse` is a line reader (not an interpreter) giving, per
button id, the statements of its arm (an arm ends at the first `break`, next
`case` or closing brace, so an arm with a nested block is **cut at the
block's end** and only ever compared, never executed). A button whose layout
row starts no script binds **only** when its arm is exactly `terminate`,
mapped to `Action::Quit` when the state table has exactly one user `Quit` row
from that screen. Every other arm is reported verbatim
(`UnboundReason::HandlerNotATransition`); a button with no arm is `NoHandler`.
Without the script text (`OriginalScreens::build`) behaviour is unchanged.

## What the retail data states

| button | handler in the script | outcome |
| --- | --- | --- |
| `MM_B_QUIT` | `case VW` → `terminate` | **bound** to `Quit` (302,528, 197x42) |
| `MM_B_CAMPAIGN` | arm shared with preferences: `deactivate(this)`; the screen it starts is the layout row's `Campaign` | unchanged (`ScriptNotPaired`) |
| `FC_B_FLYMISSION` | `callback($$A$$, 1, 1)`, `script_end "flightcheck.script"` | **unbound**: ends the screen, the native's meaning (start mission?) is unknown |
| `FC_B_RETURNBRIEF` | `script_end @flightcheck@`, `mail (11003, @globals@RR)`, `callback($$A$$, 6)` | **unbound**: destination (the briefing?) is not named |
| `SBTOC_B_VIEW`, `_REPLAY`, `_CURRENT` | `callback($$E$$, 2405, …)` + `mail (20002, this)`; replay has a conditional | **unbound** |
| `PF_B_MAINMENU`, `PC_B_NEWMISSION`, `PC_B_SAVE`, `PX_B_CANCEL`, `PX_B_SELL` | conditional / `script_run` of other scripts / natives | **unbound** |
| `PF_B_RETURNTOGAME` | no arm in `PREFERENCES.SCRIPT` | `NoHandler` |

So exactly one more original button (Quit) is a hotspot; the main menu still
has no deck entry (`NoFullPane`, see the earlier record).

## Unknown, and what it gates

- **The `callback($$A$$, n[, m])` and `callback($$E$$, n, …)` natives** (A 1,
  A 6, A 24, E 2405, …) and **`mail (110xx/20002, …)`** messages: no table
  of their numbers exists, so fly-mission, return-to-briefing, view/replay
  and the conditional handlers cannot be mapped to designed actions. Gates
  "every visible original button works" and any claim that `Launch` /
  `Back` on the flight check match the original. Resolving task: the native
  call table work (F-series script/native tasks), none specific yet.
- Whether `terminate` is the whole-process quit or a screen-script end: the
  script statement is read as the former because the menu has no other
  exit; unverified against a run (gates a Quit-behaviour fidelity claim).
- Handlers behind arms the line reader cuts at a nested block.
