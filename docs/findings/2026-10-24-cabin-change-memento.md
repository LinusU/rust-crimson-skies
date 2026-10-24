# Cabin Change Memento screen (`MomentoSelection`)

Date: 2026-10-24. Task #1957 (follow-up of #1841). Implementer `sonnet-2`; no
independent review yet. Nothing here is `verified_original`: the original
executable was not run. Tests: `accept_parity5_cabin_change_memento_` in
`crates/cs_app/tests/ui/parity5_cabin_change_memento.rs` (synthetic ones run in
CI; the retail screen needs `CS_GAME_DIR`, the table check `CS_ENGINE_IMAGE`).

## What the screen is

Layout section `MomentoSelection` (`ASSETS/LAYOUT.CSV`): panes
`MS_BACKGROUND` (`Z` 2), `MS_GLASS` (3) and `MS_MEMENTO` (`Z` 1, no art, at
184,48), buttons `MS_B_ACCEPT` and `MS_B_CANCEL` (both start `PassengerCabin`,
`EndScript` 1), `MS_B_ARROWL` / `MS_B_ARROWR` (no script). The script
`MOMENTOSELECTION.SCRIPT` builds the picture on `MS_MEMENTO` as
`assets\graphics\scrapbook\<FX>` where `FX` comes from native `E 2150`:

| script call | meaning |
| --- | --- |
| `2150, 0, -1, FX` (create) | the pilot's current memento name |
| `2150, 0, 100, FX` (right arrow, `GX = 100`) | the next name |
| `2150, 0, 101, FX` (left arrow, `GX = 101`) | the previous name |
| `2150, 1, FX` + `A 12, 0` (Accept) | store `FX` as the pilot's memento |

Cancel starts the cabin with no call, so the stored memento stays.

## The engine's side (static, decrypted image, digest `43540fc9…`)

`E 2150` is `0x40c7f4` (mode in `ebx`, index and out-string arguments):

* mode 1 copies the name into the buffer `0x64b684` and stores its table
  index in `[0x647b90]` (`0x410270`, a case-insensitive lookup; a name not in
  the table gives 1);
* mode 0, index `-1` returns the buffer's name (and sets `[0x647b90]` the same
  way); index `100`/`101` steps `[0x647b90]` by one, wrapping (previous from
  row 0 goes to the last row), repeating until a row is offered;
* **the table** is 24 twelve-byte records at `0x61af60` — name pointer, record
  number, bit — ended by a pointer to an empty string (`0x6463b0`, in `.data`'s
  zero-filled tail). Names: `MyMemento.jpg`, the seven "initial" pictures
  (`MS_P_InitialPinup1`, `…Pinup3`, `JustineBattleax`, `Mom`, `DoggiePhoto`,
  `Swan&NathaninCabin`, `ZacharyandPlane`) and 16 gated ones
  (`MS_P_<mission>_<n>_…`). All 23 pictures other than `MyMemento.jpg` are
  members of `ASSETS/GRAPHICS/SCRAPBOOK/` in `crimson.rof`.
* a row is offered when its record is `0` (always: the seven), when its record
  is `-1` and `Assets\Graphics\Scrapbook\MyMemento.jpg` exists
  (`0x61f354`, `0x5d01d0`; the retail installation has none), or when the
  mission record's flags word (`0x64cca4 + 168 * (record - 1)`, the same
  168-byte records the intro-movie gate reads at stride 840 per five) has
  **bit 0 and bit `bit`** set.
* `A 12, 0` clears the byte `0x64dd39` and calls session method `0x419c20`
  with it: the cabin hand-back; it is not a visible save.

`src/ui/front_end/memento.rs` carries the table (cited: claim
`OWNER-STATIC-2026-10-24-MEMENTO-TABLE`, `static`) and the stepping rule; the
ignored test `…_table_matches_the_engine_image` re-reads the image's bytes.

## What was built

* `StartFlow` opens the section from Change Memento (the flow's `Memento`
  step; no notice), stores the choice on Accept and keeps it on Cancel, and
  reopens on the stored choice. `MS_MEMENTO` is placed (below the layout's
  panes, at 184,48 and the picture's own extent) by `StartFlow::sync_memento`.
* `NOT_IMPLEMENTED` lost its Change Memento row; the two others stay.
* The window decodes the 24 pictures up front and redraws on each step.
* Smoke scene `memento` (`CS_FRONT_END_SMOKE_SCENE=memento cs --cs-path <retail>
  --front-end --smoke-seconds 14 --capture-dir <dir>`): the cabin, the screen,
  next, back-wrapping, Cancel, reopen, Accept, reopen. The shots were looked at:
  the pilot's initial picture (the woman in the feathered hat, the same one the
  owner's cabin capture shows) and the next one, inside the original frame art
  with its two brass buttons and the arrows.

## Not done, recorded rather than guessed

* **The cabin frame's own picture** (`PASSENGERCABIN.SCRIPT`: `2150, 0, -1`
  into `PC_MEMENTO`, `GK/HK = 20`, `IK = 1`, `JK = 3`) is still not drawn: the
  name is now known (the pilot's memento) but its size and position in the
  frame are not. Measured on this project's cabin shot: the frame's key patch
  is about 71x92 logical pixels with its left top at about 180,330, while the
  row is at 169,325; the pictures are about 362x460, so `GK/HK` as a 20 %
  scale would fit the size but not the position. Affected content: the cabin
  frame's interior (shows the key colour). A follow-up task is filed.
* **Which flags words a pilot has.** Nothing in this project writes the mission
  records' flags (`0x64cca4 + 168 n`; what sets bit 0 and bit `n` when a
  mission is flown is unread), and the start flow keeps no per-pilot progress,
  so [`MementoUnlocks::new_pilot`] is used: a pilot is offered only the seven
  unconditional pictures. Affected content: the sixteen gated pictures of any
  pilot who has flown missions. `MementoUnlocks` is the interface a progress
  model fills.
* **Persistence.** The choice lives in `StartFlow`: it survives the cabin
  and the screen, not the process. Where the engine saves `0x64b684` is unread.
  The mission session's pause map still draws the default
  (`2026-10-24-pause-map-memento-photo.md`).
* The script's hidden hotspot for `ms_p_initialpinup1.jpg` (`z` 1000, region
  370,245 to 380,255), the arrows' ten-frame blink on creation (messages
  10018/10000), the hourglass picture drawn while the left button is held
  (`lbutton_update`) and `PC_FRAME` (clicking the cabin's photo presses Change
  Memento) are not modelled.
* The shot of the owner's original screen was not compared: there is none for
  this screen.
