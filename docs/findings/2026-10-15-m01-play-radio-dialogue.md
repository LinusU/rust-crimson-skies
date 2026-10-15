# M01-PLAY-RADIO: which directives fire M01's dialogue, where the voice files live

Date: 2026-10-15. Task: `M01-PLAY-RADIO` (Rally #1438), "Play M01's radio
dialogue and music cues and show objective text". Capability used: `retail`
(read-only `$CS_GAME_DIR`), read through **production code**
(`cs_formats::zbd` reader-archive and sound-archive readers,
`cs_content::stunts::decode_zrd`); no original executable was run, so nothing
here is `verified_original`. Static-code measurements cited below come from
the findings they name, not from this task.

## 1. Which mission directives trigger dialogue and sound

M01's control record (`ZBD/C1C/M01/zrdr.zbd` → `objectives.zrd`, 58 numbered
`OBJECTIVE<N>` blocks) carries two sound keys per block, measured in
`docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`:

| Key | Sites in M01 | Fires when | Measured effect |
| --- | ---: | --- | --- |
| `WAKEUP_SOUND_GROUP <name>` | 18 (13 distinct names) | the block **wakes** (`0x469ce0` → woken channel `0x46cc70`) | request sound group `name` |
| `COMPLETED_SOUND_GROUP <name>` | 23 (21 distinct names) | the block **completes** (`0x46a95b` → completed channel `0x46cc50`) | request sound group `name` |
| `STOP_QUEUED_SOUNDS <name…>` | 3 | a block completes | flag the queued entry with that name |
| `SET_HELP_LABEL` | 2 (both labels are `" "`) | a block completes | give the resolved object a label — M01's label text is one byte |

A `WAKEUP_SOUND_GROUP` naming one of the executable's seven built-in
`music_*_sg` groups (`music_prebattle_sg`, `music_primaryobj_sg`,
`music_secondaryobj_sg`, `music_tertiaryobj_sg`, `music_battlesuccess_sg`,
`music_missionsuccess_sg`, `music_battle_sg`) is a **music state request**,
not an ordinary cue — M01 spells five of the seven over six sites.

The engine model `cs_content::objectives_engine` reproduces both transitions
and emits a `CueEmission` per firing (wake → `CueKind::Wakeup`, completion →
`CueKind::Completed`, plus `CueKind::RoleJingle` for a completed objective
whose `IDENTITY` role code is 1–3, whose group the record does not name).

## 2. Where the voice files live

* **Group table:** `sounds.zrd`, a member of `ZBD/zrdr.zbd` (3.99 MB root
  reader archive). Two sections matter:
  * `SETS` — named sets of rows `[<group name>, <member>.wav, <flags…>]`;
    the row's second element is the archive member. M01's own lines are in
    the `c1cm01` set (45 rows).
  * `SOUND_GROUPS` — 710 records `[<group name>, <child>…]`; a child is a
    group name or a one-element list holding one. `MUSIC` marks a music
    state request; `DYNAMIC_WEIGHTS` and its number are properties beside a
    group, not names.
* **Voice files:** `ZBD/soundsh.zbd` (128 350 792 bytes, 2 521 members) and
  `ZBD/soundsl.zbd` (62 580 330 bytes, 2 520 members). Both hold the same
  `VO_c2-NW-m1_<Speaker>_<index>.wav` members for M01 (46 of them); 2 299
  members of each archive start with `VO_`. Which of the two archives the
  original opens is **unmeasured** (F14-D.7), so the consumer reads the same
  labelled choice the launch sound reads
  (`ZBD/soundsh.zbd`, `LAUNCH_SOUND_CONTAINER`).
* **Encoding:** the archives are mostly MS ADPCM (`soundsh`: 1 954 members
  at 11 025 Hz mono 4-bit block 256, 470 at 22 050 Hz block 512, plus small
  PCM minorities), so a line's length is only known after decoding — the
  consumer decodes each member it will play through
  `cs_formats::zbd::decode_sound_sample` and counts frames ÷ channels ÷ rate.

## 3. The opening conversation, derived

`OBJECTIVE1` spells `BEGIN_DORMANT [2]` and `WAKEUP_SOUND_GROUP
["snd_NW1Start"]`: the measured lifecycle model wakes it two mission-clock
seconds in (`EngineObjectives::advance` → `wake` → `CueEmission`), which is
the first cue a normal M01 run emits.

`snd_NW1Start` is a `SOUND_GROUPS` record whose children are
`"snd_c2-NW-m1_WorkersVoyage_1"` and `["snd_c2-NW-m1_Zachary_2"]`; both are
`SETS` rows mapping to `VO_c2-NW-m1_WorkersVoyage_1.wav` and
`VO_c2-NW-m1_Zachary_2.wav`. **The opening crew conversation is those two
voice lines**, queued in that order. The same chain gives the other M01
groups, for example `snd_NW1Fass` (six lines) and `snd_NW1BSwan` (two).

Speakers come from the member's own spelling `VO_<mission>_<speaker>_<index>`
(`WorkersVoyage`, `Zachary`, `Jack`, `Tex`, `Sparks`, `Zachary`, …) — the
installation's naming, read rather than invented. A member whose name does
not spell that pattern is refused by name instead of being given a speaker.

## 4. Subtitle text: absent, not invented

The root reader's members hold no prose: the longest `Text` value anywhere
in `ZBD/zrdr.zbd` is a path (`..\data\common\images\dialogs\briefing`,
102 bytes), and `Dialog.zrd`/`dlgMessage.zrd` are UI box definitions
(`dlgMessage.zrd` decodes to an empty list). `voice.zrd` (992 bytes) is a
table of integer lists — a voice-index mapping, not text. No key maps a
voice member or a sound-group name to a caption.

Localized text does exist elsewhere (the `RT_STRING` images
`00000409.016`/`00000409.256` and `strings.dll`, F12), and M01's `IDENTITY`
sites carry message ids (`MSG_BRF_NWM1_OBJ1`…`OBJ4`) that the briefing/UI
table builder consumes — but nothing connects those ids to an individual
voice line. **Subtitles therefore stay `None`** on every queued line
(`radio.rs`'s module doc records the same verdict), and resolving the
`MSG_BRF_*` ids to text is the objective-HUD work this task did not reach.

## 5. What landed, and what is still open

* `crates/cs_app/src/mission_session/radio.rs` — the `sounds.zrd` reader (both
  sections), the group expansion (a child keeps every name it spells, however
  deeply the record nests it), the member preparation (identity, speaker,
  measured length) and the cue → line plan;
  `cs_app::mission_session::offer_cues` is the host's whole sound step,
  factored out so the acceptance suite drives the very code the composition
  drives. `MissionRadio` is read in `mission_session::stage_for` from the
  declarations the engine model holds, and `MissionHost::step` publishes each
  tick's answers on `MissionHostTick::radio`.
* Acceptance: two retail members in
  `crates/cs_app/tests/campaign/m01_play_radio.rs` plus the
  `accept_m01_play_radio_*` unit members inside `radio.rs`:
  * `accept_m01_play_radio_the_opening_conversation_is_queued_at_mission_start`
    (`CS_GAME_DIR` only) drives the production path — recovery → engine
    lifecycle model → `MissionRadio::read` → the model's own clock at 1/120 s
    per pass → `ObjectiveLifecycles::take_cues` → `offer_cues` → a real
    `AudioSession`. **Measured on the owner's installation:** the opening
    conversation's two lines reach the radio queue at the 2.0 s wake,
    `VO_c2-NW-m1_WorkersVoyage_1.wav` first (1 137 ticks ≈ 9.5 s), then
    `VO_c2-NW-m1_Zachary_2.wav` behind it.
  * `accept_m01_play_radio_the_composition_queues_the_opening_conversation`
    (`CS_GAME_DIR` **and** `CS_ENGINE_IMAGE`, because M01's player start is
    only known from the owner's decrypted image) runs the whole
    `build_headless` composition and reads the answers off
    `MissionHostReport`. It fails loudly by design when the image is unset,
    so it was **not run on the host that wrote this note** — the reviewer or
    the owner runs it with the image.
* Open, deliberately not guessed here — filed as follow-ups: device-side
  playback and the missing radio tick (`M01-RADIO-DEVICE` #1461), the
  objective HUD and the text behind `MSG_BRF_NWM1_OBJ1..4`
  (`M01-HUD-OBJECTIVE` #1462), and the music state requests
  (`M01-PLAY-MUSIC` #1463).
