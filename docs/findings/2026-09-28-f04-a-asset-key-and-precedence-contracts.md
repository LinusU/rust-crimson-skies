# F04-A: AssetKey and precedence contracts

Date: 2026-09-28. Task: F04-A "Define AssetKey and precedence contracts"
(`specs/F04-context-aware-virtual-filesystem-and-precedence.md`).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only.

## Files and the one observable failure (listed before editing)

- `crates/cs_types/src/asset_id.rs` (new): `MountNamespace`/`NamespaceError`,
  `AssetVariant`/`VariantError`, `AssetKey` (`new`, `from_spelling`,
  `logical_key`, logical `PartialEq`/`Eq`/`Hash`/`Ord`),
  `AssetKeyError`, `WorldGroup`, `MissionScope`, `ModId`,
  `ModStack`/`ContextError`, `PrecedenceClass` (+ `rank`, `label`),
  `PRECEDENCE_ORDER_STATUS`, `ResolveContext` (+ `new`, `with_*`),
  `SourceSpan`/`SourceSpanError` (+ `new`, accessors, `byte_span`).
  `crates/cs_types/src/lib.rs` gains `pub mod asset_id;` (wiring only).
- `crates/cs_assets/src/vfs/mod.rs` (new): module doc and re-exports.
- `crates/cs_assets/src/vfs/mount.rs` (new): `MountId`, `MountScope`,
  `SkipReason`, `MemberRecord`, `MemberKey`, `MountBuilder`
  (`new`, `with_*` scope setters, `add_member`, `add_member_variant`,
  `build`), `Mount`, `MountError`.
- `crates/cs_assets/src/vfs/resolve.rs` (new): `Vfs` (`new`, `mount`,
  `resolve`), `ResolvedAsset`, `ResolutionTrace`, `ResolutionAttempt`,
  `AttemptOutcome`, `ConflictOrigin`, `ResolveError`.
  `crates/cs_assets/src/lib.rs` gains `pub mod vfs;` (wiring only).
- Tests (`crates/cs_assets/tests/`, both selected by `accept_f04_a_`):
  `accept_f04_a_asset_key_and_context.rs` and
  `accept_f04_a_two_worlds_each_resolve_own_texture.rs`.
- `tools/cs_inspect/src/resolve.rs` is **not** created in this stage: the
  `resolve` command needs mounted archive sources and a byte-reading VFS,
  which arrive with F04-B/F04-C. Until then `cs-inspect resolve` keeps
  refusing with a nonzero exit (never a success it did not produce).

**One observable failure:** with world selection removed from
`Vfs::resolve` (the natural bug — return the first eligible member
regardless of the context's world group), resolving the same
`AssetKey` under world `zbd/c2` returns world `zbd/c1`'s digest, so
`accept_f04_a_two_worlds_each_resolve_own_texture` fails on
`member_sha256`/`container_path`. Verified by mutation after
implementation (results recorded below).

## Design decisions

- **`AssetKey` is (mount namespace, logical path, variant) with logical
  identity.** The namespace and the variant are validated lowercase labels
  (`[a-z0-9._-]`, max 64 bytes — the `FileFamily` rule); the retail
  vocabulary for both is *unknown* and is not guessed here (see
  "Recorded unknowns"). The logical path is a `RelativePath`, so the F02
  validation of `..`, absolute spellings, drive prefixes, `.`/`//`
  components and NUL already applies to every key, and the original
  spelling is retained while `logical_key()` (separator-normalized,
  ASCII-lowercased) is what lookups compare. Two keys that differ only in
  spelling or separator style are **equal**, hash the same and order
  equal — the F02 logical-identity principle applied to lookups
  (non-negotiable behavior 1).
- **`ResolveContext` requires an installation fingerprint.**
  `installation: ContentHash` is the F02 installation fingerprint
  (`cs_assets::install::fingerprint`), so every `SourceSpan` the VFS
  returns can name its installation as `IDENTITY-CONTENT` requires; a
  resolution that could not name its installation is impossible to
  construct rather than a runtime surprise. `world_group`, `locale`,
  `mission` and the `ModStack` are the other four context fields named by
  the sheet.
- **Precedence is an explicit, ordered contract labeled `designed`.**
  `PrecedenceClass` ranks `Mod > Patch > MissionWorld > Shared`
  (non-negotiable behavior 2: opt-in mods over verified patch overlays
  over mission/world-specific sources over shared sources) and
  `PRECEDENCE_ORDER_STATUS` is `ClaimStatus::Designed`: until F04-D
  measures original lookup behavior, every resolution report says the
  baseline order is designed, not measured. What proves a patch overlay
  *verified*, and what the original ordering is, are outside this stage.
- **A mount declares a scope; the context admits it.** `MountScope`
  carries optional world-group, mission, locale and mod bindings. A
  binding present on the mount must match the context (`Mod` bindings
  must additionally be opted into `ModStack`); a mount with no bindings
  serves every context. The comparison is logical: world groups compare
  by case-folded key, locales case-insensitively, labels are already
  lowercase. All-`None` scope + `Shared` class is the plain shared source.
- **No first-wins map.** `Vfs::resolve` collects every eligible mount that
  holds the key, ranks them by `(precedence rank, position in the mod
  stack)`, and takes the top rank. One mount at the top rank wins; two or
  more return `ResolveError::Ambiguous` carrying **both origins**
  (mount id, container path, member spelling, precedence class). Register
  order is never a tiebreak (non-negotiable behavior 3), it only
  stabilizes the trace. A case-only repeat of a member inside one mount
  fails at `MountBuilder::add_member` with both spellings.
- **The result is an immutable span plus a trace.** `ResolvedAsset`
  carries the key, the winning mount, its precedence class, a
  `SourceSpan` (installation hash, container path, member key, offset,
  length, member hash) and a `ResolutionTrace` with one ordered attempt
  per in-namespace mount: `Selected`, `Candidate` (holds the key but a
  higher tier won), `Miss`, or `Skipped(ScopeMismatch |
  ModNotOptedIn)`. Errors carry the same trace, so a `NotFound` says
  which mounts were consulted and why each did not serve.
- **`SourceSpan` validates its range at construction.**
  `offset + length` is a checked `u64` range and an empty container or
  empty member key is rejected (`SourceSpanError`); `member_key` is
  provenance only — it is never joined to a filesystem path, so a hostile
  spelling is *recorded* there and *rejected* where a member is mounted
  (`MountBuilder::add_member` wraps `RelativePathError`). This is the
  consumer-side validating constructor the F03-A finding left open;
  no hash value is invented by it. It lives in `cs_types::asset_id`
  because `cs_types` owns source spans and `cs_content` must reach it
  without depending on `cs_assets`.
- **Mount lifetimes and byte reads are not this stage.** `Vfs` mounts and
  resolves; it does not open files (non-negotiable behavior 5 and the
  mount session lifecycle are F04-B/F04-C). Nothing here writes outside
  the repository and no original data is read.

## Recorded unknowns (recorded, not guessed)

- The retail **mount-namespace** and **asset-variant** vocabularies are
  unknown. The labels are open, validated and engine-authored in this
  stage; no exhaustive set is claimed.
- The retail **precedence order** is unmeasured: the baseline order above
  is `designed` and F04-D must measure original lookup behavior for every
  observed collision.
- Whether the original scopes mounts by **locale**, and how it binds a
  source to a **mission**, are unknown; both are carried in the context
  and honored by a designed matching rule only.
- The **mod stack order** (a later entry outranks an earlier one) is a
  designed load-order convention, not an observation; F53-A owns mod
  manifests.
- **Symlink escapes** cannot be checked without a filesystem mount, so
  they are deliberately left to F04-B path validation; the logical-path
  rules (`..`, absolute, drive prefix, NUL) are enforced already.
- `cs_types::evidence::SourceSpan` (a two-field byte span inside
  `ObservationLocator`, F01) and `cs_types::asset_id::SourceSpan` (the
  six-field `IDENTITY-CONTENT` span) share a name on purpose; they are
  distinct types in distinct modules and `SourceSpan::byte_span()`
  converts the latter into the former.

## Commands run

(filled in when the slice is complete — see below)
