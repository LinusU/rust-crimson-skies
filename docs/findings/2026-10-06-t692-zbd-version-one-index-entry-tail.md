# Task #692: the 76 unexplained bytes of a version-one ZBD index entry

Date: 2026-10-06. Task #692 "Decide what the 76 unexplained bytes of a
version-one ZBD index entry mean", the follow-up #341 filed from its own
section F (`docs/findings/2026-10-05-f04-d-original-lookup-order.md`).
Capabilities used: `retail` (read-only, `$CS_GAME_DIR`) plus ordinary
build/test. Test prefix: `accept_t692_`. Evidence:
`docs/findings/evidence/T692.json` (with unresolved `unknowns`, so validate
without `--require-pass`).

## Sources

- **Pinned reference source**: mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` (`docs/research/SOURCES.md` S02),
  `crates/mech3ax-archive/src/archive.rs` — declares the region
  `garbage: Bytes<76>` and assigns it no meaning.
- **Same author's format documentation**:
  `https://terranmechworks.com/mech3doc/archive-files.html` says the 76 bytes
  were *intended* as `{u32 flags; u8 comment[64]; u64 time}` with `time` a
  Windows `FILETIME`, and warns that some archives hold unzeroed memory in
  the region. This states archiver intent only; it does not show any engine
  reading the fields, and it is not the pinned source — so it is cited for
  field *names*, not for runtime semantics.
- **Retail installation**: install fingerprint
  `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`, content
  fingerprint `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`
  (F02 production discovery; both quoted from the T343 evidence report, the
  same installation). The per-archive table (spelling, SHA-256, size, member
  count, word histogram, stamp bounds, mismatch counts) is the private
  artifact `zbd-index-tail.json`, referenced by digest in the report.
- **Owner's decrypted executable**: `crimson.decrypted.exe`, SHA-256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` (from
  `crimson.icd` `0e3b4724f045e0bedf7203cd40cdeb5b6e0b9a0bab78c3d04c278cb146e9833b`;
  provenance in the #341 finding). Addresses are virtual addresses, image
  base `0x400000`. Static analysis only — addresses are named, no bytes or
  decompiled code are committed.

## Measured region shape

The last 76 bytes of every 148-byte entry (`entry + 72`) split, on every
entry measured, as:

| Region bytes | Field | Slot name (archiver doc) | Measured content |
| --- | --- | --- | --- |
| 0..4 | `word` | `flags` | nonzero u32; distribution below |
| 4..68 | `name_again` | `comment` | byte-identical copy of the entry's 64-byte name field, padding included |
| 68..76 | `stamp` | `time` | nonzero u64; reads as a little-endian Windows `FILETIME` |

Measured through the production parser (`read_version_one_index` →
`IndexEntry::unexplained` → `word`/`name_again`/`stamp`) by
`accept_t692_retail_*`, over every archive production dispatch routes to a
version-one trailer index — the 62 `zrdr.zbd` reader archives plus
`ZBD/soundsh.zbd` and `ZBD/soundsl.zbd`, 6,334 entries in 64 archives. (Two
`mis_anim.zbd` files also end in bytes that read as version 1, but dispatch
routes them to the animation family and `read_version_one_index` refuses
them by design; their trailing `count` of 65,535 is not a real index.)

- `name_again == name_field` byte for byte on **all 6,334 entries, 0
  mismatches** — the comment slot is populated with the name everywhere on
  this installation. Whether the original archiver ever stored anything else
  there cannot be told from this data.
- `stamp` is a nonzero `FILETIME` on all 6,334 entries, all inside
  2000-08-26 — one build session:

  | Archives | Entries | Stamp window |
  | --- | --- | --- |
  | 62 `zrdr.zbd` | 1,293 | `126117504540000000` .. `126117508180000000` (2000-08-26T08:00:54Z .. 08:06:58Z, 42 distinct values) |
  | `soundsh.zbd` | 2,521 | `126117538340000000` exactly (08:57:14Z) |
  | `soundsl.zbd` | 2,520 | `126117540800000000` exactly (09:01:20Z) |

- `word` is never zero, and its distribution is per archive family and per
  member storage (see below).

## The word (`flags`): measured distribution, unestablished meaning

| Entries | Word | Members behind it (through `SoundEntry::wave` → `read_wave_header`) |
| --- | --- | --- |
| 1,293 reader | `2` | `.zrd` reader members (not WAVE) |
| 2,521 `soundsh` | `62` | MS ADPCM (`0x0002`) or PCM (`0x0001`) |
| 1,963 `soundsl` | `62` | MS ADPCM or PCM |
| 2 `soundsl` | `2` | PCM — `train2.wav`, `pilot_eject1.wav` |
| 555 `soundsl` | increasing series `29,688,754` .. `30,092,802` | all IMA ADPCM (`0x0011`), and *every* IMA member is in the series |

The series values are strictly increasing in index order (546 deltas of
724, a few boundaries of 700–2,460), all ≡ 2 (mod 4). They do **not** equal
member starts in `soundsl` or `soundsh`, do not correspond to same-name
`soundsh` starts, and there are no gaps between members for them to index —
the member extents tile the data region exactly. They are consistent with
the archiver documentation's warning that the region can hold unzeroed
memory (an increasing counter leaked into a `|2`-flagged slot), but that is
an account, not a measurement.

**What `2` encodes is not established.** The word is codec-correlated inside
the sound archives (`62` = MS ADPCM/PCM, the series = IMA ADPCM, `2` = the
two PCM members) and constant `2` across reader members. It is not an
archive version (it varies inside `soundsl`), not a member offset (the
series matches none), and not a count. Its semantic name stays
`unknown` — see *Limitations*.

## What the original engine does with the region: nothing read

Static analysis of `crimson.decrypted.exe` (SHA-256 above, VAs at image
base `0x400000`):

- The archive object's index readers copy the whole 148-byte record and use
  only `entry + 0` (`start`), `entry + 4` (`length`) and `entry + 8` (name):
  directory parse at `0x59dbf0`, name lookup at `0x59de50`/`0x59dd80`
  (a 148-byte record copy), member extent read at `0x59de50`.
- The only site that *writes* the fields is the member-add path at
  `0x59dcc0`: it `or`s the word at `entry + 72` with `2` and copies an
  optional caller-supplied name and `FILETIME` into `entry + 76` and
  `entry + 140`. **Both in-binary callers** (`0x5c01d0`, `0x5c05a0` — the
  save/archive writers) **pass null** for both optional arguments, so on
  this binary the populated-comment/time path is dead code written for the
  archiver, matching the `garbage` name in the pinned source.
- `CompareFileTime` is called in exactly two places. The loose-file
  override at `0x579c60` calls it at `0x579d6b` — and the archive side of
  that comparison is `GetFileTime` on the **`.zbd` file's own handle**
  (obtained at `0x59de00`, which seeks the archive file to the member
  start), i.e. the archive file's last-write time, **not** the entry's
  stored `stamp`. The other call (`0x405455`, inside the `0x405440`
  comparator) compares `+0x16c` FILETIMEs inside a different record type
  unrelated to 148-byte index entries.

**Answer to #341's open question:** the trailing `u64` is *not* the member
time the loose-override rule compares. The rule compares the loose file's
time against the archive *file's* own last-write time; the entry's stored
stamp has no reader in the audited index paths. It is archiver metadata —
a real `FILETIME` the tool wrote — not engine input. (Residual caveat:
static analysis cannot exclude a reader in code paths not audited; see
*Limitations*.)

## What production now exposes

`cs_formats::zbd::trailer` keeps `UnexplainedBytes` verbatim, located, and
labelled `ClaimStatus::Unknown` — the *meanings* are unexplained. The
measured *shape* is now readable raw:

- `UnexplainedBytes::word()` → the u32 (`flags` slot);
- `UnexplainedBytes::name_again()` → the second 64-byte field (`comment`
  slot);
- `UnexplainedBytes::stamp()` → the trailing u64 (`time` slot);

with `INDEX_UNEXPLAINED_WORD_BYTES`/`_NAME_BYTES`/`_STAMP_BYTES` (4 + 64 +
8 = 76) partitioning `INDEX_UNEXPLAINED_BYTES`. No accessor asserts a
semantics: `word` returns raw values whose meaning is unrecorded anywhere,
`name_again` returns bytes that happen to copy the name on this
installation, and `stamp` returns a `FILETIME`-shaped u64 the engine does
not read. `UNEXPLAINED_REASON` now says the subfields are exposed raw and
uninterpreted.

This was chosen over keeping all 76 bytes opaque (the shape is measured,
and tests should pin it through production accessors rather than test-local
offsets) and over naming typed fields `flags`/`comment`/`filetime` (those
are the archiver doc's *intended* roles; presenting them as established
would read a meaning into bytes the engine never uses).

## Limitations (recorded, not implied)

1. **The word's semantics are unknown.** `2`, `62` and the series correlate
   with member storage, but no original reader was found and no source
   names the values. Affected content: the `flags` slot of all 6,334
   version-one index entries.
2. **The `soundsl` series' meaning is unknown.** "Unzeroed counter" is the
   most consistent account (archiver doc warning + the `|2` write path),
   not a measurement. Affected content: the 555 IMA-ADPCM `soundsl`
   members' `flags` words.
3. **`name_again` as `comment` is intent only.** On this installation it
   always copies the name; whether the archiver could store a different
   comment, and whether anything ever read it, is unknown. Affected
   content: the slot on all 6,334 entries.
4. **`stamp` as member time is intent only.** It is a real `FILETIME`, but
   the one engine comparison found uses the archive *file's* time instead.
   Whether some unaudited original path reads the stored stamp cannot be
   excluded by static analysis alone — an owner-supplied capture would be
   needed to settle runtime use. Affected content: runtime semantics of
   the `time` slot on all 6,334 entries.
5. The executable evidence is **static analysis of one binary**. No
   runtime capture exists; findings name addresses and digests, not
   observed executions.

## Tests

- `accept_t692_tail_accessors_read_the_measured_subfields` (synthetic):
  authored tail through `read_version_one_index`; `word`/`name_again`/
  `stamp` read their offsets, the region still reassembles to the verbatim
  76 bytes, `evidence()` stays `unknown`.
- `accept_t692_retail_every_index_entry_tail_has_the_measured_shape`
  (`#[ignore = "requires CS_GAME_DIR"]`): all 64 dispatched archives,
  6,334 entries — name copy byte-identical, every word and stamp nonzero,
  stamps inside the measured family windows.
- `accept_t692_retail_word_distribution_is_family_and_codec_shaped`
  (`#[ignore = "requires CS_GAME_DIR"]`): the word distribution table
  above, pinned through the production parser and `read_wave_header`,
  including the two named `soundsl` PCM members and the series' bounds,
  monotonicity and ≡ 2 (mod 4) shape.
- `evidence_report_t692_writes_the_acceptance_report`: the CLI-EVIDENCE
  harness; emits `zbd-index-tail.json` (per-archive word histograms and
  stamp bounds, no archive bytes) and `acceptance.json` with the three
  `unknowns` above.
