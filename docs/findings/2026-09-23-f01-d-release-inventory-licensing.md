# F01-D: release-inventory provenance check and the vertical-slice licensing review

Date: 2026-09-23. Task: F01-D "Review licensing and factual provenance for
the first vertical slice" (`specs/F01-evidence-ledger-provenance-and-reference-policy.md`,
`### F01-D`). Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
Capabilities used: ordinary build/test for the deliverable; the `retail`
capability additionally powered the positive control described below (the
installation was opened read-only; nothing from it was copied or committed).

## Files and the one observable failure

- `crates/cs_types/src/evidence.rs` (extended): `InventoryEntry`,
  `ProhibitedContent`, `InventoryMatch`, `InventoryViolation`,
  `InventoryReport`, `check_release_inventory`, the signature tables
  (`EXECUTABLE_SIGNATURES`, `GAME_DATA_SIGNATURES`, `MEDIA_SIGNATURES`,
  `DOCUMENT_SIGNATURES`) and the extension tables
  (`EXECUTABLE_EXTENSIONS`, `GAME_DATA_EXTENSIONS`, `MEDIA_DOC_EXTENSIONS`).
- `tools/cs_inspect/src/evidence.rs` (extended): `AUTHORED_CONTENT_ROOTS`
  (this repository's declared authored root, `fixtures/synthetic`), the
  producers `scan_inventory_dir` and `committed_inventory` (git
  `ls-files` + worktree bytes), `audit_release_inventory` wiring and
  `InventoryError`.
- `tools/cs_inspect/tests/accept_f01_d_release_inventory.rs` (new):
  eleven `accept_f01_d_*` tests over the production path.
- Observable failure if the implementation is removed or stubbed: a
  committed inventory containing a PE executable next to ordinary sources
  reports clean (AC04). Covered by
  `accept_f01_d_committed_retail_binary_is_detected`; making
  `check_release_inventory` return an empty `violations` list fails six of
  the eleven tests.

## Design decisions

- **Detection is signature + name + provenance, never extension alone.**
  The committed fixture set legitimately contains authored `.bm`/`.rof`/
  `.interp` binaries — `synthetic.interp` even carries the real INTERP
  signature `0x08971119` — so a retail-format signature cannot condemn a
  file by itself. What condemns an entry is *where* it is: outside the
  declared authored roots, game-data signatures/names, media and document
  signatures/names, and any non-text blob are all prohibited.
- **Executables are exempt from the exemption.** `MZ` (covers PE/COFF
  `.exe`/`.dll`/`.icd`), ELF and Mach-O signatures plus executable
  extensions condemn an entry *before* the authored-root check — no
  authored fixture is an executable image, so a committed binary cannot
  hide under `fixtures/synthetic`.
- **Text-compatible document formats are checked on text too.** PDF and
  RTF decode as text, so their signatures run regardless of the `text`
  verdict; binary-only signatures require a binary entry, which keeps a
  source file that merely begins with `MZ`-looking letters unflagged.
- **Unidentified binary is a violation, not a shrug.** Any non-text entry
  outside the authored roots that no rule recognized is condemned as
  `UnidentifiedBinary`: a retail file renamed to `data.bin` cannot slip
  through by dropping its signature and extension. (In this repository
  every committed binary lives under `fixtures/synthetic`, so the rule is
  precise today.)
- **The committed inventory is `git ls-files`, not a directory walk.**
  Tracked files only: `target/`, `private/` and other ignored output can
  never enter the inventory. Bytes are the worktree's, so a staged edit is
  scanned as it would be committed.
- **A mispointed root is an error, not an empty pass.** `git ls-files`
  under a *subdirectory* of a checkout exits 0 while listing only the
  tracked prefix — for `target/` that is the empty set, a vacuously clean
  report. `committed_inventory` therefore requires `rev-parse
  --show-toplevel` to resolve to the requested root
  (`InventoryError::NotCheckoutRoot`), and any git or I/O failure
  propagates instead of producing a report.
- **`text` is a bounded sample verdict.** Producers read up to
  `TEXT_SAMPLE_LEN` (8 KiB, the usual binary-sniff window); a truncated
  multi-byte sequence at the sample boundary does not condemn a longer
  text file. Whole-file purity is deliberately not claimed.
- **No content hashing here.** Identity matching against fingerprinted
  original files belongs to the F02 installation inventory; this gate
  needs only signatures, names and provenance.

## Integration evidence collected

### Committed tree (this checkout)

`committed_inventory` over the work-tree root + `audit_release_inventory`
(test `accept_f01_d_committed_tree_of_this_repo_is_clean`):

| observation | value |
|---|---|
| tracked files inventoried | 220 on the submitted tree (`git ls-files | wc -l` agrees) |
| violations | 0 |
| binaries present | only under `fixtures/synthetic/` (`*.bm`, `*.rof`, `*.interp`), exempt as declared authored content |

The committed tree holds no game executables, original data, bundled
media, fonts or manuals — matching the `NOTICE.md` provenance statement
with a check rather than a claim.

### Positive control: the owner's installation (retail capability)

`scan_inventory_dir` + `audit_release_inventory` over `$CS_GAME_DIR`
(read-only; counts and names only, no content leaves the directory):

| observation | value |
|---|---|
| entries scanned | 228 |
| violations | 227 |
| executable image | 24 (e.g. `crimson.exe` by `MZ` signature — extension-independent: `crimson.icd` is flagged the same way) |
| original game data | 185 (184 `*.zbd`/`*.rof` names plus the `interp.zbd` INTERP signature) |
| media/font/document | 18 (`*.mpg`, `*.tga`, `EULA.RTF`, `Readme.rtf`, the BMP-signature `00000409.016/.256`, …) |
| unidentified binary | 0 |
| clean | 1 — `EBUSetup.sem`, a zero-byte file |

Run by `accept_f01_d_retail_installation_is_flagged`
(`#[ignore = "requires CS_GAME_DIR"]`, passes locally under
`--include-ignored`, skipped by CI which has no retail capability). This
proves the detector is non-vacuous: the same code that passes the
committed tree flags every real retail file.

## Licensing review of the first vertical slice

- **Workspace license:** MIT (`[workspace.package] license`, `LICENSE`).
- **Dependency licenses** (`cargo metadata --locked`, 599 packages): all
  permissive — MIT, Apache-2.0, BSD-2/3-Clause, Zlib, ISC, CC0-1.0,
  Unlicense, Unicode-3.0, BSL-1.0, MIT-0 and combinations. The only
  copyleft-adjacent entries are two `r-efi` builds licensed `MIT OR
  Apache-2.0 OR LGPL-2.1-or-later` — an OR-disjunction, so the permissive
  options apply; recorded here rather than silently accepted. No
  GPL-only, AGPL, EUPL or proprietary-licensed crate is present.
- **Reference implementations:** `mech3ax` (EUPL-1.2, S02/S06/S17) and
  `crimsonskies2blend` (S03–S05, S08–S10) remain external references per
  `docs/research/SOURCES.md`; nothing is vendored and spec F01
  non-negotiable 3 (no EUPL code copied into permissive files without a
  reviewed licensing decision) holds — the committed tree contains only
  Rust/Python sources authored here, docs, JSON bindings and the declared
  synthetic fixtures.
- **Factual provenance:** the first vertical slice's claim fixture
  (`synthetic_claim_fixture`) carries `designed`, `observed_tool` and
  `documented` claims only — nothing asserts `verified_original`, and the
  record rules from F01-A would refuse one without a fingerprinted,
  located original observation. Format knowledge that is still
  `observed_tool` (ROF length semantics, INTERP semantics, BM plane
  meaning) stays labeled that way in `docs/research/FORMAT-NOTES.md`.

## Regressions discovered

None in existing behavior: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --locked --
-D warnings` and `cargo test --workspace --locked` were green on the base
commit before this change. One genuine defect was found *while testing
this slice* and repaired inside it: `git ls-files` under a checkout
subdirectory exits 0 with a truncated (empty for ignored dirs) listing,
which would have produced a vacuously clean inventory — the producer now
refuses non-work-tree roots (`NotCheckoutRoot`, covered by
`accept_f01_d_subdirectory_of_a_checkout_fails_loudly`). Review added a
second repair: `scan_inventory_dir` followed directory symlinks with no
visited set, so a cyclic link (`self -> .`) looped the walk forever — each
resolved directory is now scanned once, and linked directories wait until
every real directory has been walked so entries record the real name
(`accept_f01_d_scan_inventory_dir_survives_symlink_cycles`).

## Recorded open questions (not guessed)

- **Decompiled source and copied manuals have no signature.** The gate
  detects binaries, containers and document formats; recognizing lifted
  code or prose inside an otherwise-legitimate text file remains a human
  review duty. Recorded here rather than automated with a fake heuristic.
- **Whole-file text purity is sampled, not proven** (`TEXT_SAMPLE_LEN` =
  8 KiB); a file that turns binary only after the window would read as
  text. The header signatures still apply to its first 512 bytes.
- **Worktree bytes, tracked paths.** A file deleted from the worktree but
  still tracked fails `committed_inventory` loudly (`Io`); whether the
  gate should instead read index blobs is a later decision — scanning
  what would be committed errs on the safe side.
- **Known-retail-hash matching** does not exist yet: there is no recorded
  list of original-file SHA-256s to match inventory entries by identity.
  F02's hashed installation inventory can feed such a check later.
- **`synthetic.interp` legitimacy is asserted by location**, not by
  per-file registration: `fixtures/synthetic` is a protected path so its
  membership is owner-controlled, but the gate does not yet compare
  fixture contents against `expected.json` hashes.
