# #457: the ECS-integrated follower binds and flies a loop-terminated route

Date: 2026-10-01. Task: #457 "Let the ECS-integrated F31 follower bind a
loop-terminated route" (follow-up to #447, which owned
`crates/cs_sim/src/ai/navigation.rs`, and to #446, which owns
`crates/cs_app/src/ai/navigation.rs`). Spec:
`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (stage `### F31-C`,
spec non-negotiable behaviors 1 and 3). Capabilities used: ordinary build/test
only — **no `CS_GAME_DIR` read and no evidence report required**.

## The one observable failure, listed before editing

`crates/cs_app/src/ai/navigation.rs::bind_route` refused a declared loop with
`RouteBindingError::UnsupportedTermination { termination: "loop" }` and built
its graph with `RouteGraph::try_new`, which defaults the termination to
`RouteTermination::End`. So the runtime could *follow* a loop (#447) but the
mission ECS could not hand it one. Affected content: **every AI aircraft on a
loop route inside the integrated ECS flight loop**.

## What changed

* `bind_route` maps the declared content `RouteTermination` onto the runtime
  one and calls `RouteGraph::try_new_terminated`, exactly as
  `tools/cs_inspect`'s `project_route` does. The moving-anchor binding and the
  authored node id -> runtime node id map are untouched.
* `RouteBindingError::UnsupportedTermination` is **removed**, not left unused —
  nothing can produce it any more.
* `BoundRoute::termination()` exposes the declared termination to a caller, and
  `BoundRoute::max_resume_reached()` reports the resume headroom the route has.
* `RoutePursuit::resume_past_headroom()` plus a new
  `NavigationRefusalReason::ResumePastRouteEnd` let the driver refuse an
  un-flyable resume **by name**, which
  `NavigationSet::register_resuming` cannot do (it takes a count and no route).
* `AiNavigation::laps()` was added and `AiNavigation::reached()`'s doc was
  corrected: since #447 `reached()` is a monotonic cross-lap **total**, not a
  position, so it must not be used to index a loop route's nodes.
* `crates/cs_app/tests/accept_t446_ai_navigation_wiring.rs`: the loop-refusal
  case became
  `accept_t446_bind_route_refuses_unbound_anchor_and_binds_a_declared_loop`.
* New `crates/cs_app/tests/accept_t457_loop_route_ecs_binding.rs`: 4
  `accept_t457_` tests.

## The seeded-actor hazard the handoff flagged, and how it is handled

`NavigationSet::register_resuming(actor, reached)` is public and takes **no
route**, so it cannot check `reached` against the node count.
`RouteProgress::is_complete` treats `next_index >= node_count` as complete for
*every* termination (deliberately, per #447, so an over-counted progress holds
station instead of indexing off the node list). On an **ending** route that
count is a real, legitimate state — "the aircraft is already past the route's
last node" — so a mission must be able to resume there. On a **loop** it is not:
a loop reaches a wrapped target only through a caller-supplied count, so a count
at or past `node_count` means "finished" and yet can never wrap. That follower
has no live target and holds station forever.

So the two terminations have different bounds, and both are now stated:

| termination | `max_resume_reached()` | resuming at the node count |
| --- | --- | --- |
| `End` | `node_count()` | legal — the finished state |
| `Loop` | `node_count() - 1` | refused by name (`ResumePastRouteEnd`) |

An over-count is refused, not clamped, and both branches are pinned
(`accept_t457_a_loop_resume_past_the_last_node_is_refused_by_name`,
`accept_t457_an_ending_route_resume_past_its_end_is_still_legal`). The refusal
is per tick, so a mission that fixes its count is not wedged forever.

One consequence recorded rather than hidden: a refused pursuit is never
registered, so the follower produces **no command** for it and the aircraft keeps
whatever `FlightInput` it last held (level flight, for a freshly spawned one).
That is a loud, named failure in the tick report, not a silent station hold —
`AiNavigation::reached`/`laps` return `None` and `NavigationTickReport::refused`
climbs by one every tick — but the aircraft is not flown home either. Clamping the
count instead would hide the authoring error, so the refusal stays.

## What the loop test claims, and what it deliberately does not

The route is three **collinear** nodes down `-Z` (`entry`, `leg`, `turn`),
declared `Loop`, world-anchored. Through the real `PhysicsFixture` +
`FlightForcesPlugin` + `AiNavigationPlugin` the aircraft converges on every
node, reaches the last one, and the wrap is real: `reached()` is monotonic
across it, `laps()` becomes 1, the target re-arms to node 0
(`next_index() == 0`), and the route is never reported complete.

**One wrap is the honest ceiling of the integrated loop, and this test does not
claim more.** A closed circuit needs a U-turn, and #446 measured that the F24
synthetic airframe cannot fly one through the production loop: `roll`'s sign is
inverted (a commanded turn drifts the aircraft away from the marker) and the
envelope/airframe mismatch makes the achievable turn rate far too low. I
re-measured this for this task through the production path with a temporary
scratch probe (removed, not committed): with the target directly behind, the
follower commands a saturated `roll = ±0.667` that **flips sign every tick**,
because the heading error sits at the `±pi` wrap boundary, so the aircraft nets
almost no yaw at all (heading `-0.013 rad` after 2 s) and then spirals off on
the wrap leg. That is #451, not this task, and it is left unclaimed rather than
hidden behind an inflated arrival radius.

The *several-lap* behavior of the follower itself is pinned at the runtime level
in `crates/cs_sim/tests/accept_t447_loop_route_progress.rs` (#447), where the
committed heading step is applied directly and a kinematic closure can fly a
circuit. The ECS binding is what this task adds, and the ECS binding is now
exercised over the nodes the integrated follower can converge on.

Two other things from the handoff, handled by documentation rather than code:

* **`reached()` is a cross-lap total.** Its doc no longer says "how many
  leading route nodes", and `laps()` is the companion a caller should read. A
  caller that indexes a loop route with `reached()` would index past the node
  list; the tests read `laps()` and the graph's own `next_index` instead.
* **There is still no per-lap mandatory-marker query.** `FollowOutcome::
  first_mandatory_reached` and `PursuitState::progress` answer "the first
  mandatory marker ever reached", which on a loop is the same node after lap 1.
  A mission that must fire a trigger *each* lap has no API, and none was faked
  by rewinding progress — that would break the monotonic contract #447
  established. Recorded here as an open gap, not a limitation of this task's
  wiring.

## Unknowns that gate later stages

1. **The original route encoding remains unmeasured.** Nothing here parses an
   original route; whether the 2000 encoding expresses a loop, patrol or end
   termination at all is unknown. F31-D measured only that every mission
   directory carries the `aiv.zrd` control member. Resolving task: **#455**
   (`F31-ROUTE-ENCODING`, needs `retail`), which depends on F13-D (#52). This
   task therefore makes **no original-data claim** and can award at most
   **checked**.
2. **The wrap leg of the integrated loop needs #451** (`bind_route` binds a loop
   today, but a mission that expects its AI to lap a patrol in flight does not
   get that yet). Affected content: every AI aircraft on a loop route in the
   integrated ECS flight loop.
3. **No per-lap mandatory-marker query** (above). Affected content: any mission
   event that must fire once per lap on a loop route.

## Mutation probes (each applied, run, and reverted; none committed)

| # | probe | result |
| --- | --- | --- |
| 1 | `bind_route` builds with `try_new` (termination defaulted to `End`) | 3 of 4 `accept_t457_` FAILED + 1 `accept_t446_` FAILED |
| 2 | driver resume-headroom refusal disabled (`if false &&`) | `accept_t457_a_loop_resume_past_the_last_node_is_refused_by_name` FAILED |
| 3 | `max_resume_reached` made a blanket `node_count` | 2 of 4 `accept_t457_` FAILED + 1 `accept_t446_` FAILED |

Probe 1 is the important one: it shows the new tests fail on the old behavior
rather than passing vacuously.

## Review note (bunny-alpha-1, #457 review; the reviewer is the implementing agent)

The reviewer's probe found a real defect in the first version of the core
scenario: **it did not discriminate who flew the aircraft.** The route is
collinear straight down `-Z` and the spawn already points down it, so a neutral
straight-and-level command reaches the same three nodes. Probe 4 replaced the
follower's decision with `FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false)` in
`decide_navigation`'s candidate list: all four `accept_t457_` tests still
passed, so "the follower flew the loop" was not established by that test.

The fix (in the test, no production change) is an `unguided_flight()` control:
the **identical** production spawn in a world with `FlightForcesPlugin` and **no**
`AiNavigationPlugin`, advanced in lockstep with the guided world, plus an
assertion that the two aircraft's altitudes differ by more than 5 m at the wrap.
The measured separation is wide: the guided aircraft climbs to about **+10 m**
above the route (the follower's pitch channel commands the climb) while the
trim-only control sinks to about **-8 m**. Under probe 4 the new assertion
FAILS, so the loop flight is now causally attributed to the follower.

Probe 4 (after the fix): the follower's command replaced by a neutral
straight-and-level input -> `accept_t457_a_bound_loop_route_is_flown_and_re_arms_at_the_wrap`
FAILED (guided and unguided positions became identical).

The same review also corrected the stale note in
`docs/findings/2026-10-01-f31-c-original-routes-and-moving-frames.md`, which
still said `bind_route` refuses a loop.

## Commands run

```
cargo fmt --all -- --check                                                 -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                            -> 0
cargo test --workspace --locked -- accept_t457_ --include-ignored           -> 0 (4 tests selected, all passed)
cargo test --workspace --locked -- accept_t446_ --include-ignored           -> 0 (8 tests selected, all passed)
cargo test --workspace --locked -- accept_t447_ --include-ignored           -> 0 (9 tests selected, all passed)
cargo test --workspace --locked -- accept_f31_  --include-ignored           -> 0 (47 tests selected, all passed)
```

## Evidence

Synthetic fixtures and project design only. No original-data, visual, audible
or ordinary-play claim; this stage can award at most **checked**. No evidence
report is produced: #457 needs no capability beyond plain build/test and makes
no fidelity claim. F31-D owns the retail carrier-coverage evidence
(`docs/findings/evidence/F31-D.json`); **#455** owns the route decode.

## Sources

- `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (non-negotiable
  behaviors 1 and 3, stage `### F31-C`), `docs/contracts/FLIGHT-PHYSICS.md`.
- `docs/findings/2026-10-01-f447-loop-route-termination.md` (the runtime loop
  semantics this task binds; limitation 1 recorded the missing `bind_route`),
  `docs/findings/2026-10-01-f31-ecs-integrated-follower-gap.md` (the measured
  integrated-turn gap, #451),
  `docs/findings/2026-10-01-f31-c-original-routes-and-moving-frames.md`.
- `crates/cs_sim/src/ai/navigation.rs` (`RouteTermination`, `RouteGraph`,
  `RouteProgress`, `NavigationSet::register_resuming`),
  `crates/cs_content/src/routes.rs` (`RouteDefinition::try_new`),
  `tools/cs_inspect/src/routes.rs` (`project_route`).
