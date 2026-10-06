# Task #352: the measured texture-archive selection rule

Date: 2026-10-05, reviewed and corrected 2026-10-06. Task: #352
(`F08-C-texture-archive-selection`), the
follow-up that closed F08-C's recorded unknown "which texture archives a
mission actually uses"
(`docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md`,
"Recorded unknowns"). Test prefix: `accept_f08_c_selection_`. Owner paths:
`docs/findings/`, `crates/cs_content/src/textures.rs`.

**Outcome: the rule is established and implemented.** A world load registers
exactly one texture-archive descriptor and opens exactly one archive from it;
the descriptor comes from the renderer and the `TextureMemory_HW`/`_SW` detail
setting, the file from a measured descending walk of candidate names; a texture
name is then looked up in that archive, then in the shared `rimage.zbd`, then
as a loose `.tif`/`.bmp`. The earlier finding for this task
(`docs/findings/2026-09-29-t352-texture-archive-selection.md`, commit
`d2eda6c5`, on the same task branch and **not merged**) recorded that the rule
could not be established from the installation's data alone and blocked the
task; the owner has since measured it by static analysis, so this file
supersedes that outcome and keeps its census.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/textures.rs` (owner path): `TextureMemory`,
  `TextureDetailRow`, `RendererMode`, `TextureBudget`, `WorldTextureLoad`,
  `texture_budget`, `TextureDirectory`, `TextureFiles`, `FoundFile`,
  `ArchiveProbe`, `WorldArchiveChoice`, `select_world_archive`,
  `TextureLookupSource`, `texture_lookup_order`, `WorldTextureError`,
  `PROJECT_HARDWARE_TEXTURE_MIB`, `TextureCatalog::open_world`, and the
  `accept_f08_c_selection_` tests in the file's `#[cfg(test)]` module.
- Wiring only: the `[`textures`]` paragraph of `crates/cs_content/src/lib.rs`
  (one clause naming the selection rule; it edits F08-C's existing paragraph in
  place, as `docs/architecture/crate-module-docs.md` requires).

**One observable failure:** before this stage a caller had to name a tier
itself, so nothing in the crate could tell a caller that world `c1` needs
`rtexture15.zbd` and not `texture.zbd`; any name it invented was a guess, and
`accept_f08_c_selection_retail_world_groups_select_the_measured_archive` fails
without the rule because nothing selects an archive at all.

## Sources

### Owner-supplied static code evidence

| Item | Value |
| --- | --- |
| Decrypted image | `$CS_GAME_DIR/crimson.decrypted.exe` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` |
| Encrypted original | `crimson.icd`, SHA-256 `0e3b4724f045e0bedf7203cd40cdeb5b6e0b9a0bab78c3d04c278cb146e9833b` |
| Method | the owner's analysis of that image with the Kuna decompiler v1.692, every branch condition checked in the disassembly |

The digest above was re-checked on this machine on 2026-10-05 and matches. The
image is read-only and is **not** committed, and neither are its bytes, its
strings or any decompilation: this file records addresses, constants, counts
and behaviour only. Addresses are virtual addresses in that image; below
`0x643000` the file offset of `.text`, `.rdata` and `.data` is VA − `0x400000`.

This is static code evidence, not a runtime capture: nothing here may be
recorded as `verified_original`.

### Retail data used in this task

| Item | Value |
| --- | --- |
| Installation fingerprint | as fingerprinted on T340/T343/T346 (`b4e780ab…1978`) |
| Read access | read-only listing of `ZBD/<group>` and one texture-package header parse per archive |

The eight world groups, their shipped tiers and their top tiers (from the
2026-09-29 census, re-checked by directory listing on 2026-10-05):

| Group | Files | Largest shipped tier |
| --- | --- | --- |
| `ZBD/C1` | `texture.zbd`, `rtexture2/4/6/8/15.zbd` | 15 |
| `ZBD/C1B` | `texture.zbd`, `rtexture2/4/6/8/11.zbd` | 11 |
| `ZBD/C1C` | `texture.zbd`, `rtexture2/4/6/8/10.zbd` | 10 |
| `ZBD/C2` | `texture.zbd`, `rtexture2/4/6/8/14.zbd` | 14 |
| `ZBD/C2B` | `texture.zbd`, `rtexture2/4/6/8/9.zbd` | 9 |
| `ZBD/C3` | `texture.zbd`, `rtexture2/4/6/8/12.zbd` | 12 |
| `ZBD/C4` | `texture.zbd`, `rtexture2/4/6/8/14.zbd` | 14 |
| `ZBD/C5` | `texture.zbd`, `rtexture2/4/6/8/14.zbd` | 14 |

Header census re-read on 2026-10-05 from the 0x18-byte package header
(`[+4]` version, `[+8]` palettes, `[+0xc]` textures): `ZBD/C1/texture.zbd` and
`ZBD/C1/rtexture15.zbd` are version 1 with 881 entries and 0 palettes,
`ZBD/rimage.zbd` has 254 entries, and **every** stored name in those three
files is lower case.

## The rule

### 1. One descriptor per world load (`0x463f40`)

* `0x52fb10` → `0x531700` clears the texture slots and empties the
  texture-archive list at `0x7581e8`.
* `textures.zrd`, a development-only override with sections `COMMON`/`DEFAULT`
  and keys `MB%d`/`SOFTWARE`/`HARDWARE` (read by `0x530ca0`/`0x530da0`), is
  tried first and exists in no retail archive, so that path never runs.
* `0x52faf0(1)` → `0x530fe0(1)` then registers exactly one descriptor
  `{base "texture", budget N, r-flag}` through `0x530a90`.

`0x530fe0` sets `N` and the r-flag:

| Case | `N` | r-flag |
| --- | --- | --- |
| Hardware renderer **and** `0x5a0ae0(-1, &total, &free)` returned non-zero | `total >> 20` — the device's total texture memory in MiB, rounded down | 1 |
| Hardware renderer with no DirectDraw object | `TextureMemory_HW`'s budget | 0 |
| Software renderer | `TextureMemory_SW`'s budget | 0 |

`0x5a0ae0` calls `IDirectDraw::GetAvailableVidMem` (vtable slot `+0x5c`) with
`DDSCAPS_TEXTURE` (`0x1000`). It returns 1 whenever the DirectDraw object
(`0xa17104`) exists — reporting 0/0 if the DirectDraw call itself fails — and 0
only when there is no DirectDraw object at all. **Total, not free, is used.**

Setting value → budget (`0x530fe0`):

| Value | Token | Budget (MiB) |
| --- | --- | --- |
| 0 | `TEXMEM_MAX` | 0 |
| 1 | (unnamed) | 12 |
| 2 | (unnamed) | 10 |
| 3 | `TEXMEM_8MB` | 8 |
| 4 | `TEXMEM_6MB` | 6 |
| 5 | `TEXMEM_4MB` | 4 |
| 6 | `TEXMEM_2MB` | 2 |
| anything else | — | 0 |

Enum table at `0x623328` holds `{name, value}` pairs; values 1 and 2 have no
name. `TEXMEM_MAX` therefore means **budget 0**, not "the largest tier".

`ZBD/zrdr.zbd` member `detail.zrd` defaults: `TextureMemory_HW` is
`TEXMEM_MAX` on every machine; `TextureMemory_SW` is `TEXMEM_MAX` from
256000 KB of RAM, `TEXMEM_8MB` from 128000 KB, `TEXMEM_4MB` from 64000 KB and
`TEXMEM_2MB` below that.

### 2. Which file the descriptor opens (`0x531500`, reached lazily on the first texture request through `0x531280`)

1. If `N == 0`: probe `rtexture.zbd` — only when the r-flag is set — then
   `texture.zbd`. If neither exists, `N` becomes 8 and the loop below runs,
   which probes the unnumbered name a second time.
2. For `k = N` down to `0`: the candidate is `texture<k>.zbd` for `k ≥ 1` and
   `texture.zbd` for `k = 0`; with the r-flag set the `r`-prefixed name is
   probed first, then the plain one.
3. **The first candidate that exists wins.** If none exists, the descriptor is
   marked failed (`[+0xae]`) and the world has no archive textures.

"Exists" is `0x59d170` finding the name in the texture search directories
(`0x72c174`), with the global default directory `zbd` as the fallback. The
directories are added by INTERP `SetTextureDirectory` (`0x52fb40`), which keeps
only directories that exist and searches the **most recently added first**. On
retail the list is `zbd\<world>` alone — the `..\data\…` directories the INTERP
scripts name do not exist — so an archive can only come from the world's own
directory or from `zbd`, never from another world.

### 3. What this selects on this installation

* **Software renderer, and hardware without a DirectDraw object:** always
  `texture.zbd`, the palettized archive. The r-flag is never set, so no
  `rtexture*.zbd` is probed, and no plain numbered `textureN.zbd` ships, so
  every setting value lands on the unnumbered name.
* **Hardware renderer with a DirectDraw object:** `rtextureK.zbd`, where `K` is
  the largest shipped tier at or below the device's total texture MiB:

  | Total texture memory | `C1` | `C1B` | `C1C` | `C2` | `C2B` | `C3` | `C4` | `C5` |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | ≥ 16 MiB | 15 | 11 | 10 | 14 | 9 | 12 | 14 | 14 |
  | 12 MiB | 8 | 11 | 10 | 8 | 9 | 12 | 8 | 8 |
  | 8 MiB | 8 | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
  | 7 MiB | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 |
  | 1 MiB | — | — | — | — | — | — | — | — |
  | < 1 MiB, or `GetAvailableVidMem` failed (0/0) | — | — | — | — | — | — | — | — |

  The last two rows select `texture.zbd`: at 1 MiB only `k = 1` and `k = 0`
  are walked, and at 0 the loop starts at the unnumbered name.
* Exactly one archive is ever opened per world. There is no tier-to-tier
  fallback for an individual texture.

### 4. Where one texture name is looked up (`0x531b60` → `0x531900` → `0x531a60` → `0x531930`)

1. Two archive lists in the pointer table `0x635264`: the texture list at
   `0x7581e8` (the world's one archive, from §2) and the image list at
   `0x7581f8`, which is registered once at app init by `0x4a7e20` →
   `0x52fa50(0,1)` → `0x530fc0(1)` → `0x530ee0("image", 0, 1)`. The image
   list's r-flag is always 1, so it resolves to **`rimage.zbd`** through the
   global `zbd` directory whatever the budget is.
2. The world's archive is searched first, then `rimage.zbd`.
3. Inside an archive the requested name is lower-cased and binary-searched in
   the sorted directory (`0x531930`).
4. A name in neither is looked for as a loose `<name>.tif` (`0x534cf0`) and
   then `<name>.bmp` (`0x534060`), in the texture directories and then `zbd`,
   behind a flag the image keeps set (`0x635260` = 1).
5. The archive header `0x531280` reads: a 0x18-byte header with `[+4]` version
   (which must be 1), `[+8]` palette count and `[+0xc]` texture count, then
   `count` × 0x28-byte entries `{char name[0x20]; u32 offset; i32 palette_index
   or -1}`, then palettes of 256 × `u16` each, converted 565→555 when the
   device's pixel-format code is 5.

**The last-hit toggle.** The search starts in whichever of the two lists the
toggle at `0x758234` names (initially the texture list), and when a name is
not found and the caller's second argument is 0 the toggle flips **and stays
flipped** before the other list is searched. It is a last-hit cache, not a
lookup rule: `rimage.zbd` shares no name with any world archive on retail, so
no order can change an answer there, and the fixed order this crate implements
is equivalent.

### 5. The video-options texture dropdown (`VIDEO.SCRIPT` `vp_d_texture`, callback `2133`)

* The GOS `callback($$E$$, …)` lands in the native `uiData` dispatcher at
  `0x4093a0`: `2114` fills the dropdown with three entries (string ids
  `0x83`+index) and `2133` gets and sets nine values in a UI copy at
  `0x648350..0x648370`, the texture value being `+0x38` (`0x648368`).
* `gosCallback` (`0x407670`) for `callback($$A$$, …)`: id 13 (`0x418d30`) loads
  the options and copies the live block `0x64b450` (0x204 bytes) into the UI
  copy; id 14 copies it back and applies it (`0x419160`).
* Load (`0x418f1a`), setting value → dropdown index: 0, 1 and 2 → **2**; 3 →
  **1**; 4, 5 and 6 → **0**.
* Apply (`0x419392`, via `0x440950`), dropdown index → setting value: index 2 →
  `TEXMEM_MAX` (0); index 1 → `TEXMEM_8MB` (3); index 0 → `TEXMEM_6MB` (4).
* The value is written to `TextureMemory_HW` (entry `0x64f708`) when the
  hardware device is selected (`0x64f6a8`), otherwise to `TextureMemory_SW`
  (`0x64f704`).
* Those settings' **only** consumer is `0x530fe0`. So on a hardware DirectDraw
  device the dropdown cannot change the archive at all, and on the software
  renderer (or without a DirectDraw object) it always resolves to
  `texture.zbd`. The dropdown has no 2 MB or 4 MB row, and the low row writes
  `TEXMEM_6MB`.

## Design decisions on top of the measurement

* **Designed: this project's texture budget.** The original reads the device's
  total texture memory through `GetAvailableVidMem`; this project's renderer
  has no DirectDraw and no equivalent device query. `PROJECT_HARDWARE_TEXTURE_MIB`
  is therefore a **designed default**, recorded as such: 16 MiB, which is at
  least the largest tier any retail world group ships, so every group selects
  its top tier — what the original selects on a card with 16 MiB or more of
  texture memory. It is a project choice on top of the measured rule, not a
  measurement, and it is visible at the call site
  (`WorldTextureLoad::project_default`).
* **Designed: the file search starts at the largest shipped tier.** The
  original counts `k` down from `N` one integer at a time.
  `TextureFiles::highest_tier` derives the largest tier the search directories
  actually hold and the walk starts there, because every candidate above it is
  missing by construction: the same archive is opened, in the same order below
  it, without walking thousands of names that a large or corrupt device total
  would otherwise ask for.
* **Designed: the search list is built by the caller.** `TextureFiles` holds
  names only, so the VFS stays the producer of bytes and the caller (the
  renderer-settings consumer, see below) lists the directories it can see. The
  world directory's files resolve as `world/default/<file>` and the global
  directory's as `install/default/zbd/<file>`, which is what F04's designed
  mount layout serves.
* **Designed: a name no key can spell is never a candidate.** `TextureFiles::find`
  skips a listed name it cannot spell as an `AssetKey` instead of reporting it
  as found, because a candidate the session could never open would make the
  walk promise an archive it does not have; `TextureDirectory::new` refuses a
  namespace or prefix that could not spell a key at all, where it is built,
  rather than leaving such a directory to answer every probe with "does not
  exist". `WorldTextureError` therefore has one variant, `NoArchive`. Every name
  the walk generates (`texture.zbd`, `rtexture<k>.zbd`, `rimage.zbd`) spells, so
  this only decides what a hostile listing does.
* **Adopted unchanged: exact candidate comparison.** Candidate names are
  generated lower case and compared exactly against the listing. Every name
  the original probes is lower case and retail spells every archive that way,
  so the comparison decides the same files here.
* **Adopted unchanged: no tier-to-tier fallback.** A name the chosen archive
  does not hold falls to `rimage.zbd` and then to the loose files, never to
  another tier. `TextureCatalog::resolve` keeps refusing a name the
  catalogued archive does not store.
* **Adopted unchanged: an unopenable choice stays a failed row.** A selected
  tier that exists but is not a texture package becomes a failed catalog row
  (`archive_failed`), not a silently dropped archive.

## Answers to the 2026-09-29 unknowns

| Unknown (2026-09-29 finding) | Answer |
| --- | --- |
| How `TEXMEM_*` and the dropdown map to a file | §1, §2 and §5. `TEXMEM_MAX` is budget 0, which starts at the unnumbered name; the dropdown has no 2 MB or 4 MB row and its low row writes `TEXMEM_6MB`. |
| Whether palettized `texture.zbd` is ever loaded | Yes: with the software renderer, without a DirectDraw object, or when the device's total texture memory is under 2 MiB. |
| Whether several tiers are loaded or fallen back on | No. Exactly one archive is opened per world; a missing name falls through to `rimage.zbd` and then to loose `.tif`/`.bmp`, never to another tier. |
| Mission scope | Unchanged: the search list is `zbd\<world>` on retail, so a mission uses its world group's archive. |
| Aircraft (`planes.zbd`) textures | Still not established; out of this task's scope (see below). |

## Recorded unknowns and limitations

* ~~**The name lookup folds case; `TextureCatalog::resolve` does not.**~~
  **Settled by task #689** (`docs/findings/2026-10-06-t689-texture-name-case-fold.md`,
  2026-10-06): the fold is adopted. `TextureCatalog::resolve` folds the request
  through `folded_texture_name` and compares the folded spelling against the
  archive's stored spellings byte for byte, which is what `0x531930` does; ~~both
  it and `texture_lookup_order` fold through that one function, and~~
  **Amended 2026-10-06 by task #705, recording task #703's change**
  (`docs/findings/2026-10-06-t703-loose-file-name-folding.md`): only
  `TextureCatalog::resolve` folds, and only where a name is searched inside an
  archive — `texture_lookup_order` folds nothing at all, because the original's
  two loose-file probes receive the request unfolded (`0x534cf0`'s
  `sprintf(buf, "%s.tif", name)` and `0x534060`'s `%s.bmp`, re-derived there).
  `TextureId`/the catalog rows keep the archive's own spelling. The census it
  rests on covers every texture-family archive in the installation (49
  archives, 37 004 stored names): all are already the folded spelling, no two
  fold onto each other, and every name table is sorted.
* **The file probe's case handling is not established.** The original probes
  the host file system, whose names it folds without regard to case. ~~Every
  candidate this crate generates is lower case, so the comparison is
  equivalent here, but~~ **Amended 2026-10-06 by task #705, recording task
  #703's change:** the archive candidates this crate generates are still lower
  case, but a loose candidate now carries the request's own spelling, so
  `TextureFiles::find`'s exact comparison decides a mixed-case loose request
  here. No claim is made about a listing that spells a tier in another case,
  and no retail content reaches the loose probes (0 loose `.tif`/`.bmp` files
  under `ZBD`, listed 2026-10-06).
* **The dropdown's out-of-enum branch is not covered.** `TextureDetailRow::for_setting`
  implements the measured mapping for the enum's whole value domain (0..=6); a
  value outside it is only reachable from a hand-edited profile and is reported
  as the bottom row, which the evidence does not decide.
* **The loose-file fallback is implemented as an order, not as a reader.** §4.4
  says the original tries `<name>.tif` and `<name>.bmp`; whether the interface
  actually uses loose files, and which reader serves them, is not established
  (F08-B.03/.04 and F08-C's "Recorded unknowns" already say so).
* **Which renderer-settings consumer supplies `TextureFiles`,
  `RendererMode` and the two settings is not wired** — it lives outside this
  task's owner paths and is filed as task #688. Until then the selection rule
  is reachable through `TextureCatalog::open_world` and nothing calls it in the
  running game.
* **Which archive serves the names in `planes.zbd`'s materials** is still
  unknown; unchanged from the 2026-09-29 finding.
* **The header layout in §4.5** (version at `[+4]` must be 1, palette count at
  `[+8]`, texture count at `[+0xc]`, 0x28-byte entries, 256 × `u16` palettes)
  is the owner image's reading and is consistent with F08-B.02's reader, which
  is what this crate actually parses. The 565→555 conversion for
  pixel-format code 5 is **not** implemented here: it is a presentation
  decision at the upload boundary (F17-B), and F08-C lists
  `rgb565_expansion_unknown` for every 565 texture.

## Tests

| Test | Covers |
| --- | --- |
| `accept_f08_c_selection_budget_zero_and_large_budgets_walk_the_measured_loop` | §1/§2: budget 0 without the r-flag, a 16 MiB device on a 15-tier group, the `r`-first order at one tier, the budget capping the walk, `k = 0` ending at the unnumbered pair, and the global-directory fallback with its key |
| `accept_f08_c_selection_missing_tiers_walk_down_to_the_unnumbered_name` | §2: a group shipping only tiers 6 and 2 at 16/5/1 MiB with the full walked sequence, a group with no candidate (`no_texture_archive` naming what it probed) |
| `accept_f08_c_selection_the_setting_path_registers_the_detail_setting` | §1: the whole value → budget table including the unnamed values and out-of-domain values, `detail.zrd`'s RAM defaults, which setting each renderer reads, the device total outranking both, the setting path never probing an `r` name, and both spellings present so the r-flag decides |
| `accept_f08_c_selection_the_video_dropdown_rows_map_to_the_measured_settings` | §5: the three rows with their indices and written settings, the load direction for every enum value, the round trip, and every row leaving a retail-shaped group on `texture.zbd` for the software and no-DirectDraw paths while a hardware device's total decides alone |
| `accept_f08_c_selection_the_lookup_order_is_the_world_archive_image_archive_then_loose_files` | §4: the four-source order, the key of each source, case folding, the `.bmp` fallback, the order following the selected archive and skipping one that does not exist |
| `accept_f08_c_selection_open_world_catalogs_the_archive_the_rule_selects` | the rule reaching production: `open_world` opens the selected tier, the resolved texture's texels come from that tier, the software path opens the other world's unnumbered archive, `rimage.zbd` opens from the global directory, and an unopenable selection stays a failed row |
| `accept_f08_c_selection_a_name_no_key_can_spell_is_never_a_candidate` | the probe cannot promise a file the session would refuse: a listed name no `AssetKey` spells is skipped rather than reported found, so no walk opens it, while a spellable source beside it stays reachable; and `TextureDirectory::new` refuses a namespace or prefix that could never spell a key, while the world's and global constructors stay infallible |
| `accept_f08_c_selection_retail_world_groups_select_the_measured_archive` (ignored) | §3 on the real installation: all eight groups, their tier census, the selection at 16/12/8/7/1 MiB and without DirectDraw with the r-flag check, the software renderer at every setting, `open_world` opening `ZBD/C1/rtexture15.zbd` with 881 entries, `rimage.zbd` opening with 254 entries, and the two namespaces sharing no name |

Retail result on this machine (installation fingerprinted `b4e780ab…1978`):
all eight groups match §3 exactly, world one's project default opens
`ZBD/C1/rtexture15.zbd` (881 entries, version 1, no failures),
`ZBD/rimage.zbd` opens with 254 entries, and no name is in both namespaces.
The test takes about 52 seconds in a debug build, almost all of it the
installation walk.

## The retail case of the global directory's spelling

The original's default texture directory is spelled `zbd`; the installation
spells the same directory `ZBD`, and so do these tests' fixtures. The session
resolves the two without regard to case — `cs_assets::vfs` indexes every mount
member by the case-folded `logical_key` — so the key `install/default/zbd/<file>`
that [`TextureDirectory::global`] builds opens `ZBD/<file>` on any host. That is
production behaviour, verified by the retail test above.

A **test** that reads the directory off the host file system has no such
protection: joining the literal `zbd` finds `ZBD` on a case-insensitive file
system and fails on a case-sensitive one. Both `accept_f08_c_selection_missing_tiers_walk_down_to_the_unnumbered_name`
and `accept_f08_c_selection_open_world_catalogs_the_archive_the_rule_selects`
were green on the case-insensitive macOS host and **failed in CI on Linux**
(`/…/zbd is listed: No such file or directory`) for exactly that reason. The
helpers now resolve the fallback directory by comparing the root's entries
without regard to case, and
`accept_f08_c_selection_open_world_catalogs_the_archive_the_rule_selects`
asserts that the fallback listing is the installation's shared directory and
holds `rimage.zbd`, so the search list cannot silently degrade into an empty one
on a case-sensitive host. Retail measurements were re-checked by reading the
three package headers directly: C1 `texture.zbd` and `rtexture15.zbd` are
version 1, 881 entries, 0 palettes; `rimage.zbd` is 254 entries; every stored
name is lower case; and the two namespaces share no name.

Mutation probes (each applied, `cargo test -p cs_content --locked --
accept_f08_c_selection` run, then reverted; the file is restored after every
one):

| Mutation | Result |
| --- | --- |
| r-prefixed candidate probed after the plain one | 3 fail |
| the tier walk counts up instead of down | 5 fail |
| a missing candidate falls through to the next tier instead of winning | 5 fail |
| the setting path sets the r-flag | 3 fail |
| the highest-tier cap removed, so the walk starts at the budget | 2 fail |
| `TextureMemory` value 3 mapped to 7 MiB | 1 fails |
| `TextureDetailRow::for_setting` sends 3 to the top row | 1 fails |
| the loose `.tif`/`.bmp` sources dropped from the lookup order | 1 fails |
| `open_world` always opening `texture.zbd` | 1 fails |
| `TextureFiles::find` reporting an unspellable name as found | 1 fails |
| `TextureFiles::find` panicking on an unspellable name (the pre-review code) | 1 fails |
| `TextureDirectory::new` accepting a namespace or prefix that cannot spell a key | 1 fails |

The three probe rows above were run by the reviewer. The first six were the
implementer's, run before the review fixes and re-verified by the reviewer where
the fix touched the same code.

## Checks run on this branch (2026-10-05)

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | clean |
| `cargo test --workspace --locked` | 386 test binaries ok, none failed (2026-10-06, reviewer run on this branch). The implementer's 2026-10-05 run reported one failure, `cs_xtask --test accept_t430_ci_disk_budget::accept_t430_a_panic_backtrace_names_the_file_and_line`, reproduced then on a clean `origin/main` worktree; filed as task #691 and **not reproducible** on 2026-10-06, when that test alone and the whole suite both passed. It is neither this branch's defect nor a proven environment fault. |
| `cargo test --workspace --locked -- accept_f08_c_selection --include-ignored` | 8 passed, 0 failed (7 synthetic + the retail one, ~53 s) |
| each of the eight with `--exact`, alone | 1 passed each |
| `env -u CS_GAME_DIR cargo test -p cs_content --locked -- accept_f08_c_selection --include-ignored` | the retail test **fails** with "CS_GAME_DIR must point at the original installation for this test", the seven synthetic ones pass |
| CI (Linux, `ubuntu-latest`) | **failed on the implementer's pushed commits and passed only after the review fixes.** Two of this task's own tests failed there for the case-sensitive-file-system reason above; that is what found it. |

No evidence report: this task's retail use is the census and the per-world
selection a test observes, following F08-C and F08-B's precedent, and the
fingerprinted decode audit against a pinned reference is F08-D.

## Out of scope, filed as tasks

* #688 — wire the renderer-settings consumer: own `TextureMemory_HW`/`_SW` and
  the video-options dropdown, build the world's `TextureFiles` from the
  installation, report the texture budget, and call
  `TextureCatalog::open_world` where a world loads its textures.
* #689 — decide whether `TextureCatalog::resolve` adopts the measured case
  fold. **Done** 2026-10-06: the fold is adopted, see
  `docs/findings/2026-10-06-t689-texture-name-case-fold.md`.
