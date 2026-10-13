# F40-D2: the retail cinematic trigger inventory

Date: 2026-10-13. Task #661, a slice of stage `### F40-D` of
`specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`. Capabilities
used: `retail` (read-only, `$CS_GAME_DIR`) plus ordinary build/test. Test
prefix: `accept_f40_d2_`. Evidence: `docs/findings/evidence/F40-D2.json`.

## Problem

F40-A defined the cinematic schema (`cs_content::cinematics`) with every
load-bearing value an explicit `Resolved`, and F14-D's video collection
inventoried the ten retail `.mpg` files and measured *that* two naming
sources name them — but not **when each one plays, whether it can be
skipped, what runs at its end, or what its pause policy is**. This stage
measures exactly that, from original bytes, and records what stays unread.

The measurement is production code:
`cs_content::cinematics::measure_retail_cinematics` reads the decoded
original sources and fills every `Resolved` of the retail records **only**
where those bytes state the value. A value no named source states stays an
explicit `Resolved::Unknown` carrying its claim id and reason (AGENTS.md
rule 4). The acceptance test drives it with production-read retail bytes;
the synthetic tests drive it with authored fixtures that pin each measured
decision and each measured absence.

## What the original bytes say, per video

All ten files ship under `GOSDATA/ASSETS/GRAPHICS/MPG/`. The sources: the
`fmv.zrd` member of `ZBD/zrdr.zbd` (decoded exactly, 717 bytes), and six
members of `GOSDATA/ASSETS/crimson.rof` — `ASSETS/LAYOUT.CSV`,
`ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT`,
`ASSETS/SCRIPTS/FINALCINEMA.SCRIPT`,
`ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT`, `ASSETS/SCRIPTS/CTL.SCRIPT` and the
four backdrop pages `MAINMENU`/`SAVE`/`LOAD`/`PREFERENCES.SCRIPT`.

| Video | Measured trigger | Skippable | Semantic end |
| --- | --- | --- | --- |
| `MSopen1.mpg` | FMV sequence `INTRO` (`PLAYAVI` after the splash+copyright `WAIT 5.0`) | **unknown** (engine player) | **unread** (engine) |
| `zipper.mpg` | FMV sequence `INTRO` (second `PLAYAVI`, after `FADEOUT`) | **unknown** (engine player) | **unread** (engine) |
| `Chap0.mpg` | FMV sequence `CHAP0` (single `PLAYAVI`) | **unknown** (engine player) | **unread** (engine) |
| `chap1.mpg`…`chap5.mpg` | chapter construction: cabin → intro page → control | **yes** (`key_escape`/`key_space`/`key_return`/left-click → the intro page's own `11006` done-handler) | `script_continue @passengercabin@` — the paused cabin page resumes |
| `CrimFlag.MPG` | five layout records: the `movie` key behind main-menu, save, load and preferences, plus the `cm_movie` record under `[@CampaignIntro@]` whose file the intro page **supersedes** with the constructed chapter name | **unknown** (menu backdrop; the pages state no end and no skip) | loops forever (`# Loops` 0) |
| `Final.MPG` | the `cf_movie` record under `[@FinalCinema@]`, played by `FINALCINEMA.SCRIPT` when native callback 3104 is truthy | **yes** (`key_escape`/left-click → the cinema page's own `11006` done-handler) | `script_run "scrapbook.script"` — the scrapbook page runs |

Every scene's **pause policy** and **master clock** are explicit unknowns
(claims `f40.d2.retail.pause.unmeasured`, `f40.d2.retail.clock.unmeasured`).
No named source states an objective-event or aircraft-control action for any
of the ten (claim `f40.d2.retail.actions.none`): the semantic transitions
the sources state are page-level, recorded as the semantic end, and the
records' `actions` are measured empty. The **format label** is the measured
`.mpg` extension the original's own records spell (claim
`f40.d2.retail.format.mpg`) — a label, not a codec identity.

## The evidence, member by member

### `fmv.zrd` — the engine FMV sequences (`ZBD/zrdr.zbd`, member index 11)

The decoded document is exactly `[IMAGE_PATH ["video"], INTRO […], CHAP0 […]]`
(717 bytes, no trailing bytes; the F14-D finding measured the same member).
`INTRO` holds `SHOWIMAGE MM_splashbackground`, a `TEXTLIST` of the two
copyright notices at positions (400,550)/(400,565) in font `CopyrightNotice`,
`WAIT 5.0`, `PLAYAVI "MSopen1.mpg"`, `WAIT 1.0`, `FADEOUT`, `PLAYAVI
"zipper.mpg"`, `WAIT 1.0`; `CHAP0` holds exactly one action, `PLAYAVI
"Chap0.mpg"`. The measurement reads the (sequence, video) pairs directly and
records no skip, pause or end for any of them: the FMV player is engine-side.

The decrypted engine (`$CS_ENGINE_IMAGE`, read-only, never committed)
corroborates without being taken on trust: the strings
`Failed to find FMV definitions (fmv.zrd)`, `D:\zipper\gamez\zFMV\
fmv_script.cpp`, the action-tag table, and the path literal
`Assets\Graphics\MPG`. At startup one function (its immediate references at
file offsets `0x4a7cfe`…`0x4a7d59`) pushes the MPG directory literal and both
sequence names with `fmv.zrd` — the engine registers `INTRO` and `CHAP0`
against the definitions at boot. **Who invokes each sequence was not read**:
this stage did not follow the call sites, and no script of the installation
names the sequences. That is the `f40.d2.retail.skippable.engine_fmv`
unknown, and why `msopen1`, `zipper` and `chap0` carry
`RetailSemanticEnd::EngineUnread`.

### The chapter construction — `chap1`…`chap5`

Three members form the measured chain (each fragment's offset and occurrence
count is in the acceptance artifact):

1. `PASSENGERCABIN.SCRIPT` — when the cabin page is created with
   `@globals@XQ` (the symbol map names it `fflag`) set, it calls native
   callback **2151** on `$$E$$` into `FC`, and only `if (FC != 0)` runs the
   intro member (`script_run "campaignintro.script", 0x1999`) and pauses the
   cabin page (`script_pause @passengercabin@, 2`).
2. `CAMPAIGNINTRO.SCRIPT` — builds `GC.BL = "chap" conv$(FC) ".mpg"` and
   binds the control to the `cm_movie` layout key. The skip inputs are a
   `switch (getchar())` case list `key_escape`/`key_space`/`key_return` and a
   `lbutton_update` left-click handler; both mail `11006` to the page. The
   `11006` handler (guarded by an `EC` flag, so exactly once) does
   `script_continue @passengercabin@` then `script_end @campaignintro@`.
3. `CTL.SCRIPT` — the `AL` control class is the movie player (the symbol map
   names it `s_movieplayer`, its file field `szmovie`, its handle `avimovie`):
   `movie CL`, `CL = "assets\\graphics\\mpg\\" BL` (the record's file name is
   the default when the page sets no `BL`), `scale (CL) = GK, HK`, and a
   `gui_execute` poll `getstatus(CL)` whose stopped path decrements the
   record's `# Loops` and, when it runs out, `framerate = 0; mail (11006,
   parent)`.

So **skip and natural completion converge on the same handler** in the
original's own design: a skip reaches the same semantic end as the movie's
end, exactly once (the `EC` flag) — the measured original behavior behind
F40's non-negotiable behavior 2. Skippability of the five chapter videos is
therefore `Known(true)` under claim
`f40.d2.retail.skippable.campaignintro`, and their semantic end is
`UiScriptResume { page: "passengercabin" }`.

What stays unread, unchanged from the F14-D chapter-binding finding: the
**value domain of callback 2151** (whose symbol-map name is `nmissid`), and
so which chapter number each `chap<N>.mpg` belongs to. The construction could
name `chap0.mpg` if `FC` were `0`, but the cabin gate `if (FC != 0)` skips
exactly that value — a measured constraint on the domain, not a binding.
`chap0.mpg` is instead named by the `CHAP0` FMV sequence. The engine's
`chap1.mpg…chap6.mpg` string blob and the absent `chap6.mpg` are recorded in
the F14-D finding and unchanged here.

### `CrimFlag.MPG` — the menu backdrop

`ASSETS/LAYOUT.CSV` holds five `M` records naming `CrimFlag.MPG`, each
measured field by field (`MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0`: X/Y/Z 0,
scale 250/250, `# Loops` 0, `Region?` 0; the `CM_MOVIE` record has `# Loops`
1):

- `[@MainMenu@]`, `[@Save@]`, `[@Load@]`, `[@Preferences@]` — the pages bind
  the `movie` key to the `@ctl@AL` control without setting `BL`, so the
  record's own file name plays. With `# Loops` 0 the control takes its loop
  path (`mail (11002) → loop CL`) and never reports stopped: the backdrop
  **loops forever** until the page is replaced. No page states a skip or an
  end for it — that is the measured absence behind the
  `f40.d2.retail.skippable.menu_backdrop` unknown, not a guess.
- `[@CampaignIntro@]` — the `CM_MOVIE` record also spells `CrimFlag.MPG`,
  but the intro page sets `GC.BL` to the constructed chapter name **before**
  `initialize(GC)`, and `CTL.SCRIPT` only defaults `BL` from the record when
  it is empty. The record's file is therefore **superseded** on that page —
  the measured reason the layout source alone overstates where CrimFlag
  plays.

The load and preferences pages create their movie control inside an
`if ($$KW$$)` block — `$$freturntogame$$` in the retail symbol map — so the
backdrop there only plays when returning to the game; the main-menu and save
pages are unguarded (save's own `if ($$KW$$)` is elsewhere in the script and
does not wrap the creation — measured by brace-scoped block search). The
guard is recorded per trigger as `freturntogame_guarded`.

### `Final.MPG` — the ending cinema

`FINALCINEMA.SCRIPT` gates the whole scene on `if (!callback($$E$$, 3104))`:
when the callback is falsy the page runs the scrapbook and ends immediately,
so the cinema plays only when callback **3104** is truthy. Its meaning is
engine state, unread — the same class of unknown as 2151, recorded as such.
When the cinema plays, `GC.YC = "cf_movie"` binds the `[@FinalCinema@]`
record (which spells `Final.MPG`, `# Loops` 1 — play once), and the skip
inputs `key_escape == getchar()` and a left-click mail `11006` to the page.
The `11006` handler (`EC`-guarded, exactly once) runs
`script_run "scrapbook.script", 0x1000`, mails `11005` to `@globals@RR`
(`oambient` — the ambient-sound page) and `script_end @finalcinema@`. Skip
and natural completion again converge on that one handler, so final's
skippability is `Known(true)` under claim
`f40.d2.retail.skippable.finalcinema` and its semantic end is
`UiScriptRun { script: "scrapbook.script" }`.

### What no mission says

A strings scan over every mission-scoped archive (`ZBD/<group>/<mission>/`
`interp.zbd`/`zrdr.zbd`, 53 directories) found no `.mpg`, no `PLAYAVI`, no
`movie`/`cinema` reference in any mission script — the one hit,
`snd_Movie_2Fail`/`snd_Movie_1Suc` in `ZBD/C2/M02/zrdr.zbd`, is a sound-cue
name. All ten videos are shell/menu/campaign-frame content; no mission plays
one.

## What is still unknown (and why nothing claims it)

- **The invocation of `INTRO`/`CHAP0`** and the skippability of their three
  videos — engine-side; the registration site was located, the invocation
  sites were not read. Resolving task: F40-D4's executable analysis or an
  original-run capture.
- **Callback 2151's domain** (chapter binding, unchanged from F14-D) and
  **callback 3104's domain** (when the final cinema is gated off).
- **Pause policy and master clock** of every scene — no byte states them.
- **Codec, duration, frame size** — the streams are not decoded; F40-B owns
  the decoder question, and the schema's `duration_ticks`/`frame_size`
  stay unknown rather than filled from an uncontracted parse.
- **The teardown path**: the pages send no `11000` stop to the movie control
  when they skip; how the engine stops a handle whose page ended
  (`script_end`) is engine-side, unread.

These limitations are repeated in the machine-readable evidence
(`docs/findings/evidence/F40-D2.json`, `review.method`), each naming the
affected content and what would resolve it.

## Verification

- `cargo test --workspace --locked -- accept_f40_d2_ --include-ignored` — 8
  tests (7 synthetic + retail), all passing. The synthetic fixtures author
  every source byte-by-byte (including the `.zrd` grammar's `count + 1` quirk
  and CRLF script text); removing the skip-fragment measurement fails
  `accept_f40_d2_missing_skip_input_leaves_skippable_unknown_but_the_end_measured`
  (mutation-checked during implementation), and a fixture missing one chain
  link produces no chapter records at all.
- The retail test re-derives everything from the installation through the
  production readers on each run — no trusted constants for the measured
  values — and validates every recorded fragment offset against the member
  bytes it names.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings` and `cargo test --workspace
  --locked` pass.

No original bytes, no executable and no screenshot are committed. All
fixture bytes in the acceptance tests are authored for that file.
