# F56-C (task #226): maps, host options, scoring and the end-of-match record wired into one match owner

Date: 2026-10-08. Task: #226 "Wire maps, host options, scoring and end-of-match
UI" under stage `### F56-C`
(`specs/F56-original-multiplayer-scenarios-and-mode-rules.md`). Capability:
`retail` (the installation at `$CS_GAME_DIR`). Parent findings:
`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`,
`docs/findings/2026-10-03-f56-a-slot-mode-bindings.md` and
`docs/findings/2026-10-08-f56-b-objective-ownership.md`. Contract:
`docs/contracts/UI-NETWORK.md`.

## Files and the observable failure

- `crates/cs_sim/src/multiplayer/session.rs` (new): `MatchSession`, the one
  owner of a running match — it holds the scoring resolver, the possession
  board and the match clock together, starts from one plain `SessionConfig`
  (map id, session generation, roster, score table, limits, victory rule,
  objective count), and renders the sealed result into `EndOfMatch`, the
  record a results screen consumes (map, session, end reason, outcome,
  standings, every recorded delivery, and the stable keys
  `match.end.*` / `match.outcome.*` in the shape
  `cs_net::lobby::RevokeReason::message_key` already uses).
- `crates/cs_net/src/rules.rs`: `RuleDraft::from_lobby` (a host's team
  grouping and late-join policy become rule fields instead of being
  re-derived elsewhere), `LaunchPlan` (the selected scenario bound to the
  rules that resolved, refusing a pairing that contradicts itself), and
  `Victory::label` / `Victory::ALL` — the wire vocabulary that carries the
  victory condition across the `cs_net` ↔ `cs_sim` boundary.
- `crates/cs_sim/src/multiplayer/result.rs`: `VictoryRule::label`,
  `VictoryRule::from_label`, `VictoryRule::ALL` — the resolver's half of that
  vocabulary pair; an unknown label is refused rather than coerced onto
  `HighestScore`.
- `crates/cs_content/src/multiplayer.rs`: `SlotCatalog::get` and
  `ModeCatalog::get` (a host's selected content id resolves to *its* entry,
  never to the first one) and `ScenarioSlot::possessable_objectives`
  (`Some(n)` when the slot's records decode, `None` while they do not, so a
  launch is refused instead of declaring a guessed number of objectives).
- Observable failure without the change: no type owned a match. A host that
  had resolved `MatchRules` had no production path onto the resolver and the
  board, nothing proved a restarted match started from nothing (a restart
  could carry a previous score, a held pickup or closed ticks), and no
  end-of-match record existed for a results screen to show. With it,
  `accept_f56_c_restart_leaves_no_score_pickup_or_timer_leak` plays a match to
  its result, restarts it, and the new generation has zero scores, home
  objectives with empty ledgers, no clock, no stale `EndOfMatch`, and the
  finished generation's packets are refused as `WrongSession`.

## What was wired, and where each piece stops

The three crates may not depend on each other (`docs/01-ARCHITECTURE.md`), so
the wiring is a chain of plain `cs_types`-typed records with one conversion
each — the same discipline as F56-B's `ResolverLimits`:

1. **Map.** `LobbyRules::scenario` (a `ContentId`) → `SlotCatalog::get` for
   the slot and `ScenarioSlot::possessable_objectives` for the objective
   count → `SessionConfig::scenario` / `objectives` → `EndOfMatch::scenario`.
2. **Host options.** `LobbyRules` → `RuleDraft::from_lobby` fills
   `teams`/`late_join` → `resolve()` (still `Blocked` for every field the
   installation does not answer) → `LaunchPlan::new(&lobby, rules)` binds the
   map to those rules and refuses a contradiction by name
   (`StartError::TeamModeMismatch`, `StartError::LateJoinNotAllowed`).
3. **Scoring.** `LaunchPlan::resolver_limits()` (the single limits
   conversion) and `rules.victory().label()` → `VictoryRule::from_label` →
   `SessionConfig::limits`/`victory` → the resolver that seals exactly one
   `FinalResult`.
4. **End-of-match UI.** `MatchSession::end_of_match()` → `EndOfMatch` with
   `reason_key()` / `outcome_key()` / `winner()`; the record is scoped to the
   session that produced it, so after a restart it is `None` again until the
   new generation ends.

Teardown/retry: `MatchSession::restart` builds the next match *before*
dropping the current one, so a refused configuration changes nothing and the
caller can correct and retry (`SessionError::SameGeneration`,
`TooManyObjectives`, `Config`). Once a result is sealed, further submissions
are `SessionError::MatchOver` on both consumers and later `close_tick` calls
keep returning the same finished match.

Documented order at a tick close: possession adjudicates first, then scoring,
then the limits once; a tick past the time limit is judged *as* the limit
tick, so nothing (a lethal event or a pickup) is applied after it.

## What this is not

- The session's vocabulary, the restart discipline and the localization keys
  are **engine design**, not measured original behavior: how the original
  restarted a match, what its end-of-match screen showed and what a flag
  delivery was worth are unknown (F56-A/F56-B findings).
- No rule value was invented. Every original per-mode value is still
  `Resolved::Unknown`, so a production launch stays `Blocked` naming the
  unknown fields — the acceptance tests supply authored values explicitly
  labeled as test inputs, the same way F56-A/F56-B do.
- Objective deliveries are recorded on the board's ledger and reported in
  `EndOfMatch::captures`; they are **not** added to the resolver's score,
  because the points a capture is worth are unknown.
- The two consumer crates' call sites are not wired: `cs_net::lobby` never
  calls `rules`, and `cs_app` has no multiplayer results screen. Both live
  outside this task's owner paths and are filed as follow-up **#799
  (F56-C2)**.

## Still unknown (gating F56-D and fidelity claims)

- Every per-mode rule value (spawn, respawn, lives, time/score limits,
  friendly fire, victory/draw, disconnect, late join, human scaling,
  component limit), the score table's points, and which deathmatch variant a
  `Deathmatch`-family slot launches under.
- What a flag delivery scores, how possession interacts with a lost
  connection or a death in flight, and the original restart/end-of-match
  presentation.
- `net.zrd` semantics (candidate: spawn placement; unmeasured).
- Real-client integration over the new protocol: **F56-D** (`network_real`,
  `retail`), which this stage does not claim.

Evidence: `docs/findings/evidence/F56-C.json` (acceptance report, validated
with `tools/validate_evidence.py --require-pass`).
