# F33-D: Neutral-traffic population and the original installation's carried traffic

Date: 2026-10-03. Task: F33-D "Verify original roster, relation changes and
allied behavior" (`specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
section `### F33-D`). Shared contracts: `docs/contracts/STATE-TRANSACTIONS.md`
(the session-generation discipline) and `docs/contracts/CLI-EVIDENCE.md` (the
evidence report). Capabilities used: `retail` (read-only access to
`$CS_GAME_DIR`) and ordinary build/test (unignored synthetic tests). No
`gpu`, `audio`, `human_play` or `human_review` was used or is claimed.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/roster.rs`: the F33-D **runtime population seam** —
  `NeutralSpawn`, `PopulationError`, `NeutralPopulation` and
  `build_neutral_population`. The population is built from
  `LoweredRoster.neutral_traffic` and nothing else; `spawn`/`actor` answer an
  unauthored traffic index with `None`, and `register_into` registers each
  authored neutral under `AllyRole::Neutral(index)` with no voice.
- `crates/cs_app/src/roster.rs`: the F33-D **retail census** —
  `OBSERVED_PLACED_TRAFFIC_MEMBER`, `RetailMissionTraffic`,
  `RetailNeutralTrafficCensus`, `TrafficCensusError`, `scan_archive` and
  `survey_retail_neutral_traffic`, which reads the owner's installation through
  the production `cs_assets::install::discover` and the production
  `cs_formats::script_raw::discover_container`.
- `crates/cs_app/tests/accept_f33_d_neutral_traffic_population.rs` (new): the
  cross-crate AC04 scenario, the authored-only population failures, and the
  production census over authored synthetic installation trees (unignored, so
  CI exercises them) plus the `#[ignore]`d retail measurement.
- In-source `#[cfg(test)]` tests in `crates/cs_app/src/roster.rs`, named
  `accept_f33_d_*` and selected by the same filter.
- `crates/cs_app/tests/evidence_report_f33_d.rs` (new): the evidence harness
  (not part of the acceptance suite; named without the prefix).
- `docs/findings/evidence/F33-D.json`: the committed report copy.
- This file.

**One observable failure:** the F33-C finding recorded (lines 94–97) that
"no F33 spawn system places neutral actors in the world yet (AC04's runtime
half is F33-D)". Before this stage there was no runtime population at all, so
a mission that authored no neutral traffic could be handed a world populated
from a fixed table: the lowered `neutral_traffic` list went nowhere. The
population seam makes the global-population system **unreachable by
construction** — the only constructor consumes the authored lowered list, and
the count of actor serials must match the authored count exactly — and
`accept_f33_d_omitted_neutral_traffic_spawns_nothing` (unit) and
`accept_f33_d_population_spawns_only_authored_neutral_traffic` (integration)
fail if an omitted mission yields any spawn, if an unauthored index resolves,
or if `register_into` registers an actor for an empty authored list.

## Semantics defined at this stage

- **Authored-only, by construction.** `build_neutral_population` takes the
  lowered authored list and one one-based actor serial per authored neutral.
  A serial-count mismatch (`SerialCount`), a repeated authored index
  (`DuplicateTraffic`), a zero serial (`ZeroSerial`) and a zero session
  (`ZeroSession`) all refuse. There is no fallback table, no default actor and
  no way to ask for a traffic index the mission did not author.
- **Omitted means empty.** An omitted declared list already lowers to an empty
  list (F33-A, `accept_f33_a_omitted_neutral_traffic_lowers_to_nothing`); F33-D
  is the runtime half: the empty list builds an empty population and registers
  nothing.
- **Role, not paint.** An authored neutral registers under
  `AllyRole::Neutral(index)`, the F33-C role that classifies a protected-neutral
  loss as a mission event rather than a kill. Faction and geometry are recorded
  separately, so a later capture changes only the faction.
- **No invented voice.** The neutral record carries `voice: None`. A neutral
  whose mission authored no voice speaks through none, never a random line
  (F33 non-negotiable 5); `accept_f33_d_authored_neutral_binds_its_actor_and_role`
  asserts this.
- **The census refuses, it does not shorten.** An installation that cannot be
  discovered, an archive the installation declares that cannot be read, and a
  spelling that is not a usable relative path each return a named
  `TrafficCensusError`. A failed read never reads as "the mission authored no
  traffic" (`accept_f33_d_census_refuses_an_undiscoverable_installation`).

## The retail observation (measured, read-only)

`survey_retail_neutral_traffic($CS_GAME_DIR)` was run through the production
discovery. The installation fingerprints of the observation are
`install_sha256 =
b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
and
`content_sha256 =
a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`.

Measured, over the **53** directories `ZBD/<group>/<mission>`:

- **50** mission archives carry the member `zeppelins.zrd` in their own
  `ZBD/<group>/<mission>/zrdr.zbd`;
- **3** do **not**: `c1/m02`, `c2/m01` and `c5/mp2`. These are the authored
  omissions AC04 protects.
- **No** installation-scope archive (`ZBD/zrdr.zbd` or
  `ZBD/<group>/zrdr.zbd`) carries the member: `installation_scope_carrier` is
  `false`. A session population driven by an installation-scope archive would
  be exactly the "global population system" AC04 forbids; the census checks for
  it and reports its absence here.

Each row carries its archive key, its SHA-256 and its reader-archive member
count, so every number can be traced back to the bytes. The unignored
`accept_f33_d_census_reads_mission_scoped_carriers` and
`accept_f33_d_census_detects_an_installation_scope_carrier` prove the census
mechanics on authored synthetic installation trees in CI.

## Unknowns recorded (not guessed)

- **`zeppelins.zrd` is a name rule, not a decode.** The member's role and its
  encoding are **not** decoded. The name is inferred from the mission archives
  (it is mission-scoped and absent exactly where a mission likely authors no
  placed traffic), so the census does not show that the original used it for
  neutral traffic, nor how an actor is encoded in it.
- **No mapping from a runtime `ActorId` to any original script/actor id.** The
  census reads container members; no correspondence to a session actor was
  measured, so none was fabricated.
- **The original's runtime behavior is unmeasured.** Whether the original
  spawns any neutral traffic at all, how it treats an omitted author list,
  whether it postpones a death that happened during a cutscene, which loss/voice
  lines it plays, and whether a captured or bailed-out aircraft could still fire
  are unknown: no original executable was run, and reading the installation's
  files is not evidence of how the game behaves. This is not `verified_original`.
- The other 50 missions' carriers are counted but their contents are not
  decoded; a future format task can pick up the encoding.

These follow-ups are filed with `create_tasks` rather than guessed here.

## Test counts

9 `accept_f33_d_*` tests, all passing under
`cargo test --workspace --locked -- accept_f33_d_ --include-ignored`:

- 3 unit tests in `crates/cs_app/src/roster.rs` (omitted → empty; authored
  binding and role; serial/duplicate/zero refusals).
- 6 integration tests in
  `crates/cs_app/tests/accept_f33_d_neutral_traffic_population.rs` (AC04
  end to end, serial-count refusal, mission-scoped carriers, installation-scope
  carrier detection, undiscoverable installation, and the `#[ignore]`d retail
  measurement of the 53/3 corpus).

## Sensitivity probes (run and reverted; no probe committed)

1. `build_neutral_population` was made to push a default neutral spawn for an
   omitted authored list (`if spawns.is_empty() { ... }`):
   `cargo test -p cs_app --locked --lib -- accept_f33_d_` failed
   `accept_f33_d_omitted_neutral_traffic_spawns_nothing` at
   `assert!(population.is_empty())` (2 passed, 1 failed) — the omitted mission
   would have been populated by a global default.
2. The installation-scope carrier detection was disabled (the scope archives
   were read but `installation_scope_carrier` was never set):
   `cargo test -p cs_app --locked --test
   accept_f33_d_neutral_traffic_population -- accept_f33_d_census_detects_an_installation_scope_carrier
   --exact` failed (0 passed, 1 failed) — the global carrier was invisible.

Both probes were reverted; the committed tree is the green one.

## Not claimed

No original-data verification, no `verified_original`, no runtime ECS spawn
system beyond the population seam, no decode of `zeppelins.zrd`, no audio or
visual evidence. The task awards at most **checked** status.

## Commands run (all exit 0 on the committed tree)

- `cargo fmt --all -- --check` → clean
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  → 0 warnings
- `cargo test --workspace --locked` → all targets pass, 0 failures
- `cargo test --workspace --locked -- accept_f33_d_ --include-ignored`
  → 9 passed, 0 failed (3 in `cs_app`'s lib, 6 in the `cs_app` integration
  target)
- `python3 tools/validate_evidence.py private/evidence/F33-D/acceptance.json
  --artifact-root private/evidence/F33-D --require-pass` → valid
