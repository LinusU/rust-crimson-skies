# F52-D: the accessibility flows reviewed on a real GPU

Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
stage `### F52-D`. Contract: `docs/contracts/CLI-EVIDENCE.md`.
Code: `crates/cs_app/src/accessibility/gpu_capture.rs`.
Tests: `crates/cs_app/tests/accessibility/{gpu_capture,record}.rs` (prefix
`accept_f52_d_`, 10 tests) plus the evidence harness
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

   The three digests differ, so no capture is a static fixture, and the
   capture test **decodes each written PNG and samples the centre of every
   quad** `objective_page_boxes` asked for: the pixel has to be that quad's
   own fill (±1 byte for the GPU's rounding), and what no quad asked for has
   to be one untouched background shared by all three frames. Sampled from
   the 100 % frame (RGB, independently decoded by the reviewer from the
   artifact): the cue column at x = 16 holds `158,163,168` (neutral),
   `242,191,64` (attention), `89,204,102` (success), `229,77,77` (failure),
   `158,163,168` (neutral), `102,107,115` (muted); the row band is `41,48,61`
   and the untouched frame is `11,14,19`. Under `monochrome` every cue box is
   the neutral grey while every centre and size is byte-identical to the
   colour run — the colour-independence half of non-negotiable behaviour 2 at
   the pixel level: colour never moves or resizes a row. The shape and text
   alternatives behaviour 2 also requires are F46/F51's to draw and are not in
   these frames (limit 4).

   The pure geometry is pinned without an adapter as well: the quads' expected
   **horizontal** numbers (cue column, band start, band right edge on the
   frame's edge, the two touching without overlap), the clipping of a
   half-visible row, and — the branch the 1:1 tests never take — that a
   viewport taller than the frame scales **every** quad down by one uniform
   factor, anchored at the frame's top and centred horizontally, so a
   scaled-down band ends short of the frame's edge by that same factor.

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
   its measured box — and the row band's span to the page's right edge is a
   frame choice, because `ObjectivePage` records no row width and no text
   extent (that edge is the frame's edge at 1:1 and uniformly short of it
   when a viewport taller than the frame is scaled down). The frame also
   shows the page from its own top: `ObjectivePage` carries no scroll state,
   so `scroll_to_show` never reaches a capture. Row geometry itself is
   designed (F52-B limit 1), and none of this is the original's appearance.
   Affects: any claim that the objectives page *looks* right. Resolving:
   F52-W2 with F46/F51, and owner review.
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
| `cargo test --workspace --locked -- accept_f52_d_ --include-ignored` | 0 (10 selected, 10 passed, 0 ignored) |
| `cargo test -p cs_app --test accessibility -- <name> --exact --include-ignored`, once per task test | 0 (exactly 1 passed, 43 filtered, each of the 10) |
| `CS_EVIDENCE_DIR=… cargo test --locked -p cs_app --test accessibility -- evidence_report_f52_d --ignored` | 0 |
| `python3 tools/validate_evidence.py private/evidence/F52-D/acceptance.json --artifact-root private/evidence/F52-D --require-pass` | 0 (`structurally_valid: true`) |

## Review

Recorded per `AGENTS.md` ("record the actual implementer/reviewer identities
and whether the reviewer's context was fresh"): implementer **bunny-2**
(`opencode/mimo-v2.6-Flash`, Rally #211 implement claim of 2026-10-08);
reviewer **bunny-2**, a *fresh session* with no memory of the implementation
beyond its own submission summary — same model, so this review is **not**
independent evidence for a fidelity claim, and no agent review replaces the
owner's human approval.

What the review checked and changed (all inside the F52-D owner paths):

- **Test sensitivity, measured.** With the two production behaviours it names
  disabled (`objective_page_boxes` returning no quads, and
  `FidelityLabel::metadata` dropping the `assists` entry), **9 of the 10**
  `accept_f52_d_` tests fail; only the refusal test still passes, which is
  correct — it asserts that nothing to draw is refused. Reverted, 10/10 pass
  again.
- **Every test alone with `--exact`**, as `docs/contracts/CLI-EVIDENCE.md`
  requires: 10 runs, each exactly one passing test (the earlier attempt with
  an unsplit name list ran *zero* tests and exited 0, which is precisely the
  empty-selection pass the contract warns about; the recorded numbers above
  are from the corrected run).
- **The pixels were decoded again, independently**, from the committed
  artifacts with a separate decoder: the numbers in item 1 are what the PNGs
  hold, run by run — and the capture test now samples them itself.
- **Two gaps fixed rather than noted:** the x geometry and the
  does-not-fit-the-frame branch had no test (both now have one), and the
  evidence harness accepted inputs it never bound to the tree — it now
  refuses to write a report unless `git status --porcelain` is empty (the
  contract's "clean checkout") and unless the log's test counts equal this
  task's own assertions (a wider green run cannot stand in for the
  selection).
- **Two overclaims narrowed:** the module doc's "band runs to the edge of the
  frame" and "never a row resized to make it fit" were false whenever the
  viewport does not fit the frame, and "non-negotiable behaviour 2 at the
  pixel level" is only that behaviour's colour-independence half. The docs
  and limit 4 above now say what the code does; no assertion, no
  specification and no mission code was weakened to get there.

The claim recorded is `implemented`: a Rally merge awards `checked`, and
neither an agent review nor a green CI run is original-reference evidence.
