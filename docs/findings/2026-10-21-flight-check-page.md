# 2026-10-21 — Flight Check page: names, silhouettes, captions, fonts

Task: Rally #1728 `PARITY2-M01-FLIGHT-CHECK-PAGE` — make the Flight Check page
show pilot and wingman names, plane silhouettes and the original button
captions, and record the original typefaces. All facts below were re-measured
in this session; earlier per-session notes live under `private/` only.

## Provenance

- Engine image: `$CS_ENGINE_IMAGE` = `crimson.decrypted.exe`,
  sha256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`.
  All VAs are file-relative (`va = 0x400000 + file_offset`).
- Install: `$CS_GAME_DIR` (retail). Script
  `crimson.rof!ASSETS/SCRIPTS/FLIGHTCHECK.SCRIPT`, layout
  `langui.pif!ASSETS/LAYOUT.CSV`, UI string image
  `GOSDATA/ASSETS/BINARIES/langui.dll`, mission message names `strings.dll`
  (UTF-16), font table `langui.pif!ASSETS/FONTS.ZRD`.
- Owner capture of the original running the M01 Flight Check:
  `private/1728/original.png` (full frame), `private/1728/pilot_line_big.png`,
  `private/1728/wingman_line_big.png`, `private/task1728/frame5.png`
  (silhouette frame). Private, never committed.
- Extracted dumps under `private/fc1728/`, `private/flight-check-task/`,
  `private/task1728/` (private, never committed).

## Original script — FLIGHTCHECK.SCRIPT

`crimson.rof!ASSETS/SCRIPTS/FLIGHTCHECK.SCRIPT`, gui `FlightCheck` ($$T$$=1).
`gui_create` order, with the script's object variable and the layout row its
`YC` names:

| Var | YC (layout row) | Purpose |
| --- | --- | --- |
| LA | `fc_background` | background pane |
| LP | `fc_p_pilotplane` | pilot plane pane; `LP.CK = callback($$E$$,2010,-1)` |
| MP | `fc_b_flymission` | Fly Mission button |
| NP | `fc_b_returnbrief` | Return to Briefing button |
| OP | `fc_b_changeplane` | pilot Change Plane pill |
| PP | `fc_b_changeammo` | pilot Change Ammo pill |
| QP | `fc_t_title` | FLIGHT CHECK caption |
| RP | `fc_t_pilot` | PILOT caption |
| SP | `fc_t_pilotplane` | pilot name text; `callback($$E$$,2011,-1,SP.BC)` |
| TP | `fc_t_mission` | mission line; `callback($$E$$,2009,-1,0x01,TP.BC)` |
| UP | `fc_t_objtitle` | Objectives caption |
| VP | `fc_t_objectives` | objectives text; `callback($$E$$,2022,-1,VP.BC)`, `VP.NJ=1` |
| WP | `fc_t_guntitlep` | GUNS caption (pilot) |
| XP | `fc_t_rockettitlep` | ROCKETS caption (pilot) |
| YP | `fc_t_gunlistp` | pilot gun list; `YP.TJ=2005` |
| ZP | `fc_t_rocketlistp` | pilot rocket list; `ZP.TJ=2006` |
| AQ | `fc_b_changeplanew` | wingman Change Plane pill |
| BQ | `fc_b_changeammow` | wingman Change Ammo pill |
| CQ | `fc_p_wingplane` | wingman plane pane; `CQ.CK = callback($$E$$,2010,-2)` |
| DQ | `fc_t_wingman` | WINGMAN caption |
| EQ | `fc_t_wingmanplane` | wingman name text; `callback($$E$$,2011,-2,EQ.BC)` |
| FQ | `fc_t_guntitlew` | GUNS caption (wingman) |
| GQ | `fc_t_rockettitlew` | ROCKETS caption (wingman) |
| HQ | `fc_t_gunlistw` | wingman gun list; `HQ.TJ=2007` |
| IQ | `fc_t_rocketlistw` | wingman rocket list; `IQ.TJ=2008` |

Branches and dispatch:

- Top of `gui_create`: `switch($$KP$$){case 13: case 17: callback($$E$$,2021)}`.
- `callback($$E$$,2014,-1,-3,0)` / `(…,-2,-3,0)` reset the two selection cells.
- `if (!callback($$A$$,27)) deactivate(AQ); deactivate(BQ)` — no wingman
  record hides the wingman pills (M01 has a wingman: they show).
- `switch($$KP$$){case 13: case 17: deactivate(OP)}` — pilot Change Plane off
  in campaign modes 13/17.
- `int JQ=-1; callback($$E$$,2018,JQ); if (3 > JQ) {deactivate(OP);
  deactivate(AQ)}` — both Change Plane pills hidden when the count the native
  answers is under 3.
- `@globals@AR` is set to `8` then `32` before each list create — the list's
  item pitch parameter.
- Mailbox case `10003`: OP/PP press → `@globals@ZQ=-1`, AQ/BQ → `ZQ=-2`;
  NP → `script_end @flightcheck@` + `mail(11003,…)` + `callback($$A$$,6)`;
  MP → `callback($$A$$,1,1)` + `script_end "flightcheck.script"`.

## Layout — `LAYOUT.CSV`, `[@FlightCheck@]` (line 435)

Section vars: `V2=132` (pilot pill band), `V3=350` (wingman pill band),
`V4=FC_B_PaperButton.Png`, `V5=0xFF354560` (caption colour). Header comment
formats (top of file):

- Pane `P,ArtPath,X,Y,Z,NUMFRAMES,IsRegion?,AlphaType,Volatile?,HelpID`
- Button `B,ArtPath,X,Y,Z,TabOrder,ResID,HelpID,ScriptToExe,…` — in the data
  the ResID slot carries the `IDS_*` *name* and the next slot the goto-screen
  name (`PlaneSelection`, `OrdinanceLayout`).
- Text `T,ResID,X,Y,Z,Width,Height,Color,Justify`
- Text list `A,X,Y,Z,Width,Height,TexTCOLOR,Justify,ItemSpacing`

Rows:

| Row | Data |
| --- | --- |
| FC_BACKGROUND | `P,FC_BackGround.jpg,0,0,1,1,0,0,0` |
| FC_P_PILOTPLANE | `P,FC_PlaneIcons.Png,144,190,0,12,0,1,1` |
| FC_P_WINGPLANE | `P,FC_PlaneIcons.Png,144,408,0,12,0,1,1` |
| FC_B_FLYMISSION | `B,FC_B_FlyMission.png,551,<GX>` — art carries caption |
| FC_B_RETURNBRIEF | `B,FC_B_ReturnToBriefing.png,341,<GX>` — art carries caption |
| FC_B_CHANGEPLANE | `B,<V4>,128,<V2>,0,IDS_FC_B_CHANGEPLANE,PlaneSelection,0x1000,1,…,0xff767676,0xff133d77,0xff374D6B,0xff000000` |
| FC_B_CHANGEAMMO | `B,<V4>,274,<V2>,0,IDS_FC_B_CHANGEAMMO,OrdinanceLayout,0x1000,1,…` |
| FC_B_CHANGEPLANEW | `B,<V4>,128,<V3>,…,IDS_FC_B_CHANGEPLANE,PlaneSelection,…` |
| FC_B_CHANGEAMMOW | `B,<V4>,274,<V3>,…,IDS_FC_B_CHANGEAMMO,OrdinanceLayout,…` |
| FC_T_TITLE | `T,IDS_FC_TITLE,132,36,0,190,24,<V5>,1` |
| FC_T_PILOT | `T,IDS_FC_PILOT,138,102,0,94,20,<V5>,0` |
| FC_T_PILOTPLANE | `T,,236,106,0,300,<STDTEXTH>,<V5>,0` — no ResID, filled by native 2011 |
| FC_T_GUNTITLEP | `T,IDS_GN_GUNS,240,167,0,150,<STDTEXTH>,0xFF222428,0` |
| FC_T_ROCKETTITLEP | `T,IDS_GN_ROCKETS,394,167,0,150,<STDTEXTH>,0xFF222428,0` |
| FC_T_WINGMAN | `T,IDS_FC_WINGMAN,138,320,0,94,20,<V5>,0` |
| FC_T_WINGMANPLANE | `T,,236,323,0,300,<STDTEXTH>,<V5>,0` — filled by native 2011 |
| FC_T_GUNTITLEW | `T,IDS_GN_GUNS,240,382,…` |
| FC_T_ROCKETTITLEW | `T,IDS_GN_ROCKETS,394,382,…` |
| FC_T_MISSION | `T,,136,70,0,400,0,0xFF2D3843,0` — filled by native 2009 |
| FC_T_OBJTITLE | `T,IDS_FC_OBJECTIVES,558,80,0,130,35,0xFF2D3843,0` |
| FC_T_OBJECTIVES | `T,,554,120,0,206,360000,0xFF2D3843,0` — filled by native 2022 |
| FC_T_GUNLISTP | `A,240,184,0,150,16,0xFF121315,0,1` |
| FC_T_ROCKETLISTP | `A,394,184,0,150,16,0xFF121315,0,1` |
| FC_T_GUNLISTW | `A,240,400,0,150,16,0xFF121315,0,1` |
| FC_T_ROCKETLISTW | `A,394,400,0,150,16,0xFF121315,0,1` |

`<GX>` is the bottom button band (global var). `<STDTEXTH>` resolves to the
standard text height (global).

## Native callbacks (image VAs)

| Native | VA | Semantics |
| --- | --- | --- |
| 2009 | `0x40989b` | `id = [0x64b340](position) + 0xd7a - 1 + (arg==2 ? 0x1e : 0)` → `GetString(id,1)` → `SetText`. Long titles live at `3450+position-1`, short at `3480+position-1`. M01 → 3450. |
| 2010 | `0x4094a7` | resolves the slot via `0x40fac0`, answers the live roster record's `plane_id` (+0x2c) → the pane's `CK` frame index. |
| 2011 | `0x409443` | resolves the slot, formats `record+0x04` nickname + `"("` + `GetString(0xbb8 + plane_id)` + `")"` via the composer at `0x4054b0` (`add eax,0xbb8` @ `0x409461`; 0xbb8 = 3000). |
| 2014 | `0x409518` | arg3==0 → `selection_cells[slot](0x6480bc) = call 0x4101d0(slot)` — reset pending selection to the slot's current plane; else copies a 0xcc record. |
| 2018 | `0x4097ea` | `*JQ = call 0x40fd10() - (position∈{0xd,0x11} ? 1 : 0)` — the count of planes the choice offers. |
| 2021 | `0x40942f` | thin wrapper `call 0x405f00` — Change Plane gate for campaign modes 13/17. |
| 2022 | `0x4098d4` | enumerates the mission's objective strings (`0x407670`, stride 0x100), seeds the buffer with `GetString(84)` = `[AB14I]`, concatenates items separated by the literal at `0x61f3cc` = `"\n"`; `0x61f3c8` = `" - "`. |
| — | `0x40f4ff` | shared `SetText(BC, buf, 1)` tail the text natives jump to. |
| — | `0x40fac0` | slot resolver: `-1`/`-2` → live record 0/1 (pilot/wingman); `-3`/`-4` → pending-selection cells. |

Roster records: 25 × 0xcc bytes; `+0x04` nickname, `+0x2c` plane id, `+0x84`
pair mask, `+0x88` guns[4], `+0x98` ammo[4], `+0xa8` rockets[8] — committed in
`data/owner-static/engine-facts.toml` (`flight_check_layout`,
`flight_check_default`).

## Strings — langui.dll rows

| Id | Text |
| --- | --- |
| 84 | `[AB14I]` |
| 506 | `(%1!d!) ` |
| 511 | `]Gypsy Magic` — pilot nickname on the M01 capture |
| 512 | `]The Knave` — wingman nickname on the M01 capture |
| 519 | `%1!d!)` — numbered-item template |
| 1001 | `[TREB9B]Change Plane` |
| 1002 | `[TREB9B]Change Ammo` |
| 1011 | `[TREB15B]FLIGHT CHECK` |
| 1012 | `[TREB13B]PILOT` |
| 1013 | `[TREB13B]WINGMAN` |
| 1014 | `[AB19I]Objectives\n\n` |
| 1015 | `[COUR9B]GUNS` |
| 1016 | `[COUR9B]ROCKETS` |
| 3000–3010 | `Ford Hoplite`, `Focke-Wulf Fw 193 Hellhound`, `Bristol Type 140 Balmoral`, `Hughes Bloodhawk`, `Fairchild F6II Brigand`, `Hughes P21-J MKIII Devastator`, `Hughes-Lockheed Firebrand`, `Curtiss-Wright J2 Fury`, `McDonnell S2B Kestrel`, `William & Colt Peacemaker 370`, `Curtiss-Wright P2 Warhawk` |
| 3450 | `[AB14I]Hawaii - The Lost Treasure of Sir Francis Drake` (M01 long) |
| 3480 | `[AB14I]The Lost Treasure` (M01 short) |
| 3320–3325 | `.30-cal.` `.40-cal.` `.50-cal.` `.60-cal.` `.70-cal.` `No Gun` |
| 3365–3369 | `Slug` `DD` `AP` `EX` `None` |
| 3395–3406 | `Armor-piercing` `High explosive` `Flak` `Sonic` `Flash` `Rear flash` `Smoke` `Choker` `Beeper` `Seeker` `Torpedo` `None` |

`]` on rows 511/512 is a table marker the capture does not render; strip it.
Fly Mission / Return to Briefing captions are drawn into
`FC_B_FlyMission.png` / `FC_B_ReturnToBriefing.png` (no ResID on those rows).

## Fonts — `FONTS.ZRD` `WINDOWS_FONTS` block

`[TAG]` prefixes name font entries; measured faces for this screen:

| Tag | Face |
| --- | --- |
| `TREB9B` / `TREB13B` / `TREB15B` | Trebuchet MS Bold 9 / 13 / 15 |
| `COUR9` / `COUR9B` / `COUR12B` | Courier New (Bold) 9 / 12 |
| `AB14I` / `AB19I` | Andy Bold (italic) 14 / 19 |
| `CSB9I` | Comic Sans MS Bold Italic 9 |
| `PEP16` | Pepita MT 16 (langui 54 is `[PEP16]/font=<PEP16.ttf>`) |

All are proprietary Windows-era faces not shipped as usable font files in the
install. **Open owner decision: license/package the original faces or accept a
stand-in** — follow-up task #1901 (`PARITY2-UI-FONTS`). Interim: the engine
default font draws all rows.

## Silhouettes

`FC_PlaneIcons.Png` is an 80×960 strip = 12 stacked 80×80 frames; the pane's
`CK` = the live record's `plane_id` (native 2010). M01: both records carry
plane id 5 (`flight_check_default` engine facts) → frame 5, the Devastator
silhouette on the capture (`private/task1728/frame5.png`).

## Measured M01 screen (owner capture `private/1728/original.png`)

- Pilot line `Gypsy Magic (Hughes P21-J MKIII Devastator)` —
  nickname row 511 + long name 3005.
- Wingman line `The Knave (Hughes P21-J MKIII Devastator)` — 512 + 3005.
- One `Change Ammo` pill under each column; **no `Change Plane` pills** —
  `3 > JQ` fired (fresh campaign offers < 3 planes; the count source
  `0x40fd10` is unmeasured, the absence is observed).
- Both lists show 8 numbered rows; empty slots draw as a bare `n)`.
- Objectives body shows the `MSG_BRF_M01_OBJn` texts verbatim — each string
  in `strings.dll` carries its own `n)` number (measured at `0x1a7bc`).

## Unknowns carried forward

- Which writer stores the nickname string ids into roster records `+0x04` on
  a fresh campaign (rows 511/512 observed on the capture, writer unmeasured).
- What `0x40fd10` counts for native 2018 (the plane-choice count).
- Button state-frame order inside `FC_B_PaperButton.Png` /
  `FC_B_FlyMission.png` strips (only the rest frame is drawn).
- `VP.NJ=1` on the objectives row (probably multi-line join), unmeasured.
