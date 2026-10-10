# VS-M01-RT-MISSION-HOST.02: one terminal, two sources, one exit the caller can trust

Rally #1279, subtask `.02` of #1217 (`VS-M01-RT-MISSION-HOST`). Implementer:
`bunny-alpha-2`. Measured and written 2026-10-10.

## What this stage had to settle

`.01` (#1278) gave the composition one composed per-tick entry
(`mission_host_tick`) that advances the stage's records in the documented
order and writes what each of them produced into `MissionHostReport`. It did
not end a run. This stage makes a terminal state end the run cleanly, with the
exit the caller can trust.

The result lives in
`crates/cs_app/src/mission_session/terminal.rs` (new) and
`MissionHost::settle` in `crates/cs_app/src/mission_session/host.rs`:

* `MissionTerminal` — the outcome, which runtime reported it, the host tick,
  the session generation, the objective symbol that asked (when the record
  named one), the undrained cue count, the still-bound audio-loop count, the
  exit, and the one report line the run writes.
* `MissionTerminalSource` — `Objectives` (the F39 session's own
  `SessionTick::outcome`) or `ControlProgram` (the declared control program's
  own terminal state).
* `MissionExit` — `code()` is `0` **only** for `TerminalOutcome::Success` and
  `1` for `Extraction` and `Failure`; `app_exit()` is the same mapping as the
  Bevy message.

## Two sources, one funnel, and why the order is what it is

`MissionHost::settle` reads the F39 `SessionTick::outcome` **first** and the
control program's `MissionTick::terminal` second, and the report line says
which one settled. The order is not arbitrary and is not a precedence rule:

1. It is the order `MissionHost::step` produces the answers in (stage 3 is
   animations/markers/objectives, stage 4 is the script host), so within one
   tick the F39 outcome is observed first because it was produced first.
2. Across ticks, whichever source settles on the earlier tick settles the run,
   because `settle` is called on the very step that produces the terminal
   answer.
3. It is the source this whole feature is named after: for an original mission
   the F39 declared conditions, timers and count reactions are the measured
   semantics that decide an ending, so a run that both settled would name
   `Objectives`.

This is **not** the designed terminal precedence. That
(`TerminalPrecedence::SyntheticConservative`) is reserved by
`docs/contracts/SCRIPT-MISSION.md` for synthetic tests, and the host still
never fills `TickInput::terminal_requests`: the funnel only *reads* what a
record already settled.

## Measured facts relied on, re-checked 2026-10-10

| # | Fact | Where |
| --- | --- | --- |
| 1 | `ObjectiveSession::outcome()` is the runtime's own settled outcome, and `SessionTick.tick.events` carries the `OutcomeSettled` event whose `EventKey::source` is the declaration that asked (`resolve_outcome` pushes it with `requested.get(&outcome)` — the requesting timer's or objective's symbol). | `crates/cs_app/src/objectives.rs:1358`, `crates/cs_sim/src/objectives/runtime.rs:1757-1790` |
| 2 | `MissionTick.terminal` is `TerminalState`; `EventKind::TerminalRequested(Outcome)` carries `event.key.source`. `Outcome` is `Succeeded`/`Failed`/`Aborted`; `TerminalState` adds `Running` and `Unsupported` (the latter reachable only through a launch refusal, never from evaluation). | `crates/cs_script/src/runtime.rs:202`, `:1015`, `crates/cs_sim/src/mission.rs:2150` |
| 3 | `Action::Finish(Outcome)` is the mission IR's one terminal action and validates (`ir.rs`'s action check returns `Ok(())` for it); M01's measured `INSTANTWIN`/`INSTANTLOSS` lowering registers exactly it. | `crates/cs_script/src/ir.rs:819`, `:1277`, `crates/cs_app/src/control_lowering.rs:560-565` |
| 4 | `ObjectiveSession::drain_cues()` hands over and removes every pending cue; `pending_cues()` counts them. Cues are enqueued only from `ObjectiveEventKind::CueEmitted`, which a declared `TimerAction::Cue` produces. | `crates/cs_app/src/objectives.rs:1383`, `:1392`, `:1472`; `crates/cs_sim/src/objectives/runtime.rs:2045` |
| 5 | `AudioSession` is a public field-bearing resource with `drain()`, `drain_radio()` and a public `router`; `AudioRouter::active_loop_count()` counts the bound loops. Nothing else in `audio/` needed to change — no owner path outside `mission_session/` was touched. | `crates/cs_app/src/audio/loops.rs:131`, `:161`, `crates/cs_sim/src/audio_events.rs:688` |
| 6 | The mission composition inserts **no** `AudioSession`: `add_composition` never calls `audio::insert_audio_session`, and the only constructor site is that function. So the terminal's `audio_loops_bound` is 0 today and `MissionHostRefusal::MissionAudioStillBound` is unreachable until a composition starts a mission-bound emitter. | `crates/cs_app/src/audio/handoff.rs:202`, `crates/cs_app/src/mission_session/compose.rs:550` |
| 7 | `AppExit::Error` carries a `NonZero<u8>`; `App::default()` registers `Messages<AppExit>`, and `World::write_message` is the production way to raise it. `Messages` double-buffers, so a test that wants to prove a *later* frame wrote no second exit must keep one `MessageCursor` across the frames — a fresh cursor reads nothing two updates later. | `bevy_app-0.19.1/src/app.rs:1560`, `:130`, `bevy_ecs-0.19.1/src/message/messages.rs:95` |
| 8 | `MissionState::terminal()` is the control program's live terminal state (`Running` while it runs). | `crates/cs_script/src/runtime.rs:1554` |

## The exit mapping, stated exactly

| Record that settled | `TerminalOutcome` | `MissionExit::code()` | `AppExit` |
| --- | --- | --- | --- |
| control program `Succeeded`, or F39 `Success` | `Success` | 0 | `AppExit::Success` |
| control program `Failed` | `Failure` | 1 | `AppExit::Error(1)` |
| control program `Aborted` | `Failure` | 1 | `AppExit::Error(1)` |
| control program `Unsupported` | `Failure` | 1 | `AppExit::Error(1)` |
| F39 `Extraction` | `Extraction` | 1 | `AppExit::Error(1)` |
| F39 `Failure` | `Failure` | 1 | `AppExit::Error(1)` |

`Aborted` is not a mission outcome this engine reaches from a record, so it is
mapped to failure rather than being read as the success it was not: an exit
code is never a swallowed failure (`docs/contracts/CLI-EVIDENCE.md`, "Never
return zero after only logging a failure"). `Unsupported` cannot arrive from
evaluation — it is a launch-refusal state — and is mapped to failure for the
same reason. `Running` never reaches the mapping: the funnel returns `None`
for it.

## The terminal sequence, and where each step lives

1. **Stop stepping.** `MissionHost::step` returns
   `MissionHostStepError::Settled` once `settle` has run, and
   `mission_host_tick` returns early when the host already carries a terminal
   (before it even looks at the tick). A settled run therefore advances
   nothing: the host tick stands still, `MissionHostReport` keeps standing for
   the last stepped tick, and no second `AppExit` is written.
2. **Drain the objective session's cue queue.** The count on the terminal
   (`undrained_cues`) is what the session **still owned** when the run ended,
   so cues the player will never hear are named rather than dropped. This is
   the same report the `TeardownReport` makes on a retry, on the path that
   does not relaunch anything.
3. **Resolve the audio session's pending queues** (`AudioSession::drain` and
   `drain_radio`), then count what is still bound. A non-zero count is recorded
   as `MissionHostRefusal::MissionAudioStillBound { count }`, because
   `cs_sim::audio_events::EmitterStopReason` measures no mission-end reason and
   the loops are reported rather than stopped under a reason they did not have.
4. **Write the one report line**, e.g.

   ```text
   mission terminal: outcome=success source=control_program tick=1 session=3 requested_by=SymbolId(0) undrained_cues=0 audio_loops_bound=0 exit_code=0
   ```

   It is produced by `MissionTerminal::new`, printed once by `settle` and
   carried on the terminal (`report_line`), which is what the tests read.

The `AppExit` message is written by `mission_host_tick` immediately after
`settle`, from `MissionTerminal::exit` — the one place a mission outcome
becomes a process exit.

## M01 still cannot reach a terminal headlessly

Nothing here changes #1278's fact 7. M01's 58-block program keeps both `Finish`
blocks (23 `Succeeded`, 39 `Failed`) dormant behind the `-1` sentinel and every
wake path needs world facts a headless run does not have, so the retail member
`accept_vs_m01_runtime_host_02_retail_m01_settles_no_terminal_and_writes_no_exit`
holds the composed entry to **no** terminal, **no** `AppExit` and a control
program that is still `Running` at the last composed tick. Terminal behaviour
is therefore proven on synthetic stages whose declared programs really do
settle, exactly as the task prescribes; the retail member proves M01's real
program runs through the same entry.

## What is **not** claimed

* No original executable ran; `retail` is read access to the owner's
  installation. Nothing here is `verified_original`.
* The synthetic stages' `Finish` actions are authored content with
  `Origin::SyntheticFixture` and designed provenance — they say nothing about
  M01's ending, only that the funnel and the exit mapping are the ones that
  carry whatever a real record settles.
* No `ObjectiveSpec`, `CountCondition`, `MissionTimer`, `SweptTrigger` or
  spawn group is constructed for an original mission (AGENTS.md rules 4 and 5).
* A **restart** after a terminal is still open (#1217's `.03`): nothing is
  unloaded or reloaded here, so a run that ends leaves the world resident
  until `teardown` runs.
* `run_windowed`'s `teardown(&mut app)` after `app.run()` is still a no-op:
  `App::run` moves the `App` into the runner and leaves `App::empty()` behind,
  so nothing can be read from the app afterwards (the runner drops the `App`).
  Measured and re-confirmed; noted, deliberately not fixed here — the line is
  `.03`'s to change together with the restart it belongs to.

## Sensitivity: the tests fail without the terminal path

With `MissionHost::settle` stubbed to `return None` (the terminal path
removed), all three synthetic members fail with "the composed entry never
settled the run". The members assert on the `AppExit` message the composed
entry writes and on the report line the terminal carries — both produced by
that path and by nothing else, never on a flag the test set itself.

## Checks

| check | result |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 — 496 `test result` lines, 0 failed |
| `cargo test --workspace --locked -- accept_vs_m01_runtime_host_ --include-ignored` | 0 — 7 passed in 623.6 s: the 3 new synthetic members, the new retail member, and the three `.01` members (the selection is a superset of the required `accept_vs_m01_runtime_host_02_`, which is 4 tests, all passing) |

Sources: `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering"),
`docs/contracts/IDENTITY-CONTENT.md`, `docs/contracts/CLI-EVIDENCE.md`,
`docs/findings/2026-10-10-vs-m01-rt-window-composition.md`,
`docs/findings/2026-10-10-vs-m01-rt-content-objectives-program-refuses-and-plan-premises.md`,
and the code cited in the table above.
