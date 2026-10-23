# #1879 FLAK-C-BURST-EFFECT-PARTS: what starts the `flak_effect` at a burst, the rocket range end, who consumes turret `AMMO`

Date: 2026-10-23. Static analysis only: `$CS_ENGINE_IMAGE` (read-only, never committed) was disassembled with
radare2 into `private/`; the original executable was not run, so every claim is `static` and nothing is
`verified_original`. Implementer: `sonnet-2` (Sonnet 5.5); no independent review yet. Addresses are virtual addresses
in the image. No table rows were added: every cited instruction is re-read by the ignored test
`accept_flak_c_burst_effect_parts_cited_sites_match_the_image` (`crates/cs_app/tests/campaign/flak_c_burst_effect_parts_static.rs`),
which holds the VA and the bytes of each site. Builds on `2026-10-22-flak-c1-burst-presentation-and-descriptor-flag.md`
(C1) and `2026-10-21-flak-a-turret-update-law.md`.

## 0. Answers

| Question | Answer |
| --- | --- |
| C1's open item: why does the `esp+0x44` guard in front of the row `ANIMATION` start look unreachable? | **It does not test the hit-record pointer.** At `0x5ac814` the impact code stores the *return value of the hit handler `0x5abcf0`* into `[esp+0x5c]` while the six argument words of that call are still pushed; that slot is the impact code's own second argument slot (`esp+0x44` after the cleanup at `0x5ac81b`). The guard at `0x5ac93a` therefore asks "did the hit handler return non-zero?" (section 1). |
| Does a flak burst start the row `ANIMATION`? | **Yes, unless the hit handler returns non-zero.** For the rocket's own node it returns 0 (section 2), so the burst starts the default row's `ANIMATION flak_effect` at the burst position. |
| Which `flak_effect` parts run, and where? | The weapon code starts **one animation instance** and never selects parts: `flak_flashes`, `call_flak_trails`, `large_black_smokeball` and the `IF PLAYER_RANGE` flash are events of that animation (F20/F27 runtime). The weapon code's only inputs are the handle and the burst position (`rocket+0x48`, copied into the hit record) (section 3). Nothing else of the default row runs for `wep_27`: it has no `SURFACE_ANIMATION` and no `EFFECT` key. |
| What does a rocket without `DETONATE_AT_RANGE` do at range end? | C1's table is confirmed byte by byte (section 4). A rocket with neither `LOCK_ON` nor `DETONATE_AT_RANGE` ends **silently**: no burst, no animation, no sound. **Both** the silent end and the burst clear `rocket+0x20`. |
| Who consumes turret `AMMO`? | **Nothing in the image reads it as an ammunition count.** `0x5aef40` and `0x441830` do not touch the turret entry's `+0x164`; the key string has one reference, the parser (section 5). A turret never runs dry in the original code that was traced. |

## 1. The animation guard is the hit handler's return value

`0x5ac7a0(weapon, record, rocket, override)` (cdecl, C1 calls it from `0x5ac3a0`). Its prologue is `sub esp, 0x2c`
(`0x5ac7a0`), `push ebx`, `push ebp`, then `push esi`, `push edi` (`0x5ac7b0`), so with the return address the
arguments sit at `esp+0x40` (weapon, loaded into `esi` at `0x5ac7c3`), `esp+0x44` (record), `esp+0x48` (rocket,
loaded into `edi` at `0x5ac7cf`) and `esp+0x4c` (override animation handle, read at `0x5ac956`). The record was
copied to `ebp` before the last two pushes (`0x5ac7a5`).

The call `0x5ac80f: call 0x5abcf0` is preceded by six `push`es (`0x5ac7ee..0x5ac80a`) and followed by
`0x5ac814: mov [esp+0x5c], eax` and `0x5ac81b: add esp, 0x18`. With the six words still on the stack, `esp+0x5c`
is `0x5c - 0x18 = 0x44` in the cleaned-up frame: the record argument's slot. The animation start block then reads
that slot (`0x5ac93a: mov eax, [esp+0x44]; test eax, eax; jne 0x5ac987`). C1 read the slot as the record pointer
(always non-null), which is why it found the block unreachable; the slot holds the handler's result.

Conditions before the block, in order (all in `0x5ac7a0`):

| site | gate | meaning for the row animation |
| --- | --- | --- |
| `0x5ac7c7..0x5ac7dd` | the hook `weapon+0x20c` (`TANGLER`, C1) is called if set; its result goes to `[esp+0x10]` | `wep_27` has none, so the flags are 0 |
| `0x5ac820..0x5ac84e` | weapon flag `0x10` (`CRATER`) and hook flag `2` clear: skip flag `[esp+0x14] = (weapon flag 0x800000 clear) & crater(0x5ac690)` | the crater step returns 0 unless the node has mask `0x10000` (`0x5ac6a0`, `0x5ac69e`), so the skip flag stays 0 |
| `0x5ac850..0x5ac8f9` | weapon flag `0x40000`, hook flag `2` clear: `0x5ac580` is called (once or twice) | `0x5ac580(weapon, record, row, rocket)` returns at once when its third argument is non-zero (`0x5ac58a`; in the common call the row index, 0 for a burst) and otherwise needs mask `0x10000` on `[record+0x24]` (`0x5ac597`) |
| `0x5ac910` | hook flag `1` clear: `0x5ad100(weapon, record, row, 1.0)` | the `SOUND` pick (C4) |
| `0x5ac927` | skip flag non-zero: jump over the animation block | a crater was placed |
| `0x5ac933` | hook flag `4` set: skip the block | not set for `wep_27` |
| `0x5ac93a` | **hit handler result non-zero: skip the block** | section 2 |
| `0x5ac942..0x5ac984` | the row `ANIMATION` handle `[[weapon+0x15c] + row*100 + 4]`, replaced by `override` when that is non-null; if the handle is non-null, `0x4edc10(handle, 0, x, y, z, 0,0,0,0,0,0)` with `(x, y, z) = record+0xc..0x14` | **the start** |

Whether a node ever has mask `0x10000` is not shown: the only setter of that mask on `node+0x24`, `0x4ccb20(node, on)`,
has three call sites (`0x4c2184`, `0x4d7d48`, `0x5bad7c`), none in the weapon pool/fire code (`0x5aeca0`,
`0x5aef40`). So the C1 assumption (no crater for an air burst) stays an assumption, now with this evidence.

## 2. What `0x5abcf0` returns for a rocket burst

`0x5abcf0(weapon, rocket+4, position, record, damage[2], override)`. `esi` is the record, `[record+0x24]` the node
(the rocket's own node, `0x5ac3d2`), `edi = [node+0xbc]` the node's hit handler.

* The result is a local initialised to 0 at `0x5abd38`, returned at `0x5abf63`. It is overwritten only at `0x5abe6b`
  by the handler's own function: `call [edi+4]` at `0x5abe68`.
* No node, or no handler at `node+0xbc`: the function returns 0 at once (`0x5abd1c..0x5abd31`, `0x5abf6c`).
* `node+0xbc == 1` (`0x5abd35`) with a damage pointer: the rocket node is treated as a damageable thing; the code
  subtracts the damage from `rocket+0x670` and `+0x674` (clamped at 0, `0x5abd51..0x5abdc9`) and jumps to
  `0x5abf51`, so the result stays 0.
* `node+0xbc` is written **once in the weapon code**: `0x5af6a9` (`mov dword [node+0xbc], 1`) in the launch
  `0x5aef40`, only for a weapon with flag `0x8` (`0x5af664: test al, 8`); the other branch clears the node's mask
  `0x8000000` and never writes the field. The pool allocator `0x5aeca0` does not write it. No pointer handler is
  installed by either function.

So for a rocket's own burst the handler result is 0 and the row `ANIMATION` start in section 1 runs. (The handler-present
branch, `0x5abe62..0x5abf4e`, itself starts animations: the weapon's `[weapon+0x18c]` when the global `0xa1e174` is set,
the override, a list at `weapon+0x168` selected by the damage scale `0xa1e178`, or the row animation when weapon flag
`2` and `[0xa1d7dc]` are set. None of it is reachable with `node+0xbc` 0 or 1.)

Which key sets weapon flag `0x8` (and whether `wep_27` has it) is not identified; it does not change the result.

## 3. The start, and where the parts run

`0x4edc10` re-pushes its eleven arguments and calls `0x4ed730(handle, node, x, y, z, a, b, c, ...)`. `0x4ed8c0`
turns the handle into a running instance (it returns 0 for several instance kinds at `0x4ed8d5..0x4ed8f0`, in which
case nothing starts), then `0x4ed730`:

* if the instance has bit `0x100` in `+0x9c` (`0x4ed751..0x4ed75a`), moves the instance's root node `[inst+0x48]`
  to `(x, y, z)` (`0x4d1d50` for node kind 5, `0x4d2710` for kind 1) and, unless bit `0x200` is set, orients it
  (`0x4d1a30`/`0x4d2490`);
* stores the last three floats at `+0xc0/+0xc4/+0xc8`, clears the instance's counters (`+0x7c..+0x98`) and calls
  `0x525570` with the full argument list (`0x4ed8aa`).

The burst passes node 0, the position from the record and six zeros. **Where the parts run** therefore follows from
the animation's own record: the compiled `flak_effect` events (flashes, trails, black smoke ball, the
`PLAYER_RANGE` flash) are placed by the animation runtime relative to that root. This task did not decode the
event runtime (`0x525570` and the sequence code), so it cannot say which node a part attaches to or when the `IF`
is evaluated.

The remaining row members, for completeness (`0x5ae9bf..0x5aeb57`, the row parser; row stride 100):

| row offset | key | when it runs at a burst |
| --- | --- | --- |
| `+4`, `+8` | `ANIMATION` (up to two names) | `+4` is started as above; `+8` is not read by the impact code |
| `+0x1c` | `SURFACE_ANIMATION` | `0x5ac996..0x5ac9f3`: started at the position, oriented from the record, when non-null; skipped under the same gates as the block above |
| `+0x24` | `EFFECT` | `0x5aca07..0x5aca1b`: `0x525d90(handle, record+0xc)` unless hook flag `2`; runs for every burst that has the key |

`wep_27`'s default row has `ANIMATION flak_effect` and `SOUND snd_missile_flak` only (C1), so for a flak burst
**only the `ANIMATION` start and the `SOUND` pick run**. This corrects C1's wording: the `row+0x24` start at
`0x5aca1b` is the row's `EFFECT` key (the name is resolved at parse time by the table lookup `0x526160`), not the
`SOUND` key.

## 4. Range end (`0x5b01e6..0x5b0318`)

The test is `rocket+0x664` (travelled distance, `0x5b01e6`) against `weapon+0x1c` (`RANGE`, `0x5b01ec`); below the
range the proximity/time branch at `0x5b0216` runs. At or beyond it, with `L` = `0x8000` (`LOCK_ON`), `E` = `0x80`
(`EXPIRES`) and `D` = `0x2000000` (`DETONATE_AT_RANGE`) of `weapon+0x74`:

| `LOCK_ON` | `EXPIRES` | `DETONATE_AT_RANGE` | result | site |
| --- | --- | --- | --- | --- |
| no | any | yes | burst | `0x5b01f9 test ah,0x80` -> `0x5b0206 test eax,0x2000000` -> `0x5b02ba` |
| no | any | no | silent end | `0x5b020b je 0x5b0318` |
| yes | no | any | burst | `0x5b01fe test al,0x80; je 0x5b02ba` |
| yes | yes | yes | burst | falls to the `D` test |
| yes | yes | no | silent end | |

The burst copies `rocket+0x48` three times into the impact globals `0xa1d7a4..0xa1d7c4` (`0x5b02ba..0x5b0309`), calls
`0x5ac3a0(weapon, rocket)` (`0x5b0310`) and **then falls through into `0x5b0318: mov dword [esi+0x20], 0`**, the
same store the silent end performs. So the entity is released in both cases; the silent end differs only by the
missing impact call. This confirms C1 and adds the shared store.

## 5. `AMMO`

* The key string `AMMO` is at `0x629c98`; the image has exactly **one** reference to its address, the parser's
  `push 0x629c98` at `0x4aa2de`. The value is stored at `0x4aa2f6` (`mov [ebx+0x164], ecx`), defaulted to 50 at
  `0x4a9bed` and copied by the save-state code at `0x4ac693`/`0x4ac6f7`. No other instruction in
  `0x4a9000..0x4ad000` has displacement `0x164` (FLAK-A said the same for `0x4a9950..0x4ac720`).
* `0x5aef40` (launch, `0x5aef40..0x5af714`) takes the weapon, the mount node and the lock-on record, not the turret
  entry; its bytes contain no `0x164` displacement. `0x441830` (`0x441830..0x441a19`) is not an ammunition
  routine: it sets the rocket's per-frame hooks (`rocket+0x688 = 0x441780` for descriptor flag `0x8000`,
  `rocket+0x68c = 0x4417c0`, `rocket+0x690 = 0`) and allocates the `0x70`-byte tracker objects for descriptor flags
  `0x20` and a positive `weapon+0x44`; its bytes contain no `0x164` displacement either.

So `AMMO` and `AMMO_LIMIT` (`wep_27`: 9999) have no consumer among the code traced. Code that reads the entry through a
different base register outside `0x4a9000..0x4ad000` was not searched instruction by instruction (the whole image
was grepped for `[reg+0x164]` and has hundreds of unrelated structures with that field).

## 6. Unknowns kept open, with the content they affect

| unknown | evidence searched | content affected | resolves in |
| --- | --- | --- | --- |
| Which node the `flak_effect` parts attach to; when `IF PLAYER_RANGE` is evaluated | `0x4ed730` read; `0x525570` and the sequence runtime not decoded | placement of trails/smoke relative to the burst; timing of the screen flash | animation event runtime trace (F20/F27) |
| Whether `flak_effect`'s instance has bit `0x100` in `inst+0x9c` (what positions the root at the burst point) | who sets that bit not traced | whether the burst position is applied to the root at all | same |
| Whether the rocket node ever has mask `0x10000` (crater/`0x5ac580` gates) | setter `0x4ccb20` call sites listed | a crater would suppress the animation | node-mask trace |
| The key behind weapon flags `0x8` and `0x40000` | parser `0x5ad630` keys with a plain shift are listed; these two are set another way | `0x5af6a9`, `0x5ac850` gates | weapon parser trace |
| Other writers of `node+0xbc` | only the weapon code was searched | a handler returning non-zero would skip the animation | node-class trace |
| Ammunition readers through other base registers outside `0x4a9000..0x4ad000` | not searched | turret ammunition | only if a capture shows a turret going silent |

Method: radare2 disassembly of the decrypted image (`pD` at 32-bit, base `0x400000`), a full-image disassembly
for cross-reference searches (callers of `0x4ccb20`, `0x441830`, the `AMMO` string address as a 4-byte value);
the committed test re-reads the instruction bytes named in sections 1-5 and the derived facts (call targets, the
slot arithmetic of section 1, the unique `AMMO` reference, absence of the `AMMO` displacement in the two launch
routines).
