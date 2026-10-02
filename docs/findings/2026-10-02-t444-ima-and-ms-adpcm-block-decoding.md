# Task #444: IMA and MS ADPCM block decoding of the sound archive members

Date: 2026-10-02. Task #444 "Decode IMA and MS ADPCM sound members in
`cs_formats`", a follow-up to tasks #340, #343 and #344
(`docs/findings/2026-09-28-t340-zbd-family-headers-and-archive-names.md`,
`docs/findings/2026-09-28-t343-zbd-version-one-member-index.md`,
`docs/findings/2026-09-28-t344-zbd-sound-member-wave-headers.md`) and to stage
`### F06-C` (`docs/findings/2026-09-28-f06-c-vfs-members-and-audio-assets.md`).
Capabilities used: `retail` (read-only, `$CS_GAME_DIR`) plus ordinary
build/test. Test prefix: `accept_t444_`. Evidence:
`docs/findings/evidence/T444.json`.

## What this task decoded, and what it did not

Stage F06-C reads a member's RIFF/WAVE header and decodes uncompressed PCM;
task #344 measured that **every** retail sound member is one of three formats —
PCM, Microsoft ADPCM (`wFormatTag` `0x0002`) or Intel/IMA ADPCM (`0x0011`) — so
almost the whole corpus was reported `SampleFormatError::UnsupportedFormat`
carrying its own tag. This task decodes the two block layouts, one enum variant
per tag, in `crates/cs_formats/src/zbd/adpcm.rs`, and teaches
`crates/cs_formats/src/zbd/sound_sample.rs` to plan them from a member's own
bytes.

Two entry points exist, and that is deliberate:

| Entry point | Decodes | Used by |
| --- | --- | --- |
| `SampleFormat::from_header` (stage F06-C) | uncompressed PCM only; a compressed tag is refused as `UnsupportedFormat` carrying its own tag and RFC 2361 name | stage F06-C's own tests; **not** the runtime consumer since Rally #524 |
| `SampleFormat::from_member` / `from_header_with_blocks` (this task) | PCM, IMA ADPCM and MS ADPCM, including the `fmt ` extension | `crates/cs_assets/src/zbd.rs` (`SoundAsset`, `SoundAssets::new`) since Rally #524 |

The F06-C entry point is unchanged and still refuses a compressed member with its
own tag, and its F06-C tests still pass; the two entry points are pinned apart by
`accept_t444_the_pcm_entry_point_still_refuses_a_compressed_member_with_its_tag`.
The consumer switch was outside this task's owner paths, so it was filed on its
own (Rally #524, "Switch the `cs_assets` sound consumer to the block-aware decode
plan") and has since landed: `cs_assets` now plans each member through
`SampleFormat::from_header_with_blocks`, so every retail sound member — 5,019
compressed and 22 PCM — is reported `SoundReadiness::Decoded` rather than
`UnsupportedFormat`.

## Sources

- **RIFF/WAVE container, `fmt ` chunk and `cbSize` extension:** IBM Corporation
  and Microsoft Corporation, *Multimedia Programming Interface and Data
  Specifications 1.0*, August 1991, sections "RIFF File Format" and "WAVE Form
  Type" (task #344's source, reused here for the container and for the
  format-specific `fmt ` tail).
- **Format tag names:** RFC 2361, "WAVE and AVI Codec Registries" (1998),
  appendix A: `0x0001` `WAVE_FORMAT_PCM`, `0x0002` `WAVE_FORMAT_ADPCM`
  (Microsoft), `0x0011` `WAVE_FORMAT_DVI_ADPCM` (Intel, IMA ADPCM).
- **The two block layouts and the three codebooks:** the primary algorithm
  documents (the IMA ADPCM codebook as published with the AIFF `ima4`
  specification, and the Microsoft ADPCM block format as published with the
  Windows Media codecs) could **not** be re-fetched in this session, so the
  values in `adpcm.rs` are taken from an independent implementation and then
  **verified against the retail data**, which is the claim this task makes:

  > FFmpeg, `libavcodec/adpcm_data.c` — `ff_adpcm_step_table` (89 entries),
  > `ff_adpcm_index_table` (16 entries), `ff_adpcm_AdaptCoeff1`/`AdaptCoeff2`
  > and `ff_adpcm_AdaptationTable`, and `libavcodec/adpcm.c` —
  > `adpcm_ima_wav_expand_nibble`, `ff_adpcm_ima_qt_expand_nibble` (the
  > four-bit WAVE path), `adpcm_ms_expand_nibble`, `get_nb_samples` and the
  > `CASE(ADPCM_MS, ...)`/`CASE(ADPCM_IMA_WAV, ...)` block readers
  > (`https://ffmpeg.org/doxygen/trunk/adpcm_8c_source.html`,
  > `adpcm__data_8c_source.html`). The reviewer re-read those files on the
  > current trunk and the cross-check itself ran against FFmpeg 8.1.1;
  > FFmpeg's own header lists its ADPCM reference sources.

  FFmpeg's `AdaptCoeff1`/`AdaptCoeff2` are the retail coefficient table divided
  by four, which is the same table the format stores undivided: `64, 128, 0,
  48, 60, 115, 98` times four is `256, 512, 0, 192, 240, 460, 392`, and the pairs
  `0, -64, 0, 16, 0, -52, -58` times four are `0, -256, 0, 64, 0, -208, -232`. The
  measured retail table is exactly that. The table was *not* taken from the retail
  data into the code: `MsAdpcmCoefficients` is read from each member's own `fmt `
  chunk, and a test decodes a member that declares a different table to prove it
  (`accept_t444_the_block_decode_uses_the_coefficients_the_member_declares`).
- **Retail installation:** the same install fingerprint task #344 used,
  `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`.

## Code

`crates/cs_formats/src/zbd/adpcm.rs` (new) holds the block layer:

- `read_adpcm_extension(tag, fmt)` reads the `fmt ` payload's format-specific
  tail. IMA needs `cbSize` + `wSamplesPerBlock` (4 bytes); MS needs `cbSize`,
  `wSamplesPerBlock`, `wNumCoefs` and that many coefficient pairs (6 bytes plus
  4 per pair). A tag with no tail bytes is `AdpcmExtension::Absent`; a tail too
  short for its tag is `AdpcmExtension::Short { tag, declared_len, needed }`.
  Nothing is read outside the member's own `fmt ` span.
- `AdpcmLayout::{Ima, Ms}` carries only what the member declared: the
  `wSamplesPerBlock` and, for MS, the member's coefficient table.
- `block_sample_count`, `full_block_sample_count` and `sample_count` are pure
  geometry: they say how many values a block, a full block and a whole payload
  hold, from the layout's documented byte layout alone. `sample_count` is what
  `sound_sample` books against the parse's allocation budget **before** the
  buffer exists, so a hostile `data` length costs a refusal and not an
  allocation; the decode is then checked against it with `debug_assert`, so the
  booked count and the decoded count cannot drift apart.
- `decode` walks the payload block by block and appends each block's values to
  the caller's buffer in the order the block stores its data: frame by frame and
  channel by channel, which for a stereo MS block is the two history values of
  every channel, then one frame per nibble byte.
- `AdpcmError` names the block's byte offset, the sizes involved and, where one
  is named, the offending step index or coefficient index. It never carries
  sample bytes.

`crates/cs_formats/src/zbd/sound_sample.rs` gains:

- `SampleLayout::{Pcm, Adpcm}` replacing the bare `PcmLayout` in `SampleFormat`,
  with `pcm()`, `adpcm()`, `is_block_coded()`, `bits_per_sample()` and
  `stored_bytes_per_sample()` (the last is `None` for a block codec: a nibble is
  half a byte and the block is the unit, so there is no per-sample byte count to
  report).
- `SampleFormat::from_member` and `from_header_with_blocks`, which read the
  header and the `fmt ` extension out of the member's own bytes and check the
  declaration: four bits per sample, a block size that can hold the layout's
  block header, a block size that divides into whole per-channel nibble groups,
  a `wSamplesPerBlock` equal to what one full block of that size holds, and a
  channel count the layout is read for. Each refusal names the condition it
  found: a block that cannot hold its own header is `BlockAlignTooSmall`, one
  that cannot be split into the declared channels is the `PartialBlock` error
  itself rather than a block size that is large enough, and a MS ADPCM
  `wNumCoefs` above the seven the layout reserves is
  `AdpcmCoefficientTableTooLong` rather than a `fmt ` length shortfall the
  member does not have.
- `SampleError::Block(AdpcmError)`. The typed block error is carried out of the
  `ParseContext::parse` attempt and reported as itself; the attempt still fails,
  which is what rolls the reservation back, so a corrupt member leaves no charge
  behind for its siblings.
- `DecodedSound::byte_len` is now the payload length the decode accounted for.
  For PCM that is still `frames * frame_bytes` (a PCM payload must be a whole
  number of frames, or it is refused as `PartialFrame`); for a block codec it is
  the payload's own length, because the format's final block may be shorter than
  `nBlockAlign`. `frames()` counts blocks for a block codec, a trailing short
  block included, and `samples_per_frame()` is `wSamplesPerBlock * nChannels`
  there, so `frame(index)` splits the values the same way for both layouts —
  except for a trailing short block, which `frames()` counts and `frame()`
  answers `None` for, because the payload ends inside it.

## The two block layouts, as read and as measured

**IMA (DVI) ADPCM, `0x0011`, four bits per sample.** A block is `nBlockAlign`
bytes: an `i16` initial predictor, a `u8` step index, one reserved byte, then
the nibbles **low nibble of each byte first**. A block therefore holds
`1 + 2 * (nBlockAlign - 4)` samples. Each nibble moves the predictor by
`step / 8` plus `step / 4`, `step / 2` and `step` for each magnitude bit set,
subtracted when bit 3 is set, clips to `i16`, and then moves the step index by the
index table, clamped to `0..=88`.

**Microsoft ADPCM, `0x0002`, four bits per sample.** A block is `nBlockAlign`
bytes and starts with seven bytes per channel: a `u8` index into the member's own
`aCoefs`, an `i16` initial delta and the two `i16` history samples. The header
fields are stored **grouped by field across the channels** — every channel's
coefficient index, then every channel's delta, then every channel's newer history
sample, then the older one. The two history values are the block's first two
output values, in the order the layout stores them: the older sample first. The
encoded nibbles follow, **high nibble of each byte first**: a mono byte holds the
channel's next two samples, and a two-channel byte holds the first channel's next
sample in its high nibble and the second's in the low one. Each nibble predicts

```text
sample = (sample1 * aCoefs[index].predictor + sample2 * aCoefs[index].difference) / 256
         + (nibble >= 8 ? nibble - 16 : nibble) * delta
```

with the division truncating towards zero, the result clipped to `i16`, and then
`delta = max(16, ADAPTATION_TABLE[nibble] * delta / 256)`. The delta is capped at
`i32::MAX / 768`, the same bound to the same value FFmpeg's `adpcm_ms` applies
as its `idelta overflow` guard, so a decode that reaches the cap agrees with
that implementation instead of drifting from it; the cap also keeps
`delta * nibble` inside `i32`, and while the delta sits at it a nonzero nibble
moves the predictor further than any `i16` can hold, so the predictor clip is
what decides the value.

Both relations hold exactly for every retail member:

| Tag | Shape | Members | `wSamplesPerBlock` | geometry |
| --- | --- | --- | --- | --- |
| `0x0011` | mono, 11025 Hz, align 256 | 555 | 505 | `1 + 2 * (256 - 4)` |
| `0x0002` | mono, 11025 Hz, align 256 | 3,908 | 500 | `2 + 2 * (256 - 7)` |
| `0x0002` | mono, 22000 Hz, align 512 | 3 | 1012 | `2 + 2 * (512 - 7)` |
| `0x0002` | mono, 22050 Hz, align 512 | 470 | 1012 | `2 + 2 * (512 - 7)` |
| `0x0002` | mono, 44100 Hz, align 1024 | 24 | 2036 | `2 + 2 * (1024 - 7)` |
| `0x0002` | stereo, 9710 Hz, align 512 | 1 | 500 | `2 + 2 * ((512 - 14) / 2)` |
| `0x0002` | stereo, 22050 Hz, align 1024 | 58 | 1012 | `2 + 2 * ((1024 - 14) / 2)` |

5,019 of the two archives' 5,041 members are compressed (the other 22 are PCM);
every one of them decodes, and every declared `wSamplesPerBlock` matches the
geometry above, which is why `SampleFormat` refuses a member whose declaration
does not. The IMA `fmt ` payload is 20 bytes with `cbSize` 2; every MS ADPCM
`fmt ` payload is 50 bytes with `cbSize` 32, `wNumCoefs` 7 and one coefficient
table across all 4,464 members:

| Index | `aCoefs[i][0]` | `aCoefs[i][1]` |
| --- | --- | --- |
| 0 | 256 | 0 |
| 1 | 512 | -256 |
| 2 | 0 | 0 |
| 3 | 192 | 64 |
| 4 | 240 | 0 |
| 5 | 460 | -208 |
| 6 | 392 | -232 |

`nAvgBytesPerSec` is consistent with `rate * nBlockAlign / wSamplesPerBlock`
truncated for every shape (5,644; 11,130; 11,155; 22,179; 9,943; 22,311; 5,588),
which is a third, independent confirmation that the declared block geometry is
the one being decoded.

## Cross-check against an independent implementation

The layouts above were checked against FFmpeg's `adpcm_ima_wav` and `adpcm_ms`
decoders — a separate implementation of the same two formats — on the real
members, not on synthetic ones. For one member of every distinct retail shape
the harness decodes the member with production code, extracts it into the
private evidence directory, decodes the extracted file with FFmpeg to signed
16-bit PCM, and compares the two sample sequences value for value, lengths
included. All seven shapes agree exactly (6,091,417 samples over the seven
members compared). The numbers are in the evidence report's `review.method`
text, the extracted members and both decodes stay in the private evidence
directory, and the comparison runs again whenever the harness is run with
`CS_FFMPEG` set.

This is a **format** cross-check, not original reference evidence: FFmpeg is
another implementation of the same published layouts, and no original
Crimson Skies decode was available to compare against. What it establishes is
that this crate's decode of these files equals another implementation's, on the
actual data, for every shape the installation contains. One limit of it is
worth naming: FFmpeg's MS ADPCM decoder applies its own copy of the coefficient
table rather than the member's, so it can only confirm a member that declares
that same table. The retail members all declare it (one table across all 4,464
of them, measured above); this crate reads the member's own table, which is what
the format says to do, and a member declaring another table is decoded with it
and is expected to differ from FFmpeg.

## Unusual and unobserved cases, recorded as found

- **`nSamplesPerSec` of 22000 and 9710** (3 and 1 members) are read as declared.
  Nothing here decides whether they are intended.
- **Stereo members exist only for MS ADPCM** (59 members); every IMA member is
  mono. A multi-channel IMA block would have to place its channels' nibbles in
  some order this crate has not read and no retail member exercises, so it is
  refused (`AdpcmChannelsNotObserved`) rather than guessed. MS ADPCM beyond two
  channels is refused the same way.
- **No retail member's `data` payload ends in a short block**: every payload is a
  whole number of `nBlockAlign` blocks. The layouts do define a shorter final
  block, and this crate decodes it from the same geometry
  (`accept_t444_a_trailing_short_block_is_decoded_by_the_documented_geometry`),
  but that case is covered by synthetic tests only.
- **Blocks do not continue each other.** Each block restates its predictor (IMA)
  or its two history samples (MS) in its own header, and on the retail data those
  stated values are usually *not* the previous block's last output values: 1,241
  of 112,929 IMA block boundaries and 5,406 of 773,332 MS block boundaries
  restate a value that matches, i.e. 111,688 and 767,926 do not. A decoder must
  therefore reset its state per block, and this crate does. This says nothing
  about how the game played the sound.
- **Two MS ADPCM first-block layouts are described in print.** Some references
  give the first block a two-byte initial predictor and a three-bit initial step
  index per channel with two-byte deltas afterwards; the layout read here gives
  every block, the first included, seven header bytes per channel. What decides
  it is evidence, not preference: the retail members declare
  `wSamplesPerBlock` equal to what the seven-bytes-per-channel layout holds for
  their `nBlockAlign` with no slack in the first block, `nAvgBytesPerSec` agrees
  with that geometry, FFmpeg's `CASE(ADPCM_MS, ...)` reads exactly those seven
  bytes per channel grouped by field in every block and its own sample-count
  formula reduces to the same count, and decoding all 5,019 members under this
  layout leaves 0.195% of samples at the `i16` clip (721,561 of 370,748,400
  across both archives), which is what a decode of a real signal looks like
  rather than one that has lost its predictor. A decoder reading the other
  layout would produce different samples from the same bytes. What remains
  unknown is what the original executable did: nothing here shows which layout
  *it* read, only that the installed members fit this one and that this one
  decodes them.
- **No retail member carries a loop point** and none declares a `smpl` chunk
  (task #344), so this decode produces a whole member and nothing decides
  whether the game loops it.
- **What the original executable did with the samples** — pitch, volume,
  spatialisation, mixing — is not decided by these bytes and is not decided
  here. That is F41's work, and the decoded values are handed over unchanged.

## The zero-extension ADPCM classification recorded for #530

`read_adpcm_extension` returns `AdpcmExtension::Absent` for a known ADPCM tag
whose `fmt ` payload holds only the 16 common fields, while a payload with 1 or
more but still too few extension bytes is `AdpcmExtension::Short`. Through
`SampleFormat::from_declared` the two become different refusals: `Short` becomes
`AdpcmExtensionShort`, which the runtime consumer reports as `Undecodable { code:
"adpcm_extension_short" }`, and `Absent` becomes `UnsupportedFormat` carrying the
member's own tag and RFC 2361 name, which it reports as
`SoundReadiness::UnsupportedFormat`. A bare-16 ADPCM member is therefore reported
`UnsupportedFormat` at runtime even though this crate does read its tag.

This was left as it is. The reasons are the code and the consumer contract, not
preference:

- Rally #524's consumer contract already pins the distinction.
  `accept_t524_a_block_coded_member_without_a_readable_fmt_extension_is_refused`
  in `crates/cs_assets/src/zbd.rs` asserts `SoundReadiness::UnsupportedFormat`
  for an IMA member with no extension, and `Undecodable { code:
  "adpcm_extension_short" }` only for the `short.wav` fixture whose MS payload
  stops after `cbSize`. Returning `Short` for a zero-extension known tag would
  change that published consumer row, and `crates/cs_assets/` is not an owner
  path of this work.
- `Absent` means "no ADPCM extension is declared or present", and the refusal it
  produces keeps the member's own tag and name visible, so no fact about the
  member is lost: an `UnsupportedFormat` row still says exactly which codec the
  member claims.
- Either classification still refuses the member; neither weakens a refusal. The
  F06-C `from_header` path is unaffected either way, because it refuses a
  non-PCM tag before any extension is consulted.

The alternative — returning `AdpcmExtension::Short { declared_len: 16, needed:
20 for IMA / 22 for MS }` for a known ADPCM tag with no extension bytes — is
therefore not taken. `crates/cs_formats`' own synthetic test
`accept_t444_a_compressed_member_without_a_readable_fmt_extension_is_refused`
pins the decision: it asserts `AdpcmExtension::Absent` and
`SampleFormatError::UnsupportedFormat` for the bare-16 IMA member, and
`AdpcmExtensionShort { declared_len: 18, needed: 22 }` for the MS member that
stops after `cbSize`.

## Tests

`crates/cs_formats/tests/zbd/t444.rs`, prefix `accept_t444_`:

| Test | What it pins |
| --- | --- |
| `an_ima_adpcm_member_decodes_the_blocks_its_header_declares` | the exact IMA ramp, worked out from the codebook and the quarter-step weights, plus counts and the frame split |
| `an_ima_block_saturates_by_clipping_and_clamps_its_step_index` | clipping at `i16`, the index clamp at the codebook's end, and the refusal of a step index past it |
| `an_ms_adpcm_member_decodes_the_blocks_its_header_declares` | the exact MS mono values from the member's own coefficients, delta and adaptation |
| `a_stereo_ms_adpcm_block_decodes_one_sample_per_channel_in_order` | the grouped-by-field header, the per-channel nibble order and the frame-by-frame, channel-by-channel value order |
| `the_ms_predictor_sums_both_history_samples_before_dividing` | one sum divided once, with the division truncating towards zero: a decoder that divided each term alone, or floored, lands elsewhere |
| `the_block_decode_uses_the_coefficients_the_member_declares` | a member whose table is not the retail one, and the refusal of an index its table lacks |
| `the_format_codebooks_are_the_ones_the_two_layouts_name` | the step, index and adaptation tables and the two block-header sizes |
| `a_trailing_short_block_is_decoded_by_the_documented_geometry` | a short final block counted and decoded from the geometry, `frame()` answering `None` for the frame the payload ends inside, a block too short for its header refused, and the rollback |
| `a_stereo_block_without_whole_channel_groups_is_refused` | the odd-nibble-byte refusal, while decoding a block and while planning one — the plan-time refusal is the `PartialBlock` error, not a block size that is large enough |
| `a_compressed_member_without_a_readable_fmt_extension_is_refused` | `AdpcmExtensionShort` with the member's own tag and lengths, `Absent`, an unnamed tag, and `AdpcmCoefficientTableTooLong` for a `wNumCoefs` the layout does not reserve |
| `a_declaration_the_block_layouts_cannot_honour_is_refused` | `wSamplesPerBlock` against the block size, a block too small for its header, a width that is not 4, and both unobserved channel counts |
| `the_block_decode_is_bounded_by_the_parse_allocation_budget` | the booked charge from the geometry, a refusal one byte short of it, and the funded retry |
| `the_pcm_entry_point_still_refuses_a_compressed_member_with_its_tag` | the deliberate split between the two entry points: `from_header` still refuses a compressed member with its own tag, and an unreadable header is refused by the new one |
| `retail_every_compressed_sound_member_decodes_to_its_declared_counts` | all 5,019 compressed members of both archives: every one decodes, every count matches the declared geometry, every MS member declares seven coefficient pairs, and the seven distinct shapes are the measured ones |

`accept_t444_retail_every_compressed_sound_member_decodes_to_its_declared_counts`
is `#[ignore = "requires CS_GAME_DIR"]`, so CI skips it and the implementer and
reviewer run it with `--include-ignored`. It decodes each member on its own
`ParseContext`: a shared context accumulates every member's charge and the whole
corpus is far larger than one parse's allocation budget.

The evidence harness `evidence_report_t444_writes_the_acceptance_report` is not
an acceptance test; it writes `private/evidence/T444/acceptance.json` and the
`zbd-adpcm-decode.json` artifact (per-archive counts, shapes, hashes and the
number of samples at the clip), and runs the FFmpeg comparison when `CS_FFMPEG`
names a binary. The extracted members and FFmpeg's decodes of them are written
beside it and listed in the report as artifacts, so the comparison's inputs can
be re-hashed; their contents stay in the private directory. Its doc comment
gives the four commands.