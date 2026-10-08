# M01-LC-CAMPAIGN-AIRFRAME-POSE: the campaign airframe is engine state, and what is still missing

Date: 2026-10-08. Task: #770 `M01-LC-CAMPAIGN-AIRFRAME-POSE`, sessions 1 and 2
of an unfinished implementation claim (§1–§8 are session 1, §9–§10 are session
2). Capabilities used: **`retail`** (the owner's installation read-only) and
**static analysis** of the owner-supplied decrypted executable. **No original
run happened; nothing here is `verified_original`**, and no airframe or yaw is
bound for M01 (AGENTS.md rules 4 and 5).

## Sources and method

* Retail data, read-only: `$CS_GAME_DIR` (`CS_CAPABILITIES=retail,gpu,audio`),
  notably `ZBD/C1C/M01/zrdr.zbd` and `ZBD/interp.zbd`.
* `$CS_GAME_DIR/crimson.decrypted.exe`, sha256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` — the same
  image #390/#436/#715 worked from. Virtual addresses below `0x643000` are file
  offset `VA − 0x400000` (`.data` raw ends at VA `0x643000`; everything at
  `VA ≥ 0x643000` — including every global named below — is zero-initialised
  `.bss`). Read with `strings`, radare2 (`izz`, `/r`, `/x`, `pd`) and small
  Python scans over the file; only addresses, constants and control flow are
  recorded here, never bytes or decompiled code.
* Prior measurements this builds on: #715 (`docs/findings/2026-10-06-m01-lc-player-airframe-source.md`),
  #676/#634 (`docs/findings/2026-10-05-m01-lc-player-config.md`), #436's owner
  note of 2026-10-05 (metre, +Y up, right-handed, `.zrd` angles in degrees).

This note answers, with evidence, **which state assigns a campaign mission's
player airframe** — #715's open question 1. It does **not** answer the start
pose's heading zero direction and handedness (question 2): that conversion
routine was not located in this session and stays unknown.

## 1. The runtime `aiv.zrd` loader

* `0x439540`..`0x43968e` builds the member path with `sprintf` from the chapter
  (`0x4639d0`, e.g. `c1c`) and mission (`0x463a50`, e.g. `m01`) of the session
  object at `0x71b480`, using the format `..\data\%s\%s\zrdr\aiv.zrd`
  (`0x62283c`), opens it (`0x579c60`) and parses the records into the container
  at **`0x64f620`** = `{ +0: flags, +4: record count, +8: array of record
  pointers }`, each record `0xc` bytes (`{ +0: name, +4: name copy, +8: document
  object }`, allocated in the loop at `0x4395dc`).
* The container is read by the aircraft-spawn path at `0x474cbb` / `0x474cdb`
  (record index from `[esp+0x14]`, then `record->document`), and by the
  `params` reference resolver at `0x452ab9`..`0x452b42` (a named document field
  `params` naming an `aiv` record: linear scan over `[record->name]` with
  `strcmp`, then the matched record's document is stored at `obj+0x3c`).

## 2. How the `player` record's airframe is chosen (the spawn site)

In the spawn routine, for a record whose name compares equal to the literal
`player` (`0x474d1b`, compared against `0x627b2c`):

| address | instruction | meaning |
| --- | --- | --- |
| `0x474d31` | `mov ecx, 0x71b480` | the session object |
| `0x474d36` | `call 0x4639b0` | returns `session->field_0x700 == 3` (the field is `0x71bb80`) |
| `0x474d3d` | `je 0x474d48` | not mode 3 → the branch below |
| `0x474d3f` | `call 0x45be70` | `mov eax, [0x718cdc]; ret` — the scenario object's field `+4` |
| `0x474d48` | `mov esi, [0x71daec]` | **the campaign/global player-airframe index** |
| `0x474d4f` | `call 0x426cc0` | index → row's **scene root** (`0x620c74 + i*28`) |
| `0x474d55` | `call 0x47fcb0` | apply the scene root |
| `0x474d5b` | `call 0x426ce0` | index → row's **model** (`0x620c78 + i*28`) |

The index→row accessors `0x426ca0`/`0x426cc0`/`0x426ce0`/`0x426d00`/`0x426d20`/
`0x426d40`/`0x426d60` are seven 20-byte functions (`lea ecx,[eax*8]; sub ecx,eax`
→ `i*7`, then `mov eax,[ecx*4 + 0x620c70 + field*4]`), i.e. row stride `0x1c`
with the same seven pointers #715 transcribed. The only *other* reference to the
table base `0x620c70` in the whole image besides `0x426d80`'s walk is `0x426cad`
(the first accessor), so **the table is read by name (0x426d80) and by index
(accessors only)** — there is no mission→airframe map in it, confirming #715.

For a record named `wingman_1` (`0x474e3e`) the same block reads the global
`0x71db4c` and replaces the "none" value `11` with `5` (`0x474e76`..`0x474e85`);
for `wingman_4` (`0x475016`) the global `0x71daec` (the player's own airframe) is
used **only** when chapter/mission is `c3`/`m05` or `c4`/`m04` (`0x47502c`..
`0x4750a4`), compared with the chapter (`0x4639d0`) and mission (`0x463a50`)
accessors of the same session object. M01 is neither, so that override does not
apply to it.

## 3. Which mode a campaign mission runs in

`0x4639b0` answers `field_0x700 == 3`; `0x4639c0` answers `== 2`. The loading
screen selection at `0x4a19d3`..`0x4a1a17` names the modes:

* mode **3** → document `ia_loading.zrd` (`0x62952c`),
* mode **2** → document `mp_loading.zrd` (`0x62953c`),
* otherwise → document `loading.zrd` (`0x62954c`).

The mode is written as a constant immediately before `0x4638f0` (which loads
`support\init.gw`, `support\main.gw` and `support\%s\%s.gw` for the chosen
chapter and mission):

| site | value | context |
| --- | --- | --- |
| `0x41728d` | 1 | after the selected plane record is applied (§4) — a mission start with the player's plane |
| `0x41a8ac`, `0x427bde`, `0x442b2e` | 1 | other mission-start/return paths |
| `0x41758a` | 3 | after `[0x64aba4]` (0..6) selects the mission type and `[0x64aba0]` (0..3 → 0/1/4/2) sets the scenario object's `+0` at `0x417554`..`0x417578` — the instant-action type selection |
| `0x442b7c` | 3 | tiny setter, `0x4638f0` then `ret` |
| `0x496c7e` | 2 | multiplayer |

So **a campaign mission (mode 1) takes the player's airframe from `0x71daec`**;
only instant action (mode 3) takes it from the scenario object, which is the
`ia.zrd` path #715 already measured.

## 4. What `0x71daec` is, and who writes it

Four absolute references in the whole image (`/x ecda7100`):

* `0x416eb5` — **setter**: `0x416ea0(ptr)` computes `idx = 0x416db0(ptr->field_0x2c)`
  and stores `idx` into `0x71daec` when `idx < 11`. `0x416db0` is an identity
  table walk over the 11 pairs at `0x61f670` (`{0,0}..{10,10}`), answering `11`
  for any other value — so `ptr->field_0x2c` **is an airframe row index**.
  Its two callers:
  * `0x413a8a` — passes the fixed parameter block **`0x642fd0`** (`.data`,
    zero-initialised, `+0x2c` = `0x642ffc`). That field is written by the menu
    code at `0x40e45d`, `0x40e486`, `0x40ef7c`, `0x40fa46`, `0x4144af` and read
    back at `0x40e4ac` (`push 0x642fd0; call 0x40fdf0`).
  * `0x41712c` — passes `&records[[0x64b67c]]` where `records` is the roster at
    **`0x64b78c`**, record stride **`0xcc` (204)**, `field_0x2c` again the
    airframe index (`0x64b78c + 0x2c = 0x64b7b8`, read at `0x405d2b`,
    `0x4078b1` uses the same `lea … 0x64b78c` with the `*204` scale). This call
    sits inside the function `0x417114`..`0x41740c` that also sets mode 1 and
    starts the chapter/mission scripts (§3), i.e. **the campaign start applies
    the selected plane record's airframe to `0x71daec`**. The same function then
    builds the `player` (at `0x41737b`) and `wingman_1` (at `0x4173ca`) record
    overrides through `0x414f40`, with the wingman's airframe taken from
    `records[[0x64b680]].field_0x2c` (`0x4173c2`).
* `0x474d49` / `0x4750a5` — the two readers (§2).
* `0x4b3786` — **reset default**: a parameter-reset routine spanning
  `0x4b3750`..`0x4b37f7` writes `0x71daec = 5` alongside `0x71daf0 = -1`,
  `0x71daf8`/`0x71dafc`/`0x71db00`/`0x71db04` = `-1.0f`, the wingman block
  `0x71db4c`..`0x71db64`, and the flags `0x71dac8`..`0x71dae8`. It has no direct
  `call`/`jmp` reference in `.text`, so it is reached indirectly (vtable or
  jump table); its entry is still unlocated.

Row **5** is `Devastator` / scene root `player_pfighter` / model `piratefighter`
(the table #715 transcribed, pinned on main by `AIRFRAME_TABLE`).

The instant-action path has the *same* default: the scenario setup (`0x4574d0`,
whose only caller is `0x49ff10`; the per-method entry `0x459390` is called at
`0x45a222` with `this = 0x718cd8`) reads `player_plane` at `0x4593e5`, resolves it
with `0x426d80`, stores the answer at `this+4` (`0x459409`) and, when the answer
is `11` (none), stores **`edi = 5`** (`0x4593c9` sets `edi`, `0x45940e` stores
it). `0x718cd8` is therefore a small scenario object `{+0: mission-type index,
+4: player airframe row}` and `0x718cdc` is its field `+4`.

## 5. The selection index and the profile variable

`[0x64b67c]` is the index of the selected roster record:

* `0x405f9c`..`0x405fbc` scans `records` (`add eax, 0xcc`) for the record whose
  `+0` dword is `2` **and** whose `+0x2c` equals the wanted airframe index, and
  stores the found index into `[0x64b67c]` — a "select the plane of this
  airframe" helper;
* `0x4094f7` stores a UI result: `[edi*4 + 0x64b67c] = 0x4101d0(arg)` (with
  `[edi*4 + 0x6480bc] = -3` written beside it at `0x4094fe`);
* `0x405d25` copies it into the profile variable registered as **`nPrevPlane`**:
  the registration block at `0x402128`..`0x402141` pushes `0x6480c4` (address),
  type `1`, and the name `nPrevPlane` (`0x61e854`), so `nPrevPlane` **is**
  `dword [0x6480c4]`, written from `[0x64b67c]`.

Roster record layout as used: `+0` type (value `2` selects a plane, `0x405fa1`),
`+4` name (copied with `lstrcpyA` at `0x405df6`), `+0x2c` airframe row index.
The roster is `.bss` (base `0x64b78c ≥ 0x643000`), so its **initialisation
routine was not located in this session**; `0x4078a4` also addresses
`records[0x19]` (25), i.e. the roster is larger than the eleven airframes.

## 6. Verdict for the two open questions

### 6.1 Campaign player airframe — narrowed, not yet bound

**Measured:** for a campaign mission the player's airframe comes from engine
state — the **airframe field of the selected plane record of the roster**, the
profile/flight-check selection (`nPrevPlane` names it), applied to the global
`0x71daec` at mission start; it is *not* the mission's data, *not* a file in the
installation (#715's negative results stand) and *not* an instant-action
document. Both engine paths default the same row, **5 = `Devastator`**, when no
selection exists (`0x4b3786`; `0x45940e` for instant action).

**Still unknown, therefore still `Resolved::Unknown` in production:**

1. the roster's and `[0x64b67c]`'s value in a fresh, **profile-less**
   installation: the roster initialisation routine (what `records[0].field_0x2c`
   is before any profile or menu writes it) and the function that reaches the
   `0x4b3750` reset were not located, so "M01 launches in a `Devastator`" is a
   *candidate with two measured defaults*, not a measurement of M01's launch
   (AGENTS.md rule 5). Resolving: continue this static pass (locate the roster
   init for `0x64b78c` and the entry of `0x4b3750`), F44/F45/F48's
   profile/flight-check shape, or #358's owner-supplied original run.

### 6.2 Start pose heading zero direction and handedness — unchanged

Not measured. This session did not locate the routine that turns the player
record's field 1 (three floats) and field 2 (degrees-like heading) into a world
orientation. What was measured and is recorded here so the next pass does not
repeat it:

* the aiv container readers (`0x64f620`): `0x4396a4` (teardown), `0x439af2`/
  `0x439bb9` (name list building), `0x452ab9` (by-name reference), `0x474cbb`/
  `0x474cdb` (spawn), `0x4232b1`..`0x425f33` (container registry);
* the spawn routine around the airframe selection reads *named* fields of a
  document (`kind_of` at `0x474ba5`) and never an obvious positional field 1/2
  in the region `0x474b80`..`0x475340`;
* `.. \data\cx\mxx\zrdr\aiv.zrd` also appears at `0x4384f0`/`0x439570` (the
  sprintf buffer of §1);
* the **zeppelin** pose parse at `0x4bda64`..`0x4bdac1` reads named `position`
  (three floats → `obj+0x20/0x24/0x28`), `yaw` and `pitch` (degrees → radians
  with `qword [0x6040e8]`, stored at `obj+0x2c` / `obj+0x30`) — a *different*
  record family (the `zeppelin` records of `zeppelins.zrd`, which carry the keys
  `node position yaw pitch max_speed …` in M01's archive), useful only as proof
  that `.zrd` angles are degrees at load (#436) and that a shared conversion
  helper may exist;
* `origin`/`rotation` (three degrees→radians at `0x4528ec`..`0x452910`) belong
  to the `zeppelin`/`open_anim` records of `placezeps.zrd`, not to `aiv`;
* the deg→rad constant `qword [0x6040e8]` has 123 references; the ones inside
  the spawn region (`0x47439d`…`0x4748fa`) are flight-parameter parsing
  (`maxAOA`, `liftAOAs`, `smokescreen_stun_angle`), not the pose.

Resolving: a decoded native spawn/conversion routine (continue above), or #358's
owner-supplied original run — never an invented yaw.

## 7. What changed in this session

* **No production code and no tests were changed.** Neither question is answered
  strongly enough to return `Resolved::Known` (rule 5): an airframe chosen
  between two measured defaults, and a heading with no landmark at all.
* This note only.

## 8. Affected content and resolving work

| unknown | affected content | resolving work |
| --- | --- | --- |
| the roster/selection value on a profile-less install (which row `0x71daec` holds for M01) | M01's and every campaign mission's player spawn; the `player_configuration` launch surface (#359) | **closed by session 2 for a profile-less install (§9.1); a player who has moved the hangar selection can still differ, and where a profile is stored is unmeasured** |
| heading zero direction and handedness | `initial_pose()` for every mission; camera/spawn yaw | **the conversion site is now located (§9.2); what is left is the `Object3d` rotation triple's world meaning**, then this row or #358 |
| frame relation between `aiv.zrd` starts and the world grid (194/777/826 stored units outside `c1c`) | every start pose's world placement | #436's remaining work, F16-E-2's prose sync |

---

# Session 2 (same claim, same task): the roster default, and the pose's
# conversion site

Same date, same capabilities (`retail` + static analysis of
`$CS_GAME_DIR/crimson.decrypted.exe`, sha256 `43540fc9…`), same method
(strings, radare2 `pd`, small Python byte scans over the file). **No original
run happened; nothing here is `verified_original`, no production code and no
test was changed.**

## 9.1 The campaign airframe's initial value on a profile-less install

Session 1 ended with two open sub-questions: the roster's initialisation and
the entry of the `0x4b3750` reset. The **roster initialisation is now
measured**, and it makes the reset's entry irrelevant to the value below.

**The init function `0x4113b0` (…`0x411639`) is called from exactly two sites,
`0x407a7e` and `0x411361`; `0x411361` sits inside the startup parameter
routine that first sets `[0x6480c8] = 0x619f58`.** That pointer is written
once in the whole image (`0x41134d`) and read only by `0x4113b0`'s body, so it
is the fixed `.data` block `0x619f58` for the rest of the run.

What `0x4113b0` does, in order (addresses from `crimson.decrypted.exe`):

| address | instruction | meaning |
| --- | --- | --- |
| `0x411420` | `mov ecx, 0x52e` / `mov edi, 0x64b78c` / `rep stosd` (eax = 0) | **zero the whole roster**: 1326 dwords = 5304 bytes = 26 records of 204 bytes (`0x64b78c`..`0x64cc50`) |
| `0x411477` | `mov dword [0x64b67c], ebx` (ebx = 0) | **selection index := 0** |
| `0x41147d` | `mov dword [0x64b680], 1` | wingman-1 roster index := 1 |
| `0x411487` | `lstrcpyA(0x64b684, …)` | the pinup name |
| `0x411579`…`0x4115a6` | `mov ecx, [0x6480c8]` / `lea esi, [ecx+0x8c4]` / `mov edi, 0x64b78c` / `rep movsd` (51 dwords) then `lstrcpyA(0x64b790, string(0x1ff))` | **roster[0] := the 204 bytes at `0x619f58 + 0x8c4` = `0x61a81c`**, name from string id 511 |
| `0x4115aa`…`0x4115d5` | same with `esi = [0x6480c8] + 0x990`, `edi = 0x64b858` | **roster[1] := `0x619f58 + 0x990` = `0x61a8e8`**, name from string id 512 |
| `0x4115e2` | `mov esi, 0x64b78c` / `mov edi, 0x64cb78` / `rep movsd` | a second copy of roster[0] at `0x64cb78` (the template the hangar paths copy from, `0x40b60a`) |
| `0x411600`…`0x411633` | gated on `cmp dword [0x647b5c], ebx` / `je 0x411635`; copies `0x619f58 + (i−2)·204` into records 2..12 and stores `1` at each record's `+0` | the 11 airframe records; **`[0x647b5c]` is `.bss` and no reference in the image writes it**, so on the measured image this gate is false and records 2..12 stay zero — either way record 0 is untouched |

The source block's own contents, read straight from the file:

```
src[ 0..10]  +0 = 0   +0x2c = 0,1,2,…,10     (one row per airframe index)
src[11] va=0x61a81c  +0 = 1  +0x2c = 5   <- seeds roster[0]
src[12] va=0x61a8e8  +0 = 1  +0x2c = 5   <- seeds roster[1]
```

So **`roster[0] + 0x2c` (`0x64b7b8`) starts at 5**, and the wingman's
`roster[1] + 0x2c` starts at 5 as well.

**Nothing but the init writes `roster[0] + 0x2c`.** All nine absolute
references to `0x64b7b8` in the image decode as reads (`mov reg, [..]`):
`0x405d2b`, `0x405de5`, `0x409453`, `0x4094bc`, `0x409836`, `0x40a123`,
`0x417042`, `0x4172bc`, `0x4173c2`. The only writes are the `rep movsd` /
`stosd` above. (A record written through a base register is the same nine
sites; there is no other `mov edi, 0x64b78c`-style store.)

**`[0x64b67c]` (the selection) has seven writers** and only one of them is
startup: `0x411477` (init, = 0). The other six are `0x405fb4` (find the type-2
record whose `+0x2c` equals a wanted airframe), `0x406030` (select a plane by
index), `0x4094f7`, `0x409580`, `0x4095db` (menu handlers), `0x40b61a` (copy
the `0x64cb78` template into a record and select it) — every one of them a
hangar/plane-selection path, none on the way from startup to a campaign
mission start. Every other reference reads it (`0x405d05`, `0x405dcc`,
`0x4094d1`, `0x40a039`, `0x40a1c1`, `0x4101e5`, `0x41022f`, `0x411xxx`,
`0x417114`, `0x417131`, `0x417172`, `0x4172a9`, `0x443dfd`), including the
whole campaign-start function `0x417114`…`0x41740c`.

**The chain therefore closes for a fresh, profile-less installation:**

```
startup 0x4113b0 : selection = 0, roster[0] = src[11] (+0x2c = 5)
campaign start 0x41712c : call 0x416ea0(&roster[selection])
0x416ea0/0x416db0 : idx = identity_table(roster[0]+0x2c) = 5, 5 < 11 -> [0x71daec] = 5
spawn 0x474d48 : mode != 3, so the airframe row read from [0x71daec] = 5
row 5 = Devastator / scene root `player_pfighter` / model `piratefighter`
```

Three independent engine defaults now agree on row 5: this roster default,
the `0x4b3786` reset value (session 1) and the instant-action default
(`0x45940e`, session 1).

**What is still not measured, and why this is not yet a `Resolved::Known`:**

* where a **stored profile** lives and what it can change. `nPrevPlane`
  (`dword [0x6480c4]`, registered at `0x402128`…`0x402141`) is a *copy* of the
  selection (`0x405d25` writes it from `[0x64b67c]`) and has no other reference
  in the image, so nothing in this pass reads it back; but the six menu writers
  above do change both the selection and the roster, and the file or registry
  the settings at `0x407670` (called with ids `0xd` and `0x3ff`) read from was
  not identified. #715's "the installation holds no profile or hangar file" is
  a statement about the installation only.
* the caller chain of `0x413a8a` — the *other* caller of the setter
  `0x416ea0`, which passes the fixed parameter block `0x642fd0`. That block is
  itself filled from the same source array by `0x411230`
  (`lea esi, [eax*4 + 0x619f58]` → `rep movsd` into `0x642fd0`), with the index
  `[0x64e2a8]`, so its `+0x2c` (`0x642ffc`) is one of `src[0..10]`'s — but
  which function `0x413a8a` belongs to, and when it runs relative to a
  campaign start, was not established in this session. `0x416ea0` has exactly
  two callers in the image (`0x413a8a`, `0x41712c`), so whichever runs last
  wins.

Resolving: identify the settings store behind `0x407670` (F44/F45/F48's
profile shape) and the function containing `0x413a8a`, or #358's owner run.

## 9.2 The start pose's conversion site — located

Session 1 could not find where the `player` record's field 1/field 2 become a
world pose. **It is `0x47c210`.** The spawn loop ends each record with

```
0x47531e   call 0x47c210        ; (model/name, record, 0, extra)
```

and inside `0x47c210`, with `edi = [ebp+0xc]` = the record object:

| address | instruction | meaning |
| --- | --- | --- |
| `0x47c234` | `mov eax, [edi+8]` / `push eax` / `push 7` / `call 0x4d0280` | the record's **name** (`[edi+8]`) — the same field the spawn loop compares against `player` |
| `0x47c2a6` | `lea ecx, [edi+0x18]`; reads `[edi+0x18]`, `[edi+0x1c]`, `[edi+0x20]` into `[ebp-0x44]/-0x40/-0x3c` | **the position vector** — record `+0x18..+0x20`, i.e. aiv field 1 |
| `0x47c4b3`…`0x47c4c0` | pushes that vector and `call 0x4d1d50(vehicle, x, y, z)` | the position setter (`Object3d.c`) |
| `0x47c4e5` | `mov edi, [ebp+0xc]` / `fld dword [edi+0x24]` | **the heading** — record `+0x24`, i.e. aiv field 2 |
| `0x47c4ee` | `fmul qword [0x6040e8]` | **degrees → radians** (the same `pi/180` constant `0x3f91df46a2529983` the zeppelin parse uses) |
| `0x47c4f4`…`0x47c4fa` | `fstp dword [esp]`, `push esi`, `call 0x4d1a30(vehicle, 0, heading_rad, 0)` | **the rotation setter** |

`0x4d1a30` is in `D:\zipper\gamez\zclass\Object3d.c` (its own assertion
string at `0x62d118`; the file's functions are the 34 sites `0x4d126a`…
`0x4d20e3`). It takes `ecx = [node+0x38]` (the class data) and stores

```
a0 -> [class+0x18],  a1 -> [class+0x1c],  a2 -> [class+0x20]
```

so for a plain `aiv` record **only the middle component, `class+0x1c`, is the
player's heading**; the first and last are 0. The vehicle's own position goes
through `0x4d1d50` instead. (The other branch at `0x47c4cc`, taken when
`[esp+0x10] != 0`, passes three floats of some override vector to the same
setter; for M01's player that argument is the literal `0`, so the fallback
with the record's heading runs.)

**What is still missing is the meaning of that triple** — the heading's zero
direction and its handedness:

* `[class+0]` is a flags word (`or eax, 1` / `test dl, 8` around `0x4d1a85`),
  so the class data is not itself a matrix, and no matrix build was found that
  reads it: scanning the image for `fld dword [reg+0x1c]` (46 sites) and
  pairing it with a `mov reg, [reg+0x38]` class-data load in the preceding 96
  bytes with the same register gives **no pairs**, i.e. the consumer keeps the
  pointer across a longer region or fetches it differently;
* `0x4cdb50`…`0x4ce400` is a plain 3×3 matrix multiply (nine floats at
  `+0x00..+0x20` of both operands, results to `[esp+0x10..0x2c]`) — a
  *matrix* product, not the Euler/triple → matrix build;
* `0x4cf830`, called immediately after the setter (`0x4d1ad6`), only propagates
  dirty flags down the node tree (`or` bits, recursive over `[esi+0x5c]`);
* the retail scene has a node literally named `compass` (string `0x627cc4`,
  looked up at `0x476bbb` beside `pfhorizon`/`rtracks`/`ltracks` into
  `[esi+0x4e4]`) — a cockpit/world compass card, and the cheapest remaining
  landmark: whoever rotates it by the aircraft's heading spells out both the
  zero direction and the sign.

Resolving: follow `[esi+0x4e4]` (or the `Object3d` triple → matrix build), or
#358's owner run. **Never an invented yaw** (AGENTS.md rule 4).

## 10. What changed in session 2

* This note only. `recover_retail_start_configuration` still returns
  `Resolved::Unknown` for both the airframe and the initial pose, because the
  pose's zero direction and handedness are unmeasured (rule 5: a source is
  either measured or it is not).
* The acceptance criterion naming
  `crates/cs_app/tests/campaign/vs_m01_runtime.rs` could not be exercised:
  that file exists only on `origin/rally/359-wire-one-original-mission-into-the-playa`
  (#359, blocked on this task), not on `main`. It was not created on `main`
  and #359's branch was not touched.

---

# Session 3 (same claim, same task): the second `0x71daec` setter's owner,
# and the `Object3d` rotation triple's exact world meaning

Same date, same capabilities (`retail` + static analysis of
`$CS_GAME_DIR/crimson.decrypted.exe`, sha256 `43540fc9…`, plus the retail
`ZBD/interp.zbd` / `ZBD/planes.zbd`), same method (strings, radare2 `pd`,
small Python byte scans over the file). **No original run happened; nothing
here is `verified_original`, no production code and no test was changed.**

## 11. The airframe: the *other* setter of `0x71daec` belongs to the
## multiplayer start, and the settings live outside the installation

Session 2 closed with two open items (§9.1). Both are now answered.

### 11.1 `0x413a8a` sits in `0x4136e0`, whose exit starts a mode-2 session

* The function containing `0x413a8a` begins at **`0x4136e0`** (prologue
  `55 8b ec`; `r2`'s `af` splits it, the byte scan does not). It is reached
  through the pointer table that holds `0x4136e0` at `0x60338c`, `0x6033bc`
  and `0x6033cc`; the object that carries that table is written at
  `0x41232a` / `0x412fec` and published at **`0x64e724`** — the very object
  `0x4136e0` itself dereferences at `0x413a6e` (`mov ecx, [0x64e724]`).
* The fall-through path of `0x4136e0` ends at **`0x413bee call 0x496c60`**
  and then returns (`0x413bf3`…`0x413c06`). `0x496c60` is the routine
  session 1 measured as the **mode-2 writer**: `0x496c7e mov dword
  [0x71bb80], 2` immediately before `0x496c88 call 0x4638f0` (the chapter /
  mission script loader), i.e. the multiplayer start.
* Consequence: **`0x416ea0`'s two callers are on different start paths.**
  A campaign launch (mode 1, function `0x417114`, setter `0x41712c` with
  `&roster[[0x64b67c]]`) does *not* also run `0x413a8a` (the fixed parameter
  block `0x642fd0`), because the only other thing `0x4136e0` does on that
  path is start a mode-2 session. Session 2's "whichever runs last wins"
  caveat therefore does not apply to a campaign start: **on the measured
  campaign path the roster is the only writer of `0x71daec`.**

### 11.2 `0x407670` is a message dispatcher, and the settings store is the
### registry / an INI, i.e. outside the installation

* `0x407670` opens `sub esp, 0x404`, reads an id from `esi`, bounds-checks it
  against `0x3e8`, then `jmp dword [ecx*4 + 0x408368]` — a **jump-table
  message/event dispatcher**, not a settings reader. Session 1's phrase "the
  settings at `0x407670`" was imprecise; it is a dispatch on an id (its
  callers pass ids such as `0xd` and `0x3ff`).
* The installation does have an off-install settings store, found as import
  and string evidence in the image: **`SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\1.0`**,
  **`SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\Special`**,
  `RegOpenKeyExA`, `GetPrivateProfileStringA`, `CRIMSON_SKIES_SAVELOAD_VERSION`
  and `D:\zipper\Crimson\config.cpp`. #715's "the installation holds no
  profile or hangar file" is thus a statement about the *files* only: a
  player's persisted selection can live in the registry or an INI where no
  retail file can show it, and this installation (a file copy on a
  non-Windows host) cannot exhibit it either way.
* What this leaves for the airframe: **on a fresh, profile-less launch into a
  campaign mission the chain of §9.1 closes at row 5 `Devastator`**, with
  three independent engine defaults agreeing (roster init, `0x4b3786` reset,
  instant-action `0x45940e`), and the campaign path now shown to be the only
  writer (§11.1). What can still differ is a player whose hangar selection or
  persisted registry/INI value differs — that is the profile/flight-check
  shape, and it names the affected content rather than an unknown in the
  chain.

## 12. The start pose: the `Object3d` rotation triple and its world matrix

### 12.1 The triple is `(pitch, yaw, roll)` in radians, and it is `SetRotation`

* `0x4d1a30` (`Object3d.c`) stores its three `float` arguments into
  `class+0x18`, `class+0x1c`, `class+0x20`; **`0x4d1b40`** reads the same
  three slots back in the same order (`GetRotation(node, &a, &b, &c)`), and
  `0x4d1d50` stores the *position* at `class+0x54/0x58/0x5c`. The aiv spawn
  calls it as `0x4d1a30(node, 0, heading_rad, 0)` (`0x47c4fa`), so the
  stored heading lands in **`class+0x1c`** and the triple is `(0, ψ, 0)`.
* Independent corroboration from content: `ZBD/interp.zbd` carries the script
  `FindNode cargozep1` … `Object3DRotate -0.000010 -3.144009 -0.000000` —
  the same three-component rotation, **in radians** (`−3.144009 ≈ −π`), next
  to `Object3DTranslate x y z` in world coordinates.

### 12.2 `0x53bf40` builds the world matrix from the triple; it decodes to
### `M = Ry(r1) · Rx(r0) · Rz(r2)` with the **right-handed** matrices

The HUD object reads the vehicle node's triple with `GetRotation` into
`+0x1f8/+0x1fc/+0x200` (`0x4762a6`) and rebuilds a 3×3 at its `+0x180` with
**`0x53bf40(r0, r1, r2, out)`** (`0x47631e`). Decoding `0x53bf40`'s nine
stores (with `A = r0`, `B = r1`, `C = r2`):

```
M0 = sinB*sinA*sinC + cosC*cosB   M1 = sinC*cosA   M2 = sinC*cosB*sinA - cosC*sinB
M3 = sinB*cosC*sinA - sinC*cosB   M4 = cosC*cosA   M5 = sinC*sinB + cosC*cosB*sinA
M6 = sinB*cosA                    M7 = -sinA       M8 = cosB*cosA
```

and three zeros at `M9..M11`. Read as **columns** (`M0..M2` = image of local
`+X`, `M3..M5` = `+Y`, `M6..M8` = `+Z`) this is exactly
`M = Ry(r1)·Rx(r0)·Rz(r2)` built from the standard **right-handed** rotation
matrices (`Ry(θ)(1,0,0) = (cosθ, 0, −sinθ)`, `Rx(θ)(0,1,0) = (0, cosθ, sinθ)`,
`Rz(θ)` usual).

Three independent cross-checks, all measured:

1. **The inverse.** `0x53df30` (matrix → Euler) reads the same nine floats and
   answers `pitch = asin(−M7)`, `yaw = atan2(M6, M8)`,
   `roll = atan2(M1, M4)` — algebraically exactly `r0`, `r1`, `r2` for the
   matrix above. `0x53def0` is the one-component form `atan2(M6, M8)`.
2. **The helpers.** `0x53e160` decodes to `out = (v0, c·v1 − s·v2, s·v1 + c·v2)`
   (right-handed `Rx`) and `0x53e1e0` to `out = (c·v0 + s·v2, v1, c·v2 − s·v0)`
   (right-handed `Ry`) — the same two factors `0x53bf40` composes.
3. **The HUD compass** (`0x49f8ec`…`0x49f90d`): the node found by the retail
   scene lookup `FindNode compass` (string `0x627cc4`, stored at
   `hud+0x4e4` in `0x476bc9`) is given
   `SetRotation(compass, 0, −yaw, 0)` where `yaw = atan2(M6, M8)` of the
   vehicle matrix — **the compass card counter-rotates against the vehicle's
   yaw**, which is the behaviour landmark #436 recorded as missing, and it
   only works if `class+0x1c` is read back as that yaw, i.e. if the triple is
   `(pitch, yaw, roll)` composed as above.

### 12.3 What this does and does not fix

**Fixed (measured):**

* the composition and its handedness: `ψ = 0` ⇒ `M = I`, so at heading 0 the
  airframe node's **local axes are the world axes** (`+X = +X`, `+Y = up`,
  `+Z = +Z` in #436's right-handed, identity-mapped, metre frame);
* positive `ψ` rotates the node **right-handed about `+Y`**, taking local
  `+Z` toward `+X` (and local `−Z` toward `−X`);
* the conversion itself: stored degrees × `π/180` (`0x47c4ee`, the same
  `qword [0x6040e8]` as every other `.zrd` angle) is *the* value the original
  puts in `class+0x1c`.

**Still open — the airframe model's own nose axis.** Which *local* axis the
`player_<x>` scene root points along (so that "heading 0 = world …" can be
spelled as a world direction) is a property of `support\planes.gw` /
`support\util\planesurgery.gw` and the `common\planes\<model>\<model>.flt`
geometry, not of the heading convention, and it is **not** measured here.
Two independent pieces of engine evidence point at **local `−Z`**, but neither
is yet tied to the spawned aiv record end to end:

* the flight module's motion integrator `0x470550` (called from the actor
  update `0x4bf9d0` at `0x4bf9fd`) advances an actor's world position along
  **`(−sinψ·cosφ, sinφ, −cosψ·cosφ)`** where `ψ = actor+0x2c`, `φ = actor+0x30`,
  and the same module's `0x4bf950` writes exactly that pair back into the node
  as `SetRotation(node, φ, ψ, 0)` — so *for every object that module drives*,
  world motion at `ψ = 0` is `(0, 0, −1)`;
* `0x48a3e1`/`0x48a3f5` build a direction by rotating the literal base vector
  **`(0, 0, −1)`** with `0x53e160` (pitch) and then `0x53e1e0` (yaw);
* against that, `0x53de20` (world direction → `(pitch, yaw, 0)`, with
  `yaw = atan2(dx, dz)`) is what *aligns local `+Z`* with a direction, and it
  is used that way for props/cameras.

What is missing between them: **the store that puts the aiv record's heading
into the flight actor's `+0x2c`** (the spawn allocates the `0xa20`-byte
aircraft object at `0x47c502`, sets `+0xc8/+0xcc/+0xd0/+0xd4/+0x210/+0x21c`,
but no `… [reg+0x2c]` store fed by a `… [reg+0x24]` read exists anywhere in
the image, and `GetRotation`'s eleven callers do not include the spawn). Until
that link or the `.flt`/`planes.gw` nose axis is measured, "heading 0 faces
world −Z" is a **candidate with two measured supports, not a measurement**,
and §10's verdict stands unchanged.

## 13. What changed in session 3

* This note only. `recover_retail_start_configuration` still returns
  `Resolved::Unknown` for both the airframe and the initial pose: the
  airframe's remaining question is now only the profile/flight-check shape
  (§11), but the pose still has no measured nose axis (§12.3), so binding
  either would be a guess between candidates (AGENTS.md rules 4 and 5).
* The acceptance criterion naming
  `crates/cs_app/tests/campaign/vs_m01_runtime.rs` still could not be
  exercised: the file exists only on
  `origin/rally/359-wire-one-original-mission-into-the-playa` (#359, blocked
  on this task), not on `main`. It was not created on `main` and #359's
  branch was not touched.

---

# Session 4 (same claim, same task): both values bound, with their sources

Same date, same capabilities (`retail` + static analysis of
`$CS_GAME_DIR/crimson.decrypted.exe`, sha256 `43540fc9…`, plus retail
`ZBD/*.zbd`), same method. **No original run happened; nothing here is
`verified_original`.** This session changed production code and tests for the
first time in this task.

## 14. What is bound, what it means, and what is still named

### 14.1 The airframe: the profile/flight-check shape, at its measured default

`recover_retail_start_configuration` now answers
`Resolved::Known` for M01's player airframe, and
`MissionStartConfiguration::read` (the document alone) still answers the
refusal of §6.1 — the split is deliberate: the value does not live in
`aiv.zrd`, and a document-only read has no source to name.

* **Value**: [`CAMPAIGN_AIRFRAME_ROW`] = `5` → row 5 of `AIRFRAME_TABLE`
  (`Devastator` / `player_pfighter` / `piratefighter`), bound as
  `ContentId` kind `airframe` with the **scene root** as its key — the node
  the original's own scripts look up (`FindNode %player_plane%`, and the
  cockpit table that pairs `player_plane` with each root in
  `ZBD/interp.zbd`).
* **Source, named**: [`CAMPAIGN_AIRFRAME_SOURCE`] spells the chain
  (`0x4113b0` roster init → `0x411477` selection `= 0` → `0x411579` copy from
  `.data` `0x61a81c` → campaign start `0x417114`/`0x41712c` → spawn
  `0x474d48`) and calls it what it is: **the profile/flight-check shape**.
  The provenance span is the deciding byte itself —
  [`CAMPAIGN_AIRFRAME_RECORD_OFFSET`] = `0x21a81c`, `+0x2c`, 204 bytes —
  read out of this installation's own inventory by
  `engine_state_source`, which refuses an image that is absent or that
  hashes to anything else.
* **Verification, not transcription**: the retail acceptance test re-reads
  that `u32` out of the image file and compares it with
  `CAMPAIGN_AIRFRAME_ROW`, so deleting or changing the binding breaks the
  test against original bytes rather than against the constant that was
  transcribed from them.
* **Residues, named**: a player's hangar selection and the registry/INI
  profile (§11.2) select another row by design, and the mission language that
  may still assign one is undecoded (F13-B/C, F38). Neither sits *in* the
  chain: the chain has one writer on the campaign path (§11.1) and three
  agreeing defaults (§9.1). Nothing is `verified_original`.

### 14.2 The pose: the record's value through the measured convention

`initial_pose` is now `Resolved::Known` = position as stored ×
`STORED_POSITION_METRES_PER_UNIT`, heading as stored through
`stored_heading_radians`.

* **Value**: `[-3694, 1318, -12482]` metres and `170 × 0.01745329251994`
  radians for M01 — the record's own two fields (#676), never an invented
  yaw.
* **Conversion, exact**: `STORED_HEADING_DEGREES_TO_RADIANS` is the image's
  **double** `0.01745329251994` at VA `0x6040e8` (file offset
  `0x2040e8`): fourteen significant digits of π/180, *not* the double nearest
  π/180. `stored_heading_radians` widens, multiplies and rounds exactly as
  `0x47c4e5`..`0x47c500` does, so the bound heading is the value the original
  hands `SetRotation`, not an `f32::to_radians` approximation of it. The
  retail test reads the eight bytes back out of the image and compares them
  with the constant.
* **Source, named**: the provenance span is
  [`HEADING_CONVERSION_OFFSET`] = `0x7c4e5`, the `fld`/`fmul`/`call`
  sequence itself.
* **What "heading" now means, precisely**: the yaw of the airframe **node**,
  composed as §12.2 measured — `M = Ry(yaw)·Rx(pitch)·Rz(roll)`,
  right-handed, `yaw = 0` ⇒ `M = I` (the node's local axes *are* the world
  axes in #436's identity-mapped metre frame), positive yaw turning local
  `+Z` toward `+X`. The behaviour landmark §12.2 found (the compass card
  given `−yaw`) fixes the sign, and §12.1 fixes that this value is what the
  original stores.
* **What this binding does *not* claim**: which local axis the *model's nose*
  lies along (§12.3) — that is a property of the unparsed `.flt` geometry and
  of `support\planes.gw`, and it decides how one spells "the world direction
  the aircraft faces at yaw 0", not the pose: the pose is the node's
  transform, which §12.2 fully determines. Session 3 treated the missing nose
  axis as blocking the *whole* pose; on reflection the blocking question was
  only the prose gloss, so the pose is bound and the nose axis stays a named
  residue below. A reviewer who judges the gloss part of the claim should
  send `initial_pose` back to `Resolved::Unknown` and the residue resolves
  through a `.flt`/`planes.gw` nose-axis measurement.
* **New supporting observation, not yet a measurement**: the routine at
  `0x48a110` — first argument carrying the spawned object's own fields
  (`+0xc8`, `+0xd0`, `+0xd4`, `+0x940`, matching `0x47c502`'s allocation) —
  builds a direction as `Ry(a)·Rx(b)·(0,0,-1)` from the literal `(0,0,-1)`
  (`0x48a3cc`), i.e. the same composition §12.2 measured with base `−Z`
  instead of `+Z`. What `a` and `b` are in that routine (one of them is read
  from `+0x220`, inside the triple the spawn writes at `+0x21c`) is **not**
  established, so this is a lead for the nose-axis question, not evidence for
  it.

### 14.3 Residues, affected content and resolving work

| Residue | Affected content | Resolves in |
| --- | --- | --- |
| the player's own hangar / registry/INI plane selection | every campaign mission's player airframe | F45/F48 (profile and flight-check shape), or #358's owner capture |
| the undecoded mission language | any mission statement that may reassign an airframe or move the player before launch | F13-B/C, F38 |
| the airframe model's nose axis (`.flt` geometry unparsed) | the *wording* "yaw 0 faces world …" for every airframe; not the pose value | a `.flt`/`support\planes.gw` nose-axis measurement |
| the frame relation of a stored start to the world node grid (M01's player start lies 194 stored units, its wingmen 777 and 826, outside `c1c`'s `[-12288, 0]²` node bounds) | the meaning of the start position relative to the tiles, not its unit | #676's frame-relation follow-up |

### 14.4 What changed in session 4

* `crates/cs_app/src/mission_start.rs` — the binding: `ENGINE_IMAGE`,
  `ENGINE_IMAGE_SHA256` (reusing F16-E's `cs_content::coordinates` digest so
  the two static-analysis surfaces cannot drift), `CAMPAIGN_AIRFRAME_ROW`,
  `CAMPAIGN_AIRFRAME_RECORD_{OFFSET,LENGTH}`,
  `HEADING_CONVERSION_{OFFSET,LENGTH}`,
  `HEADING_DEGREES_CONSTANT_{OFFSET,LENGTH}`,
  `STORED_HEADING_DEGREES_TO_RADIANS`, `stored_heading_radians`,
  `CAMPAIGN_AIRFRAME_SOURCE`, `EngineStateError`, `EngineStateSource`,
  `engine_state_source`, `MissionStartConfiguration::bind_engine_state` and
  `::refuse_engine_state`; `recover_retail_start_configuration` wires them.
  The two refusal constants were reworded to say what is still true of a
  *document-only* read (every substring the #715 tests pin is kept).
* `crates/cs_app/tests/campaign/m01_lc_campaign_airframe_pose.rs` (new,
  registered in `main.rs`) — `accept_m01_lc_campaign_airframe_pose_*`: the
  source span against a synthetic inventory (present / absent / wrong
  digest), the bind against a synthetic document (and the record-without-pose
  case), the conversion against the constant, and the retail end-to-end test
  that re-reads both byte ranges out of the image.
* `crates/cs_app/tests/campaign/m01_lc_player_config.rs` and
  `…/m01_lc_player_airframe_source.rs` — their retail members now assert
  `Resolved::Known` with the measured values instead of the refusals (their
  synthetic members still assert the refusals, which `read` still returns).
* One measured exception, added after the first green run: an archive that
  carries **both** `aiv.zrd` and an `ia.zrd` with `player_plane` — the eight
  `IA1` archives, no campaign one (#715) — keeps its airframe refused under
  that scenario's own assignment, because mode 3 reads the key (`0x4593e5`)
  and the campaign chain is not the chain that decides. Without it,
  `recover_retail_start_configuration(…, "zbd/c1c/ia1")` would have answered
  row 5 while the scenario says `Fury`. The pose is bound either way: the
  record's pose takes the same conversion whatever the mode. Pinned by
  `accept_m01_lc_campaign_airframe_pose_an_instant_action_scenario_keeps_its_own_assignment`.
* `crates/cs_app/tests/campaign/m01_lc_campaign_airframe_pose_evidence.rs`
  (registered in `main.rs`) — this task's evidence harness, the same kind of
  member as `f39_e3_evidence.rs`, deliberately linked into the campaign
  binary rather than starting a test binary of its own (the CI runner-disk
  finding `docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md`). It
  re-reads both byte ranges out of the image, re-runs the production
  bindings, and writes `acceptance.json` for
  `docs/findings/evidence/M01-LC-CAMPAIGN-AIRFRAME-POSE.json`.
* The acceptance item naming
  `crates/cs_app/tests/campaign/vs_m01_runtime.rs`
  (`accept_vs_m01_runtime_retail_launch_is_refused_with_source_diagnostics`
  and its retail closure test) **still could not be exercised**: re-checked
  this session with `git ls-tree origin/main` and
  `git ls-tree -r origin/main | grep vs_m01_runtime` — the file is on neither
  `main` nor this branch; it exists only on
  `origin/rally/359-wire-one-original-mission-into-the-playa` (#359, blocked
  on this task). It was not created here and #359's branch was not touched.

### 14.5 Checks

```text
cargo fmt --all -- --check                                            -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings  -> 0
cargo test --workspace --locked                                       -> 0 failures
cargo test --locked -p cs_app --test campaign accept_m01_lc_ -- --include-ignored
                                                                      -> 15 passed, 0 failed
```

(The family count is 15 on the final tree: the instant-action guard test of
§14.4 was added after this block was first written. Nothing in this session
is `verified_original` and no owner approval is claimed; the bindings are
`observed_tool`.)

### 14.6 Independent review (bunny-alpha-1, 2026-10-08)

Review ran from a fresh context under a different agent name than the
implementer (`bunny-alpha-1` vs `bunny-2`), on the branch rebased onto
`origin/main` (rebase clean, no `Cargo.toml`/`Cargo.lock` in the incoming
commits, no file overlap).

**The two deciding values were re-derived by the reviewer, not taken from the
constants.** `$CS_GAME_DIR/crimson.decrypted.exe` hashes to
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`
(= `cs_content::coordinates::ORIGINAL_IMAGE_SHA256`); the `u32` at file offset
`0x21a81c + 0x2c` reads `5` and the eight bytes at `0x2040e8` read
`0.01745329251994`, i.e. `CAMPAIGN_AIRFRAME_ROW` and
`STORED_HEADING_DEGREES_TO_RADIANS` out of the bytes themselves.

**The two code landmarks were re-disassembled.** At `0x47c4e5`: `mov edi,
[ebp+0xc]`, `fld dword [edi+0x24]` (`0x47c4e8`), `fmul qword [0x6040e8]`
(`0x47c4ee`), `fstp dword [esp]`, then `call 0x4d1a30` (`0x47c4fa`) with the
stack arguments `(esi, 0, yaw, 0)` — the `(node, 0, yaw, 0)` the binding
documents. At `0x49f8ec..0x49f90d`: `mov eax, [esi+0x4e4]` (the compass node
of §12.2), `fchs` on the yaw, and the same `0x4d1a30` called on it — the
counter-rotation landmark that fixes the sign. Both decode as the finding
says; no claim in §14.1/§14.2 was found to exceed its evidence, and every
residue of §14.3 is named rather than folded into a value.

**Checks rerun by the reviewer** (repository root, `CS_GAME_DIR` set):

```text
cargo fmt --all -- --check                                            -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings  -> 0
cargo test --workspace --locked                                       -> 0 failures
cargo test --workspace --locked -- accept_m01_lc_campaign_airframe_pose_ --include-ignored
                                                                      -> 6 passed, 0 failed
cargo test --locked -p cs_app --test campaign accept_m01_lc_ -- --include-ignored
                                                                      -> 15 passed, 0 failed
python3 tools/validate_evidence.py private/evidence/M01-LC-CAMPAIGN-AIRFRAME-POSE/acceptance.json \
  --artifact-root private/evidence/M01-LC-CAMPAIGN-AIRFRAME-POSE --require-pass
                                                                      -> structurally valid, 2 artifacts
```

The evidence report was regenerated by the reviewer on that rebased tree
(`candidate_tree` `64f810a3`) and compared field by field with the
implementer's copy: identical in `source`, `tests`, `capabilities`, `claim`,
`engine` and `unknowns`, differing only in the tree, timestamps, cwd,
reviewer identity, artifact digests and the order the parallel tests
finished. The copy that reaches this document is the reviewer's run.

**Still not exercised:** the acceptance item naming
`crates/cs_app/tests/campaign/vs_m01_runtime.rs` (§14.4, last bullet) — the
file exists only on #359's branch, whose task is blocked *on this one*, so it
cannot be updated or run from this branch. Recorded as an unmet criterion in
the review handover and handed to #359, whose own acceptance requires its
closure tests to be re-derived against these `Resolved::Known` bindings when
it rebases.

Nothing in this review is `verified_original`: no original executable ran,
and no agent review replaces the owner's human approval.
