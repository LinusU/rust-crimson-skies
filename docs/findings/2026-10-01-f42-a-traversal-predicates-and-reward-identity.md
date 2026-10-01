# F42-A: traversal predicates and reward identity — plan, semantics and unknowns

**Task:** #171, branch `rally/171-define-traversal-predicates-and-reward-i`,
prefix `accept_f42_a_`. Stage A of
`specs/F42-stunts-fame-photos-and-optional-achievement-events.md`; shared
contract `docs/contracts/STATE-TRANSACTIONS.md`.

This record is written **before** the code (sheet requirement: "Before
editing, list the specific functions/files and one observable failure"). The
"what was built" and "checks" sections are appended when the slice is done.

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

* **What counts as a traversal.** A traversal is the crossing of the gate's
  **mid-plane** inside the aperture (|right|, |up| within the authored
  extents, |depth| within the authored half depth). A segment that only clips
  the slab's rim is a miss, not a pass — a full swept-box overlap test would
  credit a shallow graze of the rim, which is not "flying through the gate".
* **Direction.** The travel direction is the swept segment's own direction and
  is compared with the gate normal by cosine against the authored
  `min_forward_cosine`. The segment direction is used rather than the
  airframe's nose vector so the predicate reads one canonical pose pair and
  cannot disagree with the sweep collision uses; a backwards pass is
  `dot = -1` and refuses for any rule above 0.
* **Clearance.** The margin is measured from the crossing point to the nearest
  aperture rim, in the gate's in-plane frame: `min(right_half - |r|,
  up_half - |u|)`. A pass inside the aperture but inside the authored margin
  is `InsufficientClearance`, not a pass.
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
