# F02-A: installation inventory and compatibility-profile schema

Date: 2026-09-23. Task: F02-A "Define installation inventory and
compatibility-profile schema"
(`specs/F02-installation-discovery-versions-and-exhaustive-inventory.md`).
Capabilities used: ordinary build/test only.

## Files and the one observable failure

- `crates/cs_types/src/install.rs` (new): `RelativePath`/`RelativePathError`,
  `FileFamily`/`FamilyError`, `FileRole`, `ParseState`, `InstallFileRecord`,
  `ManifestError`, `InstallFileRecord::validate`, `InstallIdentity`,
  `InstallManifest::new`, `InstallManifest::logical_identity`,
  `INSTALL_IDENTITY_HEADER`, `InstallationClass`, `LocaleLabel`,
  `DimensionLabel`, `RuleCompatibility`, `InstallationEdition`,
  `CompatibilityProfile` (+ `validate`), `ProfileError`.
- `crates/cs_assets/src/install.rs` (new): `DiscoveredFile`,
  `inventory(host_root, discovered)`, and the private
  `relative_spelling` root-strip.
- `tools/cs_inspect/src/install.rs` (new): `synthetic_install_fixture`.
  Each crate's `src/lib.rs` gains its `pub mod install;`.
- Tests: `crates/cs_types/tests/accept_f02_a_manifest_and_profile_schema.rs`
  (5), `crates/cs_assets/tests/accept_f02_a_cased_host_path_inventory.rs`
  (6), `tools/cs_inspect/tests/accept_f02_a_synthetic_install_fixture.rs`
  (2) — 13 tests selected by `accept_f02_a_`.
- Observable failure if the implementation is removed or stubbed: an
  installation inventory keyed on the host root (the natural bug) makes
  `accept_f02_a_cased_host_paths_share_logical_identity`,
  `accept_f02_a_cased_relative_spellings_share_identity_and_keep_originals`,
  `accept_f02_a_logical_identity_tracks_data_not_host_paths`,
  `accept_f02_a_compatibility_profile_dimensions_stay_independent` and
  `accept_f02_a_fixture_identity_is_stable_across_cased_roots` fail; a
  case-sensitive root strip makes
  `accept_f02_a_root_case_mismatch_still_inventories_one_identity` fail.
  Verified by mutation: making `logical_identity` append `host_root` and
  making the root strip compare case-sensitively produced exactly those 6
  failures; restoring the implementation returns all 13 to green.

## Design decisions

- **Logical identity is data, not path or analysis.**
  `InstallManifest::logical_identity()` encodes rows sorted by
  case-folded, `/`-separated logical key as `len:key|size|sha256` behind a
  versioned header (`INSTALL_IDENTITY_HEADER`, currently `v1`). It excludes
  the host root, input row order, the letter case and separator style of
  the original spelling, and the analysis columns (family, role, parse
  state): re-classifying the same bytes is an engine knowledge change, not
  an installation change. A one-byte digest or size change does change the
  identity (the row SHA-256 is inside the encoding). The encoding is
  injective: the key's byte length disambiguates separator bytes inside a
  key. The header is the version anchor so a future format change must bump
  it rather than silently reinterpreting stored identities.
- **The host root stays out of the manifest's identity but inside the
  manifest.** `InstallManifest::host_root` is kept for diagnostics and
  re-opens (F02-B/F02-C need to open the files again); identity never sees
  it. AC01 is therefore discriminating: the two inventories really have
  different `host_root` values in the tests.
- **Root stripping is ASCII case-insensitive per component.** A
  `CS_GAME_DIR` whose letter case disagrees with the discovered paths still
  inventories (Windows/macOS case-insensitive semantics; matches AC01's
  "differently cased host paths"). Exact matches are the normal case in
  F02-B because discovery derives paths from the root itself. On a
  case-sensitive host a mismatched-case prefix would be accepted for
  spelling derivation only; the file would then fail to re-open under that
  root and be reported, never silently dropped. Relative spellings join
  components with `/` so the same tree discovered on either host type
  yields one identity; component letter case is preserved as discovered.
- **Nothing is ever omitted.** Every discovered file becomes a row or the
  whole inventory fails by name: `NotUnderRoot`, `EmptyRelative`,
  `NonUtf8Path` (no lossy conversion), `RelativePath` (validation failure,
  including `..` escapes), `DuplicateLogicalKey` (two rows differing only
  in case are one ambiguous key on the case-insensitive trees this engine
  targets), `EmptyRoleReason`, `EmptyParseDiagnostic`, `EmptyRoot`. Unknown,
  unparsed and failed rows stay in the collection (IDENTITY-CONTENT:
  collections cannot exclude failed entries).
- **The role vocabulary is exactly the sheet's** (non-negotiable 4):
  `Consumed`, `NeededUnimplemented`, `OptionalMedia`,
  `UnusedWithReason` (reason mandatory), `PlatformSupport`, `Unknown`.
  `Unknown` never means unused.
- **`FileFamily` is an open validated lowercase label, not an enum of
  guessed families.** None was observed in this environment; format tasks
  (F05/F06/F07) emit labels as they recognize layouts, and an undetected
  family is `None`, never a fabricated variant.
- **The compatibility profile keeps the four architecture dimensions
  independent.** `installation` (edition/locale tied to the manifest's
  logical identity), `rules` (`Stock`/`Modified { note }`/`Unknown`),
  `presentation` and `assists_mods` (validated `DimensionLabel`s or `None`
  while unrecorded). A modded ruleset can never compare equal to stock, an
  undetermined ruleset is explicitly `Unknown`, and a `None` dimension
  never reads as "original". `InstallationClass` (full, partial, patched,
  localized, demo-like) is a canonicalized *set*, so a patched, localized
  full installation is not conflated into one label.
- **This stage stops at the schema.** No disk walk, no SHA-256 computation,
  no family detection, no readiness gate: `inventory` consumes measured
  facts as typed input. That is F02-B (discovery/hashing/diagnosis), F02-C
  (`cs-inspect` report wiring) and F02-D (private audit, AC04 readiness)
  scope. The synthetic fixture in `cs_inspect` mirrors F01-A's
  `synthetic_claim_fixture`: authored rows through the canonical
  constructor, covering all six role variants, so future commands and
  tests have a real input without touching `$CS_GAME_DIR`.

## Recorded unknowns (not guessed, none blocking)

- **Which file families exist and their exact labels** — open vocabulary
  by design; the observed sets belong to F05 (ROF), F06 (ZBD families),
  F07 (interp) and are inventoried by F06-A.
- **Which evidence makes an installation `patched`/`localized`, and the
  locale list** — discovery decides in F02-B/F02-D; `LocaleLabel` is an
  opaque validated label and `None` stays "unknown". No locale or patch
  table was invented here (spec F02 "Research boundary").
- **The expected-content denominator that separates `Full` from `Partial`
  (AC04 readiness)** — not derivable from this schema; F02-D's retail audit
  plus the F13/F14 catalogs fix the denominator. No readiness function is
  claimed in this stage.
- **Non-UTF-8 installation paths** — rejected by name
  (`ManifestError::NonUtf8Path`); how discovery diagnoses and reports them
  on the owner's machine is F02-B work.
- All of the above are covered by already-queued tasks (F02-B, F02-C,
  F02-D, F06-A); no new task was needed, so `create_tasks` was not used.
