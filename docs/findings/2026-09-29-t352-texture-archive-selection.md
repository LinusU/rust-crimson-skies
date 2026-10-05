# Task #352: which texture archive a world uses

> **Superseded 2026-10-05 by
> `docs/findings/2026-10-05-t352-texture-archive-selection-rule.md`**, which
> records the rule measured by the owner's static analysis of the decrypted
> executable and implements it. This file is kept as the retail census that
> preceded it: the archive structure, the tier table, the `TextureMemory_*`
> settings and the disjointness of the two namespaces all still hold, and its
> "What was tried" and "What remains unknown" sections are answered there. Read
> its **"Outcome"** paragraph as a record of what the installation's data alone
> can show, not as the state of this task: the mapping lives in native code,
> which the data cannot reveal but a disassembly of the owner's decrypted image
> can.

Date: 2026-09-29. Task #352 (`F08-C-texture-archive-selection`), a follow-up
to F08-C (`docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md`,
"Recorded unknowns"). Test prefix: `accept_f08_c_selection_`. Capabilities
used: `retail` (read-only, `$CS_GAME_DIR`). Implementer: claude-1 (Claude
Opus 5.5).

**Outcome: no selection rule is established.** The installation's data shows
that the rule is a **texture-memory detail setting**, and it shows how the
archives are structured. It does not show how that setting maps to a file
name, whether `texture.zbd` is ever loaded, or in what order a name is
looked up. The mapping lives in native code that the data cannot reveal
(see "What was tried"). No rule is adopted, `crates/cs_content/src/textures.rs`
is unchanged and no `accept_f08_c_selection_` test exists. The task is
blocked on an original-run observation.

Nothing from the installation is committed except paths, sizes, hashes,
offsets, counts and identifier names. The survey scripts were ad hoc Python
over the documented layouts. Their outputs, and the shell scripts extracted
for reading, stay in `private/t352/` (ignored by Git).

## Sources

Installation as fingerprinted on T340/T343 (install fingerprint
`b4e780ab…1978`).

| File | SHA-256 |
| --- | --- |
| `ZBD/zrdr.zbd` | `76b510d821edd2268040d2ccb18c462ec07ad580cdba571b3066228e2cf592dd` |
| `ZBD/interp.zbd` | `f5251cb559db1992320247b9674d159a149572e077bc8579ae34d5fbd16254c7` |
| `ZBD/rimage.zbd` | `fc5f07385b72297e1de0ff05a07188cd0e09ae8609c3f38a7aedee1e2d4e1f13` |
| `GOSDATA/ASSETS/crimson.rof` | `acc9946874110e9741183384010bc02fd48923ae3d608fcf02a96498c3731174` |
| `crimson.icd` | `0e3b4724f045e0bedf7203cd40cdeb5b6e0b9a0bab78c3d04c278cb146e9833b` |
| `crimson.exe` | `f92e09e9c907d65a7da3df4e10243571d77eadb7bd3360f26577be12c8b7de5b` |
| `strings.dll` | `7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21` |

The layouts used are the version-one member index (T343), the reader member
tree (typed nodes: 1 int, 2 float, 3 length-prefixed string, 4 list whose
count includes itself; this is mech3ax S02's reader layout, and every byte of
the member below was consumed), the INTERP layout (FORMAT-NOTES), the ZBD
texture package (F08-B.02) and the ROF directory (FORMAT-NOTES).

## Evidence

### E1. A texture-memory detail setting exists

`ZBD/zrdr.zbd` member 5, `detail.zrd` (start 212450, length 1994, SHA-256
`c6ce22c17769a15e6060ae7707e0cb68752b819d3ec9a3a67c8350c51a32bad3`), is a
table of detail settings. Each setting holds a list of `(condition, value)`
rows ending in a `DEFAULT` row. Conditions compare `RAM_KB` or `CPU_MHZ`.
Two keys concern textures:

| Key | Rows (condition variable → value token) |
| --- | --- |
| `TextureMemory_SW` | `RAM_KB` → `TEXMEM_MAX`; `RAM_KB` → `TEXMEM_8MB`; `RAM_KB` → `TEXMEM_4MB`; `DEFAULT` → `TEXMEM_2MB` |
| `TextureMemory_HW` | `DEFAULT` → `TEXMEM_MAX` |

The thresholds are in the member and are not repeated here. The same member
holds the other `_SW`/`_HW` pairs (`EffectsLevel`, `ObjectLOD`,
`GlobalLight`, `HUDFlag`) plus `InGameVMode`, `LoadOptionalSounds`,
`SoundLOD`, `MusicOn` and `Perspective`. The suffixes suggest software vs
hardware rendering, but no data states it. No other `zrdr.zbd`, `gamez.zbd`
or `interp.zbd` names a `TEXMEM_*` token.

### E2. The player can change it

`crimson.rof` member `ASSETS/SCRIPTS/VIDEO.SCRIPT` (SHA-256
`f0b0ae62dc582a55dfc6eaf4a8b2937ef2084a03596b8fa52542a5de4512afa9`) builds
the video options panel. It has a texture dropdown, control `vp_d_texture`
(`LAYOUT.CSV` row `VP_D_Texture`; titles `IDS_VP_TEXTURE_TITLE` 1106 and
`IDS_VP_TEXTURE_DESC` 1115 in `RESOURCE.H`). The script neither lists the
dropdown's items nor maps them to files. Native callback `$$E$$ 2133` reads
the current values (argument `0`) and writes them back (argument `1`).
`strings.dll` holds no plain-text item label for this control.

### E3. The loading scripts name the world directory, not the file

In `ZBD/interp.zbd`, each `support\<group>\init.gw` (all eight groups) sets
`SetTextureDirectory %ZBD_DIR%\%CAMPAIGN_DIR%` and then two
`%DATA_DIR%` source directories. `support\main.gw` reads `gamez.zbd` and
`planes.zbd`. No INTERP line names `texture.zbd`, `rtexture*.zbd` or
`rimage.zbd`. The world group directory is therefore the texture location,
which is consistent with F04's layout. The file inside it is chosen
elsewhere.

### E4. The `rtexture` tiers are alternative copies of `texture.zbd`

Across all eight world groups:

- Every `rtexture<N>.zbd` stores **exactly** the name set of that group's
  `texture.zbd`: no extra names, no missing names, one entry per name.
- Tiers 2/4/6/8 store most textures **smaller** than `texture.zbd`, and never
  larger. For example, C1 has 881 names, of which 864/864/740/598 are smaller
  in tiers 2/4/6/8. The top tier (9, 10, 11, 12, 14 or 15, one per group)
  stores every texture at `texture.zbd`'s dimensions, with the same stretch
  field.
- `texture.zbd` stores **every** texture palettized (8-bit indices plus a
  local palette). Every `rtexture` tier stores **every** texture as raw
  RGB565. The flags word differs for every texture between `texture.zbd` and
  the top tier.
- `N` is a size budget in MiB. Every tier's file is at most `N` MiB, and
  the top tier's `N` is its size rounded up to whole MiB:

| Group | Top tier | Bytes | MiB |
| --- | --- | --- | --- |
| C1 | 15 | 15 572 624 | 14.85 |
| C1B | 11 | 10 574 912 | 10.08 |
| C1C | 10 | 9 710 992 | 9.26 |
| C2 | 14 | 13 897 912 | 13.25 |
| C2B | 9 | 9 423 312 | 8.99 |
| C3 | 12 | 11 925 368 | 11.37 |
| C4 | 14 | 13 684 192 | 13.05 |
| C5 | 14 | 13 756 504 | 13.12 |

  The files for tiers 2/4/6/8 range from 1.93–1.95, 3.81–3.86, 5.67–5.77
  and 7.63–7.68 MiB.

Per-group archive hashes are in the table at the end.

### E5. `rimage.zbd` is a separate namespace for interface images

`ZBD/rimage.zbd` stores 254 names. None of them occurs in any world group's
`texture.zbd` or `rtexture*.zbd`, so the two can never compete for a name.
The names are referenced by root `ZBD/zrdr.zbd` members for the 2D
interface: `Loading.zrd` (45), `ia_escape.zrd` (37), `escape.zrd` (36),
`Briefing.zrd` (33), `fonts.zrd` (7), `Dialog.zrd` (3), `vehicle.zrd` (1)
and `balmoral.zrd` (1). No world `gamez.zbd` contains any of them as a
string token. `planes.zbd` and `interp.zbd` each contain one.

## What remains unknown

1. **Mapping setting → file.** `TEXMEM_2MB`/`4MB`/`8MB` line up with
   `rtexture2`/`4`/`8` by name only. No `TEXMEM_6MB` token exists, yet
   `rtexture6` is present in every group. Whether `TEXMEM_MAX` means the top
   `rtexture<N>` or `texture.zbd` is unknown, as is how the dropdown (E2)
   maps its items to either.
2. **When `texture.zbd` is loaded.** Whether the palettized `texture.zbd` is
   loaded at all is unknown: it might serve a software/8-bit path, be a
   build intermediate, or only be read by tools. `main.gw` sets
   `VideoSetDither on`, and the `*_SW` settings exist, but neither ties
   `texture.zbd` to a renderer.
3. **Lookup order across archives.** Because E4 and E5 show no name overlap
   between world archives and `rimage.zbd`, and every tier holds the full
   set, a world texture name needs no fallback within a group once one
   archive is chosen. Whether the original loads **several** tiers at once
   or ever falls back is unobserved.
4. **Mission scope.** No mission directory holds a texture archive. Missions
   use their world group's archives (E3); no per-mission override was found.
5. **Aircraft (`planes.zbd`) textures.** The archive that serves the names
   in `planes.zbd`'s materials was not established in this task.

## What was tried

- `strings` over `crimson.exe`, `crimson.icd` and every DLL: there are no
  `zbd`, `rtexture`, `rimage`, `TEXMEM`, `TextureMemory`, `zrdr` or `gamez`
  strings. `crimson.exe` (345 KB) is a loader stub, and the game-specific
  strings of `crimson.icd` are not in plain text (only GOS runtime strings
  are). Decrypting or decompiling it is out of bounds for this project, so
  the executable yields no mapping.
- A search of every `zrdr.zbd` (root, world and mission), every
  `gamez.zbd` and all INTERP scripts for archive names and `TEXMEM` tokens:
  results as in E1 and E3.
- The ROF shell scripts, `RESOURCE.H`, `LAYOUT.CSV` and `strings.dll`'s
  UTF-16 text: results as in E2. `cs-inspect rof` still refuses
  `crimson.rof` for overlapping extents (the known F05-D issue), so the
  scripts were read with a private directory walk.

## What would establish the rule

This needs an **original-run observation supplied by the owner**. A
file-access trace (for example Process Monitor filtered to `ZBD\`) of the
original game should be captured:

- with a hardware (3D) device and with the software renderer, if offered;
- at each item of the video panel's texture dropdown;
- while loading one mission from each of two world groups (for example
  M01 in C2 and a C1 mission).

The capture should record which `texture.zbd` / `rtexture<N>.zbd` /
`rimage.zbd` files are opened and when. With that trace, the rule belongs to
the renderer-settings consumer that owns the texture-detail setting. It
would then pass one archive key to `TextureCatalog` per world, which F08-C
already supports.

## Archive hashes (SHA-256)

| Group | `texture.zbd` | tiers (N: hash prefix) |
| --- | --- | --- |
| C1 | `1dfd789c…c551` | 2 `a71e6206…`, 4 `dda410e0…`, 6 `8d8aaf43…`, 8 `948e8b16…`, 15 `cca39c7a…` |
| C1B | `71a82c23…0732` | 2 `22292017…`, 4 `a3a11b30…`, 6 `bba6d626…`, 8 `cd3e1850…`, 11 `36a3c8b0…` |
| C1C | `55da80aa…b7c9` | 2 `e914ced6…`, 4 `36346efc…`, 6 `0e5b32bd…`, 8 `9e5b5c4e…`, 10 `de03f94b…` |
| C2 | `43f2a040…9d72` | 2 `9078e31e…`, 4 `fbf2497f…`, 6 `b4b0fab1…`, 8 `7169da60…`, 14 `1b496c5e…` |
| C2B | `b3a0aeba…18e3` | 2 `548d313a…`, 4 `453a0205…`, 6 `e9d4c437…`, 8 `2b7c289a…`, 9 `4edc7326…` |
| C3 | `9e97da6f…1d76` | 2 `cd841035…`, 4 `02997412…`, 6 `97f29aee…`, 8 `d8717d18…`, 12 `c1f6977f…` |
| C4 | `3c349de1…fcb2` | 2 `04c73646…`, 4 `10271491…`, 6 `485da1b4…`, 8 `80a8bbd3…`, 14 `6d1c229f…` |
| C5 | `d129eaf8…aa7e` | 2 `edac177a…`, 4 `91848465…`, 6 `d0c6d826…`, 8 `b0c9f1c2…`, 14 `4224d1ad…` |
