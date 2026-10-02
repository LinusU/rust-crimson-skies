# F21-A catalog kind: the canonical namespace of a declared camera mode set

Date: 2026-10-02. Task: #431 `F21-A-CATALOG-KIND` "Decide the canonical
catalog namespace for camera mode records". Follows F21-A
(`specs/F21-cameras-cockpit-views-and-spyglass.md`, stage `### F21-A`).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no evidence report, no GPU.

## The question

F21-A defined `cs_content::cameras::DeclaredCameraModes` — one owner's set of
declared view modes — and gave it a subject in the existing
`cs_types::content::ContentKind::CameraTrack` namespace, because the canonical
catalog had no camera-*mode* namespace and `CameraTrack` was the only
camera-ish variant. F21-A recorded the reuse as a stopgap and filed #431.

The catalog contract owns which namespaces exist and what each addresses
(`IDENTITY-CONTENT`, "Required catalog collections"), so this is a contract
decision, not an F21 implementation detail.

## Decision

**A declared camera mode set has no catalog namespace of its own. It is a
*subordinate* record, addressed inside the catalog element that owns the
camera, and the only kinds that may own one are:**

| owner | `ContentKind` | what it owns | why |
| --- | --- | --- | --- |
| the aircraft a session flies | `Airframe` | the views that aircraft offers, including whether a `cockpit` mode exists at all | F21 non-negotiable behavior 1: "Cockpit viewpoint comes from verified model/config bindings" — a cockpit projection is authored *against an aircraft*, and an aircraft with no verified cockpit binding declares no cockpit mode |
| the launchable content a session starts from | every kind `ContentKind::is_launchable()` accepts: `Mission`, `IaScenario`, `MultiplayerScenario` | the view a session begins in | the set's `default_mode` is a property of *starting* something; expressing the launchable half as `is_launchable()` instead of a restated list keeps the owner vocabulary from drifting from the launchable baseline the catalog already measures readiness over |

So the id scheme is: **a mode set is addressed by its owner's `ContentId`** —
it has no id of its own, and nothing in the record gives it an identity
separate from the element that owns the camera. The production rule is one
total function over `ContentKind::ALL`:

```rust
// crates/cs_content/src/cameras.rs
pub const fn owns_camera_modes(kind: ContentKind) -> bool
```

`DeclaredCameraModes::try_new` refuses any other subject with
`CameraModesError::SubjectKindMismatch`.

This is the same shape the workspace already uses for a record the catalog
has no namespace for: `cs_content::world::SectorId` /
`WorldObjectId` are subordinate identities inside a `WorldDefinition`, and
`cs_content::environment::EnvironmentId` is "the identity of one authored
environment, addressed inside a world or scenario". Their module docs give
the reason in one sentence — inventing a kind "would claim a namespace
`IDENTITY-CONTENT` does not reserve" — and that reason is the whole argument
here. Sibling questions #400 (sectors, world-object instances) and #407
(environment records) are the same decision for other records and are still
open; this task decides only the camera one.

## Why the catalog was not changed

1. **`IDENTITY-CONTENT` reserves no camera-mode collection.** Its "Required
   catalog collections" list ends at "legacy custom-plane resources"; camera
   modes are not in it. `ContentKind`'s own module doc
   (`crates/cs_types/src/content.rs`) defines the enum as "the union of the
   catalog collections the `IDENTITY-CONTENT` contract requires … and the
   collections spec F14 names in its deliverable". A `camera_mode` variant
   would be in neither source, so it would make that doc false and would
   claim a namespace the canonical contract does not list. The contract is
   owner-authored and protected: this task cannot amend it
   (`allowProtectedChanges` is false), and the owner is the only party who
   can.
2. **A new kind is not a local change.** `ContentKind` is total in several
   places: `ContentKind::ALL`, `label`, `from_label`, and two functions in
   `cs_content::mods::overrides` — `classify_effect`, documented as "a total
   function over `ContentKind::ALL` — there is no fall-through and no
   `Unknown` arm", and `classify_validation`, documented as "Total over
   [`ContentKind::ALL`] for the same reason". A new variant needs a new arm in
   both, plus a decision about which list it belongs in
   (`COSMETIC_CONTENT_KINDS`, `SANDBOXED_PROGRAM_CONTENT_KINDS`) and about
   every other exhaustive `match` over kinds. A namespace claimed without the
   contract amendment would leave those classifications undefined.

## Why `CameraTrack` was rejected (the stopgap is not correct)

`camera_track` already has a different, live owner in this workspace:
**an authored in-engine camera sequence.** `cs_content::cinematics`
(F40-A) names a cinematic's `DeclaredPresentation::InEngine` resource with a
`ContentKind::CameraTrack` id, requires that resource to be a camera track
(`require_kind(camera_track, ContentKind::CameraTrack)`), and accepts a
camera track as a *cinematic subject* (`subject.kind()` must be
`ContentKind::Video | ContentKind::CameraTrack`).

Three consequences, all in the tree today:

1. **One namespace, two unrelated records.** The F21-A fixture's id was
   `camera_track/synthetic.camera-modes`: a player's view list stored under
   the same namespace, and with the same `kind`, as `camera_track/<intro
   flyby>`. A catalog element carries `dependencies`, `parse_state`,
   `readiness` and `runtime_consumers`; a view set and a keyframed sequence
   have none of those in common, so dependency closure and readiness would
   have to answer for one id meaning either.
2. **`camera_track` is classified cosmetic.** `cs_content::mods::overrides`
   puts `ContentKind::CameraTrack` in `COSMETIC_CONTENT_KINDS` and returns
   `OverrideEffect::Cosmetic` for it, and the doc is explicit: "A mod that
   overrides nothing else is a cosmetic-only mod, and its mount need not mark
   sessions, saves, replays or handshakes for gameplay reasons." A player's
   view set — its projection, its default view, whether the spyglass exists
   — is gameplay state. Hiding it in the `camera_track` namespace would let
   a cosmetic-only mod change the camera a session runs under without
   marking anything. Under the decision, a mode set is owned by an
   `Airframe` or a launchable kind, all of which `classify_effect` already
   classifies as gameplay.
3. **A track is not an owner.** A camera track is *referenced* by a mode: a
   `CameraModeKind::AuthoredSequence` mode is driven by one. F21-C will
   reference it as a `Resolved<ContentId>` of kind `CameraTrack` **inside the
   mode**, which is a dependency edge, not the set's identity. Making the
   set *be* a track inverts that.

## What changed

* `crates/cs_content/src/cameras.rs`
  * new `owns_camera_modes` — the total owner rule, documented with both
    roles and the reason `CameraTrack` is not one;
  * `DeclaredCameraModes::try_new` validates against that rule;
    `CameraModesError::SubjectKindMismatch`'s message now names the two
    roles instead of "the camera namespace";
  * the module doc gains "The mode set's namespace: a subordinate record,
    not a catalog kind" with the two recorded reasons;
  * new `SYNTHETIC_CAMERA_MODE_OWNER_KEY` (`"synthetic.camera-plane"`); the
    fixture's owner is now `airframe/synthetic.camera-plane` instead of
    `camera_track/synthetic.camera-modes`, because under this decision the
    id names an aircraft, and a key reading "camera-modes" would claim such
    an aircraft exists.
* `crates/cs_app/tests/camera/records.rs`: the fixture and the set-rule tests
  assert the airframe owner; new
  `accept_f21_a_catalog_kind_mode_set_owner_vocabulary_is_an_airframe_or_launchable_content`.
* `crates/cs_app/tests/camera/modes.rs`: new
  `accept_f21_a_catalog_kind_a_mode_set_lowers_for_every_owner_kind`.
* `crates/cs_types/src/content.rs` and `docs/contracts/IDENTITY-CONTENT.md`:
  **unchanged.** The decision is precisely that the catalog does not change.
* No runtime consumer changes: `cs_app::camera::lower_camera_modes` never
  inspected the owner's kind, and still does not.

## What this decision deliberately does not decide

* **Precedence when both owners declare a set** (an airframe offers a
  default view *and* a scenario starts in one). That is a resolution policy
  for the camera path, i.e. F21-B's wiring, not an identity question. It is
  recorded here as an open item, not decided and not coded.
* **How a session looks a set up** (registration, lookup, catalog rows).
  Because a mode set is subordinate, it is not a catalog element and gets no
  row of its own; the owner is the catalog element. Whether *subordinate
  records should be catalog rows* is the general question in #400 and #407.
* **Which original data, if any, maps onto either owner.** See below.

## Unknowns (recorded, not guessed)

No original bytes were read for this task; `CS_GAME_DIR` was not opened and
`CS_CAPABILITIES` was not needed. The whole scheme is **designed engine
contract**, and the following stay unknown:

| unknown | evidence | resolves in |
| --- | --- | --- |
| the original PC view list, its names, its default view, and whether views are per-aircraft at all | no original data read; `CameraModeKind` is a designed four-value vocabulary and the owner vocabulary is designed from F21 behavior 1, not measured | F21-D (`retail` + `gpu`) and any F21 import stage |
| whether the original declares one view set per aircraft, per mission, or one global list | nothing in the tree reads an original camera record; the decision therefore offers *both* owners and defers precedence to F21-B | F21-D, then F21-B |
| which original file a mode set would come from, if any | `IDENTITY-CONTENT` lookup forbids filename guessing; no member was opened | the F21 import stage, once a member is evidenced |
| whether `ContentKind::CameraTrack` is itself the right namespace for authored sequences | F40-A uses it and this task did not audit F40-A's choice | F40's own catalog-kind question, if the owner opens one |

## If the owner decides a first-class namespace instead

The reversal is small and localized, because the rule is one function: the
owner amends `IDENTITY-CONTENT`'s "Required catalog collections" to include
camera mode sets, adds `ContentKind::CameraModeSet` (with `ALL`, `label`,
`from_label`, a `classify_effect` arm — gameplay, since it changes the
camera a session runs under — and a `classify_validation` arm), then
`owns_camera_modes` and the two tests above change and `DeclaredCameraModes`
gains an id of its own. Until the contract lists such a collection, the
subordinate scheme keeps the workspace inside the contract it is written
against.

## Test sensitivity

One mutation, applied and reverted, against production code: `owns_camera_modes`
replaced by the pre-decision rule `matches!(kind, ContentKind::CameraTrack)`.
`cargo test -p cs_app --test camera -- accept_f21_a` → **9 of 15 failed**,
including both new
`accept_f21_a_catalog_kind_*` tests ("airframe: the launchable half of the
vocabulary must stay expressed as is_launchable"), the fixture test (its
airframe owner is refused) and the two pre-existing tests that assert the
subject id. Reverted; the tree is back to the implementation above.

## Commands run (exit codes)

```text
cargo fmt --all && cargo test -p cs_app --test camera -- accept_f21_a    → 0 (15 passed)
cargo fmt --all -- --check                                            → 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings → 0
cargo test --workspace --locked                                       → 0
cargo test --workspace --locked -- accept_f21_a_ --include-ignored      → 0
```

Nothing in this task is `#[ignore]`d: it needs no `CS_GAME_DIR`.

## What is not claimed

A code/test pass awards at most **checked**. This task decides an engine
identity scheme and reads no original data; it is not `verified_original` and
it is not `release_approved`. The owner still owns the canonical catalog
contract: the decision here is the one that *does not* need a contract
amendment, and the owner confirms it by leaving `IDENTITY-CONTENT` and
`ContentKind` as they are — or by opening #400/#407 to add collections
deliberately.
