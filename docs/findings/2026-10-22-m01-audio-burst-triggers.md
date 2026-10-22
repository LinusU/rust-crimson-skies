# M01 audio timeline: what plays the 40.5-48.5 s burst

Task #1919 (unblocks #1865, PARITY6-M01-AUDIO-TIMELINE.05). Capabilities:
`retail`, `CS_ENGINE_IMAGE`. Provenance of every row: `static` (read from the
decrypted engine image; nothing is `verified_original`). No disassembly is
committed. The engine-side facts are `[[audio_fact]]` rows, the 59 call sites
are `[[audio_site]]` rows and the burst map is `[[audio_burst]]` rows of
`data/owner-static/engine-facts.toml`; the ignored test
`accept_parity6_m01_audio_07_cited_rows_and_every_call_site_match_the_engine_image`
re-reads the image and requires the site table to equal the image's list of
direct calls. The retail tuning below (`player.zrd`, `weapons.zrd`,
`sounds.zrd`, `touchdown.zrd`, `soils.zrd`) was read from `ZBD/zrdr.zbd` and is
not covered by the image test (it needs `CS_GAME_DIR`; see "Not guarded").

Companions: `2026-10-22-m01-audio-timeline-voice-laws.md` (the question this
answers, section d), `2026-10-22-m01-audio-timeline-sample-identity.md` (the
candidate members per decoded size).

## The two helpers and their 59 call sites

`0x593590(handle, gain)` is the position-less play and `0x593b80(handle, gain,
position*, flag)` the 3D play; for an ordinary handle the first forwards to the
second (`play.helper_dispatch`). A byte scan of `.text` finds exactly 59 direct
calls (49 of `0x593590`, 10 of `0x593b80`) and no `jmp` to either. Every
sound the game starts goes through one of them, but most callers play a
**handle held in a variable**; the group name is read only where the code
looks it up by a string next to the play.

| Class (`event`) | Sites | What plays it |
| --- | --- | --- |
| `gun_shot` | 1 | `0x5af3de` in the weapon-fire routine `0x5aef9c`: handle `[weapon+0xc4+4*index]`, 3D at the muzzle (`play.weapon_fire`) |
| `weapon_row_pick` | 2 | `0x5ad14e` / `0x5ad1ae`: the random pick of a weapon row's sound list (`0x5ad100`, twin `0x5ad160`), 3D at the record position; callers `0x5ac91f` (burst), `0x5ad039`, `0x5b1387`, `0x5b0b64` |
| `bullet_impact` | 1 | `0x4b9ed8`: the player aircraft hit routine plays `player.zrd` `bullet_hit_sound` 2D |
| `warning_shot` | 1 | `0x4b9ebc`: the same routine plays `warning_shot_sound` 2D when `[aircraft+0x91c]` is set and `0x440ad0() == 0` (`play.player_bullet_hit`) |
| `sequence_sound` | 3 | `0x4e7564`, `0x4e7585`, `0x4e9ba5`: the executors of a sequence `SOUND` event (`0x4e74e0`, `0x4e8fa0`): 3D at the named node, else 2D. Destruction, touchdown and puffer-trail effects all play here |
| `sound_node` | 2 | `0x4e0df2`, `0x4e0e3b` (existing row `soundnode.update`) |
| `aircraft_voice` | 2 | `0x4b14a6`, `0x4b14c4`: engine and prop loop slots |
| `music` / `dispatcher` | 3 | `0x59a689`, `0x59a698` (mission-state music), `0x593607` (the forward above) |
| `ui_cue` | 21 | weapon select/empty (4), target switch (9), `NO_AMMO_WARNING` (1), front-end widgets and menus (7, including the hard-coded `snd_ricochet1` window cue at `0x5cbd82`, gain 0.2) |
| `danger_zone_cue`, `mission_cue`, `multiplayer_cue` | 1, 2, 8 | `snd_dangerzone_camera`; zeppelin destroyed/lost; capture-the-flag and team-zeppelin cues |
| `unknown` | 12 | caller-supplied or object-held handles whose group is not decoded (`0x41e8a7`, `0x41e96e` are dead: the handle is only ever written 0) |

There is **no engine call site that names an explosion, a ground contact or a
voice-over line**. Explosions and ground contacts are *data*: sequences in the
effect files carry `SOUND` events and the weapon rows carry sound lists, both
played by the sites above. Voice-over lines are queued by the mission layer
(the wrappers of `0x593590` in the earlier `m01-lc-directive-d` finding) and by
the dialogue names `snd_%s_id%d_random` / `snd_id%d_%s` built in the engine.

## Which groups the retail data feeds those sites

- **Player-aircraft bullet hit**: the weapon rows (`weapons.zrd`) give every
  machine-gun round (`30 AP`, `30 DD`, `30 EX`, ... 27 rows) the `IMPACT` row
  `player` -> `bullet_hit_sg`, the `default` row (ground, objects) ->
  `snd_grnd_bullet` and the `water` row -> `snd_water_bullet`; `player.zrd`
  `bullet_hit_sound` is `bullet_hit_sg` too. `bullet_hit_sg` is
  `DYNAMIC_WEIGHTS` over `snd_ricochet1..4` = `ricco4.wav`, `ricco1.wav`,
  `ricco3.wav`, `ricco2.wav`. The puffer-trail sequence `sound`
  (`pufftrails.zrd`) also plays `snd_ricochet2`, `snd_ricochet3` and
  `snd_ricochet1` with `EVENT_OFFSET 0.25` (random weight 0.5).
- **Warning shot**: `player.zrd` `warning_shot_sound` is `bullet_warning_sg` =
  `snd_bulletpass1..3` (`bullet_pass1..3.wav`). The members named
  `warning_shot2.wav` / `warning_shot3.wav` / `warning_seeker.wav` are **not**
  what the engine's warning-shot path plays.
- **Explosions**: `air_mixed_exp_sg` = `explosion_1, 3, 4, 5, 7` (`snd_exp_hit1,
  2, 3, 3a, 5`), `ground_mixed_exp_sg` = `explosion_1, 2, 3, 4, 6`;
  both are the weapon `IMPACT` rows' sounds (the `player` row of `gb` is
  `air_mixed_exp_sg`) and the destruction sequences'. `snd_exp_hit5`
  (`explosion_7.wav`) is also played by `pufftrails.zrd` `small_injure_fireball`.
  Aircraft destruction (`player_plane_destruct`, `ai_plane_destruct` and the
  per-airframe files) plays `plane_destroy_sg` = `snd_exp_plane1..4` and
  `snd_exp_ground_a`.
- **Ground contact**: mission start builds `touchdown_<soil>` animation names
  (`config.touchdown_names`; soils: player, enemy, airstrip, opensesame,
  death, buildings, dzone, dirt). `touchdown.zrd` defines `touchdown_water`
  (sequence `glance`: `SOUND snd_exp_water_b AT_NODE MAIN_ROOT_NODE`),
  `touchdown_default` (`glance_spark`: `snd_exp_ground_b`) and `touchdown_dirt`
  (`glance_dust`: `snd_exp_ground_b WITH_NODE`).
- **Never named**: `snd_bullet_hit`, `snd_warningshot2`, `snd_warningshot3`,
  `snd_seeker`, `snd_train_derail` and `snd_enginelooped` (outside the M05
  mission files) appear in no effect, weapon or mission file and in no string
  of the engine image: only in the sound table. By name nothing starts them.

## The burst sizes (answer to item 3)

| Decoded bytes | Member | Status | Why / what would decide |
| --- | --- | --- | --- |
| 267,168 | `explosion_7.wav` (`snd_exp_hit5`, in `air_mixed_exp_sg`) | named | the other SFX member `train_derail.wav` is a looped group nothing starts; two `VO_*` lines share the size and are not excluded statically |
| 22,264 | `ricco4.wav` (`snd_ricochet1`, in `bullet_hit_sg`) | named | `tny_exp_2.wav` and `warning_shot2.wav` are never named; event: a bullet hit on an aircraft |
| 16,192 | `ricco3.wav` (`snd_ricochet3`) | named | `warning_seeker.wav` is a looped group nothing starts |
| 18,216 | `ricco2.wav` (`snd_ricochet4`) | named | `warning_shot3.wav` is never named |
| 78,936 | `exp_water_b.wav` (`snd_exp_water_b`) | named | the only SFX member; the event is the `touchdown_water` ground contact; three `VO_*` lines share the size and are not excluded statically |
| 220,616 | `exp_plane_1`, `exp_plane_3` (`plane_destroy_sg`), `exp_ground_a` (destruction), `exp_ground_b` (land touchdown) | candidates | `enginelooped.wav` is excluded (a loop, named only by M05). Which of the four plays depends on the event (destruction or land contact) and, for `plane_destroy_sg`, on the `rand()` pick |

Reading of the burst: three ricochet sizes, one explosion, one water contact
and one destruction/contact sound in 8 s are consistent with the airframes
being shot at and touching the water; the pairs of equal sizes are two plays
of one group (the previous finding), which for `bullet_hit_sg` (one random
member per play) is a coincidence of the pick, for the `pufftrails` ricochet
sequence a designed repeat. This is a reading of the data, not a proof of
what M01 does at 40.5-48.5 s.

## Unknown (rule 4) and who can resolve it

| Unknown | Affected content | Resolving work |
| --- | --- | --- |
| Whether a `VO_*` line, not an SFX member, is the buffer for 267,168 and 78,936 B | those two burst buffers | the mission script's dialogue queue at 40-48 s (the `m01-lc-directive-d` wrappers) against the owner's log |
| The event behind the 220,616 B buffers (destruction, land contact, or both) | two burst buffers | the M01 sequence of events at 40.5-48.5 s (our run's destruction and `touchdown_*` calls) and the `rand()` stream |
| The 12 `unknown` sites' groups (`0x42b463`, `0x42b496`, `0x42eb6c`, `0x43da4c`, `0x44bc8c`, `0x44be5f`, `0x45e4ee`, `0x46cb57`, `0x46cce9`, `0x4afcbe`, ...) | none of the burst sizes reached them | decode their callers |
| The rows served by `0x5ad039`, `0x5b1387`, `0x5b0b64` and the keyword of `0x4e8fa0` | which weapon event picks a row sound | the weapon-section parser and the fire path (`0x5aef40`) |
| What `[aircraft+0x91c]` and `0x440ad0()` select in the player hit routine | whether a hit plays `bullet_warning_sg` or `bullet_hit_sg` | trace the writer of `[aircraft+0x91c]` |
| Whether `touchdown_water` is the animation a low pass over M01's sea calls | the 78,936 B buffer | trace the caller of the `touchdown_` animations |

## Not guarded

The `ZBD/zrdr.zbd` statements (group members, `IMPACT` rows, the `never named`
list) were read with the project's own `unzbd reader` extraction into
`private/` and are not asserted by a test; they restate what `sounds.zrd`
already says about which `snd_*` is in which group and what the other files
name. A later task can pin them with a `CS_GAME_DIR` test.
