# F39-C: mission-program wiring, the display read model and teardown/retry

Status: designed behavior, **not** original-verified. Code:
`cs_content::objectives` (declared schema), `cs_app::objectives`
(`lower_program`, `ObjectiveSession`, consumers), acceptance:
`crates/cs_app/tests/accept_f39_c_objective_session.rs` (9 tests, prefix
`accept_f39_c_`) plus the two schema tests inside
`crates/cs_content/src/objectives.rs` (`accept_f39_c_` unit tests).

Acceptance case this stage owns: the stage's minimum scenario — **"Retry
after several waves and confirm no old timers, actors or cues survive"** —
and the wiring F39-B deferred: producer, consumers, teardown/retry, error
propagation.

## What F39-B left and what this stage adds

F39-B shipped the continuous `ObjectiveRuntime` but nothing could reach it:
no declared program produced a `TickInput`, no path consumed the
`ObjectiveEvent` stream, and there was nothing to retry. That was the
observable failure this stage closed. The wiring is three pieces:

- `cs_content::objectives::DeclaredObjectiveProgram` — the
  provenance-carrying authored record: objectives with initial states and
  reveal rules, count conditions with rosters, timer declarations with
  starts/domains/actions, swept trigger volumes, spawn-group bindings and a
  declared terminal precedence carried as `Resolved<DeclaredPrecedence>`.
  Because `cs_content` cannot depend on `cs_sim`/`cs_script`, the record keeps
  its own vocabulary (`ProgramSymbol`, `ProgramActor`, `Declared*` enums) and
  `try_new` closes the world: every reveal rule, timer start, timer action
  and count reaction may only name a declaration of the same program; a
  signal name may name the open set but never an ambiguous one — not the
  reserved actor-event symbol, not a symbol the program declares — because a
  raised signal reports under its own name and would alias that
  declaration's events; and a move or watch may only target a state a
  declared event can produce (`Active`, `Succeeded`, `Failed`, `Superseded`),
  because the transitions into `Hidden`, `Pending` and `Optional` all start
  from `Hidden`, which only the reveal rule may leave.
- `cs_app::objectives::lower_program` — the conversion boundary
  (`LoweredObjectives`): each declared record lowered field-wise into the
  runtime declarations (`ObjectiveSpec`, `CountCondition`, `MissionTimer`,
  `SweptTrigger`), the spawn-group symbols bound to their subject `ContentId`.
  Every `Resolved::Unknown` is refused by name — an unmeasured precedence can
  never become a silent default — and a timer declared on a non-gameplay
  domain (`UiWall`, `MediaUnscaled`) is refused by name rather than letting a
  menu frame or a cutscene advance a mission deadline.
- `cs_app::objectives::ObjectiveSession` — the producer→runtime→consumer
  path. `step` hands the runtime one `TickInput` (the whole producer surface:
  lifecycle transitions, real movement segments, signals, timer requests,
  objective requests, terminal requests, committed ticks) and dispatches the
  ordered `ObjectiveEvent` stream to the consumers it owns.

## The consumers and what each one answers

- `ObjectiveDisplay` — the read model the UI shows. One row per declared
  objective, `revealed` set only by `ObjectiveRevealed` and `state` set only
  by `ObjectiveChanged`, so `visible()` filters `revealed &&
  state.is_visible()` from stream facts alone. A refused change moves nothing
  on the display — the same fact the runtime reported, seen by the player.
  This is F39 non-negotiable behavior 5 on the consumer side: the display is
  event-driven, never runtime-queried, so a UI built on it cannot show an
  objective the reveal rule has not fired.
- `VecDeque<EmittedCue>` + `drain_cues` — the dialogue consumer's input. A
  `CueEmitted` event lands in the queue once; draining hands it over once.
  Each cue is stamped with its `SessionGeneration`, so a stale one can never
  be confused with the current generation's.
- `SpawnDirective` — the world consumer's input for one admitted wave. It
  carries the group's bound subject `ContentId` (the part the event stream
  alone does not name), the session generation, the idempotency key and the
  exact `ActorId`s the runtime allocated — the world instantiates exactly
  these ids, never its own count.
- The live wave registry — `live_actors()`/`live_wave()`: the wave instances
  admitted this generation and still live. A `Counted` event in a
  gone-category (`Destroyed`, `Captured`, `Escaped`, `Despawned`) releases
  the actor; `Disabled` deliberately does not, because a disabled actor still
  exists in the world and is still this session's to tear down.
- `Vec<SessionRefusal>` — every refusal the stream reported, lifted into one
  typed list: a refused objective change, a refused timer request, a request
  naming no declaration, a repeated spawn key (carrying the ids the first
  admission took), a repeated cue, an admitted wave whose group is unbound
  (unreachable from a lowered program, reported anyway). Error propagation is
  a queryable fact, not a line buried in a trace — and `step` itself returns
  the runtime's `RuntimeError` unchanged for a refused tick, matching "a
  refused tick changed nothing".

## Teardown/retry

`retry(new_generation)` is the contract the minimum scenario is about, and
the ordering matters: it builds the `TeardownReport` **first** — the live
wave actors the world must despawn, the cues emitted but never drained, the
declared timers still armed, the settled outcome — because the fresh
runtime's instance counter reuses the same `ActorId`s, and a world that
misses the report meets the old wave twice. The fresh runtime is built
before the old one is released, so a launch defect can never leave the
session half-torn-down. After the swap nothing survives: no objective state,
counter, trigger `inside` flag, timer, ledger key, signal eligibility or
latch — because the session owns every piece of mutable state and replaces
all of it (display re-seeded, cue queue drained, wave registry cleared, fresh
`ObjectiveRuntime` with a fresh `EmissionLedger`).

Stale-session handling is structural: every `SpawnDirective` and
`EmittedCue` carries its generation, and every event key carries it too, so
a consumer holding a generation-1 artifact can prove it stale by comparison
rather than by convention.

## Measured sensitivity of the acceptance suite

The eight `accept_f39_c_` tests in `crates/cs_app/tests/` and the two in
`cs_content` were written against the production path only. Behaviors that
fail the suite if removed:

- `lower_program` gone or incomplete: the tests do not compile / no launch.
- The event dispatch removed (spawns, cues, display, live registry,
  refusals): every consumer assertion fails.
- `drain_cues` made replayable: the "a drained cue is gone" check fails.
- The live registry releasing `Disabled` actors: the gone-category assertion
  is the one place the distinction is tested.
- `retry` keeping the old runtime: every post-retry check fails — armed
  `WAVE_3` surviving as `Armed`, expired wave timers surviving as `Expired`,
  the settled `Failure` still latched, instance ids continuing instead of
  restarting at 1, and the re-admission of `synthetic.f39c.wave-1` becoming a
  `SpawnRefused` under the stale ledger.
- The teardown report dropped or built after the rebuild: the
  `teardown.actors`/`teardown.cues`/`teardown.armed_timers` assertions fail.
- Refusals swallowed: `SessionRefusal::{Request,Timer,Spawn,ObjectiveChange}`
  assertions fail; a swallowed `NotAdvancing` fails the error-propagation
  check.
- Cross-reference validation dropped from the schema: the
  dangling-name/dead-declaration refusals in
  `accept_f39_c_the_schema_refuses_dangling_names_and_dead_declarations`
  fail — including the signal non-aliasing refusals (a signal named `0` or a
  declared symbol) and the dead-target refusals (a move or watch aiming at
  `Hidden`, `Pending` or `Optional`). One subtlety found while wiring: a
  `Vec` roster with duplicates would have passed `required <= roster.len()`
  while the lowered `BTreeSet` collapsed it below `required` — a dead
  declaration; `required` is now checked against the deduplicated roster.
- `retry` accepting the live generation: the
  `accept_f39_c_a_retry_cannot_rebuild_the_live_generation` test fails.

## Designed rules (synthetic only)

- **The declared program is closed.** A reveal rule, timer start, timer
  action or count reaction may only name a declaration of the same program;
  a mission whose deadline or wave was never declared is refused at
  `try_new`, where F39-B noted the runtime could only catch a dangling name
  at use. Signal symbols are the declared open set, bounded by
  non-aliasing: a signal reference naming the reserved actor-event source
  or a declared symbol is refused, and a move or watch may only target a
  state a declared event can produce — `Hidden`, `Pending` and `Optional`
  are unreachable targets and refused as dead declarations.
- **Declared actors and spawned instances share one `ActorId` space.** The
  runtime allocates wave instances from `ActorId(1)` upward in admission
  order (earlier tick first, then the expiring timers' symbol order), so a
  roster or trigger naming one of those ids names the spawned instance —
  the only way a condition counts a wave. A pre-placed actor's id must
  therefore sit outside the range its program's waves allocate; the fixture
  keeps `actor(7)` and `actor(41)` above the six ids its three two-raider
  waves take. Which actor ids original missions used is unmeasured.
- **A retry must advance the generation.** `retry` refuses the live
  `SessionGeneration` by name: the whole staleness guarantee is that the
  rebuilt session's artifacts carry a stamp the torn-down ones do not.
- **A raised signal is the producer's contract.** The schema refuses the
  colliding names it can see; a `TickInput.signals` fact under a declared
  name or `0` is a producer-side violation the runtime does not police —
  the producer surface already injects lifecycles, movements and requests
  on trust.
- **Declared domains map to declared policies.** `Simulation` lowers to
  `ClockPolicy::single_player_simulation` and `AuthoritativeGameplay` to
  `authoritative_gameplay` — both freeze while paused; a `UiWall` or
  `MediaUnscaled` deadline is refused at lowering rather than re-domained.
- **Instance ids restart per generation**, so the teardown report is not
  optional bookkeeping: the world must despawn the named actors before the
  new session's first step or meet them twice.
- **A settled outcome stops later work**, and a retry clears the latch — the
  retried session spawns where the failed generation's settled runtime
  refused to work.
- **Mid-mission save is declared unsupported** for this runtime: a snapshot
  of `ObjectiveRuntime` would need its own format, and
  `docs/contracts/SCRIPT-MISSION.md` permits declaring mid-mission save
  unsupported rather than inventing one. Retry is the session lifecycle this
  stage wires; an ordinary post-mission save does not depend on a
  mid-mission format.

## Unknown / deferred

- **Everything about the original game.** Whether an original mission
  program exists in this form at all; the original reveal rules, terminal
  precedence, timer domains, spawn-group vocabulary and dialogue-cue
  structure. The fixture's subject is `mission/synthetic.f39c.rescue`,
  `Origin::SyntheticFixture`, designed provenance under claim
  `f39c.synthetic-rescue` — it can never be mistaken for retail content.
  F39-D calibrates the rules with `retail`.
- **The producer's other arms.** `TickInput` has six fact surfaces; this
  stage's session feeds all of them, but who produces lifecycle transitions,
  movement segments and signals inside a running mission is the mission
  loop's job, not this boundary's. The fixture's program arms its timers by
  explicit `TimerRequest::Arm` because that is the producer contract a
  mission program drives.
- **`Disabled` and `Escaped` still have no producer** (unchanged from
  F39-A/B): reachable only by a caller that reports them. The live registry
  treats `Escaped` as gone and keeps `Disabled`, on the semantics of the
  categories themselves.
- **Self-watching and mutually watching declarations are still allowed.**
  An `OnObjectiveState` watch on its own symbol, or a cycle of them, simply
  never fires — a deferred liveness question, not a dead-declaration one,
  so the schema does not refuse it.
- **Reward and spawn-subject content kinds are unchecked.** A
  `GrantOptionalReward` id that is not an `objective`/reward kind and a
  spawn-group subject that is not an airframe still lower and run: the
  original vocabulary for both is unmeasured, so this stage keeps them
  open rather than guessing a kind.
- **Cue playback and spawn instantiation are hand-offs, not consumers.**
  `drain_cues` hands one `EmittedCue` to the dialogue system once; what plays
  the `dialogue` content id is the audio/dialogue stage's business (F41+).
  `SpawnDirective` hands one wave to the world; what instantiates an
  `airframe/` subject is the world's spawn path (F23+). This stage proves
  the ordered, once-only, generation-stamped hand-off — not the sound or the
  mesh.
- **The display is a read model, not a widget.** `ObjectiveDisplay` is the
  state a HUD renders; the Bevy UI binding that draws it belongs to the HUD
  stage (F46).
- **Trigger shapes beyond sphere and AABB** remain unmeasured.
