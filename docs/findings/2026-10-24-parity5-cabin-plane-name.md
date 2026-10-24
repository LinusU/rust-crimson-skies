# The cabin's Plane Construction button: the Plane Name dialog (`PlaneName`)

2026-10-24, task #1958, follow-up of #1841. Implemented by `sonnet-2`. Static
readings of the installation's own data and of the owner-supplied engine
image (`CS_ENGINE_IMAGE`): nothing here is `verified_original`, the original
executable was not run, no disassembly or original screenshot is committed.
No independent review yet.

## What shipped

`PC_B_PLANEX` (layout script `PlaneName`) opens the real dialog instead of the
marked notice (`crates/cs_app/src/ui/front_end/plane_name.rs`, drawn by
`plane_name_view.rs`, wired in `window.rs`):

* `start_flow`: `PlaneName` is paired with `Screen::Construction`
  (`EXTRA_SECTIONS`; `section_of_screen` prefers the extra pairs), and
  `presentation(Construction)` is `Shell`. `Cabin --OpenConstruction-->
  Construction` and `Construction --Cancel--> Cabin` carry the moves.
  `NOT_IMPLEMENTED` lost its Plane Construction entry (Change Memento remains).
* The shell centres the dialog as the script does (below).
* **Cancel / Esc** leave through the table's `Cancel` row, back to the cabin.
* **OK with an empty name** keeps the dialog and shows string 203's text under
  the edit box. **OK with a name** is the way on to the construction screen:
  `PlaneConstruction` is not built, so the marked notice
  (`CONSTRUCTION_NOT_BUILT`, recorded in `not_implemented_seen`) is put up
  over the dialog; closing it returns to the dialog with the name kept.
  The cabin therefore still cannot be released (rule 5) until the
  construction screen exists.
* The owner-command flow smoke visits the dialog (typed name, Cancel):
  `private/evidence/PARITY5-CABIN-PLANE-CONSTRUCTION/` (not committed).

## Readings

`PLANENAME.SCRIPT` (`gui_create`): creates, in order, `px_p_background`, then
`pn_p_background`, `pn_t_title`, `pn_t_default`, `pn_b_ok`, `pn_b_cancel`,
`pn_e_name` (class `IM`), `pn_b_default`, each with `relative = 1` except the
first, and sets `location = ((resx - plate.WB) / 2, (resy - plate.DD) / 2)`.
So every row's `X,Y` is an offset from the centred plate
`PX_PlaneNameBackground.Png`; the backdrop `PX_BackGround.jpg` is absolute.
Integer division is assumed. The shell now applies this when it presents the
section (`plane_name::centre_dialog`), the page applies it to the edit box
and captions.

Layout rows: `PN_E_NAME=E,FONT_PN_EDIT,23,40,0,211,16,16,0xFFFF0020,,0xFFEF0010`
(font, X, Y, Z, width, height, max chars, text colour, frame, caret);
`PN_T_TITLE` and `PN_T_DEFAULT` (`T` rows, `IDS_PN_TITLE`,
`IDS_PN_DEFAULTLOADOUT`); `PN_B_OK`, `PN_B_CANCEL` (`PX_B_OKCancel.png`);
`PN_B_DEFAULT`, a type-1 check box (`MP_B_CheckBox8States.png`) that starts
checked (`Checked? = 1`).

Script messages: `gui_char` Esc → `JOA = 1`, mail 12004. OK (10003 from `NOA`):
empty name → `@globals@OR` message box 203 and `focus(MB)`; otherwise
`mail(10021, MB)` (refusal check), `$$M$$ = 0`, `E 2215(1, name)`, run
`planeconstruction.script`, `mail(12001, @planeconstruction@)`, end. Cancel
→ `JOA = 1`, mail 12004: `E 2014(-1, -3, 0)`, then `$$TGA$$` → multiplayer
main menu, `$$CV$$` → Instant Action, else the cabin. The cabin's arm sets both
to 0.

Engine image, dispatcher `0x4093a0`, `E 2215` (`2215 - 0x834 = 0x73`, byte
table `0x40f788`, jump table `0x40f5b4`, handler `0x40ad9b`, static): with
first argument 1 it copies the string argument to the global buffer
`0x64cb7c` (via the import at `0xa1fc84`, a string copy by its call shape);
otherwise it answers that buffer. The plane's name is that buffer.

## What stays unknown

* **What `PN_B_DEFAULT` does.** The script never reads the check box; the
  engine's use of its state is unread. The page keeps the state and hands it
  to the caller in `NameOutcome::Construct`. Affected: the starting loadout
  of a plane the dialog names. Resolving task: the construction screen.
* **`callback($$NB$$, 4005, …)`** at creation, **`mail(10021, MB)`** on OK and
  **`E 2014(-1, -3, 0)`** on leaving: unread, not modelled; an edit box that
  holds text is always accepted.
* **The `Construction` screen proper** (`PlaneConstruction`, `planeconstruction.script`)
  is not built, so OK with a name ends at the marked notice. Affected: the
  whole blueprint editor. Resolving task: filed as a follow-up.
* **Enter pressing OK** is inferred from `MB.JM = NOA`.
* **Caption faces and colours.** The buttons' four colour columns
  (`0x41A5A500`, `0xFFFFFF92`, `0xFFFFFF00`, `0x37277A00`) are unread, the
  captions are drawn black in the engine's default face and the dark Cancel
  plate reads poorly; `FONT_PN_EDIT` is a GDI id with no face in the
  installation (stand-in face). Message box 203 is a plain line of text, not
  the original's `MessageBox` section.
* **`$$TGA$$` / `$$CV$$`** (Cancel to the multiplayer menu or Instant Action)
  have no producer here: the dialog is only reachable from the cabin.
* **Where the name goes.** The start flow keeps no per-pilot plane; the name
  lives in `NameOutcome::Construct` and the shell's outcome log.

## Verification

`cargo test --locked -p cs_app accept_parity5_cabin` (all cabin suites,
`--include-ignored` with `CS_GAME_DIR`). Task prefix
`accept_parity5_cabin_plane_construction_`: seven synthetic tests (CI) and one
retail test (real rows, the real plate's placement and string 203).
