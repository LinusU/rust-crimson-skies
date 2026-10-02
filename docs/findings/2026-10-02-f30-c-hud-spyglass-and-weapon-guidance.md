# F30-C: HUD, spyglass and weapon guidance consumers

Date: 2026-10-02. Task: F30-C "Connect HUD, spyglass and weapon guidance"
(`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
section `### F30-C`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence
report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/targeting.rs`: `SelectionClearReason` (new),
  `ClearedSelection` (new), `TargetPhase::cleared` (new field),
  `TargetStore::clear_reason` (new), `TargetStore::present` (new), and the
  `phase` rewrite that derives the clear reason before it prunes.
- `crates/cs_app/src/targeting.rs`: `AssistanceOption`/`AssistanceOptions`
  (new), `LoweredTargetRules::lead_indicator_provenance`/`aim_assistance_provenance`
  (new fields), `TargetingSession::assistance` (new accessor), `ClearedTarget`
  carried through the views, `HudTargetReadout`/`SpyglassTarget`/
  `SpyglassReadout`/`AssistanceOffer`/`GuidanceReadout`/`TargetConsumers`/
  `ConsumerBinding` (new), `apply_target_consumers` and
  `teardown_target_consumers` (new entries).
- `crates/cs_content/src/target_rules.rs`: documentation of the F30-C consumer
  contract only. The declared schema needed no new record: the two assistance
  options already arrive as separate `Resolved<bool>` values carrying their own
  `Provenance`, which is the evidence classification F30 non-negotiable 3 asks
  for.
- `crates/cs_sim/src/lib.rs`, `crates/cs_app/src/lib.rs` (wiring only): module
  documentation.
- `crates/cs_sim/tests/accept_f30_c_phase_clear_reason.rs`,
  `crates/cs_app/tests/accept_f30_c_target_consumers.rs`.
- This file.

**One observable failure:** if the consumer pass publishes the *selection
pass's* phase record instead of deriving its own at the tick it renders, a
target destroyed after the selection pass is still described by the record the
spyglass renders. `accept_f30_c_destroyed_selection_clears_before_the_spyglass_view`
destroys the selected actor with the **real** `DamageResolver` *after*
`apply_selection_edges` published a phase record and *before* the consumer
pass, and then asserts that the spyglass view, the reticle and the guidance are
all empty and that the view reports the clear with its reason
(`SelectionClearReason::Ended(Destroyed)`). A `last_phase()`-based
implementation renders the destroyed target and fails.

## Wiring boundary, and why there is no plugin here

F30-C owns the **consumer** half of the path F30-B wired on the producer half.
`crates/cs_app/src/ui/hud/mod.rs` (F46), `crates/cs_app/src/camera` (F21) and
`crates/cs_app/src/weapons.rs` (F27) are **not** F30-C owner paths, so this
stage does not edit them; it publishes the records those consumers read, and
the acceptance tests are the consumers. Three existing tasks depend on exactly
this contract and own the consuming code: #115 (F21-B, spyglass rigs), #118
(F27-B, gun cadence) and #196 (F46-B, HUD gauges and target display).

The consumer entry keeps the shape of the three F30-B entries (an exclusive
`&mut World` function plus a report), rather than a Bevy `Plugin`. A schedule
that drives the producers and this pass together is a session-wiring decision
that no owner path here owns; it is recorded as a follow-up below rather than
guessed.

## Semantics defined at this stage

- **The consumer pass derives its own phase record.** `apply_target_consumers`
  calls [`TargetStore::phase`] itself, at the tick the consumers render, after
  the tick's roster sync, command edges and damage tick have run. That is what
  makes AC03 true at the render boundary rather than only at the selection
  boundary.
- **Clear reasons are a roster fact, not a UI rule.**
  `SelectionClearReason` lives in `cs_sim::targeting` because "why did this
  actor stop being selectable" is a statement about the store: a lifecycle
  transition, a reveal that went false, a script phase that closed, or an
  entity that left the world. `TargetPhase::cleared` carries it, so a consumer
  learns the reason in the same single read as the reticle.
- **`present` is not `eligible`.** `TargetStore::present` answers "is this actor
  still in the world" (registered and no lifecycle transition ended its
  targetability). The HUD's threat list drops cues whose attacker is not
  `present`, because a destroyed airframe cannot threaten anyone, and counts
  the dropped cues in `HudTargetReadout::withdrawn` rather than hiding them.
  It deliberately does **not** drop a cue for an attacker that merely lost
  sensor contact: a warning is exactly what a contact you cannot see is worth.
- **Guidance publishes eligibility, never a correction.** The store answers
  whether an aid *may* apply here (`WeaponGuidance`: the selected actor, its
  canonical position, the bearing toward it, its distance and the two live
  verdicts), and the two declared options travel beside it as two separate
  `AssistanceOffer`s on `GuidanceReadout`, each with the declared option's
  `Provenance`. The two facts stay separate on purpose: `GuidanceReadout::aid`
  is `None` only when the store *refused* the query (nothing selected, the target
  is not a declared hostile, or the query could not be answered), never because
  an option is off, and `offers_lead_indicator()` / `offers_aim_assistance()` are
  what join "declared on and presentable" to "a target to apply it to". An aid
  is offered for a **declared hostile** only: a friendly, neutral or undeclared
  pair is refused by the store and reported as
  `GuidanceWithheld::NotHostile`. No lead point, aim offset, correction
  magnitude or hit path exists in any of the records: targeting holds no target
  velocity and no measured ballistics, so a lead point computed here would be a
  fabricated value (AGENTS: unknown means unknown). The weapon path that owns
  muzzle velocity and projectiles (F27-B) draws the point; this stage only says
  whether it may.
- **Teardown is generation-qualified.** `TargetConsumers` is bound to
  `(session, observer)`. A pass for a different observer rebinds and reports
  `rebound`, dropping the previous binding's target instead of carrying it
  across an aircraft swap; an `ActorId` is generation-qualified, so a replaced
  session under the same serial rebinds as well. `teardown_target_consumers(world,
  session)` clears the views only when the bound session is the one tearing
  down, so a late teardown for a previous generation cannot clear a live view.
- **Errors leave nothing stale.** A pass that cannot derive a record (no session
  at all, or an observer this session's store does not hold) unbinds the views
  before returning the error, so a consumer that reads the resource after a
  failed pass finds nothing to render. The `NoSession` case matters on its own:
  the views outlive the `TargetingSession` resource that derived them, so a
  refused pass has to take them down with it. The next successful pass
  republishes — the retry path is the same code path as the first pass.

## Unknowns recorded (not guessed)

- Whether the original HUD draws a target box, a lead pip, a threat cue per
  attacker or per direction, and what the original spyglass does with a
  selected target, are unmeasured (F30-D's retail stage; F21-D for the views
  themselves). Nothing here claims `verified_original`; every fixture value is
  designed and carries `Origin::SyntheticFixture` with designed provenance.
- Whether the original game offers a lead indicator or aim assistance at all,
  and whether either is bound to a key or a difficulty option, is unmeasured.
  The fixture's `lead_indicator: true` / `aim_assistance: false` is project
  design; the guidance record carries the `ClaimStatus` so a consumer can see
  exactly that.
- Whether the original warning list drops an attacker that lost sensor contact
  is unmeasured; the designed rule keeps the cue and drops only actors that
  left the world.
- Whether a bailed-out or captured actor keeps a threat cue is the F30-A
  contract's decision (both keep targetability and their ledger evidence).
- The original cycle order, crosshair cone, reveal rules and assistance
  behavior remain F30-D unknowns, unchanged by this stage.

## Defects found and fixed while writing the acceptance tests

1. **`teardown_target_consumers` reported work it had not done.** With nothing
   published, `consumers.bound()` is `None`, so the "refuse a foreign
   generation" guard fell through and the function cleared an already-empty
   record while returning `true` — telling a caller that a live view had been
   torn down when nothing was up. It now returns `false` when no view is bound
   for the session. Discriminated by
   `accept_f30_c_teardown_clears_its_own_generation_only`.
2. **A failed pass cleared the previous observer's selection.** The rebind
   guard dropped the held selection before deriving the phase, so a pass for an
   observer the store does not hold destroyed the live observer's target on its
   way to reporting the error. The clear is now guarded by
   `store.is_registered(&observer)`: a rebind to an unknown observer is refused
   without having mutated anything. Discriminated by
   `accept_f30_c_rebind_and_failed_pass_leave_no_stale_view`.
3. **The store's guidance query answered for an ally.** The first version
   returned a `WeaponGuidance` carrying `hostile: false` and left the gate to
   each consumer, which meant a consumer that checked only `aid.is_some()`
   would offer an aid toward a captured aircraft. The gate now lives in the
   roster fact: `TargetStore::guidance` refuses with
   `TargetError::NotHostile { actor, allegiance }`, carrying the allegiance
   (or its absence) that produced the refusal, and the consumer view reports it
   as `GuidanceWithheld::NotHostile`. Discriminated by
   `accept_f30_c_capture_updates_all_three_views_in_one_pass`,
   `accept_f30_c_crosshair_selection_reaches_the_views` (which selects the
   wingman through the real crosshair edge) and the sim-layer
   `accept_f30_c_guidance_is_offered_to_a_declared_hostile_and_refused_otherwise`.

## Reviewer sensitivity probes

The implementer ran these probes before handing over. Each was reverted
afterwards; the branch is byte-identical to the pushed commit afterwards.

| Probe | Result |
| --- | --- |
| 1. `apply_target_consumers` publishes the previously published phase record instead of deriving its own (the "reuse `last_phase`" implementation) | `accept_f30_c_destroyed_selection_clears_before_the_spyglass_view`, `accept_f30_c_capture_updates_all_three_views_in_one_pass`, `accept_f30_c_reveal_and_phase_changes_clear_the_views_with_their_reason`, `accept_f30_c_rebind_and_failed_pass_leave_no_stale_view` and `accept_f30_c_session_runs_the_lowered_rules_not_fixture_constants` **failed** |
| 2. the HUD threat split uses `eligible` instead of `present` | `accept_f30_c_hud_threat_list_withdraws_cues_whose_attacker_left_the_world` **failed** (the unrevealed attacker's cue was withdrawn) |
| 3. the guidance gate refuses only `Neutral`, letting a friendly and an undeclared pair through | `accept_f30_c_capture_updates_all_three_views_in_one_pass`, `accept_f30_c_crosshair_selection_reaches_the_views` and the sim-layer `accept_f30_c_guidance_is_offered_to_a_declared_hostile_and_refused_otherwise` **failed** |
| 4. `SelectionClearReason` order changed (reveal before lifecycle) | `accept_f30_c_each_ineligibility_has_its_own_reason` **failed** |
| 5. `teardown_target_consumers` drops its generation guard | `accept_f30_c_teardown_clears_its_own_generation_only` **failed** |
| 6. `AssistanceOption::presentable` returns `enabled` alone, ignoring the evidence class | `accept_f30_c_assistance_options_carry_their_own_evidence` and `accept_f30_c_three_views_are_one_read_of_the_roster` **failed** |
| 7. the whole F30-C production path removed from `cs_sim::targeting` and `cs_app::targeting` | both acceptance files fail to compile — the tests call production code, not a parallel test-only implementation |

## Known limits left for later stages

- **No schedule wiring.** `apply_target_consumers` is an exclusive `&mut World`
  entry like the three F30-B producer entries, so nothing drives it yet from a
  fixed tick. A session that owns the targeting resources must call it once per
  rendered frame after that frame's roster sync, selection edges and damage
  tick. Filed as a follow-up task; the consumer stages that own the rendering
  own the call site.
- **`HudTargetReadout::withdrawn` is a reporting field, not a state.** Nothing
  consumes it yet; F46-B decides whether the HUD shows anything for a
  withdrawn cue or only stops drawing it.
- **A per-observer session is not modelled.** `TargetingSession` owns **one**
  `TargetSelection`, so a pass for a different observer rebinds and clears the
  selection rather than keeping one selection per aircraft. That matches a
  single-player session; a split-screen or an AI-driven observer would need
  per-observer selection state, which is a session-wiring decision no owner
  path here owns.
- **`apply_selection_edges` does not consult the consumers' binding.** The
  rebind clear lives in `apply_target_consumers`; a caller that runs only the
  selection pass and never the consumer pass has no rebind behaviour at all.
- **The original warning-list rule is unmeasured.** Keeping the cue for an
  attacker that merely lost sensor contact is the designed rule here; whether
  the original HUD does the same is F30-D's to measure.

## Review pass (bunny-2, 2026-10-02)

The reviewer of this branch is the same agent identity (`bunny-2`) that
implemented it, but in a **fresh context**: it read the pushed commit, the sheet,
the contract and the surrounding modules from scratch and shares no state with
the implementing session. That is weaker than the independent review AGENTS.md
prefers for format and mission semantics, so the notes below are recorded as a
self-review with fresh context, not as independent evidence. F30-C claims no
original behavior, so nothing here rests on it.

### Defect found in review and fixed

**A pass refused for `NoSession` left the previous session's views published.**
`apply_target_consumers` borrowed the `TargetingSession`, so it could not call the
`&mut World` unbind helper from inside that borrow and instead returned
`Err(NoSession)` directly — while its own `# Errors` contract and the "Errors
leave nothing stale" section above both promised that *every* refusal unbinds
first. The observable failure: a world whose `TargetingSession` resource is
removed without a `teardown_target_consumers` call (a mission teardown, a plugin
reload, a restart path) keeps the last frame's target bound and visible, and the
next consumer pass returns an error while leaving it up. This is the exact
stale-state failure AC03 and the sheet's "clears safely" criterion are about, so
it is a real defect and not a documentation nit.

The fix restructures the entry so both refusals are *collected* inside the
borrow and handled by one unbind after it, which removes the special case rather
than adding a second one. Discriminated by the new
`accept_f30_c_a_vanished_session_leaves_no_published_view`, which fails against
the pushed implementation (probe 1 below).

### Documentation corrected in review

The file named three symbols and one behavior that the code does not have:
`lead_indicator_evidence`/`aim_assistance_evidence` (the fields are
`lead_indicator_provenance`/`aim_assistance_provenance`), `ConsumerViews` (the
resource is `TargetConsumers`), and a `WeaponGuidance` carrying two
`AssistanceOffer`s with a hostility gate in `offered()` (the offers are on
`GuidanceReadout`, and `offered()` is evidence-class only). The prose now
describes what the code does. No production behavior changed for any of these.

### Reviewer probes (each applied, observed, reverted)

| Probe | Result |
| --- | --- |
| 1. restore the `NoSession` early `return` that skips the unbind | `accept_f30_c_a_vanished_session_leaves_no_published_view` **failed**; the other 26 task tests passed, so this defect was previously uncovered |
| 2. `apply_target_consumers` gates `aid` on the declared assistance options (`aid.filter(\|_\| lead_indicator.enabled \|\| aim_assistance.enabled)`) | `accept_f30_c_an_aid_is_the_store_verdict_not_the_declared_option` **failed**, confirming the store's verdict and the declared option are separate facts |
| 3. compare the rebind on `bound.session` as well as `bound.observer` | no test changed: an `ActorId` is generation-qualified, so the observer comparison already covers a replaced session. The redundant comparison was **removed** rather than kept, and the cross-generation case is now pinned by `accept_f30_c_a_new_session_generation_rebinds_the_views` |

Probe 3 is a note about a change the reviewer decided *not* to keep: the extra
session comparison was dead weight, and the honest form of the claim is the
comment now on `rebound`.

### Reviewer checks

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings`, `cargo test --workspace --locked` and
`cargo test --workspace --locked -- accept_f30_c_ --include-ignored` (27 task
tests: 18 in `cs_app`, 9 in `cs_sim`) all pass on the reviewed commit. CI green
on the same commit.

## Not claimed

No original-data verification, no rendered reticle/lead pip/spyglass image, no
weapon integration and no schedule wiring. The task awards at most **checked**
status.