# F30-C-MULTIOBSERVER: one target selection per session (deferred per-observer)

Task #529. Decision date 2026-10-25.

## Decision

A `cs_app::targeting::TargetingSession` keeps **one** `cs_sim::targeting::TargetSelection`
for the whole session. Per-observer selections are **not** implemented.

## Reason

- `docs/00-SCOPE.md` lists split-screen as "not assumed" (not a requirement), and the
  project has a single local player per session. Networked play gives each client its
  own session and its own `TargetingSession`, so no session ever needs two live
  player selections.
- AI-driven aircraft do not use the player's selection: AI target choice lives in the
  mission AI, not in `TargetSelection`.
- Making the session a map keyed by `ActorId` would change every entry point's
  signature (`TargetingSession::selection`, the consumer pass, the HUD/spyglass/weapon
  guidance consumers and about 25 test files) for content that does not exist.
- The existing behaviour is deliberate and tested: a consumer pass for a different
  `(session, observer)` rebinds the published views and clears the selection
  (`ConsumerReport::rebound`), so one aircraft's selection never leaks to another.

## Limitation (gates split-screen / second local player)

- **Affected content:** split-screen, a second local player, or any feature that
  runs consumer passes for two observers in the same session in alternation.
- **Behaviour today:** each pass for the other observer rebinds and clears the
  selection, so the two observers cannot hold selections at the same time.
- **Gate:** no split-screen or second-local-player work may be marked done until the
  per-observer map is implemented. The resolving task is filed as
  `F30-C-MULTIOBSERVER-IMPL` (blocked on an owner decision to add split-screen to scope).
- **Implementation sketch for that task:** replace `selection: TargetSelection` and the
  per-observer view binding by `BTreeMap<ActorId, ObserverTargeting>`; every
  `TargetingSession` accessor and consumer pass takes the observer; acceptance tests use
  the `accept_f30_c_multiobserver_` prefix and prove two observers' selections are
  independent.
