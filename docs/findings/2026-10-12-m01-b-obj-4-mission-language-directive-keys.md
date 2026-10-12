# M01-B-OBJ-4: the nineteen mission-language directive keys, measured

Date: 2026-10-12. Task: #1361 `M01-B-OBJ-4` (parent #1355 `M01-B-OBJ`). Test prefix
`accept_m01_b_obj_4_`. Capabilities used: `retail` (read of `$CS_GAME_DIR`) and
`CS_ENGINE_IMAGE` (read-only static analysis of `crimson.decrypted.exe`, PE32,
2,580,480 bytes, sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` — matches the
hash every M01-LC-DIRECTIVE finding cites). **No original executable was run**;
nothing here is `verified_original`. Everything below is a reading of the
executable's code, not an observation of behavior. Addresses are virtual
addresses in the image.

This note supersedes the `Unmeasured` classification the census gave the 19 keys
the task names. Fourteen of them were already measured by the four
M01-LC-DIRECTIVE findings; this stage measured the five record-level keys the
record parser reads outside the numbered blocks, joined the two bodies of
evidence in production and recorded what stays unknown.

## The census: 50 fields, two populations

`ObjectiveRecovery` reads `zbd/c1c/m01`'s control record (`zrdr.zbd`, the
`objectives` member, 358 fields in all: 5 record-level, 353 inside the 58
`OBJECTIVE<N>` blocks). The 19 keys account for **50 fields**, split by
position, and the split is structural, not a count coincidence:

| Key | Sites | Position in M01 | Measured at |
| --- | --- | --- | --- |
| `MISSION_TIMER` | 1 | record top level | `0x466c31`–`0x466ceb` |
| `PLAYER_INIT` | 1 | record top level | `0x46777d`–`0x467891` |
| `RESTORE_ANIMS` | 1 | record top level | `0x466e47` |
| `EXECUTE_ANIMS` | 1 | record top level | `0x4674d3` |
| `INVALIDATE_ANIMS` | 1 | record top level | `0x46718f` |
| `ADD_OBJECTIVE_TARGET` | 4 | blocks 2, 11*, 17, 47, 50* | finding B |
| `REMOVE_OBJECTIVE_TARGET` | 4 | blocks 2, 11, 15, 50 | finding B |
| `ADD_OTHER_TARGET` | 3 | blocks 2, 13, 50 | finding B |
| `ANIM_STATE` | 3 | blocks 11, 15, 18 | finding C |
| `COMPLETED_STOPPOINT` | 1 | block 13 | finding B |
| `DEDG` | 8 | blocks 2, 16, 17, 20, 21, 42, 43, 44 | finding B |
| `SET_AI_NET` | 3 | blocks 2 (×2), 19, 22 | finding B |
| `SET_HELP_LABEL` | 2 | blocks 2, 50 | finding D |
| `STOP_QUEUED_SOUNDS` | 3 | blocks 11, 15, 55 | finding D |
| `TRAVELERS` | 1 | block 3 | findings B, C |
| `WAKEUP_ENEMIES` | 1 | block 13 | finding C |
| `WAKEUP_GENERATOR` | 4 | blocks 9, 13, 27, 45 | finding C |
| `WAKEUP_ZEP_TURRETS` | 3 | blocks 1, 5, 13 | finding C |
| `WAKE_ANIM` | 5 | blocks 11, 13, 15, 18, 49 | finding C |

\* block 11's `REMOVE_OBJECTIVE_TARGET` names the same object its
`ADD_OBJECTIVE_TARGET` in block 47 re-adds — the sites named are the spelled
ones, no effect is inferred.

**The five record keys appear nowhere else.** M01 never spells one inside a
block; their only five occurrences are the top-level fields the record parser
reads before the `OBJECTIVE%d` loop (the finding-A sequence `0x466b70` area, the
keys checked between `MISSION_TIMER` at `0x466c31` and `PLAYER_INIT` at
`0x46777d`, sound keys in between). Two consequences fall out of the engine's
first-match lookup (`0x57a090` → `0x579ff0`), which descends into nested lists
in document order: a repeated record key is never read twice, and a key spelled
*only* inside a block **would** be found by the record-level read — the model
reads top-level fields only and a nested-only spelling stays an acknowledged
gap (no retail record spells one).

## The record-level parse, measured

Every record-level key resolves through the same first-match lookup and every
missing key leaves the field it would have written untouched (zeroed at mission
record construction). What each parse does:

### `MISSION_TIMER` — `[real|int, "NOLOSS"?]`

Parse `0x466c31`–`0x466ceb`. Element 0 is read **unconditionally** (no element →
an out-of-bounds read in the original, treated as a shape error here): a tag-2
real's bits are copied, a tag-1 integer is converted (`fild`), tags 3 and 4 walk
the engine's own error path — `"Expecting real value, found string %s"` /
`"…reader array"`, diagnostics that cite `mission.cpp`. The value is handed to
`0x46c510(timer_obj)`, which stores it twice at the timer object `0x71b468`: the
raw `f32` at `+4`, and `value × 1000f` truncated to milliseconds at `+8`.

When a second element exists it is compared — `repe cmpsb`, 7 bytes including
the terminator — against the literal `NOLOSS`: an exact match calls
`0x46c540(timer_obj, 1)`, writing `1` to `+0x14`. Any other second element is
**silently ignored**, and elements past the second are never read. Per finding
D's start rule, the countdown starts only when the stored seconds are positive:
M01's `[0r]` never starts the timer. What `+0x14` gates beyond that reading
stays unmeasured.

### `PLAYER_INIT` — `[i, [r,r,r], [r,r,r], r, r]`

Parse `0x46777d`–`0x467891`. **Five elements are read unconditionally** — there
is no arity check, so a shorter declaration reads out of bounds in the
original:

| Element | Destination | Transform |
| --- | --- | --- |
| 0 | mission record `+0x6c0` | payload copied verbatim |
| 1, children 0–2 | `+0x6c4`, `+0x6c8`, `+0x6cc` | payload copied verbatim |
| 2, children 0–2 | `+0x6d0`, `+0x6d4`, `+0x6d8` | `fld` each payload, `fmul` by π/180 (`0x3c8efa35`) |
| 3 | `+0x6dc` | payload copied verbatim |
| 4 | `+0x6e0` | `fld` payload, `fmul` by `0.1f` (`0x3dcccccd`) |

M01 spells `[1, [-3694r, 1318r, -12482r], [0r, 170r, 0r], 0.8r, 580r]`: the two
triples are the position and the attitude — the attitude spelled in degrees,
stored in radians, matching the `player` record's spawn in `aiv.zrd` (the
player-config finding); element 0 matches the `player` record's index there.
The roles of elements 3 and 4 — what `0.8` and `580.0` configure — stay
**unmeasured**: nothing traces a consumer, and no unit or meaning is claimed.

### `RESTORE_ANIMS`, `EXECUTE_ANIMS`, `INVALIDATE_ANIMS` — one list grammar, three slots

The three keys are parsed by three copies of one loop (`0x466e47`…, `0x46718f`…,
`0x4674d3`…). For each key the parser allocates a 12-byte list record
unconditionally — `{sentinel, head, count}` — so an absent key and an empty
list leave the same empty list. Per element:

* A **tag-3 string** resolves through the global animation table (`0x523820`:
  entries of `0x110` bytes at `0x9fd14c`, entries with `+0xa0 == 5` skipped, a
  byte-wise name compare at entry `+0`); a hit's handle is appended.
* A **tag-4 list of exactly three children** — `[object, "X#", anim]` — is the
  scoped form. `object` resolves through the kind-7 registry (`0x4d0280`, kind
  `7` pushed at `0x4672bb`). `X#` must end in `#` (`0x46728a`); the selector is
  copied out **minus** the `#` (`0x4672a0`–`0x4672c3`). The stripped selector
  either names the resolved object itself — the parse compares it against the
  object's own name — or selects named descendants through the object's
  `+0x56`-counted `+0x5c` child array (`0x46d280`). `anim` resolves on each
  matched object with `0x5238a0`, which requires the table entry's `+0x48`
  owner field to equal the object. Each hit's handle is appended, in walk
  order.
* **Everything else is skipped, never an error**: a non-string non-list
  element, a list that is not exactly three children, or a triple whose
  selector lacks the `#` terminator — and any resolved name that finds no
  table entry. A malformed spelling silently contributes nothing.

The accumulated lists store at mission record `+0xc80` (`RESTORE_ANIMS`),
`+0xc84` (`INVALIDATE_ANIMS`) and `+0xc88` (`EXECUTE_ANIMS`) — confirmed at
`0x467168`, `0x4674ac` and `0x46775a` respectively. M01 spells all three as
`[]`; the three stores still happen, each writing an empty list. **What reads
the three fields, and in what order the lists' semantics differ** — the names
suggest restore/execute/invalidate — is untraced and stays unknown.

## The fourteen block-level keys

The block directive parser's measured readings are established by the
M01-LC-DIRECTIVE findings; the production table `measured_directive` carries
each key's operation, argument shape and remaining unknowns. For completeness:

| Key | Operation | M01 site(s) |
| --- | --- | --- |
| `ADD_OBJECTIVE_TARGET` | `SetTargetFlag{objective,set}`: resolve each name chain, set the leaf's `+0x4d` flag at completion | 2 `["workersvoyagezep"]`, 17 `["pzhookpoint"]`, 47/50 `[["wv_tailhook","peoplehook"]]` |
| `REMOVE_OBJECTIVE_TARGET` | same resolution, flag cleared | 2/11/15 `[["piratezep","rock_zeppelin"]]`/`[["wv_tailhook","peoplehook"]]`, 50 `["workersvoyagezep"]` |
| `ADD_OTHER_TARGET` | the `+0x4c` other-target flag, set | 2 `[["piratezep","rock_zeppelin"]]`, 13 `["blackswanzep"]`, 50 `["workersvoyagezep"]` |
| `ANIM_STATE` | `AnimationStates`: fire when ≥ required listed anims hold the state (`RUNNING`/`EXECUTED`/`INVALID` → 2/3/4) | 11 `RUNNING wv_drop_copilot`, 15 `EXECUTED wv_pickup_copilot`, 18 `EXECUTED hooked_to_klondike` |
| `COMPLETED_STOPPOINT` | `AdvanceStopPoint`: forward the `{int,bool}` pair to the named stoppoint's advance/select handler | 13 `[["SwanZep1",1,0]]` |
| `DEDG` | `EnemyGroupDepletion`: fire when group `child0` has ≤ `child1` members in play, plus pending generator spawns; evaluating rewrites three member fields (`+0x318/+0x31c/+0x320`, untraced) | 8 sites, `[group, remaining]` |
| `SET_AI_NET` | `AssignNet`: point the named vehicle/zeppelin's AI-net entry at the named network | 2 (two `devastator` pairs), 19 `bsfury_1→BlackSwan`, 22 `blackswanzep→SwanZep2` |
| `SET_HELP_LABEL` | `SetHelpLabel`: give the resolved object the localized label | 2, 50 (each `[names, " "]`) |
| `STOP_QUEUED_SOUNDS` | `StopQueuedSounds`: flag matching queued-sound entries, scheduled removal | 11, 15, 55 |
| `TRAVELERS` | `Travelers`: named subject inside/outside the anchor radius — or counting mode | 3 `["player","APPROACHING","workersvoyagezep",700r,1]` |
| `WAKEUP_ENEMIES` | `WakeEnemies`: on wake, wake the named actors that are asleep | 13 `["bsfury_1","blackswanzep"]` |
| `WAKEUP_GENERATOR` | `FeedGenerator`: on wake, add the spelled count to the generator's pending spawns | 9/27 `["workersvoyagezep",3]`, 13 `["blackswanzep",2]`, 45 `["blackswanzep",4]` |
| `WAKEUP_ZEP_TURRETS` | `WakeZeppelinTurrets`: on wake, activate the named zeppelin-turret node | 1 `["piratezep"]`, 5 `["workersvoyagezep"]`, 13 `["blackswanzep"]` |
| `WAKE_ANIM` | `WakeAnimation`: on wake, execute the named animation | 11/49 `["activate_dropoff_node"]`, 13 `["fadein_bszep"]`, 15 `["activate_pickup_node"]`, 18 `["pzhomebase"]` |

Every one of these is a **world-side effect**, not a declared-objective field:
the declared program cannot represent them, so they stay unrecovered — now
named `ControlDirective` with their measured operation, never `Unmeasured` and
never lowered from a guess.

## What changed in production

* `cs_content::objectives` gained the record-level model:
  `RECORD_FIELD_KEY_VOCABULARY` (the five keys), `RecordScalar` (the spelled
  `Int`/`Real` payload and the engine's `fld` reading of it), `MissionTimerField`
  (`seconds`, `no_loss`), `PlayerInitField` (`head`, `position`,
  `attitude_degrees`, `tail`, `scaled_input` — the measured transforms made
  explicit, the unmeasured roles named only by position), `AnimListField`/
  `AnimDirective` (the keep/skip list grammar), `MeasuredRecordFields` and
  `measure_record_fields`, with `RecordFieldError` for spellings outside the
  measured shape.
* `cs_app::objectives` gained two families. `RecordDirective` covers the five
  record-level keys; `ControlDirective` covers every block-level key
  `measured_directive` measures — the 14 in this task plus the rest of the
  measured directive vocabulary (target, team, counter, zone, timer and
  sibling keys M01 does not spell). `ObjectiveRecovery::record_fields()`
  exposes the parsed preamble beside the per-field census.
* Nothing was lowered. `program()` still refuses: the families' effects are
  world-side operations and record preamble, not declared-objective fields,
  and the unmeasured families (dormancy unit/reveal, identity labels, sound
  cue timing, optionality) are named unchanged.

## Unknowns recorded

* `PLAYER_INIT` elements 3 and 4 have no measured consumer; element 0's
  selection is a measured correlation with the `player` record index, not an
  observed use.
* `MISSION_TIMER`'s `+0x14` flag is written for `NOLOSS`; what reads it is
  finding D's start-rule territory, not a new observation.
* The three animation-list stores (`+0xc80/+0xc84/+0xc88`) are measured; their
  consumers are untraced and the lists' semantic differences are a name
  reading only.
* `DEDG`'s evaluator writes three member fields (`+0x318/+0x31c/+0x320`) whose
  consumers are untraced world state (finding B).
* `TRAVELERS`' opposite-polarity spelling and the counting-mode's group id 0
  are unspelled/unknown (findings B, C).
* `WAKE_ANIM`'s three trailing animation-call arguments — M01 passes `0,0,0` —
  are unknown (finding C).
* A record key spelled only inside a block would be found by the engine's
  descending first-match lookup but not by this reader's top-level scan; no
  retail record spells one.
* These are static code readings. The parse writes are measured; the
  *behavior* of the world once the fields are set is not observed here.
