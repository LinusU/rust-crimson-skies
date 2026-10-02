# F21-D: what the installation declares about the pilot's view

Date: 2026-10-03. Task: F21-D "Verify original view controls and cockpit
coverage" (`specs/F21-cameras-cockpit-views-and-spyglass.md`, section
`### F21-D`; acceptance scenario **AC04**, *"Match original cockpit/view
behaviors with recorded input and captures"*). Shared contract:
`docs/contracts/UI-NETWORK.md`; evidence contract
`docs/contracts/CLI-EVIDENCE.md`. Capabilities used: **`retail`** (read-only
`$CS_GAME_DIR`) and **`gpu`** (a real offscreen capture on the machine's
adapter), plus ordinary build/test. No `human_play`, `human_review`, `audio` or
`network_real` capability was used or needed, and **no original run happened**.

Owner paths: `crates/cs_app/src/camera/`,
`crates/cs_content/src/cameras.rs`, `crates/cs_app/tests/camera/`,
`docs/findings/`.

## What this stage measures, and what it cannot

F21-A declared the camera records, F21-B built the four rigs and F21-C ran them
under script cameras and capture flags — every value in all three stages being
newly authored design, because nothing about the original's views had been
read. This stage reads. It walks two subjects of the original's **own decoded
bytes** and reports them against this camera contract:

1. **the camera commands the loading scripts declare** (`ZBD/interp.zbd`), and
2. **the cockpit nodes they bind**, checked against the real node array of the
   shared airframe archive (`ZBD/planes.zbd`).

What it cannot do is the other half of AC04: **the original running**. Which key
fires which view command, what a camera does when its subject dies, whether a
capture leaves the view as it found it, and what a pilot's eye placement is at
runtime are runtime facts. No default binding ships in any readable file (F22-H
measured the label vocabulary and found the bindings themselves native in the
packed executable), and no owner-supplied original run has been supplied
(`#358 REF-OWNER-FIRST-CAPTURE`). That gap is recorded below, not papered over,
and a follow-up task is filed for it.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/camera/coverage.rs` (new): `CameraOperation`,
  `ViewControlEffect`, `ViewControlCoverage`, `ViewCommandClaim`,
  `ViewControlOccurrence`, `ViewControlRow`, `ViewControlFinding`,
  `ViewControlCensus`, `ViewControlError`, `discover_view_controls`,
  `CockpitBindingClaim`, `CockpitBinding`, `CockpitFinding`,
  `CockpitBindingDiscovery`, `CockpitCoverageError`, `discover_cockpit_bindings`,
  `CockpitAirframe`, `CockpitNodeCoverage`, `CockpitBindingCoverage`,
  `AirframeCockpitCoverage`, `CockpitEyeCoverage`, `CockpitCoverageReport`,
  `eye_placement`, `audit_cockpit_coverage`, `MAX_DECLARED_NAME`.
- `crates/cs_app/src/camera/mod.rs`: the module declaration, the re-exports and
  the doc paragraphs naming this stage. No logic.
- `crates/cs_app/tests/camera/coverage.rs` (new) and the module list in
  `crates/cs_app/tests/camera/main.rs`: the 12 `accept_f21_d_*` tests.
- `crates/cs_app/tests/evidence_report_f21_d.rs` (new): the CLI-EVIDENCE
  harness. Deliberately **not** named `accept_f21_d_*`.
- `docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`
  (this file) and `docs/findings/evidence/F21-D.json`.

**One observable failure:** a coverage audit that reports a cockpit for an
airframe the installation does not have one for. The concrete shape of it is
the one this stage spends most of its assertions on: the original's own build
script declares **twenty** cockpit nodes per airframe, and an audit that cannot
say *which aircraft* each one belongs to — or that hands an unresolved name to an
importer as if it were verified — produces a cockpit that draws the wrong
aircraft's instruments, or none. `ActorId` and `SceneNodeId` are both
identity-qualified, so a node under `player_kestrel` is not a node under
`player_warhawk` and borrowing one for the other is a silent wrong-aircraft
frame, not a missing detail. The minimum scenario is
`accept_f21_d_cockpit_coverage_resolves_each_binding_against_its_own_airframe_subtree`;
the walk-order, ambiguity, eye-placement and refusal tests sit beside it.

## The idiom: the claim is the caller's, the rows are measured

Exactly F11-D2's roster idiom, and for the same reason. `cs_content` ships **no**
table about the original: "which script declares the cockpit bindings" and
"which spellings are camera commands" are claims somebody makes against
fingerprinted bytes, and each carries its own `Provenance` with a source span.
What the engine does is **falsify** them:

- a claimed command the container never writes is
  `ViewControlError::ClaimUnseen` — a coverage verdict about a command that does
  not exist would be a confident empty row;
- a line stored with another arity, or with a non-ASCII argument, is a named
  finding on the row it belongs to — never a repaired line and never a silent
  drop;
- a claimed script the container does not hold, or holds twice, or holds without
  bindings, is refused by name;
- an airframe whose root is missing is refused rather than reported empty,
  because "no cockpit" and "no aircraft" are different findings.

Names are compared case-insensitively on stored bytes, because the container's
names have **no established encoding** (F07-B); every *value* keeps its bytes
and is reported through a lossy display form that no decision is taken on.

## Measured: the camera commands the installation declares

`support\cam_anim.gw` sets the world's animation carrier;
`support\init.gw` binds `set camName camera1`, `set worldName world1`,
`set winName window1`; `support\display.gw` then **creates camera objects** and
binds them:

```text
NewCamera %camName%          CameraSetActive on      CameraSetWorld %worldName%   CameraSetWindow %winName%
NewCamera spyglass           CameraSetActive on      CameraSetWorld %worldName%   CameraSetWindow sgwin
```

and every world group's `load.gw` levels a camera's up axis against a horizon:

```text
FindNode camera1  CameraSetHorizon horizon  CameraSetHorizonXZ zone2_cloud_floor
FindNode spyglass CameraSetHorizon horizon  CameraSetHorizonXZ zone2_cloud_floor
```

Measured over the whole decoded container (5 083 lines across its 98 scripts),
with `cs_formats::interp::decode_interp`:

| command | occurrences | where | coverage verdict |
| --- | --- | --- | --- |
| `NewCamera` | 2 | `support\display.gw` | consumed — `CameraOperation::AuthoredCamera` |
| `CameraSetActive` | 2 | `support\display.gw` | consumed — `CameraOperation::PlayerRig` |
| `CameraSetWorld` | 2 | `support\display.gw` | consumed — `CameraOperation::PlayerRig` |
| `CameraSetWindow` | 2 | `support\display.gw` | **unconsumed** |
| `CameraSetHorizon` | 16 | every world group's `load.gw`, plus `support\c1b\load.gw` twice | **unconsumed** |
| `CameraSetHorizonXZ` | 4 | `support\c1\load.gw` and `support\c1b\load.gw` | **unconsumed** |
| `CameraSetObjectHSETest` | 1 | `support\display.gw` | **unconsumed** |

29 occurrences in total, **23 of them unconsumed**: four of the seven declared
camera commands have no consumer anywhere in `cs_app::camera`, and the
`CameraSetHorizon` count is the sharpest one — the original levels a camera's up
axis against the world horizon in **every** world group, for the default camera
*and* for a camera named `spyglass`, and this camera contract has no notion of a
horizon-locked up axis at all. `CameraBasis`' up vector comes from the aircraft's
own axes, which is a design decision F21-B made and which nothing in the files
contradicts — but nothing in the files supports it either, and the original
evidently had a mechanism for it. That gap is a real coverage finding, not a
defect: it is recorded in the census artifact and filed as a task.

The coverage verdicts are the **caller's** judgement, carried with provenance
and enforced to name what is missing (`ViewControlError::UnconsumedWithoutReason`);
the **census** is measured, and the retail test pins every count above.

Two further facts the measurement established, both about the corpus rather than
about this engine:

- the original's magnified view exists in its own files as a **named camera
  object**: `NewCamera spyglass`, bound to a window (`sgwin`) and leveled to the
  horizon. This engine models the spyglass as a *mode of the player's rig*
  (F21-B), not as a camera object; whether the two are the same thing is
  unmeasured and is **not** claimed either way here.
- the world's authored camera data exists as a carrier: `support\cam_anim.gw`
  loads `%ZBD_DIR%\%CAMPAIGN_DIR%\cam_anim.zbd` and
  `..\data\%CAMPAIGN_DIR%\zrdr\cam_anim.zrd`, and all eight world groups ship
  `ZBD/<group>/cam_anim.zbd`. F20-D already fingerprints those carriers; the
  `.zan`/`.zrd` clips they reference are still an **undecoded layout**, which
  F20-C/F40 own. No clip was read here.

## Measured: the cockpit bindings, per airframe

`support\util\planesurgery.gw` renames the plane model's own `cockpit1` node,
and `support\cockpit.gw` — included once per airframe after
`set player_plane <root>` — then addresses **instrument and damage nodes inside
that aircraft**. Measured over the real container: 158 lines, **20 distinct
bindings** in first-use order, every line of the script one of the two shapes the
claim declares (so the walk has no findings at all):

```text
gungauge  4char_ammo  missilegauge  ggindicator0..3  mgindicator0..7  6char_type
rightwingdamage  leftwingdamage  taildamage  nosedamage
```

The same 20 for every aircraft — the script is generic, so *which* aircraft a
binding belongs to is the **include site**'s business, and that is exactly what
the coverage audit resolves. Against `ZBD/planes.zbd`'s real node array, for the
eleven airframes F11-D2's roster discovery declares (`player_pfighter`,
`player_bhawk`, `player_fbrand`, `player_brigand`, `player_fury`,
`player_autogyro`, `player_avenger`, `player_kestrel`, `player_peacemaker`,
`player_warhawk`, `player_balmoral`; 174–235 nodes per subtree):

| measured | value |
| --- | --- |
| declared bindings per airframe | 20 |
| (airframe, binding) pairs | 220 |
| resolved to a node with stored geometry | **176** |
| unresolved | **44** — the same four names on every aircraft |
| airframes whose 20 bindings all resolved | **0 of 11** (each resolves 16) |

The four unresolved names are container-shaped, and each state is measured from
the node's own bytes:

- `gungauge` and `missilegauge` resolve to a node whose stored `mesh_index` is
  `-1` — the node exists and the original addresses it, and it carries no
  geometry of its own (`NoMesh`);
- `4char_ammo` and `6char_type` each name **two** nodes of the aircraft's
  subtree (`Ambiguous`), so no single node can be named for them.

That is a fact about the original's own node arrays, not an artefact of this
audit: `NoMesh` reads the node's stored `mesh_index` and `Ambiguous` counts the
nodes of the aircraft's subtree. The instruments these four nodes address are
their **descendants**, and this stage does not resolve a whole cockpit subtree —
so "the pilot's eye is at the root of an instrument panel" is not measured, not
assumed and not invented. A cockpit-content pass that walks the subtree is the
obvious next step and is filed below.

So: **176 of 220 declared (airframe, binding) pairs resolve to real stored
cockpit geometry, and every one of the 176 resolves inside its own aircraft.**
The 176 are `CockpitBindingSource::ModelNode` bindings an importer may name —
which is what F21 non-negotiable behavior 1 asks for ("cockpit viewpoint comes
from verified model/config bindings"), as far as a readable file allows.

## What is not claimed: the pilot's eye

`eye_placement()` returns `CockpitEyeCoverage::Undeclared`
**unconditionally**, and `CockpitCoverageReport::eye()` carries it. This is
deliberate and it is the stage's most important negative result:

- the loading scripts say **which** cockpit nodes an aircraft has;
- the airframe archive says where those nodes sit in the stored hierarchy;
- **nothing in any file this project opens says where the pilot's eye sits
  inside them** — no transform, no offset, no orientation, in the loading
  scripts, in `planes.zbd`, or in any other readable container.

An eye derived from a mesh's bounds, an aircraft's centroid or a designer-typed
offset would be exactly the "HUD-only synthetic camera" F21 non-negotiable
behavior 1 forbids in place of a verified binding. So the value is not a
`Resolved<Radians>` a caller can fill in; it is a function that cannot return
anything else, and a synthetic test asserts that a cockpit mode whose viewpoint
orientation is an explicit unknown still refuses to lower through the real
`lower_camera_modes`.

What resolves it: an owner-supplied original run that shows a pilot's eye
placement (the same gate as `#358`), or a bounded static analysis of
`crimson.icd` that records only ids/offsets/digests and commits no decompiled
code. Both are filed as tasks below.

## The GPU half

`cs_app::world::gpu_capture::capture_world_mesh` (F18-D's production capture
path) drew two frames on the real adapter (Apple M3 Pro, Metal):

| artifact | source | result |
| --- | --- | --- |
| `f21-d-fixture-cockpit-binding.png` | an authored fixture mesh, built through `RenderMesh::build` | 8 triangles, drawn, digest re-checked against the file on disk |
| `f21-d-cockpit-rightwingdamage-39.png` | the **real** `ZBD/planes.zbd` cockpit binding `rightwingdamage`, mesh 39, 7 stored triangles, chosen from the bytes rather than from a hardcoded index | drawn: 5 947 of 76 800 pixels (77 per mille), 10 260 bytes, adapter named |

Both artifacts live in `private/evidence/F21-D/` and are hashed in the report.
The negative half is in the same test: a mesh with nothing in it is refused by
name (`EmptyMesh`) and leaves **no file** behind, because a refused capture must
never leave a PNG that reads like evidence.

What the captures are **not**: they are not a cockpit *view*. They are the
geometry a coverage row vouches for, drawn by the real renderer, measured for
coverage. Placing a camera *inside* the original's cockpit needs the eye
placement this stage could not find, so this stage does not fake one.

## Test sensitivity

Seven mutations, applied and reverted against production code, run as
`cargo test -p cs_app --test camera -- accept_f21_d_ --include-ignored`:

| # | mutation | tests that died |
| --- | --- | --- |
| 1 | `subtree_indices` stops following child slots | **4** (coverage, ambiguity, eye, retail coverage) |
| 2 | `audit_cockpit_coverage` counts a `mesh_index < 0` node as bound | **2** (coverage, retail coverage) |
| 3 | `discover_view_controls` stops reporting an arity mismatch | **1** (`a_line_the_claim_does_not_describe_is_reported_instead_of_dropped_or_repaired`) |
| 4 | `discover_view_controls` accepts an unseen claim | **1** (`a_claim_the_container_never_writes_is_refused_rather_than_reported_as_no_coverage`) |
| 5 | `discover_cockpit_bindings` stops deduplicating by node name | **4** (binding order, coverage, eye, retail census) |
| 6 | `stored_root_name` stops deriving the bare node name | **4** (coverage, ambiguity, eye, retail coverage) |
| 7 | `verified_bindings` includes unresolved bindings | **1** (coverage) |

Mutation 7 did **not** fail on the first pass: the coverage test asserted the
verified list only for the airframe whose bindings all resolved, so a
`verified_bindings` that returned every binding agreed with it. The test now
asserts the warhawk row's verified list (one binding) and that the two
airframes' lists differ, and mutation 7 dies. All seven were reverted; the tree
is back to the implementation under review.

## Commands run (exit codes)

```text
cargo fmt --all -- --check                                                   → 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings → 0
cargo test --workspace --locked                                              → 0
cargo test --workspace --locked -- accept_f21_d_ --include-ignored           → 0 (12 tests, all passed)
```

The task selection discovers exactly the 12 `accept_f21_d_*` tests in
`crates/cs_app/tests/camera/coverage.rs`: eight unignored (CI runs them) and
four marked — two `#[ignore = "requires CS_GAME_DIR"]`, one
`#[ignore = "requires a GPU …"]` and one
`#[ignore = "requires CS_GAME_DIR and a GPU adapter"]`. All four pass locally
with `CS_GAME_DIR` set and the machine's adapter. The F21-A/B/C tests in the
same binary (15, 26 and 33) still pass.

## Evidence

`private/evidence/F21-D/acceptance.json`, validated with
`tools/validate_evidence.py … --require-pass`
(`{"structurally_valid": true, "artifact_count": 4}`), committed as
`docs/findings/evidence/F21-D.json`. Artifacts: `cargo-test.log`,
`view-cockpit-coverage.json`, and the two PNGs. Source hashes are the
installation's own: install `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`,
content `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`
(both identical to F22-H's, measured on the same installation).

The report's `unknowns` are this task's own **blockers**, and they are empty
because the acceptance run passed. The **product incompleteness** this stage
measured is not dropped: it is machine-readable in the referenced
`view-cockpit-coverage.json` (per-binding state, per-airframe counts, the eye
verdict and, for each open question, the task that resolves it) and written out
below. A working audit reporting incomplete support is the point of the audit.

## Designed vs measured: the unknowns this stage met

| unknown | evidence | resolves in |
| --- | --- | --- |
| Where the pilot's eye sits inside a cockpit | nothing: the loading scripts name cockpit **nodes** and `planes.zbd` stores their hierarchy, and no readable file stores an eye transform. `eye_placement()` returns `Undeclared` unconditionally | an owner original run (`#358`), or a bounded static analysis of `crimson.icd`; filed as a task below |
| What the four container-shaped cockpit bindings (`gungauge`, `missilegauge`, `4char_ammo`, `6char_type`) contain | measured as `NoMesh` / `Ambiguous` on real nodes; their geometry, if any, is in descendants this stage does not walk | a cockpit-content pass that resolves each binding's subtree; filed as a task below |
| Whether `CameraSetHorizon` / `CameraSetHorizonXZ` level the camera's up axis, roll, or the view's world-up | 16 and 4 occurrences, arguments named `horizon` and `zone2_cloud_floor`; the **mechanism** is in the packed executable, not in a script | a static-analysis task over the native command handlers, or an original run; filed as a task below. Until then `cs_app::camera` has no horizon-locked up axis and does not claim one |
| Whether the original's `spyglass` **camera object** and this engine's spyglass **mode** are the same thing | measured: the original creates a camera named `spyglass`, activates it, binds a world and a window, and levels its horizon. This engine models a magnified view of the selected target as a rig mode (F21-B) | F21's spyglass stage with an original run; the two shapes are recorded rather than merged |
| What the default key bindings are | no shipped file holds a key/mouse/joystick → command map; F22-H measured the label vocabulary and found the bindings native | `#505 F22-J` (already filed) and the owner-gated run |
| What the `.zan`/`.zrd` camera clips contain, and whether any is a scripted view | the carriers exist in all eight world groups and are fingerprinted by F20-D; the payloads are undecoded | F20-C (decode) and F40-B (playback) |
| Whether a capture leaves the player's view exactly as it found it | nothing measured; F21-C decided the seam's own policy (borrow and return the whole view) and said so | an original run, or F17-C's `--screenshot` CLI stage |

## Follow-ups filed, not fixed here

These are outside this stage's owner paths or a different subsystem's
responsibility, so they are tasks and not edits:

1. **#540 Cockpit-content resolution.** A cockpit pass that resolves each declared
   binding's *subtree* to geometry, so `gungauge` and `missilegauge` become
   drawable instrument geometry rather than `NoMesh` container nodes, and so a
   cockpit mode has something to place an eye inside. Affected content: the
   eleven airframes of `ZBD/planes.zbd`, 44 of 220 declared (airframe, binding)
   pairs. Owner path would be F11's scene subtree work plus this camera seam.
2. **#541 Horizon-locked camera up axis.** `CameraSetHorizon` is written in every
   world group; `cs_app::camera` has no horizon-locked basis. Needs a static
   analysis of the native handlers or an original run before any such mechanism
   is modelled. Affected content: every world's external and spyglass view.
3. **#542 The pilot's eye.** Owner-gated as above; no agent can close it.

## What is not claimed

A code/test pass awards at most **checked**, and this report is not even that
yet: the claim in the evidence file is `implemented`, because the implementer
wrote the code it measures. Specifically, nothing here is `verified_original`:
no original camera data was interpreted beyond what its scripts **declare**,
no original view, cockpit or spyglass *behavior* was reproduced, no cockpit
viewpoint, look limit, smoothing rate or magnification mechanism was measured,
no authored camera timeline was read, and the two captures are geometry drawn by
the real renderer rather than cockpit views. The comparison against the original
running — the other half of AC04 — needs an owner-supplied original run, and no
agent review of this work replaces the owner's human approval.