# PARITY5-CABIN-INTRO.01: the post-pilot intro movie and the PassengerCabin layout

Date: 2026-10-21. Task #1837 (findings only; children #1838-#1842 build on it).
Implementer: `claude-2`; no independent review yet. Capability: `retail`
(read-only `$CS_GAME_DIR`). The original executable was not run: nothing here
is `verified_original`. Tests: `accept_parity5_cabin_intro_01_` in
`crates/cs_app/tests/ui/parity5_cabin_intro_01.rs` (retail, `#[ignore]`d in CI,
failing loudly without `CS_GAME_DIR`). The owner's captures (parent #1749) were
compared by eye and by a pixel score; they stay in `private/`.

## (a) The intro movie is `chap1.mpg`

`GOSDATA/ASSETS/GRAPHICS/MPG/chap1.mpg`: MPEG-1 program stream, 320x240, 30 fps,
3927 coded pictures (130.8 s), MP2 audio 44.1 kHz stereo (130.87 s). The
picture is letterboxed inside the file (image rows 28..=211, 184 rows); the
layout's `250,250` scale (2.5x) fills 800x600 with a 460-row band at y 70..530,
exactly the band of the owner's frames.

Identification: each owner frame was box-filtered to 320x240 and compared (mean
absolute grey difference over the picture, cursor masked) with every third
frame of all ten movies. Man at the window desk: `chap1` frame 462 (15.4 s),
score 6.4; the best frame of any other movie scores 36.1 or worse. Woman in the
jacket: `chap1` frame 942 (31.4 s), score 4.8; others 66 or worse. The frames
are grey (channel spread <= 5), whereas `chap0` (the Aero-tone newsreel) is
sepia. `chap0` is the engine `CHAP0` FMV sequence, not the cabin intro. The
film-scratch look is in the footage itself, not an overlay. Other movie rows:
`CrimFlag.MPG` (looping menu backdrop; the `CM_MOVIE` row under
`[@CampaignIntro@]` names it but the page overrides the file), `chap2..5`
(later chapters), `Final.MPG`, `MSopen1`/`zipper` (engine `INTRO`).

How `chap1` is named: `CAMPAIGNINTRO.SCRIPT` sets `GC.BL = "chap" conv$(FC)
".mpg"`, `GC.YC = "cm_movie"`; the movie control (`@ctl@AL`) plays
`assets\graphics\mpg\` + BL. The layout row is
`CM_MOVIE=M,CrimFlag.MPG,0,0,0,250,250,1,0` (X Y Z 0, scale 250/250, one loop).
So on the owner's capture FC was 1.

## (b) Gate, skip, end

* `CAMPAIGN.SCRIPT` (the pilot/campaign page) sets the global `XQ` (symbol
  `fflag`) to 1 on creation and, on its start path, runs
  `callback($$UB$$, 2141, 0)` then `passengercabin.script`. `INSTANTACTION`
  and `SCRAPBOOK` (when `$$SR$$`) also set `XQ`; `ORDINANCELAYOUT` reads and
  clears it.
* `PASSENGERCABIN` `gui_create`: `if (@globals@XQ) { int FC;
  callback($$E$$, 2151, FC); if (FC != 0) { script_run campaignintro.script,
  0x1999; script_pause @passengercabin@, 2 } } else { mail(11004, @globals@RR) }
  @globals@XQ = 0`. So the intro plays on entering the cabin from the Campaign
  page, only if callback 2151 (`nmissid`) is non-zero, and not on a return from
  Plane Name/Construction/Scrapbook (they do not set `XQ`).
* **Unknown: once per pilot?** Whether 2151 is non-zero only on a pilot's first
  entry, or on each campaign chapter start, is engine state, unread (the
  callback table is #159/#694). Evidence: the script only proves "per entry from
  the Campaign page, gated by FC". Until measured, a faithful implementation
  cannot claim "first time only". Resolving: engine callback 2151 analysis, or an
  original run.
* Skip: `gui_char` `key_escape`, `key_space`, `key_return`, and a left-button
  press, each `mail (11006, this)`. The `11006` handler is guarded by `EC` (once):
  `script_continue @passengercabin@; script_end @campaignintro@`. Natural end
  reaches the same handler (F40-D2).

## (c) PassengerCabin layout (`ASSETS/LAYOUT.CSV`, `[@PassengerCabin@]`)

Panes (`ID=P,art,X,Y,Z,frames,region,alpha,volatile`): `PC_BACKGROUND`
PC_BackGround.png (800x600) at 0,0 z4; `PC_PLANE` PC_PlaneTemp.png (728x339) at
46,69 (the script sets its art to `pc_p_hangar<N>.jpg`, N from callback 2010);
`PC_MEMENTO` (no art; the script sets `scrapbook\<EMA>`) at 169,325; `PC_FRAME`
PC_Mementopicframe.png (93x115) at 169,317 (clicking it mails the Change Memento
button); `PC_PIN0..4` PC_mappins.png (50x100, 5 frames) at (39,541) (0,458)
(7,507) (78,483) (332,445), one per pin count from `callback($$A$$, 7, -2)`.
Also `PC_D_MISSIONS` (dropdown, hidden until the typed code `idaho`) and
`PC_B_SAVE` (deactivated).

Buttons (`ID=B,art,X,Y,Z,TabOrder,Script,Priority,EndScript`). Art is a vertical
strip of four frames, top to bottom: disabled (grey), active (gold), rollover
(orange glow), pressed (red) — seen in the exported art and matching the
header's colour order (`DISABLED, ACTIVE, ROLLOVER, DEPRESSED`).

| Button | Art (strip) | Rect (x,y,w,h) | Layout script | Handler arm in `PASSENGERCABIN.SCRIPT` |
| --- | --- | --- | --- | --- |
| CHANGE MEMENTO | 182x128 | 259,384,182,32 | `MomentoSelection` | none (layout script starts it) |
| NEXT MISSION | 221x168 | 87,504,221,42 | none | `callback($$E$$, 2104, QG+1)` (or `-2`, or the `idaho` mission pick), `mail (11003, @globals@RR)`, `script_end @passengercabin@`, `callback($$A$$, 6)` |
| PREVIOUS MISSIONS | 201x129 | 361,539,201,? | `ScrapBook_TOC` | none |
| PLANE CONSTRUCTION | 200x128 | 452,431,200,32 | `PlaneName` | `$$TGA$$ = 0`, `$$CV$$ = 0` |
| RETURN TO MAIN MENU | 200x128 | 593,561,200,32 | `MainMenu` | none |

Creation order in the script: Change Memento, Next Mission, Previous Missions,
Return Main Menu, Plane Construction, Save. If `callback($$E$$, 2600)` is false
the script mails 10000 to Next Mission (presumably disabling it; message
meaning unread). Z-order, hover/press art and strip order as above.

Compared with the owner's cabin frame: button positions and order match
(Next Mission is the larger gold button at lower left, drawn active);
background, hangar window, radio, photo, map and blueprint are the
background/plane panes.

## Unknowns (recorded, not guessed)

* Callback 2151's value domain and so "first time per pilot" (above).
* The Previous Missions strip is 129 rows: 4x32 plus one row, frame boundaries
  unmeasured (the loader refuses it, `FrameCountUnknown`); its height is shown
  as `?`.
* Native calls `2104`, `2600`, `2010`, `2150`, `$$A$$ 6/7/31`, message numbers
  10000/10003/11003/11004/11006 meanings (engine, #159/#694). The memento art
  path and pin counts come from engine callbacks.
* Original focus/tab order (no data column; stale header comment).
* Plane Name (`PlaneName`) is what the original's Plane Construction button
  opens; the layout's `PlaneConstruction` section is a different screen whose
  entry was not measured.
* Intro-movie scratch/grain: part of the footage; no overlay measured.
