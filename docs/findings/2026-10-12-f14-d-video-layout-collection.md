# F14-D.VIDEO-LAYOUT: the layout movie records as the video collection's second source

Date: 2026-10-12. Task #1351, stage `### F14-D` of
`specs/F14-canonical-content-catalog-and-dependency-closure.md`, the follow-up
the review correction of #573 (F14-D.VIDEO) filed. Capabilities used: `retail`
(read-only, `$CS_GAME_DIR`) plus ordinary build/test. Test prefix:
`accept_f14_d_video_layout_`. Evidence:
`docs/findings/evidence/F14-D.VIDEO-LAYOUT.json`.

## Problem

The #573 review correction
(`docs/findings/2026-10-12-f14-d-video-collection.md`) decompressed the
container the original raw scan could not see into and found
`GOSDATA/ASSETS/crimson.rof`'s `ASSETS/LAYOUT.CSV` member spelling
`CrimFlag.MPG` five times and `Final.MPG` once in `M` (movie) records — two
of the seven `.mpg` files the `PLAYAVI` source never names. The correction
predicted the re-measurement: rows 3 → 5, `unnamed_video_file` 7 → 5. This
stage reads that source; it does not bind the chapter names
`CAMPAIGNINTRO.SCRIPT` constructs — Rally #1352 (F14-D.VIDEO-CHAPTER) owns
that.

## What the second source is

`GOSDATA/ASSETS/crimson.rof` is the same version-1 ROF container the paint
shop stage already mounts (846 members, 418 zlib-compressed). The stage
mounts it through the production `cs_assets::rof::mount_rof_into` reader —
compressed bytes are decoded, never scanned raw — and reads
`ASSETS/LAYOUT.CSV` through `ConfigDocument`, the same keyed-list reader the
scrapbook and layout stages use.

A record is a video reference only when **both** hold:

- the measured schema classifies it as the member's own movie record —
  `RecordSchema::for_entry` returns `RecordSchema::Layout(RecordKind::Movie)`
  for any `*MOVIE`/`MOVIE`-style key whose first field is `M`; and
- field position 1 of the record value states a name ending `.mpg`,
  case-insensitively. That position is where the member's own records carry
  the file name (`MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0`,
  `CF_MOVIE=M,Final.MPG,0,0,0,250,250,1,0`); a movie record whose field is
  not an `.mpg` name is not a video reference and is counted under the new
  `movie_record_without_mpg_name` code rather than read as one.

On the owner's installation the member holds six such records:
`CrimFlag.MPG` five times — one in a `;Multimedia` block, one as
`CM_MOVIE=M,CrimFlag.MPG,…` under `[@CampaignIntro@]`, three more
`MOVIE=M,CrimFlag.MPG,…` — and `Final.MPG` once under `[@FinalCinema@]`.

## How two sources make one collection

`video_rows` reads each source separately and resolves their union:

- `PLAYAVI` names come first, layout names second, so a file both sources
  name keeps the FMV row's provenance and the layout record counts as the
  repeat. Resolution and identity are unchanged from #573: a name resolves
  against the inventory by its file-name component, case-folded the way the
  id grammar folds case; exactly one match is one row keyed by the file's own
  `install_file_key`, zero matches is `named_file_absent`, several is
  `ambiguous_file_name`, and a second name for an already-resolved file is
  `duplicate_video_name` (never a second row — the file is the identity).
- The edge claim names the record that stated the name:
  `f14.d.video.baseline.playavi` for a `PLAYAVI` name,
  `f14.d.video.baseline.layout` for a layout record. The layout rows' edge
  provenance carries a source span at the record's own line inside the
  **decoded** member — the member's stored offset plus the record's offset
  within the decoded bytes, the record's own length, the decoded member's
  SHA-256 — built the way `member_node_span` builds the engine/gun/scrapbook
  record spans. The `PLAYAVI` rows keep the resolved file's span as their
  edge provenance, as #573 established.
- `CollectionStatus.source` names **both** containers
  (`ZBD/zrdr.zbd + GOSDATA/ASSETS/crimson.rof`) so the field reports the whole
  source set the stage consults rather than one of them. A source that is
  absent, lacks its member or holds bytes that do not read yields no rows
  from itself: it is counted under the new `unreadable_video_source` code and
  its reason is named in `CollectionStatus.diagnostic`, because the other
  source's rows are real either way. Only when no source produces any row
  does the collection report itself unpopulated, with every reason in the
  diagnostic.
- All seven gap codes are always reported, zero included:
  `playavi_name_not_text`, `movie_record_without_mpg_name`,
  `named_file_absent`, `ambiguous_file_name`, `duplicate_video_name`,
  `unnamed_video_file`, `unreadable_video_source`. `unnamed_video_file` still
  counts every inventoried `.mpg` no name reaches, so the collection stays
  visible over every video-looking file the installation holds.

## Retail result

On the owner's installation (`$CS_GAME_DIR`, fingerprint in the evidence
report), re-measured rather than trusted from the correction's prediction:

- **5 rows** — `chap0.mpg`, `msopen1.mpg`, `zipper.mpg` (claim
  `f14.d.video.baseline.playavi`) and `crimflag.mpg`, `final.mpg` (claim
  `f14.d.video.baseline.layout`), all under `GOSDATA/ASSETS/GRAPHICS/MPG/`,
  each located by its own bytes and pointing at its own `install_file` row.
- `duplicate_video_name`: **4** — the four `CrimFlag.MPG` records after the
  first.
- `unnamed_video_file`: **5** — `chap1.mpg`…`chap5.mpg`, the names
  `CAMPAIGNINTRO.SCRIPT` constructs as `"chap" conv$(FC) ".mpg"`. Rally #1352
  owns the binding.
- Every other gap code is **0**; both sources read, so
  `unreadable_video_source` is 0 and the diagnostic is empty.
- The layout rows' edge provenance was verified against a second, independent
  mount of the container in the retail test: each span sits inside the
  member's decoded extent and digests the decoded member's own bytes.
- Denominator unchanged: a video is not launchable, the collection declares
  no root, and every row stays `Unavailable`/`Unparsed` with the explicit
  `f14.d.video.playback` unknown.

## What is still unknown (and why no row claims it)

- **The chapter videos.** `chap1`…`chap5` are constructed, not spelled —
  `"chap" conv$(FC) ".mpg"` — and what `FC` (engine callback 2151) holds and
  which chapter number binds to which file is Rally #1352's question, not
  this stage's. All five stay counted under `unnamed_video_file`.
- **Codec, duration, resolution, trigger.** Unchanged from #573: no original
  byte read here states one, the MPEG stream is not decoded, and no media
  player consumer exists — F40 measures those.
- **Which scene plays each video.** The `MOVIE`/`CM_MOVIE`/`CF_MOVIE` keys
  and the `[@CampaignIntro@]`/`[@FinalCinema@]` sections place the records
  inside the layout document, but nothing read here binds a section to a
  game event; that placement stays display context, not identity.
- **What the non-name fields mean.** Positions 2+ of the movie records
  (`0,0,0,250,250,…`) are consistent-looking numbers whose contract this
  stage did not read; they are not claimed as geometry, timing or flags.

## Open questions for the owner

1. Carried over from #573, still open: should F40's cutscene rows reuse these
   `video` rows as their media dependency, or is a cutscene a separate
   content kind that references a video? The collection is keyed by the media
   file itself so either consumer can attach.
2. `ASSETS/SCRIPTS/CTL.SCRIPT` resolves `"assets\\graphics\\mpg\\" BL` at
   play time. When #1352 binds the chapter names, should the collection also
   record that resolution rule as provenance, or does the binding stay a
   script-side fact a runtime consumer re-derives?

## Verification

- `cargo test -p cs_content --test accept_f14_d_video_layout` — 4 synthetic
  tests (movie records become rows; a zlib-compressed member is read; a
  member with no movie record names nothing; an absent layout container keeps
  the FMV rows and reports the source as unreadable) plus the retail test,
  run with `--include-ignored` — 5 of 5 passing. The fixtures build their
  ROF container byte by byte, including a deflated member through the
  production reader's own flag.
- `cargo test -p cs_content --test accept_f14_d_video_collection` — the
  original suite updated for the combined source: 5 synthetic + retail, 6 of
  6 passing, retail re-measured at 5 rows / 4 duplicates / 5 unnamed.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings` and `cargo test --workspace
  --locked` pass.
- Evidence: `private/evidence/F14-D.VIDEO-LAYOUT/acceptance.json` generated
  by `evidence_report_f14_d_video_layout` from the recorded suite log,
  production discovery, the production baseline report and a second
  production ROF mount of `crimson.rof`; validated with
  `tools/validate_evidence.py --require-pass`; committed as
  `docs/findings/evidence/F14-D.VIDEO-LAYOUT.json`.

No original bytes, no executable and no screenshot are committed. All fixture
bytes in the acceptance tests are authored for those files.
