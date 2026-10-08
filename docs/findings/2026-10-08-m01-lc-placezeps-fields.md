# #791: the `placezeps.zrd` placement member's field grammar

Date: 2026-10-08. Task: #791 (`M01-LC-PLACEZEPS-FIELDS`), filed by #359
(`VS-M01-RUNTIME`). Feature sheet: `specs/F20-object-animation-and-authored-destruction-states.md`.
Capabilities used: **`retail`** (read-only `$CS_GAME_DIR`) and the owner-supplied
decrypted image for static analysis (sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, image base
`0x400000`, file offset = VA − `0x400000` for `.text`/`.rdata`; disassembled
with the platform `objdump`, every address below re-read in the disassembly).
Test prefix: `accept_m01_lc_placezeps_fields_`. Nothing here is
`verified_original`: no original run happened.

## The answer in one paragraph

The task assumed `placezeps.zrd` was a sibling of `zeppelins.zrd` with `node` /
`position` / `yaw` / `pitch` / `max_speed` / `max_accel` fields. **It is not.**
It is an `ANIMATION_DEFINITIONS` document (the same record every animation
definition member opens with) whose three `ON_STARTUP` definitions each hold a
`SEQUENCE_DEFINITION` of **state statements**: an `OBJECT_TRANSLATE_STATE`
(`NAME` + `STATE` of three numbers) and, for two of the three, an
`OBJECT_ROTATE_STATE` (`NAME` + `STATE` of three numbers). There is no `pitch`,
`max_speed` or `max_accel` in it, and no key is called `position` or `yaw`. The
`PLACEMENT_FIELDS_CLAIM` text named fields the member does not have.

## The member (production reader over the read-only installation)

`ZBD/c1c/m01/zrdr.zbd :: placezeps.zrd`: 1 535 bytes at archive offset 49 213
(`SourceSpan` from the production `discover_container`). **Only M01's reader
archive carries a member of this name**: a census of every
`ZBD/**/zrdr.zbd` finds exactly one. It decodes with
`cs_formats::zbd::placezeps::read_placezeps_member` with no byte left over.

Grammar (member-relative bytes; every word a little-endian `u32`; the `.zrd`
node rule of `zeppelins.rs`): `list(list("ANIMATION_DEFINITIONS",
list("ANIMATION_LIST", "ANIMATION_DEFINITION" list(...) ...)))`. A definition
holds `NAME`, `ANIMATION_NAME`, `ACTIVATION`, `RESET_TIME`,
`SEQUENCE_DEFINITION`.

| def | `NAME` | `ANIMATION_NAME` | definition bytes | sequence | sequence bytes |
| --- | --- | --- | --- | --- | --- |
| 0 | `piratezep` | `placepiratezep` | 111..595 | `placement` | 313..595 |
| 1 | `workersvoyagezep` | `placeworkersvoyagezep` | 623..1133 | `up_down` | 839..1133 |
| 2 | `blackswanzep` | `placeblackswanzep` | 1161..1535 | `up_down` | 1369..1535 |

All three: `ACTIVATION ON_STARTUP`, `RESET_TIME` word `0xffffffff`.

| def | statement | `NAME` bytes | `STATE` bytes | stored `STATE` (all tag-1 words, read signed) |
| --- | --- | --- | --- | --- |
| 0 | `OBJECT_TRANSLATE_STATE` | 408..433 | 446..478 | −3584, 1360, −8704 |
| 0 | `OBJECT_ROTATE_STATE` | 525..550 | 563..595 | 0, 180, 0 |
| 1 | `OBJECT_TRANSLATE_STATE` | 932..964 | 977..1009 | −5972, 1460, −7680 |
| 1 | `OBJECT_ROTATE_STATE` | 1056..1088 | 1101..1133 | 0, 180, 0 |
| 2 | `OBJECT_TRANSLATE_STATE` | 1462..1490 | 1503..1535 | −6656, 1960, −5632 |

Definition 2 (`blackswanzep`) states **no** rotation. Every statement's `NAME`
equals its definition's `NAME`, which selects exactly one `gamez.zbd` record
(the node join, resolved by the existing `AnimationTarget` machinery). No key
outside the vocabulary appears in M01's member: `unmodelled_fields` is empty for
all three.

## What the executable does with the statements (decrypted image)

- **Keyword dispatch**: `0x51cc20` compares the statement keyword and calls
  `0x5075b0` for `OBJECT_TRANSLATE_STATE` (string `0x633594`) and `0x505ff0`
  for `OBJECT_ROTATE_STATE` (string `0x63356c`).
- **Event record**: each parser allocates a `0x20`-byte record; byte `+0` is
  the opcode (`0x7` translate at `0x507609`, `0x9` rotate at `0x506049`), `+1`
  is `1`, `+4` is `0x20`. This is the same event the binary sequence blocks of
  `mis_anim.zbd` carry as opcodes 7 and 9 (F20 event grammar,
  `docs/findings/2026-10-06-f20-event-grammar.md`).
- **`STATE` numbers**: per element a tag-`2` node is stored as the float
  (`0x507710`), a tag-`1` node is **signed**: `fild dword ptr [..]` then
  `fstp` (`0x507720`..`0x507723`), any other tag raises an error string
  (`0x62f008`). The three results land at event `+0x10`, `+0x14`, `+0x18`
  (`0x507713`, `0x507919`, `0x507b1f`). Hence `0xfffff200` is −3584.
- **Rotation units**: after the three numbers, the rotate parser multiplies
  each by the `f64` at `0x6040e8` = `0.01745329251994` (degrees to radians;
  `0x506df9`..`0x506e1a`). The rotate `STATE` is therefore **degrees**, and
  the event keeps radians.
- **`NAME`** (`0x62f234`) is resolved to a node index stored at event `+0x1c`
  by `0x4ef7d0` (`0xffff` = no such node, a parse error). Optional keys the
  parsers also read, none present in M01's member: `START_TIME` (`0x62effc`
  → event `+0x8` via `0x4ef270`), `AT_NODE` (`0x62f1a4` → event `+0x1e`) and
  `RELATIVE` (`0x6313d4`, sets bit 0 of event `+0xc`).
- **Execution**: the translate executor is `0x4e8de0`: it starts from a zero
  base (or the `AT_NODE` node's position), **adds** `+0x10/+0x14/+0x18`, and
  with `RELATIVE` clear calls the absolute setters `0x4d1d50` / `0x4d2710`
  (object class 5 / 1), which store the triple, in order, at the node-state
  slots `+0x54`, `+0x58`, `+0x5c` and mark the node dirty. The rotate
  executor is `0x4e8b80`: with `RELATIVE`/`AT_NODE` clear it calls `0x4d1a30`,
  which stores the three radians in order at node-state `+0x18`, `+0x1c`,
  `+0x20`. So **translate x, y, z = stored order, no axis swap**, and an
  M01 placement is an absolute assignment, not an offset.

## What the decoder and binding carry

- `cs_formats::zbd::placezeps` (new): every field above, with member-relative
  byte ranges; `StateNumber::Int(i32)`/`Float(f32)`; `StateStatement::state()`
  (the converted numbers) and `parsed()` (rotate multiplied by the image's
  constant). A key outside the vocabulary is an `UnknownField` under
  `f20-anim.placezeps-field-unmodelled`; `RESET_TIME` is read and stays
  unknown under `f20-anim.placezeps-reset-time-unmeasured`.
- `cs_app::animation::MissionAnimationBinding::measured_placements()` (new):
  per declared placement, `node` (the join) and `translation` /
  `rotation_degrees` / `rotation_radians` as `Resolved::Known` under
  `f20-anim.placement-state-statements` (`ObservedTool`) with the `STATE` list's
  archive byte span; `yaw_degrees` as `Resolved::Unknown` under
  `f20-anim.placement-rotation-axis-unmeasured`.
- `PLACEMENT_FIELDS_CLAIM` (`f20-anim.placement-member-fields-undecoded`)
  stays, **narrowed** (`PLACEMENT_FIELDS_REASON` rewritten): it now covers only
  the composition of stored states into a pose, `RESET_TIME` and unmodelled
  keys. It no longer names `node`, `position`, `yaw`, `pitch`, `max_speed` or
  `max_accel`.

## What stays unknown

- **Which rotation component is the heading** (`f20-anim.placement-rotation-axis-unmeasured`).
  The middle number is 180 for both rotated ships and the translate triple
  reads (x, y≈altitude, z) with y around 1 400, but the matrix build that
  consumes node-state `+0x18..+0x20` was not traced to a named axis, so the
  middle component is **not** called yaw by position alone.
- **Composition into a world pose.** The executor writes node-state slots; how
  those compose with the gamez record's own transform (the world records store
  identity rotations for these nodes, `2026-10-08-m01-lc-world-actor-spawn.md`)
  and what unit the slots are in were not traced. The metre unit of #436 is
  corroboration for magnitude only (these ships sit in the same extent as the
  `zeppelins.zrd` poses), not a claim made here.
- **`RESET_TIME` `0xffffffff`.** Read, no consumer traced.
- **`START_TIME` / `AT_NODE` / `RELATIVE`** are parsed by the executable but
  absent from M01's member; the decoder reports any of them as an unmodelled
  statement key rather than guessing their effect.

## The two carriers state different poses for the same nodes

`zeppelins.zrd` (#574, `read_zeppelins_member`) and `placezeps.zrd` both place
the same three nodes, and they disagree:

| node | `zeppelins.zrd` position | `placezeps.zrd` translate | `zeppelins.zrd` yaw | `placezeps.zrd` rotate |
| --- | --- | --- | --- | --- |
| `piratezep` | (−3678.6, 1460.0, −11985.3) | (−3584, 1360, −8704) | 180 | (0, 180, 0) |
| `workersvoyagezep` | (−4912.1, 1360.0, −9120.0) | (−5972, 1460, −7680) | 220 | (0, 180, 0) |
| `blackswanzep` | (−8914.0, 1250.0, 4767.6) | (−6656, 1960, −5632) | 340 | none |

Observations, not conclusions: the two carriers agree on `piratezep` yaw only;
the altitude numbers 1 360 and 1 460 appear in both but are **swapped** between
`piratezep` and `workersvoyagezep`; `blackswanzep` differs in every component.
(The earlier finding quoted `piratezep` z = −5888 for `placezeps.zrd`; the
member states −8704.) Which mechanism the original applies last — the startup
animation's absolute assignment or the `zeppelins.zrd` spawn — is unmeasured and
is the question of #792 (`M01-LC-ZEPPELIN-ATTITUDE`). This task binds both
carriers' values side by side and picks neither.

## Residue and the tasks that can lift it

- Which rotation slot is the heading, and the compose into a world orientation:
  #792, or a trace of the matrix build that reads node-state `+0x18..+0x20`.
- Which carrier wins for the shared nodes: #792.
- `RESET_TIME`'s consumer: unassigned.
