# F56-B (task #225): mode rule fields completed, objective records decoded, and the authoritative possession machine

Date: 2026-10-08. Task: #225 "Implement verified mode state machines and
objective ownership" under stage `### F56-B`
(`specs/F56-original-multiplayer-scenarios-and-mode-rules.md`). Capability:
`retail` (the installation at `$CS_GAME_DIR`). Parent findings:
`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md` and
`docs/findings/2026-10-03-f56-a-slot-mode-bindings.md`.

## Files and the observable failure

- `crates/cs_sim/src/multiplayer/objective.rs` (new): `ObjectiveBoard`, the
  server-authoritative possession state machine. States `Home` / `Held` /
  `Dropped`; actions `Claim` / `Drop` / `Return` / `Score`; every event is
  identified by its `EventId`, queued on submission and adjudicated in id
  order (tick, producer, sequence) when its tick closes.
- `crates/cs_sim/src/multiplayer/result.rs`: `VictoryRule`, the resolver's
  declared victory condition (`MatchResolver::with_victory`, `victory()`;
  `seal` matches on it), and `Roster::contains` for participant checks.
- `crates/cs_net/src/rules.rs`: `RuleField::Spawn` / `RuleField::Victory` with
  the `Spawn` / `Victory` vocabularies, `ResolverLimits` — the one bridge from
  a resolved `MatchRules` onto the resolver's `Option<Tick>`/`Option<i32>`
  limits — and `RulesError::LimitTooLarge`, which refuses a score limit the
  resolver cannot hold.
- `crates/cs_content/src/multiplayer.rs`: `RULE_LABELS` gains `spawn` and
  `victory`; `ScenarioSlot.objectives` exposes the decoded `targets.zrd`
  records as `SlotObjective` with the designed `ObjectiveKind`
  classification and `possessable()`.
- Observable failure without the change: two clients can submit a claim for
  the same objective on the same tick and nothing arbitrates — both could
  believe they hold it. With it, the queued events adjudicate in `EventId`
  order, the lower id applies `Possessed`, the later is `Denied(AlreadyHeld)`,
  and the board names exactly one holder (`accept_f56_b_two_clients_claim_-
  one_objective_and_only_the_server_accepted_ownership_holds`).

## What the state machine is and is not

The machine is **engine design**, not measured original behavior. The
original's possession, drop, return, delivery and point rules are unknown (the
catalog finding's open items), so the arbitration rules are documented as the
engine's own and every behavior the spec's non-negotiable 2 requires is what
the board enforces:

- **Singular possession.** An objective is held by at most one participant. A
  claim applies only while it is unheld (`Home` or `Dropped`); a held
  objective denies further claims with the holder's name.
- **Idempotent reliable events.** One `EventId` applies at most once: a
  retransmission of a queued event is `Duplicate`; one arriving after its tick
  closed is `LateEvent`; either way no transition repeats.
- **Session scope.** Events stamped for another session are `WrongSession`.
  A new `ObjectiveBoard` is constructed per session, so a reconnect or a
  recycled `PeerId` cannot inherit state (tested: a fresh board of the next
  session holds `Home` while the old board is untouched).
- **Participant gate.** A claim or drop naming a peer outside the match's
  `Roster` is `UnknownParticipant`; an objective id the board never declared
  is `UnknownObjective`, never created.
- **Deterministic same-tick order.** All of a tick's queued events apply in
  `EventId` order against the single evolving state — so a flag that is
  claimed, scored and re-claimed on one tick resolves the same on every host
  (`accept_f56_b_score_and_return_are_singular_across_interleaved_events`
  documents the order: producer sorts before sequence inside one tick).

## What the retail records added

The F56-A probe work measured the full field inventory of `targets.zrd`
objective records across all 21 slots: keys `description`, `nodes`,
`help_label`, `category_label`, and the bare directives `objective` /
`other_target`. `description` takes exactly five values
(`MSG_TRGT_FLAG`, `MSG_TRGT_FLAGBASE`, `MSG_TRGT_REARM_BASE`,
`MSG_TRGT_ZEP_ENEMY`, `MSG_TRGT_ZEP_FRIEND`); `category_label` is only
`MSG_OBJ_ZEPPELIN`; `help_label` is one of `MSG_OBJ_DEFEND`,
`MSG_OBJ_DESTROY`, `MSG_OBJ_TEAM_1`, `MSG_OBJ_TEAM_2`. `ScenarioSlot.objectives`
now exposes those records, and `ObjectiveKind` classifies the five measured
descriptions; only `Flag` is `possessable`. The retail test pins that the
possessable records occur in exactly the five Capture-the-Flag slots (every
world group's `MP2`) and the enemy-zeppelin records in exactly the Zeppelin
slots — classification and mode binding agree everywhere.

Every slot archive also carries a `net.zrd` member of ~40 four-float records.
Its shape suggests spawn placements, but its semantics are not measured; it
stays undecoded rather than guessed.

## Still unknown (gating F56-C and fidelity claims)

- The original mode's per-field values: spawn, respawn, lives, time/score
  limits, friendly fire, victory/draw condition, disconnect, late join, human
  scaling, component limit — all still `Resolved::Unknown` per mode; `spawn`
  and `victory` now have field *vocabularies* but no measured values.
- The event each printed briefing point value rewards, including what a flag
  delivery is worth.
- Which deathmatch variant (team or not) a `Deathmatch`-family slot launches
  under.
- `net.zrd` semantics (candidate: spawn placement; unmeasured).
- How possession interacts with a lost connection or a death in flight in the
  original (the engine drops on `Drop`; the trigger is a later task).

Evidence: `docs/findings/evidence/F56-B.json` (acceptance report, validated
with `tools/validate_evidence.py --require-pass`).
