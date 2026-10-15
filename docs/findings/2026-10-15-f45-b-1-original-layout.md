# F45-B.1: the original front-end layout (`ASSETS/LAYOUT.CSV`)

Date: 2026-10-15. Task: #742 / F45-B.1 "Import the original front-end screen
artwork and hotspot layout" (follow-up of #193 / F45-B; context
`docs/findings/2026-10-07-f45-b-original-asset-screen-decks.md`). Capability
used: **`retail`** (read-only `$CS_GAME_DIR`). The original executable was not
run, so nothing here is `verified_original`. Implementer: `claude-2`; no
independent review yet.

## Where the original defines its screens

Not `ui_resource`/`image` rows (the baseline holds none) and not `rimage.zbd`.
`GOSDATA/ASSETS/crimson.rof` stores one plain-text member, `ASSETS/LAYOUT.CSV`
(15 078 bytes stored, 56 148 decoded), next to the 61 `ASSETS/SCRIPTS/*.SCRIPT`
screen scripts and the `ASSETS/GRAPHICS/*.PNG|JPG` art
(`cs-inspect rof --container GOSDATA/ASSETS/crimson.rof`). The patch container
`crimptch.rof` holds only `AIRFRAME.SCRIPT`, so it overrides no layout.

The installation is `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`;
the decoded layout member's SHA-256 is
`b50ea48bbe97ea098d76575115629bfbe6d0f55165fa364c939f9fb089892583`
(`docs/findings/evidence/F45-B.1.json` references the artifact that records
both).

The file holds `[GLOBALVARS]` and 34 `[@Name@]` sections (the script names:
`MainMenu`, `PassengerCabin`, `FlightCheck`, `ScrapBook_TOC`, `Preferences`,
`PlaneConstruction`, …) with **106 pane rows and 119 button rows** plus text,
list, edit-box, slider and movie rows (counted, not read). Its own header
comment documents the row kinds; the measured facts below are what the data
shows.

## What was measured

- **A button has no stored size.** A `B` row is `ID=B,art,X,Y,Z,ResID,Script,Priority,EndScript,…`:
  the position and the script it starts, nothing else. Its region is its art's
  own extent. The art is a vertical strip of equal frames: `MM_B_Campaign.png`
  is 242x200 and shows four stacked frames (disabled, active, rollover,
  depressed — the four colour sets the header and `[GLOBALVARS]` name), so the
  button is 242x50. `BUTTON_STATE_FRAMES = 4` is that reading, kept as a
  declared constant; an art whose height it does not divide
  (`PC_B_PreviousMissions.png`, height 129) or whose name says `8STATES` is
  **refused** (`FrameCountUnknown`), not rounded.
- **The header comment is stale.** It lists `TabOrder` and `HelpID` columns
  before `ResID`; no data row has them (`FC_B_CHANGEPLANE` carries the
  `IDS_FC_B_CHANGEPLANE` resource string where `TabOrder` would be). So the
  **original focus/tab order is not in the data** and stays unknown; the
  loader keeps declaration order, as F45-B did by design.
- **Screens are panes plus buttons over an 800x600 surface.** The front-end
  backgrounds are 800x600 (F45-D measured it; the loader asserts the art it
  builds is 800x600 on this installation). A screen's art is taken as the one
  single-frame pane placed at `0,0`; zero or several candidates are refused
  (`NoFullPane`, `AmbiguousFullPane`), nothing else is promoted.
- **`<NAME>` placeholders** (`<GX>`, `<Y>`, `<V4>`…) are defined by the
  section's own `V#=NAME,value` rows or by `[GLOBALVARS]`; the loader
  substitutes them (section first) and keeps an undefined one as an
  `UnresolvedPlaceholder` instead of inventing a number. None of the six paired
  sections hit one on this installation.
- **A button's transition is the script it starts.** `PC_B_RETURNMM` starts
  `MainMenu`; `SBTOC_B_RETURN` starts `PassengerCabin`; `MM_B_PREFERENCES`
  starts `Preferences`. The remaining handlers (for example the main menu's
  quit) live in `ASSETS/SCRIPTS/*.SCRIPT`, which this task does not read.

## What the loader does (`crates/cs_app/src/ui/front_end/original.rs`)

`FrontEndScreens::original_screens()` parses the member and, for each
**designed** screen paired with an original section (`SCREEN_SECTIONS`, a
*name-based design choice*, not an original fact), builds `ScreenAssets`
through the existing `ScreenLayout`/`ScreenAssets::new` rules and validates it
as a `ScreenDeck`. A button becomes a hotspot only when the script it starts is
a paired section **and** the state table has exactly one user row from this
screen to the paired one; every other button is listed with its reason
(`UnboundReason`). The deck's own checks (`DeckError`) then judge the result;
none was relaxed.

## What this installation supports (retail test `accept_f45_b_1_retail_…`)

| designed screen | original section | outcome |
| --- | --- | --- |
| `Scrapbook` | `ScrapBook_TOC` | **in the deck**: art 800x600, one hotspot (`Back`, 200x32 at 593,561). Its other three buttons have no script and are unbound. |
| `Cabin` | `PassengerCabin` | assets built; **deck refuses** `HiddenAction(OpenBriefing)`: only `Back` binds (to `MainMenu`); `PC_B_PREVIOUS` is refused (129-pixel strip), `PC_B_PLANEX` starts `PlaneName`, `PC_B_CHANGEMOMENTO` starts `MomentoSelection`, neither paired. |
| `FlightCheck` | `FlightCheck` | assets built; **deck refuses** `NoEscapeButton`: no button starts a paired screen. |
| `Construction` | `PlaneConstruction` | assets built; **deck refuses** `NoEscapeButton`. |
| `MainMenu` | `MainMenu` | **refused** `NoFullPane`: its panes are the logo and a button frame; the backdrop is a movie row, not art. Only `Preferences` binds (`OpenSettings`). The original menu has Campaign / Instant Action / Multiplayer / Preferences / Credits / Quit, which does not match the designed `NewProfile` / `ContinueProfile` / `OpenSettings` / `Quit`. |
| `Settings` | `Preferences` | **refused** `NoFullPane`: the section's panes are placed over the main menu's backdrop (`PF_BACKGROUND` is at 110,215). |

So **one** original screen enters the deck, and it is not a faithful screen:
three of its four buttons are not hotspots. The other five stay out because the
designed table and the original screens are different shapes, which is a
finding about F45-A's table, not a loader fault.

## What stays unknown (and which claims it gates)

- **The original focus/tab order** — no column carries it. Gates any claim that
  keyboard/controller focus matches the original.
- **The handler of every `NoScript` button** (Quit, Fly Mission, Return To
  Briefing, View, Replay, …) — in the plain-text
  `ASSETS/SCRIPTS/*.SCRIPT` files; reading them is the way to bind more
  buttons. Gates "every visible original button works".
- **Screens with no counterpart** in the designed table: Campaign (profile
  list), Instant Action, Multiplayer, Credits, Save/Load, Momento selection,
  Plane name/selection, Ordinance layout. Gates any claim that the designed
  front end reproduces the original's screen set.
- **The 129-pixel `PC_B_PreviousMissions.png` strip** — frame count unknown.
- **The `M` (movie) rows** that back the main menu — not read; no backdrop
  exists for `MainMenu`/`Settings`.
- **A screen-art rule other than "single pane at the origin"** — a design
  rule of this loader, not the original's.
- Whether the **designed name pairing** is right (`PassengerCabin` = Cabin,
  `ScrapBook_TOC` = Scrapbook, `Preferences` = Settings,
  `PlaneConstruction` = Construction) — a design choice awaiting owner review.
- F45-B's review note on duplicate hotspots: the layout declares no two
  buttons for one action on a built screen, so that rule needs no revision
  from this data.

## Fidelity gating

Nothing here is `original-verified`. A fidelity claim about the front end's
screens, hotspots or focus order must cite this record's unknowns. The
evidence is `docs/findings/evidence/F45-B.1.json` (claim `implemented`).

## Follow-ups

Filed with `create_tasks`: read the screen scripts for the `NoScript` buttons;
reconcile the designed state table with the original screen set.
