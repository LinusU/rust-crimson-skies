# F53-A: Mod manifest, override classification and the mount plan

Date: 2026-10-01. Task: F53-A "Define mod manifest and override validation"
(`specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
section `### F53-A`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only (no `CS_GAME_DIR` read, no evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/mods/mod.rs` (new): the set-level contract —
  `ModSet`, `MountRequest` (engine version + base content ids + limits),
  `MountLimits`, `plan_mods`, `ModPlan` (load order, per-mod entries,
  `PrecedenceReport`, `ModModification`, hash), `PrecedenceEntry`,
  `PlannedMod`/`PlannedDependency`, `ModPlanError`/`PlanProblem` with
  `code()` and `Display`, `plan_to_text`, and the synthetic fixtures
  (`synthetic_base_ids`, `synthetic_engine_version`,
  `synthetic_mount_request`, `synthetic_conflicting_mods`,
  `synthetic_tuning_mod`, `synthetic_false_cosmetic_mod`,
  `synthetic_cyclic_mods`, `synthetic_mod_claim`).
- `crates/cs_content/src/mods/manifest.rs` (new): the manifest contract —
  `ModVersion`, `VersionRange`, `EngineRange`, `DependencyStrength`,
  `ModDependency`, `PayloadKind`, `ModPayload`, `ModHeader`, `ModManifest`,
  `ManifestError`.
- `crates/cs_content/src/mods/overrides.rs` (new): the override contract
  and the two classification policies — `OverrideAction`, `ContentOverride`,
  `OverrideEffect`/`classify_effect`/`COSMETIC_CONTENT_KINDS`,
  `OverrideValidation`/`classify_validation`/
  `SANDBOXED_PROGRAM_CONTENT_KINDS`, `ModModification`.
- `crates/cs_content/src/lib.rs` (wiring only): `pub mod mods;` and the
  crate-level doc paragraph.
- This file.

Test count: 22 `accept_f53_a_*` unit tests in (21 as submitted, one added
by review for the action-premise check) in
`crates/cs_content/src/mods/`, selected by
`cargo test --workspace --locked -- accept_f53_a_ --include-ignored`.

**Why unit tests and not `crates/cs_content/tests/accept_f53_a_mods.rs`:**
`crates/cs_content/tests/` is **not** an owner path of this task (the owner
paths are `crates/cs_content/src/mods/`, `crates/cs_assets/src/mods.rs`,
`crates/cs_app/src/ui/mods.rs` and a root `tests/`), and adding a file
outside them would be a Rally-refused protected-path change. `#[cfg(test)]`
modules inside the owner path are production-path code and are discovered by
the same filter every other task uses. The asymmetry against the
`crates/cs_content/tests/accept_f*.rs` convention of F14-A…F59-A is caused by
the owner paths, not by a technical limit; a future stage that owns
`crates/cs_content/tests/` can move them.

**One observable failure:** two mods both `Replace` the base game's
`image/synthetic.hull-panel`. The first (by load order) must be reported as
*shadowed* by the second, with both names and both positions in the
precedence entry, and the plan must be byte-identical when the two manifests
are supplied in the opposite order. A plan that silently drops one claim, or
that resolves the conflict by input order, fails this.

## Semantics defined at this stage

- **Identity.** A mod is a `ModId` — the *same* validated label F04-A's
  `ModStack` and `ResolveContext` use, so a manifest's id and an opted-in mod
  are one identity, not two. Version is a `major.minor.patch` integer triple
  (`ModVersion`); no float can round a version into a compatible one.
- **Ranges.** `VersionRange` is `[min, max]`, upper bound optional;
  `EngineRange` is closed on both ends because F53 non-negotiable 1 requires
  a *declared engine range* to be validated and an unbounded window cannot
  fail. An inverted range is refused at construction.
- **Dependencies.** `Required` and `Conflict` gate enabling; `Optional` is
  recorded and reported but never blocks. A self-dependency is a cycle of
  length one and is refused at manifest construction.
- **Payloads.** `PayloadKind::from_spelling` classifies a shipped file by
  the extension of its last component against a **closed** list (`dll`, `so`,
  `dylib`, `exe`, `com`, `ocx`, `sys`). `ModManifest::try_new` refuses any
  manifest carrying a `NativeLibrary` (F53 non-negotiable 2: no native
  DLL/plugin execution from an original or mod archive). The engine has no
  loader for such a file at all — the classification exists so a refusal can
  name what the manifest asked for.
- **Paths.** Every source spelling is a `cs_types::install::RelativePath`,
  validated at construction: `..`, absolute, drive-prefixed, `.`, empty
  components and NUL are refused (F53 AC02's path half). Nothing in this
  module joins a spelling to a filesystem path; F53-B does that against a
  validated mod root.
- **The two policies, both derived (F53 non-negotiable 2 and 3).**
  `classify_effect` and `classify_validation` are *total* functions from the
  target's `ContentKind` — every variant is matched, with no fall-through
  and no `Unknown` arm, so adding a kind to `ContentKind` is a compile error
  in this module until the policies decide what it means.
  `classify_effect` is the hash policy the sheet demands: an override's
  effect is a property of the id, so a manifest cannot classify itself
  cosmetic. `ModHeader::declared_cosmetic_only` is recorded as the *author's
  claim* and cross-checked by the plan; a mod claiming cosmetic while
  claiming a gameplay id is refused with
  `PlanProblem::CosmeticOnlyMismatch`, naming the gameplay targets.
  The cosmetic set is deliberately small and is the conservative direction:
  a false "cosmetic" would let a gameplay change escape the session/save/
  replay/handshake marking, while a false "gameplay" only costs caution.
  **`ContentKind::Mesh` and `ContentKind::AnimationTrack` are therefore
  *not* cosmetic** — the two kinds this project can already show reaching
  collision or simulation state:

  * a render mesh can be the source of a derived collider
    (`docs/findings/2026-09-23-avian-collider-from-mesh-needs-bevy-asset-
    stack.md`);
  * an F20-A animation clip carries `MarkerEffect::Gameplay` markers that
    move simulation state — mission cues, state transitions, destruction
    triggers (`cs_content::animation`) — **and** its `VisibilityChannel`
    drives `AnimatedNodeState::collider_enabled` in
    `cs_sim::animated_object`, so a clip can enable or disable a collider.

  So a mesh or clip override cannot be *proved* cosmetic. Both were
  **corrected during review**: `AnimationTrack` was in the cosmetic set in the
  first submission, which would have let a gameplay-affecting clip escape the
  session/save/replay/handshake marking that non-negotiable 3 demands.
- **Each override action is checked against what the set provides.**
  `Add` claims an id is new and `Replace` claims there is something to take
  over, so `check_actions` refuses an `Add` of an id the base game already
  provides (`AddOfExistingContent`) and a `Replace` of an id nothing
  provides (`ReplaceOfMissingContent`). `MountRequest::base_ids` was
  accepted but never consulted in the first submission, while
  `overrides::OverrideAction`'s documentation promised exactly this check;
  **both the check and the field's use were added during review**. An id
  claimed both ways is reported as `ActionCollision` alone, so a contested id
  never also collects a premise fault that may not be the real one.
- **Precedence (F53 AC01).** `plan_mods` returns one `PrecedenceEntry` per
  claimed content id, sorted by id, naming the winner (the mod loaded last),
  its position and every shadowed claim with positions. The load order is fed
  straight into `cs_types::asset_id::ModStack`, so "the later mod wins" here
  is literally the rule `cs_assets::vfs` applies when resolving a key — one
  rule, not two. The order is a topological sort of the *required*
  dependency graph with ties broken by mod id, so it depends only on the
  set's contents and never on the order manifests were discovered in.
- **Every problem, not the first.** `plan_mods` collects all
  `PlanProblem`s, sorts and dedups them, and returns them together, so
  fixing one fault does not hide the next. The order is the derived `Ord` of
  the enum, which is stable across runs and input orders.
- **Action collisions and action premises.** One mod `Add`ing and another
  `Replace`ing the same id is a contradiction about whether the base game has
  it, and is refused (`ActionCollision`) rather than resolved by load order.
  Contesting the *same* action is legitimate and is reported as precedence
  instead. Independently, each action is checked against what the mounted set
  provides; see the classification section above.
- **Budgets.** Five integer limits (`max_mods`, `max_dependencies`,
  `max_overrides_per_mod`, `max_declared_bytes_per_mod`,
  `max_declared_bytes_total`), all designed bounds, all overridable through
  `MountRequest::with_limits`. Declared byte sums **saturate**: a manifest
  declaring an absurd total must stay over budget rather than wrap under it.
- **`ModPlan::hash`.** A SHA-256 over the domain separator, the engine
  version, the encoded limits, the load order, each mod's id/version/
  verdict/bytes/dependencies/overrides and the precedence report. It is
  explicitly **not** F53's compatibility signature: a signature has to cover
  the *content bytes* a mod resolves to, which this stage never reads. This
  hash is the input such a signature would be built from.

## Everything this stage does not know, and does not claim

- **No bytes are read.** `ContentOverride::declared_bytes` and
  `ModManifest::declared_bytes` are the *manifest's own statements*. The byte
  budgets are therefore declared-size budgets, and the docs say so at the
  field, at the accessor and in the module header. F53-B measures.
- **No archive is opened and no path is joined.** A `ModPayload` or override
  source is a validated relative spelling with no root. F53-B joins it.
- **`MountRequest::base_ids` is the caller's claim** about what the base
  game provides. F53-B supplies it from the real catalog; nothing here
  verifies it.
- **The original game's mod support is unmeasured.** Whether it existed,
  what a manifest looked like, which content could be overridden, how
  versions and dependencies were expressed and what "cosmetic" meant are all
  open (F53 "Research boundary"). Every label, bound and fixture here is
  newly authored project design with `ClaimStatus::Designed` provenance
  under claim `f53a.synthetic-mod-set`. Nothing here may be cited as
  evidence about the original game, and no `verified_original` state is
  reachable from this module.
- **F53 AC04** ("disable a mod and reopen a dependent save without
  destructive fallback") is not addressed here: it needs the F48-A save
  schema to mark a save's mod dependency set, which is F53-B/D work. Filed
  as a follow-up.
- **Hot reload boundaries** (non-negotiable 5) are runtime behaviour and
  belong to F53-B/C.

## Checks run (all exit 0)

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --locked`
- `cargo test --workspace --locked -- accept_f53_a_ --include-ignored`
  — 22 tests, all in `cs_content`'s lib target

## Sensitivity probes (run and reverted; none committed)

Each probe removes one behavior and re-runs the `accept_f53_a_` selection.

1. `ContentKind::Mesh` moved into the cosmetic set → 1 failure
   (`…effect_policy_is_derived_from_the_target_kind`).
2. The `declared_cosmetic_only` cross-check disabled → 1 failure
   (`…the_cosmetic_only_claim_is_checked_against_the_policy`).
3. Load order replaced by plain alphabetical order → 1 failure after the
   dependency-order assertion was added (see below).
4. The add/replace action-collision check removed → 1 failure
   (`…add_and_replace_of_one_id_collide`).
5. The native-payload refusal removed → 2 failures
   (`…native_payloads_are_classified_and_refused`,
   `…native_payloads_and_unsafe_paths_cannot_reach_a_plan`).
6. Cycle detection removed → 1 failure
   (`…a_cyclic_dependency_is_rejected`).
7. All five budget checks disabled → 1 failure
   (`…budgets_are_enforced`).

### One gap probe 3 found, and the fix

The first version of the load-order assertion passed with the topological
sort replaced by a plain alphabetical ordering, because the AC01 fixture's
required dependency (`synthetic.bright-panels`) happens to sort *before* its
dependent (`synthetic.panel-repaint`). The test asserted the right rule but
could not distinguish it from a wrong one.
`accept_f53_a_two_conflicting_mods_produce_a_deterministic_precedence_report`
now also plans a pair whose dependent id sorts *before* its dependency's
(`synthetic.aaa-user` requires `synthetic.zzz-lib`) and asserts the
dependency order. With that assertion in place, probe 3 fails as it should.

## Noted for the owner, not fixed here

- The stage implements a *policy function* (`plan_mods`) rather than only
  type declarations. This is required by the stage's own minimum scenario
  (AC01 needs something that decides precedence and prints it) and is
  bounded: the plan opens no archive, reads no byte, hashes no content and
  mounts nothing. A reviewer should confirm this split respects
  `docs/TASK-SPLITTING.md`. The natural follow-up split is
  `plan_mods` (F53-A, done) → manifest *reading* and safe mounting (F53-B) →
  selection UI, diagnostics and export (F53-C) → reproducibility evidence
  (F53-D).
- `crates/cs_content/tests/` and the other two owner paths
  (`crates/cs_assets/src/mods.rs`, `crates/cs_app/src/ui/mods.rs`) are
  untouched by this stage; F53-B and F53-C are their natural consumers.

## Review corrections (reviewer pass)

Two defects were found in review and fixed on this branch. Both were
load-bearing policy errors, not style:

1. **`ContentKind::AnimationTrack` was classified `Cosmetic`.** The
   classification policy is the hash policy F53 non-negotiable 3 relies on,
   and its own stated rule is that a kind is cosmetic only when it is
   *provably* free of simulation state. An F20-A clip is not: it carries
   `MarkerEffect::Gameplay` markers (mission cues, state transitions,
   destruction triggers — `cs_content::animation`) and its visibility channel
   drives `AnimatedNodeState::collider_enabled`
   (`cs_sim::animated_object`). A mod replacing a clip could therefore change
   the simulation while the plan reported a cosmetic-only mount and skipped
   the marking non-negotiable 3 requires. `Mesh` was already excluded for the
   same class of reason; `AnimationTrack` had been missed. It was removed from
   the cosmetic set, and the policy test now pins both kinds with the reason.
2. **`MountRequest::base_ids` was accepted and never consulted.**
   `overrides::OverrideAction`'s documentation promised that "`Add` may not
   name an id the base content already provides, and a `Replace` may not name
   an id nothing provides", but no code implemented it and the doc link it
   cited (`super::plan::PlanProblem`) did not resolve to anything. A mod could
   `Add` over base content or `Replace` nothing and still plan cleanly, making
   the mounted result depend on load order rather than on a stated premise.
   Implemented as `AddOfExistingContent` and `ReplaceOfMissingContent`, and
   the broken intra-doc links in the module were repaired.

Sensitivity probes for both corrections were run and reverted: disabling the
premise checks fails
`accept_f53_a_each_action_is_checked_against_what_the_set_provides`, and
returning `AnimationTrack` to the cosmetic set fails
`accept_f53_a_effect_policy_is_derived_from_the_target_kind`.
