# F53-C: Selection, diagnostics and private export — the producer/consumer wiring

Date: 2026-10-07. Task: F53-C "Add selection, diagnostics and private
export tooling"
(`specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
section `### F53-C`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/mods/selection.rs` (new): the whole wiring —
  `ModSelection` / `AvailableMod` / `SelectionError` (the producer),
  `mount_request` (`MountRequest::base_ids` from the real catalog),
  `modded_context` / `session_builder` / `mount_payloads` /
  `open_mod_session` (the content-session consumer),
  `content_signature` (the lobby signature the consumer announces),
  `mount_to_text` (diagnostics), `export_mounted_mods` /
  `mod_export_text` / `ModExport` / `ModExportError` /
  `OriginalDependency` / `MOD_EXPORT_REPORT` (the private export).
- `crates/cs_content/src/mods/mount.rs`: one accessor,
  `MountedMods::root`, so the export reads winners straight from their
  digest-checked mod root.
- `crates/cs_content/src/mods/mod.rs`: `mod selection;`, the re-exports,
  and the module-header paragraph naming the wiring.
- `crates/cs_app/src/ui/mods.rs` (new): `ModsView` / `ModRow` /
  `ModsNotice` (the screen projection) and `lobby_compatibility` (the fold
  into `cs_net::compat::Compatibility` — the only place `cs_net`
  vocabulary may appear, because `cs_content` must not depend on
  `cs_net`).
- `crates/cs_app/src/lib.rs`: wiring only (`pub mod mods;` inside
  `pub mod ui` and the module-doc paragraph).
- This file.

**One observable failure:** a session that enables the synthetic tuning
mod announces `MountedMods::signature` — not the base installation
fingerprint — and `cs_net::compat::evaluate_hello` refuses its hello to a
stock lobby with `HandshakeReject::ContentMismatch` naming both
signatures, in both directions; either side admits itself. Removing the
fold (`lobby_compatibility` announcing the bare fingerprint while mounts
are enabled, or `content_signature` returning the fingerprint for a
mounted set) fails this.

Test count: 11 `accept_f53_c_*` tests — 9 unit tests in
`crates/cs_content/src/mods/selection.rs`, 2 in
`crates/cs_app/src/ui/mods.rs` — selected by
`cargo test --workspace --locked -- accept_f53_c_ --include-ignored`
(exit 0, 11 executed, all passing). As in F53-A/F53-B the tests live in
`#[cfg(test)]` modules inside the owner files: `crates/*/tests/` is not an
owner path, and the virtual workspace root declares no package for a root
`tests/` directory to attach to.

## What this stage implements

- **One producer: `ModSelection`.** It is the single place "which
  discovered mods are enabled" lives — a `ModManifest` plus the directory
  it ships under per offer, `enable`/`disable`/`clear`, and the enabled
  set's `ModSet` + `MountEnvironment` built *together*, so a planned mod
  can never reach `mount_mods` without its root. Offering a duplicate id
  or enabling an unoffered id is a named `SelectionError`, never a silent
  merge or a lost root. `mount_request` closes the loop F53-A left open:
  `MountRequest::base_ids` is `Catalog::sorted_ids()`, so `Add`-of-existing
  and `Replace`-of-missing are checked against what the installation's
  catalog claims to provide, and the test proves it both ways (the tuning
  mod mounts against a catalog that provides `gun/synthetic.vulcan` and is
  refused by one that does not). There is deliberately no manifest *file*
  reader: the sheet specifies no on-disk manifest format and inventing one
  is unmeasured vocabulary (F53 "Research boundary"). The caller holds the
  typed manifest; how it was produced stays the caller's concern (filed as
  a follow-up for the owner).
- **One consumer pair, wired.** `open_mod_session` hands
  `MountedMods::mounts` — each mod-precedence and scoped to its own
  `ModId` — to a `SessionBuilder` whose `ResolveContext` opted into the
  plan's `ModStack`; the test resolves `mod/tuning/vulcan.toml` through
  the ordinary digest-checked path, and shows the same mounts refusing
  `SkipReason::ModNotOptedIn` for a context that did not opt in. The open
  is atomic (a failed payload mount drops the partial builder — no
  half-mounted session exists to leak), `ContentSession::close` is the
  owned teardown whose `released` list the test compares to the load
  order, and a `SessionAsset` resolved by the closed session is refused by
  its successor with `ReadError::ForeignSession` — the stale-state guard,
  exercised, not assumed.
- **The signature is what the lobby compares (AC03).**
  `content_signature(None, base)` is the base fingerprint — a stock
  session announces exactly the bytes it runs — and
  `content_signature(Some, base)` is `MountedMods::signature`, which folds
  that fingerprint in with the plan hash and the measured payload
  digests. `cs_app::ui::mods::lobby_compatibility` puts it in
  `Compatibility::content_sha256`; the acceptance test runs the real
  `evaluate_hello` both directions and asserts the `ContentMismatch`
  payload names *both* hashes. Because the signature is domain-separated
  from the bare fingerprint, "no mount" and "a mount" can never collide
  accidentally; because it covers payload bytes, a tuning mod cannot hide
  inside a cosmetic-looking plan hash. A cosmetic-only set is unmarked
  but still mismatched — the gate is identical bytes, not marking.
- **The wire's `mods` field stays empty, on purpose.**
  `Compatibility::mods` wants enabled mods as *catalog content ids* — and
  no `ContentKind` names a mod, so populating it would mislabel a mod as
  another kind (the F54 test fixtures used `Blueprint` for synthetic mod
  entries, a fixture choice that cannot become production vocabulary
  without an owner decision; `cs_types` is not an F53-C owner path).
  Nothing is lost: the signature already carries the enabled set and its
  bytes, so the gate is complete and `ModSetMismatch` cannot fire while
  signatures match. Filed as a follow-up.
- **Diagnostics are the mount's own report.** `mount_to_text` is the
  plan report plus the signature, the measured totals, the marking
  verdict, every mounted payload with its claim position, declared source,
  size, digest, effect and validation, and every walk rejection — so a
  refused symlink is named with its host-relative path and reason rather
  than silently absent. `ModsView` projects rows and refusals without
  owning state: `MountError::code` plus the verbatim message, and each
  `PlanProblem` with its own code — the failure a host reports is the
  failure `mount_mods` produced.
- **The private export ships a mod's own bytes and names what it cannot
  ship** (F53 non-negotiable 4). `export_mounted_mods` writes every
  *winning* payload — read from its own `ModRoot`, so `ModRoot::read`'s
  digest re-check refuses bytes swapped after the walk — below an
  `ExportDirectory` opened against the mod session (an export root inside
  a mount is refused by the same guard that protects the installation).
  A shadowed claim serves nothing and writes nothing: the test's losing
  `Replace` leaves no file. `mod-export.txt` lists the mounted mods,
  their declared dependencies, every exported file with its digest, and
  `unresolved-original` lines naming each `Replace` target the base
  provides — the dependencies that must come from the installation and
  are deliberately not copied. A repeat export is refused by the
  target-exists guard rather than overwriting.

## Unknowns, limitations and where they are resolved

- `MountEnvironment::base_fingerprint` is still supplied by the caller:
  the production producer of the F02 installation fingerprint exists
  (`cs_content::install::content_fingerprint`), but which fingerprint a
  host adopts for which session is a host decision outside this stage's
  vocabulary; the tests use a designed constant, like F53-B's.
- `Compatibility::mods` is empty for the reason above; a future task can
  carry `ModId`s on the wire once the vocabulary exists.
- The save/reload half of AC04 ("reopen a dependent save without
  destructive fallback") belongs to the save system (F48); this stage
  contributes the selection half — `clear()` leaves a stock
  announcement, an empty set cannot mount, and no mount can linger —
  exercised by `accept_f53_c_disabling_everything_leaves_a_stock_session`.
- The `ModsView` projection carries no widget layout or input (that is a
  front-end stage); notices carry stable codes and designed English
  fallbacks pending F51 localization.

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings` — exit 0.
- `cargo test --workspace --locked` — exit 0.
- `cargo test --workspace --locked -- accept_f53_c_ --include-ignored` —
  exit 0, 11 tests run, all passing.

## Sensitivity probes (none committed)

- Removing `selection.rs` or `ui/mods.rs` removes the tests' call targets
  (they are `#[cfg(test)]` modules inside the same files), so the
  selection cannot silently regress to a stub.
- The AC03 test asserts `expected: base, offered: mounted.signature()` —
  not merely that the hashes differ — so a signature that folded only the
  plan hash (unchanged by payload bytes) would still fail when the payload
  bytes changed... the payload-covering is F53-B's guarantee; the fold
  into `content_sha256` is this stage's, and the test pins both ends.
