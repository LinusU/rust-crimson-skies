# F50-C: per-mission probe routes and the human playtest route document

Date: 2026-10-08. Task: F50-C "Build per-mission automated probes and human
playtest routes" (Rally #206,
`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`, section
`### F50-C`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`; evidence
contract: `docs/contracts/CLI-EVIDENCE.md`. Capabilities used: `retail`
(read-only `$CS_GAME_DIR`, never written) and `synthetic` (authored values,
nothing executed). Implementer: **bunny-alpha-1** (Rally #206 implement claim
of 2026-10-08T16:08Z). Review is assigned by Rally after this hand-over; this
document records the implementer's side only.

## What the stage is, in one paragraph

F50-B bound the campaign's identities but planned no route: nothing said how
a mission is re-entered after it ends. F50-C adds the retry contract of spec
F50 acceptance test AC03 — "retry selected missions after death, bailout,
skip-media, save/restart and settings changes" — as production vocabulary in
`cs_content::campaign_bindings`: `probe_routes` plans **one probe route per
declared work order** of the bound campaign, where a ready route states that
whichever of the five interruptions (`ProbeInterruption::ALL`, exactly AC03's
scenario) ends a run, the next entry re-enters the same mission, world and
program under the one installation fingerprint the campaign was read under.
A work order whose identity did not resolve gets a **refused** route — named,
counted, in its declared position, never dropped (spec F50 non-negotiable
behavior 5). `missions/bindings/playtest-routes.md` is the human half: the
same plan as the table a playtester follows, pinned to production by an
acceptance test so it cannot drift. What no part of this claims: that any
mission has been played. Executing a route needs the mission launch path
(`VS-M01-RUNTIME`) and the controlled runs (`VS-M01-CONTROLLED-RUNS`);
ordinary-play evidence is F50-D's.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/campaign_bindings.rs` (production, owner path):
  `ProbeInterruption`, `ProbeReentry`, `MissionProbeRoute`, `ProbePlan`,
  `probe_routes`, `plan_route`, plus the module-doc section "Per-mission
  probe routes (F50-C)".
- `crates/cs_app/tests/campaign/f50_c.rs` (new): the nine `accept_f50_c_*`
  tests.
- `crates/cs_app/tests/campaign/f50_b.rs` (owner path): its retail and
  synthetic fixtures (`SYNTHETIC_INSTALL`, `game_dir`, `context`, `bound`,
  `synthetic_source`, `synthetic_inventory`) widened from private to
  `pub(crate)` so F50-C plans over the very campaign F50-B binds — one
  installation read for the whole suite. No assertion changed.
- `crates/cs_app/tests/campaign/main.rs` (wiring): `mod f50_c;` and a doc
  paragraph.
- `crates/cs_app/tests/campaign/evidence.rs` (owner path):
  `RETAIL_TESTS_F50_C`, `SYNTHETIC_TESTS_F50_C`,
  `evidence_report_f50_c_writes_the_acceptance_report`, its two helpers, and
  the module doc's list of stages.
- `missions/bindings/playtest-routes.md` (new, owner path): the human route
  document.
- `missions/bindings/README.md` (owner path): a section describing what the
  probe plan produces and does not claim, and a closing paragraph that no
  longer sent F50-C the work this stage does.
- `docs/findings/missions/` (this file) and
  `docs/findings/evidence/F50-C.json` (the committed copy of the acceptance
  report).

No wiring outside the owner paths was needed: `cs_app` already depended on
`cs_content` (F50-A), so no `Cargo.toml` or `Cargo.lock` change.

**One observable failure.** If `probe_routes` filtered its plan down to the
routes whose identities resolved — the exact "filtering to the working
subset" spec F50 non-negotiable behavior 5 forbids —
`accept_f50_c_the_whole_campaign_has_one_probe_route_per_declared_work_order`
fails with `assertion failed: plan.len() == inventory.len(): one route per
declared work order, none dropped and none added` (17 routes over a
24-work-order denominator), the synthetic refused-route test fails on the
same counts, and the pinned route document fails because its refused rows no
longer match the plan. That is the failure the stage exists to prevent: a
mission that disappears rather than staying red. (Measured as mutation 1
below, then reverted.)

## What was added

```rust
pub enum ProbeInterruption { Death, Bailout, SkipMedia, SaveRestart, SettingsChange }
impl ProbeInterruption {
    pub const ALL: [ProbeInterruption; 5]; // exactly AC03's minimum scenario
    pub const fn label(self) -> &'static str;      // DEATH … SETTINGS_CHANGE
    pub const fn scenario(self) -> &'static str;
}

pub struct ProbeReentry { pub interruption, pub mission, pub world, pub program, pub install_sha256 }
pub struct MissionProbeRoute { pub label, pub discovery_title, pub campaign_position,
                               pub install_sha256, pub reentries, pub refusal }
pub struct ProbePlan { /* routes in declared denominator order */ }

impl MissionProbeRoute { pub fn is_ready(&self) -> bool;
                         pub fn reentry(&self, ProbeInterruption) -> Option<&ProbeReentry>;
                         pub fn mission_id(&self) -> Option<&ContentId>; }
impl ProbePlan { pub fn routes/len/is_empty/route/ready/refused/ready_count/refused_count
                     /install_sha256 }

pub fn probe_routes(campaign: &BoundCampaign) -> Result<ProbePlan, SourceBindingError>;
```

`probe_routes` is the consumer-side rule over F50-B's producer
(`SourceContext::bind_campaign` → `BoundCampaign`). It reads the frozen
denominator first and refuses — naming the offending work order — every input
the route contract could not stand on:

| Refusal | Reason |
| --- | --- |
| a source binding for a work order the denominator does not declare | the plan would grow it without a `declare` call |
| two source bindings for one work order | a route is planned once |
| a declared work order with no source binding | its route would silently vanish |
| a fingerprint that is not canonical lowercase hex | the save/restart anchor would be unreadable |
| sources read under two different fingerprints | a restart could not re-enter "the same game" for one campaign |

Per work order: all three identities resolved → a **ready** route with five
identical reentries (mission, world, program, fingerprint); any identity
unresolved → a **refused** route naming the missing identities, still
counted, still in position. There is deliberately no partial state: a route
is ready or refused.

## What was measured on `$CS_GAME_DIR`

`install_sha256`
`c14a876f4457d8710dee7986333ab636122c9549cf72b646fd69cbe7e72c5352`,
`content_sha256`
`148a24b7b0506812e8f1ee13d8d3137a05926abebbe10161994e8c4cd300c35e` —
the same installation F50-B and the `M01-A` … `M24-A` stages read. Derived by
`SourceContext::read` + `bind_campaign` + `probe_routes` over the committed
`missions/bindings/campaign-inventory.tsv`, recorded verbatim in
`private/evidence/F50-C/probe-routes.json`.

| Fact | Value |
| --- | --- |
| declared work orders / planned routes | 24 / 24, in denominator order |
| ready routes / refused routes | 17 / 7 |
| reentries planned (ready × 5 interruptions) | 85, every one on its work order's own identity |
| installation fingerprints across the plan | exactly one, equal to production discovery |
| refused work orders | M09, M11, M14, M15, M20, M22, M23 (no mission_id, world_group_variant or program_source_map located — the seven `Uncarried` titles) |
| `CoverageReport::is_ready()` (unchanged from F50-B) | **false** |

The seven refused work orders are exactly F50-B's seven unresolved
identities: this stage neither resolves nor hides them. Which retail mission
each names is still open
(`docs/findings/2026-10-01-m05-a-source-binding.md`).

## The human route document

`missions/bindings/playtest-routes.md` carries one table row per work order
(label, campaign position, mission/world/program ids or `—`, state) and the
seven refusals verbatim, plus what a human playtester does with a ready row:
enter the mission, exercise each of the five interruptions once, confirm the
re-entry lands back in the same mission, record it against the work order.
`accept_f50_c_the_playtest_route_document_lists_every_work_order_as_planned`
parses the committed document and holds every row to the plan, so the
document cannot drift from production without a failing test.

## Test inventory (9 tests, prefix `accept_f50_c_`)

Six synthetic (unignored, so CI runs these) — the planning rule's tests:

| Test | What it pins |
| --- | --- |
| `accept_f50_c_the_minimum_scenario_is_exactly_the_five_ac03_interruptions` | the closed scenario set and its five stable labels, in spec order |
| `accept_f50_c_an_unresolved_identity_is_a_refused_route_that_stays_in_the_plan` | 3 routes over a 3-work-order denominator, the unresolved one refused (named, no reentries, `is_ready() == false`), the ready ones planning all five interruptions with their own identities and anchor |
| `accept_f50_c_a_campaign_read_under_two_fingerprints_is_refused` | the two-fingerprints refusal names both conflicting work orders |
| `accept_f50_c_a_declared_work_order_with_no_source_is_refused` | the missing-source refusal names the declared work order |
| `accept_f50_c_a_source_for_an_undeclared_work_order_is_refused` | the undeclared-label refusal names the extra work order |
| `accept_f50_c_a_fingerprint_that_is_not_canonical_hex_is_refused` | the unreadable-anchor refusal names the work order and the value |

Three retail (`#[ignore = "requires CS_GAME_DIR"]`):

| Test | What it pins |
| --- | --- |
| `accept_f50_c_the_whole_campaign_has_one_probe_route_per_declared_work_order` | 24 routes in declared order; the one anchor fingerprint equals production discovery; 17 ready with every position resolved; the 7 refused pinned by label with a re-measure reason |
| `accept_f50_c_every_ready_route_reenters_the_same_mission_after_each_ac03_interruption` | for every ready route × every interruption: the five reentries agree on mission/world/program and carry the route's anchor, and a **fresh production pass** (`SourceContext::read` + `bind_campaign` + `probe_routes` again, standing for the restart after an interruption) plans the identical reentry |
| `accept_f50_c_the_playtest_route_document_lists_every_work_order_as_planned` | the committed human route document lists exactly the planned routes: ready rows carry position and the three ids, refused rows carry no identity and the verbatim refusal, no row extra or missing |

The three retail tests share the campaign F50-B binds through its `bound()`
fixture (one `SourceContext`, one `bind_campaign` for the whole suite); the
reentry test performs the suite's only second read, deliberately: one fresh
pass is the restart the scenario names, and comparing every interruption's
reentry against it is what keeps 85 comparisons honest without re-reading the
installation 85 times.

### Mutation probes (implementation changed, test must fail, then reverted)

Each mutation was applied to `crates/cs_content/src/campaign_bindings.rs`
except 6, applied to the document; the suite was run with
`cargo test -p cs_app --test campaign --locked -- accept_f50_c_ --include-ignored`,
and the file restored from a copy in `private/scratch/` (Git-ignored);
`cmp` verified both restored files byte-identical to the committed ones.

| # | Mutation | Result |
| --- | --- | --- |
| 1 | `routes.retain(\|route\| route.is_ready())` — drop refused routes from the plan | 3 tests FAILED: the synthetic refused-route test (3 routes expected, 2 found), the retail one-route-per-work-order test (`plan.len() == inventory.len()`), the route-document test (refused rows no longer match) |
| 2 | disable the two-fingerprints refusal (`if false && …`) | `a_campaign_read_under_two_fingerprints_is_refused` FAILED (5 passed) |
| 3 | a declared work order with no source `continue`s instead of being refused | `a_declared_work_order_with_no_source_is_refused` FAILED (5 passed) |
| 4 | shrink `ProbeInterruption::ALL` to four (drop `SettingsChange`) | `the_minimum_scenario_is_exactly_the_five_ac03_interruptions` FAILED (8 passed) — the retail tests iterate `ALL` consistently, so pinning the *set* is what stops the scenario silently shrinking |
| 5 | the `SAVE_RESTART` reentry anchored under a different fingerprint | `every_ready_route_reenters_the_same_mission_after_each_ac03_interruption` FAILED with `M01 loses its installation anchor at SAVE_RESTART` |
| 6 | one world id in `playtest-routes.md` changed (`world/c1` → `world/c9` at M02) | `the_playtest_route_document_lists_every_work_order_as_planned` FAILED with `M02's row does not carry world/c1` |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f50_c_ --include-ignored` | 0 (9 tests, all passing) |
| each of the 9 task tests alone with `--exact --include-ignored` | 0 — 9 × 1 passed |
| `python3 tools/validate_evidence.py private/evidence/F50-C/acceptance.json --artifact-root private/evidence/F50-C --require-pass` | 0 |
| `python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py'` | 0 |

## Evidence

`private/evidence/F50-C/acceptance.json`, copied unchanged to
`docs/findings/evidence/F50-C.json`. Capabilities `["retail", "synthetic"]`;
9 discovered / 9 executed / 9 passed / 0 failed; claim `implemented`. The
artifact beside the report is `probe-routes.json`: the whole plan derived
from the installation by the production path (24 routes with positions,
states, identities or refusals, the one anchor fingerprint and the digest of
the pinned route document), ids and counts only. The committed copy is the
**implementer's** run; per this repository's review policy the Rally
reviewer regenerates it on the reviewed commit and their copy, naming both
identities, supersedes this one.

## What this stage does not do

- **It does not play, kill or retry a mission.** No mission runtime exists to
  drive: the launch path is `VS-M01-RUNTIME` (#359), in progress under
  another session at this hand-over and owning `mission_launch.rs` /
  `mission_session/`. The plan is the contract those consumers drive; the
  reentry test proves the plan is re-derivable and stable across a production
  restart, not that a mission session survives a death. `human_play` is not
  claimed and no original executable was run.
- **It does not bind the campaign progression.** The successor relation is
  still unmeasured; `CoverageReport::is_ready()` stays false and nothing here
  changes that.
- **It does not resolve M09, M11, M14, M15, M20, M22 and M23.** They stay
  refused routes with their reason.
- **It awards nothing.** The claim is `implemented`; a Rally merge awards
  `checked` at most, and no agent review replaces the owner's human approval.

## Review addendum (2026-10-08, the Rally review session)

Reviewed by **bunny-alpha-1** — the same Rally agent name and model as the
implementer, but a separate fresh session holding the #206 review claim of
2026-10-08T18:19Z, with no shared conversation context; the review was
reconstructed from the branch, the task history and the repository. The
branch was rebased cleanly onto `origin/main` 62ee5a3d (no overlapping files,
no `Cargo.toml`/`Cargo.lock` in the incoming commits), then the reviewer ran
the whole check set on the rebased tree 0bbd053e: `cargo fmt --check` 0,
`cargo clippy -D warnings` 0, the full workspace suite 0 (464 test-result
summaries), the 9-test task selection with `--include-ignored` 9/9, and each
of the 9 task tests alone with `--exact` (9 × 1 passed). Two independent
reviewer mutation probes re-confirmed test sensitivity: dropping refused
routes from the plan (`routes.retain(|route| route.is_ready())`) failed 3
tests, and disabling the two-fingerprints refusal failed exactly 1; both were
reverted and the tree verified clean.

**Installation note (a measured change during the review).** The owner moved
the owner-supplied `crimson.decrypted.exe` out of `$CS_GAME_DIR` at
2026-10-08T18:06:43Z (Rally owner notes on #722, #779 and siblings), so the
installation fingerprint reverted from the value recorded above
(`c14a876f…`) to `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
with `content_sha256`
`a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` — the
same value the pre-2026-10-05 evidence (F06-D, F15-D, F27-D, F38-B …)
recorded. The install's 228 regular files are byte-identical (newest ctime
2026-09-29); only the root entry came and went. The reviewer regenerated the
acceptance report under the current installation: every route fact is
identical — 24 declared / 17 ready / 7 refused, the same seven refused work
orders, 85 reentries, and the pinned `playtest-routes.md` still matches the
plan row-for-row (`accept_f50_c_*` 9/9 green). The committed
`docs/findings/evidence/F50-C.json` is now the reviewer's regeneration
(naming both identities); the measurements in the sections above describe
the installation as it stood at implement time.
