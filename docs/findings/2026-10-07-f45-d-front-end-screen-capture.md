# F45-D: the navigation review and the original screen captures

Date: 2026-10-07. Task: #195 / F45-D "Run end-to-end keyboard/controller
campaign navigation review" (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`,
section `### F45-D`). Shared contracts: `docs/contracts/UI-NETWORK.md` and
`docs/contracts/CLI-EVIDENCE.md`. Required capabilities: `gpu` and `retail`
(both present: `CS_CAPABILITIES=retail,gpu,audio`). Task test prefix:
`accept_f45_d_`. Evidence: `docs/findings/evidence/F45-D.json`, artifacts in
`private/evidence/F45-D/` (git-ignored; screenshots and original pixels never
enter Git).

Implemented by `bunny-2` (agent `bunny-2`, session of 2026-10-07). This file is
the implementer's record; it is not a review and awards no
`verified_original`/`release_approved`. `retail` below means read access to the
owner's installation, never a run of the original executable: no agent ran
`crimson.exe`, and no capture here is a screenshot of the original renderer.

## One observable failure (listed before editing)

F45-A built the table, F45-B presented it through authored artwork and F45-C
wired it to the domain — and **nothing in the repository ever walked the whole
navigation graph or drew a single front-end frame.** Concretely, with this
stage's production code removed, nothing can answer "which screens does a
player actually reach, and by which rows?" and no PNG of a front-end screen can
be produced at all.

The one observable failure this stage fixes:
`accept_f45_d_the_navigation_review_classifies_every_row_and_names_every_screen`
must report a review that classifies every one of the table's 45 rows and every
one of its 16 screens — reached *or* named as not reached, with the refusal
code of any row that stops — and
`accept_f45_d_a_screen_capture_draws_the_artwork_its_hotspots_and_the_focused_button`
must produce a real frame in which the focused button's region and the other
buttons' regions are distinguishable by their tints.

## Files and what changed

Owner paths only; the only other edits are the crate-wiring `Cargo.toml` /
`Cargo.lock` edge AGENTS.md allows.

- `crates/cs_app/src/ui/front_end/capture.rs` (new): the `gpu` half.
  - `Artwork` — decoded RGBA8 pixels with an extent, validated once
    (`EmptyArtwork`, `PixelCount`), plus `is_uniform` for the flat-slab case.
  - `CapturedButton`, `ScreenCapture`, `ScreenCaptureError`.
  - `capture_screen(view, artwork, png)` — the deck path: the `ScreenView`'s
    own aspect-fit is **recomputed and compared** (`SurfaceMismatch`), the
    artwork's extent must equal the fitted image (`SizeMismatch`), then the
    artwork sprite, every hotspot region and the focused region are drawn into
    an offscreen 800x600 frame and read back.
  - `capture_artwork(label, artwork, buttons, png)` — the retail path: original
    artwork with no deck behind it, because no original hotspot layout is
    decoded anywhere here.
  - Refusals: `NoScreenshotCaptured`, `UniformFrame` (every pixel the
    background ⇒ nothing was drawn) and `Io`. Every refusal deletes the PNG the
    renderer already wrote, so a refused capture never leaves a file that reads
    like a good one.
- `crates/cs_app/src/ui/front_end/paths.rs` (new): the navigation review.
  - `NavigationInputs` / `SuppliedInput` — the domain inputs a player supplies
    (the flight check's loadout, the construction screen's open draft), which
    are presentation-level inputs (F45-B `ScreenSession::select_loadout` /
    `open_construction`) and **never** table rows. The review records every one
    it used.
  - `review_navigation` / `review_navigation_with` — a walk that explores every
    distinct machine state (identity = everything the table's guards read:
    screen, last outcome, open loadout, open draft, open prompt, last failure),
    applies every row of every reached screen, keeps the best outcome any state
    produced for that row, and answers the discard prompt inline
    (`StepOutcome::Prompted`) rather than stopping at it.
  - `PathReview` — `steps`, `reached`, `not_reached`, `rows_unreached`,
    `supplied_inputs`, `is_complete()`, and `json()` for the report.
- `crates/cs_app/src/ui/front_end/retail.rs` (new): the `retail` half.
  - `FrontEndScreens::open` — production discovery, the install session, the
    UI texture archive `ZBD/rimage.zbd` through
    `cs_content::textures::TextureCatalog`, and the ROF mount of
    `GOSDATA/ASSETS/crimson.rof` through `cs_assets::rof::mount_rof_into`.
    Read-only; nothing is written to the installation.
  - `inventory()` — **every** image of both sources, nothing skipped: 254
    stored game textures decoded through the production ZBD reader, and every
    `ASSETS/GRAPHICS/**` image member decoded by the workspace's `image`
    decoder, each with measured extent, format, byte count, digest and an
    explicit `decodable`/`refusal`.
  - `artwork()` — one recorded image into the `Artwork` a capture draws; a game
    texture goes through `crate::render::rgb565`'s decided expansion policy
    (F17-B) and the same coverage rule the playtest draws with.
  - `MINIMUM_SCREEN_EXTENT` — the stage's **declared selection rule**: an image
    is screen-capable when it can cover 640x480, the extent of `mainmenu` and
    `escapemenu` in `rimage.zbd`. Designed and stated in the module, never
    attributed to the original; every image is in the inventory regardless.
- `crates/cs_app/src/ui/front_end/mod.rs` — `mod capture; mod paths; mod
  retail;`, the re-exports and the module-doc paragraph (wiring only).
- `crates/cs_app/tests/ui/stage_d.rs` (new): the five `accept_f45_d_*` tests.
- `crates/cs_app/tests/ui/evidence.rs` (new): the evidence harness, deliberately
  **not** named `accept_f45_d_*`.
- `crates/cs_app/tests/ui/main.rs` — the two `mod` lines and the F45-D target
  docs.
- `crates/cs_app/Cargo.toml` + `Cargo.lock` (wiring): `image = { version =
  "0.25.10", default-features = false, features = ["png", "jpeg", "tga", "bmp"] }`
  — the decoder Bevy already compiles into this workspace through `bevy_image`,
  named directly so the front end can read the original PNG/JPEG/TGA/BMP screen
  art. It adds one lock edge (`zune-jpeg`) and no new package family.
- This file.

## What was measured

**The original front-end screen inventory** (`accept_f45_d_retail_…`):

| measurement | value |
| --- | --- |
| `ZBD/rimage.zbd` stored textures, all decoded through the production reader | 254 |
| `GOSDATA/ASSETS/crimson.rof` image members under `ASSETS/GRAPHICS/` | 563 (313 PNG, 143 TGA, 107 JPEG) |
| total inventory rows | 817 |
| rows production code could **not** decode | 0 |
| screen-capable by the declared rule (≥640x480) | 68 (18 in `rimage.zbd`, 50 in `crimson.rof`) |
| `mainmenu`, `escapemenu` | 640x480 (the rule's own basis) |
| front-end backgrounds (`MM_BACKGROUND`, `FC_BACKGROUND`, `PS_BACKGROUND`, `PC_BACKGROUND`, `SB_BACKGROUND`, `IA_BACKGROUND`, …) | 800x600 |
| captures written, each drawn on the real adapter | 68 |
| distinct frames = distinct pictures | 66 = 66 (two pairs of members store the *same* picture) |
| refused as a flat slab | 0 |

The 68 captures are named `f45-d-original-<member>.png` under
`private/evidence/F45-D/`; the inventory is `front-end-screens.json` there.

**The navigation review** (`accept_f45_d_the_navigation_…`):

| measurement | value |
| --- | --- |
| table rows / screens | 45 / 16 |
| without domain inputs | 29 rows applied, 11 screens reached, 5 not reached (`Loading`, `Flight`, `Pause`, `PauseSettings`, `Results`), 16 rows behind them, refusals reported by code (`invalid_loadout` on `FlightCheck`/`Launch`, `no_draft` on `Construction`/`CommitConstruction`) |
| with the player's declared inputs | **45/45 rows applied, 16/16 screens reached, 0 refusals** |
| declared inputs used | `construction_draft` on `Construction`, `loadout` on `FlightCheck` |
| domain requests actually asked | `OpenProfile`, `CloseProfile` (twice), `CommitBlueprint`, `CommitLoadout`, `ApplyOutcome` (success *and* failure), `AbandonMission` |
| discard prompts answered inline | yes (`StepOutcome::Prompted` on the dirty construction/flight-check Back/Cancel) |

## Design decisions a reviewer should check

1. **The frame is 800x600, measured.** That is the extent of the original
   front-end backgrounds in `crimson.rof`; a 640x480 authored screen letterboxes
   through the same `AspectFit` a player's surface would, and the capture
   *refuses* a `ScreenView` fitted against any other surface, so a hotspot
   rectangle can never be drawn against the wrong picture.
2. **The uniform-frame gate is the evidence gate.** A frame whose every pixel
   is the background is deleted, not written — the test drives it with a flat
   800x600 picture and asserts the file does not exist afterwards. The
   renderer *does* write its PNG first (bevy logs it); the refusal removes it.
3. **TGA has no magic bytes.** `image`'s format sniffing cannot identify the
   143 original `.TGA` members; the reader is told the format from the member's
   extension explicitly, instead of 143 originals being reported
   "format could not be determined" (that was the first run's failure and is
   why the refusal text is asserted nowhere as success).
4. **The review explores states, not just screens.** A row refused on the state
   that first reached a screen is retried when a richer state arrives later —
   that is the only way `Results`/`Retry` (`FailedOutcome`) and
   `FlightCheck`/`Launch` (`ValidLoadout`) are ever seen to *succeed* — and the
   step kept per row is the best outcome any state produced. Two states are
   equal exactly when everything the guards read is equal, so it terminates.
5. **Frames and pictures are in bijection.** The retail capture test does not
   merely count distinct PNGs: for every pair it asserts that equal frames imply
   equal decoded pixels and equal pixels imply equal frames, so a capture that
   ignored its artwork, or that was unstable, fails even if the corpus stores
   the same picture twice (it does: two of the 68 members are byte-identical
   pictures).

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f45_d_ --include-ignored`
discovers and runs **5** tests (exit 0): the navigation review (unignored, so
CI runs it), the two synthetic GPU captures, and the two retail tests (one of
them retail+GPU). The full four checks pass locally: `cargo fmt --all
-- --check`, `cargo clippy --workspace --all-targets --all-features --locked --
-D warnings`, `cargo test --workspace --locked` (exit 0, 438 test-result
blocks green) and the selection above.

| test | what it fails without |
| --- | --- |
| `accept_f45_d_the_navigation_review_classifies_every_row_and_names_every_screen` | `review_navigation*` returning a partial walk, or the table losing a row/screen |
| `accept_f45_d_a_screen_capture_draws_the_artwork_its_hotspots_and_the_focused_button` | `capture_screen` not drawing the artwork, a hotspot, or the focus tint |
| `accept_f45_d_a_frame_that_is_not_evidence_of_a_drawn_screen_is_refused` | any refusal removed: a bad surface, wrong artwork, empty pixels or a flat frame would then leave a PNG behind |
| `accept_f45_d_retail_the_original_front_end_screen_inventory_is_complete_and_measured` | the inventory walking fewer sources/rows, or a decode failure being swallowed |
| `accept_f45_d_retail_gpu_every_screen_capable_original_image_draws_a_measured_frame` | fewer than 68 selections, a capture skipped, or the frame/pixel pairing broken |

Sensitivity was checked by mutation and then reverted (each run:
`cargo test -p cs_app --test ui -- <test> [--include-ignored]`):

| mutation | result |
| --- | --- |
| the walk stops supplying the declared domain inputs (`FlightCheck`/`Construction` arms unreachable) | the navigation test fails at `not_reached.is_empty()` (the five screens go back behind `invalid_loadout`) |
| `capture_artwork` draws no hotspot region (`buttons.iter().take(0)`) | the capture test fails: 0 focus pixels and 0 button pixels |
| the screen container's graphics half of the inventory is filtered out | the retail inventory test fails: `left: 254` against the measured 817 |

## What remains unknown (recorded, not guessed)

- **No original screen→artwork binding is decoded anywhere in this
  repository.** The inventory measures *every* image and captures the 68 that
  can fill a screen; which of them the original draws on which front-end screen
  is unread, and nothing in this stage maps an image onto a `Screen`. Resolving
  task: **#742** (`F45-B.1`). Affected content: every "this screen shows that
  artwork" claim, and the F45-B deck's real data.
- **No original hotspot rectangle, no original focus/tab order.** Retail
  captures therefore draw artwork alone; no button region of the original is
  claimed. Resolving task: **#742**. Affected content: pointer hit-testing and
  keyboard/controller focus order fidelity.
- **The captures are this engine's renderer drawing decoded original pixels.**
  The original executable never ran in any agent session, so nothing here
  compares with what the original *presents* (its scaling, letterboxing, tints
  and any compositing the original does beyond a flat image). Resolving task:
  REF-OWNER-FIRST-CAPTURE / **#358**, an owner-supplied original run. Affected
  content: every visual fidelity claim for F45.
- **The pixel decoder for PNG/JPEG/TGA/BMP is the workspace's `image` crate**
  (0.25.10, locked), not a decoder compared against an independent reference the
  way F08-D pinned `unzbd` for ZBD textures. Affected content: exact pixel
  fidelity of the 563 container images; resolving task: a decoder comparison
  stage if the owner wants one (**filed as the follow-up below**).
- **The screen-capable rule is designed.** ≥640x480 is stated in
  `MINIMUM_SCREEN_EXTENT`, not measured from the original's own screen list;
  the 18 `rimage.zbd` selection includes mission maps (`*-m*mmap`, 800x600)
  that are *not* front-end screens, because nothing readable says so. The
  inventory lists every row, so a reader can reselect.
- **Input-device mapping is not exercised here.** The sheet names this stage
  "keyboard/controller", but no device is read: the review walks `Action`s,
  which F45-A/F45-B already pin as the same transition a key press, a click or
  a controller activation applies. Mapping real devices is F22's boundary
  (`accept_f22_d_device_families_and_command_coverage`), and no agent can play.
- **No `human_play`/`human_review` evidence exists.** Those capabilities belong
  to the owner; every claim here is `implemented` at most.

## Follow-up filed

- A decoder-comparison stage for the 563 original `crimson.rof` graphics
  members (the F08-D pattern: pin a reference, compare pixel-for-pixel), so the
  PNG/JPEG/TGA decode used by these captures is measured rather than trusted.
