# F46-C: HUD pages, mission map and pause

Task #197. Session/page wiring in `cs_app::ui::hud::session`; tests are the
`accept_f46_c_*` tests in `crates/cs_app/tests/hud/`.

All page composition is **designed** and synthetic. No original map layout,
page set, pause-menu wording or in-flight recon page is claimed; the
authorities the pages read are fixed by the sessions. Awards at most
*checked*.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/ui/hud/session.rs` (new): `MissionPage`,
  `PauseCommand`, `PageOutcome`, `MissionSources`, `GeographyMark`,
  `PlayerMark`, `ContactMark`, `MapView`, `PageView`, `HudTeardown` and
  `HudSession` — the in-flight page state the flight loop drives.
- `crates/cs_app/src/ui/hud/mod.rs`: `mod session` + re-exports,
  `HudError::WorldMismatch`, `Hud::policy`.
- `crates/cs_app/src/lib.rs`: crate-map paragraph extended to F46-C
  (wiring-only doc edit).
- `crates/cs_app/tests/hud/main.rs`, `session.rs`: `accept_f46_c_*`.
- This file.

**One observable failure:** after an aircraft swap the session's view must
project the new aircraft's instruments and gauges only — a wiring that
kept the old binding would draw the previous aircraft's nose/wing mounts
and 30-round bank. `accept_f46_c_aircraft_swap_rebinds_instruments_and_gauges`
rebinds through the real `WeaponSession`/`OrdnanceSession`/`DamageResolver`
(two actors in one authority) and fails if any old mount, count or
launcher row survives; the previous aircraft's sample is refused `Stale`
on the cockpit and the map alike.

## What is wired

`HudSession` owns three things and nothing else: the bound `Hud`, the
current `MissionPage` (`Cockpit`/`Map`/`Objectives`/`Recon`), and the local
pause. `view(sample, sources)` is the single consumer entry and runs
`Hud::project`'s bound check on every page first — a stale sample can
never draw any page.

- **Cockpit** is the F46-B `Hud::frame` verbatim.
- **Map** (`MapView`) projects four read-only surfaces:
  - `geography`: the `WorldDefinition`'s objects filtered through the load
    record's `activates`, each `GeographyMark` carrying the stable
    `WorldObjectId`, the authored translation and the load's own
    `initial_condition`. A `WorldInstance` whose `definition()` is not the
    supplied definition's `id()` is refused `HudError::WorldMismatch` —
    answering `activates` for objects of another world would be a wiring
    fault, not an empty map.
  - `contacts`: `TargetStore` records that are `revealed` **and**
    `present` — the fidelity gate is in the projection, not the presenter,
    so an unrevealed actor (the fixture roster's raider 7) is never in the
    list. `allegiance` is the store's declared relation (`None` stays
    unknown); `class`/`objective`/position are the record's own fields.
    A roster of another generation is refused `ForeignAuthority` with
    `source: "roster"`.
  - `player`: the bound actor's roster position plus the instruments'
    heading — `None` when the actor is not registered, rather than an
    invented position.
  - `objectives`: `ObjectiveDisplay::visible()` verbatim — the
    event-driven rows only, so a signal-gated objective (the fixture's
    secondary) is listed only after its reveal.
  - `commands`/`pause`: `PauseCommand::ALL` (the front-end pause screen's
    own `Resume`/`OpenSettings`/`AbortMission` rows; its `Back` lands on
    `Flight`, which is `Resume` again) and the held `PauseReason`, if any.
- **Objectives** is `ObjectiveDisplay::visible()` alone.
- **Recon** is the published `SpyglassReadout` (F30-C), shown only while
  the `TargetConsumers` binding names this observer — another observer's
  readout is never drawn.

Pause is a request answered in the input path's own vocabulary:
`pause()`/`open(page)` ask for the local pause (`PlayerRequest`/`Menu`),
`resume()` releases it and always lands on `Cockpit`. Under
`SessionMode::Multiplayer` the answer is `NoLocalAuthority` — the page
opens and the world keeps flying, so "the client paused the server" is a
state this type cannot express. Conversely a held pause implies a menu
page, so "paused while flying the instruments" is equally inexpressible.

`teardown()` reports the old display (`HudTeardown`: binding, page, pause)
and leaves the session unbound — the next `view` refuses `Unbound`.
`retry(session, actor)` validates the new binding **before** releasing the
old one: a mismatched actor returns `ActorSessionMismatch` with the old
display untouched, matching `ObjectiveSession::retry`'s "a refused retry
changed nothing".

## Designed semantics

- The page set (`Map`/`Objectives`/`Recon` beside `Cockpit`) and the
  `PauseCommand` row order are designed; which pages the original grouped
  onto its pause screen, and their artwork, are F46-D's to measure.
- The map offers `Resume`/`Settings`/`AbortMission` — the front-end
  `Pause` screen's distinct transitions, in its row order.
- `Recon` presents the spyglass readout: the only recon-flavored
  production data that exists. The front-end's briefing-time `Recon`
  screen (recon images) is a different surface with no in-flight
  authority; this page is *not* claimed to be the original's.
- `MissionSources.objectives`/`world` carry no session generation, so the
  projection cannot foreign-check them — the caller supplies the running
  session's own display and load; only `roster` and the `HudSources`
  authorities are generation-stamped.

## Unknowns (not guessed)

- Whether the original pauses to a map screen at all, which page is
  default, the page order and the exact menu rows: unmeasured (F46-D).
- Whether the original's map draws every active world object or a curated
  subset (roads, airfields, no trigger volumes): unmeasured. The
  projection deliberately reports all activated objects — filtering is the
  presenter's, and `GeographyMark` keeps the stable id so a future
  measured rule can name its set.
- Whether hidden units appear on the original's map under any fidelity
  option: unmeasured. This implementation hard-wires "never" — the
  projection cannot produce an unrevealed contact.
- Whether an in-flight recon/spyglass page exists at all in the original;
  the spyglass readout's presence here is a designed consumer of an
  already-published authority, not a claim about original screens.
- The original pause-menu labels (`Resume`/`Settings`/`Abort` are the
  front-end's designed rows) and whether "abort" asks to confirm.
