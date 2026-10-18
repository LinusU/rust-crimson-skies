# M01-RADIO-CUE-INVENTORY: what each sound group M01 can cue resolves to

> **Archive note.** As in `docs/findings/2026-10-15-m01-play-radio-dialogue.md`, the archive measured is `ZBD/C1C/M01/zrdr.zbd`, campaign position `ch2-m01`, work order **M06** "The Red Menace" (`docs/findings/2026-10-17-m01-archive-decision.md`); "M01" below is that historical label.

Date: 2026-10-18. Task: `M01-RADIO-CUE-INVENTORY` (Rally #1470), follow-up of
`M01-PLAY-RADIO` (#1438). Capability used: `retail` (read-only
`$CS_GAME_DIR`), through production code only: `recover_retail_objectives` →
`declared_engine_objectives` → `declared_sound_groups` → `MissionRadio::read`
→ `MissionRadio::plan`, the method the first acceptance test drives. No
original executable ran; nothing here is audible or `verified_original`.

## Method

`declared_sound_groups` returns the union of every `WAKEUP_SOUND_GROUP` and
`COMPLETED_SOUND_GROUP` name the record spells: **34 distinct groups**
(13 + 21 distinct names over 18 + 23 sites, with overlap). Each was resolved
by `MissionRadio::read` and asked through `plan` with a wake cue and with a
completion cue.

## Result

All 34 groups are prepared. `MissionRadio::failures` is empty, no cue
answers `RadioRefusal::GroupNotPrepared`, and no group is a typed refusal
(29 voice groups, 5 music groups, 0 refused). Members are
listed in the table's spelled order, which is the order a conversation plays.
All voice files are `VO_c2-NW-m1_<member>.wav`.

| Group | Kind | Members | Reached members |
| --- | --- | ---: | --- |
| `snd_NW1BSwan` | voice | 5 | `BlackSwan_25`, `Zachary_26`, `BlackSwan_27`, `BigJohn_28`, `Zachary_29` |
| `snd_NW1Fass` | voice | 6 | `Jack_30`, `Zachary_31`, `Jack_31-1`, `Zachary_31-2`, `Jack_32`, `Zachary_33` |
| `snd_NW1FirstHook` | voice | 2 | `Jack_22`, `Zachary_23` |
| `snd_NW1Prim2` | voice | 3 | `WorkersVoyage_8`, `WorkersVoyage_9`, `Tex_10` |
| `snd_NW1Prim5Suc` | voice | 2 | `Zachary_38`, `Sparks_39` |
| `snd_NW1Sec1Suc` | voice | 3 | `BlackSwan_41`, `Zachary_42`, `Zachary_43` |
| `snd_NW1Start` | voice | 2 | `WorkersVoyage_1`, `Zachary_2` |
| `snd_c2-NW-m1_Jack_11` | voice | 1 | `Jack_11` |
| `snd_c2-NW-m1_Jack_21` | voice | 1 | `Jack_21` |
| `snd_c2-NW-m1_Jack_34` | voice | 1 | `Jack_34` |
| `snd_c2-NW-m1_Jack_44` | voice | 1 | `Jack_44` |
| `snd_c2-NW-m1_Jack_7` | voice | 1 | `Jack_7` |
| `snd_c2-NW-m1_Sparks_24` | voice | 1 | `Sparks_24` |
| `snd_c2-NW-m1_Sparks_3` | voice | 1 | `Sparks_3` |
| `snd_c2-NW-m1_Sparks_4` | voice | 1 | `Sparks_4` |
| `snd_c2-NW-m1_Sparks_40` | voice | 1 | `Sparks_40` |
| `snd_c2-NW-m1_Sparks_6` | voice | 1 | `Sparks_6` |
| `snd_c2-NW-m1_Tex_17` | voice | 1 | `Tex_17` |
| `snd_c2-NW-m1_Tex_18` | voice | 1 | `Tex_18` |
| `snd_c2-NW-m1_Tex_35` | voice | 1 | `Tex_35` |
| `snd_c2-NW-m1_Tex_5` | voice | 1 | `Tex_5` |
| `snd_c2-NW-m1_WorkersVoyage_12` | voice | 1 | `WorkersVoyage_12` |
| `snd_c2-NW-m1_WorkersVoyage_13` | voice | 1 | `WorkersVoyage_13` |
| `snd_c2-NW-m1_WorkersVoyage_15` | voice | 1 | `WorkersVoyage_15` |
| `snd_c2-NW-m1_WorkersVoyage_19` | voice | 1 | `WorkersVoyage_19` |
| `snd_c2-NW-m1_Zachary_14` | voice | 1 | `Zachary_14` |
| `snd_c2-NW-m1_Zachary_20` | voice | 1 | `Zachary_20` |
| `snd_c2-NW-m1_Zachary_37` | voice | 1 | `Zachary_37` |
| `snd_c2-NW-m1_Zachary_45` | voice | 1 | `Zachary_45` |
| `music_battlesuccess_sg` | music state request | 6 | `music_battlesuccess1.wav` .. `music_battlesuccess6.wav` |
| `music_missionsuccess_sg` | music state request | 6 | `music_missionsuccess1.wav` .. `music_missionsuccess6.wav` |
| `music_prebattle_sg` | music state request | 6 | `music_prebattle1.wav` .. `music_prebattle6.wav` |
| `music_primaryobj_sg` | music state request | 2 | `music_primaryobj1.wav` .. `music_primaryobj2.wav` |
| `music_secondaryobj_sg` | music state request | 2 | `music_secondaryobj1.wav` .. `music_secondaryobj2.wav` |

Notes:

* A music group's `SOUND_GROUPS` record also spells the `DYNAMIC_WEIGHTS`
  property leaf; the prepare step records it as unresolved beside the `MUSIC`
  marker and `plan` classifies the group as `RadioLineOutcome::Music` before
  that matters. Which track plays stays with `M01-PLAY-MUSIC` (#1463); this
  inventory only pins the track list each state reaches.
* 22 voice groups are single-line wrappers (`snd_c2-NW-m1_<Speaker>_<N>`); 7
  are multi-line conversations (`snd_NW1Start`, `snd_NW1FirstHook`,
  `snd_NW1Prim2`, `snd_NW1Fass`, `snd_NW1BSwan`, `snd_NW1Prim5Suc`,
  `snd_NW1Sec1Suc`).

## Acceptance

`accept_m01_play_radio_every_declared_group_resolves_to_the_recorded_inventory`
(`crates/cs_app/tests/campaign/m01_play_radio.rs`, needs `CS_GAME_DIR`) holds
the table: the declared set must equal it exactly, nothing may fail to
prepare, each voice group must queue exactly its recorded members in order
with a measured length and a speaker, and each music group must stay
`RadioLineOutcome::Music` with its recorded track list.
