# Task #340: ZBD family header layouts and archive names

Date: 2026-09-28. Task #340 "Read ZBD family header layouts and sound archive
names from the pinned source", a follow-up to F06-A
(`docs/findings/2026-09-28-f06-a-zbd-family-inventory-and-dispatch.md`,
"Recorded unknowns") and the F06-B note on the task.
Capabilities used: `retail` (read-only, `$CS_GAME_DIR`) plus ordinary
build/test. Test prefix: `accept_t340_`. Evidence:
`docs/findings/evidence/T340.json`.

## Sources

- **Pinned reference source** — mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` (`docs/research/SOURCES.md` S02,
  S06; the same commit as S07). Cloned into `private/` for reading only; no
  code was copied (EUPL-1.2). Files read:
  - `README.md` — the per-game support table and the archive-name list;
  - `crates/unzbd/src/main.rs`, `crates/unzbd/src/commands.rs` — which
    subcommand reads which archive and which archive version Crimson Skies
    uses;
  - `crates/mech3ax-archive/src/archive.rs` — the sound/reader archive
    layout;
  - `crates/mech3ax-gamez/src/gamez/common.rs`,
    `crates/mech3ax-gamez/src/gamez/cs/mod.rs`,
    `crates/mech3ax-gamez/src/gamez/cs/fixup.rs` — the GameZ header;
  - `crates/mech3ax-image/src/textures.rs` — the texture-package header;
  - `crates/mech3ax-anim/src/parse.rs` — the animation header.
- **Retail installation** — the read-only installation at `$CS_GAME_DIR`,
  install fingerprint
  `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`, content
  fingerprint
  `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` (F02
  production discovery; identical to the F04-D evidence). All 184 `.zbd`
  archives were dispatched by production code in
  `accept_t340_retail_every_zbd_archive_dispatches_to_its_family`; the
  per-archive table (spelling, size, SHA-256, family, basis, header words,
  trailer) is the evidence artifact `zbd-dispatch.json` (private, referenced by
  digest from the report).

## Per family

Offsets are byte offsets from the start of the file unless stated;
all words are little-endian `u32`.

| Family | Leading header | Crimson Skies archive names (level under `ZBD/`) | Retail check (184 archives) |
| --- | --- | --- | --- |
| interp | signature `0x08971119` @0, version `7` @4 (already documented, S07) | `interp.zbd` (root) | 1/1 validated |
| GameZ | signature `0x02971222` @0 (`SIGNATURE`), version `42` @4 (`VERSION_CS`); 40-byte `HeaderCsC` | `planes.zbd` (root), `gamez.zbd` (world group) | 9/9 validated |
| animation | signature `0x08170616` @0 (`SIGNATURE` in `mech3ax-anim`), version @4 — the source lists 28 (Recoil), 39 (MechWarrior 3), 50 (Pirate's Moon) and **no Crimson Skies version**; retail carries `53` | `cam_anim.zbd` (world group), `mis_anim.zbd` (mission) | 61/61 signature match, all version 53 |
| texture | no signature: `TexturesHeaderC` is u32 `0` @0, u32 `1` @4 (`has_entries`), i32 global palette count @8, u32 texture count @12, u32 `0` @16, u32 `0` @20 | `texture.zbd`, `rtexture*.zbd` (world group), `rimage.zbd` (root) | 49/49 start with `0, 1` |
| reader | **no leading header** — version-one archive: trailer u32 version `1` at `size-8`, u32 member count at `size-4`, preceded by `count` × 148-byte entries (u32 start, u32 length, 64-byte NUL-padded name, 76 unexplained bytes) | `zrdr.zbd` (root, world group, mission) | 62/62 trailer version 1, every member inside `[0, table_start)`, 10–221 members |
| sound | **no leading header** — the same version-one archive (`unzbd sounds`, CS → `Version::One`) | `soundsl.zbd`, `soundsh.zbd` (root) | 2/2 trailer version 1 (2520 and 2521 members); every member starts with `RIFF` … `WAVE` |

Why Crimson Skies uses version one: `unzbd` maps `GameType::MW | RC | CS` to
`Version::One` for both `sounds` and `reader`; only Pirate's Moon uses the
12-byte version-two trailer with a CRC.

### Answers to the task's questions

- **Does sound live in ZBD?** Yes. The mech3ax README lists `sounds*.zbd` as
  supported for Crimson Skies ("Sound archives (`sounds*.zbd`) containing sound
  effects … to ZIP archives of WAV files"), and the installation has exactly
  two: `ZBD/soundsl.zbd` and `ZBD/soundsh.zbd`, both version-one archives whose
  members are all RIFF/WAVE files. What the `l`/`h` suffixes mean is
  **unknown** (the member counts differ by one).
- **The four unnamed root archives** F06-A could not name are `rimage.zbd`,
  `soundsh.zbd`, `soundsl.zbd` and `zrdr.zbd` (plus the known `interp.zbd`
  and `planes.zbd`: six in total, matching the F02-D count).
- **Inferred rules confirmed.** The README ties `zrdr.zbd` to the reader
  family, `rtexture*.zbd` (and `rimage.zbd`) to the image/texture packages and
  `cam_anim.zbd`/`mis_anim.zbd` to the animation family, so those role rules
  are now `Documented`. One F06-A rule was **incomplete**: `zrdr.zbd` also sits
  directly under `ZBD/` (`ZBD/zrdr.zbd`, 221 members), which the old rule
  (world group and mission only) left undispatchable.

## Code changes (`crates/cs_formats/src/zbd/`)

- `header.rs`: `GAMEZ_SIGNATURE`/`GAMEZ_VERSION` (+ offsets) and
  `ANIMATION_SIGNATURE`/`ANIMATION_VERSION` (+ offsets), re-exported from
  `mod.rs`.
- `family.rs`: GameZ gets a `SignatureRule` (`Documented`, source + retail
  match cited). Animation gets a `SignatureRule` labelled
  **`ObservedTool`**, not `Documented`: the signature is in the pinned source,
  the version 53 is only observed in the installation. Sound gets the role rule
  `sounds*.zbd` at the content root; texture gains `rimage.zbd` at the content
  root; reader's `zrdr.zbd` rule gains the content-root level; every remaining
  `Inferred` role rule is upgraded to `Documented` with the README citation.
  Sound and reader keep `HeaderRule::Undocumented`, now with the reason that
  their index is a trailer; texture keeps `HeaderRule::Undocumented` with the
  reason that its documented leading words are constants, not a signature.
- `dispatch.rs`: a role whose family has a signature rule, probed with bytes
  that match *another* family's documented signature, now fails as
  `header_role_conflict` instead of `header_mismatch`. F06-A documented that
  "a family disagreement outranks" other failures; with only one signature
  family that case could not arise, with three it can (e.g. a GameZ header at
  `cam_anim.zbd`).
- `archive.rs`, `sound_archive.rs`, `reader_archive.rs`, `role.rs`, `mod.rs`,
  `lib.rs`: doc comments only. `MemberTable::named` now refuses every family
  (all six own a role rule); it is kept as the guard for a future family that
  has none.

Wiring: `crates/cs_formats/Cargo.toml` gains the dev-dependency `cs_assets`
(production discovery and SHA-256 for the evidence harness; `Cargo.lock`
updated accordingly). The test module is declared in
`crates/cs_formats/tests/zbd/main.rs`.

### Decisions

- **No texture signature rule.** A rule "u32 0 @0, u32 1 @4" would make
  header-only dispatch route any unknown file that starts with those two
  common words to the texture reader, and would turn every other file that
  starts with `0` into `unsupported_header_version`. The source itself selects
  textures by subcommand and only asserts those words after that choice. The
  layout is recorded here; dispatch stays role-only for textures.
- **No trailer rule in dispatch.** `ZbdProbe` carries the leading bytes only;
  checking the trailer belongs with reading the member table, which is task
  #343. The retail test checks the documented trailer on all 64 sound/reader
  archives so the recorded layout is evidence-backed now.
- **Animation version pinned at 53.** Dispatch refuses the other games'
  versions (`unsupported_header_version`) rather than accepting any version
  under a known signature; nothing about the Crimson Skies animation body is
  documented (the README: "`cam_anim.zbd`/`mis_anim.zbd` files are not
  supported yet").

## Tests (`crates/cs_formats/tests/zbd/t340.rs`, `accept_t340_*`)

| Test | Covers |
| --- | --- |
| `gamez_signature_validates_planes_and_world_gamez` | documented GameZ header validates at both roles; wrong/short bytes refused |
| `animation_signature_validates_camera_and_mission_archives` | animation header validates; the rule is labelled `observed_tool` |
| `other_game_versions_are_refused` | GameZ 15/27/41 and animation 28/39/50 fail with and without a role |
| `documented_signatures_conflict_with_other_roles` | six cross-family placements fail as `header_role_conflict`, including the new signature-vs-signature case |
| `header_only_dispatch_reaches_the_new_families` | GameZ/animation headers route without a role; `0, 1` alone routes nothing |
| `sound_archives_route_to_the_sound_reader` | `ZBD/sounds*.zbd` → sound, unvalidated with the trailer reason; not at other levels; the sound reader reads it, the reader reader refuses it |
| `content_root_reader_and_image_archives` | `ZBD/zrdr.zbd` → reader, `ZBD/rimage.zbd` → texture; every role rule is `documented` |
| `retail_every_zbd_archive_dispatches_to_its_family` (ignored without `CS_GAME_DIR`, fails loudly then) | all 184 retail archives dispatch; per family: animation 61, GameZ 9, interp 1, reader 62, sound 2, texture 49; signature families validate; trailer and RIFF checks above |

Updated F06-A/F06-B tests (evidence changed, assertions not weakened): the
AC01 pair is now two *documented* headers (INTERP and GameZ, both
`Validated`); the role table asserts the new names and `Documented` classes;
"sound owns no role rule" became "every family owns one"; `ZBD/zrdr.zbd`
moved from the misplaced list to the observed list (replaced there by
`zbd/c1/soundsl.zbd` and `zbd/c1/rimage.zbd`); the F06-B sound tests build
their table from a dispatch of `zbd/soundsl.zbd` instead of
`MemberTable::named`, and `named` is asserted to refuse every family.

Mutation probes (each applied, the `zbd` target run, then restored):

| Mutation | Failing `accept_t340_` tests |
| --- | --- |
| GameZ signature constant changed | 2 |
| sound role prefix changed (`soundz`) | 3 |
| signature-vs-signature conflict branch disabled | 1 |
| content-root level removed from the `zrdr.zbd` rule | 2 |

## Recorded unknowns

- **Sound and reader member tables are not read yet** (task #343): the 76
  bytes after each entry's name are unexplained in the pinned source
  (`garbage`) and stay unknown.
- **Sound sample descriptors** (format, channels, rate, loops) are still
  `SoundField::Unknown`; the members are RIFF/WAVE, and reading their headers
  is task #344.
- **`soundsl` vs `soundsh`** — the meaning of the suffixes and which one the
  game loads when is unknown.
- **Crimson Skies animation layout** after the 8-byte header is not
  documented by the pinned source; only the signature and the observed
  version are used.
- **Reader entry encoding** remains undocumented (`EncodingEvidence::Undeclared`).
