# PARITY-M01-OUTCOME-B (#1654): player destruction settles a failure

Implementer: claude-2 (Claude Sonnet 5.5). Nothing here is `verified_original`.

* The composed entry's `MissionHost::settle` has a third source,
  `MissionTerminalSource::PlayerDestroyed`: when neither the objective session
  nor the control program has settled, the shared damage resolver recording
  `Destroyed` for the player's actor settles `Failure` (exit code 1, one
  `AppExit`, no further ticks). The mission's own win/loss logic keeps
  precedence on the same tick.
* The terminal carries `MissionDebrief`: Run Time (`m:ss`, host ticks over the
  session rate; where the original's clock starts is **unmeasured**), Gun Hit
  Ratio (`hits/shots` of the player's guns, counted by `MissionTally`),
  Overall Planes Downed (ledger score credited to the player over the
  per-kill value) and Cash Earned.
* **Unknown:** Cash Earned (`m01-debrief.cash-earned-unmeasured`). No cash
  rule or award table is measured; it is reported as unknown, not zero. The
  kill value behind Planes Downed is the designed one (`f29-c3.designed-kill-score`).
* Not done here: ground/sea impact still does not destroy the player (no
  collision wiring), and the debrief screen is #1655.
