# F14-D.VIDEO: the video collection of the retail baseline inventory

Date: 2026-10-12. Task #573, stage `### F14-D` of
`specs/F14-canonical-content-catalog-and-dependency-closure.md`, the video
follow-up of #490 (F14-D.7). Capabilities used: `retail` (read-only,
`$CS_GAME_DIR`) plus ordinary build/test. Test prefix: `accept_f14_d_video_`.
Evidence: `docs/findings/evidence/F14-D.VIDEO.json`.

## Problem

`docs/contracts/IDENTITY-CONTENT.md` requires "sounds/music/dialogue/video" as
catalog collections. The baseline inventory (`cs_content::catalog::baseline`)
held **no** `ContentKind::Video` row at all.

The task's measured starting point: the installation holds **no video
container** — the only video files are ten loose `.mpg` files under
`GOSDATA/ASSETS/GRAPHICS/MPG/`, with no member index that could enumerate
them. So unlike the sound family there is no existing reader that lists them,
and the hard question is where identity comes from: **a file name is not a
cutscene** (task rule; AGENTS.md rule 4).

## What in the installation names a video

A scan of every inventoried file — every decompressed reader-archive member of
all 62 version-1 `.zbd` archives plus every loose file's raw bytes — searched
case-insensitively for `mpg`. The hits that are original content:

| Where | What |
| --- | --- |
| `ZBD/zrdr.zbd` member `fmv.zrd` | The strings `MSopen1.mpg`, `zipper.mpg`, `Chap0.mpg` inside `PLAYAVI` actions |
| `UNINSTAL.EXE` | The uninstall glob `GAME\GOSDATA\ASSETS\GRAPHICS\MPG\CHAP*.MPG` (uninstall bookkeeping, not a play reference) |

Everything else in that **raw** pass (`crimson.icd`, the DLLs, `crimson.exe`)
is binary coincidence: no `mpg` substring of those files is a file name. It
does not follow that `crimson.rof` is coincidence — that container stores its
members zlib-compressed, so a raw scan cannot see their text at all. The
review correction below decompressed them with the production reader, and the
rest of the story is there.

`fmv.zrd` is the only member named `fmv.zrd` across all archives (member
index 11 of `ZBD/zrdr.zbd`, start 439805, length 717) and the production
`decode_zrd` reader (F14-D.8's own `.zrd` decoder) reads it **exactly**, all
717 bytes, with no trailing bytes:

```
[IMAGE_PATH ["video"],
 INTRO [SHOWIMAGE …, WAIT 5.0, PLAYAVI "MSopen1.mpg", WAIT 1.0, FADEOUT …,
        PLAYAVI "zipper.mpg", WAIT 1.0],
 CHAP0 [PLAYAVI "Chap0.mpg"]]
```

The decrypted engine (`$CS_ENGINE_IMAGE`) corroborates every link in that
chain without naming a single file this stage had to take on trust:
`Failed to find FMV definitions (fmv.zrd)`, the source path
`D:\zipper\gamez\zFMV\fmv_script.cpp`, the per-action diagnostic
`Error in parsing fmv actions: file=%s, tag=%s`, the tag table
`SHOWIMAGE/LOADIMAGE/TEXTLIST/BLITIMAGE/WAIT/FADEIN/FADEOUT/PLAYAVI/PLAYMCI/
BLUR/PLAYSOUND/PLAYCD/STOPCD`, the directory literal `Assets\Graphics\MPG`,
and the adjacent literals `INTRO`/`fmv.zrd`/`CHAP0`.

So **3 of the 10 `.mpg` files are named by original bytes the raw scan could
see** — and the raw scan was not the whole installation. See the review
correction below: five of the ten are named, `crimflag.mpg` and `final.mpg`
are spelled literally and `chap1`…`chap5` are constructed by name, all of it
inside the compressed members of `crimson.rof`. The engine image does hold a
separate contiguous `chap1.mpgchap2.mpg…chap6.mpg` string blob whose context
this stage did not establish; it is corroborating, not identity, and no row
was built from it.

## Review correction (2026-10-12): the raw scan was not the whole installation

The review of this task decompressed the container the original scan could not
see into, with the production reader (`cs_formats::rof::{read_tree,
read_member}` over `$CS_GAME_DIR`; nothing extracted is committed):

| Where | What |
| --- | --- |
| `GOSDATA/ASSETS/crimson.rof`, member `ASSETS/LAYOUT.CSV` (846 members, 418 zlib-compressed, 0 read errors) | `MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0` five times — one in a `;Multimedia` block, one as `CM_MOVIE=M,CrimFlag.MPG,…` under `[@CampaignIntro@]`, three more `MOVIE=M,CrimFlag.MPG,…` — and `[@FinalCinema@]` / `CF_MOVIE=M,Final.MPG,0,0,0,250,250,1,0` once |
| same container, member `ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT` (623 bytes) | `int FC`, `callback($$E$$, 2151, FC)`, then `GC.BL = "chap" conv$(FC) ".mpg"` and `GC.YC = "cm_movie"` — the chapter video's name is *constructed*, which is why no raw scan ever spelled it |
| same container, member `ASSETS/SCRIPTS/CTL.SCRIPT` (27,465 bytes) | the movie control builds `CL = "assets\\graphics\\mpg\\" BL` and then `loop CL` / `getstatus(CL)` — the original resolves the directory itself |
| `GOSDATA/ASSETS/crimptch.rof` (1 member) | no `mpg` |

Every consequence is recorded here rather than acted on inside this stage:

* The sentence this note carried until review — "the other seven … are named
  **nowhere**: no archive member, no script line, no configuration entry of
  this installation spells them" — is **wrong**. `crimflag.mpg` and
  `final.mpg` are spelled by configuration records, and `chap1`…`chap5` are
  built by a script line. Five of the ten `.mpg` files are named by original
  bytes, not three.
* This stage still reads exactly one source (the `fmv.zrd` member of
  `ZBD/zrdr.zbd`), so it still mints three rows and still counts seven under
  `unnamed_video_file`. That code is defined as "no `PLAYAVI` name reaches",
  which stays true; what was false was this note's reading of it as "no
  original byte names them".
* No row was minted from the new names here. Adding a second container and a
  second record format is a second bounded slice, and the decisions it carries
  (what `CollectionStatus.source` says when two containers contribute, which
  provenance claim a layout record earns) belong to the stage that implements
  it.

Resolving tasks, both filed 2026-10-12, both gating any completeness or
fidelity claim for this collection:

* **Rally #1351 (F14-D.VIDEO-LAYOUT)** — read the `ASSETS/LAYOUT.CSV` `MOVIE`
  records as a second source. Expected retail re-measurement: rows 3 → 5 and
  `unnamed_video_file` 7 → 5 (to be re-measured, not trusted).
* **Rally #1352 (F14-D.VIDEO-CHAPTER)** — bind `chap<N>.mpg` to a chapter from
  the script bytes and whatever `FC` (engine callback 2151) holds, or record
  the binding as an unknown. The installation holds no `chap6.mpg`, although
  the engine image's table lists it.

## Update (2026-10-12, Rally #1351): the layout records landed as the second source

Rally #1351 (F14-D.VIDEO-LAYOUT) landed after this note's review and did
exactly the bounded slice the correction reserved: `video_rows` now reads
`ASSETS/LAYOUT.CSV` through the production ROF mount and the keyed-list
configuration reader, and a record becomes a video reference only when the
measured schema classifies it as a movie record **and** its file-name field
states an `.mpg` name. Re-measured on the owner's installation, the expected
numbers held: **5 rows** (`chap0`, `msopen1`, `zipper` under
`f14.d.video.baseline.playavi`; `crimflag`, `final` under
`f14.d.video.baseline.layout`, with edge provenance spans at the records' own
lines inside the decoded member), `unnamed_video_file` **5** (the
`chap1`…`chap5` files `CAMPAIGNINTRO.SCRIPT` constructs, still Rally #1352's)
and `duplicate_video_name` **4** (the four `CrimFlag.MPG` repeats).
`CollectionStatus.source` names both containers
(`ZBD/zrdr.zbd + GOSDATA/ASSETS/crimson.rof`), a source that cannot be read is
counted under the new `unreadable_video_source` code and named in the
diagnostic, and a movie-kind record whose name field is not an `.mpg` name is
counted under `movie_record_without_mpg_name` instead of being read as a
reference. Full record: `docs/findings/2026-10-12-f14-d-video-layout-collection.md`
and `docs/findings/evidence/F14-D.VIDEO-LAYOUT.json`.

The sections below are left as the record of the stage as it landed and was
reviewed; where they disagree with this update — "still reads exactly one
source", "still mints three rows", "still counts seven", the retail result of
3 rows and `unnamed_video_file: 7` — **this update wins**.

## What the collection now holds

`video_rows` in `crates/cs_content/src/catalog/baseline.rs`:

- The names come from `PLAYAVI` actions of the `fmv.zrd` member of
  `ZBD/zrdr.zbd`, located by the production container discovery and decoded by
  the production `.zrd` reader. Nothing is derived from a file name: the walk
  over the decoded value tree recognizes an action by its tag, never by an
  argument's extension.
- Each name resolves against the **inventory** by its file-name component,
  case-folded the way the id grammar folds case. Exactly one match is one
  `ContentKind::Video` row; the identity is the resolved file's own
  `install_file_key`, the span is the file's own bytes (offset 0, stored size,
  the file's own SHA-256) and the single static edge points at that file's
  `install_file` row under claim `f14.d.video.baseline.playavi`. The record's
  own spelling (`Chap0.mpg`, `MSopen1.mpg`) stays outside identity as the
  display name.
- Nothing decodes the MPEG stream. Every row is `Unparsed`, carries the
  explicit unknown `f14.d.video.playback` (no codec, duration, resolution or
  trigger has been read; no media player consumer is claimed — F40 measures
  those) and `MissingRuntimeConsumer`.
- Every failure stays a named gap under one of five always-reported codes:
  `playavi_name_not_text`, `named_file_absent`, `ambiguous_file_name`,
  `duplicate_video_name`, `unnamed_video_file`. The last one counts the
  inventoried `.mpg` files no name reaches, so the collection is visible over
  **every** video-looking file the installation holds rather than only over
  the three the bytes name.
- A video is not launchable: no root is added and the coverage denominator is
  unchanged (53 roots on this installation, measured before and after).

Retail result on the owner's installation
(`$CS_GAME_DIR`, installation fingerprint in the evidence report): **3 rows**
— `Chap0.mpg` (18 696 196 bytes), `MSopen1.mpg` (2 664 262 bytes) and
`zipper.mpg` (2 535 428 bytes), all under `GOSDATA/ASSETS/GRAPHICS/MPG/` —
and `unnamed_video_file: 7`; every other gap code is zero.

## What is still unknown (and why no row claims it)

- **Which mission or scene each video belongs to.** `fmv.zrd` names sequences
  (`INTRO`, `CHAP0`) but nothing in the installation maps a sequence to a
  mission directory or a menu event. The sequence label is not identity: two
  `PLAYAVI` actions in one sequence are two videos, and one file played from
  two sequences is still one file. A future F40 stage that reads a trigger
  binding from original bytes should re-derive this collection.
- **Codec, duration, resolution, playback path.** No media player consumer
  exists in the tree; reading the MPEG stream would produce numbers with no
  consumer and no unit contract. Recorded as the `f14.d.video.playback`
  unknown on every row.
- **The seven `.mpg` files no `PLAYAVI` action reaches.** The review
  correction above found where five of them are named: `crimflag.mpg` and
  `final.mpg` are spelled by `ASSETS/LAYOUT.CSV` records, and `chap1`…`chap5`
  are *constructed* by `ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT`
  (`"chap" conv$(FC) ".mpg"`, resolved under `assets\graphics\mpg\` by
  `CTL.SCRIPT`), with the engine image holding a contiguous
  `chap1.mpgchap2.mpg…chap6.mpg` table and the uninstaller listing
  `CHAP0.MPG`…`CHAP5.MPG`. What is still unknown is the binding: what `FC`
  (engine callback 2151) can take, which chapter number belongs to which file,
  and why `chap6.mpg` is listed but absent from the installation. So all seven
  stay named gaps here and no row is minted. Rally #1351 owns the two spelled
  names, Rally #1352 the chapter binding; if neither can be bound from bytes,
  an owner-supplied original-run capture — not the file name — is what can
  promote them.

## Open questions for the owner

1. ~~Is there an original run in which the engine names the chapter videos
   (a debug log, a mission script outside this installation's archives)?~~
   **Answered by the review correction:** the naming script *is* in this
   installation (`ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT` builds
   `"chap" conv$(FC) ".mpg"`). What is still open, and what Rally #1352
   carries, is the value of `FC` and the chapter-to-file binding — and why the
   engine image lists `chap6.mpg` when the installation holds no such file.
2. Should F40's cutscene rows reuse these `video` rows as their media
   dependency, or is a cutscene (a scripted sequence) a separate content kind
   that *references* a video? The collection is built so either consumer can
   attach: the rows are keyed by the media file itself.

## Verification

- `cargo test -p cs_content --test accept_f14_d_video_collection` — 5
  synthetic tests, all passing, each calling the production
  `retail_baseline` over a fixture installation whose `fmv.zrd` is built byte
  by byte; plus the retail test run with `--include-ignored`. Removing the
  `video_rows` wiring from `retail_baseline` fails
  `accept_f14_d_video_named_file_is_one_row` and
  `accept_f14_d_video_unresolved_names_are_gaps` (mutation-checked during
  implementation).
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings` and `cargo test --workspace
  --locked` pass.
- **Review verification (2026-10-12).** All four checks re-run on the reviewed
  tree, plus `cargo test --workspace --locked -- accept_f14_d_video_
  --include-ignored` discovering and passing 6 of 6 (5 synthetic + retail).
  The `video_rows` wiring was removed from `retail_baseline` again and
  `accept_f14_d_video_named_file_is_one_row` plus
  `accept_f14_d_video_unresolved_names_are_gaps` failed, so the tests really do
  exercise the production path. An independent case-insensitive scan of all
  804 MB of the installation found `.mpg` in exactly `ZBD/zrdr.zbd` (the three
  `PLAYAVI` names, counted one by one) and `UNINSTAL.EXE`; the compressed
  members both ROF containers hold were then read with the production reader,
  which produced the review correction above. The seven `python3 -m unittest
  tools.tests.test_evidence_review_identity` failures were reproduced on
  `origin/main` in a clean worktree and are pre-existing, not caused here.

No original bytes, no executable and no screenshot are committed. All fixture
bytes in the acceptance test are authored for that file.
