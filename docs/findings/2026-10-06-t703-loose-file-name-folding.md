# Task #703: the loose `.tif`/`.bmp` probes are the request, not the fold

Date: 2026-10-06. Task: #703 (`F08-C-loose-file-name-folding-divergence`), the
follow-up that #689's finding filed under "Recorded unknowns and limitations".
Test prefix: `accept_f08_c_loose_name_`. Owner paths: `crates/cs_content/src/textures.rs`,
`docs/findings/`.

**Outcome: the fold is dropped from the loose probes.**
`texture_lookup_order` now builds `<name>.tif` and `<name>.bmp` from the
caller's own string, which is what the original does; it folds nothing,
because the fold it used to apply lived only in those two `format!` calls —
the in-archive step of this function lists the archives and matches no name.

## Why option 2

The task offered three options and no owner ruling arrived, so the choice is
recorded here with its reason:

| Option | Why not |
| --- | --- |
| 1. keep the fold, leave the finding as the record | keeps a behaviour the `0x534cf0` reading contradicts, and the acceptance criterion requires the chosen behaviour to be asserted *against* that reading |
| **2. build the loose names from the unfolded request** | **the faithful option, and the only one whose assertion the reading supports** |
| 3. do nothing until the loose-file interface is established | the interface being unestablished bounds the *consequence*, not the reading: the divergence is measured, small, and closed at the same cost as option 1 |

Retail never reaches the path either way (0 loose `.tif`/`.bmp`, below). In
the **original** the two probe spellings reach the same file on every host it
ran on — those fold case — unless one directory holds both `sky.tif` and
`SKY.tif`. In **this crate** they differ more widely, because
`TextureFiles::find` compares the candidate against the listing exactly: the
request `SKY` is answered by the listing `sky.tif` under option 1 and by no
loose file under option 2. What the original answers for that pair depends on
the file probe's own case handling, which stays recorded as unestablished
(T352; `TextureFiles::find`'s doc records that the host file system folds
case). Option 2 therefore buys a measured probe string with an unestablished
match — the trade is bounded and recorded, not hidden: retail ships no loose
`.tif`/`.bmp`, and the loose-file interface is still unestablished. It was
chosen because it is what the code does, not because retail needs it.

## Sources

### Static code evidence, re-derived on this machine

| Item | Value |
| --- | --- |
| Decrypted image | `$CS_GAME_DIR/crimson.decrypted.exe` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` (computed 2026-10-06; matches the digest T352 and T689 recorded) |
| Method | `llvm-objdump -d` over the image at the recorded addresses, `llvm-objdump -s`/`xxd` on the two format literals, `r2 -c 'ii'` for the import slot |

The addresses and the reading came from the task description, which records
them from #689's finding; this task re-derived each of them rather than taking
them on trust. The image is read-only and **not** committed, and neither are
its bytes, its strings nor any decompilation: this file records addresses,
constants and behaviour only. This is static code evidence, not a runtime
capture: nothing here may be recorded as `verified_original`.

**`0x534cf0` — the `.tif` probe**

```
534cf0: movl  0x4(%esp), %eax        ; the caller's argument, untouched
        subl  $0x40, %esp
        pushl %ebx / %esi / %edi
        pushl %eax                   ; the third printf argument
        leal  0x10(%esp), %ecx       ; the local buffer
        pushl $0x635578              ; the format literal
        pushl %ecx                   ; the buffer
        calll *0xa20210              ; MSVCRT.dll!sprintf (r2 `ii`: slot 0xa20210)
```

`sprintf(buf, "%s.tif", name)` — the bytes at VA `0x635578` (file offset
`0x235578`) are `25 73 2e 74 69 66 00` = `%s.tif`. The pointer is pushed with
no write to it, so the caller's buffer is never folded here.

**`0x534060` — the `.bmp` probe**

The same shape: `movl 0x4(%esp), %eax` → `pushl %eax` → `pushl $0x6353b0` →
`pushl %ecx` (buffer) → `calll *0xa20210`. The bytes at VA `0x6353b0` (file
offset `0x2353b0`) are `25 73 2e 62 6d 70 00` = `%s.bmp`. The `%s.bmp` literal
at `0x6353b0` is **new here**: the task description named only the `.tif`
literal's address.

**`0x531b60` — one pointer, four calls**

```
531b6e: movl  0xc(%esp), %edi        ; the name argument
531b72: pushl %edi
531b7d: calll 0x531900               ; in-archive search, first list
531bac: pushl %edi
531bb2: calll 0x531900               ; in-archive search, second list
531bcb: pushl %edi
531bcc: calll 0x534cf0               ; sprintf(buf, "%s.tif", name)
531bda: pushl %edi
531bdb: calll 0x534060               ; sprintf(buf, "%s.bmp", name)
```

The same register `%edi` goes to all four, and nothing writes to that buffer
in between — so the name reaches `0x534cf0`/`0x534060` exactly as the caller
spelled it, while `0x531900` → `0x531930` folds a **private copy** (recorded
in #689's finding, and this function's doc). `0x531b73`'s `movl $0x1,
0x758238` is the two-list toggle the existing doc comment already describes.

### Retail data used in this task

| Item | Value |
| --- | --- |
| Listing | `find "$CS_GAME_DIR/ZBD" \( -iname '*.tif' -o -iname '*.bmp' \)`, 2026-10-06 |
| Result | **0 files** |

So no retail content reaches the loose probes. The loose-file interface itself
stays recorded as unestablished by F08-B.03/.04, F08-C and T352's "Recorded
unknowns" — whether the game uses loose files at all, and which reader serves
them, is still open, and nothing in this task changes that.

## What changed

- `crates/cs_content/src/textures.rs` (owner path): `texture_lookup_order`
  probes `format!("{name}.tif")` and `format!("{name}.bmp")` — the `folded`
  binding is gone, because the order matches no name inside an archive;
  `TextureLookupSource::LooseTiff`/`LooseBmp`, `texture_lookup_order` and
  `TextureFiles::find` say whose spelling the candidate carries; the
  `accept_f08_c_selection_…` mixed-case assertion changed and
  `accept_f08_c_loose_name_…` is new.
- Also in that file, adapting to #689, which merged onto `main` while this
  branch was being written: the module doc and `folded_texture_name`'s doc no
  longer say `texture_lookup_order` folds, and
  `accept_f08_c_case_fold_folds_the_request_and_not_the_stored_name` now
  asserts the split the original has — `resolve` searches the folded spelling,
  the loose probes take the request verbatim — instead of asserting the two
  agree. Its `İ` block now proves `SKYİ` misses the listed `skyİ.tif` (which
  is what the original does) rather than that `texture_lookup_order` folds
  ASCII-only.
- `docs/findings/2026-10-06-t689-texture-name-case-fold.md` (owner path):
  the three places that said both functions fold through `folded_texture_name`
  are struck and amended, the "Recorded unknowns" bullet the task names is
  retired as **Closed by task #703**, and the file-probe bullet now says which
  candidates still carry a lower-case spelling.
- Wiring: none outside those two files.

## Tests

| Test | Covers |
| --- | --- |
| `accept_f08_c_loose_name_the_loose_file_name_is_the_request_verbatim` | a listing holding `SKY.tif`/`SKY.bmp` is reached by the request `SKY` and **not** by `sky`; a listing holding `sky.tif` is not reached by `SKY`; a `.bmp`-only directory is probed under the request's spelling; `SKYİ` reaches a listed `SKYİ.tif` while `skyİ` does not (no fold of any kind, ASCII or Unicode); the two archives a request does not address are listed identically for `sky`/`SKY`/`Sky`/`SKYİ` |
| `accept_f08_c_selection_the_lookup_order_is_world_archive_image_archive_then_loose_files` | the four-source order, the key of each source, the `.bmp` fallback and the order following the selected archive — plus the changed assertion: `SKY` against a listing holding `sky.tif` yields the two archives alone, "the request is not folded into the loose probe" |
| `accept_f08_c_case_fold_folds_the_request_and_not_the_stored_name` (#689's test, adapted) | the split the original has: `resolve` searches the folded spelling for every request, while the loose probes carry the request itself — `sky` reaches the listed `sky.tif`, `SKY`/`Sky` reach nothing loose, and `SKYİ` misses the listed `skyİ.tif` that only the request `skyİ` reaches |

All three keep proving something discriminating: the second one's mixed-case
assertion used to prove the fold and now proves its absence against the same
`0x534cf0` reading, and the third used to prove that the catalog's fold and the
loose probes shared one spelling — it now proves they do not, which is what the
original does.

## Mutation probes

Each mutation was applied to `crates/cs_content/src/textures.rs`, the
selection run (`accept_f08_c accept_f10_c_02` in `cs_content`'s lib), and the
file restored from a copy:

| Mutation | Result |
| --- | --- |
| re-fold the loose names (`name.to_lowercase()` in both `format!` calls — the pre-task code) | **3 fail**: `accept_f08_c_loose_name_…`, `accept_f08_c_selection_…`, and #689's `accept_f08_c_case_fold_folds_the_request_and_not_the_stored_name` |
| drop both loose probes entirely | **4 fail**: the three above plus `mesh::tests::accept_f08_c_renderer_audit_walks_loose_files_and_keeps_the_sources_verdict` |

## Checks run on this branch (2026-10-06, on the head that was pushed)

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | clean |
| `cargo test --workspace --locked` | green: 404 suites, 0 failed |
| `cargo test --workspace --locked -- accept_f08_c_loose_name --include-ignored` | green: 404 suites ok, 1 test matched and passed (`textures::tests::accept_f08_c_loose_name_the_loose_file_name_is_the_request_verbatim`) |
| `cargo test -p cs_content --locked -- accept_f08_c accept_f10_c --include-ignored` | green: 76 passed, 0 failed, 0 ignored — every F08-C and F10-C test including the retail ones (the 49-archive case-fold census, the retail world-group selection and the F10-C world audit), because this branch also adapts #689's case-fold test |

## What still names the old behaviour

The paragraph this task was told to retire was not on `main` when the branch
was cut — #689 was still in review — so it was amended here once #689 landed:

* `docs/findings/2026-10-06-t689-texture-name-case-fold.md` — **retired**:
  the "Recorded unknowns and limitations" first bullet is struck and marked
  **Closed by task #703**, and the two other sentences that said `resolve` and
  `texture_lookup_order` fold through one function are struck and amended the
  same way. Its mutation-probe row for the now-removed fold is annotated
  rather than deleted, because those probes are a record of a run.
* `docs/findings/2026-10-05-t352-texture-archive-selection-rule.md` — **not
  edited**: its "Settled by task #689" bullet still says "both it and
  `texture_lookup_order` fold through that one function", and its next bullet
  still says "Every candidate this crate generates is lower case". Both are
  false now. That file is not this task's owner path, so the correction is
  recorded here and handed over instead of made: two sentences, and #689
  edited the same file when its own change invalidated the same bullet, so a
  follow-up is a one-line job.

Everything else that names the old behaviour is a historical record of a run
or of what #689 changed at the time, and stays as written.

The two claims that survive are unchanged: the in-archive search folds
(`TextureCatalog::resolve` does, through `folded_texture_name`), and the file
probe's own case handling is unestablished — what this task changes is only
which spelling is handed to that probe.

No evidence report: this task's retail use is a directory listing and a
static-code re-derivation a test observes, following F08-C, F08-B, T352 and
T689's precedent.
