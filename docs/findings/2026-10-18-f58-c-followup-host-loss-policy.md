# F58-C follow-up: the match-level host-loss flow follows the mode's HostLoss policy

Date: 2026-10-18. Task #1238. Test prefix: `accept_f58_c_followup_host_loss_policy_`.
Capabilities used: ordinary build/test only.

## Decision

Option (a) of the task. `RecoveryFlow` now carries the mode's `cs_net::lobby::HostLoss`
(`with_host_loss_policy`; default `EndSession`, so every existing caller keeps the
"ends once" behavior of spec F58 non-negotiable 3):

- `EndSession`: unchanged. The first `host_lost` ends the match, aborts in-flight
  interactions once and answers `ReturnToMenu`; later reports are duplicates; no resume.
- `PauseThenEnd { ticks }`: the first `host_lost` at tick `t` answers
  `PauseMatch { until: t + ticks }` and ends nothing; the flow stays resumable and
  `recover` still decides on its merits. `host_returned(tick)` lifts the pause while
  `tick < until`. At `tick >= until`, `poll_host_loss` (or another `host_lost`) ends
  the match exactly as `EndSession` does, and the end is irrevocable: `host_returned`
  answers `TooLate` and `recover` is refused with `MatchEnded`. A zero grace ends at once.

This matches `Lobby::remove_peer`, which already pauses the lobby for the grace period.

## Unknowns (not guessed)

- No original grace period, or any original host-loss behavior, has been measured. The
  number is the mode's own `ticks`; the semantics above are newly authored engine design.
- `Lobby` has no host-return transition yet: `Phase::HostPaused` is left only by the
  session closing. The match-level flow can resume, the lobby cannot, so a runtime that
  wires both still needs a lobby resume (and a timer that closes a lapsed `HostPaused`
  lobby). Neither component is wired to a runtime (#592).
