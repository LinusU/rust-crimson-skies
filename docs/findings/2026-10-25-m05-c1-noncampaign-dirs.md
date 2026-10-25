# ZBD/C1 `IA1`/`MP1`..`MP3`: what the original program makes of them (#471)

Task #471 (`M05-C1-NONCAMPAIGN-DIRS`), follow-up of M05-A. Capability used:
`retail` (read-only `$CS_GAME_DIR`; static reading of `$CS_ENGINE_IMAGE`, whose
sha256 is the one in `data/owner-static/engine-facts.toml`). Every engine row is
`static`; no original run happened and nothing here is `verified_original`.
No original bytes and no disassembly are committed; addresses are virtual
addresses in the image (file offset = VA - 0x400000).

## 1. Identification, from the original program's own load path

The image holds one table of mission directory names and one routine that
selects from it (item 1 of the task):

| VA | content |
| --- | --- |
| 0x625a70..0x625a8c | world-group names `c1 c1b c1c c2 c2b c3 c4 c5` (8 pointers' worth of 4-byte slots) |
| 0x625a90..0x625aa0 | `m01 m02 m03 m04 m05` |
| 0x625aa4..0x625ab4 | `mp1 mp2 mp3 mp4 mp5` |
| 0x625ab8 | `ia1` |
| 0x625b08 | format `zbd\%s\%s\zrdr.zbd` (group, mission directory) |
| 0x625b1c | format `zbd\%s\zrdr.zbd` (group only) |

* The group-name routine (0x4639d0) maps a session field (+0xc1c, 1..8) to the
  group name.
* The mission-directory routine (0x463a50) switches on a *game type* field
  (+0x700) and an index field (+0xc20): type 1 selects `m01`..`m05` by index
  1..5; type 2 selects `mp1`..`mp5` by index 1..5; type 3 selects `ia1` only
  for index 1. Anything else returns the caller's own value unchanged.
  Two sibling one-line predicates (0x4639b0: type == 3, 0x4639c0: type == 2)
  exist beside it.
* The mission-directory routine has 21 call sites and the group routine 26, among them the objectives loader
  that formats `..\data\%s\%s\zrdr\objectives.zrd` / `zbd\%s\%s\zrdr.zbd`
  (0x4acc4a, 0x4acca1, referencing 0x629ed0).

So `ZBD/<group>/IA1` is the **instant-action** mission directory and
`ZBD/<group>/MP<n>` the **multiplayer** mission directory, selected by the
game type exactly as `M<nn>` is selected for the campaign; all three are loaded
through the same `zbd\<group>\<mission>\zrdr.zbd` path. This agrees with, and is
independent of, the member-shape evidence in
`2026-10-02-f14-d-1-reader-archive-directories.md` (`ia.zrd` only in `IA1`;
`MP<n>` named and shaped like missions) and with the slot-to-mode decode in
`2026-10-03-f56-a-slot-mode-bindings.md`.

The numeric values of the game-type field are a static reading of the routine's
compare chain, not an observed lobby setting. The lobby setting 0x642f90 and
global 0x71c194 in `2026-10-24-m01-world-multiplayer-root-gating.md` are a
different encoding and are not equated with +0x700 here.

## 2. Does a campaign mission's `zrdr.zbd` reference them?

No reference was found.

* The engine side: the directory is chosen by (game type, index) in code, not
  read from mission data. Nothing in the routine consumes a string from an
  archive.
* The data side: the stored bytes of all 22 campaign `ZBD/C*/M*/zrdr.zbd`
  archives contain no `ia1`, `mp1`..`mp5` or `_ia<n>` token, and no
  `instantaction` token (a raw, case-insensitive scan of the files). The only
  `mp<n>` word-tokens found are inside MP archives themselves.
* `cs-inspect closure --mission mission/ch1-m02` and `mission/ch1-m05` each
  reach three nodes (the mission, its `zrdr.zbd` install file and its program
  script), none an `ia_scenario`, `multiplayer_scenario` or `IA1`/`MP*` file.
  The closure is shallow (catalog-level), so it is corroboration only.

**Limit, recorded as unknown:** the raw scan reads stored bytes; members held
compressed were not decoded, so a reference inside a compressed member is not
excluded by it. Affected content: the program closure of every campaign mission
(`closure_sha256`). Resolving task: a decoded-member scan through the production
reader, to be filed if a campaign mission's closure is to be claimed complete.
The engine-side result (no data-driven selection) makes such a reference
unlikely but does not prove its absence.

## 3. Bindings citing these directories

`grep -il '/(ia1|mp[1-5])/' missions/bindings/*` matches no file: no binding,
M05's included, cites a span inside an `IA1`/`MP*` directory, so none needs
item 1 to justify it. The only references to the directories are the catalog's
own `ia_scenario/*`, `multiplayer_scenario/*` and `script/*-ia1|mp<n>-zrdr`
rows (F14-D.1), now justified by section 1.

## 4. Consequences and remaining unknowns

* A campaign mission's dependency closure need not include `IA1`/`MP*`; their
  presence in the world-group directory is not an unknown that blocks M05-A/B.
* `mp4`, `mp5` are in the engine's table but no installation directory exists
  for them (`ZBD/<group>/MP4` and `MP5` are absent): whether any retail path
  reaches them is unknown. Affected content: none present.
* `ZBD/C1B` and `C1C` `IA1`/`MP1`/`MP3` follow by the same table (M03-A's
  recorded unknown is resolved at the family level); only C1/C1B/C1C and the
  IA1/MP1 shapes were re-observed here.
* Whether the game exposes every directory to the player is not shown by file
  access or static reading; that needs an owner-supplied original run.
