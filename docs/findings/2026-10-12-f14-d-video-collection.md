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

Everything else (`crimson.rof`, `crimson.icd`, the DLLs) is binary coincidence:
no `mpg` substring of those files is a file name.

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

So **3 of the 10 `.mpg` files are named by original bytes**. The other seven
(`chap1`…`chap5`, `crimflag`, `final`) are named **nowhere**: no archive
member, no script line, no configuration entry of this installation spells
them. The engine image does hold a separate contiguous
`chap1.mpgchap2.mpg…chap6.mpg` string blob whose context this stage did not
establish; it is a lead, not identity, and no row was built from it (see
"Open questions").

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
- **The seven unnamed `.mpg` files.** `chap1`…`chap5` sit next to `chap0`,
  which the `CHAP0` sequence plays; the engine image holds a
  `chap1.mpgchap2.mpgchap2…chap6.mpg` contiguous blob. That is a credible
  lead that chapter cutscenes are named somewhere this installation does not
  store (a script compiled into the executable, or per-mission data this
  reimplementation has not located), but it is **not** evidence of which file
  plays when, so all seven stay named gaps and no row is minted. If the owner
  runs the original and a capture shows a chapter transition playing
  `chap1.mpg`, that capture — not the file name — is what can promote it.

## Open questions for the owner

1. Is there an original run in which the engine names the chapter videos (a
   debug log, a mission script outside this installation's archives)? That
   would settle the seven unnamed files.
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

No original bytes, no executable and no screenshot are committed. All fixture
bytes in the acceptance test are authored for that file.
