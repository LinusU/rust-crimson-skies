# F20-C: the mission-marker consumer — AnimationLog gameplay markers reaching the objective layer

Date: 2026-10-05. Task: F20-C-marker-consumer (#507). Spec:
`specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-C`, non-negotiable behaviors 1, 2 and 5, AC02 and AC04. Shared
contracts: `docs/contracts/IDENTITY-CONTENT.md` (session generations,
`EventId`) and `docs/contracts/SCRIPT-MISSION.md` ("Mission state", "Objective
event ordering"). Capabilities used: ordinary build/test only — no `CS_GAME_DIR`
read, no render, no audio, so no `private/evidence/` report is produced.

This is the slice that closes the seam
`docs/findings/2026-10-02-f20-c-wired-session-integration.md` recorded as
missing. That finding's item 7 said the consumer "is not stubbed" and filed it:
*"the mission/objective marker layer that should turn a fired
`engine_started`/`door_opened`/pickup marker into gameplay does not exist yet
(F37/F39) … Inventing a parallel 'marker registry' here would be a guess about
a layer this task does not own."* F39-C's `ObjectiveSession` exists now, so the
layer exists and this is its consumer.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/mission_markers.rs` (**new**, ~900 lines): the consumer.
  `MissionMarkerBindings` (the declared cue → mission-signal table),
  `MissionMarkerConsumer` (`admit`, `drain`, `retry`, the applied ledger),
  `MarkerActivation`, `MarkerDelivery`, `MarkerRefusal` and the composed
  `step_mission_with_markers`.
- `crates/cs_app/src/lib.rs` (wiring only): the module declaration and one doc
  paragraph.
- `crates/cs_app/tests/accept_f20_c_marker_consumer.rs` (**new**): the nine
  `accept_f20_c_marker_consumer_` tests below.
- This file.

**One observable failure (before editing):** after F20-C's integration slice a
real `PhysicsSession` published `door_opened` into
`AnimationLog::drain()` and **no gameplay state anywhere answered it**. The
log was drained, `event.effect.is_gameplay()` was a public predicate nothing
called, and the mission layer's declared signal surface
(`TickInput::signals`) had no producer from animation. The marker was
observable and inert.

## Designed decisions (this feature is designed, not original-verified)

The original `mis_anim.zbd` / `cam_anim.zbd` marker encoding is undecoded (F13)
and `MarkerEffect` is a **designed** vocabulary (F20-A; MechWarrior semantics do
not transfer, F20 behavior 2). No rule below claims an original counterpart.

1. **A gameplay marker becomes a declared mission signal, and nothing else.**
   A fired marker carries an authored **cue label**, not a mission symbol, so
   which signal a cue raises is a *content binding*. `MissionMarkerBindings` is
   input — this module ships no table — and an unbound cue is refused by name
   (`MarkerRefusal::UnboundCue`) instead of being given a symbol. Inventing a
   symbol would repeat the mistake
   `docs/findings/2026-10-02-t415-spawn-tick-trigger-crossing.md` refused to
   make: "a guessed content id in a mission's trigger table is worse than a
   missing one".
2. **The signal is the only request.** The consumer does not set, reveal,
   complete or fail an objective, spawn a wave, arm a timer or grant a reward.
   F39 behavior 3 requires a waypoint to unlock or reset objectives *only*
   through a declared program action, and the declared action is reached
   through the signal the program's own rules listen for
   (`RevealRule::OnSignal`, `TimerStart::OnSignal`, conditions). A marker is
   therefore a producer of `TickInput::signals` like any other mission producer.
3. **"Exactly once per activation" is this layer's rule, not a borrowed one.**
   The evaluator already keeps a one-shot gameplay marker to one firing per
   activation and the playback already holds a backwards head, but the duplicate
   would become a duplicate *mission event* here, so the consumer holds the
   guarantee itself: every applied marker records
   `MarkerActivation { session, producer, marker }` and a second presentation of
   that activation is refused. The **producer serial** is what makes the key
   right rather than merely conservative — the playback allocates one per
   started instance and never recycles it, so two instances of one clip are two
   activations (two engines starting are two events), a rebind after teardown is
   a *new* activation (its gameplay effect applies again, exactly as the
   producer side already asserts in
   `accept_f20_c_stopping_an_instance_releases_it_and_a_rebind_plays_again`), and
   a loop pass, a skip, a reversal or a re-drained batch all keep the same
   serial. The tick and the event sequence are deliberately **not** part of the
   key.
4. **Checks run in the order a caller can act on them:** the event's own
   session first (a stale firing belongs to another mission), then whether the
   effect is gameplay at all, then the per-activation ledger, and only then the
   declared binding. So an unbound cue is a statement about the mission's
   program and never consumes an activation — a later binding of the same
   firing still applies it.
5. **Every drained entry is accounted for.** The delivery reports the blocked
   effects and the held advances as named refusals, the fired events as raises
   or refusals, and the presentation cues as a count. A held advance is named
   because the consumer drained the record saying so: a mission that could not
   see a rewind could not tell "the cinematic was rewound" from "the marker
   never fired" (F20 behavior 5).
6. **A refused objective tick hands its batch back.** The log is drained before
   the session is stepped, because it is drained whether or not the step is
   accepted. `ObjectiveSession::step` refuses a non-advancing tick *after* the
   input is built, so `MissionStepRefusal` carries the delivery it could not
   apply and the caller retries the tick with those signals. Re-draining
   instead applies nothing — the activation was consumed, which is what makes
   the layer idempotent.
7. **The composition adds signals, it never substitutes them.** The host's own
   `TickInput` facts stay; the marker's declared signals are appended. A signal
   bound to the reserved actor-event source (`SymbolId(0)`) is refused at the
   table, because such a raise would alias every counted actor's own event —
   the same rule `crate::objectives` states for its host-injected signals.
8. **`retry` is the F39-C generation change, at this layer's scope.** It clears
   the applied ledger and reports how many activations it released. It does not
   *restore* anything: the evaluator fires a one-shot marker once per
   activation, so a mission that needs the same marker's effect again must
   restart the animation (a fresh producer serial, a new activation) rather than
   expect a re-raise.

## What is still not wired, and who owns it

- **No production mission host drives this.** `ObjectiveSession` is host-owned
  (a plain struct, not a resource) and no app composition steps it; the
  animation side of that composition is filed as #509. The entries here are
  therefore plain functions, the same shape `bind_animated_node` and
  `apply_attachment_transitions` already use, and the tests drive them against a
  **real** `PhysicsSession` and a **real** `ObjectiveSession`.
- **Pickups and ammo are not this layer's.** F20 behavior 5 names them next to
  mission events. What is guaranteed here is that a gameplay marker reaches the
  mission layer exactly once per activation; what a raised signal is *spent*
  on — a pickup, an ammo grant, an objective — is the declared program's
  decision (F27/F36 own those effects). This slice names and counts no such
  effect, so it claims none.
- **The scene spawn/despawn callers of `bind_animated_node` /
  `release_superseded_instances`** are #508 and untouched here; the consumer
  binds to what those entries publish, not to the scene loader.

## Tests

`crates/cs_app/tests/accept_f20_c_marker_consumer.rs`, all nine prefixed
`accept_f20_c_marker_consumer_`. Every test calls production code: the real
`PhysicsSession` + `AnimationPlugin`, `bind_animated_node`, `advance_animation`,
`MissionMarkerConsumer::{admit, drain, retry}`, `step_mission_with_markers`,
and the real `ObjectiveSession` launched from the F39-C lowering of
`declared_synthetic_objectives`.

| test | what it pins |
| --- | --- |
| `..._a_fired_gameplay_marker_drives_the_mission_exactly_once` (minimum, AC04) | the wired session's door marker, crossed at its authored tick and then skipped past, becomes one mission signal; the F39 runtime raised it once and revealed the objective whose declared `OnSignal` rule names it; later fixed ticks, the skip and a re-offer of the same firing each raise nothing; the refused repeat is `RepeatedActivation` and the mission does not move |
| `..._a_loop_pass_raises_one_signal_and_no_presentation_cue` (AC02, behavior 1) | a looping rotor over four passes fires its gameplay marker once and its presentation cue once per pass; the consumer raises the gameplay marker once, counts the presentation cues, and raises none of them |
| `..._an_unbound_cue_is_refused_by_name` | a table that binds nothing refuses the door's cue by clip, marker, cue and pass; no signal is invented, no activation is consumed, and the same firing still applies to a consumer whose table binds it |
| `..._a_stale_session_marker_is_refused` | a firing published in session 44 is refused by a consumer serving session 45, before its cue is resolved, and consumes no activation |
| `..._a_blocked_marker_effect_is_reported_and_never_applied` (behavior 2) | a clip whose marker effect is `Resolved::Unknown` publishes a block that reaches the mission layer with the unknown's claim and reason, raises nothing, and is reported once rather than per tick |
| `..._the_declared_table_refuses_what_it_cannot_decide` | an empty cue, the same cue bound to two signals, and a binding to the reserved source are each refused by name; a valid table resolves by cue and nothing else |
| `..._a_refused_objective_tick_keeps_the_markers` | a non-advancing objective tick is refused, its delivery is named rather than dropped, the retry with those signals reveals the objective once, and a blind re-drain applies nothing |
| `..._a_reversed_cinematic_is_named_and_raises_nothing` (behavior 5) | a backwards advance is held, offers no marker, and the hold is reported with clip, instance and both clip times instead of draining away |
| `..._a_generation_change_releases_the_ledger` | `retry` reports the activations it released, leaves the ledger empty, and the old generation's firing is stale to the new one |

## Mutation probes

Each probe edited one production file, ran
`cargo test -p cs_app --locked --test accept_f20_c_marker_consumer` (exit 101
on failure), then restored `crates/cs_app/src/mission_markers.rs` byte for byte
(`cmp` clean against `private/scratch/507-probes/mission_markers.rs.orig`; the
selection was green again afterwards).

| probe | edit | tests that fail (of 9) |
| --- | --- | --- |
| A the ledger is not consulted | `admit` skips the `applied.contains` guard | `..._a_fired_gameplay_marker_drives_the_mission_exactly_once` |
| B the effect kind is not read | `admit` skips the `is_gameplay` guard | `..._a_loop_pass_raises_one_signal_and_no_presentation_cue` |
| C the session is not checked | `admit` skips the stale-session guard | `..._a_stale_session_marker_is_refused`, `..._a_generation_change_releases_the_ledger` |
| D blocked effects are dropped | `drain` iterates `blocked_markers().iter().take(0)` | `..._a_blocked_marker_effect_is_reported_and_never_applied` |
| E the composition raises nothing | `step_mission_with_markers` does not extend the input's signals | the three composed-step tests (`..._exactly_once`, `..._loop_pass_...`, `..._reversed_cinematic_...`) |
| F **the consumer is removed** | `drain` never reads the `AnimationLog` resource | six tests, including the acceptance test |

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0.
- `cargo test --workspace --locked` — exit 0; 377 test-suite results, 0 failed
  (log: `private/checks/507/workspace-test.log`).
- `cargo test --workspace --locked -- accept_f20_c_ --include-ignored` — exit 0,
  **64 tests matched** across the seven `accept_f20_c_*` files (9 of them this
  slice), all passing.
- the six mutation probes above.

**Host note, reported because it changed a run of record.** This machine
exports `CARGO_PROFILE_DEV_DEBUG=0`, which overrides the committed
`[profile.dev] debug = "line-tables-only"` and makes the pre-existing
`tools/cs_xtask` test `accept_t430_a_panic_backtrace_names_the_file_and_line`
fail (its child prints symbol names but no `file:line`). The first full
workspace run, made under that variable, therefore stopped on that test. The run
of record above was made with the variable unset
(`env -u CARGO_PROFILE_DEV_DEBUG cargo test --workspace --locked`), which is the
committed configuration; the same test passes there. Neither the failure nor the
fix touches this task's code, and no test was weakened to accommodate it.

No command needed `CS_GAME_DIR`; no `accept_f20_c_marker_consumer_*` test is
`#[ignore]`d; `CS_CAPABILITIES` (`retail,gpu,audio`) was not exercised.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim: this stage can award at most **checked**, and
the reviewer must inspect the declared-table boundary, the activation identity,
the refusal paths and the test sensitivity. The original marker encoding is
undecoded (F13), so no original cue label, signal or marker order is claimed.

## Sources

- `specs/F20-object-animation-and-authored-destruction-states.md`
  (`### F20-C`; behaviors 1, 2 and 5; AC02, AC04),
  `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
  (non-negotiable behavior 3 and the declared signal surface),
  `docs/contracts/IDENTITY-CONTENT.md` (session generations, `EventId`),
  `docs/contracts/SCRIPT-MISSION.md` ("Mission state", "Objective event
  ordering").
- `docs/findings/2026-10-02-f20-c-wired-session-integration.md` (finding 7 and
  the boundary this slice fills),
  `docs/findings/2026-10-02-f20-c-02-fixed-tick-instances-and-teardown.md` (the
  per-activation dedup and the producer serial this layer keys on),
  `docs/findings/2026-09-30-f20-a-animation-channels-and-event-markers.md` (the
  designed marker vocabulary and the undecoded original),
  `docs/findings/2026-10-02-t415-spawn-tick-trigger-crossing.md` (the
  "never invent a symbol for a content reference" boundary reused here),
  `docs/findings/2026-10-01-f39-a-objective-trigger-spawn-semantics.md` and
  `crates/cs_app/src/objectives.rs` (the producer surface and the
  reserved-source rule).
- `crates/cs_app/src/animation/playback.rs` (`AnimationLog`, the drain seam,
  `AnimationRefusal::Held`), `crates/cs_sim/src/animated_object.rs`
  (`MarkerEffect`, `AnimationEvent`, the evaluator's per-activation dedup),
  `crates/cs_sim/src/objectives/runtime.rs` (`TickInput::signals`,
  `ACTOR_EVENT_SOURCE`) — read-only for this task.
