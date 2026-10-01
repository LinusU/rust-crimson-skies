# F45-A: front-end states and transition table

Task #186. Table, machine and checks in `cs_app::ui::front_end`; authored-screen
layout and aspect-fit in `cs_content::ui_layout`; tests are the `accept_f45_a_*`
tests in `crates/cs_app/tests/ui/`.

All behavior is **designed**, synthetic and not original-verified. Awards at
most *checked*. No original screen, hotspot coordinate, string or file was read.
The screen list is the one the spec and `UI-NETWORK.md` name, not an observed
original list.

## One observable failure

Press Cancel on the construction screen after editing the draft: the machine must
ask before discarding and request nothing. A table row with the wrong `Guard`
(or a machine that ignored it) leaves to the cabin at once and
`accept_f45_a_back_from_a_dirty_draft_asks_and_keep_editing_loses_nothing` fails.

## Designed semantics

- **One table.** `TABLE` holds a row per `(screen, action)`; `validate_table`
  proves no duplicates, an escape (Back/Cancel/Quit/Pause) on every screen, every
  screen reachable from install selection and every screen able to reach the menu.
- **Domain requests, not edits.** `FrontEnd` has no cash, ownership or objective
  field; transitions emit `Effect::Request`. Flight check commits aircraft and
  ammunition as one `CommitLoadout`.
- **Resources.** Each screen holds one input context, one audio scope and
  optionally the world; a transition lists releases before acquires, so leaving a
  mission releases the world and no input context is bound twice.
- **Drafts.** Back/Cancel from a dirty draft asks (`AskDiscard`); a load failure
  and a Cancel from loading keep the loadout; reaching the cabin or menu drops
  every working copy.
- **Hotspots.** `AspectFit` is integer arithmetic, centred, never stretched;
  hotspot edges use the image's scale and offset; `check_layout` rejects a visible
  button with no transition on its screen.

## Unknowns (not guessed)

- The original screen list, art, hotspot coordinates, tab order and voice:
  F45-B imports them; F45-D captures them (needs `retail, gpu`).
- Whether a mission requires a wingmate: the caller declares it
  (`set_wingmate_required`); the original rule is not known.
- The ammunition and aircraft options per mission and the F44 budget check of a
  loadout: only slot kinds are validated here.
- The F41 audio buses behind `AudioScope`, and whether the original keeps menu
  music across screens.
- Whether the original Back from the cabin closes the profile, and its
  confirmation wording.

## Not in this table

Lobby/multiplayer screens (F55-A), the instant-action customize flow (F49-A),
the scrapbook contents (F47-A), accessibility navigation (F52-A) and the
construction rules themselves (F44). `UI-NETWORK.md` lists those paths; each
belongs to its own feature's state table and must extend `Screen`/`TABLE`.
