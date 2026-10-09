# #792: the zeppelin attitude composes as `Ry(yaw)·Rx(pitch)`, and the startup placement is what applies last

Date: 2026-10-09. Task `M01-LC-ZEPPELIN-ATTITUDE` (#792), filed by #359
(`VS-M01-RUNTIME`). Feature sheet: `specs/F34-*.md` (the world-actor schema
and its runtime stages) with `docs/contracts/SCRIPT-MISSION.md`. Capabilities
used: **`retail`** (read-only `$CS_GAME_DIR`) and **static analysis** of the
owner-supplied decrypted image (sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, image base
`0x400000`, file offset = VA − `0x400000` for `.text`/`.rdata`), disassembled
with the platform `objdump -d -M intel` into a scratch listing and re-read at
every address below. **No original run happened: nothing here is
`verified_original`.** Test prefix: `accept_m01_lc_zeppelin_attitude_`.

## The answer in two paragraphs

**Compose.** A `zeppelins.zrd` record's attitude reaches its world node as an
`Object3d` rotation triple: the record loader writes `yaw`/`pitch`
(degrees→radians, the image double `0.01745329251994` at `0x6040e8`) into
`obj+0x2c`/`obj+0x30` (`0x4bda9b`..`0x4bdac1`), clamps `pitch` to the record's
own `min_pitch`/`max_pitch` (`0x4bdbc6`), and the *same* routine ends by
handing the pair to `Object3d::SetRotation` for the node its `node` key
resolved: `0x4becf8` → `0x4bf930` → `0x4bf950` → `0x4d1a30`, which stores
pitch at `class+0x18`, yaw at `class+0x1c` and `0` at `class+0x20`. #770 §12.2
measured the matrix built over exactly those slots (`0x53bf40`, inverted by
`0x53df30`) as `M = Ry(r1)·Rx(r0)·Rz(r2)` with the right-handed matrices, so
the node's orientation is **`Ry(yaw)·Rx(pitch)`** in #436's identity-mapped
metre frame — `yaw = 0 ∧ pitch = 0` ⇒ identity, positive yaw turning local
`+Z` toward `+X` (the compass counter-rotation at `0x49f8ec` fixes the sign).

**Precedence.** `placezeps.zrd` writes the *same* slots of the *same* node
through the *same* setter — its rotate executor `0x4e8b80` reaches
`0x4d1a30(node, [event+0x10], [event+0x14], [event+0x18])` for M01's member
(`0x4e8cf5` → `0x4e8c62`, with `AT_NODE`, the second flag and `RELATIVE` all
clear) — and that write happens once, from the animation instance's first
tick, while the spawn write happens inside the mission-start call stack, before
any frame can tick. **The startup placement is the later write**, so each
record's declared attitude is taken from it where the member states a rotation
for that node, and from the record's own spawn attitude where it does not. The
losing source stays named on the row
(`SpawnedZeppelinActor::attitude_residue`), never dropped.

## 1. The compose, address by address

| address | instruction | meaning |
| --- | --- | --- |
| `0x4bd8f2` | `mov ebx, ecx` | the record loader (`0x4bd8d0`, `ret 4` at `0x4bed35`), `this` = the spawned object |
| `0x4bd8f7` | `push 0x62b6a8` | key `node` |
| `0x4bd916` | `push 7; call 0x4d0280` | the name resolved through the node registry table `0x62cfb8[7]`, stored at `obj+0x1c` (failure aborts the record) |
| `0x4bda64` | `push 0x62b6dc` | key `position` → `obj+0x20`/`+0x24`/`+0x28` (`0x4bda75`..`0x4bda87`) |
| `0x4bda87` | `push 0x62b6e8` | key `yaw` |
| `0x4bda98` | `fmul qword [0x6040e8]` | degrees→radians, then `fstp dword [ebx+0x2c]` |
| `0x4bdab5` | `push 0x62b6ec` | key `pitch` |
| `0x4bdaba` | `fmul qword [0x6040e8]` | degrees→radians, then `fstp dword [ebx+0x30]` |
| `0x4bdbc6`..`0x4bdbfb` | `fld [ebx+0x30]; fld [ebx+0xbc]` … | `pitch := clamp(pitch, obj+0xb8 = min_pitch, obj+0xbc = max_pitch)` |
| `0x4becf8` | `mov ecx, ebp; call 0x4bf930` | end of the same routine: apply the pose |
| `0x4bf950`..`0x4bf99f` | clamp again, then `push 0; push [ecx+0x2c]; push clamped; push [ecx+0x1c]; call 0x4d1a30` | `SetRotation(node, pitch, yaw, 0)` |
| `0x4bf9b0`..`0x4bf9c0` | `call 0x4d1d50` | `SetPosition(node, obj+0x20, +0x24, +0x28)` |
| `0x4d1a30` (`Object3d.c`, assertion string `0x62d118`) | `ecx = [node+0x38]`; `[esp+0xc] → ecx+0x18`, `[esp+0x10] → ecx+0x1c`, `[esp+0x14] → ecx+0x20` | the triple lands at `class+0x18`/`+0x1c`/`+0x20` |
| `0x53bf40` / `0x53df30` / `0x49f8ec` | — | #770 §12.2: `M = Ry(r1)·Rx(r0)·Rz(r2)`, its inverse, and the compass landmark that fixes the sign |

So for the spawn: `r0 = pitch`, `r1 = yaw`, `r2 = 0` ⇒ `M = Ry(yaw)·Rx(pitch)`,
right-handed, metres, identity axis map (#436 owner note, #677 census). The
conversion is the image's own `0.01745329251994` f64→f32 round trip, the one
`cs_app::mission_start::stored_heading_radians` reproduces (`0x47c4ee`), so no
`f32::to_radians` approximation enters the binding.

## 2. The startup half: `placezeps.zrd` reaches the same slots

* **It is loaded.** The mission's `anim.zrd` carries an
  `ANIMATION_DEFINITION_FILE` whose value is `placezeps.zrd` (retail bytes,
  `ZBD/C1C/M01/zrdr.zbd`, member offsets 12 384..12 446), and the loader
  `0x520420` — called with `anim.zrd` from `CZMission::Load`'s data pass
  (`0x4647b2`) — walks that list and loads each listed member recursively
  (loop `0x52073c`..`0x520864`) into the definition table at `0x9fd14c`.
* **Its activation is `ON_STARTUP`.** `0x51e390` stores `4` for the keyword
  `ON_STARTUP` (`0x633dc4`) into the definition's activation byte `+0xa1`.
* **It is started, not run, at start.** `0x522fd0` walks the table and, for
  every definition with `+0xa1 == 4`, calls `0x4edc50` (`0x52309b`) with
  five zero arguments. `0x522fd0` is called from `0x465513` (inside the start
  routine `0x4654e0`) and from `0x46b99d`. `0x4edc50` allocates the instance
  (`0x4ed8c0`), zeroes its time fields and registers it (`0x5256e0`); it
  dispatches no event.
* **The events run from a per-frame callback.** The instance start registers
  `push 0x4ecd20; call 0x4ccde0` at `0x4eda92`..`0x4eda98`; `0x4ecd20` walks
  the instance and calls `0x4ecbb0`, which dispatches the current event
  through `call dword ptr [4*eax + 0x727de0]` at `0x4ecc7f` and then advances
  the event pointer past it (`0x4ecc90`..`0x4ecca3`) — **each state statement
  executes once**, when its time is reached (M01's statements carry no
  `START_TIME`, so the instance's first tick).
* **The rotate executor's M01 path.** `0x4e8b80` resolves the statement's
  `NAME` (event `+0x1c`) to a node (`0x4e8d60`), requires node type `5`
  (`0x4e8bbe`), takes `AT_NODE` clear (`0x4e8bd6` → `0x4e8c76`), the second
  flag clear (`0x4e8c76` → `0x4e8cf5`) and `RELATIVE` clear
  (`0x4e8d04` → `je 0x4e8c62`) and then calls
  `0x4d1a30(node, dword[esi+0x10], dword[esi+0x14], dword[esi+0x18])` —
  the three radians #791 measured the rotate parser to store at event
  `+0x10`/`+0x14`/`+0x18`, in stored order, into `class+0x18`/`+0x1c`/`+0x20`.

This also closes #791's open question *"which rotation component is the
heading"* for this path: the executor feeds the statement's **middle** number
to `0x4d1a30`'s second value, which lands at `class+0x1c` — #770's yaw slot.

## 3. The write order

Both writers target one node's one storage. The order is set by the mission
start, which is synchronous code:

| step | address | what happens |
| --- | --- | --- |
| data pass | `0x465370` (`CZMission::Load`) → `0x464680` | `anim.zrd` (+`placezeps.zrd`) definitions loaded at `0x4647b2`; `zeppelins.zrd` loaded and its pose applied at `0x46489c`; every zeppelin's drive byte `+4` cleared at `0x4648e0` (`0x41f250` → `0x4bd390`) |
| start | `0x416f40` → `0x4654e0` (callers `0x4170ce` and `0x43ed8f`) | time cleared, `ON_STARTUP` definitions started at `0x465513`, zeppelin manager cleared at `0x465562`, `zeppelins.zrd` loaded and its pose applied **again** at `0x4655c0`, drive byte cleared at `0x4655f0` |
| first frame | callback `0x4ecd20` → `0x4ecbb0` → `0x4ecc7f` | the startup state statements execute: the node's position and rotation become `placezeps.zrd`'s `STATE` |

No frame, and therefore no animation tick, can interleave inside either call
stack, so **every spawn write precedes every startup-state write**.

The spawn's own re-application path does not run after that on a
single-player launch. `obj+0x2c`/`obj+0x30` are written to the node again only
by the object's behavior block (`0x4bf9d0` → `0x4bf710` → `0x4bf930`) or by
the motion integrator (`0x470550` → `0x4706e0`), and both are gated:

* the behavior block needs the object's drive byte `+4` non-zero
  (`0x4bfa0f`), and `+4` has exactly three writers: the constructor
  (`0x4bd483`, clear), the clear at `0x4bd390` (the load path above) and the
  set at `0x4bd370`, whose only callers are the console handlers `… revive`
  (`0x43e4e1`, `0x43ecd0`, `0x47e510`) and the zeppelin-tick tails
  `0x496610`/`0x49669b`, both guarded by `0x5b4210`;
* `0x5b4210` belongs to `D:\zipper\gamez\znetwork\znet_dplay.cpp` /
  `znet_session.cpp` (the assertion strings at `0x622ec4`..`0x63db38` sit in
  that module) — it answers whether the registered DirectPlay session object
  is still current, i.e. it is a network-game predicate;
* the integrator additionally needs `0x440ad0()` non-zero (`0x4bf9f3`), whose
  byte prior tasks (#770 session 3, #740) recorded as an unmodelled
  game-state byte.

So the last rotation the node takes at startup is the startup state's where
the member states one, and the spawn attitude where it does not.

## 4. What M01's three records end with

Retail `ZBD/C1C/M01/zrdr.zbd`, both carriers decoded by the production
readers:

| node | `zeppelins.zrd` yaw / pitch (min/max) | `placezeps.zrd` rotate `STATE` | applied orientation `[x, y, z, w]` | residue |
| --- | --- | --- | --- | --- |
| `piratezep` | 180° / 0° (−30/30) | (0, 180, 0)° → (0, π, 0) rad | `Ry(180°)` = `[0, 0.999999999999999, 0, −4.371139000186241e-8]` | the spawn attitude (yaw 180°, pitch 0°) |
| `workersvoyagezep` | 220° / 0° (−30/30) | (0, 180, 0)° → (0, π, 0) rad | `Ry(180°)` = `[0, 0.999999999999999, 0, −4.371139000186241e-8]` | the spawn attitude (yaw **220°**, pitch 0°) — the case the refusal text named |
| `blackswanzep` | 340° / 0° (−30/30) | none (#791: definition 2 states a translate only) | `Ry(340°)` = `[0, 0.17364829201905368, 0, −0.9848077328488366]` | `placezeps.zrd` states no rotation for it |

`crates/cs_app/tests/accept_m01_lc_zeppelin_attitude.rs` re-derives all three
from the two members, recomputes the compose as a **matrix**
(`Ry·Rx·Rz`, converted to a quaternion by the trace/branch form) rather than
calling the implementation, and asserts the values above.

## 5. What changed

* `crates/cs_app/src/mission_world_actors.rs`:
  * `SPAWN_ATTITUDE_CLAIM` (`f34-world.zeppelin-attitude-compose`) replaces
    `f34-world.zeppelin-attitude-unmeasured`; `ATTITUDE_PRECEDENCE_UNKNOWN_CLAIM`
    and `ATTITUDE_PRECEDENCE_UNKNOWN_REASON_PREFIX` cover the scope whose
    `placezeps.zrd` refuses to decode — an unreadable startup carrier settles
    nothing and binds no orientation;
  * `AttitudeSource` (`StartupPlacement { rotation_radians }` /
    `CarrierSpawn { yaw_radians, pitch_radians }`), `AttitudeBinding`
    (`Measured` / `Open`) and `SpawnedZeppelinActor::{attitude,
    attitude_source, attitude_residue}`;
  * `object3d_orientation(r0, r1, r2)` — #770's `M = Ry(r1)·Rx(r0)·Rz(r2)` as
    the unit quaternion the world-actor runtime consumes;
  * `read_startup_member` / `StartupRead` — the scope's `placezeps.zrd`,
    its rotations keyed by the `NAME` they address, and the member's own
    `SourceSpan` for provenance; absence is measured, refusal keeps the
    attitude open;
  * `spawn_attitude` — the record's yaw and the pitch clamped exactly as the
    load clamps it; `declare_row` picks the source, `declare_actor` binds
    `orientation` `Resolved::Known` under the compose claim with the
    provenance span of the member the value came from.
* `crates/cs_app/src/mission_launch.rs`: the `world_actors` detail now names
  the compose claim and both findings. **The gate is untouched**: `orientation`
  is measured, `faction` is still refused, the surface still reports
  `unsupported` and `plan.launchable()` is still false.
* `crates/cs_app/tests/campaign/vs_m01_runtime.rs`: the closure's expected
  claim list names `f34-world.zeppelin-attitude-compose` instead of the
  retired unknown.
* `crates/cs_app/tests/accept_m01_lc_zeppelin_attitude.rs` (new): the retail
  acceptance test above, plus the residue and the provenance checks.

## 6. Residues, affected content and resolving work

| residue | affected content | resolves in |
| --- | --- | --- |
| ~~the **position** half of the same ordering~~ — **resolved**: the startup translate writes `class+0x54`..`+0x5c` (`0x4e8de0` → `0x4d1d50`, re-verified) after the spawn write, so `position_m` now binds the source the original applies last — `placezeps.zrd`'s triple for all three M01 nodes — under `SPAWN_POSE_CLAIM`, the record's own `position` (`obj+0x20`..`obj+0x28`) named as the residue | M01's three zeppelin positions bound (`piratezep` −8 704, `workersvoyagezep` −7 680, `blackswanzep` −5 632) | **#814** (`M01-LC-ZEPPELIN-PLACEMENT-POSITION`): `docs/findings/2026-10-09-m01-lc-zeppelin-placement-position.md`; #359's launch-surface re-derivation consumes it |
| the **animation-start reset**: `0x4edc50` zeroes the target node's position and rotation (`0x4edc8e`, `0x4edca5`) when instance flag `0x100` is set, computed at `0x4ed600` from the target node's type and from `0x4db0c0([0x9fd160], node)` — a third writer at the same moment as the startup start | the one record the startup carrier does not rotate (`blackswanzep`) would keep the spawn attitude only if the reset does not fire; on the measured order it cannot matter at startup, because `0x4654e0` starts the animations at `0x465513` **before** the records are (re)loaded at `0x4655c0`, so the spawn write follows any reset | decoding `0x4db0c0` and the list at `0x9fd160`; the objective-time `WAKE_ANIM` starts (`fadein_bszep`, OBJECTIVE13) run the same path later in the mission and are outside this startup ordering |
| who drives the actor after startup (`targets`, `deactivated`, the drive byte `+4`, the undecoded mission program) | whether the spawn attitude is re-applied mid-mission | unchanged from `2026-10-08-m01-lc-world-actor-spawn.md`; the mission scripts (F13-B/C, F38) and #359 |
| `0x440ad0()`'s game-state byte (gates the motion integrator at `0x4bf9f3`) | whether the spawn attitude can be rewritten by motion | `F37-D-FU8` (#740) and #770 session 3's open item |

Nothing here is `verified_original`: every binding is `ObservedTool`, the
session cadence stays `Designed`, and every refusal stays `Unknown`.

## 7. Checks

```text
cargo fmt --all -- --check                                             -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                        -> 0 failures
cargo test --workspace --locked -- accept_m01_lc_zeppelin_attitude_ --include-ignored
                                                                       -> 1 passed
```
