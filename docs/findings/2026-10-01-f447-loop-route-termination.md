# #447: the runtime follower expresses loop-terminated routes

Date: 2026-10-01. Task: #447 "Let the runtime follower express loop-terminated
routes" (follow-up to F31-C #127). Spec:
`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (stage `### F31-C`,
spec non-negotiable behaviors 1 and 3). Capabilities used: ordinary build/test
only — **no `CS_GAME_DIR` read and no evidence report required**.

## Files and the one observable failure (listed before editing)

Owner paths, per the F31 spec and this task: `crates/cs_sim/src/ai/navigation.rs`,
`crates/cs_content/src/routes.rs`, `tools/cs_inspect/src/routes.rs`, `tests/`.

Observable failure before editing: a declared `RouteDefinition` with
`RouteTermination::Loop` was refused by `tools/cs_inspect/src/routes.rs::project_route`
as `RouteProjectionError::UnsupportedTermination { termination: "loop" }`, because
`cs_sim::ai::navigation::RouteGraph` carried no termination and `RouteProgress`
was a single monotonic `next_index` over a finite node list.

## The decision: option (a), explicit loop semantics

The task offered two options and required that neither guess at the original
encoding. **Option (a) was chosen**: give the runtime real loop semantics.

Option (b) — "record the finding that the original 2000 route encoding has no
loop termination and remove the content variant" — was **not** taken, because it
would itself be a guess. F13 locates mission programs but recovers no route
layout, so this repository has no measurement of what the original encoding can
express. Deleting a declared content variant on the strength of an unmeasured
format is exactly the kind of fabricated finding AGENTS rule 4 forbids. Option
(a) keeps the declared variant and makes it followable, which needs no claim
about the original.

**This makes no original-data claim.** Whether the original route encoding
expresses a loop at all remains unknown. It was still unknown when this branch
was rebased: F31-D (#128) landed on `main` on 2026-10-01 and measured the
**carrier** — all 53 mission directories of the owner's installation carry the
observed `aiv.zrd` control member — while recording the route encoding itself as
`"state":"unmeasured"` and filing the decode as **#455** (`F31-ROUTE-ENCODING`).
So the resolving task for limitation 2 below is **#455**, not F31-D, and option
(b) was still unavailable at review time.

## The designed behavior

`RouteGraph` gained a `termination: RouteTermination` field, and
`RouteProgress` gained two counters beside `next_index`:

| field | meaning |
| --- | --- |
| `next_index` | the live target, which wraps to 0 at the wrap |
| `laps` | how many times the route has re-armed |
| `reached_total` | nodes reached across **all** laps; monotonic |

Three properties follow from that split, and they are what make a loop a route
rather than a rewind:

1. **Monotonic progress survives the wrap.** `reached()` counts across laps and
   never decreases; the wrap moves the *target*, not the count. A caller
   comparing progress between ticks does exactly what it does on an ending
   route. This is why `reached()` is no longer simply `next_index`.
2. **The wrap edge has no special geometry.** The wrap is the ordinary
   last-to-first edge, so arriving at node 0 after a wrap is the same swept test
   against node 0's own authored `arrival_radius_m` as any other arrival. No
   wrap-edge radius is invented. Measured: the fixture's node-0 arrivals fire at
   ~12.5 m into its 12 m radius, i.e. on the first tick whose swept segment
   crosses the volume.
3. **A mandatory marker is re-armed, not remembered.** Each lap re-targets
   every node in order, so no lap can skip a mandatory marker; there is no
   "already fired this lap" shortcut. Measured: every authored node id is
   targeted at least twice over the run, and the mandatory marker is targeted
   again on later laps.

A loop with fewer than two nodes cannot wrap — re-arming would re-target the node
the follower already occupies, so progress could never advance. That is refused
by name at **both** boundaries (`RouteError::LoopNeedsMultipleNodes` in
`cs_content`, `RouteGraphError::LoopNeedsMultipleNodes` in `cs_sim`), with the
threshold pinned to the same `MIN_LOOP_NODES` constant on both sides.

`project_route` now carries the declared termination across and
`RouteProjectionError::UnsupportedTermination` is **removed** — the acceptance
criterion required that `project_route` no longer report an unexplained
unsupported termination. Nothing can produce that variant any more.

## The `is_complete` bound: a defect the probes found and the review confirmed

Probe 1 (removing the loop re-arm) failed with `index out of bounds: the len is
3 but the index is 3` rather than with a clean assertion. That is not a probe
artifact: the same panic is reachable in production, not only under the probe.

`RouteProgress::is_complete` short-circuited to `false` for a loop, so the
bound `next_index >= route.nodes.len()` was never checked for that termination.
`NavigationSet::register_resuming(actor, reached)` is public, takes **no route**
and validates nothing, so a `RouteProgress` seeded past the end of a loop route
is an ordinary caller state — and `Navigator::decide_with_tie_break` then indexed
`route.nodes[next_index]` off the end. Before this task the bound covered every
route, because a loop could not exist; adding the termination is what opened the
hole.

It is fixed by keeping the bound for every termination:
`is_complete` is `self.next_index >= route.nodes.len()`. A loop followed through
`advanced()` never trips it, because the wrap keeps `next_index` inside the list;
a progress seeded past the end holds station — no target, no teleport — exactly
as it does on an ending route. Pinned by
`accept_t447_a_resume_past_the_end_of_a_loop_holds_station_instead_of_panicking`
(`crates/cs_sim/tests/accept_t447_loop_route_progress.rs`), which panics without
the fix.

## Known limitations that gate later stages (not silently dropped)

1. ~~**`crates/cs_app::ai::bind_route` still refuses a loop.**~~ **Resolved by
   #457** (2026-10-01): `bind_route` carries the declared termination through
   `RouteGraph::try_new_terminated`, `RouteBindingError::UnsupportedTermination`
   is gone, and `BoundRoute::termination()` exposes the declaration. #457 also
   gave the loop its own resume bound (`BoundRoute::max_resume_reached`), because
   `cs_app::AiNavigation::reached` reports the cross-lap total for a loop and an
   over-counted resume holds station (see above) instead of flying the route — an
   over-count is now refused by name (`ResumePastRouteEnd`) rather than wedging
   the follower silently. See
   `docs/findings/2026-10-01-t457-ecs-loop-route-binding.md`.
   One part of the affected content is still open and tracked there: through the
   **integrated** flight loop the wrap leg needs the turn fix in #451, so a
   mission that expects its AI to lap a patrol in flight does not get that yet.
2. **The original route encoding remains unmeasured.** No original route is
   parsed; whether the original expressed loop, patrol or end termination at all
   is unknown. F31-D measured the `aiv.zrd` carrier only and recorded the
   encoding itself as unmeasured. Resolving task: **#455** (`F31-ROUTE-ENCODING`,
   needs `retail`), which depends on F13-D (#52).
3. **The declared edges do not carry the wrap.** `cs_content` still requires
   edges to connect adjacent sequences, so a loop's last-to-first edge is
   *implied* by `RouteTermination::Loop` rather than authored. If an original
   record authors that edge explicitly, the content validator would refuse it as
   `EdgeNotAdjacent`. Unknown until **#455** measures a real record; recorded
   here rather than guessed at.

## Mutation probes (tests fail when the behavior is removed)

Each probe was applied to the working tree, run, and reverted with
`git checkout -- <file>`; none is committed.

| # | probe | result |
| --- | --- | --- |
| 1 | `RouteProgress::advanced` re-arm arm disabled (`if false &&`) | 4 of 8 FAILED |
| 2 | `project_route` maps content `Loop` to runtime `End` | 2 FAILED |
| 3 | `RouteGraph::validate` one-node-loop refusal removed | 1 FAILED |
| 4 | `RouteDefinition::try_new` one-node-loop refusal removed | 1 FAILED |
| 5 | arrival tested against the **last** node's radius instead of the target's | 1 FAILED |
| 6 | `reached()` returns `next_index` (rewinds on wrap) | 1 FAILED |
| 7 | `laps()` hardcoded to 0 | 1 FAILED |
| 8 | wrap credits two arrivals (would skip a marker) | 1 FAILED |
| 9 | `is_complete` short-circuits to `false` for a loop (the defect above) | 1 FAILED (panics) |

Probe 5 initially did **not** fail. The first version of that test measured the
distance from node 0 at the decision that *re-armed* the target — which is taken
at the last node, 177 m away, so it could not see which radius the wrap edge was
tested against. The test was rewritten to measure the node-0 **arrivals** and
bound the firing tick from both sides (`node0_radius - 1.0 < from < node0_radius
+ 2.0`); probe 5 then failed as expected. Recorded here because the first
version of the test was not discriminating and a reviewer should know it was
replaced for that reason.

The review re-applied probes 1, 2 and 9 independently on the pushed commit:
probe 1 failed 4 of 8, probe 2 failed the wrap-radius test at the lower bound
(`from` had collapsed into the last node's 6 m radius), and probe 9 — which the
implementer had already diagnosed in prose but had not left in the code — panicked
as described.

## Commands run

Exit codes as printed by the implementer and again by the review, on the tree
rebased onto `main`. The workspace total moved while other stages were merging
(1785 before any rebase, then F31-D, M02-A and F35-A landed); the task
selections did not move, and every rebase was re-run in full:

```
cargo fmt --all -- --check                                                -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                           -> 0 (1823 passed, 0 failed, 113 ignored)
cargo test --workspace --locked -- accept_t447_ --include-ignored         -> 0 (9 tests selected, all passed)
cargo test --workspace --locked -- accept_f31_ --include-ignored          -> 0 (63 tests selected, all passed)
```

The review additionally ran the whole ignored suite on this machine
(`CS_GAME_DIR` set, `CS_CAPABILITIES=retail,gpu,audio`): 1859 passed, 30 failed.
All 30 are pre-existing environment failures unrelated to this task — 28
`evidence_report_*` harnesses that require `CS_EVIDENCE_DIR`, the F18-D GPU
capture (no writable screenshot target), and two pinned-reference retail tests
whose extracted references (`private/f08d-reference`, `private/f09d-reference`)
are not present in this checkout. None of them touches routes or navigation, and
CI does not run ignored tests.

## Evidence

Synthetic fixtures and project design only. No original-data, visual, audible
or ordinary-play claim; this stage can award at most **checked**. No evidence
report is produced because #447 needs no capability beyond plain build/test and
makes no fidelity claim. F31-D owns the retail *carrier*-coverage evidence
(`docs/findings/evidence/F31-D.json`); **#455** owns the route decode.

Two prior-stage test files were touched, for compilation only:
`crates/cs_sim/tests/accept_f31_a_navigation.rs` and
`accept_f31_b_navigation.rs` build `RouteGraph` with a struct literal, so they
now name the new field as `RouteTermination::End`. No behavior those tests
cover changed.

## Sources

- `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (non-negotiable
  behaviors 1 and 3, stage `### F31-C`),
  `docs/contracts/FLIGHT-PHYSICS.md`.
- `crates/cs_content/src/routes.rs` (`RouteTermination`, `RouteDefinition::try_new`),
  `crates/cs_sim/src/ai/navigation.rs` (`RouteGraph`, `RouteProgress`,
  `Navigator::decide_with_tie_break`), `tools/cs_inspect/src/routes.rs`
  (`project_route`).
- `docs/findings/2026-10-01-f31-c-original-routes-and-moving-frames.md`
  (limitation 3: loop termination refused, not followed — resolved by this task),
  `docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md`
  (F13 recovers no route layout),
  `docs/findings/2026-10-01-f31-d-route-coverage-in-every-mission-type.md`
  (F31-D measured the `aiv.zrd` carrier and recorded the route encoding as
  unmeasured, filed as #455).
