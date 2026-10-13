# F14-D.VIDEO-CHAPTER: the chapter videos' names are constructed, and the binding is unread

Date: 2026-10-13. Task #1352, stage `### F14-D` of
`specs/F14-canonical-content-catalog-and-dependency-closure.md`, the
follow-up Rally filed after `### F14-D.VIDEO` (#573) and
`### F14-D.VIDEO-LAYOUT` (#1351) left five inventoried `.mpg` files under
`unnamed_video_file`. Capabilities used: `retail` (read-only, `$CS_GAME_DIR`)
plus ordinary build/test. Test prefix: `accept_f14_d_video_chapter_`.
Evidence: `docs/findings/evidence/F14-D.VIDEO-CHAPTER.json`.

## Problem

The owner's installation ships ten `.mpg` files under
`GOSDATA/ASSETS/GRAPHICS/MPG/`. The two naming sources the earlier stages
read — `PLAYAVI` actions of `ZBD/zrdr.zbd`'s `fmv.zrd` member and the `M`
(movie) records of `GOSDATA/ASSETS/crimson.rof`'s `ASSETS/LAYOUT.CSV` —
resolve five of them into rows. The five that remain are
`chap1.mpg`…`chap5.mpg` (and `chap0.mpg` is already a row through
`PLAYAVI`). The task: bind `chap<N>.mpg` rows from the original script
bytes, or record the binding as unknown. This stage did the second —
measured.

## What the original bytes actually say

`CAMPAIGNINTRO.SCRIPT` does not spell any `chap<N>.mpg` name. It builds one.
All three members live in `GOSDATA/ASSETS/crimson.rof` (the same
version-1 ROF the layout records came from, mounted through the production
`cs_assets::rof::mount_rof_into` reader; compressed members are decoded,
never scanned raw), and the chain is:

1. **`ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT`** (2,640 bytes decoded) — the
   trigger. Its bytes spell:

   ```
   int FC
   callback($$E$$, 2151, FC)
   if (FC != 0)
   {
   script_run "assets\\scripts\\" "campaignintro.script", 0x1999
   ```

   The cabin script fills `FC` by calling native callback id **2151** on
   `$$E$$`, and runs the intro member only when `FC != 0` — so `0` is a
   value `FC` can take, and one the intro is deliberately skipped for.

2. **`ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT`** (623 bytes decoded) — the
   construction. Its `gui_create` block spells `int FC` and the same
   `callback($$E$$, 2151, FC)` site; its `gui_init` block spells:

   ```
   GC.BL = "chap" conv$(FC) ".mpg"
   GC.YC = "cm_movie"
   ```

   `conv$` renders `FC` into the name — the file name is a runtime product
   of the callback's return value.

3. **`ASSETS/SCRIPTS/CTL.SCRIPT`** (27,465 bytes decoded) — where the
   constructed name becomes a played path. `object GC = @ctl@AL` in the
   intro's `gui_create` means `GC.BL`/`GC.YC` are fields of the control
   member's `AL` movie object, and CTL spells `movie CL`,
   `CL = "assets\\graphics\\mpg\\" BL`, `loop CL` and `getstatus(CL)` — the
   declaration, the directory prefix, the playback loop and the status
   poll.

All six measured fragments and both `callback` sites are verified
byte-for-byte on the retail members by the retail test and again by the
evidence harness (which writes each member's decoded SHA-256, site spans
and fragment offsets into `chapter-members.json`). The members scan
cleanly through the production F38-B host-call scanner
(`scan_ui_program`): each `callback` site is a `DispatchForm::Callback`
site stating integer native id `2151`.

## Why the names stay unbound

The chain establishes that the name is *constructed* and *played*. It does
not establish the name: `FC`'s value domain is the return of a native
callback, and no member, record or flag of the installation states it.

- The script's own symbol map (`ASSETS/SCRIPTS/DEBUGINFO.TXT`) names `FC`
  `nmissid` and `$$E$$` `$$uidata$$` — suggestive text, but a map's label
  is still not a value domain, and `nmissid` does not say which mission id
  maps to which `chap<N>.mpg`.
- `ASSETS/LAYOUT.CSV` spells `cm_movie` beside the `[@CampaignIntro@]`
  `CM_MOVIE=M,CrimFlag.MPG,…` record — so the movie object's `YC` field
  points back at a layout key — but no layout record spells a `chap` name.
- The ZBD tree holds chapter directories `C1`…`C5` — the same cardinality
  as `chap1`…`chap5` — but nothing in the scripts or records binds a
  chapter directory to a `chap<N>` number. A count that matches is a
  corroboration, not a binding.
- The owner-supplied decrypted engine (`$CS_ENGINE_IMAGE`, read-only,
  outside the installation) spells `chap1.mpg`…`chap6.mpg` contiguous in
  its string table. Adjacent strings in an executable are the compiler's
  layout, not a stated value domain: they do not say which string the
  callback's return selects, and **`chap6.mpg` ships in no inventoried
  file** — whether the sixth name is an obsolete slot, an optional asset
  or a missing file is not stated by any byte read.
- F38-B's host-call work already records that native callback ids'
  *meanings* are not readable from script bytes; the semantics of 2151
  live in the engine's dispatch, whose resolution through the packed
  executable this stage did not reach.

So the honest deliverable is the task's second option: a machine-readable
record that the construction was measured and the binding was not read.

## What the collection now reports

`video_rows` mounts the shared archive a second time under the
`rof-video-chapter` label and verifies the chain member by member through
the production scanner. Two new gap codes, always reported:

- `chapter_name_unbound` — one inventoried `.mpg` file whose **file name**
  spells the construction's product shape (`chap` + decimal digits +
  `.mpg`, ASCII case-folded) and that no record of either naming source
  reached. The file stays counted under `unnamed_video_file` as well: the
  two codes count different facts about the same file, not two files. No
  row is minted: `conv$` renders digits, and which digits appear is the
  unread `FC` domain.
- `chapter_source_unreadable` — one chain member that cannot be verified
  (container absent or unmountable, member absent or unreadable, program
  that does not scan, a measured `callback` site absent, a measured
  fragment absent), named in `CollectionStatus.diagnostic`. A nonzero
  count means the `chapter_name_unbound` record rests on the file-name
  shape alone.

Retail result, re-measured: **5** under `chapter_name_unbound`
(`chap1.mpg`…`chap5.mpg`), **0** under `chapter_source_unreadable`, video
rows still **5**, `unnamed_video_file` still **5**, every other code
unchanged — the collection gains an explicit record, not new rows.

## What is still unknown

- **`FC`'s domain** — which values callback 2151 returns and which chapter
  number each `chap<N>.mpg` belongs to. That is engine state: resolving it
  needs either an original-run capture of the callback's return for each
  chapter, or a verified executable-analysis stage that reads 2151's
  dispatch in the decrypted image. Neither is an agent capability today;
  agents cannot produce original runs (`human`/`original-run` evidence is
  owner-supplied only). Until then no row may name a `chap<N>.mpg`.
- **`chap6.mpg`** — spelled in the engine's string table, absent from the
  installation. Its status (obsolete, optional, missing) is unverifiable
  from available bytes.
- **Codec, duration, resolution, trigger** — unchanged from #573: no
  original byte read states them; the MPEG streams are not decoded; F40
  owns the media consumer.
- **Which scene plays each video** — carried over; the `[@CampaignIntro@]`
  placement is display context, not identity.

## Open questions for the owner

1. `$$uidata$$` callback 2151 returns `nmissid`. Does the owner have an
   original-run capture path (or engine knowledge) that states its domain
   — e.g., does `FC` take the campaign chapter index 1–5, or a different
   numbering?
2. `chap6.mpg` is referenced by the engine's string table but absent from
   the disc image. Was there a sixth chapter video in any release, or is
   the string vestigial?

## Verification

- `cargo test -p cs_content --test accept_f14_d_video_chapter` — four
  synthetic tests plus the retail test, run with `--include-ignored` —
  5 of 5 passing. The fixtures author the ROF container byte by byte,
  including a nested `ASSETS/SCRIPTS/` member layout: the measured chain
  verifies and the constructed names stay unbound; an absent member is
  counted under `chapter_source_unreadable`; a file merely named
  `chap7.mpg` never becomes a row.
- `cargo test -p cs_content --test accept_f14_d_video_collection` and
  `--test accept_f14_d_video_layout` — the sibling suites updated for the
  two new codes: synthetic and retail all pass with the new zero/nonzero
  counts.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings` and `cargo test --workspace
  --locked` pass.
- Evidence: `private/evidence/F14-D.VIDEO-CHAPTER/acceptance.json`
  generated by `evidence_report_f14_d_video_chapter` from the recorded
  suite log, production discovery, the production baseline report, a
  second production ROF mount plus `chapter-members.json` (per-member
  decoded SHA-256, site spans, fragment offsets); validated with
  `tools/validate_evidence.py --require-pass`; committed as
  `docs/findings/evidence/F14-D.VIDEO-CHAPTER.json`.

No original bytes, no executable and no screenshot are committed. All
fixture bytes in the acceptance tests are authored for those files.
