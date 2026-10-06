# M01-LC-DIRECTIVE-A: the `CZMission` objective directive parser, mapped in the retail executable

Date: 2026-10-06. Task: `M01-LC-DIRECTIVE-A` (#679), "locate mission.cpp's
objective directive dispatch in crimson.decrypted.exe". Parent:
`M01-LC-DIRECTIVE-MEANING` (#675). Capability used: `retail` (read-only
`$CS_GAME_DIR`). Evidence report:
`private/evidence/M01-LC-DIRECTIVE-A/acceptance.json`, committed as
`docs/findings/evidence/M01-LC-DIRECTIVE-A.json`.

**Everything below is static code evidence** from disassembling the owner's
executable. The original program was never run for this task; nothing here is
`verified_original` runtime behavior. Where a semantic could not be recovered
from the code alone it is recorded as **unknown** rather than guessed.

## Provenance

| Item | Value |
| --- | --- |
| File | `$CS_GAME_DIR/crimson.decrypted.exe` |
| Format | PE32 executable, image base `0x400000` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` |
| Address convention used below | virtual address (VA); for the region studied `VA = file offset + 0x400000` |
| Directive string table | file offsets `0x226040..0x2268c0` (VA `0x626040..0x6268c0`), `MISSION_WON_SOUND..NAP_OBJECTIVE_WHEN_I_COMPLETE` |
| Error/assert logger | `0x415330(message, line, file, 0x200)`; `file` resolves to `D:\zipper\Crimson\mission.cpp`; observed line numbers `0x8fc..0xff3` (decimal `2300..4083`) |
| Tools | `r2`/`rabin2`/`objdump` on a read-only copy; Kuna v1.692 for orientation |

## The `.zrd` document model the parser walks

A decoded `.zrd` member is a **flat alternating stream of `Text(key)` children
and `List(value)` children** — measured both in the production decoder
(`cs_content::stunts`, tag `4` = `u32 count` followed by `count - 1` children)
and in this parser. A "record" is therefore `Text("KEY"), List(args)` pairs,
and nesting means a `List` child containing more pairs. M01's
`zbd/c1c/m01/objectives.zrd` top level spells mission-level pairs
(`MISSION_TIMER`, `PLAYER_INIT`, the anim and sound keys) then one
`Text("OBJECTIVE<N>"), List(...)` pair per objective; each objective value is
itself a flat key/value stream of directive pairs.

### In-memory node model (measured)

* Field lookup **`0x57a090(container_node, "KEY")`** is `0x579ff0(container,
  "KEY", 1)`: it scans the container's children from record 1 for a `Text`
  element equal to the key. The compare is a case-sensitive inline `strcmp`,
  and a `List` child is searched **recursively** (depth-first), so a key
  nested inside a value list is also found. On a match it returns **the
  record immediately after the matched `Text`** — the value list for a
  `Text,List` pair — whatever its tag. `0` when absent.
* Two value-extractor variants share the idiom. **`0x57a0f0(container,
  "KEY", start)`** runs the same case-sensitive compare through `0x579f70`
  but over a **flat** scan (no recursion into `List` children) and returns a
  `char*`: found tag `3` → its payload; found tag `4` → the first child's
  payload when that child is tag `3`; else `0`. The `NAME`/`STATE` queries
  inside `ANIM_STATE` spec records use it (e.g. `0x46927c`, called
  `(spec, "NAME", 1)`). **`0x57a1b0(container, "KEY", out*)`** reuses
  `0x57a090` and writes an integer through `out`: found tag `1` →
  `*out = payload`; found tag `4` → `*out =` the first child's payload when
  it is tag `1`; returns `1`/`0`. `COMPLETION_COUNT` uses it (`0x4693b5`).
  `_stricmp` appears in this parser only on **value** strings (IDENTITY's
  `PRIMARY`/`SECONDARY`/`TERTIARY`, ANIM_STATE's `RUNNING`/`EXECUTED`/
  `INVALID`, TRAVELERS tokens use `repe cmpsb`), never on key names.
* A node is `{tag@+0, aux@+4}`: tag `4` marks a list and `aux` is the element
  vector `V`. Several sites assert `[found+0]==4`.
* `V` is a vector of 8-byte `{tag@+0, payload@+4}` records at stride 8 from
  `V+0`. Record `0` (at `V+0`) is a header `{kind@+0 (observed =1),
  count@+4}` where **count = N children + 1** — so `[V+4]` is the element
  count including the header itself. Record `i` at `V+8·i` is child `i−1`
  for `i≥1`. In code terms: sites read child *j*'s tag at `[V+8+8j]` and its
  payload at `[V+0xc+8j]` for `j` in `0..N-1`, bounds-checked against
  `[V+4]` as `j+1 < [V+4]` or `[V+4] > needed` (e.g. `[V+4]>2` admits
  child 1).
* Value tags: `1` integer, `2` real (f32), `3` string (`char*`), `4` list
  (payload = the list's own element vector `V'` — confirmed by `SET_AI_NET`
  reading grandchildren through `payload+0xc`/`+0x14`).
* A "bare" directive is spelled `Text("KEY"), List([])`: the list exists with
  count `1` (no children). Presence-only sites only test whether the lookup
  returned nonzero.

### Real/integer argument idiom

Every real-valued argument uses the same dispatch: tag `2` → store payload as
f32; tag `1` → `fild` payload then store f32; tag `3` → log
`Expecting real value, found string %s`; tag `4`/other → log
`Expecting real value, found reader array.` Both error paths then still store
a stack scratch value; a malformed argument is an *error report*, not a parse
abort. `mission.cpp` line numbers in the log calls pinpoint each argument.

### The `CZMission` reader

* **`0x466b70`** — `CZMission`'s mission-file parser (`thiscall`, `ecx` =
  mission). Reads the document through `0x579c60` into `this+0x6f4`, reports
  `Failed to read %s` on failure, then walks mission-level keys with one
  `0x57a090` lookup each, then loops `sprintf("OBJECTIVE%d", n)` +
  `0x57a090` to fetch each objective's value list.
* **Objective records** are `0x5e4` bytes each (the per-objective update loop
  at `0x46a630` strides `esi += 0x5e4` per record over the array at
  `this+0xc4c`; mission fields live at `this+0xc4c`/`+0xc48` count bounds and
  `+0x6f0` mission clock, `+0x6f4` document).
* Inside an objective record the parser runs the same fixed sequence of
  `0x57a090(record, "KEY")` calls; **any key it does not look up is ignored**.
* `CZMission::Update` at **`0x46a490`**; per-objective tick at **`0x46a630`**;
  per-objective wake at **`0x469af0`**; state transition **`0x46b160`**
  (`objectives, index, mode`); completion-time objective kill **`0x46b130`**.
  Mission outcome setters: **`0x463c10`** sets mission WON (`this+0xc58`),
  **`0x463c20`** sets mission LOST (`this+0xc5c`).

## The key → handler map for M01's 43 spelled keys

Column "args read" uses `s`=string, `i`=int, `r`=real (tag 1 or 2), `l`=list,
`bare`=empty list presence. Offsets are inside the per-objective `0x5e4`-byte
record unless marked `[mission]` (the `CZMission` object). "cap 10" arrays are
fixed inline buffers the parser truncates at 10 entries. All entries are
**measured** — each is backed by the cited code; unresolved *semantics* are
listed under Unknowns, not guessed here.

### Outcome, identity and dependency

| Key | M01 shape(s) | Parse site | Args read | Record fields |
| --- | --- | --- | --- | --- |
| `INSTANTWIN` | bare | `0x468b95` | presence only | `+0x554 = 3` (instant-win class) |
| `INSTANTLOSS` | bare | `0x468bbb` | presence only | `+0x554 = 4` (instant-loss class) |
| `IDENTITY` | `[s,i]`, `[s,i,s]` | `0x468ae1` | requires `[V+0]==1`, `[V+4]≥3`, child0 tag 3, child1 tag 1 | child0 `_stricmp` `PRIMARY/SECONDARY/TERTIARY` → `+0x0 = 1/2/3` (no match leaves `+0x0`); child1 int → `+0x4`. **child2 (the `MSG_*` text, present in 4 of 5 M01 sites) is never read** — see Unknowns |
| `TICK_DEPENDS_ON_OBJ` | `[i]` | `0x4679e1` | child0 payload, raw | `+0x10 = arg−1` (1-based→0-based dep index); default `−1`; also clears `+0x588` |
| `DEDG` | `[i,i]` | `0x467a0a` | child0/child1 raw payloads; optional child2 (`[V+4]>3`, must be 3rd arg) strdup'd | `+0x580 = a0`, `+0x584 = a1`, `+0x588 = strdup(a2)` |
| `WAKE_OBJECTIVE_WHEN_I_COMPLETE` | `[i,…]` | `0x468c26` | each child payload, `dec`'d | `−1`-terminated 0-based index array at `+0x1c`; the earlier `WAKE_OBJECTIVE` lookup (`0x468c0f`) writes the same array — the two spellings are aliases at one field |
| `KILL_OBJECTIVE_WHEN_I_COMPLETE` | `[i,…]` | `0x468c5b` | each child payload, `dec`'d | `−1`-terminated 0-based index array at `+0x94` |
| `NAP_OBJECTIVE_WHEN_I_COMPLETE` | `[i,r]` | `0x468cd3` | child0 payload `dec`'d → `+0xd0`; child1 (gated `[V+4]>2`) r/i → `+0xd4`; missing child1 → `+0xd4 = 0x3e99999a` (f32 bits ≈ 0.3) plus error `NAP_OBJECTIVE_WHEN_I_COMPLETE has no nap_time specified.` |

### Lifecycle and timers

| Key | M01 shape(s) | Parse site | Args read | Record fields |
| --- | --- | --- | --- | --- |
| `BEGIN_DORMANT` | `[r]` ×52 | `0x467e09` | up to 4 children, each r/i | presence: `+0x5c8 = 0` (dormant) and `+0xc = 0` (inactive); absent: `+0xc = 1`, `+0x5d0 = 0`, `+0x5c8 = 1`. child0→`+0x5d0`, child1→`+0x5d4`, child2→`+0x5d8`, child3→`+0x5dc`. Defaults before parse: `+0x5cc=0`, `+0x5d0=0`, `+0x5d4=+0x5d8=+0x5dc=−1.0f` (`0xbf800000`) |

Runtime use (measured, `0x46a630` state machine on inactive objectives):
mission clock `this+0x6f0` reaching `+0x5d0` self-wakes via `0x469af0`; state 1
accumulates frame dt (`0x9ad744`) into `+0x5cc`; `+0x5cc ≥ +0x5d4` (>0) → state
2 via `0x46b160` and wakes the `+0xdc` list; `+0x5cc ≥ +0x5d8` (>0) →
self-wake again; `+0x5dc ≥ 0` and clock ≥ `+0x5dc` → state 3 via `0x46b160`
plus `+0xdc` wake. A `−1` duration reads as "no such transition".

### Wake lists, AI and targets

| Key | M01 shape(s) | Parse site | Args read | Record fields |
| --- | --- | --- | --- | --- |
| `WAKEUP_ENEMIES` | `[s,…]` | `0x468037` | children strdup'd, cap 10 | `+0x124` count, `+0x128..` names |
| `WAKEUP_ZEP_TURRETS` | `[s,…]` | `0x4681d2` | children strdup'd, cap 10 | `+0x29c` count, `+0x2a0..` names |
| `WAKEUP_GENERATOR` | `[s,i]` | `0x4688ee` | child0 tag-3 → strdup → `+0x118`; resolve `0x451720(ecx=0x654170, name)` → `+0x11c`; child1 (tag 1, gated `[V+4]>2`) → `+0x120` (set to `1` when the name resolves) | `+0x118/+0x11c/+0x120` |
| `SET_AI_NET` | `[[s,s],…]` | `0x468331` | each child must be a list; grandchildren payloads read as `char*` (no per-child tag check) → 8-byte `{s,s}` | `+0x398` count, `+0x3a0..` pairs, cap 10 |
| `TRAVELERS` | `[s,s,s,r,i]` | `0x467a70` | `[V+4]≥5` required (≥4 args). child0 tag3 → `0x465e30` resolve → `+0x594`, unresolved → `+0x58c` strdup, non-string → `+0x598` payload. child1 tag3 → `repe cmpsb "APPROACHING"` (12 B) → `+0x59c = 1/0`. child2 tag3 → resolve → `+0x5a0` else `+0x590` strdup; child2 tag4 → grandchildren 0,1 as reals → `+0x5a4`,`+0x5a8`. child3 r/i → `+0x5b0`, then **squared**. child4 (gated `[V+4]≥6`) raw payload → `+0x5b4` (default 1). child5 (gated `[V+4]≥7`, tag3) `cmpsb "DELETE_ON_SUCCESS"` (18 B) → `+0x5bc = 1` (default 0) | `+0x58c..+0x5bc`; `+0x5b8` defaulted 0 |
| `ADD_OTHER_TARGET` | `[s]`, `[[s,s]]` | `0x4684b9` | per child: `0x465fa0` builds a 20-byte string-vector record (child tag3 → 1 string; tag4 → **all** grandchildren payloads appended); pointer appended to a `realloc`'d array | `+0x4bc` count, `+0x4c0` array |
| `REMOVE_OTHER_TARGET` | — (parser supports) | `0x46856c` | same | `+0x4c4`/`+0x4c8` |
| `ADD_OBJECTIVE_TARGET` | `[s]`, `[[s,s]]` | `0x46861f` | same | `+0x4cc`/`+0x4d0` |
| `REMOVE_OBJECTIVE_TARGET` | `[[s,s]]`, `[s]` | `0x4686d2` | same | `+0x4d4`/`+0x4d8` |
| `COMPLETED_STOPPOINT` | `[[s,i,i]]` | `0x46842d` | each child = list; grandchildren payloads: [0] strdup, [1] raw int, [2] nonzero→byte | `+0x440` count, `+0x444..` 12-byte `{name,int,bool}` records, cap 10 (the cap check is at `0x468465`) |
| `STOP_QUEUED_SOUNDS` | `[s,…]` | `0x468884` | children strdup'd, cap 10 | `+0x510` count, `+0x514..` names |
| `COMPLETED_SOUND_GROUP` | `[s]` | `0x468a8a` | child0 payload → `0x596120` sound-group lookup | `+0x550` handle |
| `WAKEUP_SOUND_GROUP` | `[s]` | `0x468a4a` | child0 payload → `0x596120` | `+0x54c` handle |

### Condition blocks

| Key | M01 shape(s) | Parse site | Args read | Record fields |
| --- | --- | --- | --- | --- |
| `INACTIVE1`..`INACTIVE18` (loop is `INACTIVE%d`, `d=1..100`) | `[s]`, `[s,s,s]` | `0x469070` (`sprintf` loop) | per row via `0x469180`: child0 string → `0x465e30` object resolve; children 1.. each → `0x4cc990(handle, name)` chained member lookup; returns last handle, 0 breaks | resolved handles collected in a stack buffer → `calloc`'d array `+0x564`, row count `+0x560` (only nonzero resolutions counted) |
| `INACTIVE_COMPLETION_COUNT` | `[i]` | `0x46911c` | child0 payload raw | `+0x55c`; default = `+0x560` when key absent |
| `ANIM_STATE` | `[s,[s,[s],s,[s]]]` spelled `{ANIM: [{NAME:[s], STATE:[s]}]}` | `0x4691d0` (helper) | children iterated as `Text("ANIM")`(5-B cmpsb) + `List` pairs; each spec record queried via `0x57a0f0`: `"NAME"` string → `0x523820` anim resolve; `"STATE"` `_stricmp` `RUNNING`→2 / `EXECUTED`→3 / `INVALID`→4 | appended 12-byte `{name_strdup, anim_handle, state}` records into a `{?@0, count@4, array@8}` header; result stored `+0x578` |
| `SET_HELP_LABEL` | `[s,s]`, `[[s,s],s]` | `0x4687eb` | child0 via `0x465fa0` → 20-byte string-vector record → `+0x508` (ptr); child1 (gated `[V+4]>2`) strdup'd if non-empty → `+0x50c`, else 0 | `+0x508`, `+0x50c` |
| `WAKE_ANIM` | `[s]` | `0x468986` | child0 tag3 → `+0x53c`; optional child1 tag3 (gated `[V+4]>2`) → `+0x540`, each strdup'd independently (M01 spells only the one-name form) | `+0x53c`, `+0x540` |

## Runtime consumers measured for the mapped fields

* **Tick gate** (`0x46a630`, per objective, when not completed): requires
  `+0xc` (active) ≠0, `+0x14` (completed) ==0, and dep `+0x10`'s objective
  `+0x5c8`==1 when a dep is set. Then each condition helper returning nonzero
  jumps to completion; afterwards the pending fields must all be clear to also
  complete: `+0x560`==0, `+0x56c`==0, `+0x578`==0, `+0x594`==0, `+0x58c`==0,
  `+0x598`<0, and the `+0x580>0 && +0x584<0` `DEDG` pair; finally `+0x5c8`==1.
* **Condition evaluators**: `0x469a60` walks `+0x560`/`+0x564` and counts
  handles whose target flag `entry+0x24` bit `4` is clear (the "inactive" bit),
  completing at `+0x55c`. `0x469ab0` counts nonzero bytes in `+0x574` against
  `+0x568`. `0x4697a0` walks `+0x578` records calling `0x4ed530` on each anim
  handle. `0x46b090` evaluates the `+0x57c` counter record. `0x465910` is the
  `DEDG` evaluator (`+0x580/+0x584/+0x588`, resolves the name via
  `0x451720(0x654170, …)`). `0x465b40` is the `TRAVELERS` evaluator: lazily
  resolves `+0x590`→`+0x5a0` (freeing the string on success) and completes per
  the `+0x594/+0x58c/+0x598` gate above.
* **Completion** (`0x46a94c`): sets `+0x14=1`, `+0xc=0`, `+0x5c8=3`; plays
  `+0x550` via `0x46cc50(0x71b438,…)`; by `+0x0` class plays mission sound
  `+0xc64/+0xc68/+0xc6c`; HUD label call `0x4a2350(0x71d2a0, class(+0x0),
  index(+0x4))` plus `0x4ad240(+0x4)`; `+0x554`==3→`0x463c10` (mission WON),
  ==4→`0x463c20` (mission LOST); then runs the on-complete lists (`+0x1c`,
  `+0x94`, `+0x58`, `+0xd0/+0xd4`, timer `+0x5c0/+0x5c4`, `+0xdc` on
  sleep-transition), `+0x544/+0x548` sleep-anims, `+0x510` stop-queue, target
  lists `+0x4bc..+0x4d8`, `+0xd8` hide.
* **Wake** (`0x469af0`): consumes `+0x124/+0x270/+0x29c` name lists,
  `+0x118..+0x120` generator, `+0x53c/+0x540` wake anims, `+0x54c` sound group,
  `+0x57c` counter (`ON_WAKEUP`), `+0x5e0` reset-timer; sets `+0x5c8=1`,
  `+0xc=1`, clears `+0x5cc`.
* **Mission-level context** (not among the 43; same parse idiom, mission
  object fields): `MISSION_TIMER` real → `0x46c510(0x71b468,…)` + optional
  `"NOLOSS"` child → `0x46c540(1)`; `PRIMARY/SECONDARY/TERTIARY_COMPLETE_SOUND`
  → `0x596120` → `+0xc64/+0xc68/+0xc6c`; `MISSION_WON/LOST_SOUND`,
  `OBJECTIVES_WON/LOST_SOUND` siblings at adjacent fields; `PLAYER_INIT`
  `[i,[r×3],[r×3],r,r]` → `+0x6c0..+0x6e0` with the angle triple ×π/180 and one
  scalar ×0.1.

## Parser keys present but NOT spelled by M01

For the B/C/D stages: the same parser sequence handles
`MISSION_WON_SOUND`, `MISSION_LOST_SOUND`, `OBJECTIVES_WON_SOUND`,
`OBJECTIVES_LOST_SOUND`, `PRIMARY_COMPLETE_SOUND`,
`SECONDARY_COMPLETE_SOUND`, `TERTIARY_COMPLETE_SOUND`, `MISSION_TIMER`,
`PLAYER_INIT`, `RESTORE_ANIMS`, `EXECUTE_ANIMS`, `INVALIDATE_ANIMS`,
`WIN_ANIM`, `LOSS_ANIM`, `RESET_TIMER` (`+0x5e0` + timer reset fields
`+0x5cc/+0x5d0/+0x5d4/−1/+0x5d8/−1`), `WARP_VEHICLE` (`+0x150..+0x154`),
`WAKEUP_TURRETS` (`+0x270`), `SET_AI_TEAM` (`+0x2c8/+0x2d0`, name+int pairs),
`SET_AI_ATTACK_RADIUS`, `COMPLETED_ZEPCANNONS` (`+0x3ec/+0x3f4`,
name+bool-byte records), `START_TAXI` (`+0x4dc/+0x4e0`),
`REMOVE_OTHER_TARGET`, `SLEEP_ANIM` (`+0x544/+0x548`), `SLEEP_OBJECTIVE_
WHEN_I_COMPLETE` (`+0x58`), `WAKE_OBJECTIVE`/`WAKE_OBJECTIVE_WHEN_I_COMPLETE`
(`+0x1c`), `WAKE_OBJECTIVE_WHEN_I_SLEEP` (`+0xdc`), `HIDE_OBJ` (`+0xd8`),
`TIMER_ADJUST` (`+0x5c0=2`, `+0x5c4`), `ADJUST_TIMER_WHEN_I_COMPLETE`
(`SET`→1/`ADJUST`→2 at `+0x5c0`, value `+0x5c4`), `END_TIMER` (`+0x18=1`),
`WON` (`+0x554=2`), `LOST` (`+0x554=1`), `TEST_COMPLETE`, `OBJECTIVE_HD_a`/
`OBJECTIVE_HD_b`, `DANGER_ZONES_COMPLETED` (`+0x56c/+0x570/+0x574`),
`DANGER_ZONES_COMPLETION_COUNT` (`+0x568`), `COUNTER` (helper `0x4693d0` →
`+0x57c`), `COMPLETION_COUNT` (`0x57a1b0`), `DELETE_ON_SUCCESS` (only as a
`TRAVELERS` argument token — the `0x57a090` string `DELETE_ON_SUCCESS` is
consumed by `repe cmpsb`, not looked up).

## Unknowns and limits (each with its evidence)

1. **`IDENTITY` child2**: M01 spells `[text,int,text]` at 4 sites; the parse
   only ever reads children 0 and 1. Whether any later code path re-reads the
   value list (the document survives at `this+0x6f4`) or the `MSG_*` string is
   a documentation-only field is **unknown** — no consumer of that slot was
   found.
2. **`+0x59c` polarity semantics**: set to 1 iff TRAVELERS child1 ==
   `"APPROACHING"` else 0; what the evaluator does with each polarity is
   inside `0x465b40`'s deeper logic — recorded but not fully reduced
   statically.
3. **`+0x5b4`, `+0x5b8`, `+0x5bc` semantics**: measured writes (arg4 raw →
   `+0x5b4` default 1; `+0x5b8` default 0; `DELETE_ON_SUCCESS` → `+0x5bc=1`)
   but their roles inside the travelers evaluation are not fully resolved.
4. **`DEDG` semantics**: the field pair and the name are stored; the evaluator
   `0x465910` and the `+0x580>0 && +0x584<0` gate are measured, but the
   counting model (what decrements what, and what `0x4658d0` counts) is left
   to stage B — **unknown** beyond the storage and gate shape.
5. **`+0x588` shared slot**: `TICK_DEPENDS_ON_OBJ` clears it, `DEDG` may
   strdup into it, and `0x465910`/`0x451720` resolve it as a name. One field
   serving both roles is measured; the original name is not recoverable
   beyond "owner/target name".
6. **Element tag fragility**: several parsers read `payload` as `char*`
   without checking the tag (`SET_AI_NET` grandchildren, all `strdup` name
   lists, `DEDG` raw ints, index lists). A non-string child in those slots
   would misbehave at runtime; static-only observation, not a verified crash.
7. **Error-path field values**: real-arg type mismatches log an error but the
   code then stores a stack scratch value into the field — the value stored
   after a logged error is **not** a meaningful default.
8. **`0x57a090` recursion reach**: the key scan descends into nested `List`
   children (the `0x57a0f0` flat variant does not). No M01 spelling depends on
   it, but a directive key spelled *inside* a value list would satisfy a
   top-level lookup — measured in `0x579ff0`, unexercised by M01.
9. **Consumers of `SET_AI_NET`/`COMPLETED_STOPPOINT`/`STOP_QUEUED_SOUNDS` /
   `WAKEUP_*` fields**: storage is fully mapped; the world-side consumers are
   other `CZMission` methods and were not traced in this stage (stages B/C/D).
10. **`this+0x6f4` document retention**: the whole decoded tree stays resident
    after parsing; whether anything re-reads it later is unknown.

## What this delivers to stages B/C/D

Every key M01 spells now has a measured handler site, arity/type parse and
record-field destination; every condition field has a named evaluator; the
objective lifecycle states (`+0x5c8`: 0 dormant / 1 awake / 2 nap / 3 done)
and completion write-paths are located. Semantic meaning of the fields —
what an "INACTIVE member flag", a "DEDG pair" or a "travelers subject"
actually counts — is the B/C/D work, and the consumers named here are the
functions those stages read next.
