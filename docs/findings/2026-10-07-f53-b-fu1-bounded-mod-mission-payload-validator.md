# F53-B-FU1: A real bounded validator for sandboxed mod mission payloads

Date: 2026-10-07. Task: F53-B-FU1 (#743) "F53-B follow-up: a real bounded
validator for sandboxed mod mission payloads"
(`specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
non-negotiable 2; the follow-up was filed by F53-B #213). Shared contracts:
`docs/contracts/IDENTITY-CONTENT.md` and
`docs/contracts/SCRIPT-MISSION.md` ("IR requirements", "Program security").
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Owner paths (decided before editing)

The task said to decide them, and pointed at #468 (F53-A-FU1) as the same
decision problem. The constraint is `docs/01-ARCHITECTURE.md`: `cs_content`
may not depend on `cs_script`, so a payload cannot be both decoded to the IR
and validated inside the crate the mount lives in. The choices:

- **`crates/cs_app/src/ui/mods.rs` (new)** — the host side of a mod mount.
  The F53 sheet already lists this exact path as F53's `cs_app` owner path,
  and `cs_app` is the crate that already owns the crossing
  (`cs_app::control_lowering` lowers measured control records into
  `cs_script::ir::MissionProgram` for the retail census). Everything new
  lives here.
- **`crates/cs_app/src/lib.rs`** — wiring only: `pub mod mods;` inside the
  existing `pub mod ui { … }` block plus two doc sentences (AGENTS rule 1).
- **`crates/cs_content/src/mods/mount.rs`** — F53 sheet owner path; one
  doc-comment paragraph of the `ProgramValidator` trait, whose rationale
  ("no measured decoder from mod-authored bytes into a mission program
  exists in this workspace yet") this task makes false. No code changed.
- **`docs/findings/`** — this file.

Not taken: a new crate, `crates/cs_content/src/mods/` code, `cs_script`
changes. Tests stay **inline in `crates/cs_app/src/ui/mods.rs`** so they sit
inside an owner path rather than in `crates/cs_app/tests/`.

**Review rebasing note (2026-10-08).** While this task was in review, F53-C
landed on `main` (`docs/findings/2026-10-07-f53-c-selection-diagnostics-and-private-export.md`)
and created that very file with the mod-screen projection (`ModsView`,
`ModRow`, `ModsNotice`, `lobby_compatibility`), the `ModSelection::mount`
producer that takes a `ProgramValidator`, and its own `accept_f53_c_*`
suite. The rebase was an add/add conflict in `crates/cs_app/src/ui/mods.rs`
and a doc conflict in `crates/cs_app/src/lib.rs`; the review resolved both by
keeping **both stages in the one owner file** (shared module doc, shared
imports, one `mod tests` carrying both suites — 11 tests in the file, 6 with
this task's prefix). Nothing of F53-C was dropped or weakened, and its two
tests in that file still pass unchanged.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/ui/mods.rs` (new): `MissionPayloadRefusal`,
  `decode_mission_program`, `MissionPayloadValidator`
  (`cs_content::mods::ProgramValidator`), `mount_selection` (the F53-C
  producer path with the validator attached), `mount_environment` (the raw
  `MountEnvironment` path), and the `accept_f53_b_fu1_*` suite.
- `crates/cs_app/src/lib.rs`: wiring only.
- `crates/cs_content/src/mods/mount.rs`: the stale trait-rationale sentence.
- This file.

**One observable failure:** under `mount_environment(...)` a mod payload
that decodes, lowers and validates **mounts**, and the very same set under
an environment built without that wiring is refused with
`unvalidated_program`. Delete the `with_program_validator(...)` call in
`mount_environment` and
`accept_f53_b_fu1_a_valid_sandboxed_mission_payload_mounts` fails.

**One observable failure on the mount that hosts the mod:** the same is
true of `mount_selection`, which fills F53-C's `ModSelection::mount(…,
validator: Option<&dyn ProgramValidator>)` slot. Replace its
`Some(&HOST_MISSION_PAYLOAD_VALIDATOR)` with `None` and
`accept_f53_b_fu1_the_selection_mount_carries_the_validator` fails with
`UnvalidatedProgram` (probe A below).

Test count: 7 `accept_f53_b_fu1_*` tests, all unit tests in
`crates/cs_app/src/ui/mods.rs`, selected by
`cargo test --workspace --locked -- accept_f53_b_fu1_ --include-ignored`.
The same file also carries F53-C's 2 `accept_f53_c_*` tests after the
rebase merge; both still pass.

## What this stage implements

**The decode chain is the retail chain.** `decode_mission_program(target,
bytes)` runs, in order:

1. `cs_content::stunts::decode_zrd` — the bounded `.zrd` reader (unknown tag,
   a count larger than the bytes left, an over-deep document and trailing
   bytes are all refusals), so nothing after step 1 ever sees a partial
   document;
2. `cs_content::mission_control::measure_control_record` plus the
   `blocks() == 0` check — the measured control-member rule applied to the
   document itself: a payload with no numbered `OBJECTIVE<N>` block is not a
   mission program;
3. `cs_app::control_lowering::lower_control_record` — the adapter the retail
   census runs per row. The program's mission identity is the mount's
   `target` id (`Ok(target.clone())`), never anything the payload asserts;
4. `cs_script::ir::MissionProgram::validate` — called on the bound program,
   exactly as F53 non-negotiable 2 requires. Its verdict is *not* taken on
   trust from the adapter's accounting: `LoweredControlRecord::program()`
   returns a program that has been assembled but not necessarily accepted,
   so step 4 is what refuses it.

The result is `cs_script::ir::ValidatedProgram` — the only type that may be
handed to a runtime — returned to the caller and, through
`MissionPayloadValidator`, used as the mount's verdict.

**The wiring — two attach points, one validator.** The validator is *not* a
caller choice: it is this build's capability, so it is attached where a
mount is built rather than at each call site. `HOST_MISSION_PAYLOAD_VALIDATOR`
is `static`, `'static` and stateless, so both attach points hand the mount a
borrow that outlives any root list:

* `mount_selection(selection, request, base_fingerprint)` — **the mount that
  hosts the mod.** F53-C landed `ModSelection::mount(request,
  base_fingerprint, validator: Option<&dyn ProgramValidator>)` as the
  producer path ("set, roots and validator cannot disagree because the
  selection supplies all three"); this host answers its validator parameter
  with `Some(&HOST_MISSION_PAYLOAD_VALIDATOR)`, so a caller of this function
  cannot forget the gate. `None` is exactly F53-B's fail-closed state.
* `mount_environment(base_fingerprint)` — the raw `MountEnvironment`
  (`MountEnvironment::new(...).with_program_validator(...)`) for a caller
  that assembles its own roots and calls `mount_mods` directly. Roots stay
  the caller's (`with_root`), because only the caller knows where a mod's
  root directory is.

**Refusals.** `classify_validation` marks nine content kinds sandboxed.
`ContentKind::Mission` is the only one with a payload encoding this build can
read, so `Script`, `Objective`, `Trigger`, `Route`, `Instruction`,
`NativeBinding`, `IaScenario` and `MultiplayerScenario` overrides are
refused by name (`MissionPayloadRefusal::UnsupportedKind`) rather than
decoded as if they were missions. Every refusal is quoted in
`MountError::ProgramRejected`; none is a warning and none falls back.

## The payload encoding: measured program, designed packaging

This is the decision the task flagged ("if the payload format for a
mod-authored mission is itself unmeasured … keep the mount refused rather
than inventing a format"). What was decided, and why it is not an invented
format:

- **Measured:** the bytes are a `.zrd` control record — the encoding the
  M01-LC stages measured for a mission's control program
  (`docs/findings/2026-10-04-m01-lc-mission-program.md` and the stage A–E
  findings), read by `cs_content::stunts::decode_zrd`. The tag grammar
  (`1` int, `2` float, `3` text, `4` list, `count - 1` children), the
  one-element wrapper and the flat key/value record are all measured, and
  the fixtures here encode with the production `ZRD_TAG_*` constants so the
  fixture cannot drift from the reader.
- **Designed (labelled as such):** that a mod ships the control record
  document itself instead of the retail `zrdr.zbd` reader archive that wraps
  it in the installation. Original mod support is unmeasured (F53
  "Research boundary"), so *no* mod packaging was measured — but the choice
  here does not invent a byte layout, only which measured layout a mod
  file carries. The alternative (a wrapped reader archive) would additionally
  need an installation path to dispatch on: `cs_formats::zbd::dispatch`
  identifies a reader family by its **installation role**, and a mod has no
  measured `zbd/<group>/<mission>/zrdr.zbd` layout to offer it, so the path
  would have had to be invented too. Recorded as an open unknown below; if
  packaging is ever measured to be the wrapped archive, step 1 changes and
  nothing else does.
- **No format was invented to make the test pass.** Nothing in this task
  adds a serializer, a magic number or a new content kind.

## Unknowns, limitations and where they are resolved

- **The decoded program has no runtime consumer yet.** This stage answers
  the gate question ("may these bytes be enabled?"), and the
  `ValidatedProgram` it returns is the evidence for that answer. Since the
  F53-C rebase merge, `mount_mods` *does* have an in-tree caller —
  `ModSelection::mount`, which `mount_selection` fills with this
  validator — but nothing yet loads a mod mission *program* into a session:
  `ModSelection`'s consumers mount payload **bytes** into a VFS session, no
  stage hands a decoded `ValidatedProgram` to `cs_sim`, and the mod screen
  has no widget/input wiring yet (F53-C's own stage note: "the front-end's
  stage"). Missing capability named, not stubbed.
- **Mod packaging is designed-unmeasured** (see above): a payload is the
  `.zrd` control record, not the retail reader archive. Resolved by F53-D /
  any original-run evidence about mod layout.
- **Eight sandboxed kinds have no payload encoding** and stay refused:
  `Script` is the interesting one, because its retail bytes *are* the reader
  archive (F14 baseline: the `ContentKind::Script` row is the archive's own
  bytes), so a `Script` override cannot mount until the archive form is
  decodable here. Affected content: any mod that overrides `script/*`,
  `objective/*`, `trigger/*`, `route/*`, `instruction/*`, `native_binding/*`
  or an IA/multiplayer scenario's program. F53-C landed without adding an
  encoding for any of them, so the resolvers are now F53-D together with the
  packaging question above; until then those overrides are refused, never
  passed.
- **A payload's mission id must satisfy the `ContentId` key grammar** for
  its objective ids (`lower_control_record` refuses rather than fabricating
  one), so a target id whose key makes an invalid objective id is refused by
  the adapter's own accounting.
- **`cs_content/src/mods/mount.rs`'s trait doc** was corrected (it claimed no
  decoder existed); the *dated* F53-B finding keeps its own record of the
  state when it was written and was not edited.

## Checks run (all exit 0, after the 2026-10-08 review rebase)

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked` → 0, 4046 tests passed / 0 failed
  (438 `test result` lines), including `accept_t383_this_worktrees_effective_target_dir_is_per_worktree`
- `cargo test --workspace --locked -- accept_f53_b_fu1_ --include-ignored` →
  0, 7 tests executed, all passing

**Target-dir remediation.** The first full-suite run of the review failed
exactly one test, `accept_t383_this_worktrees_effective_target_dir_is_per_worktree`:
this checkout's `target/` still held artifacts copied in from another,
still-present checkout (`swe2-max-1` — 898 dep-info files naming it), so the
per-worktree target gate refused to trust any local run. This is environment
state, not source: the gate's own message prescribes the fix, and
`cargo clean --target-dir …/bunny-alpha-2/target` (after restoring the
`CACHEDIR.TAG` cargo requires) removed 15 261 files / 25.4 GiB. Every check
above was then re-run from a cold build; the gate passes and the run of
record is trustworthy.

## Sensitivity probes (run and reverted; none committed)

Each removed one behavior and produced failures named below:

1. `mount_environment` without `with_program_validator` → **all 5** tests
   fail (nothing sandboxed mounts: `unvalidated_program`), the wiring test
   among them.
2. `MissionProgram::validate` made permissive → exactly
   `…_a_payload_that_fails_mission_program_validate_is_refused` fails.
3. The `target.kind() != ContentKind::Mission` gate removed → exactly
   `…_a_sandboxed_target_without_a_measured_encoding_is_refused` fails.
4. The `decode_zrd` refusal swallowed (undecodable bytes treated as an empty
   document) → exactly `…_an_undecodable_payload_is_refused` fails.
5. `cs_script::bindings::lower_program` allowed to drop unbound calls →
   exactly `…_a_payload_with_an_unmeasured_directive_is_refused` fails.
6. **(review, probe A)** `mount_selection` passing `None` instead of the
   validator → exactly `…_the_selection_mount_carries_the_validator` fails
   (`UnvalidatedProgram`), proving the F53-C producer path really carries
   the gate.
7. **(review, probe C)** the `record.blocks() == 0` guard removed → exactly
   `…_a_payload_that_is_not_a_control_record_is_refused` fails, which is the
   coverage the review found missing.

After every probe the tree was restored and the selection re-run green
(7/7).

## Identities

Implementer: `bunny-alpha-2/bunny-alpha-2` (Rally agent `bunny-alpha-2`),
session of 2026-10-07.
Reviewer: `bunny-alpha-2/bunny-alpha-2` — Rally assigned the review to the
same agent token, but in a **fresh session**: this review started from
`request_work` with an empty context (no access to the implementing
session's conversation) and re-derived every check, probe and the F53-C
rebase merge from the branch itself. It is therefore context-fresh but not
a different model instance; per the 2026-10-01 owner directive that is
recorded here rather than implied, and it does not substitute for the
owner's human approval.
No evidence report: the task declares ordinary build/test capability only.
