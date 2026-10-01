# F31-B: pursuit and bounded obstacle avoidance

Date: 2026-10-01. Task: F31-B "Implement pursuit and bounded obstacle avoidance"
(`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, section
`### F31-B`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required). The parent F31 acceptance criteria AC01–AC04 are preserved.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/ai/navigation.rs` (extended): the stateful pursuit half —
  `DeviationSide`, `PursuitState`, `PursuitRequest`, `PursuitDecision`,
  `NavigationSet` (one `PursuitState` per session-qualified `ActorId`),
  `tie_break_draw` and `Navigator::decide_with_tie_break`; `Navigator::decide`
  now delegates to it with an unseeded draw so F31-A behavior is unchanged.
  The synthetic pursuit fixture (`synthetic_pursuit_route`,
  `synthetic_pursuit_start`, `synthetic_pursuit_set`,
  `synthetic_pursuit_tie_blocker`, `synthetic_pursuit_actor`,
  `SYNTHETIC_PURSUIT_{DT_S,SEED,SESSION}`) lives beside the F31-A one.
- `crates/cs_sim/src/lib.rs` (wiring only): the `[`ai`]` doc paragraph now
  names the F31-B set and its per-actor seed.
- `crates/cs_sim/tests/accept_f31_b_navigation.rs` (new): the
  `accept_f31_b_*` integration tests.
- This file.

`cs_content::routes` and `tools::cs_inspect::routes` were **not** changed:
F31-B only consumes the F31-A contract, and F31-C owns the content projection,
the ECS wiring and the original routes.

**One observable failure:** the F31-A `Navigator` has one bounded step per tick
and no memory, so a follower whose direct step is blocked by a centred blocker
either holds (`Blocked`) or flips between two symmetric clearing sides, and a
follower displaced off route never carries the progress it reached. Handing the
same roster to the set in two different orders had no defined output at all,
because no stateful, seeded owner existed. `NavigationSet` makes each actor's
draw a pure function of `(mission seed, actor id, tick)` and remembers the side
of a contiguous deviation, so reordering the observations cannot move a local
decision; `accept_f31_b_reordering_actors_keeps_each_actors_decision_sequence`
fails when the set is removed or when the tie-break reaches the decision
unseeded, and `accept_f31_b_same_seed_replays_and_a_different_seed_moves_the_tie_break`
fails when the seeded draw is ignored.

## The designed behavior

- **Per-actor, seed-derived draws.** `tie_break_draw(mission_seed, actor, tick)`
  is the domain-separated mission AI stream
  (`AI_NAVIGATION_DOMAIN`, the same label `cs_app::environment::RunSeeds`
  derives a mission AI stream from) subdivided by a stable mix of the actor id
  and the tick. It reads no roster, no ECS order and no advancing counter, so
  two actors — and two ticks of one actor — draw from independent streams
  (F31 non-negotiable behavior 5, AC02).
- **Order-independent roster.** `NavigationSet::decide_all` sorts the requests
  by stable `ActorId` before evaluating any of them, emits the decisions in
  that same ascending order, and refuses an actor named twice with
  `NavigationError::DuplicateActor`. The order the ECS presents the actors in
  therefore cannot reach a decision (AC02).
- **Remembered deviation side (hysteresis).** `PursuitState` keeps the side of
  the current contiguous deviation; while it is deviating the draw is pinned to
  that side so a symmetric blocker cannot make it flip-flop, and the side is
  cleared the moment the direct step clears again. The seed decides only the
  first side of a fresh deviation.
- **Monotonic progress and diagnostics.** The set owns each actor's
  `RouteProgress`; `PursuitState::stalled_ticks` counts consecutive no-headway
  ticks for diagnostics only and is never a teleport trigger (F31 behavior 4).
  A `Blocked` hold keeps the last committed side, so a follower that clears the
  blocker on the same side keeps going that way.
- **Session confinement.** The set refuses a foreign session/command generation
  (`ForeignSession`) and a request for an actor it does not own
  (`UnknownActor`), mirroring `cs_sim::targeting::TargetStore`.
- **No Bevy, no clock.** `cs_sim` depends only on `cs_types`/`cs_script`;
  `decide`/`decide_all` read no renderer, wall clock or RNG.

## Designed vocabulary, not original data

Every fixture value, seed, session id, envelope bound, clearance, arrival
radius and domain label here is **newly authored project design**. None of it
is a measurement of the 2000 PC game. In particular the following remain
**unknown** and are not guessed:

- the original AI decision cadence and any seed the original used;
- the original tie-break rule when two avoidance sides are symmetric;
- the original avoidance rejoin rule and its per-airframe maneuver limits;
- the original route encoding and mandatory-marker semantics (F13 locates
  mission programs but recovers no route layout).

Resolving tasks: **F31-C** (wire original routes and moving frames) and
**F31-D** (validate route coverage in every mission type, needs `retail`).

## Known limitations that gate later stages (not silently dropped)

Affected content: the whole F31 navigation path. Resolving tasks: **F31-B**
(this stage, for the pieces it now owns), **F31-C**, **F31-D**.

1. **Still no ECS/runtime wiring.** Nothing integrates `NavigationSet` with the
   Avian/flight loop or an ECS system, and the F31-B tests drive the set with a
   kinematic closure, not the integrated flight body. Producing commands into
   F23/F24 and closing the loop is F31-C's bounded slice.
2. **No original route is parsed and the parallel fixtures are not yet asserted
   equal.** `cs_content::routes::declared_synthetic_arch_route` and
   `cs_sim::ai::navigation::synthetic_arch_route` still mirror each other by
   hand; F31-C owns the projection and the equality assertion.
3. **The deviation is a bounded per-tick arc, not a full rejoin planner.** The
   follower always steers back to the current node, so AC03 is demonstrated on
   the synthetic route; a real off-route recovery in a moving frame needs
   F31-C/F31-D.
4. **The tie-break is only read at an exact two-sided tie.** When one bounded
   side clears before the other the draw is irrelevant, so an order-dependent
   draw can only change behavior on ticks that actually present a symmetric
   blocker. The pure-accessor test
   `accept_f31_b_seeded_tie_break_is_per_actor_and_seed_deterministic` pins the
   draw to `tie_break_draw`, and `decide_all`'s sort removes input order before
   the draw is taken; the reordering test is the AC02 end-to-end check.

## Mutation probes (tests fail when the behavior is removed)

Each probe was applied to the committed tree, the named test was run, and the
tree was restored (`git checkout -- crates/cs_sim/src/ai/navigation.rs`,
verified clean). No probe was committed.

1. The `ordered.sort_by_key(|request| request.actor)` line removed from
   `decide_all` → `accept_f31_b_decide_all_is_stable_in_actor_id_order`
   `FAILED` (input order leaked into the emitted order).
2. `actor_stream_domain` reduced to a constant `0` (all actors share one
   stream) → `accept_f31_b_seeded_tie_break_is_per_actor_and_seed_deterministic`
   `FAILED` ("actor ids must subdivide the stream").
3. `deviate` forced to `tie_break_signs(0.0)` (the draw ignored) →
   `accept_f31_b_explicit_tie_break_chooses_the_requested_deviation_side` and
   `accept_f31_b_same_seed_replays_and_a_different_seed_moves_the_tie_break`
   `FAILED`.
4. The remembered deviation side dropped (always draw fresh) →
   `accept_f31_b_deviation_side_persists_across_consecutive_deviation_ticks`
   `FAILED`.
5. The blocker test bypassed (`let (step, avoidance) = (desired, OnRoute)`) →
   `accept_f31_b_blocked_wall_holds_without_crossing_or_teleporting`,
   `accept_f31_b_reordering_actors_keeps_each_actors_decision_sequence`,
   `accept_f31_b_same_seed_replays_and_a_different_seed_moves_the_tie_break`
   and `accept_f31_b_deviation_side_persists_across_consecutive_deviation_ticks`
   all `FAILED`.

A sixth probe (the draw derived from a per-set counter advanced on every call,
instead of the pure function) was applied with the sort **in place**: the
reordering test still passed, because `decide_all` sorts before any draw and
each actor's stream is `(seed, actor, tick)`-pure in the passing design. This
is recorded as limitation 4 above; the per-actor purity is instead pinned by
probe 2's test and the accessor equality assertion.

## Commands run

All four required checks, run from the repository root; exit codes as printed.

```
cargo fmt --all -- --check                                              -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                         -> 0 (163 "test result: ok", no failures)
cargo test --workspace --locked -- accept_f31_b_ --include-ignored      -> 0 (12 tests selected, all passed)
```

The 12 selected tests are 3 unit tests in `cs_sim`
(`ai::navigation::tests::accept_f31_b_*`) plus 9 integration tests in
`crates/cs_sim/tests/accept_f31_b_navigation.rs`. No test is `#[ignore]`d, so
`--include-ignored` selects the same set. (The counts above are the re-run after
the review fix; the submitted commit had 11.)

## Evidence

Synthetic fixtures and design only. No original-data, visual, audible or
ordinary-play claim; this stage can award at most **checked**. F31-D owns
retail route-coverage evidence.

## Review fixes

Reviewed by `deepseek-1` (same agent identity and model as the implementer, in
a fresh session and a separate review claim — **not** an independent reviewer;
this stage makes no original-reference claim, so the review is not offered as
original evidence). Re-ran all four checks on the submitted commit
`edf2b61` and reproduced three mutation probes (the seeded tie-break forced to
one side, `decide_all`'s sort removed, and `world_position` made to ignore the
frame origin); each made the expected test fail and the tree was restored.

One gap was found and fixed:

1. **AC04's moving-waypoint half was untested.** The submitted
   `accept_f31_b_origin_shift_does_not_reset_progress_or_fire_arrival` covers
   only a rigid origin shift (state and frame translated together). The
   acceptance criterion is "A **moving waypoint** and origin shift do not reset
   progress or trigger false arrival", and no test moved the frame between
   ticks. Added
   `accept_f31_b_moving_waypoint_does_not_reset_progress_or_fire_false_arrival`,
   which (a) places the aircraft exactly at a node's authored *local* position
   under a displaced frame and requires no arrival, then (b) flies to the first
   marker, jumps the moving frame 200 m down the route between ticks, and
   requires that the set-owned progress is neither reset nor falsely advanced
   and that the follower re-targets the next mandatory marker at its new world
   pose. It fails when `ReferenceFrameSample::world_position` stops applying the
   frame origin (the displaced frame is then read as an arrival).

No production-code defect was found; the stateful set, the per-actor seeded
draw and the hysteresis all behaved as documented.

## Sources

- `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (`### F31-B`,
  acceptance tests AC01–AC04, non-negotiable behavior, "Research boundary"),
  `docs/contracts/FLIGHT-PHYSICS.md`, `docs/01-ARCHITECTURE.md`,
  `docs/contracts/CLI-EVIDENCE.md`.
- `crates/cs_sim/src/targeting.rs` (session-qualified store / `ActorId`
  precedent), `crates/cs_sim/src/damage/events.rs` (`ActorId`),
  `crates/cs_app/src/environment/session.rs` (`RunSeeds::mission_ai_stream`),
  `crates/cs_sim/src/ai/navigation.rs` (F31-A contract).
- `docs/findings/2026-09-30-f31-a-route-graph-and-maneuver-envelope.md`
  (findings template and the limitations F31-B addresses).
