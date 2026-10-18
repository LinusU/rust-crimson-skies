# F40-D3: what `cam_anim.zbd` actually stores — the measured camera-cinematic format

> **Update (#1662, 2026-10-18).** The statement below that no measured field carries a camera pose
> is true of the record walk only. The poses are `.zan` tracks in the region after the last record;
> see `docs/findings/2026-10-18-parity-m01-lc-cinematic-camera-pose-source.md`. The rest of this
> document stands.

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
   trailer — the same two-word grammar task #690 measured as `tag` +
   `length` over the same corpus
   (`docs/findings/2026-10-06-f20-event-grammar.md`); this task re-derives
   it keyed by the full type id and adds the camera-carrier-specific findings
   below;
2. **59 event type ids** occur across the retail animation family, 58 of them
   with one fixed `total_bytes` in every container (the exception, `0x12f`,
   stores 548 or 1156); they decompose exactly as #690 measured — low byte =
   opcode (the 35 opcodes #690 names), second byte = `1`/`2`/`3`;
3. the camera cinematics are the records whose `object_name` **and**
   `root_name` are `camera1` — **58 records in 31 carriers**:
   `generic_intro`, `gi_scene1`, `player_setup` and `speed_cue`(`2`) in the
   eight world groups, `mission_intro_animation` and `scene1` in
   `ZBD/C1/M04`'s mission carrier, `bdrop_ew_cam`/`bdrop_we_cam` in
   `ZBD/C4/M03`, and `multiplayer_setup` in the 21 multiplayer mission
   carriers;
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
streams are exactly the sequence-block count #650 walked independently, and
the block/event totals reproduce #690's event-grammar census (56 994 blocks,
242 391 events, 16 132 948 bytes) exactly — **three independent derivations
agree**. Event `total_bytes` spans **12 .. 1156**.

The reviewer of this task re-ran the census script from a fresh session
(2026-10-13): 61 carriers, 56 994 streams, 242 391 events, 0 issues, 59
types of which 58 single-size and `0x12f` = `[548, 1156]` — identical to
every number above.

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
* the payload is **not decoded by this task**: the reader keeps it verbatim
  (`CameraAnimEvent::payload`) and never reads a pose, tick or name out of
  one. Payload *semantics* are partly measured by task #690 for the same
  bytes — opcode spellings joined to the installation's own declarations,
  `START_TIME` at payload word 0, `RUN_TIME` at the last word for its five
  value-matched opcodes — and those claim ids stand; this task neither
  re-derives nor contradicts them.

### The same grammar as #690, keyed differently

#690 (`docs/findings/2026-10-06-f20-event-grammar.md`) measured each event
as `tag word` + `length word`, where the tag's low byte is an opcode and its
second byte is `1`, `2` or `3` (a meaning it left unmeasured). The 59 type
ids of this task are that decomposition keyed by the full first word: the
35 stored opcodes #690 names, each with the second bytes it occurs with —
`0x101`/`0x201`/`0x301` are opcode 1; `0x216`/`0x217`/`0x218`/`0x219` are
opcodes 22–25 with second byte 2; 35 + 10 + 14 = 59. A size table keyed by
opcode would have to assume the decomposition; keyed by the stored word it
measures it.

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
  `0x318` all 80; `0x12a`/`0x22a`/`0x32a` all 596) — the second byte is what
  #690 measured as the tag's `group` byte (`1`, `2` or `3`, travelling on
  every `DecodedEvent`); **what it selects** is unmeasured;
* `0x12f` is the only type with two stored sizes (548 and 1156, both in
  mission carriers; 1156 is not two 548s). Nothing measured tells the two
  shapes apart, so the reader accepts both and never guesses between them.

## The camera cinematics

**Classification rule (measured):** a record is a camera cinematic when its
`object_name` **and** `root_name` are both `camera1` (`CAMERA_OBJECT_NAME`).
Measured over **every** carrier of the installation (all 8 camera carriers
and all 53 mission carriers): **58 camera-cinematic records in 31 carriers**.

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
| `ZBD/C1/M04` | `mission_intro_animation`, `scene1` |
| `ZBD/C4/M03` | `bdrop_ew_cam`, `bdrop_we_cam` |
| 21 multiplayer mission carriers (`C1`/`C1B`/`C1C`/`C2`/`C2B`/`C3`/`C4`/`C5` × `MP1`–`MP3`, where present) | `multiplayer_setup` |

(The mission carrier `ZBD/C4/M01` — the first mission's own `mis_anim.zbd` —
holds **no** camera cinematic; the mission-intro records live in
`ZBD/C1/M04`. The first draft of this document said `c4/m01`; the reviewer's
full-installation scan corrected it.)

The acceptance test pins the **whole inventory**: the exact per-carrier name
sets for all 31 carriers, the 58-record total, and the exact set of 21 event
type ids that occur inside camera cinematics
(`accept_f40_d3_retail_mission_camera_cinematics_are_inventoried`).

These records *orchestrate*; their event payloads reference other animation
names and small indices (observed structure, semantics unmeasured):

* `0x118` (80 bytes) carries a 32-byte NUL-terminated name —
  `speed_cue`, `pzep_engine_start`, `letterbox`, `gi_scene1`, `gi_scene2`,
  `gi_1stperson` inside `generic_intro`/`player_setup`; per #690 this type is
  opcode 24 `CALL_ANIMATION`, so the name is the *called animation*;
* `0x119`/`0x319` (48 bytes) carry a float and a name (`gi_scene1` at
  `15.9`, `gi_scene2` at `12.4`, `gi_1stperson` at `9.9` inside
  `generic_intro`'s scene block); per #690 this type is opcode 25
  `STOP_ANIMATION`, the float is payload word 0 = `START_TIME`
  (vocabulary evidence), and the same values are the start times #690
  measured for `generic_intro`'s four stops — its measured duration 15.9 is
  this record's last stop. Whether the stored unit is seconds or something
  else remains **unmeasured** (#690's `f20-anim` time-unit gap);
* `0x106`/`0x10f`/`0x110`/`0x123` (12..20 bytes) carry small integer words
  that track the record's node-table and animation-ref indices;
* `0x10c` (76 bytes) appears once per `gi_scene` with the scene's index
  (`0`, `1`, `2`, `3`);
* the record's `objects`/`nodes` tables name world nodes (`camera1`,
  `player`, `piratefighter`, `world1`, ...), so the cinematic's *subject* is
  the world's `camera1` node.

Two measured facts bound what a camera cinematic can contain:

* **`CAMERA_STATE` (type `0x114`, opcode 20) occurs inside no camera
  cinematic.** The installation itself names one opcode `CAMERA_STATE`
  (#690's census: 10 events corpus-wide), but all 10 live in
  `gi_1stperson` (the eight camera carriers), `C1/M02`'s `cabpkup_player`
  and `C1/M04`'s `1stperson` — none in a `camera1`-rooted record — and it is
  a single 48-byte statement with no timing, not a track. The
  installation's own camera-named event is therefore not the missing pose
  source either.
* **Two of #690's four unjoined opcodes live in a camera cinematic.**
  `ZBD/C1/M04`'s `mission_intro_animation` reset block carries exactly one
  `0x11a` (opcode 26) and one `0x11c` (opcode 28) event — types no
  declaration in the installation joins (`f20-anim.event-opcode-not-measured`).
  #690's decoder refuses such records; this task's survey walks them
  verbatim and reports the types, so the mission-intro cinematic's two
  unknown events are neither dropped nor guessed at.

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

#690's decoded payload semantics do not yield keyframes either: what it
measured for camera cinematics is *statement timing* (`START_TIME`,
`RUN_TIME`, and per-record durations such as `generic_intro`'s 15.9 in the
original's stored unit) — spans over a statement list, not a pose track; and
the stored unit, the tick rate and every transform field remain its named
unknowns (`f20-anim.tick-rate-unmeasured`,
`f20-anim.event-pose-transform-not-decoded`).

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
| the event TLV grammar (8-byte header, `total_bytes`, exact tiling) | `ObservedTool` | 242 391 events of all 61 retail carriers tile exactly; zero counterexamples; #690 measured the same grammar independently |
| the 59 type ids and their fixed sizes | `ObservedTool` | measured across every carrier each type occurs in; the size table pins them in code |
| the `camera1` classification rule and the full 31-carrier inventory | `ObservedTool` | a description of the files, not of the engine; pinned carrier-by-carrier by the retail inventory test |
| `CAMERA_STATE` (`0x114`) occurs in no camera cinematic | `ObservedTool` | the 10 corpus occurrences are enumerated; the inventory test pins the exact type set inside camera cinematics |
| what a payload field means; what the second byte of a type id selects; whether the stored time unit is seconds | partly **#690-measured**, otherwise **unknown** | opcode spellings and the two timing words are #690's measured claim ids; node indices, vectors, names and the unit are unmeasured; candidates listed above |
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
- `crates/cs_app/tests/accept_f40_d3_camera_anim.rs` (**new**): six
  acceptance tests — four synthetic (TLV parse, fail-closed refusals, the
  pinned size table, the unknown-declaration/lowering-refusal pair) and two
  `#[ignore = "requires CS_GAME_DIR"]` retail tests: the eight-carrier
  survey (re-measuring every stream, type and size on each run) and
  `accept_f40_d3_retail_mission_camera_cinematics_are_inventoried`, which
  walks **every** mission carrier and pins the full inventory — the exact
  per-carrier camera-cinematic names of all 31 carriers, the 58-record
  total, the exact 21-type event set inside camera cinematics, and that
  `CAMERA_STATE` (`0x114`) is not among them (added in review; see below).

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
  alone) fails the retail test's common-name assertions and the full
  inventory test;
* the corrected carrier attributions (`mission_intro_animation`/`scene1` in
  `c1/m04`, not `c4/m01`; `bdrop_ew_cam`/`bdrop_we_cam` in `c4/m03`) are
  pinned by `accept_f40_d3_retail_mission_camera_cinematics_are_inventoried`,
  so a survey that mis-attributes a record to the wrong carrier fails it;
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

* **This module decodes no payloads.** The TLV grammar gives boundaries and
  types, not meanings; an animation still cannot be *played* from this data.
  Payload semantics are partly measured by #690 for the same bytes (opcode
  spellings, `START_TIME`, `RUN_TIME` for five opcodes) — see
  `docs/findings/2026-10-06-f20-event-grammar.md`; the remaining fields
  (node indices, vectors, names, the stored time unit) are its named
  unknowns, and completing them is the natural follow-up.
* **No camera pose keyframes** are established; the F40-B `CameraTimeline`
  remains designed-only for original data, now with measured evidence for
  why — including that the installation's own `CAMERA_STATE` event occurs in
  no camera cinematic and is a single statement, not a track.
* The type-size table is measured over the **retail installation this
  repository tests against**; a modded or other regional install could carry
  other sizes, which the survey would report as mismatches rather than trust.
* `0x12f`'s two shapes are not told apart; a consumer that needs to index
  into its payload cannot, and must not, guess.
* Nothing is `verified_original`: no original executable ran, and no human
  viewed or played anything (`human_play`/`human_review` are owner-gated).

## Review (2026-10-13)

Implementer: **bunny-alpha-1/bunny-alpha-1** (session of 2026-10-13).
Reviewer: **bunny-alpha-1/bunny-alpha-1 — the same agent identity, a fresh
session** that started from `request_work` and re-measured from the branch
diff and the installation. Because the identity is the same, this is a
self-review: `checked` is its ceiling and it is not independent
original-reference evidence; a fresh-identity or owner review of the format
claims is still owed. What the review changed:

* **A carrier mis-identification was corrected.** The first draft named
  `ZBD/C4/M01` as the mission carrier of `mission_intro_animation`/`scene1`.
  A full-installation scan (all 61 carriers, the same classification rule)
  measured those two records in **`ZBD/C1/M04`**; `C4/M01`'s `mis_anim.zbd`
  holds no camera cinematic at all. The scan also found camera cinematics the
  first draft did not report: `bdrop_ew_cam`/`bdrop_we_cam` in `ZBD/C4/M03`
  and `multiplayer_setup` in the 21 multiplayer mission carriers — 58
  camera-cinematic records in 31 carriers in total, not the ~11 the draft's
  table implied. The inventory is now pinned carrier-by-carrier by
  `accept_f40_d3_retail_mission_camera_cinematics_are_inventoried`.
* **The missing cross-reference to #690 was added throughout.** Task #690
  (`docs/findings/2026-10-06-f20-event-grammar.md`, production module
  `cs_app::animation::events`) had already measured this exact two-word
  grammar over this exact corpus and decoded opcode spellings, `START_TIME`
  and `RUN_TIME`; the draft presented the grammar as new and said "no field
  inside a payload has a measured meaning", which is not true of this
  repository's measurements. The finding and the module docs now state the
  relation (same grammar, different keying: the 59 type ids decompose into
  #690's 35 opcodes × their `1`/`2`/`3` second byte) and defer payload
  semantics to #690's claim ids.
* **Two measured facts were added that bound the negative result:**
  `CAMERA_STATE` (`0x114`, the one opcode the installation itself names for
  the camera) occurs in **no** camera cinematic (all 10 corpus events are in
  `gi_1stperson`/`1stperson`/`cabpkup_player`) and is a single statement,
  not a track; and `C1/M04`'s `mission_intro_animation` reset block carries
  one event each of #690's unjoined opcodes 26 and 28 — the mission-intro
  cinematic therefore contains events whose meaning no declaration in the
  installation joins.
* **The census was independently re-run** from the reviewer's session:
  61 carriers, 56 994 streams, 242 391 events, 0 layout issues, 58/59
  single-size types, `0x12f` = `[548, 1156]` — identical to the reported
  numbers, and the block/event totals equal #690's independently published
  census.
* No production behavior changed in review; the code changes are
  documentation (module docs, `declared_camera_sequence`'s doc wording) plus
  the one added retail test.
