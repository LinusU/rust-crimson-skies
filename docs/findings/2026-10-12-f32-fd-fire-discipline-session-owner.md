# F32-FIRE-DISCIPLINE: the reported per-actor fire discipline gets a session owner — and the probe's firing-tick count becomes a discriminator

Date: 2026-10-12. Task: #571 `F32-FIRE-DISCIPLINE`, resolving
`f32.d.limit.fire_discipline` from
`docs/findings/2026-10-03-f32-d-original-ai-roles-and-difficulty.md`.
Capabilities: ordinary build/test only. No `retail`, no original run — the
claim was about **this** engine's runtime, not about original behavior.

## The gap

F32-B made `FiringSolution` *report* `fire_discipline_ticks` and
`aim_error_rad` rather than keep them as cross-tick state, and F32-C recorded
that the state was owned by no session. F32-D's difficulty probe showed the
consequence: two tiers that differ only in those two knobs measured as
identical, because neither field reached a decision. The probe's
`firing_ticks` column was a control precisely because nothing could
discriminate it — a cadence nobody applies fires on every eligible tick
whatever number it carries.

## What changed

`cs_sim::ai::combat`:

* `FireDiscipline`, the session's per-actor cadence state: a map of actor →
  earliest tick its next shot is allowed on, owned by the one `CombatRuntime`
  of the session generation. A never-firing actor reads `Tick(0)`; a recorded
  shot advances the wait by the cadence the shot's own firing solution
  reported, and `ready_at` is monotone — a record moves it forward or leaves
  it, never back.
* `CombatRuntime::step` is now `&mut self` — the honest signature for a step
  that advances per-actor state. It gates every firing solution it produces
  with `FiringSolution::hold_until(fire.ready_at(observer))` and, when a
  mount still fires, records the shot: the observer's next shot may go out no
  earlier than its reported `fire_discipline_ticks` later. A step that fires
  nothing records nothing — a held shot, a refused request and an unarmed
  role all leave the wait to expire on its own, so an actor is never made to
  pay the cadence twice.
* `MountFireState::DisciplineHold` and `FireHoldReason::DisciplineHold`:
  a mount the arsenal cleared but the actor's cadence holds is **named** in
  the trace, with the ticks remaining — a *schedule* refusal like
  `CoolingDown`, except the schedule is the actor's, not the weapon's.
* `FiringSolution::fire_ready_at` records which session state produced the
  verdicts: `None` for a planner-produced solution that consulted no session,
  `Some(ready_at)` for a session-gated one.
* `aim_error_rad` is resolved the *declared* way: `AIM_ERROR_RAD_APPLIED` and
  `FiringSolution::aim_error_applied()` are `false`, pinned by
  `accept_f32_fd_aim_error_is_a_declared_report_only_field`. The firing
  solution names an error; it owns no trajectory to disperse. Applying the
  error to a real shot is the weapon system's job and is filed as #1347
  `F32-AIM-ERROR-APPLY` — deliberately not both applied and declared.
* `TierComparison::firing_ticks` joins the probe's comparisons and
  `DifficultyProbeReport::outcomes_differ` counts it: the run counts a firing
  tick only after the session's discipline gated the mounts, so two tiers
  that differ only in cadence now measure different counts — the column stops
  being a control.

`cs_content::ai` (the side that owns the measurement record): the
`f32.d.limit.fire_discipline` entries in `UNRESOLVED` and in the F32-D
evidence harness's `REVIEW_METHOD` now record the closure and name #1347 for
the aim-error half, matching how #568–#570 updated their claims.

`crates/cs_app/tests/accept_f32_e_combat_lowering.rs`: one binding gains
`mut`, the only edit outside the owner paths — mechanical fallout of
`step(&mut self)` on a test that steps one runtime twice. No `cs_app` source
calls `step`.

## Tests

`accept_f32_fd_*` — seven in
`crates/cs_sim/tests/ai/accept_f32_fd_combat.rs`: the session holds an actor
to its declared cadence (shot → `ready_at` advance → named `DisciplineHold`
inside the window → boundary tick fires again); the state is per actor and
per session generation; a step that fires nothing advances no cadence
(weapon-refused, request-refused); two profiles keep different cadences on
one runtime; a zero cadence never holds; the aim error is a declared
report-only field; and the load-bearing one — a roster whose four tiers
differ **only** in `fire_discipline_ticks` now measures strictly increasing
`firing_ticks` at every adjacent pair while the selection, protected and
deferred comparisons stay `Same`. An implementation that kept reporting the
cadence without applying it fails the hold, the boundary, the isolation and
the discrimination tests.

## What stays a claim

`aim_error_rad` reaching a real trajectory is open — #1347
`F32-AIM-ERROR-APPLY`. Whether the cadence the original applies is anything
like the designed `fire_discipline_ticks` is unmeasured original behavior,
gated by `f32.d.limit.difficulty_effects` (#566). And the session-owned state
is per generation exactly like the coordinator's: a network join or replay
that rebuilds a `CombatRuntime` starts every actor's cadence at `Tick(0)`,
which is what "never fired in this generation" means.
