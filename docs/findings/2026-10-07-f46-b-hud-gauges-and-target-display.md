# F46-B: HUD gauges and target display

Task #196. Projection in `cs_app::ui::hud`; tests are the `accept_f46_b_*`
tests in `crates/cs_app/tests/hud/`.

All behavior is **designed** and synthetic. Every fixture value is authored
here (`Origin::SyntheticFixture` / designed provenance); no original unit,
threshold, gauge layout or target semantic is claimed. Awards at most
*checked*.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/ui/hud/mod.rs`: `AircraftSample` loses the `weapon`
  field (the gauge is derived from the authority, never caller-asserted),
  `Instruments` loses `weapon`, `WeaponSample` and the F46-A `WeaponGauge`
  shape are removed, `HudError` gains `ForeignAuthority`, `Hud` gains
  `frame`.
- `crates/cs_app/src/ui/hud/frame.rs` (new): `HudSources`, `MountedGun`,
  `WeaponGauge`, `DamageZone`, `MountedOrdnance`, `HudFrame` and
  `Hud::frame` — the gather that derives the whole frame from the F27-C
  `WeaponSession`, the F28-C `OrdnanceSession`, the F29 `DamageResolver`
  and the F30-C `TargetConsumers`.
- `crates/cs_app/tests/hud/main.rs`, `binding.rs`: fixtures updated to the
  authority-derived gauge (the F46-A assertions keep their meaning through
  the new surface).
- `crates/cs_app/tests/hud/gauges.rs`, `targets.rs` (new): `accept_f46_b_*`.
- This file.

**One observable failure:** an actor whose selected bank fires its last
round must show `empty` on the very next frame while the bank's mounts stay
selected — a projection that cleared the selection, refilled the mount, or
kept the previous gauge fails
`accept_f46_b_zero_ammunition_is_a_gauge_state_not_a_selection_change`,
which drives the real `WeaponSession` (`register` + `consume_round`) and
reads the gauge through `Hud::frame`.

## What is implemented

`Hud::frame(sample, sources)` is the F46-B production path. It:

1. runs the F46-A `Hud::project` for the bound `(session, actor)` — the
   `Unbound`/`Stale`/`NonFinite` refusals are unchanged;
2. derives the weapon gauge from `WeaponSession::state(actor)` plus the
   cadence's registered `GunDefinition`s: one `MountedGun` row per mounted
   gun (mount, kind, ammunition type, rounds, selected, disabled), the
   selected bank's aggregate rounds, the distinct ammunition types the bank
   loads, `has_selection` and `empty`;
3. derives the airframe panel from `DamageResolver::graph(actor)` +
   `part_state`: one `DamageZone` per graph node with the `PartState`
   verbatim (`Unknown` is reported, never guessed) and the declared
   `scene_binding` carried through unresolved when it is unknown;
4. derives the launcher cluster from `session_ordnance_audit`: one
   `MountedOrdnance` per launchable component registered for the actor;
5. carries the published `HudTargetReadout` only when
   `TargetConsumers::bound()` is this `(session, observer)` — another
   observer's published views are never shown.

An authority that belongs to another session generation is **refused**
(`HudError::ForeignAuthority`), not read as an empty gauge: an empty gauge
is a display state and a stale table is a wiring fault, and the sheet's
"no stale ammo, damage or target on plane swap" is only safe if the two can
never be confused. An authority that is simply absent (`Option::None` in
`HudSources`, an unregistered actor, a closed ordnance session) produces no
rows — the frame field is `None` so the presenter draws nothing and nothing
is invented.

`empty` is the aggregate gauge state AC02 names: a bank is selected and
every selected mount is out of rounds. It never touches `selected`,
`has_selection`, cooldowns or the mount rows — the same rule `WeaponState`
itself follows (`select` never refills).

`cs_content::hud` is unchanged: no new declared policy was needed — the
measured cockpit binding names are a scene vocabulary the presenter maps
through each node's `scene_binding`, not a HUD policy.

## Designed semantics

- `MountedGun` rows come from `GunCadence::definitions` (the registered
  arsenal) rather than the selected bank alone, so a mount that is
  unselected, disabled or dry is still a named row — the `ggindicator`
  cluster shows guns, not selections.
- `WeaponGauge::ammunition` is the deduplicated set of ammunition types the
  *selected* mounts load. A one-type bank gives the `6char_type` field its
  answer; a mixed bank reports every type instead of inventing a winner
  (whether the original can even mix types on one bank is unmeasured).
- `DamageZone` carries `Option<Resolved<ContentId>>` verbatim. The measured
  indicator names (`rightwingdamage` etc.) are matched by the presenter
  through this binding; the HUD does not guess a zone-to-name table.
- `target` reuses `HudTargetReadout` directly — it is the record F30-C
  publishes *for* the HUD; re-shaping it would be a second truth.

## Unknowns (not guessed)

- **No ordnance rounds-remaining authority exists.** `StackLoad` is a
  declared mass budget that launching never decrements and the session
  keeps a launched component launchable forever, so `MountedOrdnance`
  reports mounted, launchable components only — a per-hardpoint count
  cannot be shown until the authority models one. Filed as a follow-up.
- Which mount the original maps to `ggindicator0`–`3` and which hardpoint
  to `mgindicator0`–`7`, what `4char_ammo`/`6char_type` render for a
  mixed-type bank, and whether `empty` has an original display treatment:
  the binding *names* are measured (F21-D), their semantics are not.
- The original target-reticle/threat-cue appearance and the display unit of
  target range (`Reticle::distance` stays SI `Meters`).
- The boost gauge: no measured binding names it, so no nitro row is
  projected; `SessionNitroRow` already exposes the authority a later stage
  can consume.
- The original bank names and cycle order stay unmeasured (F27/F30-D);
  `GunBank` is a mount set, and the gauge reports mounts, not names.
