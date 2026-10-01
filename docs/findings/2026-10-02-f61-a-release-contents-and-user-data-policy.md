# F61-A: release contents and user-data directory policy — decisions and limits

Date: 2026-10-02. Task: F61-A "Define release contents and user-data directory
policy" (`specs/F61-distribution-installation-ux-notices-and-release-artifacts.md`).
Capabilities used: ordinary build/test only. Every fixture is newly authored
synthetic text in this repository; nothing here is derived from the original
game and no `$CS_GAME_DIR` was read.

## Files and the observable failure

- `tools/cs_xtask/src/package.rs` (new): `ReleasePolicy`, `ReleasePolicy::scan`,
  `classify`, `MemberClass`, `ProprietaryKind` + `PROPRIETARY_SUFFIXES`,
  `ORIGINAL_CONTENT_ROOTS`, `HASH_MANIFEST_*`, `REQUIRED_NOTICES`,
  `ENGINE_BINARIES`, `CandidatePackage`, `PackageMember`, `Finding`,
  `PackageReport`, `parse_manifest`/`read_manifest`, `unsafe_path`/`UnsafePath`,
  and the user-data half: `UserDataPolicy::{resolve, layout, check}`,
  `UserDataEnv`, `UserDataLayout`, `UserDataArea`, `HostPlatform`.
- Wiring only: `pub mod package;` and a module doc line in
  `tools/cs_xtask/src/lib.rs`; a `verify-package --manifest <file>` subcommand
  plus its usage text and option parsing in `tools/cs_xtask/src/main.rs`.
- `packaging/README.md` and `packaging/fixtures/candidate-clean.manifest`: the
  synthetic candidate the scan is exercised against.
- Tests: `tools/cs_xtask/tests/accept_f61_a_release_contents.rs` (10) and
  `tools/cs_xtask/tests/accept_f61_a_user_data_policy.rs` (4), plus two more in
  the first file added in review (see the review corrections below).
- Observable failure without the implementation: a candidate carrying
  `textures/plane00.dds` scans as releasable, and a candidate with no `NOTICE.md`
  scans as releasable — `accept_f61_a_proprietary_content_is_refused` and
  `accept_f61_a_a_missing_notice_is_refused` fail. Verified by mutation:
  making `ReleasePolicy::scan` return no findings fails 7 of the 8
  release-contents tests; making a missing platform variable fall back to a
  default path fails `accept_f61_a_a_missing_or_relative_platform_variable_is_refused`;
  narrowing `UserDataPolicy::check`'s application rule to equality fails
  `accept_f61_a_user_data_is_never_inside_the_installation_or_the_application`.

## Decisions

- **A release archive is allow-listed, not deny-listed.** A member is refused
  unless the policy classifies it as the engine, a required notice,
  documentation or a hash manifest. `MemberClass::Unclassified` is a finding, so
  a gate that passes what it does not understand is not a gate. The engine is
  the one executable a release may carry and is matched by name in the archive
  root or `bin/`, so `docs/crimson-skies` cannot stand in for it.
- **The required notices are six**, one per spec sentence: `LICENSE`,
  `THIRD-PARTY-NOTICES.md`, `REFERENCE-TOOLS.md` (reference-tool licensing
  "recorded separately from new-engine code", non-negotiable 2), `NOTICE.md`
  (provenance and non-affiliation, non-negotiable 3), `COMPATIBILITY.md` (the
  "versioned compatibility report") and `README.md` ("user instructions").
- **A source hash manifest is not content** (non-negotiable 1's explicit
  exception: "A source hash manifest is not an asset bundle"). Hash-manifest
  suffixes and names are matched *before* the proprietary table, but *after* the
  original-content-root rule, so the exception cannot launder content out of
  `original/`.
- **The user-data base is derived from the platform and an environment, and
  from nothing else.** `resolve` has no argument through which a build path, a
  working directory or an executable location could enter the answer, which is
  how non-negotiable 4 is enforced structurally rather than by convention. This
  answers the open item F48-A recorded: "User-data base directory choice: F61".
  `%APPDATA%` (Windows), `~/Library/Application Support` (macOS),
  `%XDG_DATA_HOME%` with the XDG default fallback (Linux). A missing variable
  is `MissingEnvironment`, never an invented default.
- **The profile area *is* the base**, because F48 derives
  `base/<population>/profile-<id>` from the base it is handed. `cache/` (F15)
  and `logs/` sit beside it, so the cache can be deleted without touching
  saves. Mod directories are **not** in the layout: F53 owns mod mount points
  and this stage does not invent one.
- **Containment is component-wise** (`Path::starts_with`), so
  `/games/CrimsonSkies2` is not inside `/games/CrimsonSkies`; a string-prefix
  check would refuse a legitimate sibling and a loose one would write into
  read-only original data.
- Absoluteness is checked **for the platform being resolved for**
  (`is_absolute_for`), not for the host. `Path::is_absolute` would make a
  Windows-shaped `%APPDATA%` look relative on macOS, and the rule under test
  would then be the host's rather than the policy's.

## Not proven here, and who resolves it

- **A name is not a content check.** Classification uses the member path,
  because a path is all a packaging step has before the archive is written.
  That catches a release assembled wrongly; it does not prove the bytes inside
  an allowed member are newly authored, and a renamed original file would pass.
  Resolving: F61-B — compare every member's SHA-256 against a recorded
  original-content digest set, and inspect actual archive members.
- **The compatibility report's *contents* are unchecked.** Stage A requires the
  report to be present, not to parse, name the release version or report
  verified content. Resolving: F61-B, which owns the packaging step and the
  report's schema.
- **No archive is built, extracted or run.** No member is hashed, nothing is
  written to a real filesystem, and the packaged binary is not started from an
  odd directory. Resolving: F61-B (packaging and automated content/notices
  checks, AC01/AC02) and F61-D (real artifacts on every platform, AC04).
- **Branding and trademark are owner decisions.** `APP_DIR_NAME` is
  `CrimsonSkiesRust` so that nothing this project writes presents itself as
  Microsoft or Zipper software, and the notice file names are this project's
  choices. Nothing here is a legal assurance, and non-negotiable 3 says the
  branding decision stays with the owner. Resolving: F61-C's documentation
  stage, with the owner.
- **The original installation's file extensions are unknown and are not
  guessed.** `PROPRIETARY_SUFFIXES` is a policy list of publicly known formats
  a redistributable archive never carries; it makes no claim about what the
  original game called its own files. Formats it has never heard of are caught
  by the original-content-root rule and by `Unclassified`, never by a guessed
  suffix.
- **`check` cannot protect a directory the caller does not name.** Passing
  `installation: None` disables the installation rule. The first-launch path
  must pass the installation it resolved. Recorded so F61-C does not read a
  passing check as a general guarantee.
- **Platform coverage is logic, not runs.** The three platforms are resolved
  from synthetic environments in one test binary; no Windows or Linux machine
  has run this code. Resolving: F61-D.

## Review corrections (independent reviewer, 2026-10-02)

Three defects were found in review and fixed on the task branch rather than
handed back. Each fix is pinned by a test that fails without it.

- **A backslash-spelled member could satisfy the engine rule.** `classify` split
  only on `/`, so `docs\crimson-skies` was classified `EngineBinary` and set the
  "this release ships an engine" flag — a member that a `/`-written manifest
  cannot produce, and a rule the same module documents as "the archive root or
  `bin/`". `classify` now normalises `\` to `/`, so it agrees with
  `unsafe_path` (which already read `\` as a separator) and
  `original\data\plane00.dat` keeps the original-content-root rule instead of
  falling through to `Unclassified`.
- **`total_bytes` panicked on an absurd manifest.** Declared sizes come from a
  text file and were summed with `sum()`, which overflows: two members of
  `u64::MAX` aborted the gate instead of reporting on it. The sum now saturates,
  documented on the field. The gate decides on findings, never on the total.
- **`--workspace-root` was accepted and ignored by `verify-package`.** Every
  other subcommand checks the root it was handed; this one ignored it, so a bad
  root could not be told apart from a good one. It is now checked like its
  siblings.

Also added: a process-level test of `verify-package` (exit 0 releasable, exit 1
with every finding on stderr, exit 2 for a bad request, exit 1 for an unreadable
manifest or an unusable root) — the subcommand was wiring the library tests never
exercised — and an assertion that every `ProprietaryKind` has a spelled-out case,
so a new class of proprietary content cannot arrive covered by nothing.

## Why this stage did not touch `.github/`

The feature sheet lists `.github/workflows/` among the owner paths, but Rally
marks `.github/` protected for this task (`allowProtectedChanges: false`) and
refuses to merge a branch that touches it, so no workflow file was edited. If
a release gate belongs in CI, the owner adds it; the local command to run is
`cargo run -p cs_xtask -- verify-package --manifest <manifest>`.