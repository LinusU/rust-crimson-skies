# PARITY5-START-FLOW.01: the original's screen-to-screen transitions (#1829)

Date: 2026-10-22. Task #1829 (findings only; part of #1747). Implementer:
`claude-2/claude-1` (Rally); no independent review yet. Capability: `retail`
(read-only `$CS_GAME_DIR`, plus `$CS_ENGINE_IMAGE` read statically). The
original executable was **not run**, so nothing here is `verified_original`;
every engine row is provenance `static`. No code changed. The decoded screen
scripts and the disassembly stay in the ignored `private/`.

## Sources

* **Screen scripts.** `GOSDATA/ASSETS/crimson.rof` members
  `ASSETS/SCRIPTS/{MAINMENU,CAMPAIGN,CAMPAIGNINTRO,PASSENGERCABIN,FLIGHTCHECK,
  SCRAPBOOK,SCRAPBOOK_TOC,PREFERENCES,CREDITS,GLOBALS,SCRIPTLOADER}.SCRIPT`
  (read with `cs-inspect rof --member … --export-dir`). Names are obfuscated;
  button ids are the `YC = "..."` strings, which are `ASSETS/LAYOUT.CSV` row ids
  (decoded member sha256 `b50ea48b…9892583`, as in
  `2026-10-15-f45-b-1-original-layout.md`). Script grammar and the 4-frame
  button strips are in `2026-10-18-f45-b-2-screen-script-handlers.md` and
  `2026-10-21-parity5-cabin-intro-measurement.md`.
* **Engine image** (`crimson.decrypted.exe`, sha256 `43540fc9…c37d75`, VA base
  `0x400000`). Not run; read with a disassembler. VAs below.
* **Owner frames** (Drydock, retail 1.02, 800x600; stills, reference only,
  never committed). A still shows a screen, not a transition: **no frame can
  show what Esc or a button does**, so the transition rules below come from the
  scripts and the image, and the frames only confirm which screens and buttons
  exist.

  | screen | frame |
  | --- | --- |
  | main menu (#1748) | https://locousercontent.com/7Z3km3MCrcAO6szISHeHaCg6a245fu9B6_7YClXCkm1ebvWTMfhOJffArhXOJNFV/original.png |
  | pilot selection (#1748) | https://locousercontent.com/J2wxG-n7wRjBqFL96Ax6-BV0Fhz6xzWtJLLG0VgxfcZkfJI9Fm2IcvciViJjvt1o/original.png |
  | intro movie, two frames (#1749) | https://locousercontent.com/WUnf8nHMlA5q39oAuVNITp7s4dEiag9r9Zha9bPl6yxmi2iYdBFSru6l4vf33zle/original.png , https://locousercontent.com/D7uCBymR0u3kPaeQYekdU347Yzn1FP2m_OvMbh2C2J_Y3OhboJv_OAAts6ubcb9B/original.png |
  | cabin (#1749) | https://locousercontent.com/_-sxh5hQ4ph9TCW27upPQfCRQ4ALnc2eGlr8CSDkxKiEl8W6YxQ2cwXza5Ixnru-/original.png |
  | briefing map (#1750) | https://locousercontent.com/IU35vaxUwdw7X3CZBgdo8TJDvkltJDvzKEiuCMz0oIpZx7yf7g5TIn4xBz_bmg3G/original.png |
  | Flight Check (#1728, #1645) | https://locousercontent.com/q4n16O80a85VV7ox2vfVfW4-h7k9X8L0JTwLSnp290luwHzL3n4_1iFx_TcsdvNA/original.png |
  | Esc pause map (#1740) | https://locousercontent.com/v05ynb4eNq--qHdQcl9r_6XsPBuvXLT186LaAMHg2ZSU8SF0Wlg_wXrlzxaRxQ-x/original.png |
  | crash fireball (#1645) | https://locousercontent.com/jQaarV4T-deIerM4CuOAcZjOHJcU-pVsuJazZAlK90EHF4O3zXzSB2rGWimzUmAq/original.png |
  | debrief, Mission Failed 1:37 (#1655, #1645) | https://locousercontent.com/kA3rJ45vbJjzomsaFfLJD5j7kX3KfMrPseM3lx4_TnxhNC30wgXS7ee941aynJ9V/original.png |
  | debrief, Mission Failed 1:11 (#1655, #1645) | https://locousercontent.com/sbJeFoguQE_N49JJ4aMxO6r4yfGZuEtR6hexFHR9ovgZjpEjRecYEJ6pDbAKN7Y4/original.png |

  The one owner statement about a transition: on the pause map "Esc again
  resumes; selection is by mouse" (#1740, owner text, not re-measured).

## What the scripts are

Each screen is a script with `gui_create` (builds controls), `gui_mailbox`
(answers a button press: message `10003` with `sender` = the button) and
optionally `gui_char` (a key). `script_run X` starts a screen script,
`script_end X` ends it, `activate(@x@)` / `deactivate(this)` show or hide a
script that stays loaded, `terminate` is the process exit statement. A
`LAYOUT.CSV` button row may name a *layout script*: pressing the button starts
that script; the button's own `case` arm, if any, runs as well.

**Key handling is the important negative finding.** Only four screen scripts
define `gui_char`: `CAMPAIGNINTRO` (Esc, Space, Return), `CREDITS` (Esc), and the
two cheat-string readers in `PASSENGERCABIN` (`idaho`) and `SCRAPBOOK_TOC`
(`ispy`). **`MAINMENU`, `CAMPAIGN`, `FLIGHTCHECK`, `SCRAPBOOK` and `PREFERENCES`
have no `gui_char`**: no script maps Esc to anything on them. Whether the edit
box on the Campaign page or the engine's own window procedure handles Esc is
unread (below).

`@globals@RR` in `mail (11003|11004|11005, @globals@RR)` is not a navigation
target: `GLOBALS.SCRIPT` creates it as `@ctl@SK`, the music control (it takes
`music_splash.wav`). 11003 / 11004 / 11005 are read as pause / continue / change
of the screen music; their effect on navigation is nil.

## Engine natives the transitions go through

`callback(group, id, …)` calls the executable. Measured dispatchers (static):

| group | dispatcher | table | notes |
| --- | --- | --- | --- |
| `$$A$$` (campaign session) | `0x407670` | jump table `0x408368`, ids 1..31 | all handlers call methods on the session object at `0x71d57c` |
| `$$E$$` (data / UI queries) | `0x4093a0` | byte index table `0x40f788` + jump table `0x40f5b4` (ids 2100..) and `0x40f518` (ids 2000..) | 2099 at `0x409e96` |

| call | handler VA | what the code does (static) | reading |
| --- | --- | --- | --- |
| `A 1, flag` | `0x4076a7` | reads selected mission `[0x64b340]` and completed count `[0x64b678]`; if selected > completed **and** `flag != 0` calls session method `0x417090(sel-1)`, else `0x417410(sel-1)` | start the selected mission (Flight Check "Fly Mission" passes `1`; the debrief "Replay Mission" passes `0`) — which method is "new mission" and which "replay" is inferred, not decoded |
| `A 6` | `0x40783e` | session method `0x419880([0x64b340]-1)`, which calls `0x427df2` | open the briefing for the selected mission. The same function family names `briefing.zrd` / `BRIEFINGDIALOG` / `brief_c%d%d` at `0x427646` / `0x4276f5` / `0x427bca` |
| `A 7, -2` | `0x40793c` | session method `0x419ce0(arg == -2 ? [0x64b678] : arg-1)` | the query behind the cabin's pin count and the intro gate; its value domain is unread |
| `A 24` | `0x407862` | session method `0x4194c0`: clears a byte, calls `0x5b62b0(0x71d3a0, 0)` | resume from the in-game Preferences ("Return to Game") |
| `A 16` / `A 17` / `A 9` / `A 13` / `A 31` | `0x407af3` / `0x40776c` / `0x407a7e` / `0x4077ac` / `0x40772c` | `A 16` clears `0x648534` (0x318 bytes) then `0x419ad0`, `0x405460`; others call session methods | the Campaign page calls 16 on create and 17 after a delete (refresh of the pilot list); meanings of 9, 13, 31 unread |
| `E 2104, n` | `0x409f27` | `n == -2` selects `[0x64b678]+1`, else `n`; stores clamped to 24 into `[0x64b340]` | select the mission (`-2` = the next one) |
| `E 2151` | `0x40bf80` | `ch = A7(-2)`; returns `ch` if the two words at `0x64cc54 + 840*(ch-1)` and `0x64cca8 + 840*(ch-1)` are both zero, else `0` | the **intro-movie gate** (below) |
| `E 2100` / `E 2101` / `E 2150` / `E 2106` / `E 2108` | `0x40c790` / `0x40c7d7` / `0x40c7f4` / `0x40a29a` / `0x40a2cb` | not decoded | Campaign page pilot-name calls and message-box text |

Engine message handler `0x4084a0` (HUD/UI message queue; ids `0x3e9…`, and 2..8):
message **2** starts the `FlightCheck` script (string `0x61f1b0`, via the
script starter `0x406d30`, priority `0x1000`), message **3** starts
`PassengerCabin` (string `0x61f1bc`). `0x416d80(0)` posts 3 and `0x416d80(1)`
posts 2; `0x419c07` posts 3 after the session method `0x419b30`. This is where
the engine, not a script, moves between the cabin and the Flight Check.

Chapter movies: the filename table `chap1.mpg … chap6.mpg` is at `0x61e68c`.

## Per screen: what each control does

Rows are `Esc` / each button → effect → evidence. "none" means no handler was
found, not that the original was observed to ignore the key.

### Main menu (`MAINMENU.SCRIPT`)

* **Campaign** (`MM_B_CAMPAIGN`, layout script `Campaign`, priority `0x1000`):
  the `case QW` / `case TW` arm shares `deactivate(this)`: the menu is **hidden,
  not ended**, and the Campaign screen starts.
* **Preferences** (`MM_B_PREFERENCES`, layout script `Preferences`): same arm
  (menu hidden). **Instant Action**, **Multiplayer**, **Credits** have no arm
  and only start their layout scripts (`InstantAction`, `MultiPlayerMain`,
  `Credits`, priority `0x1000`); the menu stays loaded behind them (the arm
  that would deactivate it is not run for them — unmeasured whether they hide it
  another way).
* **Quit** (`MM_B_QUIT`, no layout script): `case VW` → `terminate`. Immediate,
  **no confirmation script is run**. `terminate` as a whole-process exit is the
  reading of the keyword (`terminate` string `0x60d59c`, keyword-table entry
  `0x6403c4`, token 24); the VM handler was not decoded.
* **Esc**: none.
* **Saved per pilot**: nothing (no pilot is chosen yet).

### Campaign page (`CAMPAIGN.SCRIPT`) — pilot selection

Controls: edit box `cm_e_name` (default button = Continue), list `cm_l_players`,
`cm_b_start` (Continue), `cm_b_deleteplayer`, `cm_b_cancel`. On create it sets
the cabin's intro latch `@globals@XQ = 1`; in `gui_init`: `A 16`, then
`E 2099(name, …)` fills the list; **Delete is deactivated (message 10000) when
the list is empty**.

* **One text field + Continue, not "New" and "Continue".** Continue (or Enter in
  the field, message `10026`) with a **non-empty name**: `mail 11004` →
  `E 2100(name)`; if it returns non-zero the engine calls `A 9` (new-pilot cue,
  inferred); name `crashcheat!` sets a cheat flag. An empty name → message box
  id **200** (`@globals@OR.DI = 200`, OK only) and the page stays.
* If the pilot's progress `IB` (from `E 2099`) is **below 24**: `script_end
  mainmenu.script`, `script_end campaign.script`, `callback($$UB$$, 2141, 0)`,
  then `script_run passengercabin.script` (priority `0x1000`). **The intro movie
  is not played here**; it plays from the cabin's own create (next section). If
  `IB >= 24` (campaign finished): message box with text `E 2106` and the page
  stays. (A "load.script" branch behind `TB = 0` is dead code.)
* Clicking a list row (message `10015`) copies that name into the field.
* **Delete Player**: sets a confirmation message box (`OR.VR = 3`, id **201**,
  two-button `0x4`); on confirm `E 2100(name)`, `A 17`, re-initialises the list
  and clears the field, focus back on the field. Engine side: the pilot's
  directory is deleted (`0x46b3b0`, below).
* **Cancel** (`CM_B_CANCEL`): `activate(@mainmenu@)`, `script_end
  "campaign.script"` — back to the **still-loaded** main menu.
* **Esc**: none in the script.

### Intro movie (`CAMPAIGNINTRO.SCRIPT`), started by the cabin

* **Esc, Space, Return, or a left-button press** → message `11006`; the handler
  is once-only (`EC`): `script_continue @passengercabin@`, `script_end
  @campaignintro@`. The natural end of the movie reaches the same handler.
  Skip and end are the same transition: into the cabin.
* **Gate (engine, static).** The cabin runs the intro only if
  `@globals@XQ` (set by the Campaign page, and by the debrief journal
  `SCRAPBOOK` when `$$SR$$`) is true **and** `E 2151` is non-zero; `XQ` is
  cleared when the cabin finishes creating. `E 2151` (`0x40bf80`) returns the
  chapter number `ch = A7(-2)` only while the **first mission record of that
  chapter** (stride 840 bytes from `0x64cc54`; words at `+0x00` and `+0x54`)
  is all-zero. The movie is `chap<ch>.mpg`. Reading: **the intro is a
  per-chapter opening played when the pilot enters the cabin and that chapter's
  first mission has no recorded state yet — not a once-per-pilot flag.** There
  is no "intro seen" flag in any script or in the file names the save code
  uses (below). What the two record words mean, and that `ch` is the chapter
  and not another count, is unread: **unknown**; the claim is therefore
  "static reading", and the original run (new pilot vs. returning pilot) would
  settle it.
* Return from Plane Name / Construction / Scrapbook does not set `XQ`, so no
  intro there; the journal debrief does set it (see Results).

### Cabin (`PASSENGERCABIN.SCRIPT`)

Buttons and what they do (layout rects in the cabin measurement finding):

* **Next Mission** (`PC_B_NEWMISSION`, no layout script): `E 2104(QG+1)`, or
  `-2` normally (next mission), or the `idaho` cheat pick; `mail 11003`
  (music); `script_end @passengercabin@`; **`A 6`** → briefing. It is disabled
  (message 10000) when `E 2600` is false.
* **Previous Missions** (`PC_B_PREVIOUS`): layout script `ScrapBook_TOC` (the
  mission list); no arm.
* **Change Memento** (`PC_B_CHANGEMOMENTO`; clicking the framed photo `PC_FRAME`
  presses it too): layout script `MomentoSelection`.
* **Plane Construction** (`PC_B_PLANEX`): layout script `PlaneName`; its arm
  sets two globals to 0.
* **Return to Main Menu** (`PC_B_RETURNMM`): layout script `MainMenu` — a
  **fresh `MainMenu` script** (the Campaign page ended the old one).
* **Esc**: none. (`gui_char` reads typed letters only for the `idaho` cheat.)
* **Save** (`PC_B_SAVE`) exists but is `deactivate`d on create.
* **Saved per pilot**: `E 2104` stores the selected mission in `[0x64b340]`
  (engine memory); no write is visible in the script.

### Briefing (engine-native, no script)

There is no `BRIEFING.SCRIPT`: the screen is `ZBD/zrdr.zbd#Briefing.zrd`
(`2026-10-21-parity5-briefing-map.md`) driven from the engine, entered by
`A 6`. The three buttons' labels are `MSG_BTN_REPLAY_BRIEFING`,
`MSG_BTN_RETURN_TO_CABIN`, `MSG_BTN_GO_TO_FLIGHT_CHECK`. **Their handlers were
not located**: the engine posts message 2 (Flight Check) and 3 (cabin) through
`0x416d80`/`0x4084a0` (above), which is the likely path for Go to Flight Check
and Return to Cabin, but no caller of `0x416d80` was traced to the briefing.
Replay Briefing, Return to Cabin and Esc on the briefing are therefore **unknown
at the engine level**; the owner frame cannot show them.

### Flight Check (`FLIGHTCHECK.SCRIPT`)

* **Return to Briefing** (`FC_B_RETURNBRIEF`): `script_end @flightcheck@`,
  `mail 11003`, **`A 6`** — exactly the tail of Next Mission, so it **opens the
  briefing again from its start** (not a resume).
* **Fly Mission** (`FC_B_FLYMISSION`): **`A 1, 1`**, `script_end
  "flightcheck.script"` — starts the selected mission (cinematic / loading are
  engine-side, unmeasured here).
* **Change Plane / Change Ammo** (`FC_B_CHANGEPLANE[W]` → layout script
  `PlaneSelection`, `FC_B_CHANGEAMMO[W]` → `OrdinanceLayout`): the arm sets
  `@globals@ZQ` to -1 for the pilot's Change Plane / Change Ammo (`OP`, `PP`)
  and -2 for the wingman's (`AQ`, `BQ`), and the layout script starts. Both
  Change Plane buttons (`OP`, `AQ`) are deactivated when `E 2018` returns < 3.
* **Esc**: none. **Quit**: none.

### Flight → Esc pause (engine-native; `escape.zrd`)

Data in `2026-10-21-pause-map-screen-data.md`: buttons Resume, Restart,
Preferences, Quit (`MSG_BTN_RESUME`, `_RESTART`, `_PREFERENCES`, `_QUIT`; the
record names the Quit button `MAINMENU_BTN`). Owner: Esc again resumes.

* **Preferences** reuses `PREFERENCES.SCRIPT` with the in-game flag `$$KW$$`
  set: the main-menu button becomes **`PF_B_RETURNTOGAME`** (layout art
  `GN_B_ReturnToGame.Png` at 455,509) and its arm is **`A 24`** = resume the
  mission. Out of game (`$$KW$$` clear) the same slot is `PF_B_MAINMENU`: arm
  `activate(@mainmenu@)`, `script_end "preferences.script"`. The four option
  buttons (`GameOptions`, `Audio`, `Video`, `ControlsPrefs`) start their
  layout scripts, and the `default:` arm `deactivate(this)`s the Preferences
  page while they are open.
* **Resume / Restart / Quit handlers**: not located. Quit's target
  (main menu, cabin, or confirmation) is **unknown**; the record's
  `MAINMENU_BTN` name is a hint, not a measurement.

### Results: the journal debrief (`SCRAPBOOK.SCRIPT` with `$$SR$$` set)

The mission-end screen is the Scrapbook journal opened in debrief mode: on
create `if ($$SR$$)` it sets `XQ = 1` and `mail 11005` (screen music). Its
buttons:

* **Return to Cabin** (`SB_B_RETURNPC`, layout script `PassengerCabin`): starts
  the cabin script; because `XQ` is set the cabin re-runs the **intro gate**
  (so a new chapter's intro plays here when the gate opens).
* **View All Missions** (`SB_B_TOC`): `mail 20003` → `script_pause
  @scrapbook@`, `script_run`/`script_continue` of `SCRAPBOOK_TOC` (the mission
  list; its own **Return** also goes to the cabin through the layout script).
* **Replay Mission** (`SB_B_REPLAY`, `ZRA`): `FRA = E 2405(0)` (the displayed
  mission); `E 2104(FRA)`; `script_end @scrapbook@`; then **if `FRA == $$KP$$`
  and `$$SR$$`** (the displayed mission is the one just played, in debrief):
  `E 2013(-1, $$OSA$$)` and **`A 1, 0` — relaunch directly**, skipping the
  briefing and the Flight Check; **otherwise** `mail 11003`, `A 31`, `A 6` —
  the briefing for that mission. `SCRAPBOOK_TOC` has the same arm for its own
  Replay button.
* **Best to Date / Most Recent** tabs: `E 2402` calls (100 / 101 / 0) change
  the shown record; Next/Prev page buttons likewise.
* **Esc**: none. **No direct "main menu" button.**
* The Mission Succeeded variant and the tab states are not in the owner's
  frames (recorded unknown, #1655).

### Preferences (from the main menu) and Credits

* **Preferences → main menu**: `PF_B_MAINMENU` → `activate(@mainmenu@)`,
  `script_end "preferences.script"`.
* **Credits → main menu**: `Esc` (`gui_char`) → `script_end @credits@`,
  `script_run mainmenu.script`. Its on-screen button (`QH`) opens a message box
  with `E 2108`; the button's role is not read.

### Quit, everywhere

Only `MM_B_QUIT` runs `terminate`. No other screen script has a Quit button or
calls `terminate`; the pause screen's Quit is engine-native and unmeasured.

## What is saved per pilot

Static strings and code (engine image):

* Directory `SavedGames\<pilot>\` (format `SavedGames\%s` at `0x626fa8` /
  `0x627120`). Files: **`Status.dat`** (`%s\Status.dat`, `0x626f78` / `0x626f88`;
  save chunk `PilotStatus`, `0x6270b4`), **`Mission.<c><mm>`** (`Mission.%1d%02d`,
  `0x627004`) and **`Persist.<c><mm>`** (`Persist.%1d%02d`, `0x627018`) for
  chapter `c` and mission `mm`, **`*.sav`** manual saves (`SavedGames\%s\*.sav`
  `0x61f7cc`, `%s\%s.sav` `0x61fa4c`), **`AutoSave.sav`**
  (`SavedGames\%s\AutoSave.sav`, `0x61f3dc`; used at `0x409dce`), and per-pilot
  custom aircraft under `Planes\%s` (`0x61fa34`). The save container registers
  the sections `Persistence`, `Mission`, `Objective`, `PilotStatus`
  (`0x46bb00`; strings `0x627094…0x6270b4`; `MissionData`, `ObjectiveData`
  `0x627178` / `0x627184`).
* Registry key `SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\1.0`
  (`0x61f0d0`, used at `0x4073df`): settings; per-pilot use not measured.
* **Delete Player** removes the pilot's directory: `0x46b3b0` enumerates
  `SavedGames\<pilot>\*.*` with `0x46b2f0`, deletes every non-directory file,
  then removes the directory and, on failure, reports `Can't delete player
  directory %s (%d)` (`0x626fbc`, `mission.cpp` line 5413).
* The pilot list is the set of pilots under `SavedGames` (`E 2099`, `A 16/17`);
  the enumeration code was not decoded.
* **Not found:** a per-pilot "intro seen" flag, a "current mission" field name,
  and the byte layout of `Status.dat`/`Mission.*` (**unknown**; gates "persisted
  in the original save layout", #1748). The selected mission is engine memory
  `[0x64b340]`, the completed count `[0x64b678]`.

## Comparison with the F45 `TABLE`

`crates/cs_app/src/ui/front_end/mod.rs` `TABLE` (rows read from
`origin/main` at the time of writing). "match" means no change needed on the
measured evidence; "differs" rows are what the follow-up slices (#1830…#1832)
should change or decide.

| TABLE row | original (measured) | verdict |
| --- | --- | --- |
| `MainMenu --Quit--> MainMenu` | `terminate`, no confirmation | match (host quits); the table's "stay" is the machine's way to say so |
| `MainMenu` has no `Back` | no Esc handler | match; **the window must not map Esc to Quit/Back here** |
| `MainMenu --OpenCampaign--> ProfileSelect` | Campaign page; menu hidden, not ended | match |
| `MainMenu --OpenSettings--> Settings`, `Settings --Back--> MainMenu` | Preferences; `PF_B_MAINMENU` reactivates the menu | match |
| `MainMenu --OpenCredits--> Credits`, `Credits --Back--> MainMenu` | Esc on Credits → fresh `MainMenu` script | match; Credits also has an on-screen button (role unread) |
| `MainMenu --OpenInstantAction--> IaSelect`, `OpenLobby` | layout scripts `InstantAction`, `MultiPlayerMain`; their screens not read here | unmeasured |
| `ProfileSelect --NewProfile / ContinueProfile` (stay, set intent) | one name field + Continue; existing name selects, new name creates; empty name → message box 200; no separate New | **differs**: no two-intent choice; Delete Player and its confirmation (201) have no row |
| `ProfileSelect --ConfirmProfile--> Cabin` | Continue → cabin script, **unless** progress `>= 24` → message box (2106) and stay | **differs**: guard missing; the cabin then runs the intro gate |
| (no row) intro movie | cabin create runs `campaignintro` when `XQ` and `E 2151 != 0`; Esc / Space / Return / click skip = same exit to the cabin | **missing screen**: the table has no `Intro` screen between `ProfileSelect` and `Cabin`; the gate is per chapter, not per pilot (static reading) |
| `ProfileSelect --Back--> MainMenu` | Cancel → `activate(@mainmenu@)` | match; **Esc is not bound** in the script |
| `Cabin --OpenBriefing--> Briefing` | Next Mission: `E 2104(-2)` then `A 6` | match; also disabled when `E 2600` is false |
| `Cabin --OpenScrapbook--> Scrapbook` | Previous Missions → `ScrapBook_TOC` | match (name pairing) |
| `Cabin --OpenConstruction--> Construction` | Plane Construction → `PlaneName` (not `PlaneConstruction`) | **differs** (known, F45-A.1) |
| (no row) Change Memento | → `MomentoSelection` | **missing screen** |
| `Cabin --Back--> MainMenu` (+`CloseProfile`) | Return to Main Menu → fresh `MainMenu` script; **no Esc** | match for the button; **Esc must not map to Back** |
| `Briefing --ReplayBriefing / ContinueToFlightCheck / Back--> Cabin` | engine-native; handlers not located | unmeasured (names only) |
| `Recon --Back--> Briefing`, `Briefing --OpenRecon--> Recon` | no script and no button in the briefing record | unmeasured |
| `FlightCheck --Back/Cancel--> Briefing` (Return to Briefing) | `A 6` restarts the briefing; button only, **no Esc** | match for the button; **Esc rows (`Cancel`) have no original counterpart** |
| `FlightCheck --Launch--> Loading` | `A 1, 1`, script ends | match in effect; the cinematic/loading are engine-side, unmeasured |
| `Loading --Cancel--> FlightCheck`, `LoadFailed --> FlightCheck` | none found | unmeasured |
| `Flight --Pause--> Pause`, `Pause --Resume/Back--> Flight` | Esc opens the pause map; Esc again resumes (owner); Resume button | match |
| `Pause --OpenSettings--> PauseSettings`, `PauseSettings --Back--> Pause` | Preferences page with `$$KW$$`: the bottom button is **Return to Game** (`A 24` = resume the mission), not "back to the pause screen" | **differs**: Back from in-game Preferences resumes flight |
| `Pause --AbortMission--> Cabin` | Quit's target unread (record name `MAINMENU_BTN`) | **unknown**; the table's cabin is a guess |
| (no row) Restart | `MSG_BTN_RESTART` exists; handler unread | **missing** row |
| `Flight --MissionSucceeded/Failed--> Results` | journal debrief = Scrapbook with `$$SR$$` | match |
| `Results --ReturnToCabin--> Cabin` | Return to Cabin → cabin; re-runs the intro gate | match; add the gate |
| `Results --Retry--> Loading` (guard `FailedOutcome`) | Replay Mission → `A 1, 0` relaunch **directly** (no briefing / Flight Check) when the displayed mission is the one just played, else `A 31, A 6` | **differs**: the original offers Replay for any outcome; the table's guard and its `Loading` target need the owner's decision |
| `Results --OpenScrapbook--> Scrapbook` | View All Missions → `ScrapBook_TOC` | match |
| `Results --ReturnToMenu--> MainMenu`, `Results --Back--> Cabin` | no main-menu button; no Esc | **differs**: no original control for either |
| `Scrapbook --Back--> Cabin` | `SB_B_RETURNPC` → cabin | match; no Esc |

## Unknown (rule 4), with the claims they gate

* **Esc anywhere in the front end except the intro, Credits and the in-game
  pause.** No script handles it; whether the engine window procedure turns Esc
  into a message (the pause is message 11037 per #1740) or the Campaign edit box
  eats it is unread. Gates "Esc behaves as the original" on every screen.
* **Briefing buttons, the pause screen's Resume/Restart/Quit, Loading failure
  and cancel**: engine-native, handlers not located (the next step is to trace
  callers of `0x416d80` and the `escape.zrd` button table).
* **Once-per-pilot / once-per-chapter intro**: the static reading above; the
  meaning of the two record words and of `A7(-2)` is unread. Needs either a
  decode of `0x419ce0` and the writer of those words or an original run (a new
  pilot, then a returning pilot). Gates the intro wiring (#1839).
* **Byte layout of `Status.dat`, `Mission.*`, `Persist.*`, `*.sav`**; whether
  the pilot list is a directory scan. Gates "pilot persisted in the original
  layout" (#1748); the own profile file stays in use meanwhile.
* **`A 9`, `A 13`, `A 31`, `E 2100`/`2101` exact meanings**, the `UB 2141`
  call, message ids 10000 / 10018 / 10026 (read as disable / enable / Enter).
* **Whether `terminate` is a process exit** (reading only).
* Instant Action, Multiplayer, Momento, Plane Name / Selection, Ordnance and
  Save/Load screens' own transitions (not part of the campaign path).
* The owner frames are stills; the transition rules were not seen in motion. A
  Drydock capture of Esc on the main menu, the Campaign page and the cabin, and
  of Quit on the pause screen, would settle most of the unknowns above without
  more engine work.

## Follow-ups

None filed from here (the rows above name the slices #1830…#1832 that should
consume them); a task to decode the pause/briefing button handlers and the
`0x419ce0` / `0x40bf80` chapter gate is the natural next measurement.
