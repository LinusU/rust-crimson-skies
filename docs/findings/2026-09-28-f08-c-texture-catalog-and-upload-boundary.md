# F08-C: texture catalog, per-world resolution and the upload boundary

Date: 2026-09-28. Task: F08-C "Connect images to the content catalog and GPU
upload boundary" (`specs/F08-texture-archives-and-conventional-image-decoding.md`,
section `### F08-C`, AC03 and non-negotiables #3 and #4). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Test prefix: `accept_f08_c_`.
Capabilities used: ordinary build/test for the synthetic tests; read-only
`$CS_GAME_DIR` for one `#[ignore = "requires CS_GAME_DIR"]` test. No evidence
report: the task needs only ordinary build/test, and the fingerprinted decode
audit is F08-D.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/textures.rs` (new): `TextureArchive`, `TextureId`,
  `TextureRef`, `TextureCatalog` (`open`, `resolve`, `decode`,
  `prepare_upload`, `records`, `retry_failed`, `require_session`),
  `ResolvedTexture`, `TextureAttempt`, `TextureUpload`,
  `PresentationUnknown`, `ImageRecord`, `ImageReadiness`, the error types,
  and the `accept_f08_c_` tests in its `#[cfg(test)]` module (the owner path
  is this one file, so the tests live there).
- Wiring only: `crates/cs_content/src/lib.rs` (`pub mod textures;` and a
  crate-doc paragraph). The `cs_assets` dependency it needs was already added
  on `main` by F07-C.

**One observable failure:** before this stage nothing connected
`read_zbd_textures` to the VFS, so `world/default/texture.zbd:sky` could not
be resolved at all; a lookup that searched every archive for the name would
give world `c2`'s `sky` to world `c1` whenever `c1` did not store it, and
`accept_f08_c_missing_or_duplicate_name_fails_visibly_without_fallback`
fails.

## Producer and consumer

- **Producer:** `TextureArchive::open(session, key)` resolves the key in a
  F04 `ContentSession`, reads it through F06-C's `ZbdContainer::open` (its
  dispatch must name the texture family; any other family is
  `wrong_family`, never re-read by the texture reader) and reads it with the
  production `read_zbd_textures`. The archive owns its bytes; each texture
  keeps its descriptor and the byte range of its stored level.
- **Consumer:** `TextureCatalog::prepare_upload` decodes with the production
  `decode_base_level` and hands a `TextureUpload` to the renderer adapter
  (F17-B): one level, rows top first, tightly packed
  (`row_bytes = width * channels`), values exactly as decoded (565 words
  stay words, palette indices and the alpha plane stay separate), plus the
  list of `PresentationUnknown`s. This is the single boundary where a later
  adapter converts; nothing here converts (non-negotiable #3).

## Decisions

- **Identity** (non-negotiable #4): `TextureId` = installation-relative
  archive path + serving mount + key variant + entry index + stored name.
  The same name in two chapter archives gives two unequal ids.
- **Lookup** (contract "Lookup contract"): a `TextureRef` is an archive key
  plus an exact stored name. The world is chosen by the session's context
  through the VFS (F04's designed mount layout); the name is looked up only
  in that one archive. Not stored: `texture_not_found`; stored twice:
  `duplicate_texture_name` with both entry indices. No case folding, no
  aliases, no fallback to `rtexture*.zbd`, `rimage.zbd` or another world.
  The result carries the ordered attempts: the VFS trace (with the other
  world's mount skipped) and the name lookup.
- **Catalog rows:** one per texture (`kind` `image`, archive origin span,
  archive dependency, parse/normalize state, consumer `gpu_upload`,
  readiness, unsupported reasons, SHA-256 of the stored level) and one per
  failed archive with its error code. A repeated key is opened once.
- **Readiness:** `DecodedWithUnknowns` unless every presentation decision is
  established. For the ZBD package that is never the case today: color
  space and alpha test are `Unknown` for every texture, 565 expansion and
  stretch are unmeasured, and simple alpha on palette textures has an
  unknown alpha source. These are the row's `unsupported_reasons`.
- **Stale state:** the catalog is stamped with the session generation; a
  foreign session is `foreign_session` for `resolve`, `decode`,
  `prepare_upload` and `retry_failed`; a `ResolvedTexture` from another
  catalog is `not_from_this_catalog`. Uploads own their values and survive
  `ContentSession::close`.
- **Retry:** `retry_failed` reopens only failed archives in the same
  session. Because F04 pins every mounted file's digest, a file repaired on
  disk still fails in that session (`changed_on_disk` / digest mismatch);
  a remount (new session, new catalog) loads it. Tested both ways.
- **Scope:** only the ZBD texture package is catalogued. The BMP
  (`00000409.*`) and TGA (`GOSDATA/ASSETS/GRAPHICS/*.tga`) readers have no
  catalog role here: which consumer loads them is not yet established.
  `tools/cs_inspect/src/textures.rs` is not created; the private contact
  sheet and full audit are F08-D.

## Retail observation (tool observation, not an evidence report)

`accept_f08_c_retail_same_name_textures_resolve_per_world` discovers the
installation, opens sessions for world groups `ZBD/C1` and `ZBD/C2` with the
production mount layout, and catalogs `world/default/texture.zbd` in each.
Result on this machine (installation as fingerprinted on T340,
`b4e780ab…1978`): **641** names are stored exactly once in both archives;
each resolves to its own world's archive, and its upload equals what
`read_zbd_textures` + `decode` give for that entry of the file read directly
from the host. **8** of the 641 store different bytes in the two worlds.
The test takes about four minutes in a debug build, almost all of it
installation hashing.

## Tests

| Test | Covers |
| --- | --- |
| `accept_f08_c_same_name_texture_in_two_chapter_archives_resolves_per_world` | AC03: two worlds, same key and name, own origin, entry, trace, texels; distinct ids |
| `accept_f08_c_missing_or_duplicate_name_fails_visibly_without_fallback` | no fallback to the other world, exact names, duplicate entries, uncatalogued archive |
| `accept_f08_c_upload_keeps_alpha_and_indices_and_lists_presentation_unknowns` | upload layout, alpha plane, palette indices, unknown list |
| `accept_f08_c_world_switch_refuses_stale_catalog_and_resolution` | foreign session, foreign catalog, upload after close |
| `accept_f08_c_failed_archive_is_a_catalog_row_and_recovers_after_remount` | failed rows (trailing byte, unresolvable key, non-package), retry, remount, texture rows |
| `accept_f08_c_container_of_another_family_is_refused` | dispatch family checked before reading |
| `accept_f08_c_retail_same_name_textures_resolve_per_world` (ignored) | the retail scenario above; panics without `CS_GAME_DIR` |

Mutation probes (each applied, `cargo test -p cs_content -- accept_f08_c_`
run, then restored):

| Mutation | Result |
| --- | --- |
| a duplicate name resolves to its first entry | 1 test fails |
| generation check removed | 2 fail |
| failed archives left out of `records` | 1 fails |
| dispatch family not checked | 1 fails |
| id without the archive path | 1 fails |
| stored level range shifted by one byte | 4 fail |

## Recorded unknowns

- Which texture archives a mission actually uses: `texture.zbd` versus the
  `rtexture2/4/6/8/14/15.zbd` tiers of the same world, and when
  `rimage.zbd` is consulted. The catalog takes the archive keys from its
  caller and never picks one itself; the selection rule belongs to the
  consumer that knows it (renderer settings or scene data) and needs
  evidence.
- Whether the original engine binds `texture.zbd` to the world group the
  way F04's designed layout does (F04-D: unmeasured).
- Every presentation unknown listed above (color space, alpha test, 565
  expansion, stretch, simple alpha on palette textures) — unchanged from
  the F08-B findings.
