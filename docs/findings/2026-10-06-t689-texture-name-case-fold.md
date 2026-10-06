# Task #689: the measured texture-name case fold

Date: 2026-10-06. Task: #689 (`F08-C-resolve-name-case-fold`), the
follow-up that closed F08-C's recorded divergence "the name lookup folds case;
`TextureCatalog::resolve` does not"
(`docs/findings/2026-10-05-t352-texture-archive-selection-rule.md`,
"Recorded unknowns"), which the same file had filed as this task. Test prefix:
`accept_f08_c_case_fold_`. Owner paths:
`crates/cs_content/src/textures.rs`, `docs/findings/`.

**Outcome: the fold is adopted.** `TextureCatalog::resolve` now folds the
requested name the way the original does and compares the folded spelling
against the archive's **stored** spellings byte for byte; `TextureId` and the
catalog rows keep the archive's own spelling, unchanged. One function,
[`folded_texture_name`], is the fold, and both `resolve` and
`texture_lookup_order` call it, so they cannot drift apart.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/textures.rs` (owner path): the new production
  function `folded_texture_name`, `TextureCatalog::resolve`'s comparison and
  its `TextureAttempt::Name` value, `texture_lookup_order`'s fold,
  `TextureRef::name`'s and `TextureAttempt::Name`'s documentation, the module
  docs, and the `accept_f08_c_case_fold_` tests.
- Wiring only: none outside that file.

**One observable failure:** before this task `resolve` compared stored names
**exactly**, so the request `"SKY"` returned `texture_not_found` for an archive
that stores `sky`. The original folds the request and finds it. One observable
behaviour, one file, one focused path — no split needed.

## Sources

### Static code evidence, re-verified on this machine

| Item | Value |
| --- | --- |
| Decrypted image | `$CS_GAME_DIR/crimson.decrypted.exe` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` (re-computed 2026-10-06; matches the digest T352 recorded) |
| Method | `llvm-objdump -d` over the image at the recorded addresses; the two import slots named by `r2 -c 'ii'` |

The owner's static analysis (Kuna v1.692, per the T352 finding) located the
fold; this task re-derived the instruction sequence itself rather than taking
the claim on trust. Addresses are virtual addresses in that image; below
`0x643000` the file offset of `.text` is VA − `0x400000`. The image is
read-only and **not** committed, and neither are its bytes, its strings nor any
decompilation: this file records addresses, constants, counts and behaviour
only. This is static code evidence, not a runtime capture: nothing here may be
recorded as `verified_original`.

### Retail data used in this task

| Item | Value |
| --- | --- |
| Installation fingerprint | as fingerprinted on T340/T343/T346 (`b4e780ab…1978`) |
| Read access | read-only parse of every texture archive's 0x18-byte header and name table |
| Census manifest digest | `689c46da553d1d62cdcd048cc94d8991a3688da294d7ad82ad686593d62d60b6` over `relative path : version : palettes : entry count` for all 49 archives |

**Every** texture-family archive in the installation was censused, not only the
three the T352 finding listed: all eight world groups' `texture.zbd` and every
`rtexture*.zbd`, plus the shared `rimage.zbd` — 49 archives, **37 004** stored
names. Every one is version 1 with 0 global palettes, and:

* **0** stored names are not already the folded spelling (i.e. every stored
  name is lower case);
* **0** pairs of stored names in one archive fold onto the same key, so the
  fold introduces no duplicate and merges no two candidates;
* **all 49** name tables are sorted ascending byte-wise, which is what the
  original's binary search assumes and what lets this crate's linear scan
  reach the same entry.

Per-archive entry counts (version 1, 0 palettes throughout):

| Archive | Entries | Archive | Entries |
| --- | --- | --- | --- |
| `ZBD/C1/texture.zbd` | 881 | `ZBD/C3/texture.zbd` | 732 |
| `ZBD/C1B/texture.zbd` | 667 | `ZBD/C4/texture.zbd` | 935 |
| `ZBD/C1C/texture.zbd` | 593 | `ZBD/C5/texture.zbd` | 896 |
| `ZBD/C2/texture.zbd` | 820 | `ZBD/C1/rtexture15.zbd` | 881 |
| `ZBD/C2B/texture.zbd` | 601 | `ZBD/C1/rtexture2/4/6/8.zbd` | 881 each |
| `ZBD/C1B/rtexture11.zbd` | 667 | `ZBD/C1B/rtexture2/4/6/8.zbd` | 667 each |
| `ZBD/C1C/rtexture10.zbd` | 593 | `ZBD/C1C/rtexture2/4/6/8.zbd` | 593 each |
| `ZBD/C2/rtexture14.zbd` | 820 | `ZBD/C2/rtexture2/4/6/8.zbd` | 820 each |
| `ZBD/C2B/rtexture9.zbd` | 601 | `ZBD/C2B/rtexture2/4/6/8.zbd` | 601 each |
| `ZBD/C3/rtexture12.zbd` | 732 | `ZBD/C4/rtexture14.zbd` | 935 |
| `ZBD/C4/rtexture2/4/6/8.zbd` | 935 each | `ZBD/C5/rtexture14.zbd` | 896 |
| `ZBD/C5/rtexture2/4/6/8.zbd` | 896 each | `ZBD/rimage.zbd` | 254 |

## The measurement (`0x531930`)

The in-archive name search, re-read instruction by instruction:

1. `0x531930` allocates 0x104 bytes of stack, pushes `ebx/ebp/esi/edi`, and
   calls `0x531280` — the header read, which leaves the sorted directory at
   `+0x9c` and the entry count at `+0x90`. A false return jumps to the
   not-found exit.
2. `0x53194d`–`0x53196c`: `strlen` the requested name (`repne scasb`), then
   `rep movsl`/`rep movsb` **a copy of it** into the local buffer at
   `esp+0x14`. The caller's string is not modified.
3. `0x531975`–`0x5319c9`: for every byte of the copy except the terminator,
   `movsbl` it into `edi`, `call *0xa201a0`, and if the result is non-zero
   `call *0xa201cc` and store the byte back. `r2`'s import table names those two
   slots **`MSVCRT.dll!isupper`** (`0xa201a0`) and **`MSVCRT.dll!tolower`**
   (`0xa201cc`). This is the C runtime's `strlwr` loop, expanded inline.
4. `0x5319cf`–`0x531a32`: a binary search over the directory. `+0x90` gives the
   count, `+0x9c` the table; `leal (%edx,%ebp),%edi; sarl %edi` is the midpoint,
   `leal (%ecx,%eax,8)` with `leal (%edi,%edi,4),%eax` is `index * 40` — the
   0x28-byte entry stride — and the comparison at `0x5319f7` is an inline
   two-bytes-at-a-time `strcmp` of the folded buffer against `name[0]`, `name[1]`,
   … The match branch at `0x531a44` returns the index; the not-found exit
   returns `-1`.

So the original **folds the request and nothing else**: the stored names go
into the directory verbatim (`0x531280` copies the table in, and the palette
post-pass at `0x5313ac`–`0x5314b8` only touches `+0x8c`'s palettes), and the
comparison is an exact byte comparison against them. Two consequences:

* `to_ascii_lowercase`, not a Unicode lower-casing. `isupper`/`tolower` in the
  C locale touch `A`–`Z` and nothing else, so `İ`, `ẞ` and `À` pass through
  unchanged. (A name table is `char name[0x20]` of NUL-padded ASCII by
  construction — F08-B.02's reader — so a non-ASCII request cannot match a
  stored name under any spelling.)
* **Not** a case-insensitive comparison. An archive that stored `Sky` would be
  unreachable by `Sky`, by `sky` and by every other request, because the request
  folds to `sky` and `sky` is not what is stored. Retail stores no such name
  (0 of 37 004), so the rule has no retail consequence; it is implemented
  because it is what the code does.

## The decision

Adopt the measured fold, and nothing else:

| Aspect | Behaviour in force | Why |
| --- | --- | --- |
| Fold applied to | the request only | `0x531930` copies the request and folds the copy; the stored table is copied verbatim by `0x531280` |
| Fold function | ASCII, C locale | `isupper`/`tolower` through the two MSVCRT slots |
| Stored names | never folded; `TextureId::name` and catalog rows keep the archive's spelling | the comparison is byte-exact and identity is the archive's own text (F08 non-negotiable #4) |
| Comparison | exact bytes against the stored spelling | the inline `strcmp` at `0x5319f7` |
| Aliases | none | a fold is not an alias table; F08 non-negotiable #4 requires source evidence and a collision test for one, and there is none |
| Search scope | unchanged: one archive, no cross-archive fallback | T352's "no tier-to-tier fallback" |
| Duplicate rule | unchanged: two entries holding the folded name are `duplicate_texture_name` with both indices | F08-C's visible collision refusal; the fold cannot create one on retail (0 fold collisions) |

The implementation is one function, `folded_texture_name`, called by both
`TextureCatalog::resolve` and `texture_lookup_order`. The "agreement" the task
requires is therefore structural rather than a convention: there is no second
spelling rule in the crate to disagree with.

### Why this overrules the F08-C exact-match assertion

F08-C decided "no case folding, no aliases" and asserted it in
`accept_f08_c_missing_or_duplicate_name_fails_visibly_without_fallback`
(`resolve("SKY")` → `texture_not_found`). That assertion was correct **about
the code at the time** and wrong **about the original**: it encoded an
unmeasured choice as if it were a measurement, and T352's finding later
recorded that the original does fold. A measurement overrules it. The test's
name still describes what it now proves — a name the archive does not store
fails visibly without a fallback — and the case assertion inside it changed
from *"no case folding"* to *"the fold is not a fallback"*: `resolve("SmOkE")`
folds to `smoke`, which world one does not store, and must still be
`texture_not_found` rather than reaching world two's archive.

Nothing else in that test changed, and no other F08-C criterion was touched:
AC03 (`accept_f08_c_same_name_texture_in_two_chapter_archives_resolves_per_world`),
the upload boundary, the stale-state refusal, the failed-row and family
refusals are all unchanged and still pass.

## What this breaks outside this task's owner paths

`cargo test --workspace --locked` is **not** green on this branch. Exactly one
test fails, and it is in `crates/cs_content/src/mesh.rs`, which is F10's owner
path (`specs/F10-gamez-mesh-topology-and-material-records.md`), not this task's:

```
mesh::tests::accept_f10_c_02_audit_reports_a_missing_texture_with_its_exact_name_and_archive
panicked at crates/cs_content/src/mesh.rs:5041: the second archive stores `Sky1.tif`
```

Its fixture stores a mixed-case `Sky1.tif` and then asserts that
`resolve("Sky1.tif")` succeeds against it. Under the measured rule that
assertion is false of the original as well as of this crate: the request folds
to `sky1.tif`, which the archive does not store, so no spelling of that request
reaches the stored `Sky1.tif` (`0x531280` reads the table verbatim). The
sibling test `accept_f10_c_02_audit_neither_folds_case_nor_strips_an_extension`
still passes and keeps its meaning.

This is filed as **#702**, which owns the fix and the now-incorrect "byte
equality" prose at `mesh.rs:1596`. It depends on this task, so it becomes
ready once #689 lands. This task does not touch that file: doing so would mean
editing another feature's owner path and its acceptance test, which is out of
scope here, and the conflict is a consequence of the measurement rather than a
defect in this change.

## Recorded unknowns and limitations

* **The loose-file names are built from the folded spelling here, and from the
  unfolded request in the original.** `0x534cf0` is `sprintf(buf, "%s.tif",
  name)` on the caller's string (the format literal `"%s.tif"` sits at
  `0x635578`), and `0x534060` does the same for `.bmp`; the original never
  folds before those two. This crate folds first. The two cannot give different
  answers on a host file system that folds case — which is every host the
  original ran on — and differ only if one directory holds both `sky.tif` and
  `SKY.tif`. Retail ships **no** loose `.tif` or `.bmp` under any `ZBD`
  directory (checked by listing, 2026-10-06), and the loose-file interface
  itself is already recorded as unestablished by F08-B.03/.04 and F08-C, so
  this is a bounded divergence on content that does not exist, not a claim that
  the paths are equal.
* **The file probe's case handling is not established** (unchanged from T352):
  `TextureFiles::find` compares candidate names exactly. Every candidate the
  crate generates is lower case, so this stays equivalent for retail, but a
  listing spelled in another case is not covered.
* **A duplicate is refused, the original would pick one.** The original's binary
  search returns *a* matching index; this crate returns `duplicate_texture_name`
  naming every holder, because F08-C chose to refuse a silent choice between
  equal candidates. That is stricter than the original on purpose and is
  unchanged here. No retail archive stores a name twice (0 exact duplicates in
  37 004 names), so nothing retail is affected.
* **Not a runtime observation.** Everything above is static code evidence plus a
  data census. No claim here may be recorded as `verified_original`, and the
  fold's effect on a live original run is unobserved.

## Tests

| Test | Covers |
| --- | --- |
| `accept_f08_c_case_fold_a_mixed_case_request_resolves_the_folded_stored_name` | `sky`/`SKY`/`Sky`/`sKy` all resolve entry 1 of an archive that also stores `Sky`; the `TextureAttempt::Name` value is the folded spelling; identity keeps `sky` and the archive path; the upload decodes the stored texels; all four spellings yield one id |
| `accept_f08_c_case_fold_folds_the_request_and_not_the_stored_name` | a stored `Mixed` is unreachable by `Mixed`/`mixed`/`MIXED`/`mIxEd` (the fold is not a case-insensitive comparison); the fold is ASCII-only against `İ`, `ẞ`, `ÀÉÎ`, `Straße`; `resolve` and `texture_lookup_order` fold through the same function, so the searched spelling equals the loose file name's stem |
| `accept_f08_c_case_fold_a_stored_name_is_never_merged_by_the_fold` | identity keeps all five stored spellings (`Sky`, `sky`, `Mixed`, `dup`, `dup`) as five ids; `Sky` and `sky` stay distinct; a name stored twice is still `duplicate_texture_name` naming both entries for `dup`/`DUP`/`Dup`; the catalog rows are unchanged by the fold |
| `accept_f08_c_case_fold_retail_stored_names_are_already_folded` (ignored) | the census on the real installation, all 49 archives: every stored name is already the folded spelling, no two fold onto one, every table is sorted; 64 mixed-case requests per archive reach the same entry, id and upload as the lower-case request |

The three synthetic fixtures are newly authored bytes built in the test
module; the retail test reads `$CS_GAME_DIR` only and commits nothing from it.

Retail result on this machine: 49 archives, 37 004 stored names, all already
the folded spelling and pairwise distinct under it, all tables sorted, and
3 264 mixed-case requests (64 × 49 archives, plus `rimage.zbd`) reached the
stored entry with an identical upload. The test takes about six minutes in a
debug build, almost all of it the installation walk.

## Mutation probes

Each mutation was applied, `cargo test -p cs_content --locked --lib --
accept_f08_c_case_fold` run, and the file restored:

| Mutation | Result |
| --- | --- |
| `resolve` compares the request exactly again (the pre-task code) | 2 fail |
| `resolve` uses `eq_ignore_ascii_case` on both sides | 2 fail |
| `folded_texture_name` uses Unicode `to_lowercase` | 1 fails |
| `resolve` folds the stored names too, matching either case | 1 fails |
| `resolve` records the unfolded request in `TextureAttempt::Name` | 2 fail |
| `texture_lookup_order` folds through `str::to_lowercase` again | 1 fails |
| the fold made into an alias that falls back to another archive | 2 fail |

## Checks run on this branch (2026-10-06)

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | clean |
| `cargo test --workspace --locked` | **one pre-existing F10-C test fails** — `mesh::tests::accept_f10_c_02_audit_reports_a_missing_texture_with_its_exact_name_and_archive`. Everything else passes (232 passed, 1 failed, 33 ignored in `cs_content`'s lib; the rest of the workspace green). See "What this breaks outside this task's owner paths" and #702 |
| `cargo test --workspace --locked -- --skip <that one test>` | the rest of the workspace is green |
| `cargo test --workspace --locked -- accept_f08_c_case_fold --include-ignored` | 4 passed, 0 failed (3 synthetic + the retail one) |
| `cargo test -p cs_content --locked --lib -- accept_f08_c` | 16 passed, 0 failed, 3 ignored — every F08-C test, including the one whose case assertion changed |
| `env -u CS_GAME_DIR cargo test -p cs_content --locked --lib -- accept_f08_c_case_fold --include-ignored` | the retail test **fails** with "CS_GAME_DIR must point at the original installation for this test"; the three synthetic ones pass |

No evidence report: this task's retail use is a header and name-table census a
test observes, following F08-C, F08-B and T352's precedent. The fingerprinted
decode audit against a pinned reference is F08-D.
