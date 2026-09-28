# F06-C: the ZBD member producer on the VFS, and the sound samples it yields

Date: 2026-09-28. Task: F06-C "Connect ZBD member producers to VFS and
audio assets" (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
section `### F06-C`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — **no `$CS_GAME_DIR` read**, no evidence report required, nothing
derived from original game data.

## Plan revision after a rebase conflict (read this first)

This stage was first planned against a base revision in which the sound
family had **no** observed archive name, the member index was a caller
input, and every `SoundDescriptor` field was `Unknown`. All three premises
were void on `origin/main` when the branch was rebased: tasks #340, #343 and
#344 landed in the meantime and

* #340 tied the sound family to `ZBD/sounds*.zbd`, so **dispatch now routes
  the sound family** and `MemberTable::named` no longer accepts it;
* #343 reads the version-one trailer member index out of the container, so
  the member index is **no longer a caller input**;
* #344 reads each member's RIFF/WAVE header, so the format, channel count,
  rate, bits per sample and block align are **now declared by the member
  itself**.

The first plan is therefore void, not merely stale: its sound route
(`MemberTable::named`) no longer exists, and its "declared format is a caller
input" premise is the opposite of what the data now says. This document is
the re-plan against `origin/main` = `f122a1e`; the first plan's file list,
tests and probes were discarded with the branch reset.

**One observable failure:** with the two-key dispatch removed from the
producer (or with the producer adopting whatever family a path *looks* like
instead of the family the inventory's observed rules name), the container at
`ZBD/soundsl.zbd` — an observed sound archive name, task #340 — is read by
the **reader** reader instead of the sound reader, or its member index is
taken from somewhere other than its own trailer. The test fails at
`assert_eq!(archive.family(), ZbdFamily::Sound)`: the sound archive is claimed
by the wrong family, which is exactly the silent cross-family read spec F06
AC02 forbids, and it is the failure this wiring can most plausibly introduce
because F06-C is the first stage where a **real installation path** meets the
two-key dispatch and the trailer index.

The mirrored half of the same test: a container whose bytes do not carry the
documented structure its role promises (a `sounds*.zbd` whose trailer is not
version one) is refused with the index error rather than read as an empty
archive.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/zbd/sound_sample.rs` (new): the **sample decoder**
  and the budget it is charged against —
  `SAMPLE_ENTRYPOINT`, `SampleFormat` (a decoded *plan* built from a
  `SoundDescriptor`: encoding, channels, rate, block align, bits per sample),
  `SampleFormatError`, `DecodedSound` (frames, samples, per-channel values,
  byte length), `SampleError` (`Undeclared`, `PartialFrame`, `UnsupportedFormat`,
  `BlockAlignMismatch`, `Parse`) and `decode_sound_sample`.
- `crates/cs_formats/src/zbd/mod.rs` (wiring): the module declaration, the
  `pub use` re-exports and the module-doc sentence for stage F06-C.
- `crates/cs_assets/src/zbd.rs` (new): the producer and the consumer —
  `ZbdContainer` (`open`, `routing`, `family`, `path`, `span`, `generation`,
  `require_session`, `index`, `sound_archive`, `sound_assets`),
  `ZbdRouting`, `ZbdError`, `SoundAsset`, `SoundAssets`, `SoundReadiness`.
- `crates/cs_assets/src/lib.rs` (wiring): `pub mod zbd;` and the crate-doc
  sentence naming F06-C.
- `crates/cs_formats/tests/zbd/samples.rs` (new) plus one `mod samples;`
  line in `crates/cs_formats/tests/zbd/main.rs`: the `accept_f06_c_*` tests
  for the decoder, including the stage's minimum scenario.
- `#[cfg(test)] mod tests` inside `crates/cs_assets/src/zbd.rs`: the
  `accept_f06_c_*` tests for the VFS producer and the audio assets
  (`crates/cs_assets/tests/` is not an owner path of this task, so the
  inline-test shape F04-C used in `tools/cs_inspect/src/resolve.rs`
  applies here).
- `docs/findings/2026-09-28-f06-c-vfs-members-and-audio-assets.md`
  (this file).

**Not created in this stage:** `tools/cs_inspect/src/zbd.rs`. F06-B's
findings deferred it and the acceptance case it would carry — "show a corrupt
member alongside valid siblings in audit output while returning a nonzero
strict status" — is F06-D's **minimum scenario** (spec F06 AC04, the stage
with the `retail` capability and the private corpus).

## What this stage is, now that the member index and the descriptor exist

* **The producer is the VFS plus the container's own trailer.**
  [`ZbdContainer::open`](cs_assets/src/zbd.rs) resolves a ZBD container key in
  a content session, reads its bytes, rebuilds the **installation-relative**
  [`RelativePath`] from the resolution's immutable [`SourceSpan`] (a
  *validated* join — `RelativePath::new`, never an unchecked concatenation),
  and runs the two-key dispatch. [`ZbdContainer::index`] then reads the
  container's **own** version-one trailer (task #343) and hands the readers
  the member table the archive declares. Nothing here invents a member index,
  a family or a format.
* **The consumer is the audio asset.** Each sound entry carries the WAVE
  header task #344 read; the decoder turns the `data` payload into frames
  and per-channel sample values according to that header, and the asset
  reports what it could decode and what it could not. A member whose header
  does not read, or whose `data` length is not a whole number of declared
  blocks, is a row with a recorded reason — never a truncated sample and
  never a claim of playability.
* **Error propagation and teardown** are as in the first plan: one
  [`ZbdError`] with a stable `code()` and a `source()` chain, a container
  that owns its bytes and outlives its session, a session-generation guard
  against cross-world reuse, and a listing that can be retried on a funded
  parse context.

## Recorded unknowns (not guessed)

- Every unknown recorded by #340, #343 and #344 stands and is adopted here
  rather than re-derived: the 76 unexplained bytes of every index entry, the
  loop points no member declares, and the format tag names.
- **This stage decodes PCM only.** Every retail member is IMA ADPCM, MS ADPCM
  or 8-bit PCM (task #344 findings). An ADPCM member is listed as an
  `UnsupportedFormat` row carrying its own declared tag — the tag is known
  and the block decoding is a separate, checked piece of work, not something
  to approximate here.
- Whether the original engine's mixer resamples, filters or spatially
  positions these samples is unknown and is F41's, not this stage's.
- Nothing here reads `$CS_GAME_DIR`; every fixture is authored synthetic
  bytes.

## What was implemented

`crates/cs_formats/src/zbd/sound_sample.rs` — the decoder, and the budget it
is charged against:

* `PcmLayout::{Unsigned8, Signed16Le, Signed32Le}` — the conventional
  RIFF/WAVE PCM widths (`wBitsPerSample` 8, 16, 32). These are the *format
  vocabulary*, not a property of any archive: a member declaring ADPCM, or a
  width this stage does not decode, is refused with its own declared value.
* `SampleFormat::from_header` / `from_descriptor` — the decode plan, built out
  of what **the member's own** WAVE header declares (task #344 read it). Every
  field is read by matching on its `SoundField`, so an undeclared field
  produces its own recorded reason and nothing is filled in from elsewhere.
  The plan is refused unless the declaration is self-consistent:
  `nBlockAlign` must equal `nChannels * bytes_per_sample`, or the header
  contradicts itself.
* `decode_sound_sample(context, member, format)` /
  `decode_payload(context, payload, format)` — the `data` payload into frames
  and per-channel `i32` values, exactly as stored (no resampling, filtering,
  centring, scaling or mixing). The buffer is booked against the parse's
  allocation budget before it exists; refusals are `PartialFrame` (the payload
  is not a whole number of declared frames) and the format refusals above.
* `SampleError` with a stable `code()`, a `reason()` that quotes the member's
  own explanation where it has one, and `from_wave_header` for a member whose
  header did not read.

`crates/cs_assets/src/zbd.rs` — the producer and the consumer:

* `ZbdContainer::open(session, key)` resolves, reads, rebuilds the
  **installation-relative** `RelativePath` from the resolution's immutable
  `SourceSpan` (validated), and runs the two-key dispatch. A container no key
  names is **refused**, not guessed at.
* `ZbdContainer::index(context)` reads the container's **own** version-one
  trailer (task #343) — the member index is what the archive declares.
  `sound_archive` / `sound_assets` take the index and its `MemberTable`, so
  nothing can hand out an archive that outlives its own index.
* `SoundAssets` / `SoundAsset` / `SoundReadiness` — one row per readable
  member, carrying its identity, span, its own WAVE header and its readiness
  (`Decoded`, `UnsupportedFormat`, `UnreadableHeader`, `Undecodable`). Each
  member is decoded under **its own** header.
* `ZbdContainer::require_session` (generation guard), owned bytes that outlive
  the session, and one `ZbdError` with a stable `code()` and a `source()`
  chain.

## Design decisions

- **The declaration is the member's, never the engine's.** Nothing in this
  stage fills in a channel count, a rate, a block align or a format tag. The
  retail facts task #344 measured — the ADPCM tags, the widths, the
  block-align shapes — are the *inputs* the decoder reasons about, and a
  member that declares something this stage does not decode is refused
  carrying that declaration, so the row is visible and honest.
- **PCM only, and the refusal is the point.** Task #344 measured that nearly
  every retail member is IMA or MS ADPCM. Decoding them is a separate, checked
  piece of work; approximating them here would put invented audio in the
  engine. An ADPCM member is `UnsupportedFormat { tag, name }` with the tag
  *it* declares.
- **The producer is the VFS plus the container's own trailer.** The member
  index is not a caller input any more (task #343), and this module does not
  reintroduce one: `ZbdContainer::index` reads the archive, and the readers
  are handed `VersionOneIndex::data()` — the bytes *before* the index — so a
  member reaching into the index fails its own bounds check.
- **A container nothing routes is refused.** `ZbdDispatchError::UnknownFamily`
  at open time, rather than a guessed family: "no key names a family" must
  mean "this is not a ZBD container this stage may read".
- **The composed path is validated, not concatenated.** The
  installation-relative path is a composition of a mount's container label and
  a member spelling, so it goes through `RelativePath::new` (IDENTITY-CONTENT:
  "no unchecked path join"). A mount labelled `..` is refused with
  `container_path` before dispatch sees it.
- **Owned bytes, stamped generation.** The container outlives the session that
  read it (spec F04 non-negotiable behavior 4) and `require_session` refuses
  it to a replacement session after a world switch.
- **The tests live inline in `crates/cs_assets/src/zbd.rs`.**
  `crates/cs_assets/tests/` is not an owner path of this task; the inline-test
  shape is the one F04-C used in `tools/cs_inspect/src/resolve.rs`. The
  `cs_formats` half is in `crates/cs_formats/tests/zbd/samples.rs`, which *is* an
  owner path.
- **The fixtures are complete RIFF/WAVE members and complete version-one
  archives**, written by the test modules to the layouts task #343 and #344's
  sources document, so the production header and index readers parse them
  exactly as they parse a real member. Nothing is hand-fed to the decoder
  that the decoder did not read out of a member's own bytes.

## Test inventory (`accept_f06_c_*`, 20 tests: 9 in `cs_formats`, 11 in
`cs_assets`)

`crates/cs_formats/tests/zbd/samples.rs` (9):

| Test | Covers |
| --- | --- |
| `a_short_sound_sample_matches_its_declared_byte_and_sample_count` | **AC03, the stage's minimum scenario**: a member's own header declares its format, its payload is decoded under exactly that, and `byte_len == sample_count * bytes_per_sample` and `byte_len == frames * nBlockAlign` are both asserted, with the values checked sample by sample |
| `the_declared_format_decides_what_a_frame_is` | the same 32 bytes frame three ways purely from each member's own header: 32 8-bit frames, 8 stereo frames of 2 values, 8 32-bit frames; and a 32-bit frame reads the stored bytes as one little-endian value rather than reinterpreting the samples |
| `a_data_payload_that_is_not_whole_frames_is_refused_not_truncated` | `partial_frame` with the exact remainder; the refusal charges nothing and another member still decodes |
| `a_member_declaring_a_compressed_format_is_refused_with_its_own_tag` | the two ADPCM tags task #344 measured are refused carrying the member's own tag and RFC 2361 name |
| `a_declaration_this_stage_cannot_decode_is_refused_with_its_own_value` | 24-bit PCM is `unsupported_width`; a non-RIFF member is `unreadable_header` with the header reader's own reason |
| `a_header_that_contradicts_itself_is_refused` | an `nBlockAlign` that disagrees with channels × width is `block_align_mismatch`, naming both numbers |
| `the_decode_is_bounded_by_the_parse_allocation_budget` | the buffer is booked before it exists; one byte short is refused, scoped `zbd.sample.sample.values`, rolled back, and the exact charge decodes |
| `an_empty_payload_is_zero_frames_not_an_error` | a member with an empty `data` chunk holds no samples, which is a whole number of zero frames |
| `the_data_payload_can_be_decoded_on_its_own` | `decode_payload` is the same decode over a payload a caller already holds, and produces an identical result and charge |

`crates/cs_assets/src/zbd.rs` (11, all through a mounted content session over
authored fixture trees):

| Test | Covers |
| --- | --- |
| `an_observed_sound_container_routes_to_the_sound_family_and_reads_its_own_index` | **the stage's observable failure, positive half**: `ZBD/soundsl.zbd` routes to the **sound** family on task #340's observed role with the header unvalidated; the path is rebuilt from the resolution; the member index is the archive's own trailer with its three names, extents and 76 unexplained bytes retained |
| `a_reader_archive_is_never_read_as_sound` | **the observable failure, main case**: `zrdr.zbd` routes to the **reader** family and both `sound_archive` and `sound_assets` refuse with `family_mismatch` |
| `a_container_no_key_names_is_refused_rather_than_read` | a file outside `zbd/` is `unknown_family` at open time, never read as some family |
| `a_resolution_whose_path_would_escape_is_refused_before_dispatch` | a `..` container label composes to an escaping path and is refused with `container_path` |
| `an_unresolvable_key_and_a_changed_member_both_refuse_with_their_own_code` | `resolve` and `read` codes; the mount-time digest check refuses changed bytes |
| `a_sound_container_becomes_audio_assets_with_the_samples_it_declares` | **AC03 through the VFS**: the decoded PCM member's byte and sample counts are the ones its header implies, its values are its stored samples, and the two ADPCM members are `UnsupportedFormat` with the tags they declare |
| `a_member_whose_header_does_not_read_is_a_row_with_its_own_reason` | a non-RIFF and a too-short member are rows carrying the header reader's own reasons, while their sibling stays decoded |
| `a_member_reaching_into_the_index_fails_its_own_row_only` | the readers get the bytes *before* the index: a lying extent is `member_out_of_bounds` on its own row, counted, while its honest sibling is decoded |
| `a_container_outlives_its_session_and_is_refused_by_another` | teardown and stale state: the container and its assets stay usable after `close()`, and a replacement session refuses the previous generation |
| `a_listing_refused_by_a_starved_budget_can_be_retried` | retry: a zero-budget context refuses the index, leaves the ledger and depth alone, and a funded context reads the same container |
| `an_index_read_from_another_container_is_refused` | added in review: `sound_archive`/`sound_assets` refuse an index sliced from another container's bytes, or a table carrying another container's label, with `foreign_index`, instead of applying foreign extents to this container |

## Mutation probes

Each mutation was applied to production code, the F06-C selection run, and the
file restored with `git checkout`. `cargo test --workspace --locked --
accept_f06_c_` (19 tests).

| Mutation | Failing test |
| --- | --- |
| the two-key dispatch is dropped and the family guessed from the path | `a_reader_archive_is_never_read_as_sound` |
| a container no key names is opened with a guessed family | `a_container_no_key_names_is_refused_rather_than_read` |
| the readers are handed the whole archive instead of `VersionOneIndex::data()` | `a_member_reaching_into_the_index_fails_its_own_row_only` |
| the ADPCM members are reported as decoded | `a_sound_container_becomes_audio_assets_…`, `a_container_outlives_its_session_…` |
| an unreadable header is reported as an unsupported format | `a_member_whose_header_does_not_read_…` |
| the composed path skips `RelativePath` validation | `a_resolution_whose_path_would_escape_…` |
| the session-generation check always passes | `a_container_outlives_its_session_…` |
| a partial trailing frame truncated instead of refused | `a_data_payload_that_is_not_whole_frames_…` |
| the `nBlockAlign` consistency check dropped | `a_header_that_contradicts_itself_is_refused` |
| the sample buffer not booked against the budget | `the_decode_is_bounded_by_the_parse_allocation_budget` |
| a multi-channel frame yields one value per frame | `the_declared_format_decides_what_a_frame_is` |
| an unsupported PCM width decoded as 8-bit | `a_declaration_this_stage_cannot_decode_…` |
| a non-PCM member decoded as PCM | `a_sound_container_becomes_audio_assets_…` |

Two probes initially changed **no** result and are recorded rather than
hidden; both were gaps in the fixtures, not in the production code, and both
are now closed:

* *"the readers are handed the whole archive"* changed nothing until
  `accept_f06_c_a_member_reaching_into_the_index_fails_its_own_row_only`
  existed, and then only once its lying extent was shortened to run past
  `table_start` but not past the whole archive — the only length that
  distinguishes the two.
* *"an unreadable header reported as unsupported-format"* needed the
  `UnreadableHeader` case, which the first run of that test did not assert.

## Recorded unknowns and limits

- Everything #340, #343 and #344 recorded is adopted here rather than
  re-derived: the 76 unexplained bytes of every index entry, the loop points
  no member declares, and the format-tag names.
- **ADPCM is not decoded.** Every retail member is IMA or MS ADPCM except 11
  8-bit PCM ones (task #344), so in production this stage decodes almost
  nothing. That is the honest state, not a gap in the wiring: block decoding
  is a separate, checked piece of work and belongs in its own stage.
- Loop points remain unknown for every member (no `smpl` chunk, task #344), so
  no decoded sample carries a loop range here.
- The decoded values are the stored samples widened to `i32`. Whether the
  original engine resamples, filters, mixes or spatially positions them is
  unknown and is F41's work.
- The container is read **whole** into owned memory. A ZBD sound archive can
  be tens of megabytes; a streaming member read is F15's asynchronous asset
  loading, and this stage deliberately holds one container at a time.
- No evidence-bound capability was used, so no evidence report is required and
  none was produced. `$CS_GAME_DIR` was not read; every fixture is authored
  synthetic bytes under the system temporary directory.
