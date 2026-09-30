# F31-A: route graph and maneuver-envelope contracts

Date: 2026-09-30. Task: F31-A "Define route graph and maneuver-envelope
contracts" (`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, section
`### F31-A`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/routes.rs` (new): the provenance-carrying content
  half of the route contract — `RouteNodeId`, `TriggerVolumeId`, `RouteNode`,
  `RouteEdge`, `TriggerShape`/`TriggerVolume`, `ReferenceFrame`/`MovingAnchor`/
  `AnchorKind`, `RouteTermination`, `RouteDefinition` (+ `RouteDraft`),
  `RouteError` and `declared_synthetic_arch_route`. `RouteDefinition::try_new`
  is the boundary: a node id outside the `route` namespace, an empty graph, a
  duplicate id, a non-increasing sequence, a non-finite known position, a
  non-positive known trigger shape and an edge that is not between adjacent
  sequences are each refused by name and never repaired.
- `crates/cs_sim/src/ai/navigation.rs` (new): the Bevy-free consumer half —
  `RouteGraph`/`RouteNode`/`RouteFrame`, `RouteProgress`
  (monotonic by construction), `ManeuverEnvelope`, `NavigationCadence`,
  `ReferenceFrameSample`, `Blocker`/`BlockerShape` and the swept
  `segment_hits_sphere`/`segment_hits_aabb`, `NavigationRequest`,
  `NavigationDecision`/`RouteStep`/`AvoidanceState`, `Navigator::decide` and
  the `SyntheticArchProbe` fixture.
- `crates/cs_sim/src/ai/mod.rs` (new): module docs for the F31 `ai` tree.
- `crates/cs_sim/src/lib.rs` (wiring only): `pub mod ai;` and the doc
  paragraph.
- `crates/cs_content/src/lib.rs` (wiring only): `pub mod routes;` and the doc
  paragraph.
- `tools/cs_inspect/src/routes.rs` (new): the `routes` command that renders the
  declared record as a read-only JSON report.
- `tools/cs_inspect/src/lib.rs`, `tools/cs_inspect/src/main.rs` (wiring only):
  `pub mod routes;`, the `routes` dispatch arm, the `--help` entry and the
  missing/unsupported-command text.
- `crates/cs_sim/tests/accept_f31_a_navigation.rs`,
  `crates/cs_content/tests/accept_f31_a_route_graph.rs`,
  `tools/cs_inspect/tests/accept_f31_a_routes_command.rs` (new): the
  `accept_f31_a_*` acceptance tests.
- This file.

**One observable failure:** the goal node sits behind the same wall as the
start, offset laterally, so a naive direct start-to-goal pursuit crosses the
wall — for the fixture, exactly once, through the left box at `x=100`,
`z in [-60, -12]`. A follower that pursues the objective directly, or that
skips an authored mandatory marker, therefore ends up inside a blocked wall.
`accept_f31_a_direct_goal_pursuit_would_cross_the_wall` measures that crossing
(one blocker hit by the straight line) and the arch traversal's
`crossed_blocker == None`; `accept_f31_a_route_through_narrow_arch_is_followed_without_crossing_the_wall`
fails with a `Blocked` hold or a crossed blocker when node targeting is
replaced by direct goal pursuit. `accept_f31_a_blocked_step_holds_position_without_crossing`
fails on `OnRoute` when the blocker test is removed.

## The contracts (designed)

The split mirrors `flight_tuning` ↔ `cs_sim::flight`: `cs_sim` may depend only
on `cs_types`/`cs_script` (`docs/01-ARCHITECTURE.md`), so the declared record
lives in `cs_content::routes` and the normalized consumer type in
`cs_sim::ai::navigation`. F31-C owns the conversion between them.

- **Ids versus sequence.** Every node keeps a stable `RouteNodeId` key *and*
  an authored `sequence`. Progress is derived from the ordered list and only
  ever advances by one (`RouteProgress`), never from a re-sortable index, so a
  mandatory marker cannot be skipped or re-entered on a reorder (F31
  non-negotiable behavior 1). An edge is only legal between adjacent
  sequences, so a declared graph cannot encode a shortcut.
- **Moving frames.** `ReferenceFrame::Moving` names the anchor content id and
  kind (carrier/train/escort); the runtime supplies a per-tick
  `ReferenceFrameSample` (`origin_m`, `yaw_rad`) and node positions are mapped
  with the same right-handed `+Y` rotation as `cs_types::space`
  (`FLIGHT-PHYSICS`, "Coordinate convention"), so a moving waypoint and an
  origin shift translate the state and the frame together (F31 behavior 3).
- **Swept arrival.** `Navigator::decide` tests the arrival as the segment the
  step sweeps, not an equality of float positions, so a fast aircraft cannot
  pass a waypoint between ticks (F31 behavior 3).
- **Bounded commands.** Every commanded heading change is clamped to
  `max_yaw_rate_radps * dt_s`, climb/dive to the envelope rate and speed to the
  cruise-throttle range; the emitted command is the same
  `cs_sim::flight::FlightInput` a player's controls produce, so the flight
  model has one command boundary (F31 behavior 2).
- **Blocked is explicit.** A desired step that would cross a blocker at the
  route's clearance is replaced by a bounded deviation if one clears, and
  otherwise by a held position and `AvoidanceState::Blocked` with a neutral
  command — never a crossing and never a teleport unstick (F31 behavior 4).
- **Determinism.** `decide` is a pure function of one typed request; it reads
  no clock, renderer, ECS order or RNG. `AI_NAVIGATION_DOMAIN` reserves the
  single domain stream a future declared tie-break may use, so cosmetic
  randomness cannot move the route (F31 behavior 5).

## Designed vocabulary, not original data

Every id grammar, frame kind, termination kind, trigger shape, envelope bound,
clearance, arrival radius and fixture value in both new modules is **newly
authored project design**, carrying `Origin::Designed`/`Origin::SyntheticFixture`
provenance. None of it is a measurement. The following are **unknown** and are
not guessed here:

- the original 2000 PC game's route encoding: which file stores a route, how a
  node, an edge and a trigger volume are spelled, its units and its
  orientation (F13 locates mission programs but recovers no route layout);
- how an original trigger volume fires, its shape and its association with a
  mission event;
- the original arrival rule (radius, sweep or event), its clearance and its
  maneuver limits per airframe;
- the original AI decision cadence and any seed it used.

The declared schema says so: `declared_synthetic_arch_route` is
`Origin::SyntheticFixture`, and every field's provenance is `designed`.
Resolving tasks: **F31-C** (wire original routes and moving frames),
**F31-D** (validate route coverage in every mission type).

## Known limitations that gate later stages (not silently dropped)

Affected content: the whole F31 navigation path. Resolving tasks: **F31-B**,
**F31-C**, **F31-D**.

1. **No runtime wiring in this stage.** Nothing integrates `Navigator::decide`
   with the Avian/flight loop or an ECS system; `SyntheticArchProbe` is a
   kinematic fixture that advances the state from the committed step, not the
   integrated flight body. Producing commands into F23/F24 and closing the
   loop is F31-B's bounded slice.
2. **Avoidance is one bounded step per tick and has no memory.** `deviate`
   samples only the direct heading and the envelope bounds on either side of
   it; if both are blocked the follower holds, and because the hold keeps both
   position and heading the same request recurs. Rejoining an off-route or
   blocked aircraft (AC03) needs stateful pursuit and is F31-B's slice. The
   contract, the `Deviating`/`Blocked` states and the swept blocker test exist
   here; the rejoining behavior is not implemented.
3. **No original route is parsed.** `declared_synthetic_arch_route` is the only
   producer; no original file is read and no conversion into `RouteGraph`
   exists. F31-C owns both.
4. **The declared and runtime fixtures are parallel types, not yet asserted
   equal.** `cs_content::routes::declared_synthetic_arch_route` and
   `cs_sim::ai::navigation::synthetic_arch_route` mirror each other by hand;
   F31-C is the stage that asserts the projection and the mapping agree.
5. **No seed/tie-break consumer.** `AI_NAVIGATION_DOMAIN` is declared and
   reserved but no decision draws from it; the AC02 reorder test is F31-B.

## Mutation probes (tests fail when the behavior is removed)

Each probe was applied to the working tree, the named test was run, and the
tree was restored (verified by digest). No probe was committed.

1. `clears` forced to `true` (blocker test removed) →
   `accept_f31_a_blocked_step_holds_position_without_crossing` panicked at
   `left: OnRoute, right: Blocked` (`test result: FAILED`).
2. The `heading_error_rad.clamp(-max_yaw_step, max_yaw_step)` replaced by the
   raw error →
   `accept_f31_a_heading_step_is_bounded_by_the_envelope` panicked:
   "the applied turn -1.5707963267948966 exceeds the envelope step
   0.016666666666666666".
3. Swept arrival replaced by `step.to_m == target_world` →
   `accept_f31_a_arrival_is_swept_not_position_equality` panicked:
   "a step that sweeps through the arrival sphere arrives".
4. The target always set to the last node (markers ignored) →
   `accept_f31_a_route_through_narrow_arch_is_followed_without_crossing_the_wall`
   panicked at the completion assertion after a blocked crossing.
5. The strictly-increasing-sequence check removed from `RouteGraph::validate` →
   `accept_f31_a_route_graph_refuses_duplicates_and_out_of_order_sequences`
   panicked at `left: Ok(()), right: Err(SequenceNotIncreasing {...})`.

The declared content boundary is likewise sensitive: the in-crate tests
`accept_f31_a_route_refuses_duplicate_ids_and_sequence_shortcuts` and
`accept_f31_a_route_refuses_nonfinite_position_and_carries_unknowns` fail if
`RouteDefinition::try_new` stops refusing those inputs, and the `routes`
command tests fail if the renderer stops naming the synthetic source or drops
a node.

## Commands run

All four required checks, run from the repository root; exit codes as printed.

```
cargo fmt --all -- --check                                             -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                        -> 0 (137 "test result: ok", no failures)
cargo test --workspace --locked -- accept_f31_a_ --include-ignored     -> 0 (26 tests selected, all passed)
```

The 26 selected tests are 5 unit + 9 integration in `cs_sim`, 3 unit + 4
integration in `cs_content`, and 2 unit + 3 integration in `cs_inspect`, all
carrying the `accept_f31_a_` prefix. No test is `#[ignore]`d, so
`--include-ignored` selects the same set.

## Evidence

Synthetic fixtures and design only. No original-data, visual, audible or
ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (`### F31-A`,
  acceptance tests AC01–AC04, non-negotiable behavior, "Research boundary"),
  `docs/contracts/FLIGHT-PHYSICS.md`, `docs/01-ARCHITECTURE.md`,
  `docs/contracts/CLI-EVIDENCE.md`.
- `crates/cs_types/src/content.rs` (`ContentId`, `ContentKind`, `Origin`,
  `Provenance`, `Known`, `Resolved`), `crates/cs_types/src/evidence.rs`
  (`ClaimId`, `ClaimStatus`), `docs/research/FORMAT-NOTES.md`.
- The `flight_tuning` ↔ `cs_sim::flight` producer/consumer split and the
  declared-schema patterns in `crates/cs_content/src/coordinates.rs` and
  `crates/cs_content/src/config.rs`;
  `docs/findings/2026-09-29-f24-a-flight-equations-tuning-schema-and-synthetic-probes.md`
  (findings template).
