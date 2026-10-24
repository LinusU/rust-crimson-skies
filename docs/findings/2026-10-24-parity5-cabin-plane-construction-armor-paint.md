# The Plane Construction hub's Armor and Paint panels

2026-10-24, task #2044 (P5-CONS-C, part of #2036). Implemented by `sonnet-1`.
Static readings of the installation's own data (`ASSETS/LAYOUT.CSV`,
`ASSETS/SCRIPTS/ARMOR.SCRIPT`, `PAINT.SCRIPT`): nothing here is
`verified_original`, the original executable was not run, no disassembly, script
text or original screenshot is committed. No independent review yet. No engine
handler was read for this task.

## What shipped

`crates/cs_app/src/ui/front_end/construction_panels.rs` (module docs hold the
readings), drawn by `construction_hub_view.rs` and wired through `HubPage`
(`construction_hub.rs`) and `window.rs`:

* **Armor** (`ArmorPanel`): the four zone lists `AR_D_POINT0..3` (nose, tail,
  left, right), the panel and zone titles, the armor name row and the
  description box. A click on a closed list opens it, a click on an item picks.
  **Left and right mirror**: the script's mailbox `default` arm answers zone 2
  with zone 3 and zone 3 with zone 2 and copies the choice. Hovering an item of
  an open list (message `10013`) names and describes it without picking it.
* **Paint** (`PaintPanel`): the pattern list, three colour lists each with a
  shade list beside it (a colour pick refreshes its shade list, as `10015` does),
  and three decal lists whose rows are five variants side by side
  (`for (BMA = 0; BMA < 5; ..)`). The captions are the section's `T` rows.
* **Into the draft.** `HubPage::press_at` queues `PanelEdit`s; `apply_edits`
  hands them to the production `ConstructionScreen` (`fit_armor`, `set_paint`).
  The armor zones become `ArmorFitment`s over feed-declared damage-graph nodes; the
  pattern becomes the blueprint's mask and a decal a placement over it.
* **Hub.** A built tab returns `HubOutcome::Shown` and puts up no notice; the
  panel's panes (the rules `AR_P_LINE*`, `PT_P_LINE*`; not the decal sheet, which
  the script deactivates) are added to the hub section while its tab is
  selected. A layout without a panel's rows keeps the marked notice.

## Readings (layout header and scripts, all static)

* `D` row columns, from the header: `Slider, UpArrow, DownArrow, DropUp,
  DropDown, ScriptPointer, X, Y, Z, Width, Height, TotalDisplayed, TabOrder`; the
  data rows omit `ScriptPointer`. The armor lists are 225 by 15 at x 490, y 133,
  180, 228, 275, 13 displayed; the paint lists are listed in the retail test.
  The `Z` column falls with `Y` so an upper list draws over a lower one's box.
* `S` row (the description): `BorderColor, BackColor, Slider, UpArrow, DownArrow,
  X, Y, Z, Width, Height, TabOrder, Color`; the data rows omit `ResID`.
* The decal lists open at `x - 13` and `y + height + 11` (`10025`).

## What stays unknown

* **The lists' contents.** Armor items, patterns, colours, shades and decals
  come from engine tables (`E 2246`, `2229`, `2232` to `2239`, `@shareditems@`)
  no file holds; the panels draw what a caller feeds and nothing for a list
  nobody fed, and the windowed run feeds none (the start flow keeps no per-pilot
  plane). Affected: every entry of both panels. Resolving task: #2052.
* **Which damage-graph node a zone is** (declared by the profile, as #183 found).
* **What the name and description show before a first pick or hover**
  (`E 2220(3, ..)`, `E 2225`, `E 2220(6, ..)`, `NB 1158`: handlers unread). That
  they describe the last picked or hovered item is read from the script's shape.
* **Paint beyond pattern and decals.** `PaintSelection` has no field for colour,
  shade or decal variant, so `PaintChoice` keeps them and they reach no
  blueprint; the pattern-as-mask and decal-as-placement mapping is inferred.
  Whether a custom paint is priced is unmeasured (#183). Resolving task: #2052.
* **Scrolling.** The slider and arrows (`PX_SLIDER`, `PX_UP`, `PX_DOWN`) and the
  description's scroll are not built: items beyond `TotalDisplayed` and text
  beyond the box are unreachable. Affected: lists longer than their visible rows
  (paint colours and the armor list may be).
* **The decal pictures** (`PX_P_Decals.tga`) are not drawn; a decal row is its
  five outlined cells. The lists' own look (`GN_DROPUP`, `GN_DROPDOWN`, colours
  of the box, the picked and the hovered item) is a stand-in shared with the
  Audio page.
* The hover also writes the engine's zone value in the script
  (`callback(2247, 1, ..)` on `10013`); the panel treats it as a preview only.

## Verification

`cargo test --locked -p cs_app -- accept_parity5_cabin_plane_construction_`
(`--include-ignored` with `CS_GAME_DIR`; the task's tests are
`accept_parity5_cabin_plane_construction_armor_paint_*`: nine synthetic and one
retail). The owner-command front-end smoke was not re-run for this task; the
panels' drawing is covered by the headless presenter test only.
