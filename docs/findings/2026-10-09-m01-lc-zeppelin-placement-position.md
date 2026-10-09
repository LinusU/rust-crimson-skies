# #814: the zeppelin position binds the source the original applies last — the startup placement

Date: 2026-10-09. Task `M01-LC-ZEPPELIN-PLACEMENT-POSITION` (#814), filed by
#792 (`M01-LC-ZEPPELIN-ATTITUDE`) for #359 (`VS-M01-RUNTIME`); depends on
#792's measured write order. Feature sheet: `specs/F34-*.md` (the
world-actor schema and its runtime stages) with
`docs/contracts/SCRIPT-MISSION.md`. Capabilities used: **`retail`**
(read-only `$CS_GAME_DIR`) and **static analysis** of the owner-supplied
decrypted image (sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, image
base `0x400000`, file offset = VA − `0x400000` for `.text`/`.rdata`),
disassembled with the platform `objdump -d -M intel` and re-read at every
address below. **No original run happened: nothing here is
`verified_original`.** Test prefix:
`accept_m01_lc_zeppelin_placement_position_`.

## The answer in two paragraphs

**The bind.** Each `zeppelins.zrd` record's `position_m` binds whichever
source the original's own write order leaves on the node last: the
`placezeps.zrd` `OBJECT_TRANSLATE_STATE` triple for a node the member
states one for (M01 states one for all three), and the record's own
`position` for a node it does not. The bound claim stays
`f34-world.zeppelin-spawn-pose` — now honestly describing the whole
measured pose path rather than only the carrier half — and a
startup-sourced value is provenanced from the statement's own `STATE`
bytes inside `zrdr.zbd`, not from the carrier member.

**The order.** The spawn's apply ends by calling `0x4d1d50` — the
`Object3d` position setter that stores x, y, z at `class+0x54`, `+0x58`,
`+0x5c` — with the record's `position` (`0x4bf9b0`, from
`obj+0x20`..`obj+0x28`). The startup translate's executor `0x4e8de0` —
dispatched for event opcode `7` by `[0x727dfc]` — reaches the *same*
setter with the statement's `STATE` triple for a type-`5` node whose flag
byte bit `0` is clear, and it runs from the animation instance's first
tick. #792 §3 measured that no frame can tick inside the mission-start
call stack, so the startup write lands *after* every spawn write — the
translate is the applied position. The losing source stays named on the
row (`SpawnedZeppelinActor::position_residue`), never dropped.

## 1. The translate executor, address by address

`0x4e8de0` is installed as the opcode-`7` (`OBJECT_TRANSLATE_STATE`)
dispatcher at `0x4ee1e3` (`mov dword [0x727dfc], 0x4e8de0`; the opcode-`9`
entry `[0x727e04] = 0x4e8b80` is #792's rotate executor). `esi` is the
event record #791's parser fills; `edi` is the animation instance.

| address | instruction | meaning |
| --- | --- | --- |
| `0x4e8de8` | `mov ax, word [esi+0x1e]` | the base-node selector |
| `0x4e8e0c`..`0x4e8e21` | `jle 0x4e8e23`; index `×11` into `[edi+0xe8]`, `+0x28`; `call 0x4cf490(entry, &base)` | selector `> 0`: the base triple becomes *another node's* current position — the `AT_NODE` case |
| `0x4e8e23`..`0x4e8e46` | `cmp ax, 0xff38`; load `[edi+0x7c]`..`[edi+0x88]` | selector `== −200`: the base triple is a stored triple on the instance |
| `0x4e8df4`..`0x4e8e0c` | `mov dword [esp+8..0x10], 0` | otherwise (selector `0`: no `AT_NODE`) the base is **zero** |
| `0x4e8e5c`..`0x4e8e89` | `fld base.x; fadd [esi+0x10]; fstp` … | the applied triple is `base + STATE`: `event+0x10`/`+0x14`/`+0x18` land in x, y, z with no axis swap |
| `0x4e8e60`, `0x4e8e8d` | `movsx eax, word [esi+0x1c]`; index `×11` into `[edi+0xe8]`, `+0x28` | the `NAME`'s node-table index resolves the target node (null → return) |
| `0x4e8e99`..`0x4e8ea2` | `mov ecx, [eax+0x34]; dec; je 0x4e8ef4; sub 4; jne 0x4e8f35` | node-type dispatch at `[node+0x34]`: type `1` → `0x4e8ef4`, type `5` (`Object3d`) → `0x4e8ea8`, anything else → return |
| `0x4e8ea8` / `0x4e8ef4` | `test byte [esi+0xc], 0x1` | event flag `+0xc` bit `0` is `RELATIVE` |
| `0x4e8ed1`..`0x4e8ee1` | `push z; push y; push x; push node; call 0x4d1d50` | type 5, `RELATIVE` clear: **absolute** write through the spawn's own setter |
| `0x4e8eb6`..`0x4e8ebe` | `call 0x4d1e50` | type 5, `RELATIVE` set: adds to the slots (`fadd [ecx+0x54]` …) |
| `0x4e8ef4`..`0x4e8f2d` | `call 0x4d2710` / `0x4d27d0` | type 1's own absolute/relative setters, same bit |

So a statement carrying no `AT_NODE` and no `RELATIVE` applies its bare
`STATE` triple as the node's absolute position, in stored order. A flagged
statement's applied position is *not* the bare triple — `AT_NODE` re-bases
it on another node, `RELATIVE` accumulates — so the binder refuses to bind
one (`PositionBinding::Open`), the row's reason naming the unmodelled keys
verbatim.

## 2. The setter and the slots both writers share

`0x4d1d50` (`Object3d.c`, assertion string `0x62d118`, the same source
file as #792's rotation setter `0x4d1a30`): the node is `esi`,
`ecx = [node+0x38]` is the `Object3d` class, and the three argument floats
land `[esp+0xc] → ecx+0x54`, `[esp+0x10] → ecx+0x58`,
`[esp+0x14] → ecx+0x5c` (`0x4d1d8f`..`0x4d1da6`). The spawn reaches it from
`0x4bf9b0`..`0x4bf9c0` with the object's `obj+0x20`/`+0x24`/`+0x28` — the
record's `position` as loaded at `0x4bda64`..`0x4bda87` (#770).

## 3. The write order

Unchanged from `2026-10-09-m01-lc-zeppelin-attitude.md` §3 — the same two
writers, the same node, only a different slot range:

* the spawn apply (`0x4becf8` → `0x4bf930` → `0x4bf9b0` → `0x4d1d50`) runs
  inside the mission-start call stack — at `0x46489c` in the data pass and
  again at `0x4655c0` in `0x4654e0`;
* the `ON_STARTUP` instances are *started* at `0x465513`, still inside the
  stack, but dispatch no event there (`0x4edc50` only allocates and
  registers them);
* the per-frame callback `0x4ecd20` → `0x4ecbb0` → `call dword
  [4*eax + 0x727de0]` at `0x4ecc7f` dispatches the startup statements on
  the instance's **first tick** — after the whole start stack has
  returned.

No frame can tick inside the mission-start stack, so every startup-state
write lands after every spawn write: for a node the member translates,
`placezeps.zrd`'s absolute triple is what the node keeps. The carrier's
value is not destroyed — it stays the object's own `obj+0x20`..`obj+0x28`
and reaches the node again only if the object's drive byte `+4` is set,
which #792 §3 measured cannot happen on a single-player launch (the only
setters are console handlers and network-session-gated tick tails).

## 4. What M01's three records end with

Retail `ZBD/C1C/M01/zrdr.zbd`, both carriers decoded by the production
readers. All three `OBJECT_TRANSLATE_STATE` statements carry no key the
grammar leaves unmodelled — no `AT_NODE`, no `RELATIVE` — so each applies
its bare `STATE` triple.

| node | `zeppelins.zrd` `position` (spawn write) | `placezeps.zrd` translate `STATE` (last write) | bound `position_m` | residue |
| --- | --- | --- | --- | --- |
| `piratezep` | (−3 678.6, 1 460.0, −11 985.3) | (−3 584, 1 360, −8 704) | `[−3584, 1360, −8704]` | the spawn position, overwritten on the first tick |
| `workersvoyagezep` | (−4 912.1, 1 360.0, −9 120.0) | (−5 972, 1 460, −7 680) | `[−5972, 1460, −7680]` | the spawn position, overwritten on the first tick |
| `blackswanzep` | (−8 914.0, 1 250.0, 4 767.6) | (−6 656, 1 960, −5 632) | `[−6656, 1960, −5632]` | the spawn position, overwritten on the first tick |

`crates/cs_app/tests/accept_m01_lc_zeppelin_placement_position.rs`
re-decodes both members independently of the implementation, recomputes
the last writer per record from the member contents, and asserts the bound
values, their `STATE`-span provenance and the named residues — it fails
where a carrier-binding keeps z −11 985.3, −9 120 or 4 767.6.

## 5. What changed

* `crates/cs_app/src/mission_world_actors.rs`:
  * `SPAWN_POSE_CLAIM`'s documentation now describes what is bound — the
    value the measured write order leaves on the node — instead of calling
    the ordering unmeasured;
  * `PositionSource` (`StartupPlacement { position_m, state_span }` /
    `CarrierSpawn { position_m }`), `PositionBinding` (`Measured` /
    `Open`), `POSITION_PRECEDENCE_UNKNOWN_CLAIM` and
    `POSITION_PRECEDENCE_UNKNOWN_REASON_PREFIX`;
  * `StartupRead::Decoded` now carries `StartupStatement`s — kind, `NAME`,
    the parsed triple, the `STATE` list's own `SourceSpan` and the
    statement's unmodelled keys — one shape serving #792's rotations and
    this task's translates;
  * `position_for` picks the source under the measured order; a refused
    carrier or a flagged translate settles nothing and stays open;
  * `SpawnedZeppelinActor::{position, position_source, position_residue}`
    and `declare_actor` binding `position_m` `Resolved::Known` under the
    spawn-pose claim, provenanced from the `STATE` span for a startup
    value and from the carrier member for a spawn value.
* `crates/cs_app/src/mission_launch.rs`: the `world_actors` detail names
  the measured position. **The gate is untouched**: `faction` stays
  refused, the surface still reports `unsupported`, `plan.launchable()`
  stays false.
* `crates/cs_app/tests/campaign/vs_m01_runtime.rs`: the closure comment
  records that position and attitude both bind measured.
* `crates/cs_app/tests/accept_m01_lc_zeppelin_placement_position.rs`
  (new): the retail acceptance test above; a synthetic unit test of the
  same prefix covers the carrier fallback and the open case.

## 6. Residues, affected content and resolving work

| residue | affected content | resolves in |
| --- | --- | --- |
| a **flagged translate** (`AT_NODE`, `RELATIVE`, any key the placement grammar leaves unmodelled) does not apply its bare `STATE` triple; M01's three carry none, but the binder refuses the value rather than guess one | any scope whose `placezeps.zrd` flags a translate | `PLACEMENT_FIELDS_CLAIM`'s unmodelled-key work (#791's member census) |
| the **animation-start reset** (`0x4edc50` zeroes the target node's position and rotation when instance flag `0x100` is set) | a third writer at animation-start time; on the measured order it precedes the record (re)load at `0x4655c0`, so it cannot disturb the startup pose | unchanged from #792 §6 — decoding `0x4db0c0` and the list at `0x9fd160` |
| selector `0xff38` (`−200`) at `event+0x1e` re-bases the triple on a stored instance vector (`[edi+0x7c]`..`+0x88`) | the meaning of that stored vector | the `AT_NODE`/base-selector half of `0x5075b0`'s statement parse |
| who drives the actor after startup (`targets`, `deactivated`, the drive byte `+4`, the undecoded mission program) | whether the spawn position is re-applied mid-mission | unchanged from `2026-10-08-m01-lc-world-actor-spawn.md`; the mission scripts (F13-B/C, F38) and #359 |

Nothing here is `verified_original`: every binding is `ObservedTool`, the
session cadence stays `Designed`, and every refusal stays `Unknown`.

## 7. Checks

```text
cargo fmt --all -- --check                                             -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                        -> 0 failures
cargo test --workspace --locked -- accept_m01_lc_zeppelin_placement_position_ --include-ignored
                                                                       -> 2 passed (1 synthetic, 1 retail)
```
