# F49-D.03-FU-RULES-ROSTER (#1639): which aircraft each Instant Action mission type spawns

Task #1639, branch `rally/1639-measure-which-aircraft-each-original-ia`.
Follow-up of #1583 (F49-D.03-FU-RULES): measures, per `mission_type`, which of
the aircraft an `ia.zrd` declares the original's spawn routine actually seats,
what the wingmen fly, and re-verifies `ZBD/c2b/IA1`.

## Evidence class

**`static`** — static reading of the owner-supplied decrypted engine image
(`$CS_ENGINE_IMAGE`, sha-256 `43540fc9…6c37d75`, load base `0x400000`) and of
the retail `ZBD/*/IA*/zrdr.zbd#ia.zrd` and `ZBD/zrdr.zbd#vehicle.zrd` documents.
No original executable ran; nothing here is `verified_original`. Claim id
`f49.d.03.ia-spawn-roster`.

## The routine

`0x45a390` (called from `0x47f3f1`) seats every Instant Action aircraft except
the player's own, from the scenario record `0x718cd8` that `0x459390` filled.
It reads the live type `[0x718cd8]` (`0x45a3ab`) and the type's spawn-point
vector (`0x718fe0 + 20*type`, 16-byte points). Record fields it uses:

| record | VA | field |
| --- | --- | --- |
| `+0` | `0x718cd8` | mission type index (`0` ace, `1` squadron, `2` zeppelin_run, `3` ground_target, `4` stunt_flying; `0x458c20`) |
| `+4` | `0x718cdc` | `player_plane` row |
| `+8` | `0x718ce0` | `num_wingmen` (clamped to ≤5, `0x459429`) |
| `+0xc` | `0x718ce4` | `wingman_plane` row |
| `+0x64` | `0x718d3c` | `ace_plane` row |
| `+0xd4 + 0x60*n` | `0x718dac…` | four group records: count `+0` (`num_enemies`, clamped ≤6, `0x458e27`), plane row `+0x14` (`enemy_plane`) |

Every lookup below goes through the registry `[0x71daac]`, the `vehicle.zrd`
record list the aiv spawn path also searches (`docs/findings/2026-10-15-m01-aiv-layout.md`
§3): the entry whose name equals the lookup name (`_stricmp`, `[0xa20214]`) is
the template, the record name given the aircraft is `"%s_ia%d"`, and a miss
skips the aircraft silently.

## Wingmen (`0x45a4b9`..`0x45a710`, loop `0x45a4a5`..`0x45aa14`)

For `i < [0x718ce0]` the routine looks `[0x718ce4]` (`wingman_plane`) up through
two row accessors before the type switch: `0x426d60` (`mov eax,[ecx*4+0x620c80]`,
the row's **`w`** pointer, `+0x10`) into `edi` (`0x45a4d9`) and `0x426d20`
(`[…+0x620c88]`, the row's **`base`** pointer, `+0x18`) into `esi` (`0x45a4e0`).
The type switch `0x45a4f2` (`jmp [edx*4+0x45b9b0]`, cases `0x45a4fb`,
`0x45a55e`, `0x45a5c1`, `0x45a63b`, `0x45a69b`) moves one of them into the lookup
register `ebx` as the case's second instruction:

| type | case | lookup name | `ebx ←` | mode code written |
| --- | --- | --- | --- | --- |
| 0 `dogfight_ace` | `0x45a4fb` | `w` | `edi` (`0x45a500`) | `0xc` |
| 1 `dogfight_squadron` | `0x45a55e` | `base` | `esi` (`0x45a563`) | `0xe` |
| 2 `zeppelin_run` | `0x45a5c1` | `w` | `edi` (`0x45a5c6`) | `0xf` |
| 3 `ground_target` | `0x45a63b` | `base` | `esi` (`0x45a640`) | `0xd` |
| 4 `stunt_flying` | `0x45a69b` | `w` | `edi` (`0x45a6a0`) | `0x10` |

The mode codes are measured stores whose meaning is **unmeasured**.

Data (`vehicle.zrd`, production decode): the registry holds the `w` record of
all eleven airframe rows (`wingman`, `wautogyro`, `wavenger`, `wbalmoral`,
`wbloodhawk`, `wbrigand`, `wfirebrand`, `wfury`, `wkestrel`, `wpeacemaker`,
`wwarhawk`), every one `mode: wingman` (the follower of the player,
`docs/findings/2026-10-18-m01-wingman-player-leader.md` §1.5), and the `base`
record of all eleven (`autogyro`, `avenger`, `balmoral`, `bloodhawk`, `brigand`,
`devastator`, `firebrand`, `fury`, `kestrel`, `peacemaker`, `warhawk`), none with
a `mode`. So no wingman lookup misses, and:

* types 0, 2, 4 seat the follower record; types 1 and 3 seat the plain record
  an enemy group of that airframe also uses.

### `wingman_plane`

The reader stores the airframe-table row of `wingman_plane` (key `0x625588`,
`0x45943f`..`0x459468`: `0x426d80` name→row, `cmp eax, 0xb`, `mov [ebx+0xc],
eax`; the sentinel 11 becomes 5 at `0x459468`); an absent key skips the store
(`test eax,eax; je 0x45946b`, `0x45944d`) and leaves the initialiser's
`mov [ebp+0xc], esi` (`esi = 5`, `0x459001`/`0x459012`) in `0x458ff0`.
**None of the eight descriptors declares `wingman_plane`** (production survey,
and an independent search of every `ia.zrd` member for the key text), so every
wingman is airframe row 5, **Devastator**, whatever the preset — seated as the
`wingman` record under types 0/2/4 and the `devastator` record under 1/3.

`num_wingmen` itself is a separate fact: three in seven descriptors, absent in
`ZBD/c2b/IA1` (below). The reader forces the stored value to 0 when the record's
type is `0` at read time (`0x459433`, `cmp [ebx],0; jne`): the type there is the
descriptor's own `mission_type` (read at `0x4593d7`, before the count). All eight
retail descriptors declare a non-ace default type, so none is zeroed.

## The named ace (`0x45aa24`..`0x45ad18`)

`0x45aa24`: `mov eax,[0x718cd8]; test eax,eax; jne 0x45ad18`. The ace block —
lookup name `0x426d20([0x718d3c])` (the `base` record of `ace_plane`; default
row 5, `0x459082`; needs ≥2 spawn points, `0x45aa3c`..`0x45aa47`) — runs **only
for type 0, `dogfight_ace`**. The other four types skip it, so the descriptor's
`ace_*` keys are inert there.

## The enemy groups (`0x45ad18`..`0x45b17c`)

Entry (`0x45ad36`: `cmp [0x718cd8], 2; jne 0x45b17c`) is reached when the
type's spawn-point vector holds ≥2 points (`0x45ad22`..`0x45ad34`) — every
offered retail type has four or eight — or when the type is 2. There is **no
other type test on which groups spawn**: the loop `0x45ad99`..`0x45b176` visits
the four records (`0x718dcc`, step `0x60`, end `0x718f4c`) and, per record,

* skips it when the count `[ebp-0x20]` is 0 (`0x45ad9e`, which also skips the
  placed-group counter `inc [esp+0x10]` at `0x45b169`);
* looks up `0x426d20([ebp-0xc])`, the `base` record of the group's
  `enemy_plane` (record default row 6, `0x458dd6`; an unknown name becomes 5,
  `0x458f26`), and skips the whole group on a registry miss (`0x45ae0b`);
* seats `count` aircraft `"%s_ia%d"`.

Placement differs by type: the first **seated** group of a type other than 2
takes a random spawn point of the type with a formation offset and sets the wave
counter `[0x718cd0] = 1` (`0x45b0cb`); every other group, and every group of
`zeppelin_run` (`0x45b001`/`0x45b018`, `0x45b01e`/`0x45b022` → `0x45b0d3`), is
seated at the default placement vector `0x75d1b8`. This is the dormant-wave
structure `0x45b9d0` consumes (`docs/findings/2026-10-19-f49-d-03-fu-rules-end.md`).
All eleven `base` records exist in `vehicle.zrd`, so no retail group or ace
lookup misses.

Types 3 and 4 add a block each after the groups (`0x45b17c` ground target,
`0x45b4b2` stunt gates and markers) that work on other records (`0x71dab8`,
`0x71d910`, the vector `[0x718f58]`) and do no aircraft lookup through `[0x71daac]`.
The tail `0x45b893` also branches on type 2 (the zeppelin objects); it was not
traced further here.

## Per mission type: the aircraft seated

| type | player | wingmen | enemy groups | ace |
| --- | --- | --- | --- | --- |
| `dogfight_ace` | `player_plane` | `num_wingmen` × row 5, `w` record | every declared group | seated |
| `dogfight_squadron` | `player_plane` | `num_wingmen` × row 5, `base` record | every declared group | **not seated** |
| `zeppelin_run` | `player_plane` | `num_wingmen` × row 5, `w` record | every declared group, all at the default placement | **not seated** |
| `stunt_flying` | `player_plane` | `num_wingmen` × row 5, `w` record | every declared group | **not seated** |
| `ground_target` (offered by no retail directory) | `player_plane` | `num_wingmen` × row 5, `base` record | every declared group | **not seated** |

Groups are seated with their authored count (≤6 each); the earlier finding's
conclusion that the totals exceed the per-side bound is a property of the
declared data, not of what is seated per side (which side each aircraft is on
stays unmeasured, #1582).

## `ZBD/c2b/IA1` re-verified

`player_plane` and `num_wingmen` are both absent. `0x4593f3` (`test eax,eax; je
0x459411`) and `0x459421` (`je 0x45943f`) skip the stores, so the initialiser's
values stand: **`player_plane` = row 5 (Devastator), `num_wingmen` = 0**, as
F49-D.02b resolved (`2026-10-18-f49-d-02b-ia-scenario-defaults.md`); its
`wingman_plane` is also absent, row 5, but with no wingmen nothing is seated.
The F49-D.01 / F49-D.03 skeleton entries "the player airframe and wingman count
of c2b's scenario" are therefore **resolved**, not open; the open question F49-D.02b
left on `wingman_plane` is closed here.

## What changed in the code

`cs_content::instant_action`: `IaDirectory::wingman_plane()` (new
`IaWingmanPlane`, read from the `wingman_plane` key), `ia_spawns_named_ace`,
`ia_wingman_template` (`IaWingmanTemplate`), the engine-default constants
`IA_ENGINE_DEFAULT_WINGMAN_PLANE_INDEX` / `_GROUP_PLANE_INDEX` / `_ACE_PLANE_INDEX`
and the claim `IA_SPAWN_ROSTER_CLAIM`. `cs_app::ui::instant_action::retail`:
`directory_roster` takes the mission type; wingmen carry row 5 (provenance
`f49.d.02b.ia-scenario-defaults`, `inferred`, as for c2b's player) under role
`wingman` or `wingman_base`; the ace is seated for `dogfight_ace` only. The
`IA_WINGMAN_PLANE_UNMEASURED_CLAIM` and its reason are gone: the wingman plane is
measured. The existing retail roster assertions in
`accept_f49_d_03_retail_catalog` became type-aware (ace for `dogfight_ace` only,
wingmen on row 5); none was weakened.

## Unmeasured, recorded

* **Whether the menu's type survives the load.** The menu writes `[0x718cd8]`
  (`0x417554`..`0x417578`, types 0, 1, 4, 2) *before* `0x4638f0` stores the
  mission parameters; the load later runs `0x45a150` (from `0x466bd5`), which
  re-initialises the record (`0x45a120`, type 0) and re-reads the descriptor's
  own `mission_type` (`0x4593d7`). No other writer of `[0x718cd8]` was found
  (the only stores are `0x417554`..`0x417578`; the other `mov ecx,0x718cd8` sites
  are the static constructor and destructor `0x458bd0`/`0x458bf0` and the setup
  call itself). Whether the menu's choice is re-applied after the load through
  a path this search did not trace, or the descriptor's default type is what
  actually runs, is **not resolved**. Every row above is keyed on the type value
  `0x45a390` reads; the preset catalog keeps the menu's reading (one preset per
  offered type). This also decides whether the `num_wingmen` zeroing above can
  ever fire for a preset.
* The meaning of the per-type mode codes `0xc`/`0xe`/`0xf`/`0xd`/`0x10`, the
  side (faction) every seated aircraft is on, and what the default placement
  `0x75d1b8` holds at run time.
* The rows for a group without an `enemy_plane` (row 6), an ace without
  `ace_plane` (row 5) and a name the table lacks (row 5) are read from the image
  but exercised by no retail descriptor; the catalog still refuses an unknown name
  rather than applying them, and a group without a plane stays unknown.

Evidence: `docs/findings/evidence/F49-D.03-FU-RULES-ROSTER.json`. Sources: the
decrypted image at the VAs above; the installation's eight IA `zrdr.zbd`
(`ia.zrd`) and `ZBD/zrdr.zbd` (`vehicle.zrd`) read through the production
survey and decoder.
