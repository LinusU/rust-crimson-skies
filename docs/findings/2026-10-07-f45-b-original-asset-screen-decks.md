# F45-B: original-asset menu, cabin and briefing screens

Date: 2026-10-07. Task: #193 / F45-B "Implement original-asset
menu/cabin/briefing screens"
(`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, section
`### F45-B`). Shared contract: `docs/contracts/UI-NETWORK.md`. Required
capabilities: ordinary build/test only; no original file enters the product and
no evidence report is claimed.

Implemented by `bunny-alpha-2/bunny-alpha-2` (agent `bunny-alpha-2`). This file
is the implementer's record; it is not a review and awards no
`verified_original`/`release_approved`.

## One observable failure (listed before editing)

F45-A built the state table and its machine and, separately, the authored-screen
layout with aspect-fit, but nothing joins them: a screen was never *presented*
from authored artwork, so "every visible button has a functioning state
transition" was only checked against a layout a test constructed by hand. With
this stage's deck, pressing the flight check's authored **Cancel** button after
selecting a loadout must raise the discard prompt and, once confirmed, leave to
the briefing with **no** `Effect::Request` — while a deck built without
validating the hotspots against the state table would happily carry a hotspot
whose action has no row there (a dead visible button), and a deck built
without the coverage rule would hide a designed button (say, the cabin's
construction door) from navigation entirely.
`accept_f45_b_cancel_at_every_preflight_screen_leaves_state_and_currency_unchanged`
is the test that fails: its Cancel paths must each land on the expected screen
with an empty request list, and its campaign snapshot must not move.

## Files and what changed

Owner paths only; no protected path, no wiring edit outside them.

- `crates/cs_app/src/ui/front_end/screens.rs` (new, owner path): the
  original-asset layer.
  - `Button` — one authored button: `ui-resource` id, front-end `Action`,
    logical-image `Rect`.
  - `ScreenAssets` — a screen's artwork id plus its hotspots, parsed **once**
    into buttons in declaration order (the order keyboard/controller focus
    visits them, per `cs_content::ui_layout`). `new` refuses an artwork id that
    is not an `image` (`ArtNotImage`) and any hotspot that names an unknown key
    or an application-side result (`LayoutProblem::UnknownAction` /
    `NotAButton` — the same three rules `check_layout` states, checked where
    the hotspots are parsed).
  - `ScreenDeck` — the screens the application carries, validated against the
    state table: every button must have a transition on its screen
    (`LayoutProblem::NoTransition`), the art must offer an escape
    (`NoEscapeButton`), every designed *user* row of the screen must have a
    button (`HiddenAction`), and no screen may appear twice
    (`DuplicateScreen`). After validation the deck's button set is exactly the
    screen's visible-button set; only the order may differ from the table's.
  - `ScreenSession` — the presentation: `view` (artwork + buttons mapped with
    the image's own fit), `click` (pointer through the same transform; a later
    button wins an overlap like `ScreenLayout::hit_test`; a point on no button
    returns `NoButtonAt` and changes nothing), `press`/`activate` (keyboard and
    system results take the machine's own table path), `move_focus` over the
    authored order with wrap, `confirm_discard`/`keep_editing`, and the
    screen's domain-side inputs (`open_construction`, `edit_construction`,
    `select_loadout`, `set_wingmate_required`) passed straight to the machine.
    A screen the deck does not carry is **reported**
    (`ScreenNotInDeck`) by `view`/`click`, never replaced by a placeholder,
    while focus falls back to the table's own order and the escape still
    leaves. `ScreenSessionError::{DegenerateSurface, NoButtonAt, Refused}`
    carry every refusal; `Refused` wraps the machine's `Refusal`.
  - Entering a screen focuses its authored first button; a prompt or an exit
    does not move focus (nothing was entered).
- `crates/cs_app/src/ui/front_end/machine.rs` (owner path): `FrontEnd::set_focus`
  — focus one specific button of the current screen, refusing a screen that
  has no such button for a person to press (`NoTransition`) and refusing while
  a discard prompt is open (`ConfirmationPending`). This is how the authored
  order reaches the one focus the machine owns; the session keeps no second
  focus state.
- `crates/cs_app/src/ui/front_end/mod.rs` (owner path): `mod screens;`, the
  re-exports and a module-doc paragraph (including the corrected status of the
  F45-B import, below).
- `crates/cs_app/tests/ui/screens.rs` (new, owner path): the 9
  `accept_f45_b_*` tests.
- `crates/cs_app/tests/ui/main.rs` (owner path): `mod screens;` and the F45-B
  target docs.
- This file.

## The design decisions a reviewer should check

1. **The deck is validated against the state table, not just parsed.** Three
   rules per screen: no dead button (a hotspot whose action the screen has no
   row for), no screen without an escape in its art, and no designed button
   missing from the art. The third rule is what makes "modern navigation is an
   accessible wrapper, not missing-screen placeholders" structural: a loader
   that cannot produce a hotspot for a designed button gets a refusal naming
   the button, not a screen that silently drops it. Whether the *original* art
   carries every designed button is unknown; if it does not, that is a finding
   to record, not a rule to weaken.
2. **One focus, two orders.** `ScreenSession` does not keep a focus of its
   own: it tells `FrontEnd::set_focus` which button the authored order puts
   focus on, so `FrontEnd::focus()` is always the truth and `view()` only
   reads it. A screen the deck does not carry keeps the table's row order.
3. **The pointer uses the image's transform.** `click` maps every button
   through the same `AspectFit` the artwork is drawn with, so a click and a key
   press on the same button are the same `Action` on the same table row. The
   letterbox bars therefore hit nothing — including the place the top-left
   button would occupy if the fit's 240px offset were forgotten (the test pins
   exactly that point).
4. **No provenance claim in the data.** `ScreenAssets` records an artwork id
   and rectangles and says nothing about where they came from. Nothing here is
   `original`; the deck is whatever a loader supplies, and today no loader
   supplies original data (see the unknowns below).
5. **Cancel cannot touch money because there is nothing to touch.** The stage
   adds no campaign field and applies no request: the minimum scenario asserts
   both halves — zero `Effect::Request` on every cancel path, and a real
   `cs_sim::campaign::CampaignState` (one victory applied, currency 500,
   revision non-zero) whose `snapshot()` is unchanged. Applying a request to
   the campaign is F45-C's wiring; this stage guarantees a cancel produces
   nothing to apply.

## Non-negotiable behaviour this stage encodes

1. *Every screen has valid Back/Cancel and focus; a visible button works* —
   the deck refuses a screen whose art has a dead button, no escape or a
   hidden designed button; focus wraps over the authored order and activates
   through the table.
2. *Briefing replay and recon never start the mission* —
   `accept_f45_b_replaying_the_briefing_and_viewing_recon_never_start_the_mission`
   walks the authored buttons and asserts no request, no `Resource::World`
   acquire and no held world.
4. *A failure returns to a coherent state without losing the draft* —
   cancelling the load releases the world and keeps the loadout for the retry;
   an undecorated screen reports itself and can still be left.
5. *Transitions acquire/release explicitly* — the session's outcomes are the
   machine's own `Release`/`Acquire` effects; the load-cancel test pins
   `Release(Resource::World)`.

(Non-negotiable 3 — not replacing the cabin/scrapbook/construction flow with a
debug picker — is exercised by driving those real screens; the flow itself is
F45-C's wiring.)

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f45_b_ --include-ignored`
discovers and runs **9** tests in one target, all passing (every other target
reports `0 passed ... filtered out`, so the selection is non-empty and
attributed):

- `accept_f45_b_cancel_at_every_preflight_screen_leaves_state_and_currency_unchanged`
  (the minimum scenario, AC02): every preflight screen carries authored art
  (`Screen::ALL` minus the in-flight/results screens), and the walk records
  every screen it actually cancels at so the closing assertion
  (`cancelled_at` == that list minus `Loading`) turns "every preflight
  screen" into a check rather than a claim in a comment. Each screen is
  reached and cancelled through `click` — `Quit` on the install selection and
  the main menu, `choose-another-install` on the diagnosis, `Back` on the
  cabin, `Back`/`Cancel` on the rest — dirty drafts go through the discard
  prompt and every input the open prompt receives is refused, the only
  transactions in the whole walk are opening the profile and closing it when
  leaving the cabin (the profile's own save/close, never a campaign one), and
  the campaign snapshot (currency 500) is unchanged. `Loading`'s cancel is the
  load test below, which pins the same snapshot.
- `accept_f45_b_focus_visits_the_authored_button_order_and_activates_it` —
  authored order ≠ table order on the briefing; entry focus, wrap both ways,
  activation, one-button wrap.
- `accept_f45_b_a_click_off_the_authored_art_or_between_buttons_hits_nothing` —
  letterbox and gap hits, hitless clicks change nothing, degenerate surface.
- `accept_f45_b_the_deck_refuses_a_dead_button_a_hidden_button_and_art_without_escape`
  — plus unknown key, system-result hotspot, artwork-not-an-image, duplicate
  screen, and the positive control that the full deck builds.
- `accept_f45_b_a_screen_without_authored_assets_is_reported_never_placeholdered`
- `accept_f45_b_replaying_the_briefing_and_viewing_recon_never_start_the_mission`
- `accept_f45_b_cancelling_the_load_keeps_the_selection_and_releases_the_world`
- `accept_f45_b_an_out_of_bounds_hotspot_never_reaches_a_deck`
- `accept_f45_b_artwork_is_an_image_id_and_buttons_are_ui_resource_ids`

Sensitivity was checked by mutation and then reverted (each run:
`cargo test --test ui --locked <test>`):

| mutation | tests that fail |
| --- | --- |
| `ScreenDeck::new` skips the escape rule (`if false && …`) | the deck-refuses test (`NoEscapeButton` case) |
| `ScreenDeck::new` skips the coverage rule (`if false { … }`) | the deck-refuses test (`HiddenAction` case) |
| `ScreenSession::entered` never re-focuses the authored first button | the focus test (focus stays at the table's `ReplayBriefing`) |
| `ScreenAssets::new` skips the artwork-kind check | the artwork-id test |
| `click` hit-tests logical coordinates instead of the fitted surface | the letterbox test: `(100, 100)` activates `ContinueProfile` instead of hitting nothing |
| `FrontEnd::apply` skips the open-prompt lock | AC02: the prompt's own cancel click comes back `Ok(AskDiscard)` instead of `Refused(ConfirmationPending)` |
| `FrontEnd::set_focus` skips the open-prompt lock | AC02: focus moves while a discard prompt is open |
| `FrontEnd::perform` asks `CloseProfile` on every `Quit` exit | AC02: "cancel on InstallSelect asked the domain for something" |
| the AC02 walk forgets to record one screen it cancelled at | AC02's coverage assertion names the screen (`left`/`right` differ by it) |

Full local checks before handover: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
`cargo test --workspace --locked` and the task selection above — all exit 0.

## Review (2026-10-07)

Reviewed by `bunny-alpha-2` — the same agent instance and model that
implemented the stage, in a fresh session that started from the hand-over
summary. That makes this a self-review: `checked` evidence at best, not
independent original-reference evidence, and no substitute for the owner's
human approval.

Found and fixed before hand-over:

- **AC02 did not cancel at every preflight screen.** The walk cancelled at 7
  of the 12 screens the deck must carry; `InstallSelect`, `ContentDiagnosis`,
  `MainMenu` and `Cabin` were in the asserted `preflight` list but never
  cancelled from. The walk now drives each of them (`Quit` on the install
  selection and the menu, `choose-another-install` back out of the diagnosis,
  `Back` out of the cabin, whose one legitimate transaction is
  `Request::CloseProfile`) and records every screen it cancels at, so the new
  closing assertion fails if a screen is ever dropped from the walk again.
- **Nothing pinned the open-prompt lock.** `FrontEnd::apply` and
  `FrontEnd::set_focus` both refuse while a discard prompt is open, but no
  test drove it. AC02 now presses the same cancel again and moves focus while
  the prompt is up and requires `Refused(ConfirmationPending)` both times,
  with the screen unchanged.

Considered and deliberately left alone: a screen whose art declares two
hotspots for one action is accepted. Focus is stored as an `Action`
(F45-A's design), so both regions would highlight and moving focus visits the
action rather than each region; activation is the same transition either way.
No original hotspot layout is decoded anywhere yet, so refusing a duplicate
now could block faithful art later — #742 (F45-B.1) should record what the
original layout does and revisit this rule if it needs it.

## What remains unknown (recorded, not guessed)

- **No original front-end layout is decoded anywhere in this repository.**
  The retail baseline records zero rows in the `ui_resource` collection and
  zero in `image` (`docs/findings/evidence/T584.json`, which names both among
  the 28 collections that hold no row on this installation;
  `docs/findings/2026-10-03-f14-d-derived-baseline-completeness-total.md`
  measured the same for `image`). So which original artwork belongs to which
  screen, which `ui-resource` id names which button, and every original
  hotspot rectangle are **unknown**; this stage invents none of them. Its
  fixture art is authored (`art-*` images, `ui-resource` buttons) exactly as
  F45-A's was, and says so.
- **The original art itself is decodable but unbound.** `ZBD/rimage.zbd` is
  the 254-name UI/HUD texture set and F08-B decoded every retail texture
  without error (`docs/findings/2026-09-28-f08-b-02-zbd-texture-package.md`,
  `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`), but nothing
  yet states which of those names is the main-menu background or the briefing
  panel. Resolving that is follow-up task **#742** (F45-B.1), together with
  the original hotspot layout; it feeds a loader that supplies `ScreenAssets`,
  which F45-C wires and F45-D captures (`retail,gpu`).
- **The original focus/tab order** is unknown; the stage defines focus order
  as the authored declaration order and records the original as unread.
- **Briefing voice** (F41's bus assignment) and the original confirmation
  wording for a discarded draft remain F45-A's open unknowns; nothing here
  claims them.
- **No rendering, no input device, no audio**: this stage is the data and
  navigation boundary (`click` takes a surface point). Mapping real input and
  drawing the artwork is F45-C/D's wiring.

These limitations are named here so a later fidelity claim cannot quietly
inherit them: nothing in this stage is `original-verified`, and the word
"original" in its title names the *shape of the data the layer consumes*
(artwork + hotspots), not provenance of the fixture data used to prove it.
