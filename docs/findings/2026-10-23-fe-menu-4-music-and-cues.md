# FE-MENU-4: the front end's music and click/rollover cues

Date: 2026-10-23. Task: #1836 / FE-MENU-4 "Front-end presenter 4/4: music and
click/rollover sound cues for the main menu and pilot screens"
(`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`). Shared
contract: `docs/contracts/CLI-EVIDENCE.md`. Required capabilities: `retail`,
`gpu`, `audio` (`CS_CAPABILITIES=retail,gpu,audio`). Test prefix:
`accept_fe_menu_4_`. Evidence: `docs/findings/evidence/FE-MENU-4.json`,
artifacts in `private/evidence/FE-MENU-4/` (git-ignored).

Implemented by `claude-1`. This is the implementer's record, not a review; it
awards no `verified_original`/`release_approved`. `retail` means read access to
the installation: no agent ran `crimson.exe`. The audio claim stops at the
source boundary (the output was asked to play the members and accepted every
command); nobody listened (`human_review` is the owner's).

## What the original data says (static readings)

* `ASSETS/LAYOUT.CSV` has **no `W` data rows**: the header documents the sound
  object format (`ID=W,WAVFileName,Channel,Volume,LoopCount,Autostart?`) and no
  section writes one. The sounds are in the scripts.
* `ASSETS/SCRIPTS/GLOBALS.SCRIPT` (the script every screen shares), in its
  `gui_create`:
  * six `sound` declarations under `assets\sounds\`: `mouseclick.wav`,
    `mouseover.wav`, `mouseclick.wav`, `mouseover.wav`, `entertext.wav`,
    `entertext_error.wav` (variables `DR`..`IR`);
  * `object RR = @ctl@SK` (the music control, per
    `2026-10-22-parity5-start-flow-transitions.md`) with `RR.UK =
    "music_splash.wav"`, `RR.WK = 5`, `RR.XK = <level>`, `RR.ZB = 0`, then
    `initialize(RR)`. The level is `$$PR$$ * 100`, forced to 0 when `$$CA$$` is
    false. The music therefore starts when the front end starts, not on one
    screen.
  * `PASSENGERCABIN.SCRIPT` mails `11003`/`11004` to the control on
    `gui_pause`/`gui_continue` and when leaving the cabin.
* The members are in `crimson.rof` as `ASSETS/SOUNDS/{MOUSECLICK,MOUSEOVER,
  ENTERTEXT,ENTERTEXT_ERROR,MUSIC_SPLASH}.WAV` (cues 676-1188 bytes; the music
  2 312 360 bytes, stereo 22 050 Hz). `ZBD/soundsh.zbd` and `soundsl.zbd` also
  hold a `music_splash.wav` (2 313 294 bytes in the high archive).
* No screen script plays the six sounds: the playing is the executable's
  button and edit control classes.

## What this stage built

* `sound.rs`: `MenuSoundTable::parse` reads the declarations and the music
  control from `GLOBALS.SCRIPT`'s text (nothing typed in; a script without them
  is a named `MenuSoundError`). `MenuSamples::load` decodes the members through
  the production WAVE decoder into the audio device's sample library.
  `MenuAudio` plays the music as a looping voice on `AudioBus::Music` and each
  cue as a one-shot on `AudioBus::Ui` over any `AudioDevice`; a refusal is
  recorded by its code, never silent.
* `window.rs`: the presenter starts the music in its first frame, plays
  *rollover* when a button newly takes the pointer or focus, *press* when one
  is newly held down, *text accepted/refused* for each typed character on the
  pilot page. A screen change (and the first frame) moves the focus without a
  cue. Without `audio` (or without an output device) the run presents silently
  and the smoke report says why; with an output the smoke fails unless the
  music and the three cues arrived. `report.json` gains `music_started` and
  `audio`.
* `retail.rs`: `rof_member_bytes`/`rof_member_names` (wiring-sized readers over
  the screen container; `script_text` now delegates).

## Unknown, and not claimed

* **Which control plays which declared sound, and when.** The cue each file
  serves is read from its name; press versus release, what the second
  click/over pair (`FR`/`GR`) is for, and whether disabled buttons ring are
  unknown.
* **The music's loop count and level.** The control's numeric properties are
  kept verbatim and unread; the music loops at unit gain (a design choice) and
  `$$PR$$`/`$$CA$$` are not mapped to the settings.
* **Which `music_splash.wav` the control opens.** No directory is written;
  the container's copy is played, the archive the fallback.
* **Pause/continue (`11003`/`11004`/`11005`)**: the cabin is not presented yet
  (PARITY5-CABIN-INTRO.*).
* **`crimflag.mpg`'s own MP2 track** is not played.
* **Typing cues for Backspace** and the page's list/arrow selection: the data
  states nothing.

## Reproduce

```sh
mkdir -p private/evidence/FE-MENU-4
CS_EVIDENCE_DIR=$PWD/private/evidence/FE-MENU-4 \
CS_FE_MENU_SMOKE_OUT=$PWD/private/evidence/FE-MENU-4 \
  cargo test --locked -p cs_app --test cs_app_tests_c -- accept_fe_menu_4_ --include-ignored
```
