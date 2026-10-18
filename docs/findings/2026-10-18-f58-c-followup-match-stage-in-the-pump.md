# F58-C follow-up (#1221): the host's MatchStage in the pinned pump

Closes F58-B finding 2 for the input path.

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
simulated ticks only if the host pumps once per tick. Nothing in `cs_app` pumps
a `ServerSession` yet (no caller outside tests), so this is **unverified**: a
host that pumps more often than it ticks refills the window faster than
96 per simulated second and the budget weakens. The tick window is not affected,
since it is keyed on the host-supplied `server_tick`.

## Not done

* Loadout asks and `Claim*` intents still have no wire form, so the banned set
  in the stage is carried but not consulted by the pump.
* Nothing in `cs_app` calls `pump_staged` yet; no host exists to supply the tick.
* Values remain engine design, not measured (F58-D, `network_real`).

Tests: `accept_f58_c_pump_stage_*` in `crates/cs_net/tests/accept_f58_c_pump_stage.rs`.
