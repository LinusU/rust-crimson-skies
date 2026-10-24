# Pause map: which photo the original shows (#1988)

Task `PARITY3-PAUSE-MAP-GAP.05`. Provisional: nothing here is `verified_original`.

## Data

All 52 `loading_*` entries of `escape.zrd` name the same photo,
`MEMENTO BITMAP "momento_temp"` (242x305, a sepia studio portrait of a man). The
name is a template: no entry differs per mission, so the choice is **not**
per-mission and not per-objective in the data. `momento_shad` (300x300) is only
the tilted white shadow drawn at `[661 454]`.

The owner's original screenshot (not committed) shows, inside that shadow, a
black-and-white woman in black gloves leaning beside a lamp. The retail
`rimage.zbd` texture `ms_p_initialpinup1` (256x256, the frame is part of the
texture) is that picture (viewed; `ms_p_initialpinup3` and `ms_p_mom` are other
people). `ms_p_initialpinup1.jpg` is also the one picture the memento selection
script (`MOMENTOSELECTION.SCRIPT`, command 2150) special-cases, and the cabin
screenshot of the same new pilot shows the same woman
(`2026-10-23-parity5-cabin-hub.md`).

## Reading

The photo is the pilot's cabin memento, chosen through native command 2150; a
new pilot has `ms_p_initialpinup1`. `momento_temp` is the placeholder the engine
replaces. Placed at the script's `[533 326]` a 256x256 texture spans
(533,326)-(789,582), inside the screenshot's polaroid.

**Not measured:** how a later memento choice (cabin "Change Memento") maps to a
texture name. The mission session holds no memento state, so the pause screen
always draws the default. Affected content: the photo of any pilot who changed
memento. Resolving task: the cabin memento choice (F47 / command 2150).

## Change

`PauseMapScene::memento_photo` draws `ms_p_initialpinup1` in place of the
`momento_temp` placeholder; any other script bitmap is drawn as named. Tests:
`accept_parity3_pause_map_gap_05_`.
