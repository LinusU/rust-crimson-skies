# F45-A.1: the front-end state table follows the original's screen set

Date: 2026-10-19. Task: #1457 (follow-up of #742 / F45-B.1,
`2026-10-15-f45-b-1-original-layout.md`, and #1456 / F45-B.2,
`2026-10-18-f45-b-2-screen-script-handlers.md`). Capability used: ordinary
build/test; the retail measurements are those two records'. The original
executable was not run, so nothing here is `verified_original`. Implementer:
`sonnet-1`; no independent review yet.

## Owner decision

Recorded in Rally #1457 (2026-10-18): mirror the original's screen set; the
designed `NewProfile` / `ContinueProfile` / `OpenSettings` / `Quit` menu is
superseded; only `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`
is edited among protected paths; bind a transition only where a script measures
it.

## What changed in `crates/cs_app/src/ui/front_end`

| | before | after |
| --- | --- | --- |
| main menu rows | `NewProfile`, `ContinueProfile`, `OpenInstantAction`, `OpenLobby`, `OpenSettings`, `Quit` | `OpenCampaign`, `OpenInstantAction`, `OpenLobby`, `OpenSettings`, `OpenCredits`, `Quit` (the original's Campaign / Instant Action / Multiplayer / Preferences / Credits / Quit) |
| `ProfileSelect` | confirm, import, back | `NewProfile`, `ContinueProfile` (stay on the screen, set the intent), confirm, import, back |
| screens | 22 | 23: `Credits`, left by `Back` |

`FrontEnd::profile_intent()` exposes the intent so the navigation review's
state signature tells "profile screen, no choice" from "profile screen, choice
made"; without it the review skipped the state in which `ConfirmProfile` can
succeed.

## Stated choices (not original facts)

- **Name pairing.** `SCREEN_SECTIONS` still pairs `PassengerCabin` = `Cabin`,
  `ScrapBook_TOC` = `Scrapbook`, `Preferences` = `Settings`,
  `PlaneConstruction` = `Construction`, `FlightCheck` = `FlightCheck`,
  `MainMenu` = `MainMenu` by name. It is a choice, not a claim: no original file
  says which designed screen an original section is.
- **Profile choice on `ProfileSelect`.** The original Campaign button's target
  (a profile list per the layout record) was not read, so which of its controls
  create or continue a profile is unknown. Putting the choice there keeps the
  domain request (`OpenProfile(intent)`) without a main-menu button the original
  does not have.
- **Main-menu order** is declaration order of this table, not the original's.
- **`Credits`** has no pairing in `SCREEN_SECTIONS` and no artwork: its content
  is unread, so no deck is built for it and nothing is drawn.

## Unbound transitions (unchanged, listed with reasons)

Only `terminate` -> `Quit` and `MM_B_PREFERENCES` -> `OpenSettings` are measured
transitions of the original main menu. `MM_B_CAMPAIGN` shares an arm that calls
`deactivate(this)` and starts the layout row's script; the other buttons' handlers
go through unmeasured natives (`callback($$A$$, n)`, `callback($$E$$, n, ...)`,
`mail(...)`; per-id meanings are blocked under #159 / #694) and keep their
verbatim statements and `UnboundReason` from the F45-B.2 loader. The designed
`OpenCampaign`, `OpenInstantAction`, `OpenLobby` and `OpenCredits` rows are
therefore **designed transitions** that the original buttons are expected to
correspond to, not bindings the scripts prove.

## Still unknown (gates)

- Original focus order (gates keyboard/controller fidelity claims).
- The movie rows that back `MainMenu` and `Preferences`; neither screen has a
  full-size pane, so neither enters the deck (gates drawing them from original art).
- Original screens with no designed counterpart: Save/Load, Momento selection,
  Plane name/selection, Ordinance layout, and the contents of Campaign and Credits.
- Native call table (`callback`, `mail`), blocked under #159 / #694.
