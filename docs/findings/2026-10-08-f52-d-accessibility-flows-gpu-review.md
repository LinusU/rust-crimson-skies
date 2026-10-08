# F52-D: the accessibility flows reviewed on a real GPU

Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
stage `### F52-D`. Contract: `docs/contracts/CLI-EVIDENCE.md`.
Code: `crates/cs_app/src/accessibility/gpu_capture.rs`.
Tests: `crates/cs_app/tests/accessibility/{gpu_capture,record}.rs` (prefix
`accept_f52_d_`, 8 tests) plus the evidence harness
`crates/cs_app/tests/accessibility/evidence.rs`.
Evidence: `private/evidence/F52-D/acceptance.json`, committed as
`docs/findings/evidence/F52-D.json`.

## The review itself

The stage is a review, so it started from the state of the feature on `main`:

- All 33 existing `accept_f52_{a,b,c}_` tests pass unchanged on this branch,
  and nothing in F52-A/B/C was edited to make them pass: this stage changed no
  specification, no assertion and no original-mission code path (the F52 owner
  paths are the accessibility module, `cs_content::settings`, this task's tests
  and `docs/findings/`).
- **No regression was found in the flows F52 already owns**, so none was
  repaired. What the review found instead are the four gaps below, which are
  missing wiring rather than broken behaviour; three are already filed and one
  is filed by this task.

## What this stage added

1. **The `gpu` half of the objectives page** — F52-B's finding left "nothing
   here draws" open and named F52-D as its resolution.
   `accessibility::gpu_capture::objective_page_boxes` maps the production
   `ObjectivePage` to the quads a capture frame shows (pure, so the geometry is
   testable without an adapter), and `capture_objective_page` draws that frame
   offscreen through the production F51-D path
   (`cs_app::text::capture_text_boxes`), which refuses a frame that drew
   nothing and leaves no PNG behind. Three frames were written on this
   machine's **Apple M3 Pro / Metal** adapter (artifacts in
   `private/evidence/F52-D/`):

   | capture | what it shows |
   | --- | --- |
   | `f52-d-scale-100.png` | the six rows at the designed 100 % scale, colour roles intact |
   | `f52-d-scale-300.png` | the same rows at the largest UI scale: 3× glyph, 3× band, 3× row pitch, none dropped |
   | `f52-d-monochrome-100.png` | the same geometry with every colour role gone |

   The three digests differ, so no capture is a static fixture. Sampled from
   the 100 % frame (RGB, measured from the written PNG): the cue column at
   x = 16 holds `158,163,168` (neutral), `242,191,64` (attention),
   `89,204,102` (success), `229,77,77` (failure), `158,163,168` (neutral),
   `102,107,115` (muted); the row band is `41,48,61` and the untouched frame
   is `11,14,19`. Under `monochrome` every cue box is the neutral grey while
   every centre and size is byte-identical to the colour run —
   non-negotiable behaviour 2 at the pixel level: colour never carries the
   row.

2. **AC04 end to end through the live session.** A gameplay assist is enabled
   with the production `SettingsSession::apply`, and the metadata comparison
   and replay records carry — `[("fidelity", "modified"), ("assists",
   "mouse-flight,fov-100")]` — is asserted from the session, from
   `SettingsSession::project` (whose frame's gameplay value and label come
   from one reading), from `control_profile()`, from the file on disk and from
   a session reopened on that file. The failure cases are assertions too: an
   assist configured under `ProfileKind::OriginalRules` records
   `fidelity=original-rules` and reaches no gameplay input, a refused change
   (`fov_degrees=300`) never reaches the record, presentation settings never
   enter it, and a change whose atomic write cannot land (the target path is a
   directory) stays `is_staged()` while the record still names what was in
   force and `teardown()` reports the failure instead of closing over it.

## Unknowns and limits (nothing guessed)

1. **No comparison/replay writer carries the label yet.** The record *form* is
   asserted (`AuthoredChoice` under `ChoiceSlot::Assists`, whose value is the
   label's own assist string, distinguishing a modified run from an
   original-rules one), but nothing in `crates/cs_app/src/capture/` — outside
   this task's owner paths — puts it into a `ReplayRecord` or a
   `CaptureRecord`. Affects: AC04's "records it" until the writer exists.
   Resolving: **F52-W3 (#782)**, which this task does not touch.
2. **Nothing applies the presentation settings.** The colour filter, UI scale,
   subtitles and per-bus levels still reach no drawn or audible frame: the
   capture above draws the page's *measured boxes*, not the HUD, and no
   camera-shake or screen-flash producer exists anywhere in the workspace.
   Resolving: **F52-W2 (#781)**.
3. **Nothing opens the session at boot.** No command line can request the
   safe-defaults startup and no run holds a `SettingsSession`. Resolving:
   **F52-W1 (#780)**.
4. **The capture is a geometry witness, not a rendering of the page.** No
   glyph is drawn (the `objective.*` keys still have no catalogue entry,
   F51-B), no cue *shape* is drawn — a sprite is a rectangle, so the cue is
   its measured box — and the row band's span to the frame's edge is a frame
   choice, because `ObjectivePage` records no row width and no text extent.
   Row geometry itself is designed (F52-B limit 1), and none of this is the
   original's appearance. Affects: any claim that the objectives page *looks*
   right. Resolving: F52-W2 with F46/F51, and owner review.
5. **The original option set is still unmeasured** (F52-A limit 1). This task
   added one bounded measurement: a `find "$CS_GAME_DIR" \( -iname '*.ini' -o
   -iname '*.cfg' -o -iname '*.opt' \)` over the whole installation returned
   **no matches**, so the 2000 PC game stores none of its options in the
   installation directory — where it does store them (the registry? a file
   outside the install?) is *not* measured here and cannot be measured from
   these bytes. Affects: every "original option" claim, `ModernAssist::Fov`
   treating any explicit FOV as an assist, F21's camera. Resolving: filed with
   this task as **#789 `F52-ORIGINAL-OPTIONS`** (owner's original run/registry
   capture or a separate measurement task); F21-B/D remains co-resolving for
   the FOV half.
6. **Capabilities.** The stage declares `gpu` and `synthetic` and no `retail`:
   no acceptance test here reads `$CS_GAME_DIR`, so `source` is `null` in the
   report and claiming an installation hash would be a claim of evidence never
   taken. `gpu` here means this engine's offscreen renderer drew the frames on
   a real adapter; it is **not** `human_play`, `human_review` or a run of the
   original executable, and no agent session runs the original.

## Commands

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f52_d_ --include-ignored` | 0 (8 selected, 8 passed, 0 ignored) |
| `CS_EVIDENCE_DIR=… cargo test --locked -p cs_app --test accessibility -- evidence_report_f52_d --ignored` | 0 |
| `python3 tools/validate_evidence.py private/evidence/F52-D/acceptance.json --artifact-root private/evidence/F52-D --require-pass` | 0 (`structurally_valid: true`) |

The claim recorded is `implemented`: a Rally merge awards `checked`, and
neither an agent review nor a green CI run is original-reference evidence.
