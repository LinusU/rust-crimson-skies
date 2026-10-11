# #541: the horizon commands anchor named scenery to the camera — not an up-axis lock

Date: 2026-10-11. Task: #541 "Model a horizon-locked camera up axis, or record
why not (F21-D follow-up)". Feature sheet:
`specs/F21-cameras-cockpit-views-and-spyglass.md`, stage `### F21-D`. Shared
contract: `docs/contracts/UI-NETWORK.md`; evidence contract:
`docs/contracts/CLI-EVIDENCE.md`. Predecessor:
`docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`,
whose census recorded 16 `CameraSetHorizon` and 4 `CameraSetHorizonXZ`
occurrences and filed this task to learn what the commands actually do.

Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and the
owner-supplied decrypted executable `$CS_ENGINE_IMAGE`
(`crimson.decrypted.exe`, SHA-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` — the same
image F16-E's landmarks were measured in). `gpu` and `audio` were available
and **not used**: nothing is rendered and nothing is played; the measurement
is byte inspection and script/node-archive reads.

**Nothing here is `verified_original`.** No original run happened. Byte
inspection of the owner-supplied image is *static analysis* — the same
evidence class as F16-E's landmarks and F37-D-FU2's terminal rules — and
`retail` is read access to files. Neither is the original running: how the
original's renderer composes the anchored subtrees each frame is runtime
behaviour no file records. A different agent instance with a fresh context
should review this measurement, and no agent review replaces the owner's
approval.

Installation fingerprint:
`b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` (the same
installation F21-D measured).

## The question F21-D filed

F21-D's census of the decoded `ZBD/interp.zbd` loading scripts found every
world group's `load.gw` writing `CameraSetHorizon horizon` for both `camera1`
and `spyglass`, plus `CameraSetHorizonXZ` against `zone2_cloud_floor` (C1) and
`moon_reflection` (C1B). From the spellings it classified the commands as
`LevelsHorizon`/`LevelsHorizonToZone` — **marked project classification, not
measurement** — and reported them unconsumed because this camera contract has
no horizon-locked up axis. The filed question was whether the commands level
the camera's up axis, roll, or the view's world-up.

The measured answer is that the question's premise is wrong: **the commands
never touch the camera's orientation at all.** They re-anchor named world
scenery to the camera's position each render pass — the "horizon" is a node
the sky hangs under, dragged along with the viewer so it never approaches,
and the XZ variant drags floor/reflection content laterally at its own stored
altitude. There is no up-axis lock to model; the recorded mechanism is the
"why not".

## The method

Bounded static analysis of the image bytes only: disassembly with
r2/objdump/lldb and a Python Capstone pass to enumerate the `Camera.c`
translation unit (the image carries COFF line records naming
`D:\zipper\gamez\zclass\Camera.c`, which bounds the file's 47-function
range `0x4d2000`–`0x4d6400`), cross-checked against the stored strings. No
decompiled code is committed; only addresses, field offsets, byte spans and
verdicts are recorded. Every image claim is pinned by a `SourceSpan` over
`crimson.decrypted.exe` at file offsets (VA − `0x400000`), and
`cs_app::camera::horizon::mechanism_source` refuses any image whose digest is
not the measured one.

## The measured mechanism

**Dispatch.** The interp-script command chain around VA `0x5b85e3` matches
command names with `strnicmp`, checking `CameraSetHorizonXZ` (18-byte limit)
**before** `CameraSetHorizon` (16-byte limit) — the only order that works,
since the shorter name is a prefix of the longer one and a 16-byte prefix
match would otherwise swallow it. Both handlers take the next script argument
through the argument cursor `0x5b78f0` and resolve the name through the
global registry `0x4d0280` in mode 7 — the same pool `FindNode` binds from —
producing a **scene-node pointer**, not an axis.

**Binding.** Each handler calls a `Camera.c` routine that stores the resolved
node on the camera object, refusing a null node and a camera of the wrong
class (both carry their own assert strings):

- `0x4d2ec0` (`CameraSetHorizon`): `mov [ecx+8], eax` at VA `0x4d2f2e` —
  camera data field `+0x8`, which the camera's own dump routine prints as
  `horizon = %s`;
- `0x4d2f40` (`CameraSetHorizonXZ`): `mov [ecx+0xc], eax` at VA `0x4d2fae` —
  field `+0xc`, printed as `horizonXZ = %s`.

**Application.** `0x4d6390` runs every render pass, called from exactly three
camera functions (call sites `0x4d6518`, `0x4d67b9`, `0x4d6bc6`), each time
*after* the camera's world matrix is computed (`0x4d2fc0`) and *before*
camera→render setup (`0x5409b0`). It reads the camera's world translation
from camera data `+0x2c/+0x30/+0x34` and:

- for the `+0x8` node, calls `0x4d1d50(node, cam.x, cam.y, cam.z)` — all
  three axes verbatim;
- for the `+0xc` node, first reads the bound node's **own** stored Y through
  `0x4d1cc0`, then calls `0x4d1d50(node, cam.x, node.y, cam.z)` — lateral
  only, the node slides under the camera at whatever altitude its content
  gave it;
- then runs the scene-graph update pass `0x4d0210` so the moved subtrees'
  world matrices refresh before drawing (the tail update is skipped when
  neither node is bound).

`0x4d1d50` is the set-local-translate routine: it writes the node's stored
translation and marks its subtree dirty (the `0x4cf830` flag recursion), so
the write moves the whole subtree — the zone sky and everything under it.

The routine's own five calls, decoded from the bytes and pinned by the
acceptance suite: `0x4d0210`, `0x4d0210`, `0x4d1cc0`, `0x4d1d50`, `0x4d1d50`.

**What is never read or written:** the camera's rotation, basis vectors and
up axis. The direction is **camera → scenery**, one way only.

## What the bound nodes are

Against the eight world-group archives (`ZBD/<group>/gamez.zbd`, manifest-
fingerprinted, `read_gamez_nodes`):

- `horizon` is a stored `object3d` node parented directly to `world1` in
  **every** world group, and the zone-sky containers are its children:
  `zone1`/`zone2`/`zone3`, the `moon`/`stars`/`sun` nodes and the scrolling
  cloud layers (`h_zone*scroll`). Dragging `horizon` to the camera position
  each pass is the classic infinite-distance sky trick — the sky can never
  be approached because it moves with the viewer.
- `zone2_cloud_floor` and `moon_reflection` are **not stored node names** in
  any of the eight world archives' node arrays. The commands resolve them
  through the runtime name registry, so the original instantiates them
  somewhere the readable files do not show — where is **not established**
  and is recorded as an unknown below, not guessed.
- The retail census re-confirms F21-D's counts under the corrected
  classification: 20 binding occurrences — sixteen `all_axes` `horizon`
  anchors (two cameras × eight world groups) and four `keep_own_height`
  anchors (`zone2_cloud_floor` twice in `support\c1\load.gw`,
  `moon_reflection` twice in `support\c1b\load.gw`).

One adjacent observation, recorded but not expanded: the image also interns a
`pfhorizon` name (`0x627ccc`, interned at `0x476bd2`) among player-aircraft
node names (`compass`, `ltracks`/`rtracks`, `cockpit1.t`, …) — apparently a
*player-model* horizon reference, a different node from the world's `horizon`
sky container. Its consumer was not traced; it is outside this task's
mechanism.

## What was added, and the correction it carries

`crates/cs_app/src/camera/horizon.rs` models the measured state as values —
the world side (resolving the name to a live node, marking its subtree dirty)
is deliberately left to the scene graph, exactly as `CameraSetWorld`'s
consumer is the rig:

- `HorizonAnchorRule::{AllAxes, KeepOwnHeight}` — the two slots the camera
  data holds (`+0x8`, `+0xc`); `for_command` derives the rule the way the
  native dispatch matches, and `anchored_translation(camera, node)` is the
  measured per-pass write expressed as a value;
- `HorizonBinding` — the bound state a command installs: node name, rule,
  `Origin`, claim `Provenance`; `horizon_bindings(census, origin)` derives
  the measured set from a `ViewControlCensus`, one binding per occurrence;
- `HorizonMechanism` + `mechanism_source(image)` — the five `SourceSpan`s
  over the measured image (the two stored command names, the two binding
  routines, the apply routine), refused for any image whose digest is not
  `original_image_digest()`.

The F21-D classification is corrected in place: `CameraOperation` gains
`SceneryAnchors`, and `ViewControlEffect::LevelsHorizon`/
`LevelsHorizonToZone` are **replaced** by the measured
`AnchorsSceneryToCamera`/`AnchorsSceneryToCameraLaterally` — the enum's doc
now says which values are measured and which stay project vocabulary. The
retail and synthetic claim lists mark both horizon commands `Consumed` by
`SceneryAnchors`; `CameraSetWindow` and `CameraSetObjectHSETest` stay
unconsumed (the latter still `Undetermined` — nothing measured says what a
hull-sensitivity probe does to a camera).

A supersession note records the correction at the top of the F21-D finding;
the superseded text is left intact so the dated record stays honest.

## What stays unknown (not filled in)

- **Where `zone2_cloud_floor`/`moon_reflection` are instantiated.** Not in
  any world group's stored node array; presumably created by content the
  loader reads elsewhere (`.zrd` zone documents are a candidate, unverified).
- **How the original's renderer draws the anchored subtrees.** The write is
  measured; the pixels are not — runtime behaviour needs an original run.
- **Whether the camera's basis is ever horizon-influenced by anything else.**
  The two commands establish no such path, and none was found in `Camera.c`'s
  measured range — but an exhaustive proof that *no* code reads horizon
  orientation into a camera basis is a different, larger analysis. F21-B's
  aircraft-axis up vector remains this engine's **authored** policy:
  unmeasured, now with one fewer place the original could have contradicted
  it.
- **The `spyglass` object↔mode equivalence** — unchanged from F21-D; the
  original creates a named camera object, this engine a rig mode, and the
  measured anchor applies to both.
- **What `pfhorizon` drives** (see above).

## Files changed

- `crates/cs_app/src/camera/horizon.rs` (new) — the measured model above.
- `crates/cs_app/src/camera/coverage.rs` — `CameraOperation::SceneryAnchors`,
  the two measured `ViewControlEffect` variants replacing
  `LevelsHorizon`/`LevelsHorizonToZone`, and the docs saying which vocabulary
  is measured.
- `crates/cs_app/src/camera/mod.rs` — `pub mod horizon`, re-exports and the
  module-doc bullet (wiring only).
- `crates/cs_app/tests/camera/horizon.rs` (new) — the nine `accept_t541_`
  tests.
- `crates/cs_app/tests/camera/coverage.rs` — the claim lists and pinned
  assertions under the corrected classification.
- `crates/cs_app/tests/camera/main.rs` — `mod horizon` and docs (wiring).
- `crates/cs_app/tests/evidence_report_t541.rs` (new) — the CLI-EVIDENCE
  harness (not an acceptance test).
- `docs/findings/2026-10-03-f21-d-…coverage.md` — the supersession note.
- `docs/findings/evidence/T541.json` — the validated acceptance report.

## Tests

Nine `accept_t541_` tests in `crates/cs_app/tests/camera/horizon.rs`: six
over authored fixtures (CI runs them), two retail tests
(`#[ignore = "requires CS_GAME_DIR"]`) and one engine-image test
(`#[ignore = "requires CS_ENGINE_IMAGE"]`). They pin: the two commands →
their rules under whole-name case-insensitive matching; the write's
direction (a position in, never a rotation — asserted against a non-identity
pose); `AllAxes` copying the camera verbatim and `KeepOwnHeight` preserving
the node's stored Y; the binding record's name/rule/origin/provenance;
census-derived bindings (and the refusals `MissingNode`/`NotAHorizonCommand`);
the digest refusal; the image spans byte-for-byte (stored names, both field
stores, the apply routine's five decoded call targets); the retail census
(20 bindings, the lateral set `{zone2_cloud_floor, moon_reflection} ×2`);
and `horizon` an `object3d` under `world1` with zone children in all eight
world archives while both lateral names are absent from every stored node
array.

The F21-D suite is updated rather than left to fail: the unconsumed count is
now 1 (`CameraSetWindow`), the horizon rows carry `scenery_anchors` /
`anchors_scenery_to_camera(_laterally)`, and the retail counts
(16 + 4 of 29 occurrences) are unchanged — only the verdicts moved.

## Checks run

* `cargo fmt --all -- --check` = 0;
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  = 0; `cargo test --workspace --locked` = 0; and
  `cargo test --workspace --locked -- accept_t541_ --include-ignored` = 0
  (9 discovered, 9 passed).
* Evidence: `private/evidence/T541/acceptance.json` produced by
  `evidence_report_t541` from the recorded acceptance log, production
  discovery of `$CS_GAME_DIR`, `load_engine_image` over `$CS_ENGINE_IMAGE`
  and a second full production re-run of the measured chain inside the
  harness itself; validated with `tools/validate_evidence.py --artifact-root
  private/evidence/T541 --require-pass`; copy committed as
  `docs/findings/evidence/T541.json`. Artifacts: `cargo-test.log`,
  `horizon-bindings.json` (the 20-occurrence census), `horizon-mechanism.json`
  (the five spans plus the apply routine's decoded call targets).

## Sources used

- `specs/F21-cameras-cockpit-views-and-spyglass.md` (### F21-D),
  `docs/contracts/UI-NETWORK.md`, `docs/contracts/CLI-EVIDENCE.md`,
  `docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`.
- The owner's installation, read-only, over `$CS_GAME_DIR`: `ZBD/interp.zbd`
  (the `load.gw` scripts) and the eight `ZBD/<group>/gamez.zbd` world
  archives. Installation fingerprint above.
- The owner-supplied decrypted executable over `$CS_ENGINE_IMAGE`,
  SHA-256 above; disassembled read-only with r2/rabin2/objdump/lldb and
  Capstone 5.0.7. The measured landmarks: dispatch `0x5b85e3`…, handlers
  `0x4d2ec0`/`0x4d2f40`, field stores at `+0x8`/`+0xc`, apply `0x4d6390`
  (call sites `0x4d6518`/`0x4d67b9`/`0x4d6bc6`), helpers `0x4d0280`,
  `0x4d1cc0`, `0x4d1d50`, `0x4d0210`, `0x4cf830`.
- `crates/cs_app/src/camera/` (the F21-D census and F21-B rigs this corrects
  and hangs off), `crates/cs_content/src/coordinates.rs` (the engine-image
  provenance idiom), `crates/cs_formats/src/gamez/` (the node reader).

**No original data is committed.** The artifacts carry counts, stored names,
offsets, digests and byte-level instruction pins; no decompiled code, no
extracted script, no asset and no screenshot is in the repository, and every
private output went to `private/`, outside it.
