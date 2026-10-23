# PARITY5-CABIN-INTRO.04: the cabin hub drawn from the original UI data

Date: 2026-10-23. Task #1840 (part of #1749). Implementer `claude-2/claude-1`;
no independent review yet. Nothing here is `verified_original`; no agent ran
`crimson.exe`. Tests: `accept_parity5_cabin_intro_04_` in
`crates/cs_app/tests/ui/parity5_cabin_intro_04.rs` (four synthetic, run in CI;
a retail and a windowed one, `#[ignore]`d without `CS_GAME_DIR`/a GPU).

## What is drawn

`MenuShell` already drew the `PassengerCabin` rows as written. The hub
(`crates/cs_app/src/ui/front_end/cabin.rs`) adds what `PASSENGERCABIN.SCRIPT`'s
`gui_create` does on top, as far as the data states it:

| Script statement | Effect here | Basis |
| --- | --- | --- |
| `deactivate(PMA)` (`PC_B_SAVE`) | Save is removed: not drawn, not focusable, not pressable | owner frame has no Save button; the pilot-page capture showed `deactivate` hides |
| `HMA.AK = "pc_p_hangar" N ".jpg"` | `PC_PLANE` draws `pc_p_hangar5.jpg` at the row's `46,69`, at the jpg's own 720x334 | hangar 5 fits the owner frame with a mean difference of 3.1 over the window region at offset (0,0); the ten others score 14-22 |
| `JMA[R].CK = R`, one `PC_PIN<R>` per pin | pin `R` draws frame `R` (20 rows) of `PC_mappins.png`; only the first `pins` panes exist | `CK` as the frame index is inferred (5 pins, 5 frames); one pin is what the capture shows |
| `focus(@ctl@)` | no button focused on entry; the first Down focuses the first | the owner frame draws Change Memento in resting gold |
| Previous Missions art 201x129 | four 32-row frames; row 128 is padding | rows repeat at period 32 and row 128 has no opaque texel (retail test) |

`CabinHub::NEW_PILOT` is `hangar 5, 1 pin`: what the capture of a new pilot's
cabin shows, **not** an original table. Hover (rollover frame), press
(depressed frame) and keyboard focus are the shell's generic strip rules, which
the owner-command-style smoke shows on the cabin buttons.

## Frames

`CS_FRONT_END_SMOKE_SCENE=cabin cs --cs-path <retail> --front-end
--smoke-seconds 14 --capture-dir <dir>` (cabin pass: the start flow's cabin,
hover Next Mission, press Previous Missions, slide off, Down x3) wrote
`private/evidence/PARITY5-CABIN-HUB/{cabin,cabin-hover,cabin-pressed,cabin-focus}.png`
(1600x1200 framebuffers; git-ignored). The owner's frame is
`private/ref-drydock/cabin/original.png`. Over 800x600, outside the memento
frame, the mean absolute channel difference is 5.4 (bottom third 7-8: the
desk art is sampled differently, the cursor sits elsewhere). By eye the
window, plane, radio, microphone, map with its one pin, book, blueprint, desk,
and the five brass buttons agree in placement and art.

## Not matching, recorded rather than guessed

* **The memento picture is not drawn**: the frame's interior shows the hangar
  jpg's lilac key patch. The original shows a photograph (a woman in a
  feathered hat); which `scrapbook\MS_P_*` file comes from callback 2150
  (engine state) and how `KX.GK/HK/IK/JK` (20, 20, 1, 3) size it are unread.
* **Hangar index and pin count** are engine callbacks (2010, `A 7 -2`) whose
  value per pilot is unread; the new-pilot values above are one capture.
* Clicking `PC_FRAME` (a region that mails Change Memento) is not wired.
* `PC_D_MISSIONS` (typed `idaho`) is not drawn; Next Mission's disabled state
  (callback 2600, `mail 10000`) is not modelled.
* `PC_MEMENTO`'s `Z` (2) and the `Z` column generally: draw order stays
  declaration order, which gives the right picture here.
* Next Mission / Plane Construction / Previous Missions / Change Memento
  presses are #1841.
* The cabin smoke scene is chosen by an environment variable
  (`CS_FRONT_END_SMOKE_SCENE=cabin`) because `cli.rs` has no flag for it; the
  whole-flow smoke is #1842.
