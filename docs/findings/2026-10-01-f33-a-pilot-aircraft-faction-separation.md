# F33-A: Define pilot/aircraft/faction separation

Date: 2026-10-01. Task: F33-A "Define pilot/aircraft/faction separation"
(`specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`, section
`### F33-A`). Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/pilots.rs` (new): the declared,
  provenance-carrying half — `DeclaredRoster` (subject, `Origin`, player
  faction, pilots, wingmates, neutral traffic, `Provenance`),
  `DeclaredPilot`, `DeclaredWingmate`/`WingmateSlot`,
  `DeclaredNeutralTraffic`, `DeclaredSurvivability`, the
  `RosterSchemaError` validator and the minimal synthetic fixture
  (`declared_synthetic_roster`).
- `crates/cs_sim/src/allies.rs` (new): the runtime contract — the
  per-session `AlliesRoster`, `AllyRecord`, the distinct identity newtypes
  `PilotId`/`FactionId`/`GeometryId` (plus the existing
  `cs_sim::damage::ActorId`), `IdentityError`, `SurvivabilityPolicy`,
  `WingmateSlot`/`WingmateAssignment`, the `Capture` record and the
  `AlliesError` refusals, with the synthetic fixture
  (`synthetic_ally_roster`, `synthetic_wingmates`, `synthetic_pilot`,
  `synthetic_faction`, `synthetic_geometry`).
- `crates/cs_app/src/roster.rs` (new): the lowering boundary —
  `lower_roster`, `LoweredRoster`/`LoweredPilot`/`NeutralTraffic`,
  `RosterLowerError`, and the generation-stamped `RosterBinding` ECS
  component.
- `crates/cs_content/src/lib.rs`, `crates/cs_sim/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): module declaration and the
  crate-level doc paragraph.
- `crates/cs_app/tests/accept_f33_a_roster_boundary.rs` (new): the
  cross-crate acceptance scenario, which drives `cs_content::pilots`,
  `cs_sim::allies` and `cs_app::roster` together. `crates/cs_app/tests/` is
  this task's owner path (`tests/`); `cs_content/tests/` and
  `cs_sim/tests/` are not, so the declared and runtime halves are also
  tested by `#[cfg(test)]` tests inside the owner source files, named
  `accept_f33_a_*` and selected by the same filter.
- This file.

**One observable failure:** before this stage `cs_sim` had no per-actor
identity record and no capture transaction. `cs_types::content::ContentId`
is a single kind-tagged type, so at the call site a faction id, an airframe
id and a pilot id were all just `ContentId` and nothing stopped a capture
from writing the new faction over the vehicle's identity. Concretely, a
capture implemented as one shared "appearance/identity" field changes the
mesh: probe 1 below makes `AlliesRoster::capture` rewrite `geometry` and
`accept_f33_a_capture_changes_faction_without_changing_geometry` fails,
which is exactly AC01. The design fixes this by giving geometry its own
typed field with **no setter** — `capture` only ever moves `faction` — so
the failure mode is unreachable rather than merely untested.

## Test counts

17 `accept_f33_a_*` tests, all passing:
`cargo test --workspace --locked -- accept_f33_a_ --include-ignored`

- 3 unit tests in `crates/cs_content/src/pilots.rs` (the declared schema).
- 5 unit tests in `crates/cs_sim/src/allies.rs` (the runtime roster).
- 4 unit tests in `crates/cs_app/src/roster.rs` (the lowering boundary).
- 5 integration tests in `crates/cs_app/tests/accept_f33_a_roster_boundary.rs`
  (the AC01 scenario end to end).

Every test calls production code; removing a layer fails to compile or
fails the test (see the probes).

## Semantics defined at this stage

- **Three identities, not one.** `PilotId`, `FactionId` and `GeometryId`
  are distinct types and `ActorId` is a fourth. Each constructor validates
  its catalog namespace (`pilot`, `faction`, `airframe`/`mesh`), so a
  cross-wired content record is refused at construction. The type system —
  not a convention — keeps "who flies it", "who it belongs to", "what it is
  built from" and "which session actor it is" apart (deliverable;
  non-negotiable 1).
- **Geometry is its own field with no setter.** `AllyRecord.geometry` is
  changed by no operation at all; `capture` and `set_faction` move only
  `faction` and `set_pilot` only `pilot`. AC01 ("a captured vehicle changes
  faction without changing its geometry id") is therefore a property of the
  available mutations, not of one test's assertions.
- **Capture is the ownership transaction.** `AlliesRoster::capture(actor,
  new_faction)` returns a `Capture { actor, previous_faction, new_faction,
  geometry }` where `geometry` is by construction the unchanged one.
  Publishing the previous and new faction *and the unchanged geometry* as
  its own record lets mission logic react to a capture separately from a
  destruction (non-negotiable 4); wiring it to the damage lifecycle and
  mission callbacks is F33-C. Capturing to the faction the actor already
  belongs to is an idempotent no-op capture, not an error.
- **Destruction, capture and bailout are separate.** The F29
  `LifecycleKind` vocabulary already owns death and bailout; `capture` is
  the ownership change only, so "an ally died" and "an ally joined us"
  cannot collapse into one event.
- **Pilot identity is not the airframe.** `set_pilot` rebinds the pilot an
  existing actor flies while the actor keeps its own geometry and faction
  (non-negotiable 3); the atomic A→B swap that creates the destination and
  releases the source is F33-C. `accept_f33_a_pilot_identity_survives_an_aircraft_swap`
  shows two actors with different geometry ids sharing one pilot.
- **Survivability is an explicit three-label policy.** `SurvivabilityPolicy`
  (and its declared mirror `DeclaredSurvivability`) is `Mortal`,
  `ProtectedNeutral` or `ScriptedInvulnerable`, so "an ally" is not one
  behavior and not every ally is an immortal escort (non-negotiable 4).
- **Unknowns refuse at the boundary; they are never defaulted.** A declared
  voice and a declared survivability are `Resolved<T>`. `lower_roster`
  returns `RosterLowerError::UnknownVoice`/`UnknownSurvivability` naming the
  field and the claim the unknown is recorded under — an unmeasured voice
  never becomes a random line (non-negotiable 5) and an unmeasured
  survivability never becomes a silent mortal. This is the half of
  `accept_f33_a_unknowns_refuse_to_lower` in `cs_app`; the declared half
  (`cs_content`) proves the unknown stays an explicit unknown at
  declaration.
- **An omitted neutral list is empty, not global.** `DeclaredRoster` stores
  the authored `neutral_traffic` list verbatim and `lower_roster` lowers it
  verbatim: a mission that authors none produces an empty list, never a
  population default (AC04's contract half).
- **Session generations are a hard boundary.** Every `AlliesRoster`
  operation refuses an actor from another session
  (`AlliesError::ForeignSession`), and `RosterBinding` is stamped with the
  `SceneGeneration` that spawned it, so a reload cannot leave a stale
  binding looking live (STATE-TRANSACTIONS).
- **The wingmate store refuses a half-applied assignment.** `assign_wingmates`
  validates the whole set before touching the previous one: two assignments
  sharing a slot return `AlliesError::DuplicateWingmateSlot` and leave the
  existing set intact. The *rules* that decide which pilot/loadout a
  briefing selects and how a retry resets them are F33-B; this is the typed
  store they write through.
- **Crate boundaries.** `cs_sim::allies` depends only on `cs_types` and
  `cs_script` (no Bevy, no renderer, no file access); `cs_content::pilots`
  cannot see `cs_sim`; `cs_app::roster` is the only lowering boundary.

## Unknowns recorded (not guessed)

- Which pilots, factions, aircraft, loadouts and voices the original game
  has, how it assigns wingmates, which allies it makes unkillable, what a
  neutral faction is and where any of that is stored are **unmeasured**.
  F13 locates mission programs but recovers no roster; F33-D is the retail
  stage. Every value, slot grammar, survivability label and fixture here is
  newly authored project design carrying `Origin::SyntheticFixture`
  provenance.
- Whether the original capture is a persistent faction change, a scripted
  objective transition or a temporary possession, and whether it touches the
  pilot or the loadout, is unknown and recorded, not assumed. This stage
  defines the record a capture moves; F33-B/C/D decide how it is triggered
  and shown.
- The voice is carried as a catalog `ContentId`; how a voice id maps to
  audio and subtitle files is the F21/F22 catalogue's, not this stage's.
- The wingmate assignment rules (briefing selection, retry reset) belong to
  F33-B; the ECS/AI/dialogue/mission-callback wiring belongs to F33-C; the
  retail roster and relation verification belongs to F33-D.

## Not claimed

No original-data verification, no ECS wiring beyond the binding record, no
assignment rules, no capture UI, no dialogue playback, no mission-callback
wiring, no retail roster. The task awards at most **checked** status.

## Sensitivity probes (run and reverted; no probe committed)

Each probe removes one load-bearing rule and names the test that caught it.
All were reverted; the committed tree is the green one.

1. `AlliesRoster::capture` also rewrites `record.geometry` (enemy status
   baked into the mesh) → `accept_f33_a_capture_changes_faction_without_changing_geometry`
   fails: the captured vehicle's geometry id changed.
2. `GeometryId::try_new`'s namespace check disabled (a pilot accepted as
   geometry) → `accept_f33_a_identity_ids_refuse_the_wrong_namespace` fails.
3. `DeclaredRoster::try_new`'s duplicate wingmate-slot check disabled (the
   last assignment would silently win) →
   `accept_f33_a_declared_roster_refuses_duplicate_slots_and_undeclared_pilots`
   fails.
4. `lower_roster` gives an unknown voice a fallback id instead of refusing →
   `accept_f33_a_unknowns_refuse_to_lower` fails (in both `cs_app` targets).
5. `lower_roster` synthesizes a neutral actor when the declared list is
   empty (a global population default) →
   `accept_f33_a_omitted_neutral_traffic_lowers_to_nothing` fails.

## Commands run (all exit 0)

- `cargo fmt --all -- --check` → `FMT_OK`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  → 0 warnings
- `cargo test --workspace --locked` → 2053 passed, 0 failed
- `cargo test --workspace --locked -- accept_f33_a_ --include-ignored`
  → 17 passed, 0 failed (3 in `cs_content`'s lib, 5 in `cs_sim`'s lib,
  4 in `cs_app`'s lib, 5 in `cs_app`'s `accept_f33_a_roster_boundary`
  integration target)

## Review (same session as the implementation — not independent)

The same agent that wrote this stage reviewed it, so this section is a
self-review, **not** independent evidence and not a substitute for the
owner's review. What it did: re-derived the AC01 scenario by hand, re-ran
every probe above, and checked the test selection actually exercises
production code.

### Fixed during review

1. The declared `DeclaredWingmate` does not re-validate the `pilot`
   namespace itself; `DeclaredRoster::try_new` owns the pilot list and
   cross-references it. A wingmate naming a pilot of the wrong kind is
   therefore reported as `WingmatePilotKindMismatch`, and one naming an
   undeclared pilot as `UndeclaredWingmatePilot`; both are asserted.
2. `assign_wingmates` previously reported the wrong slot on a duplicate
   (the second assignment's slot rather than the repeated one); it now
   reports the slot directly, and the failure case is asserted.

### Review limitation

Because a `cs_sim` test cannot reach `cs_content` and `cs_content/tests/`
is not an owner path, the declared and runtime halves are also covered by
in-source `#[cfg(test)]` tests rather than `crates/*/tests/accept_f33_a_*.rs`.
The tests are real and selected by the same filter. Future "define the X
schema" tasks should name the per-crate `tests/` paths.
