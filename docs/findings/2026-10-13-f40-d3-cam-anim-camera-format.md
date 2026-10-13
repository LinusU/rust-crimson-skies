# F40-D3: what `cam_anim.zbd` actually stores — the measured camera-cinematic format

Date: 2026-10-13. Task: #662 (`F40-D3`) "Identify the original in-engine
camera animation format (cam_anim.zbd)". Spec:
`specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`, stage
`### F40-D`. Capability used: **`retail`** (read-only access to
`$CS_GAME_DIR`) and ordinary build/test. No original executable ran; nothing
here is `verified_original`, and nothing says what the 2000 engine did with
any byte it stores.

## Summary

`ZBD/<group>/cam_anim.zbd` is **not a camera-keyframe container**. It is the
world group's *camera carrier*: the same animation-container format as
`mis_anim.zbd` (signature `0x08170616`, version 53, front index, 272-byte
records — measured by tasks #633/#650,
`docs/findings/2026-10-04-m01-lc-anim-carriers.md` and
`docs/findings/2026-10-05-m01-lc-anim-records.md`). What this task adds is
the decoding of what #650 left raw — each sequence block's **event stream** —
and the identification of the in-engine camera cinematics inside it:

1. every event stream is a **TLV sequence** `{ u32 type_id; u32 total_bytes;
   payload[total_bytes - 8] }`, walking exactly with no padding and no
   trailer;
2. **59 event type ids** occur across the retail animation family, 58 of them
   with one fixed `total_bytes` in every container (the exception, `0x12f`,
   stores 548 or 1156);
3. the camera cinematics are the records whose `object_name` **and**
   `root_name` are `camera1` — `generic_intro`, `gi_scene1`, `player_setup`
   and `speed_cue`(`2`) in the eight world groups, `mission_intro_animation`
   and `scene1` in mission carriers that ship one;
4. **no measured field carries a camera pose keyframe** — no per-tick
   position, orientation or field-of-view table exists anywhere this reader
   measures — so no `CameraTimeline` is built from this data; the content
   half declares the records with explicit `f40d3.*` unknowns instead
   (`cs_content::cinematics::declared_camera_sequence`).

## The census behind every number here

A read-only Python walk of `$CS_GAME_DIR` (scripts kept in `private/f40d3/`,
git-ignored; reproduction commands below), using the layout constants of
`crates/cs_formats/src/zbd/anim.rs`:

| population | carriers | sequence streams | events | layout issues |
| --- | --- | --- | --- | --- |
| all `*anim*.zbd` animation carriers (`ZBD/<group>/cam_anim.zbd` + every `ZBD/<group>/<mission>/mis_anim.zbd`) | **61** | **56 994** | **242 391** | **0** |

"Layout issues" are the things the TLV grammar refuses: a stream that does
not tile exactly (a trailing fragment), an event whose `total_bytes` is below
12, or an event overrunning its stream. **None occurred.** The 56 994
streams are exactly the sequence-block count #650 walked independently, which
cross-checks the two derivations. Event `total_bytes` spans **12 .. 1156**.

The production reader re-measures the camera carriers on every test run:
`accept_f40_d3_retail_camera_carriers_walk_their_camera_cinematics` walks all
eight through `cs_app::cinematics::camera_anim::survey_camera_carrier` and
fails on any unmeasured type, any size mismatch and any non-tiling stream.

## The container (unchanged from #633/#650, re-measured)

```text
0x00  u32  signature 0x08170616
0x04  u32  version (53 in every retail container)
0x08  u32  external count (2)
0x0C  u32  member count
0x10  external_count x { u8 path[128]; u32 stamp }   132 bytes each
       member_count   x { u8 path[80];  u32 stamp }    84 bytes each
       ... the animation payload (68-byte header, 40 zero bytes,
           then the 272-byte records of #650) ...
```

The externals are the two sibling containers, `zbd\<group>\gamez.zbd` then
`zbd\planes.zbd`, in all eight camera carriers (the `zbd\c1\gamez.zbd`
string the task description names is external row 0 of `C1`'s carrier).
Member row 0 is the carrier's own paired document
`..\\data\\<group>\\zrdr\cam_anim.zrd` (every separator but the last
doubled), as #633 measured.

## The event stream (this task's measurement)

Each sequence block of a record (reset, damage, ordinary — #650's blocks, a
64-byte info block plus `size` event bytes at info `+60`) stores its event
bytes as:

```text
event:  u32 type_id; u32 total_bytes; u8 payload[total_bytes - 8]
        ^ offset 0    ^ offset 4      ^ offset 8 .. total_bytes
next event at +total_bytes; the stream ends exactly after its last event.
```

* the 8-byte header is fixed; `total_bytes` counts the header itself;
* `total_bytes >= 12` in every retail event (smallest: `0x120`/`0x122`, a
  header plus one zero word);
* the payload is **not decoded**: no payload field has a measured meaning,
  so the reader keeps it verbatim (`CameraAnimEvent::payload`) and never
  reads a pose, tick or name out of one.

### The 59 measured type ids and their fixed sizes

`total_bytes` per type id, over all 61 carriers (this repository's
measurement, `ClaimStatus::ObservedTool`; **semantics unmeasured**):

| type | size | type | size | type | size |
| --- | --- | --- | --- | --- | --- |
| `0x101` | 28 | `0x116` | 48 | `0x201` | 28 |
| `0x102` | 72 | `0x117` | 48 | `0x205` | 112 |
| `0x104` | 152 | `0x118` | 80 | `0x20b` | 144 |
| `0x105` | 112 | `0x119` | 48 | `0x20e` | 36 |
| `0x106` | 20 | `0x11a` | 48 | `0x216` | 48 |
| `0x107` | 32 | `0x11b` | 48 | `0x217` | 48 |
| `0x108` | 28 | `0x11c` | 80 | `0x218` | 80 |
| `0x109` | 32 | `0x11e` | 20 | `0x219` | 48 |
| `0x10a` | 332 | `0x11f` | 28 | `0x21e` | 20 |
| `0x10b` | 144 | `0x120` | 12 | `0x22a` | 596 |
| `0x10c` | 76 | `0x121` | 28 | `0x301` | 28 |
| `0x10d` | 24 | `0x122` | 12 | `0x304` | 152 |
| `0x10e` | 36 | `0x123` | 16 | `0x305` | 112 |
| `0x10f` | 16 | `0x124` | 64 | `0x306` | 20 |
| `0x110` | 16 | `0x129` | 36 | `0x309` | 32 |
| `0x111` | 20 | `0x12a` | 596 | `0x30b` | 144 |
| `0x114` | 48 | `0x12e` | 188 | `0x30c` | 76 |
| `0x12f` | **548 or 1156** | `0x30e` | 36 | `0x316` | 48 |
| `0x317` | 48 | `0x318` | 80 | `0x319` | 48 |
| `0x31e` | 20 | `0x32a` | 596 | | |

Two observations, recorded but **not interpreted**:

* the ids come in ranges whose sizes mirror each other (`0x101`/`0x201`/
  `0x301` are all 28 bytes; `0x116`/`0x216`/`0x316` all 48; `0x118`/`0x218`/
  `0x318` all 80; `0x12a`/`0x22a`/`0x32a` all 596) — what the high byte
  selects is **unmeasured**;
* `0x12f` is the only type with two stored sizes (548 and 1156, both in
  mission carriers; 1156 is not two 548s). Nothing measured tells the two
  shapes apart, so the reader accepts both and never guesses between them.

## The camera cinematics

**Classification rule (measured):** a record is a camera cinematic when its
`object_name` **and** `root_name` are both `camera1` (`CAMERA_OBJECT_NAME`).
Measured over the eight camera carriers and the mission carriers sampled:

| carrier | camera cinematics |
| --- | --- |
| `ZBD/C1` | `generic_intro`, `gi_scene1`, `player_setup`, `speed_cue` |
| `ZBD/C1B` | `generic_intro`, `gi_scene1`, `player_setup`, `speed_cue2` |
| `ZBD/C1C` | `generic_intro`, `gi_scene1`, `player_setup`, `speed_cue` |
| `ZBD/C2` | `generic_intro`, `gi_scene1`, `player_setup`, `speed_cue` |
| `ZBD/C2B` | `generic_intro`, `gi_scene1`, `player_setup`, `speed_cue`, `speed_cue2` |
| `ZBD/C3` | `generic_intro`, `gi_scene1`, `player_setup`, `speed_cue` |
| `ZBD/C4` | `generic_intro`, `gi_scene1`, `player_setup`, `speed_cue` |
| `ZBD/C5` | `generic_intro`, `gi_scene1`, `player_setup`, `speed_cue` |
| `ZBD/C4/M01` | `mission_intro_animation`, `scene1` |

The common set — `generic_intro`, `gi_scene1`, `player_setup` — is what the
acceptance test pins; `speed_cue`/`speed_cue2` differ per carrier and are
reported, not pinned by name.

These records *orchestrate*; their event payloads reference other animation
names and small indices (observed structure, semantics unmeasured):

* `0x118` (80 bytes) carries a 32-byte NUL-terminated name —
  `speed_cue`, `pzep_engine_start`, `letterbox`, `gi_scene1`, `gi_scene2`,
  `gi_1stperson` inside `generic_intro`/`player_setup`;
* `0x119`/`0x319` (48 bytes) carry a float and a name (`gi_scene1` at
  `15.9`, `gi_scene2` at `12.4`, `gi_1stperson` at `9.9` inside
  `generic_intro`'s scene block) — whether the float is seconds, ticks or
  something else is **unmeasured**;
* `0x106`/`0x10f`/`0x110`/`0x123` (12..20 bytes) carry small integer words
  that track the record's node-table and animation-ref indices;
* `0x10c` (76 bytes) appears once per `gi_scene` with the scene's index
  (`0`, `1`, `2`, `3`);
* the record's `objects`/`nodes` tables name world nodes (`camera1`,
  `player`, `piratefighter`, `world1`, ...), so the cinematic's *subject* is
  the world's `camera1` node.

## Why no `CameraTimeline` keyframes come out of this

`cs_app::cinematics::timeline::CameraTimeline` (F40-B) needs keyframes of
`(tick, position, fov)`. The candidates were examined byte by byte and none
is a pose track:

* `0x10a` (332 bytes, e.g. in `large_camshake`): mostly zero words with two
  isolated floats (`0.056`, `0.559`) — no tick/position repetition;
* `0x10b` (144 bytes, e.g. in `open_map`): a struct of small signed ints and
  two radian-looking floats (`1.047`, `1.396`) — not a track;
* `0x124` (64 bytes, in `gi_scene1`): 11 zero words then `1.0`, `-0.1667`,
  `6.0` — three floats, no ticks, no repetition;
* `0x12a` (596 bytes, `speed_cue`'s puffer spawns): a 32-byte name plus
  mostly zeros — no pose;
* the containers hold no other table: the payload after the records is
  #650's unmeasured trailing region, and the record fixed parts' float
  fields (`reset_time`, `max_health`) are single values, not tracks.

So the camera's per-tick pose, if the original animates one at all, comes
from data this task does not measure (the world scene graph of `gamez.zbd`
is the plausible home of the `camera1` node — **unmeasured**, not claimed).
The task's instruction "parse into `CameraTimeline` keyframes **where
measured**" therefore yields: **nowhere**. Nothing was invented. Instead:

* `cs_content::cinematics::declared_camera_sequence` declares one measured
  camera-cinematic record as an `InEngine` cinematic whose duration, frame
  size, skippability, pause policy, clock and recovery are all
  `Resolved::Unknown` under `f40d3.*` claims;
* `cs_app::cinematics::lower_cinematic` refuses such a record by claim
  (`f40d3.pause_policy` is the first unknown it meets on the in-engine
  path), so no invented number can reach the runtime.

## Evidence classes

| claim | class | why |
| --- | --- | --- |
| the container header, index, payload header, 272-byte records | `Documented`/`ObservedTool` | tasks #633/#650, re-measured here |
| the event TLV grammar (8-byte header, `total_bytes`, exact tiling) | `ObservedTool` | 242 391 events of all 61 retail carriers tile exactly; zero counterexamples |
| the 59 type ids and their fixed sizes | `ObservedTool` | measured across every carrier each type occurs in; the size table pins them in code |
| the `camera1` classification rule and the per-carrier names | `ObservedTool` | a description of the files, not of the engine |
| what a payload field means; what the high byte of a type id selects; whether a float is seconds or ticks | **unknown** | no measured meaning; candidates listed above |
| the camera's pose source and whether the original animates a pose at all | **unknown** | no measured track; scene-graph hypothesis not tested |
| how the original engine consumed any of this | **unknown** | no original run |

## Files

- `crates/cs_app/src/cinematics/camera_anim.rs` (**new**): the measured
  event grammar and the carrier survey — `parse_camera_anim_stream`,
  `CameraAnimEvent`, `OwnedCameraAnimEvent`, `measured_event_total_size`,
  `MeasuredEventSize`, `MEASURED_MIN_EVENT_BYTES`, `MEASURED_MAX_EVENT_BYTES`,
  `CAMERA_OBJECT_NAME`, `is_camera_cinematic`, `survey_camera_carrier`,
  `CameraCarrierSurvey`, `CameraSequence`, `CameraSequenceBlock`,
  `CameraAnimStreamError`, `CameraCarrierError`. Wiring:
  `crates/cs_app/src/cinematics/mod.rs` (module declaration and the
  `### F40-D3` doc paragraph).
- `crates/cs_content/src/cinematics.rs`: `declared_camera_sequence` (every
  unmeasured field an explicit `f40d3.*` unknown) and
  `CAMERA_SEQUENCE_UNKNOWN_REASON`.
- `crates/cs_app/tests/accept_f40_d3_camera_anim.rs` (**new**): five
  acceptance tests — four synthetic (TLV parse, fail-closed refusals, the
  pinned size table, the unknown-declaration/lowering-refusal pair) and one
  `#[ignore = "requires CS_GAME_DIR"]` retail test (the eight-carrier
  survey, re-measuring every stream, type and size on each run).

## Mutation sensitivity

* deleting the TLV walk (replacing it with, say, 4-byte words) fails
  `accept_f40_d3_event_stream_parses_as_the_measured_tlv` and the retail
  survey tests;
* removing a fail-closed refusal fails
  `accept_f40_d3_event_stream_refuses_broken_streams_by_name`;
* changing one size in the measured table fails
  `accept_f40_d3_measured_size_table_pins_the_retail_shapes` and the retail
  size-mismatch assertions;
* removing the `camera1` classification (e.g. classifying by `anim_name`
  alone) fails the retail test's common-name assertions;
* making `declared_camera_sequence` return a known duration fails
  `accept_f40_d3_camera_sequence_declares_every_unmeasured_field_as_unknown`
  and the lowering-refusal assertions.

## Reproduction

The measurement scripts live in `private/f40d3/` (git-ignored; original bytes
are never committed, and nothing was written inside `$CS_GAME_DIR`):

```sh
CS_GAME_DIR=<original installation> python3 private/f40d3/explore7_full.py
cargo test --workspace --locked -- accept_f40_d3_ --include-ignored   # CS_GAME_DIR set
```

The production re-measurement on every retail test run:

```sh
CS_GAME_DIR=<original installation> \
  cargo test --workspace --locked -- accept_f40_d3_retail_ --include-ignored
```

## Limitations

* **Payloads are not decoded.** The TLV grammar gives boundaries and types,
  not meanings; an animation still cannot be *played* from this data. The
  event-grammar semantics are the natural follow-up (and would unblock the
  runtime consumers F20-C's `AnimationLog` seam names).
* **No camera pose keyframes** are established; the F40-B `CameraTimeline`
  remains designed-only for original data, now with measured evidence for
  why.
* The type-size table is measured over the **retail installation this
  repository tests against**; a modded or other regional install could carry
  other sizes, which the survey would report as mismatches rather than trust.
* `0x12f`'s two shapes are not told apart; a consumer that needs to index
  into its payload cannot, and must not, guess.
* Nothing is `verified_original`: no original executable ran, and no human
  viewed or played anything (`human_play`/`human_review` are owner-gated).
