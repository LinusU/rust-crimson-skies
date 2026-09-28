# F06-B: the reader and sound container subset, with bounds

Date: 2026-09-28. Task: F06-B "Implement reader and sound container subset
with bounds" (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
section `### F06-B`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — **no `$CS_GAME_DIR` read**, no evidence report required, nothing derived
from original game data.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/zbd/archive.rs` (new): the bounded container layer
  both F06-B readers share — `CONTAINER_ENTRYPOINT`, `MEMBER_ROW_BYTES`,
  `MemberExtent`, `MemberTable` (`from_dispatch`, `named`), `FamilyOrigin`,
  `MemberError`, `MemberStatus`, `MemberRow`, `ContainerStatus`,
  `UnsupportedRecord`, `ArchiveListing`, `ContainerError`, `FamilyMismatch`,
  `require_family`, `list_members`, `undeclared_reason` and the span helpers
  `check_extent` / `merge_ranges`.
- `crates/cs_formats/src/zbd/reader_archive.rs` (new): `EncodingEvidence`,
  `ReaderEntry`, `ReaderArchive`, `ReaderError`, `read_reader_archive`.
- `crates/cs_formats/src/zbd/sound_archive.rs` (new): `SoundField`,
  `SoundDescriptor`, `SoundEntry`, `SoundArchive`, `SoundError`,
  `read_sound_archive`.
- `crates/cs_formats/src/zbd/mod.rs` (wiring): the three module declarations,
  the matching `pub use` re-exports, and the module-doc sentence that says no
  reader parses container bytes yet (F06-B is that stage).
- `crates/cs_formats/tests/zbd/readers.rs` (new): the `accept_f06_b_*` tests.
- `crates/cs_formats/tests/zbd/main.rs` (wiring): one `mod readers;` line so
  the F06-A test target also builds the F06-B tests.
- `docs/findings/2026-09-28-f06-b-reader-and-sound-container-bounds.md`
  (this file).

Not created in this stage (later F06 stages, nothing to wire yet):
`crates/cs_assets/src/zbd.rs` (turning a VFS key/container into a member table
and mounting the result is F06-C) and `tools/cs_inspect/src/zbd.rs` (the
`cs-inspect zbd` command needs F06-D's retail pass before it can print an audit
with a nonzero strict status).

**One observable failure:** with the family gate removed from
`read_reader_archive` (or with the gate replaced by a fallback that adopts
whatever family the table happens to declare), the container at
`zbd/interp.zbd` whose bytes carry the **documented** INTERP signature
`0x08971119` / version `7` — a valid header — is read by the reader-family
reader and `accept_f06_b_valid_header_with_incompatible_family_data_fails_explicitly`
fails at `assert_eq!(mismatch.actual(), ZbdFamily::Interp)`: the reader-archive
reader claims a container whose dispatch decided the `interp` family, which is
exactly the silent fallback spec F06 AC02 forbids. The mirrored failure — the
reader family's own container handed to the sound reader, or a member whose
declared extent runs past the container — is covered by the same test and by
`accept_f06_b_member_extents_are_bounds_checked`.

## Why this slice is bounds first

F06-A recorded the constraint this stage works under: the committed research
pack documents a header layout for exactly one of the six ZBD families
(`INTERP.ZBD`), and **none** for the reader or sound family
(`docs/research/FORMAT-NOTES.md` "INTERP observed subset" [S07];
`docs/findings/2026-09-28-f06-a-zbd-family-inventory-and-dispatch.md`,
"Recorded unknowns"). Task #340 owns reading those layouts from the pinned
mech3ax 0.6 source and checking them against the installation; this stage
declares ordinary build/test only and must not define a field offset the pack
does not document (spec F06 non-negotiable #1 and its research boundary,
"Precise sound/reader header variants must be read from the pinned source").

So the subset F06-B implements is the half of a family reader that is
**independent of the member-index layout**: which family the container is (the
F06-A gate), that every member the container's own index declares is inside the
container, what the reader consumed, what it could not support, and that a
container of another family is refused instead of being re-read by a different
parser. The member index is an **input**
(`MemberTable` + `MemberExtent`), not an assumption: F06-B claims no knowledge
of which bytes of a reader or sound archive hold that index or how a member's
name, id and extent are laid out inside it.

<!-- results, test inventory, mutation probes and the remaining unknowns are
appended below once the implementation and the probes are done -->
