# F45-C.1: the front-end construction draft as a projection of the F44 screen

Date: 2026-10-17. Task: #747 / F45-C.1 "Construction draft is a projection of
the construction screen" (follow-up of #145 / F45-C; context
`docs/findings/2026-10-07-f45-c-flow-wiring.md`, "the front-end construction
draft's `components` are not translated into blueprint slot edits").
Capability used: none — all fixture data is authored synthetic data; nothing
here says anything about the original construction page.
Implementer: `swe2-max-1`; no independent review yet.

## The question the task set

F45-C's `ConstructionDraft { blueprint, saved, components }` is a flat
`Vec<ContentId>` the caller fills in. Which component belongs to which armor
zone, weapon mount or hardpoint is unmeasured, so the draft could never drive
slot edits, and its `saved != components` dirty check only ever compared two
caller-declared lists — both views of the same blueprint, neither of them the
F44 `ConstructionScreen` that actually edits. The task offered two ways out:

- **(a)** make the machine's draft a *projection* of the screen, or
- **(b)** define and measure a flat-list → slot mapping.

This stage takes **(a)**, for the reasons the F45-C finding already recorded:
a flat id cannot name the slot it came from, so a flat-list edit would have to
invent the mapping it claims to measure. Projection goes one way — slots to
ids — and that direction is already owned.

## What "the projection" is

`AircraftBlueprint::components()`
(`crates/cs_content/src/construction.rs`) is the canonical flattening the
budget validator itself prices: `(BudgetCategory, &ContentId)` pairs in
budget order — airframe, engine, armor fitments, gun fitments, ordnance
fitments, equipment. The flow drops the category half and keeps the ids, so
the draft's order is the validator's own and no second ordering policy is
introduced.

A blueprint alone does not show every pending edit: a queued sale lives on
the session, not in a slot. The projection therefore appends
`ConstructionScreen::staged_sales()` (new read-only accessor over
`ConstructionSession::sells()`) after the fitments, so
`saved != components` covers everything the screen's own `is_dirty` flag
does — edits and staged sales alike.

`saved` is the projection of the **committed** base: the fitted ids captured
the last time the screen was clean — at attach, and refreshed after every
commit, because `ConstructionScreen::commit`/`commit_saved` re-bases the
session on the committed blueprint. The session keeps no copy of the
blueprint it was opened on, so the flow remembers the projection in
`FlowDomain::construction_saved` and clears it with the rest of the
construction state when the profile closes.

## What the flow now does

- `FrontEndFlow::open_construction` opens the declared draft as before, then
  re-projects `saved`/`components` from the attached screen when the draft
  names the same blueprint. The declared lists are the machine-level input
  F45-B made them; with a screen attached they are a view the projection
  replaces, whatever they claim.
- `FrontEndFlow::edit_construction` no longer takes a flat list — there is
  no measured flat-list → slot mapping and inventing one was option (b)'s
  rejected premise. It takes a closure over the attached
  `ConstructionScreen` (`set_airframe`, `fit_armor`, `fit_guns`,
  `fit_ordnance`, `queue_sale`, … — the screen's own typed operations),
  refreshes the stored base if the edit left the screen clean (a commit
  reached through the closure), and re-projects. The machine-level
  `FrontEnd`/`ScreenSession` `edit_construction(Vec<ContentId>)` is
  unchanged: callers that drive the machine without a domain keep the F45-B
  input API.
- `FrontEndFlow::attach_construction` refuses
  `FlowError::ConstructionScreenDirty`: a screen that was edited before the
  flow saw it has no committed base to project `saved` from, so the attach
  fails rather than inventing a baseline. A screen attaches at its committed
  state — freshly `open`ed or rebased by a commit — or not at all.
- `FlowDomain::commit_blueprint` refreshes `construction_saved` from the
  committed blueprint after a successful `commit_saved`, before the run is
  re-read — so the next draft the machine opens projects against what was
  actually committed.
- A draft naming a blueprint other than the attached screen's is left
  exactly as declared: "repairing" it would hide the very divergence the
  commit's `ConstructionMismatch` check exists to catch.

## What is deliberately not claimed

- **No original behavior is measured here.** Whether the original's
  construction page held one draft object, which of its slots the player
  could edit, or what its discard prompt compared is unread. This stage
  makes the *reimplementation's own* two views consistent by construction;
  it says nothing about the original's internals.
- **No flat-list edit path exists.** A `Vec<ContentId>` still cannot drive
  slot edits through the flow; if a future task measures slot semantics it
  adds a real mapping, not a guess on top of this projection.
- **`saved`/`components` remain public machine fields.** Tests and callers
  that construct `ConstructionDraft` literals (F45-B machine tests,
  accessibility walks) keep working; only the flow's *facade* treats them as
  projection outputs rather than inputs when a screen is attached.

## Checks run

`cargo test -p cs_app --test ui -- accept_f45_c_1_`: 6 tests, all passing —
projection at attach/open, a screen edit and a staged sale each moving the
machine's dirty state (with the discard prompt firing on the real flag), a
commit re-basing `saved`, a dirty screen refused at attach, and a foreign
draft left unreconciled. Sensitivity was checked by disabling
`FrontEndFlow::sync_construction_draft`: the four projection tests fail, the
two guard tests still pass (they pin orthogonal behavior).
