# F31-C: wiring original routes and moving reference frames

Date: 2026-10-01. Task: F31-C "Wire original routes and moving reference
frames" (`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, section
`### F31-C`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required). The parent F31 acceptance criteria AC01–AC04 are preserved, and the
minimum scenario "an AI displaced off route rejoins before the next mandatory
marker" is the `--follow` probe below.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/routes.rs` (extended): the producer half of the
  conversion. `RouteNode` gains `arrival_radius_m: Resolved<f64>` — the
  navigation **swept arrival volume**, deliberately distinct from the authored
  mission `trigger` volume — validated as finite and strictly positive
  (`RouteError::NonFiniteArrivalRadius`, `RouteError::NonPositiveArrivalRadius`).
  `RouteDefinition::resolve` returns a `ResolvedRoute` (with `ResolvedRouteNode`
  and the named `RouteResolutionError`) that has every navigation field known,
  so the declared synthetic arch fixture (`declared_synthetic_arch_route`,
  arrival radii `[3, 5, 5, 5, 8]`) can be projected.
- `crates/cs_sim/src/ai/navigation.rs` (extended): the runtime consumer.
  `RouteGraph::try_new` is a validating constructor (same rules as `validate`,
  nothing clamped); `FollowPlan` bundles one run's static inputs; `follow_route`
  drives one already-registered actor over a route from a supplied `start`
  state, sampling the route frame once per tick, and `FollowOutcome` reports the
  decisions, the set-owned progress, completion and the first `Blocked` hold.
- `tools/cs_inspect/src/routes.rs` (extended): the F31-C conversion boundary,
  the same place F24-C owns the flight conversion. `project_route` maps a
  `ResolvedRoute` into a `RouteGraph` (authored sequence -> runtime node key,
  world/moving frame carried, loop and unbound moving anchor refused by name),
  `AnchorBinding` binds a content anchor to a runtime actor id, and
  `routes --follow` resolves, projects and drives the production follower end to
  end and reports the rejoin, teardown and retry counts.
- `tools/cs_inspect/src/lib.rs`, `tools/cs_inspect/src/main.rs` (wiring/doc
  only): the `--follow` mode in the command help and the module doc.

> **Superseded in part by #447** (`docs/findings/2026-10-01-f447-loop-route-termination.md`):
> `project_route` no longer refuses a `Loop` termination — it carries the declared
> termination into the runtime graph, and `RouteProjectionError::UnsupportedTermination`
> has been removed. `crates/cs_app/src/ai/navigation.rs::bind_route` still refuses
> one (filed as **#457**); `bind_route` is not an F31 owner path.
- `crates/cs_content/tests/accept_f31_c_route_resolution.rs` (new),
  `crates/cs_sim/tests/accept_f31_c_navigation_follow.rs` (new),
  `tools/cs_inspect/tests/accept_f31_c_routes_wiring.rs` (new): the
  `accept_f31_c_*` tests (13 total).
- This file.

**One observable failure:** F31-B ended with no conversion boundary at all —
`cs_content::routes::declared_synthetic_arch_route` and
`cs_sim::ai::navigation::synthetic_arch_route` mirrored each other **by hand**,
the content record carried no navigation arrival volume (`trigger` is a mission
event volume, not an arrival test), nothing projected content into the runtime
graph, nothing bound a moving anchor, and no driver advanced an actor along a
route from a displaced start. So a displaced actor could not be shown to rejoin
before its next mandatory marker, an unknown navigation field had nowhere to be
refused, and the two fixtures could silently drift apart. `project_route`
closes that gap; `accept_f31_c_projection_reproduces_the_runtime_arch_fixture`
fails when the projection mis-maps any node (mutation probe 2), and
`accept_f31_c_displaced_actor_rejoins_before_the_next_mandatory_marker` fails
when the driver forgets the set-owned progress or never advances the kinematic
state (probes 4 and 5).

## The designed behavior

- **Resolve, never default (producer).** `RouteDefinition::resolve` returns the
  first explicit unknown clearance, position or arrival radius as a named
  `RouteResolutionError`; nothing is silently zeroed. A known arrival radius is
  already refused at authoring time if it is non-finite or non-positive.
  Arrival radius and mission `trigger` are resolved independently: a node can
  fire a mission event without being a navigation marker and vice versa.
- **Projection is the conversion boundary (content -> runtime).** Authored
  `sequence` becomes the runtime node key (`RouteNodeId(sequence)`), so a
  container reorder cannot rename a marker; the content record guarantees
  sequences are strictly increasing and unique and the follower's progress is
  list-ordered. The reference frame and clearance are carried across. A route
  authored against a moving anchor is addressed at the runtime actor id the
  caller bound through `AnchorBinding`; an unbound anchor is refused
  (`UnboundAnchor`), never invented. A `Loop` termination was refused here
  (`UnsupportedTermination`) because the runtime follower had no loop semantics;
  **#447** replaced that refusal with real loop semantics, so the declared
  termination is now carried across and the follower re-arms it.
- **Per-tick frame sampling (moving reference frames).** `follow_route` samples
  `frame_at(tick)` on every decision and hands it to the set, so the same local
  node position becomes the anchor's current world position. The set, not the
  driver, owns progress, so a displaced start does not reset it and an origin
  shift cannot restart the route (spec non-negotiable behaviors 1, 3 and 4).
- **Bounded driver.** `FollowPlan::max_ticks` caps a run so a wedged follower
  cannot loop forever; the driver stops early on completion or a `Blocked` hold.
  `FollowOutcome::first_mandatory_reached` names the first reached mandatory
  marker, and the `--follow` report records its `rejoin_tick`.
- **Teardown, retry and error propagation.** An actor despawn removes its
  pursuit state (`unregister`); a retry in a fresh session generation starts
  from an empty roster; a command carrying the previous generation is refused
  (`ForeignSession`) rather than followed by the retry set. The
  `routes --follow` report declares `torn_down`, `retry_reset` and
  `stale_generation_refused`, and the binary returns exit 2 on invalid input and
  exit 1 when the follow report cannot be built.
- **No Bevy, no clock.** The content and runtime halves stay on `cs_types`; the
  boundary tool links both crates, exactly as F24-C linked them.

## Designed vocabulary, not original data

Every fixture value, sequence, mandatory flag, clearance, arrival radius,
mission seed, session id, anchor id/channel and the displaced start here is
**newly authored project design**. None of it is a measurement of the 2000 PC
game. In particular the following stay **unknown** and are not guessed:

- the original route encoding, node layout and mandatory-marker semantics (F13
  locates mission programs but recovers no route layout);
- the original AI decision cadence, seeds and tie-break;
- the original navigation arrival volume/tolerance (if any) and its relationship
  to a mission trigger volume;
- how the original binds a moving route (carrier/train/escort) to its runtime
  object, and whether it used a per-route frame at all.

Resolving tasks: **F31-D** (`retail` route coverage), **#446** (ECS and
moving-anchor wiring), **#447** (loop termination). Those follow-ups were filed
with `create_tasks` and depend on this task.

## Known limitations that gate later stages (not silently dropped)

Affected content: the whole F31 navigation path. Resolving tasks: **#446**,
**#447**, **F31-D**.

1. **No ECS / integrated-flight-loop wiring.** `cs_sim` may not depend on
   `cs_content`, and `cs_app`/the ECS is outside F31's owner paths, so the
   producer-to-consumer boundary lives in `tools/cs_inspect` (as F24-C does for
   flight) and the F31-C tests drive the set with the same kinematic closure the
   F31-B probes use, not the integrated flight body. An ECS system that owns the
   `NavigationSet`, feeds it live state and consumes `FlightInput` is filed as
   **#446**.
2. **Moving-anchor binding is content-id -> runtime-id only.** `AnchorBinding`
   maps a `ContentId` to a `u64` actor id and nothing in this stage resolves a
   *live* carrier/train/escort world transform each tick, nor carries the
   `AnchorKind`. Completing that binding is part of **#446**.
3. **Loop termination is refused, not followed.** `project_route` returns
   `UnsupportedTermination { termination: "loop" }` because `RouteProgress` is a
   finite monotonic index. Whether loops exist at all in the original encoding is
   unknown; the decision is filed as **#447**. — **Resolved by #447**: the
   runtime follower now has explicit loop semantics (a `RouteTermination` on
   `RouteGraph`, plus `laps` and a monotonic cross-lap node count on
   `RouteProgress`), `project_route` carries the declared termination across and
   no longer reports an unsupported termination. Whether the *original* encoding
   expresses a loop is still unknown and stays with F31-D. See
   `docs/findings/2026-10-01-f447-loop-route-termination.md`.
4. **The authored string node key is dropped at the boundary.** The runtime
   `RouteNodeId` is a `u64` equal to the authored `sequence`; the authored string
   id (`"start"`, `"arch"`, …) is not carried into the runtime graph. This is
   deliberate (reorder-safe progress) but means a later mission-event binding by
   authored name must carry the mapping; noted for **#446**/F31-D.
5. **The original route encoding and cadence remain unmeasured.** No original
   route is parsed; `docs/research/FORMAT-NOTES.md` still records the gap, and
   retail coverage is **F31-D** (needs `retail`).

## Mutation probes (tests fail when the behavior is removed)

Each probe was applied to the committed tree (head `d6b4870`), the named test
was run, and the tree was restored with `git checkout -- <file>` (working tree
verified clean). No probe was committed.

1. `RouteDefinition::resolve` made to default an unknown position to
   `[0, 0, 0]` instead of returning `UnknownPosition` →
   `accept_f31_c_resolve_refuses_unknown_navigation_fields_by_name` `FAILED`
   (2 passed, 1 failed).
2. `project_route` made to drop the arrival radius (`arrival_radius_m: 0.0`) →
   all five projection/follow tests in
   `tools/cs_inspect/tests/accept_f31_c_routes_wiring.rs` `FAILED` (a zero
   radius fails the graph's own validation, so the projection is refused).
3. `RouteGraph::try_new` made to skip `validate()` →
   `accept_f31_c_graph_and_decision_boundaries_refuse_invalid_input` `FAILED`
   (an empty graph was accepted).
4. `follow_route` made to ignore the per-tick frame sampler (`frame:
   ReferenceFrameSample::IDENTITY`) →
   `accept_f31_c_follow_route_samples_a_moving_frame_each_tick` `FAILED`. This
   probe first **passed** against the initially committed test (which registered
   through `synthetic_pursuit_set`, resuming past the spawn node, so its
   `reached >= 1` assertion held for free). The test was strengthened to assert
   the second tick's target distance follows the moved frame, and the probe then
   failed as expected.
5. `follow_route` made to never advance its kinematic state (no `state`
   update) → `accept_f31_c_displaced_actor_rejoins_before_the_next_mandatory_marker`
   `FAILED` (a stationary actor never reaches its marker).
6. The `routes --follow` probe made to skip teardown (no `unregister`) →
   `accept_f31_c_follow_command_reports_rejoin_teardown_and_retry` `FAILED`
   (`torn_down` was false).

## Commands run

All four required checks, run from the repository root; exit codes as printed.
The full checks below are the run on the final tree after the moving-frame test
strengthening.

```
cargo fmt --all -- --check                                              -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                         -> 0 (no failures)
cargo test --workspace --locked -- accept_f31_c_ --include-ignored      -> 0 (13 tests selected, all passed)
```

The 13 selected tests are 3 in `cs_content` (`accept_f31_c_route_resolution.rs`),
4 in `cs_sim` (`accept_f31_c_navigation_follow.rs`) and 6 in `cs_inspect`
(`accept_f31_c_routes_wiring.rs`). None is `#[ignore]`d, so `--include-ignored`
selects the same set.

The end-to-end probe (`cargo run -p cs_inspect -- routes --follow`, exit 0):

```
{"schema":"cs-inspect-routes-follow/v1","source":"synthetic-fixture","retail":false,
 "route":{"id":"route/synthetic.arch","frame":"world","clearance_m":2,"node_count":5,"mandatory_count":4},
 "follow":{"displaced_start_m":[40,0,-40],"ticks":361,"reached":5,"complete":true,"blocked":false,
           "first_mandatory_sequence":1,"rejoin_tick":58,"rejoined":true},
 "lifecycle":{"torn_down":true,"retry_session":8,"retry_reset":true,"stale_generation_refused":true}}
```

## Evidence

Synthetic fixtures and design only. No original-data, visual, audible or
ordinary-play claim; this stage can award at most **checked**. No evidence report
is produced because this task needs no capability beyond plain build/test. F31-D
owns retail route-coverage evidence.

## Review status and identity

Implemented by `deepseek-1`. At the time of submission this branch has **not**
been independently reviewed; a Rally review claim is expected to run the four
checks above (including `--include-ignored` with `CS_GAME_DIR` set), reproduce
mutation probes, and record the actual implementer and reviewer identities and
whether the reviewer's context was fresh. This record makes no original-
reference claim, so no agent review is offered as original evidence, and no
agent may self-award `verified_original` or `release_approved`.

## Sources

- `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (`### F31-C`,
  acceptance tests AC01–AC04, non-negotiable behavior, "Research boundary"),
  `docs/contracts/FLIGHT-PHYSICS.md`, `docs/01-ARCHITECTURE.md`,
  `docs/contracts/CLI-EVIDENCE.md`.
- `crates/cs_content/src/routes.rs`, `crates/cs_sim/src/ai/navigation.rs`
  (F31-A/B contracts), `tools/cs_inspect/src/routes.rs` (F31-A `routes` command
  and the F24-C conversion-boundary precedent).
- `crates/cs_sim/src/targeting.rs` (session-qualified `ActorId` precedent),
  `crates/cs_app/src/environment/session.rs` (`RunSeeds::mission_ai_stream`).
- `docs/findings/2026-10-01-f31-b-pursuit-and-bounded-obstacle-avoidance.md`
  (findings template and the limitations F31-C addresses),
  `docs/findings/2026-09-30-f31-a-route-graph-and-maneuver-envelope.md`.
