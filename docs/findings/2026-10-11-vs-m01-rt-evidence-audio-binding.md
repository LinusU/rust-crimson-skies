# VS-M01-RT-EVIDENCE-B: which sound the M01 launch legitimately starts

Date: 2026-10-11. Task #1313 (VS-M01-RT-EVIDENCE-B), part of #1218 (VS-M01-RT-EVIDENCE).
Capability used: `retail` (read access to the original installation). This is file
evidence only — it is not an original-run capture and proves nothing about audible
output of the original executable.

## Question

The M01 mission composition installs no `AudioPlugin` and starts no sound
(`mission_session/compose.rs`). Before wiring one, task #1313 requires a measured
answer: which sound does the M01 launch legitimately start, according to the
original data?

## Method

A throwaway probe (`cs_content` test target, run locally against `CS_GAME_DIR`,
not committed) walked the production readers:

1. `original_airframe::resolve_vehicle_record(docs.vehicle(), "pdevastator")` —
   the resolved player-airframe record chain and the record each sound field is
   spelled on.
2. `stunts::decode_zrd` on `sounds.zrd` (member of `ZBD/zrdr.zbd`) — the body of
   each named sound group.
3. `vfs::SessionBuilder::mount_installation` + `ZbdContainer::sound_assets` —
   presence and sha256 of the named `.wav` members in the two sound archives.
4. `stunts::decode_zrd` on `objectives.zrd` (member of `ZBD/C1C/M01/zrdr.zbd`) —
   `BEGIN_DORMANT` / `WAKEUP_SOUND_GROUP` / `COMPLETED_SOUND_GROUP` per
   `OBJECTIVE<n>` block.

## Measured

### Player airframe declares an engine sound and a cockpit engine sound

`pdevastator` resolves through the chain `basic_airplane -> player_airplane ->
pdevastator`. Both fields are spelled on the `pdevastator` record itself:

- `engine_sound = "snd_devastatorengine"`
- `cockpit_engine_sound = "snd_devastator_cp"`

### The named sound groups resolve to concrete archive members

In `sounds.zrd` (`ZBD/zrdr.zbd`):

- `snd_devastatorengine` -> `devastator.wav` (single member)
- `snd_devastator_cp` -> `devastator_cp.wav` (single member)
- `snd_NW1Start` -> group of `snd_c2-NW-m1_WorkersVoyage_1` and
  `snd_c2-NW-m1_Zachary_2` (radio/dialogue cues, not a loop)
- `snd_zepengine` -> `engine_zep.wav` (zeppelin engine, for comparison)

Both `.wav` members exist in both sound archives, through `sound_assets`:

| Container | Member | Bytes | sha256 |
| --- | --- | --- | --- |
| `ZBD/soundsl.zbd` | `devastator.wav` | 18480 | `324621f2f0175d14291919106b58ae638a175b8222f675f956e588b995d212b6` |
| `ZBD/soundsl.zbd` | `devastator_cp.wav` | 18480 | `8673e45c4097cfaf53cf9a21026a70e1b6cad6b3ed905e57e1b86be9653b9ec7` |
| `ZBD/soundsh.zbd` | `devastator.wav` | 72782 | `42454ee15f49a86c369428df00a5cfdcc51a0315f62e80904725f750663d7470` |
| `ZBD/soundsh.zbd` | `devastator_cp.wav` | 72782 | `10141dd61753b258b421b8ede45985c35dd4ab195f2f932ec175348b75af7381` |

`soundsl.zbd` is the 11025 Hz asset set and `soundsh.zbd` the 22050/44100 Hz set
(`docs/findings/2026-10-03-f14-d-7-sound-cue-collection.md`). **Which container
the original engine picks for a given cue is not measured** (same finding). The
runtime binding therefore records the container pick as a *labelled choice*, not
a measurement: this task feeds the launch from `ZBD/soundsh.zbd` because it is
the higher-rate set and both members exist in it.

### Mission-declared launch cues exist but are one-shot groups, not loops

`objectives.zrd` for M01: `OBJECTIVE1` carries `BEGIN_DORMANT 2` plus
`WAKEUP_SOUND_GROUP snd_NW1Start` — a radio cue group that fires shortly after
launch. Every other objective block is `BEGIN_DORMANT -1` or has no dormant
field; their `WAKEUP_SOUND_GROUP` / `COMPLETED_SOUND_GROUP` entries are
mission-progress radio and music one-shots (`snd_c2-NW-m1_*`, `music_*_sg`),
none of which play at tick 0 except the OBJECTIVE1 pair above.

## Conclusion

The sound the M01 launch legitimately starts, measured end to end through
production readers, is the **player airframe's engine loop**:

`pdevastator.engine_sound` -> `snd_devastatorengine` -> `devastator.wav`
(member of `ZBD/soundsh.zbd` / `ZBD/soundsl.zbd`).

`cockpit_engine_sound` (`devastator_cp.wav`) is the in-cockpit variant of the
same loop; the launch evidence binds the external `engine_sound`. The
mission-declared `snd_NW1Start` wakeup cue is a follow-on radio group, not a
launch loop, and belongs to the objectives/dialogue runtime rather than the
airframe emitter.

## Unknowns recorded (not guessed)

- Which of `soundsl.zbd` / `soundsh.zbd` the original mixer opens for a given
  cue is unmeasured (see F14-D.7 finding). Recorded as a labelled choice.
- Whether the original launch also autostarts `snd_NW1Start` at the 2-second
  dormant wakeup in the same frame budget is mission-program timing; this task
  binds only the engine loop.

## Implementation and playback (same task, 2026-10-12)

The composition now starts that binding (`mission_session/launch_sound.rs`,
`compose.rs`):

- `stage_for` measures it through the production readers (`engine_sound` of the
  player airframe record, `sounds.zrd`, the stage's sound archives); a stage
  that cannot measure it refuses by name.
- The declared catalog holds one record. The member's identity is measured;
  its bus (`engine`), mode (`loop`) and level (`1.0`) are **designed** and
  the record's `Origin::Designed` and provenance say so.
- The container is staged as a verified copy in the private cache
  (`launch-sound/ZBD/soundsh.zbd`, digest-checked against the installation's)
  and mounted as a one-file `install` source. Mounting the installation
  directory instead hashes every file below it (about 85 s measured), which a
  launch must not pay twice. A single-file mount in `cs_assets` would remove the
  copy; that is outside this task's paths.
- `AudioPlugin` installs the audible device (or a `RefusingAudioDevice` with the
  named refusal) with a `SampleProbe` attached before any voice starts. The
  load delivers the member, the population pass decodes it, the player body's
  engine emitter binds the loop, and the listener rides with the body
  (designed: right axis `+X`, 10 m / 100 m policy).
- `run_windowed` refuses before the window opens when the backend was refused;
  `MissionLaunchError::exit_code` maps that through `audibility_exit_code`
  (4 for a missing `audio` capability).
- The host still reports `MissionAudioStillBound` at settle (true: the loop is
  bound). `teardown` then despawns the emitter and stops the loop as
  `Despawned`, with one mix pass carrying the stop to the device.

Playback record (retail + audio machine, source boundary only): the retail
test writes `playback.json` with the asset id, archive, archive and member
digests, the decoded member shape and the frames the output stream pulled.
First measured run: 22050 Hz stereo, 71852 frames (3.26 s), 963 frames pulled
in the first moments, peak 0.667. It is not a speaker capture and not
`human_review`; nothing here is `verified_original`.

Still unmeasured: which archive the original opens, the original's bus, level
and attenuation for this loop, and the cockpit variant's use (`devastator_cp.wav`).
