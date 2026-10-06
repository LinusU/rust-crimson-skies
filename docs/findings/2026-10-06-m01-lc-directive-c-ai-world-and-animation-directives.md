# M01-LC-DIRECTIVE-C: what M01's AI, world and animation directives do

Date: 2026-10-06. Task: `M01-LC-DIRECTIVE-C` (#681), "measure AI, world and
animation directive semantics". Parent: `M01-LC-DIRECTIVE-MEANING` (#675).
Predecessor: `M01-LC-DIRECTIVE-A` (#679),
`docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md`,
which mapped *where* each key is parsed and *which record field* it writes.
This stage reads the handlers those fields reach and records **what the effect
is**. Capability used: `retail` (read-only `$CS_GAME_DIR`).

**Everything below is static code evidence.** The original program was never
run for this task. No statement here is `verified_original` runtime behaviour,
and no disposition is implemented by this task — stage `M01-LC-DIRECTIVE-E`
(#683) is where measured dispositions become code.

## Provenance and method

| Item | Value |
| --- | --- |
| File | `$CS_GAME_DIR/crimson.decrypted.exe` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` (unchanged from #679) |
| Address convention | virtual addresses; in this region `VA = file offset + 0x400000` |
| Tools | `r2` 6.2.0 (`pD`, `px`, `izz`), `objdump -d -M intel` over a read-only copy on an arm64 macOS host |
| Data cross-check | `crates/cs_app/tests/accept_m01_lc_directive_c.rs` re-reads every scoped site from the installation through the production `.zrd` decoder and the production census |

Stage A named the parse site, the argument read and the destination field for
each key. This stage took those fields forward into the update-time consumers
and, for every consumer, into the world object it touches. Every claim below
names the address that carries it.

### The registries these directives reach (measured, once)

| Global | What it is | Found by |
| --- | --- | --- |
| `0x71dab8` | vehicle registry | `0x4aff10(0x71dab8, name)`, handle form `0x4afee0` |
| `0x71df80` | zeppelin registry | `0x4bd3e0(0x71df80, name)` |
| `0x71d910` | turret registry, name-keyed | `0x4a97b0` (mission wildcard), `0x4a9840` (name), `0x4a9890` (id) |
| `0x71dabc` | the vehicle list the `TRAVELERS` counter walks (`[+4]` count, `[+8]` array) | `0x465d7a` |
| `0x71c120` | named-node list (`{name, next}`) | `0x465e30(name)`; a miss **creates** a node through `0x4d0280(name, 7)` and links it |
| `0x9fd14c` | animation table, entries of `0x110` bytes, name inline at `+0`, state byte at `+0xa0` | `0x523820(name)` |
| `0x654170` | generator registry (`[+4]`..`[+8]`, entries whose `+8` is the name) | `0x451720(0x654170, name)` |
| `0x64f610` / `0x64f614` | global node/path list (`[+4]` count, `[+8]` array) | net lookup `0x475f30`, `0x4bd7a0`; position restore `0x4b0f40`, `0x4bed40` |

`0x4cca30(node, on)` is the shared **activate/deactivate** call
(`D:\zipper\gamez\zclass\Class.c`: a null node logs `Null node pointer.` and
returns `5`; otherwise it dispatches on the node type at `+0x34`, valid `1..=10`,
through the jump table at `0x4ccaf8`). Every "wake", "deactivate" and
"delete on success" below is this one call.

### The original's own AI-node format comment (`.data` at `0x622508`, 805 bytes)

This is original-authored documentation **inside the executable**, quoted here
because it names the very fields three of these directives write:

```
;AI vehicles.
;Format:
; ( nodename (netids (x y z) yaw team group enabled primary_target
;   init_health active_rad active_u active_l attack_rad attack_u attack_l
;   return_rad return_u return_l title deactivated
;   dare_devil natural_touch sixth_sense dead_eye quick_draw
;   steady_hand stun_recovery talker constitution pref_engage_alt
;   signature_maneuvers (rating_biases) nitro engine
;   otherTarget objectiveTarget categoryLabel helpLabel taxiPath stickiness bait
;   pilot sclp sclr scly limp limr limy esclp esclr escly elimp elimr elimy
;   attack_time_factor anose hnose atail htail aleft hleft aright hright accentID
;   armor ace pattern decal1 decal2 decal3 r1 g1 b1 r2 g2 b2 r3 g3 b3))
; netids is either a net number or a list of zero or more net numbers enclosed
; in parenthesis.
```

The AI-vehicle loader corroborates the order: the node's `team` field is read at
`[node+0x34]` and handed to the base-class team setter at `0x47c67c`/`0x47c691`,
and `[node+0x38]` — the next field, `group` — is stored at `vehicle+0x388` at
`0x47c69a`. `vehicle+0x388` is therefore the AI **group**, which is what
`TRAVELERS`' counter compares against (below). A second embedded table, the
ZeppelinList node keywords at `0x62b660`, reads
`node deactivated team enemy ally neutral position yaw pitch max_speed …` and
`deactivated` is what calls `0x4bed40(zeppelin, 1)`.

## The scoped keys

Thirteen keys are in this stage's scope. Seven of them M01 spells
(`ANIM_STATE`, `SET_AI_NET`, `TRAVELERS`, `WAKEUP_ENEMIES`, `WAKEUP_GENERATOR`,
`WAKEUP_ZEP_TURRETS`, `WAKE_ANIM`); six it does not (`SET_AI_ATTACK_RADIUS`,
`SET_AI_TEAM`, `SLEEP_ANIM`, `START_TAXI`, `WAKEUP_TURRETS`, `WARP_VEHICLE`).
Stage A's map and the acceptance suite fix that split, so a future census that
starts or stops reporting one of them fails a test.

### `WAKEUP_ENEMIES` — wake actors that are asleep

Parse (A): `+0x124` count, names at `+0x128`. M01 spells it once:
`OBJECTIVE13 [text bsfury_1, text blackswanzep]` — one vehicle name and one
zeppelin name in the same list, which is why the handler tries both.

Consumed in the wake handler `0x469b5b`, per name:

```
0x469b80  mov eax, [edi]           ; name
0x469b83  mov ecx, 0x71dab8        ; vehicle registry
0x469b88  call 0x4aff10
0x469b8f  je  0x469ba3             ; no vehicle -> try a zeppelin
0x469b91  cmp byte [eax + 0x945], 0
0x469b97  je  0x469bc1             ; not asleep -> nothing to do
0x469b99  push 0
0x469b9c  call 0x4b0f40            ; vehicle wake (arg 0 = awake)
0x469ba3  ... 0x4bd3e0(0x71df80, name)
0x469bb4  cmp byte [eax + 5], 0    ; the zeppelin's dormant byte
0x469bb9  push 0
0x469bbc  call 0x4bed40
```

* `0x4b0f40(vehicle, 1)` is **put to sleep**: `+0x945 = 1`, `+0x91d = 1`,
  `+0x91e = 0`, `0x4cca30(vehicle->[+0xc], 0)`, then the object at `+0x948` is
  released through its virtual `+0x48(1)` and the pointer cleared.
  `0x4b0f40(vehicle, 0)` is the inverse: it clears `+0x945`, `+0x91d`,
  `+0x91e`, `+0x91f` and, when the vehicle is not AI-driven (`+0xcc == 0`) and
  carries a path index (`+0x2e4 >= 0`), copies the first position of path entry
  `[+0x2e4]` out of the global list into the vehicle before re-activating it.
* `0x4bed40(zeppelin, arg)` sets `zeppelin->[+5] = arg`, calls
  `0x4cca30(zeppelin->[+0x1c], !arg)` and, in both directions, restores the
  zeppelin's position from global list entry `[+0xd0]` into `+0x20` and calls
  `0x4bf9b0`. The same function with `arg = 1` is what a `deactivated` keyword
  in a ZeppelinList node invokes (`0x4bd94a`).

**Effect: wake only what is asleep.** The dormant markers are
`vehicle+0x945` and `zeppelin+5`, and the directive is a no-op for anything
already awake — a refinement of A, which recorded the two calls but not the
guards.

### `WAKEUP_TURRETS` — activate turret entries by name

Parse (A): `+0x270` count, names at `+0x274`. Consumed at `0x469bcf`:
`0x4a97b0(0x71d910, name)` per name, one call each, return value ignored.
`0x4a97b0` has exactly one caller in the whole image (the mission wake
handler).

The matcher walks the registry and compares the mission's name with each
entry's name (`entry+0xc`) under this rule, read instruction by instruction at
`0x4a97dc`:

* a literal character must equal the entry's character;
* a `*` in the mission's name consumes exactly **one** character of the entry's
  name, and that character must be a digit (`0x30`..`0x39`, `0x4a97e6`..`0x4a97ee`)
  — anything else ends the walk;
* the walk ends only when both strings' terminators meet (`0x4a980a` compares
  the two at the offset reached), and then `entry->[0x6e] = 1`.

`entry+0x6e` is the turret's **live** byte: the turret's own update returns
immediately while it is clear (`0x4aac16` → `0x4ab53b`), and it is part of the
turret's save/restore record (`0x4ac6a7` out, `0x4ac706` in).

**Effect: set the live flag of every turret whose name matches.** The wildcard
rule is measured; its intent is not (see Unknowns).

### `WAKEUP_ZEP_TURRETS` — activate a zeppelin-turret node and its children

Parse (A): `+0x29c` count, names at `+0x2a0`. M01 spells it three times:
`OBJECTIVE1 piratezep`, `OBJECTIVE5 workersvoyagezep`,
`OBJECTIVE13 blackswanzep`.

Consumed at `0x469c01`: each name goes through `0x465e30(name)`; a resolved
node gets `0x4bef70(node, 1)`, which

* writes the **global** byte `0x71df90 = 1` and calls `0x4bef20(node)`;
* `0x4bef20` finds the registry entry whose name equals `node->[+0xc]`
  (`0x4a9890`), writes `0x71df90` into `entry->[0x6e]`, and then **recurses**
  over the node's children (`[+0x56]` count, `[+0x5c]` array, `0x4bef46`).

**Effect: activate the turret entries of a zeppelin-turret node and everything
below it**, using the same `+0x6e` live byte as `WAKEUP_TURRETS`.

### `WAKEUP_GENERATOR` — add units to a generator's backlog

Parse (A): `+0x118` name, `+0x11c` cached handle, `+0x120` count. M01 spells it
four times: `workersvoyagezep +3` (OBJECTIVE9, OBJECTIVE27),
`blackswanzep +2` (OBJECTIVE13), `blackswanzep +4` (OBJECTIVE45).

Consumed at `0x469c3f`: the name is resolved lazily through
`0x451720(0x654170, name)` and cached in `+0x11c`; a miss logs
`Cannot find generator %s` at `mission.cpp:0x115d` (4445). When resolved:

```
0x469c93  mov edx, [esi + 0x120]     ; the count argument
0x469c99  add dword [eax + 0x80], edx
```

`generator+0x80` is a **remaining-units counter**, measured from the generator's
own update: `0x4526f0` compares it against a time delta before spawning, and on
each spawn `0x4527ad`..`0x4527cd` decrements `+0x80`, increments `+0x88`, zeroes
`+0x90` and increments the global `0x6541c4`. The spawn itself (`0x451bf0`)
builds an instance name from the node's model path by truncating at the last
`_` (`strrchr(name, '_')`, `0x451c28`).

**Effect: give the named generator N more units to produce.**

### `WARP_VEHICLE` — teleport a vehicle to one randomly chosen warp point

Parse, measured in full at `0x46809d`: `+0x150` is `strdup(child0)`, the
**vehicle name**; every child from index 1 onwards is one warp point and must be
a **sub-list**, copied into a **28-byte record** at `+0x158 + 28*i`:

| Record field | Source |
| --- | --- |
| `+0x00` (x) | child 0's payload, raw |
| `+0x04` (y) | child 1's payload, raw |
| `+0x08` (z) | child 2's payload, raw |
| `+0x0c` | `0` |
| `+0x10` (angle) | child 3's payload, raw |
| `+0x14` | `0` |
| `+0x18` (node name) | `strdup(child 4)` when the list holds ≥ 5 children, else `0` |

`+0x154` is the point count. No payload is tag-checked (A's fragility note,
confirmed here).

Consumed in the completion pass at `0x46aa4f`:

* if the count is 0 or the named vehicle does not exist, nothing happens;
* `i = rand() % [+0x154]` — the point is chosen at random;
* if record field `+0x18` is non-zero: `0x4940d0(vehicle, name)`, which resolves
  the node with `0x4511b0(0x6541b0, 0, name, 1)`, requires its position list to
  hold at least two entries (`0x49412f`) and warps to the first one, offset by
  the world's altitude at `[world+0x218]`;
* otherwise `0x493fb0(vehicle, &record.field0, &record.field3)`: it copies
  fields 0..2 into `vehicle+0x204` and multiplies fields 3..5 by the double
  constant `0x6040e8` (`0x3f91df46a2529983` = π/180) into
  `vehicle+0x1f8 / +0x1fc / +0x200` — so **the angle in a warp point is written
  in degrees**; it then rebuilds the rotation (`0x53f610`, `0x53fa40`) and mirrors
  the position into the vehicle's history list at `+0x6a4`;
* then, **only when `vehicle->[+0xcc] == 0`** (not AI-driven, `0x46aadd`): the
  code compares `world->[+0x1e8]` with `vehicle->[+0x668]`, keeps the smaller
  one, negates it, and writes the vector `(0, 0, −z) ⊙ [+0x198, +0x19c, +0x1a0]`
  into `vehicle+0x924`, with `+0x930` = |v|² and `+0x934` = |v|
  (`0x46aaf4`..`0x46ab73`).

**Effect: teleport the vehicle to one of its warp points, chosen at random, and
give it a downward velocity proportional to its altitude**; an AI-driven vehicle
is left to its AI instead.

### `SET_AI_TEAM` — write the actor's team

Parse (A): `+0x2c8` count, 8-byte `{name, int}` records at `+0x2cc`. M01 never
spells it. Consumed in the completion pass at `0x46ab79`, one call per record to
**`0x469e20`** — the function that follows the wake handler, which A did not
name:

* `0x4aff10(0x71dab8, name)`. On a vehicle: `0x453740(&slot, team)` then the
  base-class virtual at `vtable+8`, which is `0x441b80` (`this->[+8] = *slot`), so
  the **base team field** becomes the integer; if `vehicle->[+0x948]` is set it
  is released through its virtual `+0x48(1)` and cleared. Logs
  `SET_AI_TEAM: setting vehicle %s to team %d` (`mission.cpp:0x11cf`, 4559).
* else `0x4bd3e0(0x71df80, name)`. On a zeppelin: `zeppelin+0xdc = 1`,
  `zeppelin+0xe0 = team`, `0x4bf030(zeppelin)`. Logs
  `SET_AI_TEAM: setting zeppelin %s to team %d` (`0x11d3`, 4563).
* else logs `SET_AI_TEAM: can't find vehicle or zeppelin %s` (`0x11d6`, 4566).

**Effect: put the named vehicle or zeppelin on a team**, with a distinct field
per class. The AI-node format comment lists `team` as the sixth AI-vehicle field
and the loader reads it at `[node+0x34]` into that same base setter.

### `SET_AI_NET` — point the actor at a named entry of the global node list

Parse (A): `+0x398` count, `{vehicle, net}` string pairs at `+0x39c`, cap 10.
M01 spells it three times:
`devastator_2 → Bravo2` and `devastator_3 → Charlie2` (OBJECTIVE2),
`bsfury_1 → BlackSwan` (OBJECTIVE19), `blackswanzep → SwanZep2` (OBJECTIVE22).

Consumed at `0x46aba2`, one call per pair to **`0x46a000`**:

* `0x4aff10(0x71dab8, name)`. On a vehicle: `0x475f30(vehicle, net_name)`, which
  searches the global list at `0x64f610` for an entry whose name matches
  (case-sensitive) and calls `0x475fc0(vehicle, entry_id)`, which writes
  **`vehicle+0x2e4 = entry_id`** and re-seeds the vehicle from that entry. Logs
  `SET_AI_NET: setting vehicle %s to net %s` (`0x11f3`, 4595).
* else `0x4bd3e0(0x71df80, name)`; on a zeppelin `0x4bd7a0(zeppelin, net_name)`
  performs the same list walk. Logs
  `SET_AI_NET: setting zeppelin %s to net %s` (`0x11f7`, 4599).
* else `SET_AI_NET: can't find vehicle or zeppelin %s` (`0x11fa`, 4602).

**Effect: move the actor onto a named entry of the global node/path list** — the
same `+0x2e4` field the vehicle wake path uses to restore a position, and the
same list the AI-node comment names `netids`.

### `SET_AI_ATTACK_RADIUS` — set a vehicle's attack radius

Parse (A) into `+0x31c` count with **12-byte** records at `+0x320`
(`0x46ac2f`..`0x46ac44` walks them with stride `0xc`). M01 never spells it.
Consumed at `0x46ac1d`, one call per record to **`0x469f70`**:

* the record is `{name, cached handle, radius}`; `0x4aff10(0x71dab8, name)` and
  the handle is written back into the record's second field;
* on success `vehicle+0x328 = r²`, `vehicle+0x32c = −r`, `vehicle+0x330 = r`
  (`0x469f89`..`0x469fa1`) and it logs
  `SET_AI_ATTACK_RADIUS: setting %s to radius %f` (`0x11e3`, 4579);
* **vehicles only** — there is no zeppelin branch. A name that is not a vehicle
  logs `SET_AI_ATTACK_RADIUS: can't find vehicle %s` (`0x11e6`, 4582).

**Effect: set one attack radius, cached three ways** (`r²`, `−r`, `r`) in the
vehicle's radius block at `+0x328`..`+0x330`, next to the triple at
`+0x318`..`+0x320` that a neighbouring per-frame check clamps against its own
defaults (`0x46586c`..`0x4658ac`). The AI-node comment lists
`active_rad active_u active_l` and `attack_rad attack_u attack_l` as
consecutive triples; which of the three written fields is which of them is not
resolved (Unknowns).

### `START_TAXI` — release a parked vehicle

Parse (A): `+0x4dc` count, name pointers at `+0x4e0`. M01 never spells it.
Consumed at `0x46acfe`, one call per name to **`0x46a2b0`**:

```
0x46a2b0  mov eax, [esp + 4]      ; the name
0x46a2b5  mov ecx, 0x71dab8
0x46a2ba  call 0x4aff10
0x46a2bf  test eax, eax
0x46a2c1  je  0x46a2ca             ; unknown name: silently nothing
0x46a2c3  mov byte [eax + 0xd4], 0
```

`vehicle+0xd4` is a hold-off flag, measured at three sites: the vehicle
constructor clears it (`0x4b0064`); the spawn path sets it together with
`+0xcc = 1` and a position (`0x47c568`..`0x47c57e`); and the AI think returns
immediately while it is set — `0x489ea0` calls `0x48a110(vehicle)` only when
`+0xcc` is set, and `0x48a110` reads `+0xd4` first and jumps out at `0x48a129`
when it is non-zero.

**Effect: clear the flag that holds a parked vehicle's AI off.** The *original
name* of that flag is not recoverable (Unknowns); the AI-node comment's
`taxiPath` field is consistent with the reading but is not evidence for it.

### `TRAVELERS` — count actors near a position, or watch one actor

Parse, measured in full at `0x467a70`; **at least five arguments** are required
(`[V+4] < 5` skips the key):

| Argument | Destination | Notes |
| --- | --- | --- |
| child 0, a string | `0x465e30` resolves → `+0x594`; otherwise `strdup` → `+0x58c` | a name retried every tick until it resolves |
| child 0, **not** a string | raw payload → `+0x598` | the AI **group** compared in count mode; a string spelling leaves it 0 |
| child 1, a string | 12-byte compare with `APPROACHING` → `+0x59c = 1`, else `0` | a non-string child 1 writes nothing |
| child 2, a string | resolve → `+0x5a0`, else `strdup` → `+0x590` | the target node, lazily resolved |
| child 2, a list | children 0,1,2 read as reals → `+0x5a4/+0x5a8/+0x5ac` | a literal target position |
| child 3 | real → `+0x5b0`, then **squared in place** (`0x467d11`) | the radius |
| child 4 (needs 6 args) | raw → `+0x5b4`, default **1** | how many |
| child 5 (needs 7 args, tag 3) | 18-byte compare with `DELETE_ON_SUCCESS` → `+0x5bc` (default 0) | delete the actors on success |

`+0x5b8` (the tally) starts at 0. M01 spells it once:
`OBJECTIVE3 [text player, text APPROACHING, text workersvoyagezep, float 700, int 1]`.

The evaluator is `0x465b40`, called from the tick gate at `0x46a8c9`, and it has
**two modes**.

**Subject mode** — taken when `+0x594` resolved *and* `subject->[+0x24] & 4`
(`0x465c0e`):

* the target position is `0x4cf200(+0x5a0)` or the literal
  `+0x5a4/+0x5a8/+0x5ac`;
* `d² = |subject − target|²` is compared with `+0x5b0`;
* the function returns **1** when `d² < r²` (`0x465c9a` falls through to
  `0x465d37`) and also when `d² > r²` (`0x465cf5`), and returns **0** only when
  `d² == r²` exactly (`0x465cec`'s `test ah,0x41` jumps to `0x465e19`, which is
  `xor eax, eax; ret`);
* with `DELETE_ON_SUCCESS` set it first removes the subject: a vehicle
  (`0x4afee0`) through `0x47bab0(vehicle)`, any other node through
  `0x4cca30(node, 0)`;
* **`+0x59c` has no effect in this mode**: it is loaded at `0x465c85` into `eax`
  and clobbered by `fnstsw ax` at `0x465c8d`, and the `cmp eax,edi` flags
  computed one instruction earlier are discarded by the same store.

**Count mode** — every other case, and only when `+0x598 >= 0`
(`0x465d43`):

* the record and the target position are published in globals `0x71b41c`,
  `0x71b428`, `0x71b42c`, `0x71b430`, and the tally is reset at `0x71b420`;
* every vehicle in the list at `0x71dabc` is tested by `0x4659b0(vehicle)`:
  skipped when `vehicle->[+0x91d]` is set (asleep — the byte the sleep/wake
  calls write), skipped unless `vehicle->[+0x388] == +0x598` (the AI **group**),
  then `|vehicle->[+0x204] − target|²` against `+0x5b0` decides: counted when
  the distance is **inside** the radius if `+0x59c != 0`, and when it is
  **outside** the radius otherwise (`0x465a4d` vs `0x465a5f`);
* the tally is added to `+0x5b8` (`0x465da4`), and the objective completes when
  `+0x5b8 >= +0x5b4` (`0x465df9`), clearing `+0x5b8`;
* with `DELETE_ON_SUCCESS` the matching vehicles are deleted as they are found
  (`0x465a70` then `0x47bab0`).

The two `fcomp` flag readings used here — `fnstsw ax` bit 8 (CF) is `C0`
("less than") and bit 14 (ZF) is `C3` ("exactly equal"), so `test ah,0x41` is
"less than or exactly equal" — are pinned by three other sites in the same
file that use the same idiom for a "> 0" duration test (`+0x5e0` at
`0x469d14`, `+0x5d4` at `0x46a677`, `+0x5d8` at `0x46a702`), all of which skip
when the value is `<= 0` and act when it is `> 0`.

**Effect:** the count mode waits until *N* actors of a given AI group are
inside (or, without `APPROACHING`, outside) a radius around a position, and can
delete them on success; the subject mode watches one named actor against the same
radius.

### `WAKE_ANIM` — start a named animation on wake

Parse (A): `+0x53c` anim name, `+0x540` optional node name. M01 spells it five
times: `activate_dropoff_node` (OBJECTIVE11, OBJECTIVE49), `fadein_bszep`
(OBJECTIVE13), `activate_pickup_node` (OBJECTIVE15), `pzhomebase` (OBJECTIVE18).

Consumed in the wake handler at `0x469c9f`: `0x523820(+0x53c)` resolves the
animation; if `+0x540` is set it is resolved through `0x465e30` and **the whole
start is skipped** when that fails; then
`0x4edda0(anim, node, 0, 0, 0)` with `node = 0` when no node name was given.
`0x523820` walks the animation table at `0x9fd14c` comparing inline names and
**skips any entry whose state byte `+0xa0` is 5** (`0x52384e`). `0x4edda0` is a
five-argument wrapper that re-pushes its arguments and calls `0x4edc50`.

**Effect: start a named animation, optionally on a named node, at the moment the
objective wakes** — and refuse to start anything whose animation is in state 5.

### `SLEEP_ANIM` — the same animation call, at a state transition

Parse at `0x4689e9` (both `+0x544` and `+0x548` are cleared to 0 first). M01
never spells it.

**Correction to stage A**, which listed `+0x544/+0x548` among the completion
pass's on-complete lists: the completion pass (`0x46a94c`..`0x46ae86`, read in
full) contains **no reference** to either field. The only runtime consumer of
either in the mission region is `0x46b183`, inside the objective
**state-transition** function `0x46b160`, immediately after it sets `+0xc = 0`,
`+0x5c8 = mode` and `+0x5cc = 0`:

```
0x46b181  mov eax, [esi + 0x544]     ; the anim name
0x46b1a5  call 0x523820              ; resolve (skips state-5 entries)
0x46b1b3  mov esi, [esi + 0x548]     ; the node name
0x46b1be  call 0x465e30              ; resolve; a miss skips the start
0x46b1d4  call 0x4edda0              ; anim, node, 0, 0, 0
```

A search of the whole image for the `0x544` and `0x548` displacements returns, in
the mission region, only those two, the record destructor that `free`s both
strings and zeroes them (`0x466943`..`0x46696f`, alongside `+0x53c`/`+0x540`), and
unrelated code in other classes. Nothing else in the image reads either field.

**Effect: play the named animation when the objective transitions** — and since
`0x46b160` is what the nap (mode 2), kill (mode 3) and completion paths all go
through, that is every transition, not only completion. The call is the same
`0x4edda0(anim, node, 0, 0, 0)` as `WAKE_ANIM`, which is why the two keys are one
operation at two lifecycle points.

### `ANIM_STATE` — require N listed animations to be in a named state

Parse helper `0x4691d0`, stored at `+0x578` as `{required@0, count@4, array@8}`
with 12-byte records `{strdup(name), anim handle, state}`:

* it walks the value list for a tag-3 `ANIM` (5-byte compare) followed by a tag-4
  spec record;
* inside a spec, `0x57a0f0(spec, "NAME", 1)` gives the animation name, which
  `0x523820` turns into a handle, and `0x57a0f0(spec, "STATE", 1)` plus
  `_stricmp` maps `RUNNING` → 2, `EXECUTED` → 3, `INVALID` → 4 (anything else
  stays 0);
* a pair is appended only when **both** the handle and the state are non-zero,
  and `required` is incremented once per appended pair (`0x469325`);
* after the walk it reads a sibling **`COMPLETION_COUNT`** through `0x57a1b0`
  into `required` itself — the out-parameter pushed at `0x4693ae` is the header's
  first field — so a record's `COMPLETION_COUNT` lowers how many pairs must
  match.

The evaluator `0x4697a0` (tick gate, `0x46a894`) counts, over the records, those
whose `0x4ed530(anim)` equals the wanted state and returns
`matches >= required` (`cmp ebp, [edx]; setge al`, `0x4697f6`).

`0x4ed530(anim)` returns the byte `anim->[0xa0]`, and the debug-name table the
engine builds at `0x4ed550` (24-byte entries at `0x727f20`) names that enum:

| value | name |
| --- | --- |
| 0 | `UNDEFINED` |
| 1 | `DORMANT` |
| 2 | `RUNNING` |
| 3 | `EXECUTED` |
| 4 | `INVALID` |
| 5 | `CORRUPT` |
| 6 | `INVALID_AND_RUNNING` |

This independently confirms the parser's 2/3/4 mapping, and explains why
`0x523820` skips entries in state 5. M01 spells it three times, always with one
pair: `wv_drop_copilot`/`RUNNING` (OBJECTIVE11),
`wv_pickup_copilot`/`EXECUTED` (OBJECTIVE15), `hooked_to_klondike`/`EXECUTED`
(OBJECTIVE18).

**Effect: complete the objective once at least `required` of the listed
animations are in the requested state**, where `required` defaults to the number
of listed pairs and a sibling `COMPLETION_COUNT` can lower it.

## Corrections and additions to stage A

1. **`SLEEP_ANIM` is not a completion-time effect.** A listed `+0x544/+0x548`
   under completion; the only reader is the transition function `0x46b160`
   (measured above, and by an image-wide displacement search).
2. **The wake-time effects are guarded.** `WAKEUP_ENEMIES` only wakes a vehicle
   whose `+0x945` is set or a zeppelin whose `+5` is set; A recorded the calls
   without the guards, which are the difference between "wake" and "wake what is
   asleep".
3. **Three handlers A did not name**, all reachable from the completion pass:
   `SET_AI_TEAM` → `0x469e20`, `SET_AI_ATTACK_RADIUS` → `0x469f70`,
   `START_TAXI` → `0x46a2b0`, plus `SET_AI_NET` → `0x46a000` (A named the field
   and the log strings but not the function).
4. **`WAKEUP_TURRETS` and `WAKEUP_ZEP_TURRETS` share one flag** (`entry+0x6e`)
   and the zeppelin variant recurses over child nodes; A recorded only the two
   call targets.
5. **`ANIM_STATE`'s required count** is the number of appended pairs, and a
   sibling `COMPLETION_COUNT` overwrites it; A recorded the record layout and
   the state mapping but not the count field's origin.
6. **`WAKEUP_GENERATOR`'s `+0x80`** is a remaining-units counter with a measured
   spawn-side decrement; A recorded the `+=` only.
7. **The original ships its own AI-node format comment** (`0x622508`) and
   ZeppelinList keyword table (`0x62b660`) inside the executable, which name
   `netids`, `team`, `group`, `attack_rad`, `deactivated` and `taxiPath` — the
   original's own words for several fields these directives write.

## Unknowns (each with its evidence)

1. **`vehicle+0xd4`'s original name.** Measured: cleared by `START_TAXI`, set by
   the spawn path, and it gates the AI think at `0x48a129`. Not recoverable as
   a name from code; `taxiPath` in the AI-node comment is consistent, not proof.
2. **`SET_AI_ATTACK_RADIUS`'s three written fields.** Measured: `+0x328 = r²`,
   `+0x32c = −r`, `+0x330 = r`. Which of `attack_rad`, `attack_u`, `attack_l`
   each is — and whether `+0x318`..`+0x320` is the `active_rad` triple — is not
   resolved; the comment gives the order, not the semantics.
3. **The three per-axis factors and the world field in `WARP_VEHICLE`'s
   velocity.** Measured: `(0, 0, −min(vehicle+0x668, world+0x1e8)) ⊙ [+0x198,
   +0x19c, +0x1a0]`. What those three floats are (rate limits, per-axis gains,
   orientation-dependent terms) is unknown.
4. **The `*` rule in `WAKEUP_TURRETS`.** Measured: one `*` consumes exactly one
   character and that character must be a digit. No mission in M01 spells the
   key, and no campaign mission was surveyed for it in this stage, so nothing
   constrains the intent.
5. **Which `TRAVELERS` mode M01's spelling takes.** Measured: subject mode
   requires `subject->[+0x24] & 4`; whether the node M01's `"player"` resolves
   to has that bit set is a runtime property of the world build, not of the
   executable.
6. **`TRAVELERS`' subject-mode comparison.** Measured as written: it returns 1
   for `d² < r²` **and** for `d² > r²`, and 0 only for exact equality. Whether
   the original intended `<=` is not recoverable from the code; this document
   records the behaviour, not a rationalisation of it.
7. **`+0x598`'s meaning when it is 0.** With a string child 0 the count mode
   compares AI group 0; nothing measured here says which group 0 is or whether
   the mode is reachable in practice for M01's spelling.
8. **`0x4edc50`'s three trailing arguments.** The mission always passes
   `0, 0, 0`; only the mission call path was measured, so the helper's defaults
   for other callers are unknown.
9. **`+0x948`'s role in `SET_AI_TEAM`.** Whatever object a vehicle holds there
   is released with a virtual `+0x48(1)` call; the object was not identified.
10. **The anim state enum above 6.** The name table has seven slots; a state byte
    outside them indexes past the table. Not exercised by M01, not explored.
11. **`0x469a60`'s and this stage's shared `+0x24` flag bit.** Stage A read bit 2
    as the "the object is gone" bit for `INACTIVE<n>`; this stage relies on that
    reading for the `TRAVELERS` gate but did not re-derive it.

## What this delivers to stage `M01-LC-DIRECTIVE-E` (#683)

Thirteen keys now have a measured effect rather than a measured field: which
world registry each one reaches, which object field or virtual it calls, what
value it writes and under which guard. Seven of them are the keys M01 actually
spells, and the shipped arguments for every one of their sites are pinned by
`crates/cs_app/tests/accept_m01_lc_directive_c.rs`, so an implementation can be
written against measured behaviour rather than against key names.

What stage E must **not** do on this evidence: treat the wake guards, the
`WAKEUP_TURRETS` digit wildcard, the `TRAVELERS` subject-mode comparison or the
`ANIM_STATE` count override as details to tidy. Each is a measured part of the
original's behaviour, and three of them are the difference between a directive
that works and one that silently does nothing.
