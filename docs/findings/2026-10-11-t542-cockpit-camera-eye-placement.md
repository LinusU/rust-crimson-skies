# #542: the pilot's eye is the `cockpit_camera` node's stored local translation

Date: 2026-10-11. Task: #542 "Obtain an owner original run showing a
pilot's eye placement (F21-D follow-up)". Feature sheet:
`specs/F21-cameras-cockpit-views-and-spyglass.md`, stage `### F21-D`. Shared
contract: `docs/contracts/UI-NETWORK.md`; evidence contract:
`docs/contracts/CLI-EVIDENCE.md`. Predecessors:
`docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`,
which reported the eye `Undeclared` because no readable file declares it and
filed this task, and
`docs/findings/2026-10-11-t541-horizon-commands-anchor-scenery-to-camera.md`,
the sibling follow-up whose static-analysis method this task reuses.

Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and the
owner-supplied decrypted executable `$CS_ENGINE_IMAGE`
(`crimson.decrypted.exe`, SHA-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` — the same
image T541 and F16-E measured). `gpu` and `audio` were available and **not
used**: nothing is rendered and nothing is played; the measurement is byte
inspection and node-archive reads.

**Nothing here is `verified_original`.** No original run happened. The task
offered two ways forward — an owner-supplied run showing the pilot's eye
placement, or a bounded static analysis recording only IDs, offsets and
digests — and this task took the second: no owner run was available. Byte
inspection of the owner-supplied image is *static analysis*, not a run of
the original, and `retail` is read access to files. A different agent
instance with a fresh context should review this measurement, and no agent
review replaces the owner's approval.

Installation fingerprint:
`b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` (the same
installation F21-D measured).

## The question F21-D filed

F21-D audited the twenty cockpit node names `support\cockpit.gw` declares —
gauges, indicators and damage shells — and reported the pilot's eye
`Undeclared`: a verified binding is a *name*, and no file the audit opened
declared where the pilot's eye sits. The filed question was where the eye
is: an owner-supplied original run, or a bounded static analysis of the
executable.

The measured answer: **the eye is not a script binding at all.** It is a
named node in the airframe archive — `cockpit_camera`, an `object3d` marker
under `geometry → markers` in every vehicle subtree — whose stored local
translation the original's own vehicle registration reads into the eye
globals at spawn time, and which two dispatched camera modes then transform
by the aircraft's world matrix each frame. Every player airframe declares
one; so does every AI variant.

## The method

Bounded static analysis of the image bytes only: disassembly with r2 plus a
Python byte-scan for call-target and field-offset pins, cross-checked
against the stored strings and the manifest-fingerprinted `ZBD/planes.zbd`
node array. No decompiled code is committed; only addresses, field offsets,
byte spans and verdicts are recorded. Every image claim is pinned by a
`SourceSpan` over `crimson.decrypted.exe` at file offsets (VA − `0x400000`),
and `cs_app::camera::eye::eye_mechanism_source` refuses any image whose
digest is not the measured one.

## The measured mechanism

Four links, each pinned in the acceptance suite at the byte level (VAs are
`.text` unless noted):

1. **The lookup.** `0x4d8cf0` is the engine's recursive child-by-name
   search: a byte-exact (case-sensitive) name compare against each child,
   then recursion over the child array **last-to-first**, so the last
   matching child wins. Its only `e8` call targets itself.
2. **The registration.** `0x473480` runs per vehicle (three call sites:
   spawn, reset with a null argument, and a detach path). It resolves
   `healthy`, `dontmove`, `cockpit1`, `geometry`, `markers` — and
   `cockpit_camera` (`.rdata` `0x62747c`) — through the lookup, and when the
   `cockpit_camera` node resolves it calls `0x4d1cc0`, which reads the
   node's **stored local translation** — the `+0x54`/`+0x58`/`+0x5c` triple
   of its object3d record — into the eye-offset globals
   `0x71c328`/`0x71c32c`/`0x71c330`. A null vehicle restores the triple from
   defaults at `0x75d1b8..0x75d1c0`. `0x4d1cc0` is the same local-translate
   reader T541's measured apply path reads node translations through, so
   two independent mechanism claims pin the same reader; `0x4d1d50` is its
   write pair. The registration's own call list, decoded from the span: the
   lookup seven times, the reader once.
3. **The application.** The camera-mode dispatcher at `0x42c5c0` loads the
   mode index at `[cam + 0x14c]`, bounds it to ten entries, and jumps
   through the table at `0x42c648`. Entries **6 and 7** hold the `0x42c61d`
   thunk, which tail-calls `0x42d980`: the eye transform loads the three
   eye globals verbatim, applies the aircraft's world matrix at
   `[edi + 0x180]` (`eye_world = M·eye_local + t`), and feeds the result to
   the camera position setters. The stored `cockpit_camera` translation is
   therefore a *local* offset the mode re-anchors against the live
   airframe — matching the authored placement, since every ancestor
   transform in the measured corpus is identity.
4. **The overrides.** Inside the script-variable setter chain, the
   `cocky`/`cockz` branches (`0x43e3be..0x43e442`, names at `.rdata`
   `0x622ebc`/`0x622ec4`) compare the variable name and `fstp` into
   `0x71c32c` and `0x71c330` — an authored script channel that overwrites
   the **second and third** eye components. No `cockx` name sits beside
   them, so the first component has no script override in this image. The
   channel is measured; what a script writes is runtime state.

## What the archive holds

Against `ZBD/planes.zbd` (manifest-fingerprinted, `read_gamez_nodes`):

- the archive stores **22** `cockpit_camera` nodes — one under every
  `player_*` airframe root and one under every AI-variant root;
- each is an `object3d` marker child of `markers` under `geometry`, with
  every ancestor transform the identity, so the stored local translation
  is the airframe-relative authored eye offset;
- all **11** audited player airframes resolve `Declared` — exactly one
  `object3d` `cockpit_camera` inside their own children closure — and the
  eleven stored triples are authored per airframe, not one shared
  constant.

The measured triples, in the archive's source units with the axis map
deliberately unclaimed (`x`, `y`, `z` are the stored order, not axis
names):

| airframe | stored translation |
| --- | --- |
| `player_pfighter` | `2.84e-14, 0.75, -0.2` |
| `player_bhawk` | `2.84e-14, 0.75, -0.2` |
| `player_fbrand` | `-0.081, 1.168, -0.907` |
| `player_brigand` | `2.84e-14, 0.830, 0.723` |
| `player_fury` | `2.84e-14, 0.75, -0.2` |
| `player_autogyro` | `2.84e-14, 0.75, -0.2` |
| `player_avenger` | `2.84e-14, 0.75, -0.2` |
| `player_kestrel` | `1.979, 0.748, -1.210` |
| `player_peacemaker` | `-1.8, 0.948, -0.436` |
| `player_warhawk` | `0.0017, 2.31, 3.296` |
| `player_balmoral` | `4.83e-14, 0.7, -0.34` |

The near-zero first components (`2.8e-14`, `4.8e-14`) are f32 noise on a
stored zero, kept verbatim rather than rounded.

## What was added

- `cs_app::camera::coverage::resolve_cockpit_eye` — the production pass:
  for every audited airframe, the byte-exact `cockpit_camera` search over
  that airframe's own children closure (the same `subtree_indices` walk
  the binding audit uses, so the search can never leave the airframe it
  started from). One `object3d` resolves `Declared { node_index,
  translation }` — the stored triple verbatim, in source units; none is
  `Absent`; several is `Ambiguous { node_indices }` (reported, never
  resolved by mimicking the original's last-to-first tiebreak — the
  corpus holds no duplicates, so the tiebreak is unneeded and unclaimed);
  one non-object3d is `NotAnObject { node_index, kind }`, because the
  slot the original's reader addresses exists only on an object record.
- `cs_app::camera::eye` — the measured landmarks as `EyeMechanism`
  `SourceSpan`s over the image (name, lookup, registration, reader,
  dispatch head, dispatch table, thunk, apply head, override names and
  override branches), digest-gated by `eye_mechanism_source`; and
  `CockpitEyeBinding`, the bound state a `Declared` eye produces — source
  `ModelNode { node: "cockpit_camera" }`, node index and stored triple —
  the `CockpitBindingSource` a `CockpitViewpoint` carries.
- `cs_app::camera::coverage::COCKPIT_EYE_NODE` — the one measured name.
- F21-D's `CockpitEyeCoverage` and `eye_placement()` stay `Undeclared`
  **as the binding audit's verdict**, which remains true: the twenty
  declared bindings name no eye. A dated supersession note at the top of
  the F21-D finding records that the eye is declared — in the airframe
  archive, through a channel the script walk never read.

## What stays unknown (not filled in)

- **Which body axis each stored component is, and in what units.** The
  archive stores a triple; the axis map to this project's
  right/up/forward body frame and the unit conversion are a separate
  measurement, so `CockpitEyeBinding` exposes `[f32; 3]`, not a
  `BodyOffset`.
- **What camera modes 6 and 7 are called in the original UI** — the
  dispatch table's indices are measured, the mode names are not.
- **What `cocky`/`cockz` scripts write.** The channel is measured; the
  values are runtime state no file records.
- **Whether the last-to-first tiebreak ever matters.** No retail
  airframe stores two `cockpit_camera` nodes, so the original's
  selection is unexercised; the pass reports ambiguity instead of
  copying the tiebreak.
- **Whether the AI-variant `cockpit_camera` nodes are ever installed as
  a camera eye** — the registration resolves them identically, but which
  camera mode runs against an AI vehicle is runtime behavior.
- **The cockpit-mode orientation.** The eye is a position; the mode's
  yaw/pitch basis is a different function of the camera state and was
  not traced here.

## Files changed

- `crates/cs_app/src/camera/eye.rs` (new) — the mechanism spans, the
  digest-gated source and `CockpitEyeBinding`.
- `crates/cs_app/src/camera/coverage.rs` — `COCKPIT_EYE_NODE`,
  `CockpitEyeState`, `AirframeCockpitEye`, `CockpitEyeReport` and
  `resolve_cockpit_eye`, with the `Undeclared` docs re-scoped to the
  binding audit.
- `crates/cs_app/src/camera/mod.rs` — `pub mod eye`, re-exports and the
  module-doc bullet (wiring only).
- `crates/cs_app/tests/camera/eye.rs` (new) — the nine `accept_t542_`
  tests.
- `crates/cs_app/tests/camera/main.rs` — `mod eye` and docs (wiring).
- `crates/cs_app/tests/camera/coverage.rs` — the F21-D eye test and the
  census's `eye_placement_resolves_in` text re-scoped to the binding
  audit.
- `crates/cs_app/tests/evidence_report_t542.rs` (new) — the
  CLI-EVIDENCE harness (not an acceptance test).
- `docs/findings/2026-10-03-f21-d-…coverage.md` — the supersession note.
- `docs/findings/evidence/T542.json` — the validated acceptance report.

## Tests

Nine `accept_t542_` tests in `crates/cs_app/tests/camera/eye.rs`: seven
over authored archives (CI runs them), one retail test
(`#[ignore = "requires CS_GAME_DIR"]`) and one engine-image test
(`#[ignore = "requires CS_ENGINE_IMAGE"]`). They pin: the stored
translation verbatim with the node's own index; the byte-exact name
compare (`Cockpit_Camera` resolves nothing); `Ambiguous` reporting both
indices rather than resolving by position; `NotAnObject` reporting the
stored kind; the search never leaving the airframe it started from (a
sibling airframe's eye and an unparented stray are both invisible); the
binding a `Declared` eye yields and the refusals every other state gets;
`ContainerMismatch`/`ResolvedNodeAbsent` for a report handed a different
archive; all eleven retail airframes `Declared` with distinct authored
triples inside their own subtrees (and 22 `cockpit_camera` nodes across
the archive); and the mechanism spans byte-for-byte — the stored name,
the lookup's recursive self-call, the registration's push of the name's
own address plus its eight-call list (seven searches, one read), the
`+0x54`/`+0x58`/`+0x5c` field loads, the dispatch sequence and the
table's two eye entries reaching the transform through the thunk, the
eye-global loads and matrix fetch, and both `cocky`/`cockz` branch
constants.

The F21-D suite still passes unchanged in *what it measures*: the
binding audit's eye verdict stays `undeclared` — the declared binding
set really does name no eye — while its comments and the census text
now say *why* the verdict survives the T542 measurement.

## Checks run

* `cargo fmt --all -- --check` = 0;
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  = 0; `cargo test --workspace --locked` = 0; and
  `cargo test --workspace --locked -- accept_t542_ --include-ignored` = 0
  (9 discovered, 9 passed).
* Evidence: `private/evidence/T542/acceptance.json` produced by
  `evidence_report_t542` from the recorded acceptance log, production
  discovery of `$CS_GAME_DIR`, `load_engine_image` over
  `$CS_ENGINE_IMAGE` and a second full production re-run of the measured
  chain inside the harness itself; validated with
  `tools/validate_evidence.py --artifact-root private/evidence/T542
  --require-pass`; copy committed as
  `docs/findings/evidence/T542.json`. Artifacts: `cargo-test.log`,
  `eye-placement.json` (the eleven-row census), `eye-mechanism.json`
  (the ten spans plus the registration's decoded call targets).

## Sources used

- `specs/F21-cameras-cockpit-views-and-spyglass.md` (### F21-D),
  `docs/contracts/UI-NETWORK.md`, `docs/contracts/CLI-EVIDENCE.md`,
  `docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`,
  `docs/findings/2026-10-11-t541-horizon-commands-anchor-scenery-to-camera.md`,
  `docs/findings/2026-10-11-t540-cockpit-binding-subtree-content.md`.
- The owner's installation, read-only, over `$CS_GAME_DIR`:
  `ZBD/interp.zbd` (the roster and binding scripts) and `ZBD/planes.zbd`
  (the node array the eye pass walks). Installation fingerprint above.
- The owner-supplied decrypted executable over `$CS_ENGINE_IMAGE`,
  SHA-256 above; disassembled read-only with r2 5.0.7 and a Python
  byte-scan. The measured landmarks: lookup `0x4d8cf0`, registration
  `0x473480` (`cockpit_camera` at `.rdata` `0x62747c`), translate reader
  `0x4d1cc0` and writer `0x4d1d50`, eye globals
  `0x71c328`/`0x71c32c`/`0x71c330` with defaults at
  `0x75d1b8..0x75d1c0`, dispatch head `0x42c5c0`, dispatch table
  `0x42c648..0x42c670` (entries 6 and 7), thunk `0x42c61d`, transform
  `0x42d980`, override branches `0x43e3be..0x43e442` (`cocky`/`cockz`
  at `.rdata` `0x622ebc`/`0x622ec4`).
- `crates/cs_app/src/camera/` (the F21-D audit and T541's idiom this
  hangs off), `crates/cs_content/src/coordinates.rs` (the engine-image
  provenance idiom), `crates/cs_formats/src/gamez/` (the node reader).

**No original data is committed.** The artifacts carry counts, stored
names, offsets, digests and byte-level instruction pins; no decompiled
code, no extracted script, no asset and no screenshot is in the
repository, and every private output went to `private/`, outside it.
