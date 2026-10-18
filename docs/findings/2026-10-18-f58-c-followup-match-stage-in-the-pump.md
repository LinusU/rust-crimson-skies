# F58-C follow-up (#1221): the host's MatchStage in the pinned pump

Closes F58-B finding 2 at the pump: `ServerSession` can now be handed the
host's `MatchStage`, and the intent checks are live on that path (unlike the
app-side driver, which still calls the unstaged `pump`).

## What changed

* `ServerSession::pump_staged(elapsed, MatchStage)` is `pump` plus the host's
  phase, its own simulation tick and its banned set. `pump` is unchanged and
  judges no intent (no tick was supplied).
* Each admitted input packet that survives the rate ladder is judged as a
  `ClientIntent::Input` for the aircraft the gate bound to the sender, at the
  packet's newest frame tick (F58-B finding 7), by the new charge-free
  `IntentValidator::judge`. `validate` is now "truth check, charge, `judge`",
  with the same order and results as before.
* A refusal is `ServerNotice::IntentRefused`. Tick-outside-window and wrong
  phase are absorbed (nothing queued, peer kept). A `Disconnect` disposition
  (an aircraft the peer does not own) hangs the peer up with its threat class.
* No new caps or numbers. The rate budget is not charged a second time: the
  ladder from #591 already charges every offered packet against
  `RateLimits::DESIGNED`, so (a) was already live on the wire and is now tested
  together with the stage.

## Checked assumption (from #591)

The rate windows are keyed on the session's pump counter (`rate_tick`), not on
`stage.server_tick`. The window therefore equals 60 *pump calls*. It equals 60
simulated ticks only if the host pumps once per tick. The app-side driver that
will own the pump, `cs_app::network::session::drive_net_session`, runs in Bevy's
`FixedPostUpdate` and calls the **unstaged** `pump` exactly once per committed
fixed tick, so on that path one pump is one tick. But no assembled application
installs `NetSessionDrivePlugin` yet (only tests do), so nothing ships a host
that pumps at all, and the app still calls `pump` rather than `pump_staged`. A
host that pumps more often than it ticks would refill the window faster than
96 per simulated second and weaken the budget. The tick window is not affected,
since it is keyed on the host-supplied `server_tick`.

## Not done

* Loadout asks and `Claim*` intents still have no wire form, so the banned set
  in the stage is carried but not consulted by the pump.
* ~~Nothing in `cs_app` calls `pump_staged` yet.~~ Closed by #1590, see below.
* Values remain engine design, not measured (F58-D, `network_real`).

Tests: `accept_f58_c_pump_stage_*` in `crates/cs_net/tests/accept_f58_c_pump_stage.rs`.

## Closed in the app driver (#1590)

`cs_app::network::session::drive_net_session` now calls `pump_staged` through
`pump_host` whenever a committed tick exists:

* `server_tick` is the committed `PhysicsTickLedger` tick.
* The phase is the host session's own `ServerPhase`, the same one that gates
  snapshot publication: `Gathering` is `Gathering`, `Live` is `InMatch`, and
  `Finished`/`Closed` are `Closed` (input refused). The attached lobby's phase
  is not read: `ServerSession::launch` does not move it, so it can lag the
  session.
* The banned set is the attached lobby's `rules().banned`, empty without a
  lobby. It is still not consulted by the pump (no wire form for loadout asks).
* With no committed tick (already reported as `NetDriveMissing::Timeline`) the
  pump is the unstaged one, since the tick window has no anchor.

Still true: no assembled application installs `NetSessionDrivePlugin`, so the
`cs` binary does not host a net session yet. The pump-per-tick assumption above
holds for this driver. Tests: `accept_f58_c_app_pump_stage_*` in
`crates/cs_app/tests/accept_f58_c_app_pump_stage.rs`. `accept_f54_c_drive_*` now
launches the host and submits input once the committed tick is near the stamped
ticks, because the live window refuses input far from the host's tick.
