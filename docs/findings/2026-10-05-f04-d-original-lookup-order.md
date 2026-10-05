# F04-D: the original engine's file lookup order (code-derived)

Date: 2026-10-05. Task #341 `F04-D-original-order`. Follow-up of
`2026-09-28-f04-d-observed-collisions-and-async-cancel.md` and
`2026-10-02-f04-d-integrated-member-collisions.md`.

## Provenance

Static analysis of the owner-supplied decrypted image `$CS_GAME_DIR/crimson.decrypted.exe`
(sha256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, decrypted from
`crimson.icd` sha256 `0e3b4724f045e0bedf7203cd40cdeb5b6e0b9a0bab78c3d04c278cb146e9833b`),
done by a Claude session with the Kuna decompiler v1.692 at the owner's request (owner note of
2026-10-05 on #341). Also read: `GOSDATA/ASSETS/BINARIES/roffile.dll` (sha256
`1bc7b4b1adba1bf6a510e62f93473824c662acbf2a2176087b7b4b85338ba58a`), `ZBD/interp.zbd` and
`ZBD/zrdr.zbd`. Addresses are virtual addresses (image base `0x400000`).

This is **code-derived evidence, not a runtime capture**. Nothing here is `verified_original`.
No image bytes or decompiled code are committed; only addresses, constants and behavior.
Only the retail facts in section F were re-measured by this task, with
`accept_f04_d_original_order_retail_mission_shadows_world_in_five_cases`.

## A. Reader (`.zrd`) members: the `zrdr` file system

- Startup `0x4a6ff0`: default directory list gets `zbd`; loose reader path `..\data\common\zrdr`;
  `detail.zrd` is tried loose (`0x43fb50`, absent in retail), then `zbd\zrdr.zbd` is mounted as the
  **persistent root archive** (`0x57a7a0`). `0x4a93c0` hashes it with salt `0xa2332fc8` (stored at
  `0x71d8c0`), a consistency fingerprint, not a lookup.
- World/mission load `0x463cb0` (object at `0x71b480`): loose path `common`, `<w>`, `<w>\nets`,
  `<w>\<m>` below `..\data` (only existing directories count; none in retail). With USEZBD
  (`[this+0x6f8]`, default 1, set at `0x463771`; cleared only by `-nozbd` or a debug toggle) it
  unmounts every archive but the root (`0x5795b0`), mounts `zbd\<w>\<m>\zrdr.zbd`, then
  `zbd\<w>\zrdr.zbd`. Mounting appends at the tail (`0x579500` → `0x5794a0`): the order is
  **[root, mission, world]**. World table `0x625a70` (c1, c1b, c1c, c2, c2b, c3, c4, c5); mission
  tables `0x625a90` (m01..m05), `0x625aa4` (mp1..mp5), `0x625ab8` (ia1).
- Open `0x579c60`: the name is reduced to its **basename**. Archive pass `0x579630` walks archives in
  mount order; inside one (`0x59ddb0`) a linear case-insensitive `_stricmp` scan, **first entry wins**;
  first archive holding the name wins. A loose file of the same basename that is newer
  (`CompareFileTime >= 1`) overrides the archive copy. Only if no archive has the name, the loose
  pass (`0x579710` → `0x59d170`) searches the most recently added directory first, then `zbd`.
- Archive directory (`0x59dbf0`): EOF−8 trailer `[u32 version=1][u32 count]`, before it `count`
  entries of 0x94 bytes `{u32 offset; u32 size; char name[0x8c]}` (matches `cs_formats::zbd::trailer`).
- `0x4acc20` (briefing objectives) reads `objectives.zrd`; if unreachable it unmounts to root and
  mounts only `zbd\<w>\<m>\zrdr.zbd`.

## B. GameZ, planes, animation archives (INTERP scripts)

`0x463f40` sets `USEZBD`, `HARDWARE`, `CAMPAIGN_DIR`, `MISSION_DIR` and runs `support\main.gw`:
`init.gw` → `AnimAddZBDFile zbd\<w>\cam_anim.zbd` then `zbd\<w>\<m>\mis_anim.zbd`; `<w>\init.gw` →
`SetTextureDirectory zbd\<w>`, `LoadSoils soils.zrd`, `MissionZBDFile = zbd\<w>\gamez.zbd`;
`main.gw` → `GameZReadZBDFile %MissionZBDFile%`, then `zbd\planes.zbd`.

- `gamez.zbd` is per **world group only**; `planes.zbd` is read from the ZBD root.
- `AnimAddZBDFile` (`0x4ee690`, list `0x728360`, de-duplicated by exact string); `0x522ef0` loads
  **every** listed file in list order (magic `0x08170616`, version `0x35`): world `cam_anim.zbd`,
  then mission `mis_anim.zbd`. Neither replaces the other; no directory fallback.
- The INTERP run (including `LoadSoils`) happens before `0x463cb0` remounts the reader archives.

## C. Textures (see #352)

Exactly one texture archive per world: looked up in `zbd\<w>`, falling back to `zbd`, never another
world. Hardware renderer with a DirectDraw device: the largest N ≤ total texture MiB with
`rtextureN.zbd` or `textureN.zbd` present; software renderer: `texture.zbd`. `rimage.zbd` comes from
the same search (via the `zbd` fallback).

## D. ROF / GOS assets

`0x4116c0` loads `<UIAssetPath>\assets\binaries\roffile.dll` (`UIAssetPath`, default `GOSData`,
registered in `0x43fb50`), then registers in order: (1) `crimptch.rof` under
`<EXE Path>\GOSDATA\Assets\` if it exists (`EXE Path` from
`HKLM\SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\1.0`; with no key the default path does not
exist and **the patch is skipped**), (2) `<UIAssetPath>\Assets\crimson.rof`, (3) the loose
`<UIAssetPath>\` directory, (4) the current directory (two empty BSS strings at `0x64e710`,
`0x64e71c`). `roffile.dll` `AddNewROFDirectory` (`0x10001000`) push_backs; `MetaOpenFile`
(`0x100016e0`) tries entries in insertion order, first success wins:
**crimptch.rof → crimson.rof → loose GOSData → cwd**. So `ASSETS/SCRIPTS/AIRFRAME.SCRIPT` comes from
`crimptch.rof`, and `ASSETS\GRAPHICS\ARIAL8.TGA` / `FONT.TGA` come from the `crimson.rof` members,
not the loose `GOSDATA/ASSETS/GRAPHICS/*.tga`.

## E. Directory-level fallback

| Lookup | Fallback |
| --- | --- |
| reader members | yes: by basename through root → mission → world, then loose path, then `zbd` |
| texture archive | `zbd\<w>` then `zbd` |
| gamez, planes, cam_anim, mis_anim | none (explicit paths) |
| ROF | patch ROF → main ROF → loose files |

## F. What this means for this installation (retail, re-measured)

Over all 62 `zrdr.zbd`: no world or mission member name occurs in the root archive; **mission
shadows world** in exactly five cases, all with different bytes: `targets.zrd` in C1C/IA1, C1C/MP1,
C1C/MP3; `security_destroy.zrd` in C2/M01; `fueltruck.zrd` in C3/M02. The root holds two
`player.zrd` members: entry #22 (3414 B, sha256 `a8cc7547…`), the only one by-name lookup returns,
and entry #100 (34711 B, an ANIMATION_DEFINITIONS document), unreachable by name (only root
`anim.zrd` names it, parsed only if the compiled anim ZBDs fail to load, `0x5230d0`).

## G. Comparison with the designed layout

`SessionBuilder::mount_installation` mounts `install` (whole tree, `shared`) and one
`world-<n>` mount per world group directory (`mission_world`, bound to the world), file-level only.

| Designed | Original | Verdict |
| --- | --- | --- |
| collision of reader members is never decided (`UnmeasuredOrder`) | mount order root → mission → world, first hit wins | now decidable; VFS does not model it |
| reader members have no key by basename; archive members are not mounted by the VFS | requested name reduced to basename | mismatch: keying |
| no root-first precedence | root archive searched first | mismatch |
| no mission-over-world precedence for members | mission before world | mismatch |
| `crimptch.rof` is `patch`, `crimson.rof` `shared`; ROF collision `conflicting` | patch first, then main | consistent in spirit; order not pinned by status |
| loose `GOSDATA` file and ROF member are different keys, no order | ROF registered before loose directory | mismatch for GOS requests |
| texture archives per world with `rtexture*` as `shared`; order blocked | one archive per world by renderer/memory rule | see #352 |
| `gamez.zbd` per world | per world only | consistent |

`PRECEDENCE_ORDER_STATUS` stays `designed`: the evidence is static code analysis of one executable,
not a measured retail run, and the VFS does not yet implement the measured order; changing the
constant would claim the VFS follows it. Mismatches are filed as separate tasks (see the task
history of #341).

## Test

`crates/cs_assets/tests/accept_f04_d_original_order_retail_shadowing.rs`
(`accept_f04_d_original_order_retail_mission_shadows_world_in_five_cases`,
`#[ignore = "requires CS_GAME_DIR"]`, panics without it): parses all 62 `zrdr.zbd` through the
production reader-archive parser and pins section F. No production change.
