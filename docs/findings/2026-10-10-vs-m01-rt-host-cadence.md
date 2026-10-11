# VS-M01-RT-HOST-CADENCE: the world-actor session runs on the composition's 120 Hz timeline

- **Task:** #1281 `VS-M01-RT-HOST-CADENCE` — "Decide the mission host's world-actor and
  record-player cadence against the 120 Hz fixed timeline"
- **Measured by:** `bunny-2`, 2026-10-10, on `origin/main` `ff6bf82d` plus a read of #1278's
  branch `rally/1278-add-the-mission-host-s-records-to-the-st` at `03d16ff` (that branch was in
  review when these measurements were taken and was **not** part of this one; it has since merged
  to `main`, so the composed entry this note reads is now in the tree)
- **Capabilities used:** ordinary build/test, repository and `origin/*` reads. No `retail`, no
  original executable run, no human review. Nothing here is `verified_original`.
- **Decision:** answer **(1)** — the host's timeline is 120 Hz and the world-actor session
  declares that same rate. `SESSION_TICKS_PER_SECOND` now *is*
  `crate::physics::BASELINE_FIXED_HZ`.

## 1. The contradiction

`MissionHost::step` (#1278's composed per-tick entry) takes its tick from `PhysicsTickLedger`
and hands it, unchanged, to every record the composition owns — the environment session, the
world-actor session, the animation player, the markers, the objectives. The ledger counts
committed fixed ticks at `physics::BASELINE_FIXED_HZ` = **120 Hz**
(`crates/cs_app/src/physics/adapter.rs:46`, `record_tick_boundary` at `:325`).

The world-actor session was declared at **64 Hz** (`SESSION_TICKS_PER_SECOND`, previously
`crates/cs_app/src/mission_world_actors.rs:132`), and `WorldActorSet::step` applies
`dt_seconds()` = `1 / ticks_per_second` on every step
(`crates/cs_sim/src/world_actors/runtime.rs:450`, `:1120`), while
`WorldActorSession::step` loops `while self.set.tick() < input.to`
(`crates/cs_app/src/world_actors.rs:1105`) — i.e. once per tick it is offered.

So a route follower authored at `speed_m_s` travelled
`speed * host_hz / session_hz` metres per real second: **1.875×** its authored speed
(120 / 64). The divergence is measurable without #1278: `MissionContent` already declares the
composition's timeline as `MissionContent::tick_rate()` =
`TickRate::new(BASELINE_FIXED_HZ)` (`crates/cs_app/src/mission_session/content.rs:392`,
"the fixed tick the composition flies on"), starts the environment session on it
(`:599`), and binds the world-actor program at `SESSION_TICKS_PER_SECOND` (`:618`) — two
sessions of one composition, two different seconds.

## 2. What the rest of the workspace says

| Reading | Rate | What it actually is | Verdict |
| --- | --- | --- | --- |
| `crate::physics::BASELINE_FIXED_HZ` (`physics/adapter.rs:46`) | 120 | "**Designed baseline, not original data.** 120 Hz is the spec's designed starting rate (`### F23-A`)"; the rate `PhysicsTickLedger` commits and `MissionHost::step` reads | **the composition's timeline** |
| `MissionContent::tick_rate()` (`mission_session/content.rs:392`) | 120 | "the fixed tick the composition flies on"; starts `prepare_environment`'s weather session (`:599`) | same number, same composition |
| `crate::playtest` (`playtest/mod.rs:309`, `:327`) | 120 | the production playtest harness builds the real app with `PhysicsAdapterPlugin::new(BASELINE_FIXED_HZ)` and `TickRate::new(BASELINE_FIXED_HZ)` | 120 |
| `diagnostics::scenario::SIM_HZ` (`diagnostics/scenario.rs:6`) | 120 | the simulation rate the diagnostics scenarios budget against (`SIM_TICK_BUDGET_US_120HZ`) | 120 |
| `physics::PhysicsSession` default (`physics/session.rs:283`) | 120 | `fixed_hz: super::BASELINE_FIXED_HZ` | 120 |
| F20 record player (`mission_animations.rs:640-666`) | caller's | "The caller states `ticks_per_second`" — the player maps a record's stored seconds onto the ticks *it is stepped with*. #1278's host hands it `BASELINE_FIXED_HZ`; `mission_launch::started_rows` hands a literal `64` while its own doc says "the tick rate is the **host's** timeline" (`mission_launch.rs:1315`) | follows the host ⇒ 120 |
| F34 trajectory rate checks (`cs_sim/.../runtime.rs:515-519`) | any | the set refuses `TickRateMismatch` when a trajectory's declared rate differs from the set's; the synthetic fixture declares 10 Hz. It pins *internal agreement*, never a specific number | neutral |
| `crate::synthetic::TICK_HZ` (`synthetic.rs:54`) | 64 | the **asset-free development scene**'s fixed rate, "chosen as a power of two so `1.0 / TICK_HZ` is exact in binary floating point" so one `App::update` is exactly one step. It loads no assets, never hosts a mission | not a mission timeline |
| F16 frame clock (`docs/findings/2026-10-06-f16-f-original-clock-pause-and-speed-up-policy.md:220`, `:298`) | 64 | "the project's designed fixed rate"; explicitly "a designed development value, not a measured original rate" | a different, designed clock |
| Test fixtures — `environment::fixture::STORM_RATE_HZ` (`environment/fixture.rs:66`), `input/session.rs:2442`, `input/platform.rs:453` | 64 | authored fixtures whose docs say so ("the fixture's fixed rate", "a power of two … cannot drift") | fixtures |

Every production consumer that actually *advances* gameplay time on the mission composition
reads 120. Every 64 is either a fixture, the asset-free dev scene, or a clock F16 declares as a
designed development value. No site in the workspace measures the original's rate: F31's
finding still says "the original route encoding and cadence remain unmeasured"
(`docs/findings/2026-10-01-f31-c-original-routes-and-moving-frames.md:158`).

## 3. Why (1) and not (2)

* **One tick, one meaning.** `MissionHostTick` publishes the host's committed tick next to the
  world-actor answer (`mission_session/host.rs:346` on #1278's branch:
  `world_actors: Option<WorldActorSessionTick>`), the marker delivery, the animation
  `TickReport` and the objective answer. Option (2) would put two different tick domains —
  120 Hz host ticks and 64 Hz session ticks — inside one record, and every consumer (renderer
  pose reads, pickup judging, event `at:` stamps) would have to know which domain it was
  holding.
* **The environment session already chose.** The same `MissionContent` that binds the
  world-actors starts the weather session at `tick_rate()` = 120. Splitting the two makes one
  composition disagree with itself.
* **The record player already follows the host.** `MissionAnimationPlayer` takes the caller's
  rate because it is stepped by the caller; #1278 hands it 120 for exactly that reason. The
  world-actor set is stepped the same way, for the same reason.
* **Option (2) costs an acceptance amendment.** It would require rewording #1278's accepted
  criterion "the world-actor session reached the host tick" (owner amendment, per this task's
  description). Answer (1) satisfies that wording as written, so **no acceptance criterion
  changes** and no owner amendment is requested.
* **Nothing original is claimed either way.** The choice is between two designed numbers; the
  original's cadence is still unknown. Answer (1) does not touch any `ClaimStatus`.

## 4. What changed

* `crates/cs_app/src/mission_world_actors.rs` —
  `SESSION_TICKS_PER_SECOND: u32 = crate::physics::BASELINE_FIXED_HZ` (was a literal `64`), with
  a doc that states the coupling it now encodes: the composed entry steps this session once per
  committed fixed tick, so the declared cadence *is* the composition's timeline, and the
  constant claims only that the reimplementation's host and session agree. Aliasing rather than
  writing `120` again is deliberate: the two numbers can no longer drift apart silently.
* `crates/cs_app/tests/campaign/vs_m01_rt_host_cadence.rs` (new) and the matching `mod` line in
  `crates/cs_app/tests/campaign/main.rs` — the two acceptance members below.
* `docs/findings/` — this note.

`crates/cs_app/src/mission_launch.rs` is **not** an owner path of #1281 and was not touched;
see §6.

## 5. The tests, and the negative control

Task test prefix: **`accept_vs_m01_rt_host_cadence_`**.

* `accept_vs_m01_rt_host_cadence_the_session_runs_on_the_composition_timeline` — pins
  `SESSION_TICKS_PER_SECOND` against `MissionContent::tick_rate().ticks_per_second()`, the rate
  the composed entry steps at. Fails if either number becomes a number of its own.
* `accept_vs_m01_rt_host_cadence_a_route_follower_keeps_its_authored_speed_for_one_composition_second`
  — launches a declared route follower at 12 m/s through production
  (`lower_world_actors` → `WorldActorSession::launch` → `step`), offers it one full second of
  composed ticks and asserts it travelled exactly 12 m and reached exactly the tick it was asked
  for.

Both run in CI (synthetic, no installation) and were run twice, on purpose:

| Constant | `…_the_session_runs_on_the_composition_timeline` | `…_a_route_follower_keeps_its_authored_speed_…` |
| --- | --- | --- |
| `= crate::physics::BASELINE_FIXED_HZ` (this branch) | pass | pass — 12.0 m in 120 ticks |
| `= 64` (the pre-task state, restored as the negative control) | **fail**: "session 64 Hz, composition 120 Hz" | **fail**: "it advanced 22.5 m in 120 ticks at 64 ticks/s (a rate mismatch runs the follower at 1.875 times its authored speed)" |

The 22.5 m figure is the divergence of §1 measured through production code, not derived.

## 6. What this does *not* close

* **The original's cadence stays unmeasured.** This is an internal-consistency decision between
  two designed rates, recorded under the existing `designed` claim, exactly as the constant's
  doc says. It is not evidence about the original executable.
* **`mission_launch::started_rows` still builds its measurement player at a literal `64`**
  (`crates/cs_app/src/mission_launch.rs:1315`) while its own doc calls that rate "the host's
  timeline" — which is 120. That file is outside #1281's owner paths, so it was left alone
  rather than edited from the wrong task; follow-up **#1284**
  (`VS-M01-RT-ANIM-RATE-LITERAL`) was filed for it. Nothing there advances a
  tick ("nothing here is advanced or rendered, the run only proves the consumer takes these
  rows"), so no measurement changes with it.
* **Tick-counted schedules shift in real time.** A gate scripted at tick 45 meant 0.703 s at
  64 Hz and means 0.375 s at 120 Hz. No original measurement pins those either (F31 §5), and
  every such schedule in the workspace is authored fixture data.
* **`accept_m01_lc_zeppelin_allegiance` passes a literal `64`** to
  `bind_mission_world_actors` (`crates/cs_app/tests/accept_m01_lc_zeppelin_allegiance.rs:42`).
  It states its own rate for a lowering/refusal assertion, steps nothing, and contradicts no
  production call site; it was not touched.
