# PARITY3-PAUSE-MAP-SCREEN.01: where the original Esc map screen's data lives (#1816)

Date: 2026-10-21. Implementer: `claude-2/claude-1` (Rally); no independent review yet.
Capability: `retail` (read access to the installation and to `$CS_ENGINE_IMAGE`). Every claim
below has provenance `static`; nothing is `verified_original` and no original executable was run.
No original bytes, screenshots or decoded images are committed; the images were looked at in
`private/` only.

The owner screenshot shows, on Esc (`MSG_CMD_PAUSE_GAME`), a full-screen chart with flag pins, a
green aircraft, an Objectives note, a sepia photo and four buttons. All of it is **data-driven by
one script document**, found below. Probe: `crates/cs_app/tests/ui/pause_map.rs`, prefix
`accept_parity3_pause_map_01_` (5 `retail` tests, `#[ignore = "requires CS_GAME_DIR"]`, they panic
without it). Digests are SHA-256; texture digests are over the decoded texel plane as the production
UI texture catalog (`FrontEndScreens::inventory`) records them; reader-member digests are over the
stored bytes the production reader archive returns.

## The answer in one paragraph

`ZBD/zrdr.zbd#escape.zrd` is the Esc screen. Its key `loading_c61` is M01's entry (chapter 1,
mission 1; the same key names the M01 briefing script `brief_c61`, and its `MAP` bitmap is the
briefing's `HA-m1MAP`). It names the chart bitmap, its on-screen position, clip rectangle and a
`WORLD` rectangle, the memento photo, and an `ESC_SCRIPT` that lays down the objective list, the
shadow and one flag-pin picture per objective **at authored screen pixels**. The shared
`LOADINGDIALOG` names the player aircraft icon, the Objectives note, the four buttons and their
fonts. Every bitmap is a texture of `ZBD/rimage.zbd`. `map.zrd` / `map_anims.zrd` are **not** this
screen: they describe a cockpit map object.

## (a)-(f) Located art and data

Source spans of the documents (offset and length inside the container, from the production
version-one index; container `ZBD/zrdr.zbd` is 3 988 394 bytes):

| Member | Container | Offset | Length | SHA-256 |
| --- | --- | ---: | ---: | --- |
| `escape.zrd` | `ZBD/zrdr.zbd` | 219 739 | 220 066 | `c1c9694ddecb…edb74d` |
| `ia_escape.zrd` | `ZBD/zrdr.zbd` | 455 759 | 222 536 | `6237bd2a3811…e732f6` |
| `Loading.zrd` | `ZBD/zrdr.zbd` | 679 110 | 252 737 | `c610d865dcff…360b9` |
| `fonts.zrd` | `ZBD/zrdr.zbd` | 440 522 | 13 902 | `a4ad1d84e02e…cebfed6` |
| `map_anims.zrd` | `ZBD/zrdr.zbd` | 935 632 | 1 630 | `c05d263a160e…521123` |
| `map.zrd` | `ZBD/C3/M01/zrdr.zbd` | 3 834 | 651 | `07539cae3ae0…254ce7` |

Full digests are in the probe. `escape.zrd` holds 83 top-level fields: `SHARED_IMAGE_PATH`,
`LOADINGDIALOG`, `default` and 80 `loading_*` entries (52 have a `MAP` + `MEMENTO` pair; the
other 28, including every `loading_m*`, carry no chart). `ia_escape.zrd` has the same 83 fields
(the Instant Action variant; its `loading_c61` has the same `MAP`, `MEMENTO` and pins). `Loading.zrd` is the loading screen; its
`loading_c61` `PRIMITIVES` equal `escape.zrd`'s, so the chart and the pins are the *same art as the
loading screen*, with `LOADING_SCRIPT` (spinner, `Cycle`) instead of `ESC_SCRIPT`.

| Item | What the data says | Texture (`ZBD/rimage.zbd`) | Size | Decoded-texel SHA-256 |
| --- | --- | --- | --- | --- |
| (a) chart | `loading_c61 PRIMITIVES MAP BITMAP "HA-m1MAP"`, `POSITION [16 19]`, `CLIP topleft [211 51] bottomright [784 551]` | `ha-m1map` | 800x600 | `557cd19cf8aa…449992` |
| (b) pin "?" | `OBJPIN1..3 bitmap "pin6"` | `pin6` | 76x80 | `28ddc10069ad…c6eebf5` |
| (b) pin number | `OBJPIN4 bitmap "pin4"` | `pin1`..`pin8` | 76x80 each | `pin4`: `6941145b8604…f4870ba3`; `pin6_1` (a "6") `5b67169c5219…c078f6` |
| (b) pin shadow | baked into each pin texture (the flag casts it in the same 76x80 picture); no separate texture | | | |
| (c) player icon | `LOADINGDIALOG PRIMITIVES OWNSHIP BITMAP "singledev" CENTER [1]` | `singledev` | 32x32 | `66ce106ccdeb…166df2` |
| (d) note paper | `OBJECTIVESLIST BACKGROUND BITMAP "parchment"` at `[555 6]`; title `MSG_BRF_DLG_OBJECTIVES` at `[595 25]`; list at `[580 50]`, `WORDWRAP [190 255]`, `SPACING [5]`; `CHECKMARK "obj_check1"` | `parchment`, `obj_check1` | 240x312, 25x24 | `c29384c58046…12e0ba`, `7c9fbf758756…f01857` |
| (d) note font | `ObjListTitle` / `ObjList` in `fonts.zrd WINDOWS_FONTS`: face `Andy Bold`, italic, colour (16,16,16), height -16 / -14 (stored as u32 `4294967280` / `…82`), title weight 700 | **not in the installation** (see below) | | |
| (e) photo | `MEMENTO BITMAP "momento_temp"` at `[533 326]`, `REVS [0]`; `Pict MOMENTOSHAD bitmap "momento_shad" at [661 454] center` | `momento_temp`, `momento_shad` | 242x305 (sepia studio portrait), 300x300 | `3d603312aa37…d8585f`, `3a169fa10d58…428305` |
| (f) buttons | `LOADINGDIALOG BUTTONS`: `RESUME_MISSION_BTN [107 528]` `MSG_BTN_RESUME`; `RESTART_MISSION_BTN [127 559]` `MSG_BTN_RESTART`; `CONFIGURE_BTN [367 528]` `MSG_BTN_PREFERENCES`; `MAINMENU_BTN [347 559]` `MSG_BTN_QUIT`. Each: `BITMAP escape_button1`, `ROLLOVER escape_button2`, `ACTIVATE escape_button3`, label offset `[66 7]`, fonts `BtnEscapeNormal` / `Rollover` / `Activate` (Trebuchet MS Bold, colours (10,10,10) / (105,100,85) / (180,175,151), centred) | `escape_button1`/`2`/`3` | 132x28 each | `a67a65885d8c…805371`, `4d85966e9d55…788c`, `2f694946f94c…0e4bd8` |
| (f) cursor | `default CURSOR BITMAP "daglove" CENTER [0] ROLLOVER BITMAP "dafinger"` | `daglove`, `dafinger` | 40x40 each | `434b4dcce0e9…b46d726`, `8a65577ac9bf…1b2c4b07` |

Pins are numbered flags: viewing the textures shows `pin6` is the "?" flag, `pin1`..`pin5`, `pin7`,
`pin8` carry the digit of their name and `pin6_1` carries a "6". Across the 52 chart entries the pins
used are `pin1` x22, `pin2` x16, `pin3` x11, `pin4` x11, `pin5` x4, `pin6` x4, `pin6_1` x1, `pin7` x1,
`pin8` x1. M01's four are three "?" flags and one "4" flag.

Notes on the rest of the owner's description:

* The brass arrow cursor is `daglove` (40x40) with `dafinger` on rollover, in every entry. The
  container carries `ASSETS/GRAPHICS/ACTIVEPOINTERZ.PNG`, `PASSIVEPOINTERZ.PNG` (40x40) and
  `ZOOMPOINTER.PNG` (64x64) as well; their consumer was not examined (not an Esc candidate by name).
  Which cursor art is the "brass arrow" is **not decided** here; the screenshot is the check (#1817).
* The window background `loadframe` (801x601, `BACKGROUND_IMAGES [["loadframe" 0 0]]`) is the frame
  around the chart; `a green aircraft` is `singledev`; `MYZEP` names `NW-m1wv_icon` (49x165) and the
  `Loading.zrd` script also lays `NW-m1pda_icon` (40x149) at `[106 419]`. Their role is not
  decoded: `MYZEP` has a `NW-` (Northwest) name in M01's `HA-` (Hawaii) entry.

## Measured absences

* **Fonts.** `Andy Bold` and `Trebuchet MS Bold` are Windows faces named by `fonts.zrd`
  `WINDOWS_FONTS`. The installation holds no `.ttf`, `.otf` or `.fon` file (`find` over the whole
  installation) and `crimson.rof` has one bitmap font (`ASSETS/GRAPHICS/FONT.TGA`, 4 392 stored
  bytes), unrelated to these names. The handwriting look cannot be loaded from the installation;
  a later task must choose a licensed substitute and measure it against the owner's screenshot.
  Not found; resolving task: #1817 (presenter) with the owner's font evidence.
* **`escapemenu` (640x480), `EM_B_RESUME.PNG`, `EM_B_LOADGAME.PNG`, `EM_B_SAVEGAME.PNG`.** No
  decoded document of `escape.zrd`, `ia_escape.zrd`, `Briefing.zrd` names `escapemenu` or any
  `EM_B_*`, and `ASSETS/LAYOUT.CSV` has no `ESCAPE`, `PAUSE` or `EM_B_` text. `escapemenu` (a red
  pirate-flag backdrop) and the container's `EM_B_*` buttons (221x168 / 224x176) belong to an
  earlier 640x480 escape menu; `Loading.zrd`'s own `LOADINGDIALOG` still names `EM_B_Resume2`,
  `EM_B_Resume3`, `EM_B_Resume4`, which **no** `rimage.zbd` texture carries (dangling names). The
  800x600 Esc screen the owner shows uses `escape_button1..3`. Treat `escapemenu` and `EM_B_*` as
  unused by the shown screen (no consumer found; the engine image has no plain string for them,
  but names may be assembled at run time, so this is a measured absence of a reference, not a proof).
* **`PC_MAPPINS.PNG`** (50x100) is the Passenger Cabin screen's pin panes (`PC_PIN0..4`, five
  rows in `[@PassengerCabin@]` of `LAYOUT.CSV`), not the Esc pins.
* **The "?"/number choice at run time.** `ESC_SCRIPT` names one fixed bitmap per pin
  (`Pict OBJPINn bitmap …`). No field of M01's `objectives.zrd` / `targets.zrd` names a pin or a
  position. M01 starts with objectives that are dormant (`BEGIN_DORMANT`) and `IDENTITY PRIMARY n`
  numbering, which is the likely driver of "?" -> number, but **no data says so**: the switch is
  native. Not found; resolving task: #1817 with owner-captured behaviour.

## (g) `map.zrd` and `map_anims.zrd` of M01

Both decode with the production `.zrd` reader (`cs_content::stunts::decode_zrd`). They describe
a **cockpit map object**, not the Esc chart:

* `map.zrd` (651 bytes): `MAP` with `MAP_NAME map_c3m01`, `OPEN_ANIM open_map`, `CLOSE_ANIM
  close_map`, `MAP_TRANSLATE [0 -0.1 -1.7]`, `MAP_ROTATE [0 0 0]`, `MAP_SCALE [1 1 1]`,
  `MAP_DB_PRESCALE_OFFSET [0 0 0]`, `MAP_DB_SCALE [8.138021e-05 0.0001 8.138021e-05]`
  (= 1/12288, 1/10000, 1/12288), `MAP_DB_POSTSCALE_OFFSET [0.5 0 0.5]`, and one `ICON`
  (`ICON_NAME player_icon`, `DB_NAME player`, `UPDATE_FREQUENCY 1`).
  The scale and offset read as `u = x/12288 + 0.5` over a 12 288 m square centred on the origin;
  that reading is **unknown** (no consumer decoded).
* `map_anims.zrd` (1 630 bytes): two `ANIMATION_DEFINITION`s of `NAME map`, `ANIMATION_NAME`
  `open_map` / `close_map`, `ACTIVATION ON_CALL`, `EXECUTION_PRIORITY 1`, `RESET_TIME
  [4294967295 4294967295]`, one `SEQUENCE_DEFINITION callback_sequence` each, built from
  `OBJECT_ROTATE_STATE` and `OBJECT_MOTION_FROM_TO` on `MAIN_ROOT_NODE`: rotate about the first
  axis 0 -> 60 degrees over 0.75 s to open, 60 -> 0 to close. `OBJECT_MOTION_FROM_TO` and its
  siblings are already in the animation vocabulary (`cs_app::animation::keywords`).
* None of the plain strings `map.zrd`, `MAP_NAME`, `MAP_DB`, `player_icon`, `ICON_NAME` or
  `map_anims` occurs in `$CS_ENGINE_IMAGE` (SHA-256 `43540fc97347…c37d75`, 2.6 MB), and no ZBD
  grep finds a node or texture called `map_c3m01` outside `map.zrd`. So whether the retail engine
  reads these members at all is **unknown**; they are not the pause chart. Every mission reader
  carries a `map.zrd` of 650-651 bytes. Resolving task: none needed for the pause screen.

The Esc vocabulary *is* in the engine image: the plain strings `escape.zrd`, `ia_escape`,
`OWNSHIP`, `MYZEP`, `MEMENTO`, `RESUME_MISSION_BTN`, `RESTART_MISSION_BTN`, `CONFIGURE_BTN`,
`MAINMENU_BTN`, `loading_` and `ESC_SCRIPT` sit together at file offsets 2 266 120 .. 2 266 700 of
the decrypted image, and `CLIP` / `topleft` at 2 251 156 .. 2 251 192 (file offsets, not virtual
addresses; no virtual address was derived here). That is static evidence the engine consumes
`escape.zrd` with these keys.

## (h) Chart-to-world mapping and pin positions

* **Pin positions come from the script, as screen pixels.** `ESC_SCRIPT` `Pict OBJPINn … at [x y]
  center`: M01 `[506 149]`, `[396 254]`, `[181 244]`, `[106 319]`. No world position is attached.
  Neither `objectives.zrd` nor `targets.zrd` of M01 carries a pin, map or screen field; the
  `targets.zrd` nodes resolve to world points (e.g. Site #1 (-4278.5, 146.6, -4760.9) in
  `2026-10-21-m01-objective-target-points.md`) with no pin link.
* **The `WORLD` rectangle.** M01: `topleft [-10818.43 9155.875]`, `bottomright [-2204.118
  540.563]` (a 8614.3 x 8615.3 square). The chart is drawn at `[16 19]`, clipped to
  `[211 51]..[784 551]` (573x500, not square), and the bitmap is 800x600.
  - M01's `location.zrd` places `Lighthouse` (-7898, 280, -5765), `Klondike_start` (-4897, 200,
    -5018) and `Tanker` (-10000, 200, -5000). Their `x` fall inside `[-10818, -2204]`; their `z`
    (-5018..-5765) fall outside `[540, 9155]` and inside it only when negated, so the second
    `WORLD` component is probably `-z` — **unknown**.
  - Which pixel rectangle the `WORLD` corners map onto (the clip rectangle, the bitmap, a
    square sub-rectangle) is **unknown**. Mapping the clip rectangle gives Lighthouse about
    (405, 248) against the pin at `[396 254]`, which is suggestive but one match, and the other
    pins match nothing, so it is not a measurement. The three `Examine` sites (-4278, -4761),
    (-6311, -4611), (-8888, -4687) share `z` within 150 m while the pins' `y` differ by up to
    105 px, so the pins are **not** drawn at the examine sites. They are authored positions.
  - The player aircraft icon (`OWNSHIP`) is the only primitive that needs the mapping (it is
    placed from the player's world position and heading). Its placement rule, rotation sense and
    update rate are not in the data. Resolving task: #1817 must fit the mapping with the owner's
    screenshot (one known player position) and record the fit as `static`, not `verified_original`.

## Not found, and which task finds it

| Missing | Why | Task |
| --- | --- | --- |
| The `WORLD` -> pixel rule and the `z` sign | no engine decode of the `MAP` primitive | #1817 (fit with the owner's capture), or an engine-image decode task |
| Player icon rotation / scale | native code only | #1817 |
| The "?" -> number rule | native objective-state binding | #1817 |
| The `Andy Bold` / `Trebuchet MS Bold` faces | Windows fonts, not installed with the game | #1817 with the owner |
| The `Objective` command's text draw and the check mark placement | `Objective OBJn index n` names an objective; the draw rule is native | #1817 |
| What `MYZEP`/`NW-m1wv_icon` and `PANDORA`/`NW-m1pda_icon` show for M01 | not decoded | #1817 |
| The meaning of the `loading_m*` entries (no chart) | unreferenced here | out of scope |
| Which of the container's cursor PNGs is the brass arrow | not examined | #1817 |

## Verification

```
cargo test --locked -p cs_app --test cs_app_tests_c accept_parity3_pause_map_01_ -- --include-ignored
  -> 5 passed (with CS_GAME_DIR); the same run without it panics at each test's first read
```
