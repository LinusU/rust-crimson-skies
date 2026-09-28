# F05-C: mount ROF into VFS and expose inspection

Date: 2026-09-28. Task: F05-C "Mount ROF into VFS and expose inspection"
(`specs/F05-rof-directory-trees-and-compressed-members.md`, section
`### F05-C`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (the two retail observations
below are read-only observations of the already-disclosed file *sizes*,
offsets and error codes, recorded for F05-D; no evidence report is
required by this task).

## Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/rof.rs` (new): `RofMountError`
  (`UnreadableContainer`, `Format`, `NonUtf8Name`, `InvalidMemberPath`,
  `Member`, `Mount`, `Session` — each with `code()`, `container()`,
  `offset()`), `RofMemberInfo` (spelling, id, offset, stored_len,
  compressed, sha256), `RofSource` (`container`, `path`, `mount_id`,
  `namespace`, `limits`, `member_count`, `members`, `member`, `read`),
  `MountedRof`, `RofReadError` (`UnknownMember`, `Format`),
  `RofExportError` (`ForeignSession`, `ForeignMount`, `Read`, `Export`),
  the free functions `mount_rof`, `mount_rof_with_limits`,
  `mount_rof_into`, `export_rof_member`, and the private
  `join_spelling`.
- `crates/cs_assets/src/lib.rs` (wiring only): `pub mod rof;` plus one
  module-doc sentence naming F05-C.
- `crates/cs_assets/src/rof.rs` tests (`#[cfg(test)]`, 4 ×
  `accept_f05_c_`): the authored containers (`good_container`,
  `cycle_container`, `bad_name_table_container`, `outside_pointer`,
  `bomb_container`, `non_utf8_container`, `case_collision_container`)
  and the mount, read, export, session-lifecycle and refusal tests.
- `tools/cs_inspect/src/rof.rs` (new): the `rof` command
  (`rof_command`, `rof_command_result`, `RofRun`, `RofCommandError`,
  `RofReport` rendering, `ROF_REPORT_VERSION`) plus 4 × `accept_f05_c_`
  tests.
- `tools/cs_inspect/src/{lib,main}.rs` (wiring only): `pub mod rof;`,
  the `rof` dispatch arm, the help entry and the command lists.
- `tools/cs_inspect/Cargo.toml` + `Cargo.lock` (wiring only): the
  `cs_formats` dependency the command needs for `RofLimits`
  (`docs/01-ARCHITECTURE.md` allows `cs_inspect` → formats; F02-C set
  the precedent of adding a dependency here as wiring).
- `docs/findings/2026-09-28-f05-c-mount-rof-into-vfs-and-expose-inspection.md`
  (this file).

**Not changed:** `crates/cs_formats/src/rof.rs` and
`crates/cs_formats/tests/rof.rs` are owner paths but need no change —
F05-B already provides the producer (`read_tree`, `read_member`,
`RofLimits`) this stage wires in. No file outside the owner paths and
this list is touched.

**One observable failure:** with the error propagation in `mount_rof`
removed (every `read_tree` failure mapped to an empty-but-successful
mount), `accept_f05_c_mount_refuses_cycles_bad_name_tables_and_outside_pointers`
reports a refused container as a mounted archive with zero members
instead of failing with `cycle` / `name_table_length` /
`extent_out_of_bounds`, so AC03's "fail before writes" collapses: the
caller would hand the refusal to a session as content. Verified by
mutation after implementation (table below).

## Design decisions

- **What "mounted" means here.** One mount member per file entry of the
  walked tree, spelled as its root-relative path joined with `/`, with
  the extent `start .. start + raw_length` and the SHA-256 of exactly
  those stored bytes. A resolution therefore returns an immutable
  [`SourceSpan`] describing bytes that exist in the container and hash to
  what the span says (IDENTITY-CONTENT: offsets are unsigned checked
  ranges), and `Mount`'s (variant, logical path) index keeps two equal
  basenames of two directories distinct — `MIS/readme.txt` and
  `MAP/readme.txt` are two members, not one flattened key.
- **The span is the *stored* extent; the decoded bytes are derived.**
  `raw_length` is the field the reference extractor reads for compressed
  members ([S05]), so the mount records it as `stored_len` and never
  collapses the two words (the fixture authors `raw_length = 62` and
  `raw_length_on_disk = 32` for the compressed member, and the test
  pins the mount to the 62). Which word is stored and which decoded in
  the original stays the F05-D blocker; nothing here decides it from an
  English field name.
- **Why the bytes do not go through `Vfs::read_all`.** The F04 read path
  (`crate::vfs::source::read_member_range`) reads a *host file* range: it
  refuses a member whose length differs from the file on disk
  (`ReadError::ChangedOnDisk`, written for whole files) and has no
  notion of a compressed member. A ROF member lives inside a container,
  so `RofSource::read` answers bytes instead: it holds the container
  bytes the mount was built from, rebuilds the reader's `RofMember`
  record (path segments split back out of the spelling, record, start,
  both declared ends) and calls the production `cs_formats::read_member`
  with this source's limits. The mount answers resolution, the source
  answers bytes, and `Vfs::read_all` on a ROF member reports
  `no backing` — asserted in the test — rather than handing compressed
  bytes to a caller as if they were content. `RofSource` is deliberately
  not `Clone`: cloning would duplicate the container.
- **Nothing is decoded while a container is enumerated.** `mount_rof`
  walks and indexes; an expansion bomb or a corrupt stream fails when
  *that member* is read, so one bad member never refuses a whole archive
  and never blocks an inspection report that only lists members. The
  ceiling is the configured surface (`RofLimits`, default 64 MiB, exposed
  as `--max-decoded-bytes`), not a hidden constant.
- **Refusals leave nothing behind.** Every `RofMountError` is produced
  before `builder.build()` runs, so a refused container adds no mount to
  the session it was handed: the builder keeps exactly the mounts it
  held, a retry refuses the same container at the same offset
  (deterministic, asserted), and dropping it releases everything —
  F04-C's mount-failure contract applied to archive members.
  `mount_rof_into` is the one-call form of that wiring; its third error
  source (`Session`, a repeated mount id) is named rather than hidden.
- **Names are keys, not strings.** Every spelling is validated as a
  `RelativePath` before it reaches `MountBuilder`, so a `..`, an absolute
  spelling or an empty component in a container cannot become a key that
  escapes a mount root (F04 non-negotiable behavior 1). A name that is
  not UTF-8 has no spelling a lookup could match and refuses the mount
  (`non_utf8_name`, naming the container and the extent offset, never the
  bytes of the name) — the same refusal `mount_directory` makes for a
  non-UTF-8 host name. Two names that fold onto one key are refused by
  the mount index with both spellings (F04 non-negotiable behavior 3),
  asserted.
- **Export reads first, writes second.** `export_rof_member` is the ROF
  counterpart of `vfs::export_asset`: it checks the asset was stamped by
  *this* session and resolved from *this* mount, validates the name
  before any IO, reads the member (a bomb or a refused layout fails
  here), and only then hands the **decoded** bytes to
  `ExportDirectory::write`, which re-validates the name itself. The test
  asserts the export directory is empty — not even a temporary file —
  after every refusal, and that the compressed member exports its
  204-byte payload rather than the 62-byte stored stream.
- **The command's report is the evidence.** `cs-inspect rof` writes the
  same JSON on success and on refusal (exit 3 included), with the
  container's mount status, the machine-readable refusal
  (`code`/`offset`/`detail`), every member's extent and digest, and what
  the read and the export produced. Neither `--out` nor `--export-dir`
  may lie inside the installation (checked before discovery), the
  container spelling is a validated `RelativePath` that must name a
  regular, non-symlink file of the installation, and `--export-dir`
  requires `--member` so an export can only name a member that was
  resolved. `inside`, `write_report` and `jstr` mirror the private
  helpers of `resolve`/`inventory`: this task owns `rof.rs` only, so the
  same rules are re-applied there instead of editing those modules.
- **Mount provenance.** The container label the caller passes to
  `MountBuilder` is also the label of the `ParseContext` and therefore of
  every error, and the mount is registered with a derived `MountId`
  (`rof-` plus the lowercased spelling, `[a-z0-9._-]`, ≤ `MAX_LABEL_LEN`)
  in the `install` namespace as a retail source — one key space, one
  provenance line, no guessing about world or mod binding.

## Test inventory (8 tests, prefix `accept_f05_c_`)

| Test | What it pins down |
| --- | --- |
| `accept_f05_c_mounted_members_resolve_read_and_export` | a container mounted into a real session: depth-first member order with verbatim ids 11/2/3, the compression bit only on `PACK.DAT`, `stored_len` 62 (not the 32 the other word authors), a span whose offset/length/digest are exactly the stored bytes (and *not* the digest of the decoded payload), the 62→204 stored/decoded pair read through the production decoder, `Vfs::read_all` reporting `no backing` for a container member, the export of the uncompressed member and of the compressed member (204 decoded bytes, never the stream), and teardown releasing exactly this mount |
| `accept_f05_c_mount_refuses_cycles_bad_name_tables_and_outside_pointers` | **AC03 / minimum scenario (mount half):** cycle, invalid name table and outside-file pointer each refused with their code, container and offset; the session builder unchanged after each refusal (still exactly one mount); a retry refusing identically; the session still serving the mount it has; the export directory untouched |
| `accept_f05_c_expansion_bomb_fails_before_any_write` | **AC03 (the bomb):** the container mounts (enumeration decodes nothing), the 149-byte stream is refused as `expansion_bomb` at the 4 KiB ceiling when the export asks for it, the export directory stays empty, and the same member under the default ceiling decodes its 128 KiB — the bound is configured, not hard-wired |
| `accept_f05_c_unspellable_members_refuse_the_mount` | a non-UTF-8 name refuses the mount with `non_utf8_name` at the member's extent offset; a case-only duplicate refuses it with `member` quoting both spellings; both leave a fresh session builder at zero mounts |
| `accept_f05_c_rof_command_mounts_reports_and_exports` | the command end to end on the committed Python-authored fixture: exit 0, report written atomically and equal to stdout's report, mount line (`rof-pack.rof`, `install`, `shared`, retail), `member_count` 2 with both members' extents/ids/digests, the read line with `stored_len`/`decoded_len`/`trailing_len` and the decoded digest, and the exported file's bytes |
| `accept_f05_c_rof_command_refuses_a_cycle_container_without_writing` | exit 3 with `cycle` in the diagnostics, the refusal reported (`status: refused`, `mount: null`, `member_count: 0`, machine-readable error) and *written* to `--out` as the evidence, an empty export directory, and a second run without `--member` still exiting 3 |
| `accept_f05_c_rof_command_refuses_an_expansion_bomb_before_writing` | the mount succeeds, `--max-decoded-bytes 32` refuses the 100-byte member as `expansion_bomb` at its offset, the export stays `skipped`, exit 3, export directory empty |
| `accept_f05_c_rof_command_rejects_invalid_input` | exit 2 for a missing `--container`, an escaping spelling (`../outside.rof`), a container the installation does not hold, `--export-dir` without `--member`, an `--out` inside the installation (and no such file created), and a non-numeric `--max-decoded-bytes`; exit 4 without an installation; no report for input that never ran |

## Commands and exit codes

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f05_c_ --include-ignored` | 0 (8 tests: 4 in `cs_assets`, 4 in `cs_inspect`) |
| `cargo run -p cs_xtask --locked -- test-select --prefix accept_f05_c_` | 0 (`8 test(s) selected ... 8 passed`; each re-ran alone with `--exact`) |
| `cs-inspect rof --cs-path "$CS_GAME_DIR" --container GOSDATA/ASSETS/crimson.rof --out private/f05-c/crimson-rof.json` | 3 (read-only retail observation, see below) |
| `cs-inspect rof --cs-path "$CS_GAME_DIR" --container GOSDATA/ASSETS/crimptch.rof --out private/f05-c/crimptch-rof.json` | 3 (read-only retail observation, see below) |

## Mutation probes (implementation removed → tests fail)

Applied one at a time, the relevant suite run, the source restored from a
copy afterwards (verified byte-identical after every restore; the suite
was green again after each restore):

| Mutation | Exit | Failing test(s) |
| --- | --- | --- |
| P1: cycle detection disabled in `read_tree` (`ancestors.contains` never true, `cs_formats`) | 101 | `accept_f05_c_mount_refuses_cycles_bad_name_tables_and_outside_pointers` (the cycle container is walked into the recursion limit and comes back `parse`, not `cycle`); the F05-B cycle tests fail too |
| P2: name-table validation disabled (`validate_names` returns early, `cs_formats`) | 101 | the same F05-C test (the container is refused as `cycle` instead of `name_table_length`); 5 F05-B/F05-A tests fail |
| P3: both extent bounds disabled in the walk (`cs_formats`) | 0 | *none of the F05-C tests* — the mount's own extent re-check still refuses the same container with `extent_out_of_bounds`, so the system behaviour is unchanged; the producer mutation is caught by `accept_f05_b_outside_file_pointers_fail_before_any_read` and `accept_f05_b_bookings_and_refusals_leave_the_ledger_exact` (101) |
| P4: mount records `raw_length_on_disk` as the stored extent (`cs_assets`) | 101 | `accept_f05_c_mounted_members_resolve_read_and_export` (62 ≠ 32) |
| P5: configured decode ceiling ignored on read (`&RofLimits::new(u64::MAX)` instead of the source's limits) | 101 | `accept_f05_c_expansion_bomb_fails_before_any_write` **and** `accept_f05_c_rof_command_refuses_an_expansion_bomb_before_writing` |
| P6: export writes the stored extent instead of `read.data` | 101 | `accept_f05_c_mounted_members_resolve_read_and_export` (the compressed member exports 62 stored bytes, not its 204-byte payload) |
| P7: `mount_exit_code` reports content refusals as `0` (`cs_inspect`) | 101 | `accept_f05_c_rof_command_refuses_a_cycle_container_without_writing` (the report-only run keeps the exit code) |
| P8: the export directory opened *before* the read (`cs_inspect`) | 0 | *none* — `ExportDirectory::open` creates no file, so this mutation writes nothing; the property the tests actually pin is "no bytes written unless the read succeeded", which P6 shows is load-bearing |

## Retail observations (read-only, for F05-D)

`CS_GAME_DIR` is available on this machine, so the command was run
against both retail containers read-only. **Neither mounts today**, and
both refuse exactly where the spec says refusing is the only honest
answer (non-negotiable #5: absent independently documented evidence of
legitimate sharing, surface `UnsupportedLayout` rather than extracting
arbitrary spans):

| Container | Size | Refusal | Numbers |
| --- | --- | --- | --- |
| `GOSDATA/ASSETS/crimson.rof` | large | `unsupported_layout` | the extent `[37359, 93507)` overlaps the extent `[52437, 87591)` (checked at offset 52437) |
| `GOSDATA/ASSETS/crimptch.rof` | 797 bytes | `extent_out_of_bounds` | the entry at offset 127 declares extent `[127, 1768)` — past the 797-byte container |

Both refusals are the F05 research blocker showing itself on real data:
`crimptch.rof` claims a 1641-byte extent inside a 797-byte file, which is
what using the wrong one of the two length words looks like, and
`crimson.rof` has two extents that share bytes under the `raw_length`
profile. Neither is fixed here — guessing a layout, an overlap
exemption or a length word to make a retail file mount would violate
spec F05 non-negotiable #4/#5 and AGENTS.md rule 4. The commands exit 3
and write a report naming the code, offset and numbers, which is the
evidence F05-D needs; the observation was added to task #24 (F05-D)
with `add_note`. No original bytes, names or payloads appear in this
file or in Git: `private/f05-c/*.json` holds the two reports and stays
ignored.

## Recorded unknowns (not guessed here)

1. **Compressed-length semantics** — inherited unchanged from F05-A/B;
   the retail refusals above are new evidence *for* it, not a decision.
   F05-D owns the resolution.
2. **Overlap legitimacy** — `crimson.rof` has overlapping extents under
   the `raw_length` profile. No source documents that sharing as
   legitimate, so it stays `unsupported_layout` (spec non-negotiable #5).
   Whether it is the length word, a directory record's unknown meaning,
   or real shared bytes is F05-D's question.
3. **Directory record length fields** — validated to lie inside the
   container (F05-B's rule, inherited here); meaning still unknown.
4. **Non-UTF-8 retail names** — the mount refuses one if it exists; no
   retail container has been walked far enough to say whether any does.
   The refusal keeps bytes out of diagnostics either way.
5. **Decoded digests** — the mount digests *stored* bytes (that is what
   the span describes). Two containers holding the same content
   compressed differently therefore show different digests, which a
   collision comparison (task #345) must not mistake for different
   content; the decoded digest is reported by the read instead.
6. **Memory** — `RofSource` holds the container bytes for as long as its
   session or command does. Not measured against retail sizes yet
   (`crimson.rof` mounts too little today to say).

## Sources used

- `specs/F05-rof-directory-trees-and-compressed-members.md` (whole
  sheet, `### F05-C` in particular) and
  `docs/contracts/IDENTITY-CONTENT.md`.
- `docs/findings/2026-09-28-f05-a-raw-rof-structs-and-boundary-fixtures.md`
  and `.../2026-09-28-f05-b-directory-traversal-and-bounded-member-reads.md`
  for the conventions, recorded unknowns and the test/proof style this
  stage continues (including the two fixture streams, copied with their
  generator command).
- `crates/cs_formats/src/rof.rs` (the producer: `read_tree`,
  `read_member`, `RofLimits`, `RofError`) and
  `crates/cs_formats/tests/rof.rs` (fixture builders to re-author).
- `crates/cs_assets/src/vfs/{mount,source,session,export,resolve}.rs`
  (F04-B/C/D) for the mount contract, the session lifecycle, the export
  rules and the exact reason a container member cannot be read through
  `Vfs::read_all`.
- `tools/cs_inspect/src/resolve.rs` (F04-C) for the command shape,
  exit-code mapping, report style and the `inside`/`write_report`/`jstr`
  rules re-applied here.
- `docs/01-ARCHITECTURE.md` (crate dependency column: `cs_inspect` may
  use formats) and `docs/findings/2026-09-24-f02-c-...md` (precedent for
  adding a dependency to `tools/cs_inspect/Cargo.toml` as wiring).
- `$CS_GAME_DIR` (read-only) for the two retail observations; the
  reference extractor S05 as quoted by the F05-A/B findings.

## Status

**Checked, not recreated** (AGENTS.md rule 8): the eight
`accept_f05_c_*` tests, `cargo fmt`, `cargo clippy -D warnings` and
`cargo test --workspace` pass on this commit, and the mutation probes
above show which production lines they depend on. This stage reads no
original data into Git and certifies nothing about retail: both retail
containers are refused today, the evidence for that is recorded for
F05-D, and no original-data claim is made here.
