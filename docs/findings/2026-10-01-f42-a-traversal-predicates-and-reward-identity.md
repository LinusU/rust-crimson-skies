# F42-A: traversal predicates and reward identity — plan, semantics and unknowns

**Task:** #171, branch `rally/171-define-traversal-predicates-and-reward-i`,
prefix `accept_f42_a_`. Stage A of
`specs/F42-stunts-fame-photos-and-optional-achievement-events.md`; shared
contract `docs/contracts/STATE-TRANSACTIONS.md`.

This record is written **before** the code (sheet requirement: "Before
editing, list the specific functions/files and one observable failure"). The
"what was built" and "checks" sections are appended when the slice is done,
and the **review findings** section at the end records what the review of
`91ab6ad` changed.

## Files and functions this slice will add

All inside the task's owner paths
(`crates/cs_sim/src/stunts.rs`, `crates/cs_content/src/stunts.rs`,
`crates/cs_app/src/stunts.rs`, `tests/`, `docs/findings/`), plus the
module-declaration wiring each crate's `lib.rs` needs.

1. `crates/cs_content/src/stunts.rs` — the **declared** half:
   * `Gate { center_m, normal, right_half_extent_m, up_half_extent_m,
     half_depth_m, evidence: GateEvidence }` — one authored traversal
     aperture, with `GateEvidence::{Measured, Reconstructed, NotRecovered}`
     implementing sheet behavior 5 ("manually drawn replacement volumes are
     marked reconstructed until validated").
   * `TraversalRules { min_forward_cosine: Resolved<f64>,
     min_clearance_m: Resolved<f64> }` — the direction and clearance rules,
     `Resolved` so an unmeasured rule stays an explicit unknown.
   * `MissionScope` — the missions that declare the stunt. **No wildcard
     variant exists**, so "every world stunt in every mission" (sheet
     behavior 3) is not expressible.
   * `StuntReward { fame: Resolved<u32>, cash_minor: Resolved<u64>,
     media: Resolved<Option<ContentId>> }` — the reward identity's payload;
     media is a `ContentKind::ScrapbookItem` id (F47 owns its contents).
   * `StuntCriticality::{Optional, CriticalPath}`, `StuntRepeat::{Once,
     Repeatable}`, and `StuntDefinition::try_new(StuntDraft)` refusing a
     non-`Stunt` id, a non-`World` world, corrupt gate geometry, an
     out-of-range cosine rule, a negative clearance rule, a non-`Mission`
     scope entry, an empty scope and a duplicate scope entry.
   * `declared_synthetic_gate_stunt()` — the minimal fixture.
2. `crates/cs_sim/src/stunts.rs` — the **runtime** half (`cs_sim` cannot see
   `cs_content`, so it re-declares and re-validates its own vocabulary, the
   way `cs_sim::campaign::graph` mirrors `cs_content::campaign`):
   * `Gate` (lowered: `WorldPosition` centre, `UnitVec3` normal, the derived
     in-plane `right`/`up` basis, the extents) and `Gate::classify` — the
     geometry predicate.
   * `TraversalRule`, `StuntRule`, `StuntReward`, `StuntCriticality`,
     `StuntRepeat` — the lowered declared record with `is_eligible_in`.
   * `StuntMovement::{Swept, Rebased, Teleport}` and `StuntAuthority::
     {PlayerFlight, AiFlight, DeveloperCamera, Spectator}` — the typed input.
   * `TraversalRequest` (session-qualified), `PassRefusal`, `TraversalOutcome`,
     `TraversalCompletion` — the typed output.
   * `StuntRewardKey { profile, mission, stunt }`, `Admission`,
     `StuntLedger` (stale-session refusing, once-only dedup) and `StuntBook`
     (per-session judge + ledger, `completions()`).
3. `crates/cs_app/src/stunts.rs` — the **lowering boundary**:
   `lower_stunt` (declared → runtime, refusing every `Resolved::Unknown` on
   the gate, the direction rule, the clearance rule and the reward by name)
   and `lower_mission_stunts` (the mission's declared set, duplicate ids
   refused).
4. `crates/cs_app/tests/accept_f42_a_traversal_predicates.rs` plus unit tests
   inside the two schema modules, all named `accept_f42_a_*`.

## The one observable failure this slice must fix

With a proximity/plane test instead of the declared predicate, the AC01
scenario ("fly through, beside, backwards and teleport across the same
synthetic gate; only eligible traversals count") passes the *teleport* and
counts the *backwards* pass: one `StuntBook` that should report
`completions() == 1` reports `== 3`, and the once-only reward ledger pays a
second time for the same `(profile, mission, stunt)` identity. A second
observable failure: a rebase — the same physical flight re-expressed in a new
origin epoch — must still count, so a predicate that treats every
discontinuous coordinate change as a teleport loses a genuine passage.

## Rules that need a decision the sheet does not fix (designed, documented)

* **What counts as a traversal.** A traversal is a segment that *crosses* the
  gate's **mid-plane** at a point inside the aperture (|right|, |up| within the
  authored extents). Reaching the plane is part of the definition: a segment
  that stops inside the slab, turns back before it or runs parallel to it did
  not fly through the gate, however deep inside the hole its endpoints sit. A
  segment that only clips the slab's rim is a miss, not a pass — a full
  swept-box overlap test would credit a shallow graze of the rim, which is not
  "flying through the gate". The authored `half_depth_m` therefore records how
  thick the gate was *drawn*; it is validated but never decides a traversal,
  and a zero half depth (a plane gate) is a legal authored volume.
* **Direction.** The travel direction is the swept segment's own direction and
  is compared with the gate normal by cosine against the authored
  `min_forward_cosine`. The segment direction is used rather than the
  airframe's nose vector so the predicate reads one canonical pose pair and
  cannot disagree with the sweep collision uses; a backwards pass is
  `dot = -1` and refuses for any rule above 0.
* **Clearance.** The margin is measured from the crossing point to the nearest
  aperture rim, in the gate's in-plane frame: `min(right_half - |r|,
  up_half - |u|)`. A pass inside the aperture but inside the authored margin
  is `InsufficientClearance`, not a pass. A margin *wider than the hole itself*
  is an authored impossibility, not a near miss: it is named
  `UnsatisfiableClearance` by the declared constructor, the runtime rule and
  the raw predicate, so a corrupt record can never silently refuse every
  traversal of a gate as a near miss.
* **Rebase vs. teleport.** `StuntMovement::Rebased` keeps both endpoints
  exactly like `Swept` (F16 `OriginChange::Rebase` preserves swept
  continuity); only `Teleport` is discontinuous. A teleport can still *report*
  where the aircraft now is, and can therefore change a trigger's
  inside/outside state, but it never earns a stunt.
* **Who may earn.** Only `StuntAuthority::PlayerFlight` — the actor the
  session bound to flight input. A developer camera, a spectator view and a
  foreign actor cannot earn, and an AI aircraft is not the stunt's subject
  either (sheet behavior 1 names the developer camera explicitly; the AI
  exclusion is designed, see below).
* **Reward duplication.** `StuntRewardKey = (profile, mission, stunt)` — the
  identity the sheet names for one-time photo rewards. A `StuntRepeat::Once`
  stunt pays once per key; the ledger is *seeded* from the persisted record at
  session start, so a mission **retry** (new `SessionGeneration`, same
  identity) cannot re-pay. `StuntRepeat::Repeatable` pays on every traversal
  and its keys are recorded but never refuse.
* **Criticality.** `StuntCriticality::Optional` is the default shape of a
  record; nothing in this stage touches mission success, so an optional stunt
  cannot affect the critical path (sheet behavior 4). Wiring an authored
  critical-path stunt to mission success is F42-C.
* **Eligibility is per mission, structurally.** The declared scope is a
  non-empty list of mission ids, and the runtime book re-checks
  `is_eligible_in` even though the caller supplies the mission's own set. A
  stunt declared for `m01` refuses in `m02` with `MissionNotEligible` instead
  of paying.
* **A book pairs with its own ledger's session.** `StuntBook::new` refuses a
  ledger whose generation is not the book's (`StuntObserveError::LedgerSession`),
  including an unopened default ledger. Without that pairing, every later
  grant would fail *mid-traversal* — after the book had advanced its tick and
  possibly paid an earlier rule — so the "the book is unchanged on error"
  promise would be false.

## Recorded unknowns (do not guess; file follow-up tasks)

* **The original stunt encoding is undecoded.** Nothing in this repository
  measures which file stores stunts, how a gate is spelled, what its extents
  or direction rule are, or how a stunt is bound to a mission. F13 recovers
  mission programs but no stunt layout; the F14-D retail baseline lists
  "stunts" only as a catalog *kind*. Every id, shape, rule and value in this
  slice is therefore `Origin::SyntheticFixture` / `GateEvidence::
  Reconstructed` / `Provenance::designed` — never an original-fidelity claim.
  F42-D is the stage that must measure it.
* **Original repeatability rules are unmeasured.** `Once` vs. `Repeatable`
  here is a designed vocabulary, not a measured rule. F42-D's retail
  playthrough must record, for each original stunt, whether a second pass
  pays again.
* **Original gate geometry, clearance and direction thresholds are
  unmeasured.** The fixture's 12 m × 8 m aperture, 2 m margin and
  `cos ≥ 0.5` are designed values that exist so the predicate is testable.
* **Fame and cash amounts are unmeasured.** The contract fixes the *unit*
  (integer minor units) but not a stunt's payout; the fixture's 25 fame /
  500 minor units are designed.
* **Whether an AI aircraft can earn a stunt is unmeasured.** This slice
  refuses `AiFlight` so that a pursuing AI's flight cannot be a player reward
  by accident; the sheet treats AI reactions as a separate consumer (F42-C).
* **Multi-gate (sequence) stunts are out of this slice.** The authored form
  for an ordered sequence of gates is not measured, so stage A declares a
  single gate; F42-B ("swept/sequence stunt detection") extends this same
  record with the sequence variant and its cursor rather than adding a second
  schema here.
* **Fame/photo presentation** (scrapbook page unlock, fame display, the
  photograph's own asset) belongs to F47 and F42-C.

## What was built

* `crates/cs_content/src/stunts.rs` — the **declared** half:
  `GateEvidence::{Measured, Reconstructed, NotRecovered}` (sheet behavior 5
  is a field, not a convention), `Gate` (centre, authored normal, in-plane
  half extents, half depth, evidence marking) with `Gate::unit_normal` /
  `is_usable`, `TraversalRules` with both thresholds `Resolved`,
  `MissionScope` (a non-empty unique list of `Mission` ids, **no wildcard
  variant**), `StuntReward` (fame, cash in minor units, scrapbook media id —
  `ContentKind::ScrapbookItem` only), `StuntCriticality`, `StuntRepeat`, and
  `StuntDefinition::try_new(StuntDraft)` refusing a non-`Stunt` id, a
  non-`World` world, corrupt gate geometry (non-finite centre/normal, a
  zero-length normal, a non-positive extent, a negative depth), an
  out-of-range cosine rule, a negative clearance rule, non-scrapbook media, an
  empty scope, a non-`Mission` scope entry and a duplicate scope entry.
  `declared_synthetic_gate_stunt()` is the fixture, with
  `synthetic_gate_mission()` / `synthetic_gate_world()` and the eight
  `SYNTHETIC_GATE_*` designed constants.
* `crates/cs_sim/src/stunts.rs` — the **runtime** half:
  - lowered `Gate` (a `WorldPosition` centre, a `UnitVec3` normal and the
    deterministically derived `right`/`up` in-plane basis) with `gate_frame`
    and `classify`, which requires a swept segment that *reaches* the mid-plane,
    a crossing point inside the aperture, `cos(travel, normal) ≥
    min_forward_cosine` and a rim clearance of at least `min_clearance_m`.
    `GateRefusal` is the geometry-only refusal (`NoSweep`, `MissedGate`,
    `WrongDirection`, `InsufficientClearance`, `UnsatisfiableClearance`,
    `BadRule`) and names no stunt; `PassRefusal` adds the identity
    (`ForeignActor`, `NotPlayerFlight`, `MissionNotEligible`, `Discontinuous`,
    `Geometry{stunt, reason}`, `AlreadyRewarded`).
  - `TraversalRule`, `StuntRule::try_new` (its own re-validation, since
    `cs_sim` cannot see `cs_content`), `StuntReward`, `StuntCriticality`,
    `StuntRepeat`, `GateEvidence`.
  - `StuntMovement::{Swept, Rebased, Teleport}` and `StuntAuthority::
    {PlayerFlight, AiFlight, DeveloperCamera, Spectator}` (only
    `PlayerFlight` earns) as the typed input; `TraversalRequest` is
    session-qualified.
  - `StuntRewardKey = (profile, mission, stunt)`, `Admission`,
    `StuntLedger` (seeded from the persisted record, stale-session refusing)
    and `StuntBook::observe` — the one place a completion is counted.
    `StuntObserveError` carries `StaleSession`, `NotAdvancing`,
    `DuplicateStunt`, `LedgerSession` and `CorruptLedger`.
* `crates/cs_app/src/stunts.rs` — the lowering boundary: `lower_stunt`
  (declared → runtime, refusing an unknown gate, direction rule, clearance
  rule, fame, cash or media **by claim id and reason**, and a runtime
  validation failure as `Rejected`) and `lower_mission_stunts`, which filters
  a mission's declared set by scope and refuses a duplicate id.
* `crates/cs_app/tests/accept_f42_a_traversal_predicates.rs` — 18 acceptance
  tests; plus 6 unit tests inside the two schema modules. 24 `accept_f42_a_`
  tests in total, all on the production path
  (`declared_synthetic_gate_stunt` → `lower_stunt` → `StuntBook`).

## Test sensitivity (each mutation was applied, run and reverted)

Each mutation below was applied to production code, the `accept_f42_a_`
selection was run, the failure was recorded, and the file was then restored
and the full selection re-run green.

| Removed behavior | Tests that failed |
| --- | --- |
| the direction rule (a cosine below the authored minimum no longer refuses) | `..._through_counts_and_beside_backwards_and_teleport_do_not`, `..._the_predicate_uses_direction_and_margin_rather_than_proximity` |
| the teleport distinction (`Teleport` reports swept endpoints) | the same two |
| the one-time dedup (`StuntLedger::grant` always admits) | `..._one_time_reward_pays_once_across_a_retry_but_a_repeatable_one_pays_again` |
| the aperture (only the half-depth test remains, so a pass beside the hole counts) | `..._through_counts_...`, `..._a_miss_a_rim_graze_and_a_stationary_sample_never_count` |
| the subject and authority checks | `..._only_the_player_aircraft_counts_and_geometry_failures_carry_measurements` |
| the runtime mission-scope re-check | `..._the_same_world_in_another_mission_has_a_different_eligible_set` |
| the declared id-namespace validation | `cs_content::stunts::accept_f42_a_declared_stunt_names_every_authoring_mistake` |

## Review findings (review of `91ab6ad`)

The review reproduced three defects on the implementation as submitted, fixed
each on the branch, and added a regression test per defect. Each fix was
checked for sensitivity by removing it again and confirming a real
`accept_f42_a_` test fails.

1. **A traversal was credited without ever reaching the gate plane**
   (`Gate::classify`). The crossing parameter was clamped to `[0, 1]`, so for a
   segment that never reaches the mid-plane the crossing point became its
   nearer endpoint, and that endpoint was then accepted whenever it happened to
   lie inside the slab. The *unmodified* fixture demonstrates it: flying
   `[0,0,200] → [0,0,2]` — stopping two metres short of a gate with a 4 m half
   depth — was reported as a completed traversal at `[0,0,2]`, paying fame,
   cash and the one-time photo for a stunt never flown. The same clamp
   credited a segment that started inside the slab and flew away from the
   plane, and (with a legal `min_forward_cosine = 0.0`) a segment sliding
   parallel to the plane inside the slab. Fix: the segment must *span* the
   plane (`(d0 ≤ 0 ∧ d1 ≥ 0) ∨ (d0 ≥ 0 ∧ d1 ≤ 0)`), otherwise `MissedGate`.
   The `half_depth_m` term was removed from the crossing test, which also
   fixes a second bug: the authored thickness was being used as the tolerance
   for "did we reach the plane", so a legal **zero-thickness plane gate**
   refused genuine crossings on the float residue of the interpolation.
2. **A book could be paired with another session's ledger**
   (`StuntBook::new`). Nothing checked that the ledger's generation was the
   book's, so the first grant that reached it returned `CorruptLedger` /
   `StaleSession` *mid-traversal* — after `last_tick` had advanced and possibly
   after an earlier rule in the same sample had already been paid — while
   `observe` documented "the book is unchanged". Fix: `StuntBook::new` refuses
   a mismatched or unopened ledger with `StuntObserveError::LedgerSession`, so
   the grant inside `observe` cannot fail and the documented atomicity is true.
3. **An impossible clearance rule blamed the aircraft.** A margin wider than
   the gate's own hole can never be satisfied, and every crossing was refused as
   `InsufficientClearance` — a flight report blaming the pilot for an authored
   impossibility. Fix: named `UnsatisfiableClearance` (with both margins) by
   the declared constructor, the runtime `TraversalRule::new` and the raw
   `classify`, so the corrupt record is caught at lowering time and the raw
   predicate still refuses it if handed such thresholds directly. The
   comparison is strict, so the exactly achievable maximum still counts.

Two test-quality problems were fixed as well: an assertion in
`..._a_scope_cannot_be_emptied_or_widened_to_every_mission` that could not
fail (`missions().is_empty() || len() == 1`) was replaced with a real
two-mission widening check, and a doc comment on
`..._a_duplicate_stunt_id_in_one_book_is_refused` described a two-rule
foreign-actor case the test does not contain (the duplicate check in
`lower_mission_stunts`, which had no test at all, was added instead).

Additional mutations run during the review, each failing at least one real test
before being reverted:

| Removed behavior | Tests that failed |
| --- | --- |
| the plane-spanning requirement | 5 tests, including `..._a_segment_that_never_reaches_the_mid_plane_never_counts` and `..._a_parallel_segment_inside_the_slab_is_not_a_crossing` |
| the declared `UnsatisfiableClearance` check | `..._a_clearance_wider_than_the_hole_is_a_corrupt_record` |
| the runtime `TraversalRule::new` `UnsatisfiableClearance` check | the same test |
| the predicate's `UnsatisfiableClearance` check | the same test, plus `..._the_runtime_gate_classifies_a_segment_against_its_own_rules` |
| the ledger/session pairing in `StuntBook::new` | `..._a_book_whose_ledger_belongs_to_another_session_is_refused` |

## Checks run

* `cargo test --workspace --locked -- accept_f42_a_ --include-ignored`: 24
  tests pass (18 in `cs_app/tests/accept_f42_a_traversal_predicates.rs`, 3 in
  `cs_content::stunts`, 3 in `cs_sim::stunts`).
* `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings` and `cargo test --workspace
  --locked` all pass; the workspace result is in the handover summary.
* No evidence report: this stage needs ordinary build/test only, so the
  acceptance harness produces no `private/evidence/#171/acceptance.json`.
