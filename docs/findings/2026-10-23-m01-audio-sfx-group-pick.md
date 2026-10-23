# M01 audio timeline: how the engine picks one member of a `DYNAMIC_WEIGHTS` sound group

Task #1939 (`PARITY6-M01-AUDIO-SFX-PICK`, unblocks #1865). Capabilities:
`retail`, `CS_ENGINE_IMAGE`. Provenance of every row: `static` (read from the
decrypted engine image; nothing is `verified_original`; no original run).
No disassembly is committed. The engine facts are `[[audio_fact]]` rows dated
`OWNER-STATIC-2026-10-22` (the claim id every audio row shares) and the VO decision is `[[audio_vo_line]]` rows of
`data/owner-static/engine-facts.toml`. Companions:
`2026-10-22-m01-audio-burst-triggers.md` (the sizes and the 59 play sites),
`2026-10-16-m01-play-music-state-requests.md` (the same selector seen from the
music side).

## Result in one paragraph

**There is one group mechanism, not a music one and an effects one.** A sound
group (`*_sg`, the per-pilot `snd_GL-*_id<N>_random`, the music state groups)
is a record of kind 1 built by the single parser `0x599ae0`; playing any
handle whose first dword is 1 starts a *group voice*, and the voice picks its
member with the selector `0x59a440` (the routine the music finding decoded).
For an effect group such as `bullet_hit_sg` or `air_mixed_exp_sg` the pick is a
**weighted draw over members that start with equal weight**, from the C
runtime `rand()`, with a **soft repeat penalty** (the picked member's weight is
halved, then all weights are renormalised to 100). It is not a shuffle, not a
cycle and not "never the same twice". The stream is the process-wide `rand()`
seeded with `srand(time(NULL))` (or `-seed N`), so *which* member the original
played at a given moment is not recoverable statically; the owner's capture is
one realisation of it.

## The chain, step by step

1. **Handle kind** (`group.handle_kind`). `0x599ae0` callocs `0xf4` bytes,
   stores kind `1` at `[handle]` and parses the record at `handle+4`. The keyword
   string `DYNAMIC_WEIGHTS` (`0x63af8c`) is referenced from exactly one place
   (`0x599cf0`, in this parser; image-checked), so effect groups use the music
   group parser.
2. **Name to handle** (`group.lookup_by_name`). `0x596120(name)` searches the
   single-sound list first and otherwise returns `0x599a50(name)`, the lookup in
   the group list. A weapon `IMPACT` row, an effect sequence `SOUND` event, the
   player hit routine (`player.zrd` `bullet_hit_sound`) and the pilot-voice table
   all hold such a handle.
3. **Play** (`group.play_dispatch`). `0x593590(handle, gain)` sends a kind-1
   handle to `0x59ab40`, which forwards to `0x59ab60`; `0x59ab60` allocates a
   `0x64`-byte voice (`group.voice_start`), and the 3D helper `0x593b80` is
   reached only for ordinary single sounds. Only two thin wrappers call
   `0x59ab60` (image-checked).
4. **Pick** (`group.voice_init_pick`, `group.voice_init_call_selector`). The
   voice's state-0 step `0x59a3e0` calls the selector once and stores the
   member in `[voice+0x40]`. A group with no member ends at once. The selector
   has exactly **two callers** (image-checked): this one and the state-2 step
   `0x59a820` (`group.voice_repick`), which re-picks after the group's
   `DELAY_REPEAT`. A group without `REPEAT` (the default is 0;
   `group.voice_repeat_count`) therefore makes **one pick and plays one member
   per call**. All the SFX groups named here spell neither `REPEAT` nor any
   other keyword (retail test below).
5. **The draw** (`group.selector_single_member`, `group.selector_total`,
   `group.selector_draw`, `group.selector_walk`):
   * one member: returned without a draw (if its play-count word is nonzero);
   * otherwise `total` = sum of the weights of members whose play-count word
     `[m+2]` is nonzero (effect members have `0xffff`, i.e. all of them,
     `group.member_play_count_default`);
   * `r = rand() * (1/32767) * total` (`rand` is the MSVCRT import slot
     `[0xa20350]`; the constant `0x38000100` is **1/32767**, not 1/32768, so `r`
     lies in `[0, total]`);
   * members are walked in table order accumulating weights; the first member
     whose running sum plus `0.001 * total` is **not below** `r` is picked. Ties
     go to the earlier member; if the walk ends with no pick (only a degenerate
     all-zero group) the selector returns NULL and the voice ends silently.
6. **Initial weights** (`group.default_weights`, `group.weight_keyword`). After
   a group's members are parsed, if any member weight is not above `0.0001`
   (an unset `WEIGHT` is 0) **every** member gets `100 / count`; otherwise the
   spelled `WEIGHT`s are kept. Retail: all 472 `DYNAMIC_WEIGHTS` records of
   `sounds.zrd` spell no `WEIGHT`, `PLAY_COUNT` or `REPEAT`, so every group
   starts equal (`bullet_hit_sg` 4 x 25, `air_mixed_exp_sg` and
   `ground_mixed_exp_sg` 5 x 20, `plane_destroy_sg` 4 x 25).
7. **After a pick, only when the group has `DYNAMIC_WEIGHTS`**
   (`group.dynamic_weights_parse`, `group.selector_pick_store`,
   `group.selector_floor`, `group.selector_normalise`): the picked member's
   weight is multiplied by the group's factor (clamped to `0..1`; 470 retail
   groups spell **0.5**, two spell 0.2), every *other* eligible member below
   `0.001` is raised to `0.001`, and all eligible weights are multiplied by
   `100 / sum`. The weights live in the group record, so they are **shared by
   every caller of the group** (all `bullet_hit_sg` plays, from the player hit
   routine and from the weapon rows, advance one set of weights) and persist for
   as long as the record does. Whether the group list is rebuilt per mission
   (which would reset them) was not established.

### Repeat behaviour, with numbers

For a 4-member group (`bullet_hit_sg`, `plane_destroy_sg`) after member X is
picked: X 14.29, each other 28.57 (percent of the next draw). For 5 members
(`air_mixed_exp_sg`): X 11.11, each other 22.22. Picking X again is therefore
possible (a repeat) but about half as likely as for the others; a second pick
lowers that member further, and weights relax toward equal only because every
pick renormalises. Nothing excludes the previous member outright.

## Member order and what it means for the burst (#1865)

Members are table order of `sounds.zrd` (retail test below):

| Group | Members (in order) |
| --- | --- |
| `bullet_hit_sg` | `snd_ricochet1` (`ricco4`), `snd_ricochet2` (`ricco1`), `snd_ricochet3` (`ricco3`), `snd_ricochet4` (`ricco2`) |
| `air_mixed_exp_sg` | `snd_exp_hit1`, `snd_exp_hit2`, `snd_exp_hit3`, `snd_exp_hit3a`, `snd_exp_hit5` (`explosion_7`) |
| `ground_mixed_exp_sg` | `snd_exp_hit1`, `snd_exp_hit1g`, `snd_exp_hit2`, `snd_exp_hit3`, `snd_exp_hit3g` |
| `plane_destroy_sg` | `snd_exp_plane1`..`4` |
| `bullet_warning_sg` | `snd_bulletpass1`..`3` |

The three ricochet sizes of the burst are three different members of
`bullet_hit_sg` (22,264 `ricco4`, 16,192 `ricco3`, 18,216 `ricco2`), all of
which the selector can produce on any play. For #1865: implement the selector
as above with its own `rand` stream; do **not** hard-code the capture's member
order as if it were forced, and do not use `(rand*count)>>15` (that is the
weapon-row pick `0x5ad100`). The original seeds with `time(NULL)`, so a fixed
seed for the smoke is a project choice, not original behaviour.

## The `VO_*` lines that share a burst size

Rows `[[audio_vo_line]]` (nine lines, sizes 267,168, 78,936 and 220,616 B).

* M01 is "The Lost Treasure", `ZBD/C3/M01` (`2026-10-17-m01-archive-decision.md`).
  Its objective record cues 25 sound groups (21 dialogue groups
  `snd_HI1*` / `snd_c1-HA-m1_*` and the 4 music state groups), read by the
  production `recover_retail_objectives` -> `declared_engine_objectives` ->
  `declared_sound_groups`. Expanded through `sounds.zrd`, every file they reach
  is a `VO_c1-HA-m1_*` line or a `music_*` track and **none decodes to any of
  the six burst sizes** (retail test below).
* The engine builds dialogue names itself only as `snd_%s_id%d_random` /
  `snd_id%d_%s` (`pilot.voice_name_random`, `pilot.voice_name_fallback`), with
  `%s` one of 29 cue names (`GL-AllyDwn-A` is entry 22,
  `pilot.voice_cue_allydown`). Those resolve to `VO_id<N>_*` lines only.
* Result: **both `VO_*` lines of 267,168 B are excluded**
  (`VO_c1-HA-M5_Zachary_23-02`, `VO_c2-NW-M5_Zachary_19-5`: other missions'
  dialogue, named by nothing M01 or the engine builds). With `train_derail.wav`
  (a looped group nothing starts) already excluded, **267,168 B is
  `explosion_7.wav`** (`snd_exp_hit5`, a member of `air_mixed_exp_sg`).
  The four other-mission lines of 220,616 B are excluded likewise, and the two
  `VO_c5-MN-m1_Zachary_*` lines of 78,936 B.
* **Not excluded:** `VO_id1_GL-AllyDwn-B.wav` (78,936 B) is member B of
  `snd_GL-AllyDwn-A_id1_random` (`DYNAMIC_WEIGHTS 0.5`, members A, B, C), which
  the engine plays when a pilot with id 1 reports an ally down. So 78,936 B is
  `exp_water_b.wav` (the `touchdown_water` contact) **or** that callout; a
  static decision needs the pilot id of M01's aircraft (below).

## Unknown (rule 4) and who can resolve it

| Unknown | Affected content | Resolving work |
| --- | --- | --- |
| The pilot id (`%d` of `snd_..._id%d_...`) of M01's AI aircraft, and whether an ally goes down at 40.5-48.5 s | the 78,936 B burst buffer (`exp_water_b` vs `VO_id1_GL-AllyDwn-B`) | decode the writer of the pilot-voice records at `0x71db84` (stride `0x7c`, id in the first dword) and M01's aircraft records; or the owner's log of the cue |
| Whether the group list (and so the weights) is rebuilt per mission | whether the weights start equal at M01 launch | trace the callers of the group-list builder `0x599a70` |
| Which of the 29 cue names the engine plays when, and with which id | the pilot-callout part of the soundscape | the callers of the cue table at `0x608cc8` |
| The `rand()` stream position at 40.5-48.5 s | which member each play picks | not recoverable; the capture is one draw |

## Tests

Prefix `accept_parity6_m01_audio_08_` (`crates/cs_app/tests/`):

* the cited rows parse through the production reader and carry their bytes;
  the constants decode to 0.001, 1/32767 and 100.0; the VO table has the
  expected shape (both 267,168 B lines excluded, one line reachable);
* `..._cited_rows_match_the_engine_image` (ignored, needs `CS_ENGINE_IMAGE`,
  panics without it) re-reads every cited byte and checks the image-wide claims:
  the selector has exactly two callers, `0x59ab60` two, `DYNAMIC_WEIGHTS` is
  referenced once, the selector calls the `rand` slot;
* `..._retail_groups_start_equal_and_m01_names_no_burst_size_vo` (ignored,
  needs `CS_GAME_DIR`) reads `sounds.zrd`: 472 `DYNAMIC_WEIGHTS` records, factor
  0.5 (470) / 0.2 (2), no keyword in any member, the member order above; and
  M01's 25 cued groups reach no burst-size file.
