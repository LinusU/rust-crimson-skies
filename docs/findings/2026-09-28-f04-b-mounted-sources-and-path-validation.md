# F04-B: Mounted sources and path validation

Date: 2026-09-28. Task: F04-B "Implement mounted sources and path
validation" (`specs/F04-context-aware-virtual-filesystem-and-precedence.md`).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only. No original data was read.

## Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/vfs/source.rs` (new): `mount_directory`,
  `MountedDirectory`, `RejectedEntry`/`RejectReason`, `SourceError`, the
  read path `read_member_range` and `ReadError`.
- `crates/cs_assets/src/vfs/mount.rs`: a mount now records its `Backing`
  (declared locations only, or a host directory); a member records the
  host path it was walked from; `Mount::host_root`, `Mount::member_count`;
  crate-private `MountBuilder::add_host_file` shares the duplicate check
  with `add_member_variant`.
- `crates/cs_assets/src/vfs/resolve.rs`: `Vfs::read_range` and
  `Vfs::read_all`, which refuse a resolution that no longer matches its
  mount.
- `crates/cs_assets/src/vfs/mod.rs`: module doc and re-exports (wiring).
- Test: `crates/cs_assets/tests/accept_f04_b_mounted_sources_and_path_validation.rs`
  (9 tests, prefix `accept_f04_b_`).

**One observable failure:** before this stage no real source could be
mounted at all, so AC02 could only be shown over hand-declared members.
With the duplicate refusal replaced by an overwrite, mounting a directory
that holds both `hud/x.dds` and a file literally named `hud\x.dds` would
silently keep one of them.

## Design decisions

- **A directory mount is the smallest real source.** Archive members
  (ZBD/ROF/ZIP) need the format readers of later tasks; a host directory
  tree is what the installation already is, so it is the production path
  this stage mounts. The caller's `MountBuilder` supplies id, namespace,
  precedence, scope and the *container label* recorded in every span; the
  host root never enters a `SourceSpan`.
- **Path validation applies to host names, not only to keys.** Each
  walked file's spelling (relative to the root, `/`-joined) must be a
  `RelativePath`. On Unix a file may be *named* `..\..\escape.dds` or
  `hud\x.dds`: the first fails the mount with `ParentComponent`, the
  second folds onto `hud/x.dds` and fails with both spellings. Non-UTF-8
  names fail the mount by path. Failing the whole mount is deliberate —
  a partly mounted directory would make a later "not found" look like
  absent content.
- **Symbolic links are never followed.** Links below the root are listed
  in `MountedDirectory::rejected` with `symbolic_link`, so the refusal is
  visible; a root that is itself a link is refused (`symlink_metadata`).
  At read time every component below the root is re-checked with
  `symlink_metadata`, and on Unix the opened file must have the same
  (device, inode) as the checked path, closing the check/open window.
  Review added two checks: the root itself is re-checked at read time (a
  root swapped for a link is refused), and while mounting the file opened
  for hashing must be the same regular file the walk saw, so a link
  swapped in between walk and hash is refused instead of hashing its
  target (a race window; not covered by a deterministic test).
- **Reads are random, read-only and owned.** `Vfs::read_range(resolved,
  start, length)` checks the range against the member before opening,
  opens the file read-only, verifies the on-disk length still equals the
  mounted length, seeks and returns an owned `Vec<u8>`; no handle outlives
  the call. `Vfs::read_all` additionally compares the SHA-256 of the bytes
  with the digest recorded at mount time. Nothing is extracted or written
  (non-negotiable behavior 5); a test compares the tree's full listing and
  bytes before and after mounting and reading.
- **Stale resolutions are refused.** A `ResolvedAsset` is read only if the
  VFS still holds its mount and the member it names has the same
  container, spelling, range and digest; otherwise `StaleResolution` or
  `UnknownMount`. This is the stale-state guard the session lifecycle in
  F04-C builds on.
- **Declared mounts stay declared.** `MountBuilder` members added through
  `add_member*` have no host bytes, and reading them is `NoBacking`, not
  an empty success.

## Mutation checks (run locally, then reverted)

| Mutation | Failing test |
| --- | --- |
| Duplicate member overwrites instead of refusing | `separator_collision_inside_one_directory_names_both_spellings` |
| Walk follows links (`fs::metadata` instead of `DirEntry::file_type`) | `symlink_escape_is_rejected_not_followed` |
| Read-time component/inode checks removed | `member_swapped_for_link_after_mount_is_not_read` |
| Staleness, length and digest checks removed | `world_mounts_read_their_own_bytes`, `changed_member_is_refused_and_mounting_writes_nothing` |
| Spelling validation bypassed (`..` rewritten) | `escaping_host_name_is_refused` |
| Read-time root link check disabled (review) | `mount_root_swapped_for_link_after_mount_is_not_read` |

The equal-priority cross-mount ambiguity (AC02 over two mounted
directories) is decided by the F04-A `Vfs::resolve` rule; its test fails if
registration order becomes a tiebreak.

## Recorded unknowns (recorded, not guessed)

- The letter-case duplicate *inside one directory* can only exist on a
  case-sensitive host. On a case-insensitive host (default macOS APFS,
  where this was run) the test verifies the filesystem merged the names
  and that one member results; the separator-collision test covers the
  refusal on every Unix host. CI (Linux) runs the case branch.
- Whether the retail installation contains case-only duplicates,
  backslash names or links is **unmeasured**; mounting `$CS_GAME_DIR` and
  comparing every observed collision with original lookup behavior is
  F04-D (retail capability).
- On non-Unix hosts the read-time "same file" check has no inode numbers
  and falls back to the per-component link check plus a length match.
  Filed as a follow-up task.
- Which installation directories become which mounts (namespace,
  precedence class, world/mission scope) remains the designed baseline of
  F04-A; wiring mounts into `cs-inspect resolve` and a content session is
  F04-C.
