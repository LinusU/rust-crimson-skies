# F06-A: observed ZBD family inventory and dispatch

Date: 2026-09-28. Task: F06-A "Inventory observed ZBD families and define
dispatch" (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
section `### F06-A`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/zbd/mod.rs` (new): module doc, the four
  submodules (`dispatch`, `family`, `header`, `role`) and the `pub use`
  re-exports.
- `crates/cs_formats/src/zbd/family.rs` (new): `ZbdFamily` (the six
  families the F06 deliverable names, `ALL`, `as_str`,
  `file_family_label`, `file_family`, `reader`), `ZbdReaderId`
  (`label`, `family`), `ZbdFamilyRecord` (`family`, `reader`,
  `header_rule`, `role_rules`, `source`), `ZBD_FAMILY_INVENTORY` and
  `family_record`.
- `crates/cs_formats/src/zbd/header.rs` (new): `INTERP_SIGNATURE`,
  `INTERP_VERSION`, `INTERP_SIGNATURE_OFFSET`, `INTERP_VERSION_OFFSET`,
  `HeaderRule` (`Signature`, `Undocumented`, `signature`,
  `undocumented_reason`, `evidence`), `SignatureRule` (`offset`, `value`,
  `version_offset`, `version`, `source`, `evidence`, `required_bytes`,
  `probe`), `HeaderProbe` (`TooShort`, `Mismatch`, `Match`).
- `crates/cs_formats/src/zbd/role.rs` (new): `CONTENT_ROOT`, `RolePattern`
  (`Exact`, `Prefixed`, `matches`, `Display`), `RoleRule` (`exact`,
  `prefixed`, `pattern`, `evidence`, `source`), `ZbdRole` (`Observed`,
  `Unrecognized`, `family`), `role_for_path`.
- `crates/cs_formats/src/zbd/dispatch.rs` (new): `ZbdProbe` (`container`,
  `path`, `header_bytes`), `DispatchBasis`, `HeaderStatus`, `RoleStatus`,
  `ZbdDispatch` (accessors `container`, `path`, `header_bytes`, `family`,
  `reader`, `basis`, `header_status`, `role_status`, `file_family`,
  `record`), `ZbdDispatchError` (`HeaderTooShort`, `HeaderMismatch`,
  `UnsupportedHeaderVersion`, `HeaderRoleConflict`, `UnknownFamily`, with
  `code`, `container`, `Display`, `Error`) and the entrypoint `dispatch`.
- `crates/cs_formats/src/lib.rs` (wiring only): `pub mod zbd;`, the
  `pub use zbd::{...}` re-exports and one module-doc sentence naming
  F06-A.
- `crates/cs_formats/tests/zbd/main.rs` (new; Cargo builds it as the
  integration-test target `zbd` from `tests/zbd/main.rs`): authored header
  and path fixtures plus the `accept_f06_a_*` tests.
- `docs/findings/2026-09-28-f06-a-zbd-family-inventory-and-dispatch.md`
  (this file).

**Not created in this stage** (owner paths of later F06 stages, nothing to
wire yet): `crates/cs_assets/src/zbd.rs` — turning a VFS key/container into
a dispatch probe and feeding member bytes into a reader is F06-C, and
`tools/cs_inspect/src/zbd.rs` — the `cs-inspect zbd` command has to report
consumed ranges and unsupported records, which needs F06-B's readers.
Same reasoning F05-A recorded for not creating `tools/cs_inspect/src/rof.rs`
and F04-A for `tools/cs_inspect/src/resolve.rs`.

**One observable failure:** with the two-key dispatch removed (or with
`dispatch` reduced to a single hard-coded reader),
`accept_f06_a_two_distinct_headers_route_to_different_readers` fails at
`assert_ne!(first.reader(), second.reader())`: the documented INTERP header
at `zbd/interp.zbd` and the second authored header at `zbd/planes.zbd` come
back as the same reader, so the acceptance scenario of the stage (route at
least two distinct synthetic ZBD-family headers to different readers, F06
AC01) is not met. The mirrored failure — a documented header that
contradicts its observed role silently parsing under the other family — is
`accept_f06_a_header_and_role_disagree_fails_instead_of_falling_back`.
Both were verified by mutation after implementation (results below).

## The observed family inventory

"ZBD" is an extension/family label, not one layout (F06 deliverable; "No
universal ZBD header is asserted by this specification"). The inventory
below is everything the *committed* research pack establishes about which
families exist, how they can be recognized and which installation paths
carry them. Nothing here was read from `$CS_GAME_DIR` for this stage (the
task declares ordinary build/test only), and nothing is guessed: an
undocumented header stays `HeaderRule::Undocumented`, a name-based
family assignment is `EvidenceClass::Inferred` and an unobserved one is
`Unknown`.

| Family (label) | Reader slot | Header rule | Observed role rules (basename under `zbd/`) | Evidence |
| --- | --- | --- | --- | --- |
| interp (`zbd.interp`) | `Interp` | signature `0x08971119` @0 + version `7` @4 | `interp.zbd` | header: `Documented` — `docs/research/FORMAT-NOTES.md` "INTERP observed subset" [S07], repeated in `specs/F07`; name: `Documented` — `docs/research/FINDINGS.md` (`INTERP.ZBD` is the loading-script container) and the F02-C findings (observed at `ZBD/interp.zbd`) |
| gamez (`zbd.gamez`) | `GameZ` | none documented | `planes.zbd`, `gamez.zbd` | `Documented` — `specs/F10` ("GameZ data that supplies world geometry and PLANES.ZBD meshes"), `docs/research/FINDINGS.md` (`ZBD/PLANES.ZBD`, world-specific `gamez.zbd`), CLI example `unzbd cs gamez <PLANES.ZBD>` [S06] |
| texture (`zbd.texture`) | `Texture` | none documented | `texture.zbd`, `rtexture*.zbd` | `texture.zbd`: `Documented` — CLI example `unzbd cs textures <texture.zbd>` [S06]; `rtexture*`: `Inferred` from the observed names (`docs/findings/2026-09-24-f02-c…`, `accept_f02_d_classification.rs`) |
| reader (`zbd.reader`) | `Reader` | none documented | `zrdr.zbd` | `Inferred` from the observed name (present in every group and mission directory, F02-C/F02-D findings) |
| animation (`zbd.animation`) | `Animation` | none documented | `cam_anim.zbd`, `mis_anim.zbd` | `Inferred` from the observed names (F02-C/F02-D findings) |
| sound (`zbd.sound`) | `Sound` | none documented | *(none)* | family named by the F06 deliverable; **no `.zbd` archive name in committed evidence has been tied to sound bytes** — recorded unknown, filed as a follow-up task |

The role table is matched against `RelativePath::logical_key()`, so it is
case-insensitive and spelling-preserving, and it applies only under the
observed content root `zbd/` (all 184 retail `.zbd` archives live there —
`docs/findings/2026-09-24-f02-d-installation-audit.md`).

## Design decisions

- **Dispatch is two-key, and the two keys can disagree — visibly.** The
  stage deliverable is "dispatch by validated header/version and
  installation role". `dispatch(probe)` evaluates the observed role
  (`role_for_path`) and every documented signature rule
  (`SignatureRule::probe`) and then combines them:
  role family *with* a documented header rule → the bytes must validate
  against it (`HeaderTooShort` / `HeaderMismatch` /
  `UnsupportedHeaderVersion`, never a silent fallback to another parser);
  role family *without* a documented header rule → the bytes are first
  checked against every documented signature (a contradicting match fails
  as `HeaderRoleConflict`) and then recorded as
  `HeaderStatus::Unvalidated`; an unrecognized role with a documented
  signature → `DispatchBasis::HeaderOnly`; neither → `UnknownFamily`.
  A family disagreement outranks a version disagreement, so an INTERP
  signature in `zbd/planes.zbd` reports the conflict first.
- **"Unvalidated" is an explicit status, not an omission.** Only the INTERP
  header layout is documented in the committed pack, so for the other
  five families `dispatch` cannot claim it checked anything: it returns
  `HeaderStatus::Unvalidated { reason }` with the family's recorded reason
  (the spec's research boundary: "Precise sound/reader header variants must
  be read from the pinned source and checked against the installed game").
  The negative half of the check is still real and is asserted by the
  conflict test: an INTERP signature inside a role-only family fails
  instead of parsing.
- **A second documented signature is a recorded unknown, not a guess.**
  AC01 asks for two distinct synthetic headers routed to different readers.
  Two distinct authored headers are routed to two different readers — one
  identified by its documented signature plus its observed role, the other
  by its observed role with the header explicitly recorded unvalidated.
  Inventing a second magic number to make the *routing basis* look uniform
  would violate "unknown means unknown"; the follow-up task filed with
  Rally closes it (read the pinned source, then check the installation).
- **Role rules are data in `cs_formats`, not I/O.** `role_for_path` takes a
  `&RelativePath` the caller has already discovered and applies a static
  basename table; it walks no directory and opens no file, so it does not
  make the parser depend on asset-directory enumeration (`docs/01-ARCHITECTURE.md`
  lists `cs_formats`' allowed dependency as `cs_types`, which is where
  `RelativePath` lives). The alternative — putting the mapping in
  `cs_assets::zbd` — would leave production code outside this task's owner
  test path (`crates/cs_formats/tests/zbd/`), untested by the task prefix.
- **The inventory is one table, not two.** Each `ZbdFamilyRecord` carries
  its reader slot, its header rule *and* its observed role rules, so the
  family, the reader and both dispatch keys cannot drift apart; a test
  asserts the row count, the unique reader slots, the valid `FileFamily`
  labels and that `Sound` still has no observed role rule.
- **`FileFamily` is the F02 open vocabulary, reused.** `ZbdFamily::file_family`
  hands back the validated `cs_types::install::FileFamily` label
  (`zbd.interp`, `zbd.sound`, …) that F02's doc comment already anticipates,
  so a future inventory row can carry the family dispatch identifies
  without inventing a second label scheme.
- **Errors stay payload-free and machine-matchable** (F03/F05 style): every
  `ZbdDispatchError` variant carries the caller's container label plus the
  decision-relevant values (byte counts, observed/supported version,
  contradicting families, signature word) and a stable `code()`; no probe
  bytes are ever copied into an error message.
- **Fixtures are authored in the test file**, never committed as binaries
  (`fixtures/synthetic/README.md`, F05-A precedent): this stage's fixture
  is two headers and a set of installation paths, all written by the test.

## Test inventory (`accept_f06_a_*`)

All in `crates/cs_formats/tests/zbd/main.rs`; every one calls
`cs_formats::zbd::dispatch` / `role_for_path` / the inventory table.

| Test | Covers |
| --- | --- |
| `two_distinct_headers_route_to_different_readers` | AC01: documented INTERP header at `zbd/interp.zbd` → `Interp` (header+role); authored non-INTERP header at `zbd/planes.zbd` → `GameZ` (role only) |
| `role_only_dispatch_records_an_unvalidated_header` | `HeaderStatus::Unvalidated` carries the family's recorded reason; case-insensitive role match; evidence-labelled role rule |
| `documented_header_without_a_known_role_routes_to_the_interp_reader` | `HeaderOnly` basis outside `zbd/` and for an unobserved basename, with the two distinct reasons |
| `header_and_role_disagree_fails_instead_of_falling_back` | AC02 preview: INTERP signature at `planes.zbd` → `header_role_conflict`; non-INTERP bytes at `interp.zbd` → `header_mismatch` |
| `role_that_names_a_documented_family_requires_its_signature` | wrong signature / too-short probe at `interp.zbd` |
| `unsupported_header_version_fails_explicitly` | documented signature with an undocumented version, with and without a role |
| `unknown_header_and_unknown_role_are_rejected` | `unknown_family`: nothing is routed by extension alone |
| `dispatch_failures_carry_a_container_and_a_stable_code` | every error variant: stable `code()`, container label, no probe bytes in the message |
| `role_inventory_covers_every_observed_archive_name` | every archive name in the F02-C/F02-D findings maps to its family; `texture.zbd` vs `rtexture*` disjoint |
| `family_inventory_declares_one_reader_per_family` | six rows, unique reader slots, valid `FileFamily` labels, `Sound` still without a role rule |

## Mutation probes

Each mutation was applied to production code, the `zbd` test target run,
and the file restored with `git checkout`:

| Mutation | Failing tests |
| --- | --- |
| `dispatch` hard-codes `ZbdReaderId::Interp` as the reader | `two_distinct_headers_…`, `role_only_dispatch_…` |
| role-only branch skips the documented-signature conflict check | `header_and_role_disagree_…`, `dispatch_failures_carry_…` |
| version check removed (`if false`) | `unsupported_header_version_…`, `dispatch_failures_carry_…` |
| `HeaderMismatch` replaced by a fallback to header-only dispatch | `role_that_names_a_documented_family_…`, `header_and_role_disagree_…`, `dispatch_failures_carry_…` |
| `role_for_path` ignores the GameZ rows | `two_distinct_headers_…`, `role_inventory_covers_…`, `header_and_role_disagree_…`, `dispatch_failures_carry_…` |

## Recorded unknowns (filed as follow-up tasks)

- **Sound family archive names.** No `.zbd` basename in committed evidence
  is tied to sound bytes, so `ZbdFamily::Sound` has no role rule and no
  probe can reach the sound reader yet. Needs the pinned source (S02/S06)
  and a retail check (F06-B/F06-D).
- **Header layouts for sound, reader, texture, GameZ and animation.** Only
  the INTERP signature/version is documented; the other five families
  dispatch on role alone with `HeaderStatus::Unvalidated`. Each needs its
  header read from the pinned mech3ax 0.6 source and checked against the
  installation before a `SignatureRule` is added.
- **Name-inferred role rules** (`zrdr.zbd` → reader, `cam_anim.zbd` /
  `mis_anim.zbd` → animation, `rtexture*.zbd` → texture) are
  `ClaimStatus::Inferred` and must be confirmed against real bytes.
