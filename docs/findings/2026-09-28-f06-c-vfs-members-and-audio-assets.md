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
