# F02-B: safe discovery, hashing and diagnosis

Date: 2026-09-24. Task: F02-B "Implement safe discovery, hashing and
diagnosis"
(`specs/F02-installation-discovery-versions-and-exhaustive-inventory.md`).
Capabilities used: `retail` (production discovery over the owner's
installation at `$CS_GAME_DIR`) and `synthetic` (authored fixture trees under
the system temporary directory).

## Files and the one observable failure

Pre-edit state: `crates/cs_assets/src/install.rs` contained only F02-A's
`DiscoveredFile`, `inventory` and the private `relative_spelling` root-strip;
`crates/cs_types/src/install.rs` held the F02-A schema and its module doc
still described the schema-only stage; `tools/cs_inspect/src/install.rs`
still holds only `synthetic_install_fixture` (untouched here — F02-C wires
the commands).

Functions added to `crates/cs_assets/src/install.rs`:

- `Sha256` (+ `Default`), `Sha256::new/update/finalize`, private
  `sha256_compress`, `SHA256_INITIAL`, `SHA256_ROUND_CONSTANTS`, `sha256`.
- `DiscoveryError` (`Inventory`, `RootUnavailable`, `UnreadableDirectory`,
  `UnreadableFile`, `FileChangedDuringDiscovery`, `Cache`) with `Display`,
  `Error`, `From<ManifestError>`, `From<CacheError>`.
- `SkipReason` (+ `label`), `SkippedEntry`, `REFERENCE_WORLD_GROUP_LEADS`,
  `Diagnosis`, `Discovery`, `discover`, `discover_with_cache`, private
  `discover_inner`, `Walked`, `walk_directory`, `hash_file`, `diagnose`.
- `fingerprint`, `content_fingerprint`.
- `CachedAnalysis`, `CacheError` (`FingerprintMismatch`, `UnknownKey`,
  `InvalidAnalysis`), `AnalysisCache` (`for_manifest`, `fingerprint`, `len`,
  `is_empty`, `is_valid_for`, `record`, `entry`, `apply_to`).

`crates/cs_types/src/install.rs`: module doc refreshed to say the F02-B
work lives in `cs_assets` and that families arrive with the format tasks;
no schema change.

Tests (all selected by `accept_f02_b_`, 8 in total):
`crates/cs_assets/tests/common/mod.rs` (`TempTree` fixture helper),
`accept_f02_b_sha256_hashing.rs` (2),
`accept_f02_b_diagnosis_and_safe_discovery.rs` (3, one `#[cfg(unix)]`),
`accept_f02_b_one_byte_edit_fingerprint_and_cache.rs` (2, the spec's minimum
scenario AC02), `accept_f02_b_retail_installation.rs` (1,
`#[ignore = "requires CS_GAME_DIR"]`).
`crates/cs_assets/tests/evidence_report_f02_b.rs` is the evidence harness;
it is deliberately *not* prefixed `accept_f02_b_` and is `#[ignore]`d.

Observable failure if the implementation is removed or stubbed: with the
fingerprint reduced to key+size hashing (digest column dropped),
`accept_f02_b_one_byte_edit_changes_fingerprint_and_invalidates_cache`
fails — a one-byte, length-preserving edit no longer changes the
installation fingerprint and the stale cache keeps being accepted.

## Mutation verification (test sensitivity)

Three plausible-shortcut mutations were applied to
`crates/cs_assets/src/install.rs`, each run, then reverted (the tree was
`git status --porcelain`-clean afterwards):

1. `fingerprint` hashes logical key + size but **not** the row digest
   → `accept_f02_b_one_byte_edit_changes_fingerprint_and_invalidates_cache`
   FAILED (1 passed; 1 failed). AC02 is discriminating.
2. `walk_directory` **follows** directory symlinks instead of reporting
   them → `accept_f02_b_symbolic_links_are_reported_and_never_followed`
   FAILED (2 passed; 1 failed). Outside-the-root content would enter the
   inventory.
3. `hash_file` returns a **constant zero digest** (the streaming read path
   discovery really uses) →
   `accept_f02_b_retail_installation_inventories_every_regular_file`
   FAILED at the independent `python3 hashlib` cross-check: "the production
   digest of .../EBUSetup.sem must match python3 hashlib". The one-shot
   `sha256` tests alone do not cover `hash_file`; the retail test does.

## Design decisions

- **Hand-rolled streaming SHA-256 (FIPS 180-4).** The owner paths of this
  task allow no `Cargo.toml` change, so no `sha2` dependency was added. The
  implementation is the published NIST specification, checked against four
  FIPS 180-4 vectors (empty, `abc`, the 56-byte two-block input, one million
  `a`) plus streaming-vs-one-shot agreement across awkward chunk sizes, and
  cross-checked against `python3 hashlib` on original installation files —
  an implementation this workspace does not ship, so a wrong hash cannot
  self-confirm.
- **Safe walk.** Directories are visited in sorted byte order (two runs walk
  identically); symlinks are never followed (a link could point outside the
  installation) but are always reported in `Diagnosis::skipped` with a
  `SkipReason`; non-regular entries (sockets/FIFOs/devices) are reported the
  same way instead of being opened; a missing or non-directory root, an
  unreadable directory or file, a non-UTF-8 name and a file that changes
  underneath the read all fail the whole run **by name**
  (`FileChangedDuringDiscovery` refuses a torn digest) rather than yielding
  an empty or partial inventory. Nothing is ever silently omitted
  (non-negotiable 4; IDENTITY-CONTENT "collections cannot exclude failed
  entries").
- **`hash_file` brackets the read with metadata.** The reported
  `size_bytes` is exactly the number of hashed bytes; if length or mtime
  moved during the read there is no coherent digest and the run fails. The
  documented residual race is a same-length rewrite inside the read window;
  the installation is owner-read-only in practice.
- **Two evidence hashes over the actual bytes.** `fingerprint` is SHA-256 of
  the manifest's canonical logical identity (keys, sizes, per-file digests —
  excludes host root, spelling case and every analysis column, per F02-A);
  `content_fingerprint` is SHA-256 of the per-file digests alone sorted by
  logical key, i.e. the `content_sha256` vocabulary of
  `schemas/evidence.schema.json`. Neither is an EXE version string.
- **The cache is derived, bound and two-sided.** `AnalysisCache` stores
  logical key → (content digest, recorded analysis), bound to one
  installation fingerprint. `apply_to` reuses nothing when the fingerprint
  moved (returns 0, rows fall back to the explicit unknown analysis), and an
  entry whose recorded digest disagrees with the row is not reusable even
  under a matching fingerprint. `record` refuses a mismatched fingerprint or
  an unknown key by name and re-validates the analysis through
  `InstallFileRecord::validate`, so a cache entry can never smuggle in a
  classification the manifest constructor would refuse. This is the spec's
  minimum scenario: a one-byte edit changes the fingerprint and invalidates
  every entry at once.
- **Diagnosis reports, never claims.** `zbd_dir`, `planes_zbd`,
  `world_groups` and `rof_candidates` are found case-insensitively with the
  original spellings preserved and sorted by logical key (non-negotiable 2);
  `world_groups` records *every* observed group, and
  `absent_reference_groups` reports which of `REFERENCE_WORLD_GROUP_LEADS`
  (`c1,c1b,c1c,c2,c2b,c3,c4,c5`) were not observed — absence is a report,
  never a claim that the installation is incomplete, and the lead list is
  explicitly reference leads, not the authoritative mission list
  (non-negotiable 3). The expected-content denominator is F02-D work.
- **Fresh rows carry explicit unknowns.** Discovery sets `family: None`,
  `FileRole::Unknown`, `ParseState::Unparsed`: family detection belongs to
  F05/F06/F07 and classification to F02-D (non-negotiable 4), so this stage
  never guesses one. `Unknown` never means unused.
- **Retail acceptance cross-checks itself.** The retail test recounts files
  with its own walk, re-derives world groups and `.rof` candidates with its
  own reads, re-opens every row by its preserved spelling, checks
  fingerprint stability across two full runs, and compares three picked
  digests (smallest/middle/largest file) against `python3 hashlib`.

## Evidence

Harness: `crates/cs_assets/tests/evidence_report_f02_b.rs`. From the
workspace root, after `git fetch && git rebase origin/main`:

```sh
set -o pipefail
cargo test --workspace --locked -- accept_f02_b_ --include-ignored \
  2>&1 | tee private/evidence/F02-B/cargo-test.log
# -> 8 tests discovered, 8 executed, 8 passed, 0 failed, 0 ignored

CS_EVIDENCE_DIR=private/evidence/F02-B \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f02_b_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=0 \
  cargo test --locked --test evidence_report_f02_b -- --ignored

python3 tools/validate_evidence.py private/evidence/F02-B/acceptance.json \
  --artifact-root private/evidence/F02-B --require-pass
```

The harness derives every report field from real inputs — the recorded log
(libtest summaries and per-test lines), `rustc --version`, `Cargo.lock`
(bevy/avian), `git rev-parse HEAD^{tree}` (it refuses a stale
`CS_CANDIDATE_TREE`), and production `discover`/`fingerprint`/
`content_fingerprint` over `$CS_GAME_DIR` — and writes
`acceptance.json` plus a `discovery-summary.json` artifact (counts,
diagnosis, fingerprints, per-file spellings/sizes/digests; original file
bytes are never copied). It asserts the retail test actually ran and passed
before declaring the `retail` capability, and fails (not passes) when the
acceptance run failed. The committed copy lives at
`docs/findings/evidence/F02-B.json`; artifacts stay in `private/`.

Note on `candidate_tree`: it is the tree of the commit whose tests produced
`cargo-test.log`. The evidence copy commits on top of it, so the committed
report references the tested tree, not the report's own commit.

## Recorded unknowns (not guessed, none blocking this stage)

- **File families, roles and parse state for the 228 retail files** — all
  rows are `None`/`Unknown`/`Unparsed` by design here; detection is
  F05/F06/F07 scope and the zero-unclassified audit is F02-D.
- **Whether a same-length rewrite inside the `hash_file` read window was
  observed** — undetectable without a second stat pass or content
  re-read; recorded as a documented residual race, mitigated by the
  owner-read-only installation. Not guessed at, not hidden.
- **Non-UTF-8 installation paths on the owner's machine** — rejected by name
  (`ManifestError::NonUtf8Path` via `DiscoveryError::Inventory`); none
  observed (the retail run inventoried all 228 files), so the diagnosis path
  for them is untested against real data.
- **Stale doc note:** `crates/cs_assets/src/lib.rs` still says "walking a
  real installation and hashing bytes arrive with F02-B" — now shipped in
  this same change. `src/lib.rs` is outside this task's owner paths, so the
  doc was left for a later owner-path touch rather than edited here.
- All are covered by already-queued tasks (F02-C, F02-D, F05/F06/F07); no
  new task was needed, so `create_tasks` was not used.

## Sources

`specs/F02-installation-discovery-versions-and-exhaustive-inventory.md`
(F02-B slice, AC01–AC04, non-negotiable behaviors 2/3/4);
`docs/contracts/IDENTITY-CONTENT.md` (canonical lowercase hex, collections
cannot exclude failed entries, lookup contract);
`docs/contracts/CLI-EVIDENCE.md` and `schemas/evidence.schema.json`
(evidence record, task-test discovery rules);
`docs/findings/2026-09-23-f02-a-install-inventory-and-compat-profile.md`
(logical identity encoding this stage hashes);
NIST FIPS 180-4 example digests (the four vectors); the owner's original
installation at `$CS_GAME_DIR` (read-only) and `python3 hashlib` as the
independent digest reference.
