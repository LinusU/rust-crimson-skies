# F30-D: target order, reveal rules and original assistance behavior — what is measured and what is not

Date: 2026-10-02. Task: **F30-D** "Verify target order, reveal rules and original
assistance behavior" (Rally #124,
`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`, section
`### F30-D`). Shared contract: `docs/contracts/CLI-EVIDENCE.md` (evidence) and
`docs/contracts/IDENTITY-CONTENT.md` (content identity). Capabilities used:
**`retail`** (read-only `$CS_GAME_DIR`) plus ordinary build/test. No `gpu`,
`audio`, `human_play`, `human_review` or `network_real` was used or needed.

Owner paths touched: `crates/cs_sim/src/targeting.rs` and
`crates/cs_content/src/target_rules.rs` (documentation of this measurement
only — no production behavior changed), `crates/sim/tests/` and
`crates/cs_content/tests/` (the acceptance tests and the harness, the same
placement F30-A/B/C used for their `tests/`), `docs/findings/`.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/tests/accept_f30_d_crosshair_eligibility.rs` (new):
  `accept_f30_d_crosshair_selection_needs_eligibility_and_free_sight_together`,
  `accept_f30_d_the_reveal_rule_gates_the_cycle_and_the_crosshair_together`.
- `crates/cs_content/tests/accept_f30_d_original_target_vocabulary.rs` (new)
  and `crates/cs_content/tests/f30_d_support/mod.rs` (new): the retail
  measurement, its constants and the action classification.
- `crates/cs_content/tests/evidence_report_f30_d.rs` (new): the
  CLI-EVIDENCE harness.
- `crates/cs_sim/src/targeting.rs`, `crates/cs_content/src/target_rules.rs`:
  module/enum documentation recording what F30-D measured.
- This file, and `docs/findings/evidence/F30-D.json` (the report copy).

**One observable failure.** AC04 asks that crosshair selection respect
occlusion *and* eligibility together. F30-A pinned the cone, the observer
exclusion, the tie-breaks and the occlusion set as producer evidence; F30-C
pinned that a consumer never invents that set. Nothing asserted the
conjunction: an implementation whose crosshair query filtered on
`TargetStore::present` — "still in the world" — instead of `eligible` would
answer with the **unrevealed** raider 10 m from the observer while every
earlier test still passed. `accept_f30_d_crosshair_selection_needs_eligibility_and_free_sight_together`
walks one ray past five candidates, each ineligible or occluded in a different
way, and asserts that every fact independently removes a candidate and that
restoring it puts the candidate back.

## What was read, and how (all read-only)

Nothing was written inside `$CS_GAME_DIR`. Only ids, symbolic names, byte
offsets, counts and SHA-256 digests leave the measurement; **no original
display text is reproduced here or committed** (`AGENTS.md` rule 3), the same
rule F22-H followed for the control vocabulary.

Read through **production** readers:

- `cs_assets::install::{discover, fingerprint, content_fingerprint, sha256}` over
  the whole installation and over `strings.dll`;
- `cs_content::config::StringCatalog` over `strings.dll`'s `RT_STRING` tree —
  every id the test-local table yields is resolved through it, so a wrong parse
  fails the acceptance test instead of agreeing with itself;
- `cs_content::target_rules::DeclaredAction` — the project's own declared
  vocabulary, which the classification must cover exactly once.

Read by a **test-local** parser in `crates/cs_content/tests/f30_d_support/mod.rs`
(`parse_name_id_table`), because no production reader owns a PE `.data`
section: the `{pointer, id}` symbol table in `strings.dll`.

## Fingerprints

| Item | Value |
| --- | --- |
| Installation fingerprint | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| Canonical-content fingerprint | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |
| `strings.dll` | `7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21`, 131 072 bytes |

## Observation 1 — the original names eleven target commands

`strings.dll` carries a 1 023-entry `{name, id}` table in its PE `.data`
section (ids 100 … 17 142, stored twice identically), and every entry of the
two control ranges F22-H measured is named — so a command the original exposes
cannot hide behind an anonymous label. The **target** family is:

| id | name | what it names |
| --- | --- | --- |
| 10 008 | `MSG_CMD_TARGET_NOTHING` | clear the selection |
| 10 009 | `MSG_CMD_TARGET_UNDER_RETICULE` | the pick under the reticule |
| 10 015 / 10 016 / 10 017 | `MSG_CMD_TARGET_NEXT_ENEMY` / `PREVIOUS` / `NEAREST` | the enemy class |
| 10 018 / 10 019 / 10 020 | `MSG_CMD_TARGET_NEXT_ALLY` / `PREVIOUS` / `NEAREST` | the ally class |
| 10 021 / 10 022 / 10 023 | `MSG_CMD_TARGET_NEXT_GROUND` / `PREVIOUS` / `NEAREST` | the ground class |

Plus the list header the rebinding UI draws them under:
`MSG_TARGETING_CONTROLS` (3 009). All twelve ids resolve to non-empty
`RT_STRING` labels of language 1033 in the production `StringCatalog`
(112 blocks, 1 792 units, two non-`RT_STRING` leaves, zero undecodable units,
zero duplicate ids).

**What this measures and what it does not.** It measures that the original
exposes *three named target classes* with a *next / previous / nearest* action
each, plus a clear and an under-reticule pick. The project models that shape as
`SelectionFilter` axes (`Allegiance(Hostile)`, `Allegiance(Friendly)`,
`NotClass(Aircraft)`, `Objective`, `Any`) over one cycle action and one nearest
action, which is a superset of the observed three classes: `Objective` and
`Any` have **no** counterpart among the measured labels. The original's
`GROUND` is this project's `NotClass(Aircraft)` axis spelled differently; the
spelling difference is recorded rather than papered over, because a name is
the only evidence there is.

It does **not** measure the order the cycle walks, what "nearest" means in the
original (distance? screen centre? threat?), or whether the original's cycle
excludes allies of the observer's own faction inside the "enemy" class. All of
that is native data in the packed executable and in the code behind the
`2115`/`2139`/`2140` callbacks F22-H measured (F22-H Observation 3).

## Observation 2 — the classification of this project's declared actions

`ACTION_CLASSIFICATION` in the support module classifies every
`DeclaredAction` exactly once against the measured vocabulary, and the test
asserts the partition, so a new declared action cannot be added without a
classification:

| `DeclaredAction` | status | measured original label(s) |
| --- | --- | --- |
| `next_hostile` | observed | `..._NEXT_ENEMY`, `..._NEXT_ALLY`, `..._NEXT_GROUND` |
| `previous_hostile` | observed | `..._PREVIOUS_ENEMY`, `..._PREVIOUS_ALLY`, `..._PREVIOUS_GROUND` |
| `nearest_hostile` | observed | `..._NEAREST_ENEMY`, `..._NEAREST_ALLY`, `..._NEAREST_GROUND` |
| `nearest_ally` | observed | `..._NEAREST_ALLY` |
| `nearest_non_aircraft` | observed | `..._NEAREST_GROUND` |
| `under_crosshair` | observed | `MSG_CMD_TARGET_UNDER_RETICULE` |
| `clear` | observed | `MSG_CMD_TARGET_NOTHING` |
| `nearest_objective` | **absent** | — |
| `nearest_attacker` | **absent** | — |

The two `absent` rows are in the sheet's deliverable list ("enemy/objective …
nearest-attacker … actions where verified"), so both stay in the schema; each
carries designed provenance, and `cs_app::targeting::AssistanceOffer::presentable`
keeps a designed value from being drawn as original behavior. `absent` means
**absent from the shipped observation**, never "the original cannot do it": the
nearest-attacker pick is a plausible thing for a 2000 PC game to bind, and
naming it as an observation would be a guess.

## Observation 3 — the assist vocabulary that *is* named: padlock

The original names a twelve-label **padlock** family: three mode commands
(`MSG_CMD_PADLOCK_SNAP` 11 025, `MSG_CMD_PADLOCK_WATCH` 11 026,
`MSG_CMD_PADLOCK_STICK` 11 036) and nine directions (`MSG_PADLOCK_DL`, `_D`,
`_DR`, `_L`, `_M`, `_R`, `_UL`, `_U`, `_UR`, 11 027 … 11 035). It also names
four target-display **fonts**: `FONT_AIMPOINT` (17 004), `FONT_TARGETHELP`
(17 010), `FONT_TARGETTITLE` (17 011), `FONT_TARGETPOS` (17 012).

Two consequences, and a boundary neither may cross:

1. The original's assist concept is **not** the pair of independent on/off
   options this project declares (`TargetRuleSet::lead_indicator`,
   `TargetRuleSet::aim_assistance`). It is one named family with modes and
   directions. F30 non-negotiable 3 requires the two options to stay separate
   and requires original evidence before either is presented, so this stage
   keeps the schema **unchanged** and records the difference: no padlock
   semantics are declared, and nothing in the project may claim an automatic
   hit correction.
2. A font name says a text element exists; it says nothing about what is drawn
   in it, and an "aim point" font is not evidence that an aid is computed. The
   measurement stops at the name.

## Observation 4 — two measured absences

Over all 1 023 named entries, **no** name contains `REVEAL`, `VISIB`,
`SENSOR`, `DETECT`, `HIDDEN`, `STEALTH`, `LEAD` or `ASSIST`. So:

- the shipped string image names **no reveal or visibility concept**;
- it names **no lead-indicator or aim-assistance option**.

Because every entry of the table is named, these are strong statements about
the string vocabulary — and only about it. A reveal rule implemented entirely
in the executable would be invisible here, so the project's reveal axis stays
project design and no reveal semantics are inferred from the absence.

The absence measurement is not vacuous: adding a fragment the table does match
(`TARGET`) makes the test fail with the sixteen matching names, which is how
the loop was checked (probe 5 below).

## The AC04 half, at the production layer

`crates/cs_sim/tests/accept_f30_d_crosshair_eligibility.rs` pins the rule the
criterion names, on a roster lined up along one ray:

| actor | distance | state in the walk |
| --- | --- | --- |
| 7 | 10 m | raider, not revealed → **present but not eligible** |
| 2 | 20 m | raider, eligible; occluded by the producer in later steps |
| 5 | 40 m | raider, eligible; reveal cleared |
| 9 | 60 m | raider, eligible; phase closed |
| 3 | 80 m | wingman (declared friendly), eligible; then destroyed |

Each fact is applied on its own and then restored on its own, so no single
missing rule can hide behind another one answering correctly. The pick is
eligibility **and** free sight; when all four facts hold at once the query
answers "nothing under the crosshair" and clears the selection — never a
fallback to an ineligible actor. Two further properties fall out and are
pinned: occlusion is evidence about the *ray*, not about the roster (an
occluded actor stays `eligible`), and the crosshair filter is eligibility, not
allegiance (a declared friendly is picked when no eligible hostile is in the
cone).

The second test pins the reveal rule across one boundary: an unrevealed actor
is in neither the cycle nor the crosshair, a held selection on one is cleared
with `SelectionClearReason::NotRevealed`, the actor is still `present`, and
re-revealing it restores both queries in the same boundary.

## Defects found while writing the acceptance tests

**A lifecycle transition reported after a destruction can resurrect a destroyed
actor.** The rule that prevents it is one line of `TargetStore::record_lifecycle`
— only a kind that `ends_targeting` writes the recorded ending — and the test
now pins it, so that line cannot be edited away silently.

The observable failure the pin guards against: a damage tick that reports
`Destroyed` and a second report that reports `PilotBailout` for the same actor
leaves the actor **eligible again** if `record_lifecycle` also *clears* the
recorded ending for a non-terminal kind — selectable by the cycle, pickable
under the crosshair, and described by a reticle — after the damage system
destroyed it. **No F30-A/B/C test caught it**: the rebase confirms all **60**
of them still pass with that branch added, because they record the bailout
*before* the destruction, so both orders pass for them.
`accept_f30_d_crosshair_selection_needs_eligibility_and_free_sight_together`
catches it (probe 3 in the table below).

The production code is **unchanged**: the shipped `record_lifecycle` already
refuses to clear a recorded ending. The defect the probe finds is a *regression*
the current code does not have, so this is a pin, not a repair.
Nothing else in F30-A/B/C needed repair: the crosshair query, the reveal gate,
the phase record and the assistance gate behaved as their own contracts say
under every probe below.

## Repairs applied in review

Recorded so the next reader knows which edits the reviewing agent made, as
distinct from the implementing agent's work (see "Review" at the end).

1. `parse_name_id_table` now **asserts** the layout it relies on
   (`VirtualAddress == PointerToRawData` for every section) instead of
   assuming it. It reads a name's virtual address as a file offset, which is
   true of this image and false of PE images in general; the assertion makes the
   dependency explicit and fails loudly rather than resolving fewer names.
2. The `record_lifecycle` doc comment now states the pinned rule (a transition
   that does not end targetability never touches a recorded ending) and names
   the test that pins it.
3. The test's own comment no longer describes the rule as "keeps the first
   transition that ended targetability": the code keeps the **last** terminal
   kind and ignores non-terminal ones. The pinned behavior is unchanged and the
   probe still fails without it; only the description was wrong.

## Reviewer sensitivity probes

Each probe was applied, observed and reverted; the branch is byte-identical to
the pushed commit afterwards.

| # | Probe | Result |
| --- | --- | --- |
| 1 | the crosshair query filters on `present` instead of `eligible` | both F30-D tests **fail** (and F30-A's crosshair test fails too) |
| 2 | the occlusion filter is dropped from the crosshair query | `accept_f30_d_crosshair_selection_needs_eligibility_and_free_sight_together` **fails** |
| 3 | `record_lifecycle` clears the recorded ending for a non-terminal transition | `accept_f30_d_crosshair_selection_needs_eligibility_and_free_sight_together` **fails**; **all 60 F30-A/B/C tests still pass** — the regression this stage found |
| 4 | the eligibility filter is removed from the crosshair query entirely | both F30-D tests **fail** |
| 5 | `ABSENT_NAME_FRAGMENTS` gains a fragment the table does match (`TARGET`) | `accept_f30_d_retail_names_no_reveal_or_assistance_option` **fails** with the sixteen matching names |
| 6 | `nearest_attacker` is reclassified `observed` | `accept_f30_d_retail_names_no_reveal_or_assistance_option` **fails** |
| 7 | the whole F30-D test surface removed | both files fail to compile — the tests call production code, not a test-only implementation |

## Unknowns and fidelity limitations (not guessed, not removed)

Each carries a claim id, the content it gates and the task that resolves it.
They are written into the hashed artifact
`private/evidence/F30-D/targeting-vocabulary.json` (`limitations`), into the
report's own `review.method`, and into this section, so none of them is
removed from machine-readable evidence to turn a validator green. **The report's
`unknowns` list is empty because this stage has no unresolved issue *inside its
own assertions*** — every assertion is a measured file fact or an exercised
production behavior, and all of them pass; the items below are unmeasured
*original behavior*, which the stage records as limitations rather than as
failures of its claims. The report's claim is `implemented`.

| Claim id | Unmeasured original behavior | Gates | Resolving task |
| --- | --- | --- | --- |
| `f30.d.limit.target_order` | the order the original's next/previous cycle walks over its enemy/ally/ground classes, and what "nearest" means there | F30 AC01, non-negotiable 2; the engine orders by distance with an actor-id tie-break | #534 `F30-E` (needs #358), #505 `F22-J` for bindings |
| `f30.d.limit.reveal_rules` | the original's reveal/visibility rule | F30 non-negotiable 1's reveal axis; the engine's `revealed` flag is designed vocabulary | #534 `F30-E` (needs #358) |
| `f30.d.limit.assistance_behavior` | what the three padlock modes and nine directions do, and whether any corrects aim | F30 non-negotiable 3; the engine's `lead_indicator`/`aim_assistance` options are designed and are not presentable as original | #534 `F30-E` (needs #358) |
| `f30.d.limit.bindings` | which physical key fires which target command | F30-B's declared actions bound to `FlightCommand` edges | #505 `F22-J` |

Also still open, and unchanged by this stage:

- **No production occlusion producer.** `CrosshairQuery::occluded` is data an
  authoritative producer supplies and nothing in the repository supplies it
  yet, so AC04's occlusion half is an exercised interface rather than a wired
  one. Filed as #535 `F30-F`.
- **No session schedule** calls `apply_target_consumers` (recorded by F30-C).

## Research boundary

`docs/research/` and `schemas/` are protected and were not touched. The
discoveries worth folding into the research notes are: (a) the target/padlock
name families in `strings.dll` and their ids, and (b) the two measured
absences over the named table. If the owner wants them in
`docs/research/FORMAT-NOTES.md` or `SOURCES.md`, that needs a scoped
exception; this finding states the measurement and edits nothing protected.

## Tests

Task-test prefix `accept_f30_d_`:

- `crates/cs_content/tests/accept_f30_d_original_target_vocabulary.rs`
  - `accept_f30_d_retail_names_the_original_target_action_vocabulary`
    (`#[ignore = "requires CS_GAME_DIR"]`) — both installation fingerprints,
    `strings.dll`'s digest and length, the production `StringCatalog`
    accounting, the 1 023-entry `{name, id}` table and its id range, and every
    measured target / padlock / font label resolved non-empty under its id.
  - `accept_f30_d_retail_names_no_reveal_or_assistance_option`
    (`#[ignore = "requires CS_GAME_DIR"]`) — the two measured absences and the
    action classification's completeness, its evidence columns and the exact
    pair of `absent` rows.
- `crates/cs_sim/tests/accept_f30_d_crosshair_eligibility.rs` (unignored, runs
  in CI) — AC04's conjunction and the reveal rule, as described above.
- `crates/cs_content/tests/evidence_report_f30_d.rs` — the CLI-EVIDENCE harness
  (writes `acceptance.json` and `targeting-vocabulary.json`; not part of the
  acceptance suite).

Both retail tests fail loudly (they panic on a missing `CS_GAME_DIR`) when the
capability is absent; neither can pass without the installation.

## Checks

Run locally by the implementer before hand-over: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --locked -- -D
warnings`, `cargo test --workspace --locked`, and
`cargo test --workspace --locked -- accept_f30_d_ --include-ignored`.

## Claim

At most **implemented** for this branch, and at most **checked** after an
agent review and green CI. Nothing here is `verified_original`: the measured
facts are the original's *names*, ids and counts, and the three behaviors that
would make targeting original are explicitly unmeasured. No agent review
replaces the owner's human approval.

## Review

Reviewer: **bunny-2** (`bunny-2/bunny-2`, Rally #124 review claim of
2026-10-02T20:51Z), workspace
`/Users/linus/coding/rust-crimson-skies/bunny-2`.

**This review is not independent.** The same agent identity both implemented
F30-D and reviewed it, in the same continuous session: the reviewer read the
implementer's own diff, and its memory of the reasoning that produced the
constants above is not fresh. Under AGENTS.md and the owner's 2026-09-28
directive this is **not independent evidence** for format or mission semantics,
and the next reviewer should treat the sensitivity probes below as one
agent's work, not two. What the reviewer did add was a from-scratch
reproduction of the measurement and its own probe runs; see the commit log for
the repairs.

**The measurement was reproduced independently** rather than taken on trust. A
reviewer-written Python re-implementation of the PE `.data` walk (not the Rust
support module, which is under test) re-derived from `$CS_GAME_DIR`: the
`strings.dll` digest `7582feca…` and 131 072-byte length, a 1 023-entry table
with ids 100 … 17 142, **all 27** claimed name/id pairs correct, the target
family exactly the 11 `MSG_CMD_TARGET_*` names the finding lists (no twelfth),
the padlock family exactly 12, and **zero** hits for each of the eight absent
fragments. Every numeric and name claim in Observations 1–4 checked out.

**Reviewer probes.** Probes 1–6 of the implementer's table were re-run and
reproduced. Three further probes were added:

| # | Probe | Result |
| --- | --- | --- |
| R1 | `parse_name_id_table` assumes a name's virtual address is a file offset, and the image does satisfy `VirtualAddress == PointerToRawData` | verified against the image; **fixed in review** by asserting the layout instead of assuming it |
| R2 | a duplicated classification row leaves one declared action unclassified | `accept_f30_d_retail_names_no_reveal_or_assistance_option` **fails** (`nearest_objective is declared but not classified`) — the completeness check discriminates |
| R3 | `record_lifecycle` clears the recorded ending for a non-terminal transition, re-measured after the "Repairs applied in review" edits | `accept_f30_d_crosshair_selection_needs_eligibility_and_free_sight_together` **fails**, and **all 60** F30-A/B/C tests still pass, so the pin still stands after the review's edits |

R3 also corrected a factual error in the implementer's table: probe 3 as first
written recorded **23** F30-A/B/C tests, and the workspace selection contains
**60**. The count was under-reported, so the claim's direction was not
flattered, but it was wrong and is now measured.

**Not verified by this review:** anything requiring an original run. The
limitations in the table above are unchanged and still gate the fidelity
claims they name; the empty `unknowns` list in the report is the reading
argued in the section above, and this reviewer accepts it for the reason given
(the limitations are recorded in machine-readable form in the hashed
`targeting-vocabulary.json`, in `review.method`, and in tasks #534 / #505), not
because the original behaviors were measured.