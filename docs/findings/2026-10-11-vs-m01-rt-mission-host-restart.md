# VS-M01-RT-MISSION-HOST.03: the restart rebuilds the authored initial state

- **Task:** #1280 `VS-M01-RT-MISSION-HOST.03` — "Rebuild M01's authored initial
  state on restart with no leftover entity, cue or session id"
- **Implementer:** `bunny-alpha-2`, 2026-10-11, branch
  `rally/1280-rebuild-m01-s-authored-initial-state-on`, cut from `origin/main`
  `5fe4d4d2` and rebased onto `origin/main` `011dea5a` (which landed
  VS-M01-RT-MISSION-HOST `.02`, the terminal funnel this task's restart sits
  beside; the rebase merged both into `host.rs` and is recorded below)
- **Capabilities used:** `retail` (read of `$CS_GAME_DIR` through production
  code), `CS_ENGINE_IMAGE` set, ordinary build/test. Nothing here is
  `verified_original`; no original executable ran and no original capture was
  produced.

## What was built

### `MissionHost::restart`

`crates/cs_app/src/mission_session/host.rs`. The rebuild runs in the order
that cannot leave the session half-rebuilt (the task's order, verbatim):

1. `ObjectiveSession::retry(SessionGeneration)` **first** — its
   `TeardownReport` names the wave actors to despawn, the cues that will now
   never play, the armed deadlines and the settled outcome, taken *before*
   the fresh runtime exists because the fresh runtime reuses the old
   instance ids;
2. `MissionMarkerConsumer::retry(SessionId)` — the marker ledger's releases
   onto the new session id;
3. `EnvironmentSession::restart()` — the authored weather clock at tick
   zero, keeping the run's seeds;
4. a fresh `MissionAnimationPlayer` with the stage's join rows offered again
   at its tick zero (the same `offer_startup_rows` grouping the launch runs);
5. a fresh `WorldActorSession` relaunched from the stage's stored
   `LoweredWorldActors`;
6. a fresh `MissionSession` relaunched from the stored control program — its
   state is `Running` again, so whatever terminal the previous generation
   settled is cleared with the session that carried it;
7. a fresh `BlockLifecycleTable` and `WorldFactTable` (over the seed's
   `MemberResolver`), the operands re-collected, and the complete refusal
   list recomputed by `stage_refusals` — **the same function the launch now
   uses**, so a rebuilt host reports exactly the absences the launched one
   does;
8. the host's own records cleared (`stepped = None`, and the `.02`
   `settled` mark of the torn-down generation too — a rebuilt host is never
   "already settled", see the merge note below);
9. the world half: `unload_world` then `load_world` from the stage's own
   definition, instance and meshes;
10. the player body respawned from the stage's start recipe (every
    `MissionPlayerBody` despawned first, then the production
    `spawn_player`), so exactly one exists.

`SessionGeneration` and `SessionId` both advance: `restart` takes the
generation as a parameter (the composed entry mints it from the same
process-wide counter `mint_host_generation` uses at launch) and refuses the
**live** generation by name before anything is rebuilt
(`MissionHostRestartError::SameGeneration`; `IDENTITY-CONTENT`: no
cross-session id reuse). The refusal is checked here *and* inside
`ObjectiveSession::retry` — never worked around.

### The report

`MissionHostRestartReport` is the previous generation's remaining ownership
as data: the torn-down `SessionGeneration`, the new generation and
`SessionId`, the objective session's own `TeardownReport`, the marker
ledger's `MarkerTeardown`, and the world objects the unload despawned and
the reload respawned (the residency's own answers, not a narrative). A
composed failure is stored as `MissionHostRestartFailure { error }` naming
the refusing record (`CLI-EVIDENCE`: an exit is never a swallowed failure).

### The composed trigger

`mission_host_restart` is a `&mut World` system `install_mission_host`
installs in `PostUpdate`, beside the per-tick entry in `FixedPostUpdate`.
Two triggers, each consumed exactly once:

- **the playtest's own meta reset** (`R` → `PlaytestRequests.reset` →
  `perform_reset` in `Update` increments `PlaytestState.resets`), observed
  against a `MissionHostRestartWatch` resource (fact 11: `FixedPostUpdate`
  runs *before* `Update`, so a reset latch must live in `PostUpdate` or read
  `PlaytestState.resets` against a `Local`; an exclusive system has no
  `Local`, hence the watch resource);
- **a `MissionHostRestartRequest`** another system inserted — the seam "a
  restart requested after a terminal state" is latched through. On this
  branch nothing raises it automatically: `.02` has since landed on `main`
  (`0881eea4`, rebased into this branch) and its funnel **exits** the run on
  a terminal (`MissionTerminal::exit` as an `AppExit` message) instead of
  restarting it, so the windowed composition never comes back to ask. An
  automatic raise here would restart-loop any program that settles again
  immediately, which is exactly why the composed system watches requests and
  the playtest's reset — never the terminal itself. A "play again" style
  caller (or a menu path) latches this resource and gets the rebuilt initial
  state; the acceptance member `..._a_requested_restart_clears_the_settled_terminal`
  drives that seam against a settled host.

`MissionStage` is now a `Resource` the composition leaves on the world (it
already derived `Clone, Debug`): the restart's world half and the fresh
session relaunches are built from its own records. `teardown` removes it
and every new restart resource, so a second composition still starts from
nothing.

### Sitting beside `.02`'s terminal funnel

The rebase onto `011dea5a` merged this restart with `.02`'s landed work in
the same three files (`host.rs`, `compose.rs`, `mod.rs`); the two features
are complementary, and the merge needed no semantic change:

- `.02` owns **ending** a run: `MissionHost::settle` funnels the two terminal
  sources, keeps the `MissionTerminal` on the host (`settled`) and
  `mission_host_tick` sends its `AppExit` message. The restart owns
  **rebuilding** one: a fresh control session starts `Running`, so the
  settled terminal of the torn-down generation is gone with the session that
  carried it. The merge surfaced one real interaction, now fixed and
  asserted: `settled` is the **host's own** mark of a run that has ended
  (`step` answers `MissionHostStepError::Settled` and the composed entry
  early-returns on it), and the pre-`.02` restart did not clear it — a
  rebuilt host would have been "already settled" forever. `restart` now
  clears it beside `stepped`, the host's docs say so, and every acceptance
  member asserts `terminal().is_none()` after a restart (the synthetic ones
  also assert the host *was* settled before it, so the reset path is proved
  to be a restart after a terminal state).
- `mission_host_tick`'s early return for a settled host and
  `mission_host_restart`'s rebuild therefore compose: a settled host is
  stepped by nothing, and a restart request rebuilds it into the authored
  initial state, which the acceptance members assert from both sides.
- `outcome_of` (`.02`) and `with_app_world` (this task) are two independent
  helpers in `host.rs`; both survived the merge unchanged.

## Decisions this task had to make

### Fact 9: the world half reaches the residency through a scratch-`App`
world swap

`load_world`/`unload_world` take `&mut App`; a schedule system has only
`&mut World`. **Chosen: option (b)** — `with_app_world` in `host.rs` builds
`App::empty()`, swaps the live `World` into its main world, runs the call,
and swaps the mutated world back. It is safe for exactly one measured
reason, re-measured on this branch: every `app.` access in
`crates/cs_app/src/world/residency.rs` (17 sites) and
`crates/cs_app/src/world/spawn.rs` (9 sites) is `app.world()` or
`app.world_mut()` — nothing reads a plugin, schedule, runner or sub-app —
so the two entry points behave identically on an `App` that owns nothing
but a world handle. The scratch app drops with the placeholder world it was
built with; nothing of the composition lives inside it.

- **Why not (a)** (app-level `mission_session::restart(app, stage)` with the
  in-schedule system only latching): there is no mid-run `&mut App` hook.
  `App::run(&mut self)` moves the app into the runner and leaves
  `App::empty()` behind (fact 10, re-confirmed when the runner drops the
  app), so an external caller cannot exist while the windowed composition
  runs; interleaving one would mean replacing the runner, a bigger contract
  change than the restart needs and one that would fork the windowed and
  headless faces apart — the very thing `add_composition` exists to keep
  identical.
- **Why not (c)** (`create_tasks` for `World`-based residency entry points):
  it would put this task behind a new task on a non-owner path
  (`world/residency.rs`) for a conversion the measurement shows is already
  total. If a future streaming policy needs richer entry points, converting
  the signatures remains the cleaner long-term shape and this swap is the
  documented bridge until then.

### Fact 11: "the host tick back at zero" is the host's own record, not the
physics ledger

`PhysicsTickLedger` is the physics timeline's count of committed fixed
ticks; it is not an owner path of this task (`crates/cs_app/src/physics/`)
and resetting it mid-run would break every other consumer of the same
clock. The restart therefore clears what is *this host's*: `stepped` goes
back to `None`, and every session it drives stands at its authored initial
tick again (environment clock 0, world-actor set 0, record player
`advanced_through = None`, objective runtime and control session with no
stepped tick). The next composed step runs at the ledger's current tick,
which the fresh sessions accept — measured: `MissionAnimationPlayer::advance`
refuses only `tick <= advanced_through` (`None` accepts anything) and the
control session's `Countdown::tick` refuses only a non-advancing tick
(`None` accepts anything). Both acceptance members assert exactly this
reading.

### The announced load is not re-run by a restart

A restart re-runs `unload_world` + `load_world` and respawns the player; it
does **not** re-announce the F15 load. Re-announcing would mint a new
content-session generation and re-deliver bindings — that is a full
composition rebuild (`teardown` + `build_*`, VS-M01-RT-WINDOW's own path),
not a mission restart. The composition's one `LoadedItemBinding` therefore
stays exactly one across a restart, and the synthetic member asserts both
that it is not doubled and that no second one is left behind. The
`WorldMeshes` the reload needs are the stage's own caller-owned source,
which outlives the unload by design (the F18-B rule `unload_world`'s docs
already state for sectors).

### The settling and cue-emitting synthetic records

Two acceptance demands needed authored synthetic content, both through
production constructors:

- **A real terminal**: the restart suite's stage swaps the seed's control
  program for one whose single objective is `Const(true)` with
  `Action::Finish(Outcome::Succeeded)` — the IR's own terminal request —
  so the composed entry drives a program to a settled terminal headlessly.
  M01 itself cannot (its two `Finish` blocks start dormant behind the `-1`
  sentinel and every wake path needs world facts a headless run does not
  have — #1278's fact 7), so no retail program is touched.
- **A pending cue**: the F39-C fixture `declared_synthetic_objectives()`,
  production-lowered through `lower_program`, with its radio timer's start
  re-declared `DeclaredTimerStart::AtTick(1)` instead of `OnArm`. The host
  never raises `timer_requests` (fact 3's sibling rule: it raises no
  requests at all), so an `OnArm` timer could never fire through the
  composed entry; an auto-armed one fires on ordinary composed ticks and
  leaves exactly one cue pending, which the teardown report must name. No
  `ObjectiveSpec`, `MissionTimer` or cue table is constructed by hand, and
  nothing here is authored for M01.

## Residues

- **Who raises the restart after a terminal.** `.02` landed on `main` and
  this branch merged with it; the restart clears the settled terminal (see
  above). What stays open is a product path that *asks*: `.02`'s windowed
  run exits on a terminal (`AppExit`), so a "play again"/menu caller
  latching `MissionHostRestartRequest` belongs to a later task. The composed
  system deliberately does not watch the terminal itself (restart-loop).
- **`playtest::scene`/`residency` `&mut App` signatures** remain; the swap
  above is the documented bridge (see the decision), and option (c) stays
  open as a follow-up task if the owner prefers the conversion.
- The world-actor cadence divergence (#1281) and the missing block-lifecycle
  reader (#1282) are unchanged by this task; a restarted host inherits both
  exactly as a launched one.

## Acceptance

Tests named `accept_vs_m01_runtime_host_03_*` in
`crates/cs_app/tests/campaign/vs_m01_rt_host_restart.rs`, registered with
one `mod` line in that directory's `main.rs`:

- **`..._a_meta_reset_rebuilds_the_authored_initial_state`** (CI): composes
  the synthetic stage (harbor world, settling control program, cue-emitting
  objective program), steps until the environment clock and world-actor
  tick moved and a cue is pending, then raises the playtest's own reset.
  Asserts on produced state: the stored teardown report names the torn-down
  generation and every pending cue; `SessionGeneration` and `SessionId`
  strictly advanced; the world objects despawned and respawned are the same
  set; the environment clock is at tick 0, the world-actor session at its
  initial tick, the record player and marker consumer fresh on the new ids,
  the objective session on the new generation with **no** stale cue, the
  control session `Running` with no stepped tick, the host's tick record
  cleared, the stale composed-step report gone; the world resident again
  with the same authored population; exactly one player body at the stage's
  start pose flying the original-law record; the one delivered binding
  neither doubled nor gone. A second reset advances both identities again.
- **`..._a_requested_restart_clears_the_settled_terminal`** (CI): drives
  the same stage until its own `Finish(Succeeded)` settles a terminal
  through the composed entry, latches `MissionHostRestartRequest`, and
  asserts the request is consumed once, the fresh control session is
  `Running` with no stepped tick, the terminal generation's report is gone —
  and that the fresh session settles the *same* authored terminal again on
  its next step, which is the proof the rebuild is the authored initial
  state rather than an empty one.
- **`..._a_restart_that_reuses_the_live_generation_fails`** (CI): calls
  `MissionHost::restart` with the live generation and asserts
  `SameGeneration` is returned, that the host's generation, session id,
  environment clock and tick record are untouched, and that the composition
  keeps stepping the live host afterwards.
- **`..._retail_m01_restart_rebuilds_its_authored_initial_state`**
  (`#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`): M01's real
  plan and stage through `build_headless`, stepped, then restarted through
  the composed request seam. Asserts the same initial state on the real
  world (identical respawned object set), the real world-actor program at
  its initial tick, the real measured start pose, the join's startup rows
  offered to the fresh record player, the F39/`WorldObservation`/
  `BlockLifecycles` refusals still named, no synthesized objectives, an
  empty cue teardown (no declared program ever emitted one — named by being
  empty, never invented), the one binding unchanged, and that the fresh
  host still steps M01's real program to `Running` (M01 cannot reach a
  terminal headlessly).

All four fail if the restart is removed: the `MissionHostRestartReport` is
written by the restart and by nothing else, and the generation-reuse member
calls the restart itself.

## Checks run on this branch

| check | result |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | (recorded at handover) |
| `cargo test --workspace --locked -- accept_vs_m01_runtime_host_03_ --include-ignored` | (recorded at handover) |

## What is not claimed

No original executable ran; `retail` is read access to the owner's
installation. Nothing here is `verified_original`. The restart does not
reset the world's fixed physics ledger (documented above). The announced
load is not re-announced. The terminal *exit mapping* (zero only for
`Success`) is `.02`'s (landed on `main` before this branch's rebase); this
stage clears a settled terminal on request and never decides a run's exit.
