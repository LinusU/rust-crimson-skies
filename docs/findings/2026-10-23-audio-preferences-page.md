# BUG-AUDIO-PREFERENCES-UI: the Preferences → Audio page

Task: #1996. Tests: `crates/cs_app/tests/ui/audio_preferences.rs` (prefix
`accept_bug_audio_preferences_ui_`; the `retail` and windowed ones are ignored
without `CS_GAME_DIR` / a GPU). Provenance of every reading below: `static`
(`ASSETS/LAYOUT.CSV`, `ASSETS/SCRIPTS/AUDIO.SCRIPT`, `RESOURCE.H`, `langui.dll`
of the owner's installation). Nothing here is `verified_original`; no original
executable ran.

## Why the page looked broken

* The five empty boxes are **baked into `AP_BackGround.png`** (566x366): they
  are the title plates for *In-Game Music*, *Music Volume*, *Effects Volume*,
  *Voice Volume* and *Sound Quality*. The `T` rows that write on them, and the
  `Z` sliders and the `D` drop-down that sit on them, were counted by the
  generic shell and never drawn.
* The "broken checkbox" was `AP_B_MUSIC` (`B,PF_B_CheckBoxSmall.png,…,1,0`):
  the shell cut its 45x232 art into four 58-row frames. The art is **eight
  frames of 29** (the four button states unchecked, then the same four with the
  cross), and the row's `Type` column (`1` = check) says so.

## Every row of `[@Audio@]`

| Row | Kind | Reading |
| --- | --- | --- |
| `AP_BACKGROUND` | `P` | pane at 110,215 |
| `AP_B_CANCELCHANGES`, `AP_B_ACCEPTCHANGES` | `B` | `ScriptToExe` `Preferences`, 4-frame strips |
| `AP_B_MUSIC` | `B` | check box, columns 12 (`Type` 1) and 13 (`Checked` 0) once the header's omitted `ResID` is accounted for |
| `AP_T_*` (11) | `T` | `ResID,X,Y,Z,Width,Height,Color,Justify`; `IDS_AP_*` names |
| `AP_D_SQuality` | `D` | 144x17 at 137,505; the data row omits the header's `ScriptPointer` |
| `AP_S_MVOLUME`, `AP_S_EVOLUME`, `AP_S_VVOLUME` | `Z` | 1..=100, authored 50; slot `PF_B_SliderSlot.png` 171x3, knob `PF_B_Slider.png` 43x21 |

Labels (`RESOURCE.H` id → `langui.dll`): AUDIO (1090), In-Game Music (1091),
Music Volume (1092), Effects Volume (1093), Sound Quality (1094), Voice Volume
(1140), and the five descriptions (1095–1098, 1141). `IDS_LOW`/`IDS_HIGH` (106,
107) are the drop-down's items. Each text carries a `[FONT]` tag
(`COUR12B`, `TREB9B`).

## Semantics (`AUDIO.SCRIPT`)

* `callback($$E$$, 2129, 0, DA, FA, GA, HA, CA)` reads quality, music volume,
  effects volume, voice volume and the music flag into the screen.
* Check box and sliders apply **live** (`setvolume(3|5, music)`,
  `setvolume(4|1, effects)`, `setvolume(2, voice)`; off stops the music loop).
* **Cancel** runs `callback($$E$$, 2110)` and re-reads the stored values.
  **Accept** runs `callback($$E$$, 2129, 1, …)` (writes them) and
  `callback($$A$$, 14)`; a changed quality with `$$HB$$` set raises message 712
  (*must quit and restart for sound quality changes to take effect*).
* No Esc handler.

The page follows this: changes are staged and applied to the front end's own
music and cues at once, Cancel restores the committed values, Accept commits and
writes.

## Unknown, recorded rather than guessed

1. **Where the original stores the values.** Registry keys `MusicOn`,
   `MusicVolume`, `SfxVolume`, `VoiceVolume`, `SoundLOD`
   (`2026-10-18-f52-original-options.md`); `callback 2129` itself is unread. The
   values persist in the project's own `audio-settings.txt` in the profile
   directory (`$CS_PROFILE_DIR`, else `~/.crimson-skies`; a smoke's capture
   directory).
2. **The unset defaults.** Unread. The page starts from the layout's `CurrVal`
   (50), music on (the front end plays it today) and quality High: designed.
3. **The drop-down's items and their engine meaning.** `AB.TJ = 2132` and
   `@shareditems@XTA` supply them at run time. Two items, `IDS_LOW` then
   `IDS_HIGH`, are an inference from the README's *Sound Quality High/Low* and
   the description text; the mapping onto `SoundLOD` is unread.
4. **`Z` columns `Left,Top,Right,Bottom` (`0,-10,1,-10`).** Read as the knob's
   offset from the slot's top-left (`Left`, `Top`); `Right`/`Bottom` are unused.
   The knob travels the slot width less its own; the hit region is the slot
   width over the knob height. The original's exact region is unread.
5. **Volume → gain.** `setvolume(channel, 0..=100)` is an engine call; the page
   plays `value / 100` linearly.
6. **Fonts.** `COUR12B.ttf` / `TREB9B.ttf` are not available; text uses the
   engine's default face at the tag's pixel size. The drop-down's ground, frame
   and highlight colours are designed stand-ins.
7. **Checked-frame order.** The checked half repeating the unchecked half's
   state order is read off the art.
8. **The restart prompt** (message 712) has no presenter: Accept stores the new
   quality and reports `quality_changed`; nothing quits.
9. **Keyboard.** The original's focus order is unread. Down/Up walk Music, the
   three sliders, Quality, Cancel, Accept; Left/Right step a slider by 5 or the
   list; Esc closes an open list, else Cancels.

## Not done here (follow-ups)

* **Mission mixer.** The committed effects and voice volumes are stored but no
  mission consumes them (#781 `F52-W2`).
* **In-game Preferences overlay** (`mission_session::preferences_overlay`) still
  records Audio as a navigation it does not present.
* **Video / Game Options / Controls submenus** read the same `T`/`Z`/`D` rows;
  `ui::front_end::widgets` is the reusable reader.
