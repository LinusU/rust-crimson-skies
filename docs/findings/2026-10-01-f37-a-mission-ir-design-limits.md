# F37-A: mission IR design limits

The F37-A IR (`cs_script::ir`, `cs_script::runtime`, `cs_sim::mission`) is a new
engine design. Nothing in it is measured from the original game.

> **Superseded on two of these unknowns, 2026-10-07 (F37-D-FU2, `#589`)**: the
> owner's static code analysis of the decrypted executable (owner note on
> `#589`, 2026-10-05) settled *terminal precedence* and the original's
> *in-tick execution order*. The runtime now selects
> `PrecedencePolicy::MeasuredOriginal` by default (the recorded result is
> success iff the WON flag is set), so the sentence below saying what "the
> runtime uses" is historical: `SyntheticConservative` stays selectable for
> synthetic tests only. The original's *event observation* key is a separate
> question — the original emits no event stream — so the recreated `EventKey`
> order is labelled `designed-and-unmeasured`, not measured. The measured
> rules, their addresses and the `f37.d.limit.*` limitations are recorded in
> `docs/findings/2026-10-07-f37-d-fu2-mission-terminal-precedence-and-tick-ordering.md`.
> Everything else here stands: the two unknowns not named above are still
> unmeasured.

Unknowns, none resolved by this task:

- **Terminal precedence for simultaneous success/failure** is unmeasured. The
  runtime uses `PrecedencePolicy::SyntheticConservative` (Aborted > Failed >
  Succeeded), valid for synthetic tests only. Affects every original mission
  with conflicting end conditions. Resolves with F37-D reference ordering probes.
- **Effect of a protected actor dying after a success latch** is unmeasured; the
  runtime latches the first terminal state and ignores later ticks.
- **Original value semantics** (integer width, overflow, float/compat numeric
  type) are unknown; the IR uses checked `i32` and finite `f64` as a design
  choice. Resolves with F13/F38 once the original language is decoded.
- **Event ordering inside one tick** uses (session, tick, objective symbol id,
  sequence). Whether the original orders by declaration or by another key is
  unmeasured (F37-D).

Not in this slice (later stages): bounded work budget (F37-B), timers and
snapshot/restore (F37-C), host effect application (F37-C), adversarial corpus
(F37-D).
