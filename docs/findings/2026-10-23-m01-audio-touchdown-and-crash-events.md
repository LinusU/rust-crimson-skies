# M01 audio timeline: what starts the touchdown and crash sounds

Task #1940 (`PARITY6-M01-AUDIO-BURST-EVENTS`, unblocks #1865). Capabilities:
`retail`, `CS_ENGINE_IMAGE`. Provenance of every row: `static` (read from the
decrypted engine image and the retail `.zrd` files; nothing is
`verified_original`; no original run). No disassembly is committed. The engine
facts are `[[audio_fact]]` rows dated `OWNER-STATIC-2026-10-22` of
`data/owner-static/engine-facts.toml`; the ignored tests
`accept_parity6_m01_audio_09_*` re-read the image and the retail files.
Companion: `2026-10-22-m01-audio-burst-triggers.md` (the burst sizes and the
59 play sites), which left two questions open: what calls `touchdown_water`,
and which of the four 220,616 B members plays.

## Result in one paragraph

**`touchdown_*` is not a "low pass over the sea". It is the contact response of
an aircraft.** Two aircraft contact resolvers (`0x48e580`, `0x490590`, both
reached from the aircraft step `0x489790`) call `0x48d2c0(aircraft, contact*,
impulse)` whenever a contact impulse is greater than 0.0; `0x48d2c0` picks the
animation `touchdown_<soil>` from the soil of the node that was hit and starts
it at the aircraft. `touchdown_water` is soil index 1 (`water`), so it plays
when an aircraft (the player, unless a flag byte is set, or any other aircraft)
touches a node whose soil is `water`; a contact with a node without soil plays
`touchdown_default` (`exp_ground_b`, 220,616 B). Whether M01's sea nodes carry
the soil `water` is **not recoverable** from the gamez export (it has no node
descriptions), and what the M01 aircraft touched at 40.5-48.5 s is a runtime
fact, so the event behind the 78,936 B and 220,616 B buffers stays unknown. The
decoded sizes do narrow it (below): the burst has **no** water-crash buffer
(`exp_water_a`, 151,800 B) and none of `exp_plane_2`/`exp_plane_4` (242,880 B,
259,072 B).

## The soil table (`soil.engine_table`, `soil.table_reset_to_six`)

The engine keeps a table of soil names: count at `0x637b10`, name pointers from
`0x637b14`. The image's built-in entries are `default`(0), `water`(1),
`seafloor`(2), `quicksand`(3), `lava`(4), `fire`(5); `0x55ab7b` resets the
count to 6 and the `LoadSoils` loader (`0x55b815`, cap `0x64`) appends the names
of the mission's `soils.zrd` after them. Retail `soils.zrd` is `player, enemy,
airstrip, opensesame, death, buildings, dzone, dirt`, so it becomes indices
6..13. (The earlier finding listed only the `.zrd` names; `water` and `default`
are built in, which is why `touchdown_water` and `touchdown_default` exist.)

## From soil to animation (`touchdown.*`)

* **Build** (`touchdown.vector_fill`). Mission start (`0x4735b0`) walks the soil
  table and appends the handle of the animation `touchdown_<soil name>`
  (lookup by name, `0x523820`) to a vector at `0x71c2e4`. The vector index is
  the soil index. `touchdown.zrd` defines only `touchdown_default`(0),
  `touchdown_water`(1) and `touchdown_dirt`(13); the other names find no
  animation. The vector is read at exactly one place (image-checked).
* **Lookup** (`touchdown.play_lookup`). In `0x48d2c0` the contact record's
  `[+0x20]` is the node that was intersected and `node+0x20` its soil index; a
  missing node, a negative or too large index, or an empty element falls back to
  **element 0, `touchdown_default`**.
* **Start** (`touchdown.play_call`). The animation starter `0x4edc10` runs the
  animation at the aircraft's position; its sequence `SOUND` event is what is
  heard (`glance`: `snd_exp_water_b` AT_NODE; `glance_spark`/`glance_dust`:
  `snd_exp_ground_b`).
* **Gates** (`touchdown.player_gate`, `touchdown.caller_resolver_a/b`). The
  routine is skipped as a whole when the byte `[0x64f66e]` is set; for the
  player aircraft a nonzero `[aircraft+0x384]` skips the animation; every other
  aircraft plays it. Both callers (image-checked: exactly these two) call it only
  when the impulse is greater than `0.0` (`0x6032c8`).
* **Where the soil comes from** (`soil.description_parse`,
  `soil.height_query_water`). A node has a soil only through a `soil_<name>`
  token in its GameGen description (`0x4c56c0`, the string `soil_` at `0x62bf98`,
  referenced in two places only). The terrain height query `0x48f430` reads the
  same `node+0x20` to find the water layer (soil 1 sets the flag byte
  `0x71c59d` and lowers the reported height by 0.3).

## Destruction (`crash.*`)

Mission start also builds two more vectors by the same soil indices:
`player_crash_<soil>` (loader `0x476250`) and `ai_crash_<soil>` (routine
`0x478a00`). The retail definitions (ignored retail test):

| Animation | Sounds it plays | Decoded size |
| --- | --- | --- |
| `player_crash_default` | `plane_destroy_sg` (`snd_exp_plane1..4`), then `air_mixed_exp_sg` | `exp_plane_1`/`_3` 220,616 B; `_2` 242,880 B; `_4` 259,072 B |
| `player_crash_dirt` | `snd_exp_ground_a`, `ground_mixed_exp_sg` | 220,616 B |
| `player_crash_water` | none itself; calls `plane_big_splash` | `snd_exp_water_a` 151,800 B |
| `ai_crash_default` | `air_mixed_exp_sg` | explosion members |
| `ai_crash_dirt` | `snd_exp_ground_a` | 220,616 B |
| `ai_crash_water` | none itself; calls `plane_big_splash` | `snd_exp_water_a` 151,800 B |

`plane_big_splash` (`plane_splash.zrd`) plays `snd_exp_water_a`, **not**
`snd_exp_water_b`. A plane crashing into the sea is therefore a 151,800 B
buffer, and `exp_water_b` (78,936 B) is the contact, not a crash.

## Which burst buffer belongs to which event

| Buffer | Reading from the decode | Status |
| --- | --- | --- |
| 78,936 B | `touchdown_water`: an aircraft contact on a node of soil `water`. A crash into water would be 151,800 B, and `ai_crash_water`/`player_crash_water` have no sound of their own, so the crash is excluded | the only SFX member; the VO alternative from #1939 is unchanged. Which aircraft touched which node is unknown |
| 220,616 B | candidates unchanged: `exp_ground_b` (contact on default soil or dirt: `touchdown_default`/`touchdown_dirt`), `exp_ground_a` (crash on dirt), `exp_plane_1`/`_3` (player crash on default soil, 2 of the 4 draws). Not `enginelooped` | **not decided.** The contact callers are decoded; the crash callers are not (below) |
| 267,168 B | unchanged (`explosion_7`, `air_mixed_exp_sg`: weapon `IMPACT` rows and `ai_crash_default`) | named |

Two absences narrow the burst: no 151,800 B buffer (no aircraft crashed
into the water in the logged 8 s, if the owner's log is complete) and no
242,880 B or 259,072 B buffer (a player crash on default soil would draw
`exp_plane_2`/`_4` with probability 1/2, so one or two such crashes cannot be
excluded, but a series of them is unlikely).

## Unknown (rule 4) and who can resolve it

| Unknown | Affected content | Resolving work |
| --- | --- | --- |
| Whether M01's sea nodes carry `soil_water` (the gamez export has no node descriptions, and no retail script sets one) | whether the 78,936 B buffer is a sea contact or a different `touchdown_water` source | a reader of the GameGen node descriptions in `gamez.zbd`, or the owner's runtime trace |
| The reader of the `player_crash_*` and `ai_crash_*` vectors (the vectors are at `[0x1f8]`/`[0x15c]` of their records; no direct indexed read was found by byte scan) | which of `exp_plane_1/3`/`exp_ground_a` is the 220,616 B buffer | trace the callers of the destruction path from the vehicle death routine |
| Which M01 aircraft touched what at 40.5-48.5 s | both 220,616 B and 78,936 B buffers | the owner's run with the animation start logged |
| The `rand()` pick of `plane_destroy_sg` | exp_plane_1/3 against 2/4 | not recoverable statically (see `2026-10-23-m01-audio-sfx-group-pick.md`) |

## Not guarded

The retail statements (the animation -> sound chain and the decoded sizes) are
pinned by the ignored test `accept_parity6_m01_audio_09_retail_contact_and_crash_animations_play_the_sounds_the_finding_lists`
(needs `CS_GAME_DIR`). The absence of `soil_` in the gamez export and in the M01
scripts was checked by a text search of the unpacked `ZBD/C3/gamez.zbd`,
`interp.zbd`, `C3/zrdr.zbd` and `C3/M01/zrdr.zbd`; it is not asserted by a test.
