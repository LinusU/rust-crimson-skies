# F04-C: Resolve tracing and session mount lifecycle

Date: 2026-09-28. Task: F04-C "Add resolve tracing and session mount
lifecycle" (`specs/F04-context-aware-virtual-filesystem-and-precedence.md`).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test, plus a local read-only run of the
retail acceptance test against `$CS_GAME_DIR` (outputs only under
`private/`; nothing derived from original data is committed).

## Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/vfs/session.rs` (new): `SessionBuilder`
  (`mount`, `mount_directory`, `mount_installation`, `open`),
  `ContentSession` (`resolve`, `read_all`, `read_range`, `begin_read`,
  `accept`, `close`), `SessionGeneration`, `SessionAsset`, `PendingRead`,
  `CompletedRead`, `SessionTeardown`, `SessionError`, and the namespaces
  `INSTALL_NAMESPACE`/`WORLD_NAMESPACE`.
- `crates/cs_assets/src/vfs/export.rs` (new): `export_components`,
  `ExportDirectory` (`open`, `write`), `export_asset`, `ExportError`,
  `UnsafeName`.
- `crates/cs_assets/src/vfs/resolve.rs`: mounts are held as `Arc<Mount>`;
  `Vfs::mounts`; the span-match and digest checks are shared helpers
  (`member_matching`, `read_whole_member`) so a pending read runs the same
  checks.
- `crates/cs_assets/src/vfs/source.rs`: `ReadError::ForeignSession`.
- `crates/cs_assets/src/vfs/mount.rs`: crate-private `MountBuilder::id`.
- `tools/cs_inspect/src/resolve.rs` (new): the `resolve` command and its
  JSON report.
- Wiring only: `crates/cs_assets/src/vfs/mod.rs` (module doc, re-exports),
  `tools/cs_inspect/src/lib.rs` (`pub mod resolve`),
  `tools/cs_inspect/src/main.rs` (help text, dispatching `resolve`).
- Tests (prefix `accept_f04_c_`):
  `crates/cs_assets/tests/accept_f04_c_session_lifecycle_and_private_export.rs`
  (8) and the `tests` module of `tools/cs_inspect/src/resolve.rs` (4 plus 1
  retail test marked `#[ignore = "requires CS_GAME_DIR"]`).

**One observable failure:** before this stage there was no export path at
all, so the only way to extract a member for research was to join its
archive spelling to a directory by hand — `..\..\escape.dds` or a link
planted in the export tree would then write outside it. And nothing tied
a resolution to the world it was made for: a `ResolvedAsset` from world
`c1` could be read after switching to `c2`.

## Design decisions

- **Session owns the mounts.** A `ContentSession` is one context, one
  `Vfs` and a process-unique `SessionGeneration` drawn from a counter (never
  caller-supplied). Resolutions are `SessionAsset`s with private fields,
  stamped with that generation; `read_all`/`read_range`/`begin_read`/
  `accept` refuse any other generation with `ReadError::ForeignSession`.
  This is the consumer-side guard against cross-world texture reuse after
  a world switch (AC04; the asynchronous-cancel measurement itself is
  F04-D).
- **In-flight reads own their backing description, never a file handle.**
  `PendingRead` holds an `Arc<Mount>` plus the resolution. Completing it
  re-checks the member against the span, opens the file read-only, reads,
  verifies the mount-time digest and closes the file. It completes after
  the session closed; its `CompletedRead` is stamped with the issuing
  generation and only that session's `accept` releases the bytes.
  Dropping a `PendingRead` is the cancel: nothing was opened.
- **Teardown and retry.** `SessionBuilder::mount_directory` failing names
  the mount (`SessionError::Source { mount, .. }`) and keeps the mounts
  already joined, so a caller can retry that one mount or drop the builder
  to release all of them. `ContentSession::close` consumes the session and
  reports the generation and released mount ids. `cs-inspect resolve`
  closes its session on every path after the lookup.
- **Designed installation layout.** `SessionBuilder::mount_installation`
  mounts the whole tree as `install` (namespace `install`, shared,
  container label `.`) and each F02-B `Diagnosis::world_groups` directory
  as `world-<n>` (namespace `world`, `mission_world`, bound to that world
  group, container label its spelling). Which retail sources the original
  engine binds to a world is **unmeasured**; this layout is `designed`,
  like the precedence order, and F04-D must compare it with original
  lookups.
- **Export is explicit and contained (AC03).** `export_components`
  re-validates the raw member name independently of `MountBuilder`:
  `RelativePath` rules, plus no `:` in a component (drive-relative and
  alternate-stream spellings on Windows), no control characters, no
  trailing `.`/space (Windows strips them, so `.. ` would become `..`), no
  Windows device name (`CON`, `NUL`, `AUX`, `PRN`, `COM1`–`9`, `LPT1`–`9`,
  `COM¹`–`³`, `LPT¹`–`³`, `CONIN$`, `CONOUT$`, any extension). The export root must be a real directory (not a link)
  outside every mount's host root. Directories are created one component
  at a time; an existing link or non-directory is refused, the landing
  directory must canonicalize below the root, and the file is written to
  a `create_new` temporary sibling, then hard-linked into place so an
  existing target is refused (never overwritten), and the temporary is
  removed.
- **`cs-inspect resolve`.** Discovers the installation (fingerprint),
  opens one session with the designed layout, resolves
  `--asset <namespace>:<path>` in the context built from `--world`
  (must be a discovered group), `--locale`, `--mission`, and writes a
  JSON report: fingerprint, context, every mount with member count and
  refused entries, the key, the result (span, or all ambiguous origins),
  every ordered attempt with its outcome, the `designed` precedence status
  and the export outcome. Exit codes per CLI-EVIDENCE: 0 resolved; 2
  invalid input (including `--out`/`--export-dir` inside the
  installation, or an export root inside a mount); 3 not found/ambiguous,
  a mount refusing content, or a refused export; 4 no installation; 1
  runtime failures. The report is still written on exit 3.

## Mutation checks (run locally, then reverted)

| Mutation | Failing test |
| --- | --- |
| `export_components` validates nothing | `malicious_archive_names_cannot_leave_export_directory` |
| Export root containment check removed | `export_root_inside_a_mount_is_refused` |
| Session generation check always passes | `world_switch_never_reuses_previous_session_texture` |
| Export tree link check and canonical check both removed | `link_planted_in_export_tree_is_not_followed` |
| Whole-member digest check removed | `pending_read_after_close_refuses_changed_bytes` |

The export tree has two independent link defenses (per-component
`symlink_metadata` and the canonical-prefix check of the landing
directory); removing only one of them is caught by the other, so the
test fails only when both are removed.

## Retail run (local, read-only)

`accept_f04_c_retail_world_resolves_its_own_texture` ran against
`$CS_GAME_DIR` (93 s at the dev profile; the whole installation is hashed
by discovery and again by the mounts). The installation mounted with no
refused entries; `world:texture.zbd` under the first world group resolved
from that group's own directory, and every other world group appeared in
the trace as `scope_mismatch`. This is a `checked` pipeline run, not a
measurement of original lookup behavior.

## Recorded unknowns and limits

- The world/installation mount layout and precedence order are `designed`
  (above); measuring them is F04-D.
- The standard library has no `openat`, so a link swapped into the export
  tree *between* the component check and the write is a race window of
  this private research command. The export directory is owner-controlled.
- `hard_link` is used for the no-overwrite rename; a filesystem without
  hard links (e.g. FAT) fails the export with `ExportError::Io` rather than
  falling back to an overwriting rename.
- Mounting the installation re-hashes every file already hashed by
  discovery; reusing discovery digests is an optimization for a later
  stage, not a correctness issue.
- Archive members (ZBD/ROF/ZIP) still cannot be mounted: their readers
  belong to the format tasks. Their names will go through
  `MountBuilder::add_member` and, on export, `export_components`.
