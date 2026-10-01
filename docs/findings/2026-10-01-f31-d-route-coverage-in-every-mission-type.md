# F31-D: validating AI route coverage in every mission type

Date: 2026-10-01. Task: F31-D "Validate AI route coverage in every mission type"
(`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, section
`### F31-D`, task #128). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
Capability used: `retail` (read-only `$CS_GAME_DIR`). Evidence report:
`private/evidence/F31-D/acceptance.json`, committed as
`docs/findings/evidence/F31-D.json`.

## Scope and the honest boundary

The parent acceptance criteria AC01–AC04 are preserved, and F31-D's minimum
scenario "a moving waypoint and origin shift do not reset progress or trigger
false arrival" is a new `accept_f31_d_` production-path test. F31-D also adds
the retail coverage audit F31-C left to it.

The original 2000 PC **route encoding is not decoded**: F13 locates mission
programs but recovers no route-node layout, unit or trigger rule (F13-A/B/C,
F31-A/B/C findings). F31-D therefore does **not** claim to have measured an
original route. What it validates, over the actual installation, is the
**carrier coverage**: every mission directory of every mission type carries the
observed AI-navigation control member `aiv.zrd`, located by the production
F13-B reader dispatch. The report says this in-band
(`"route_encoding":{"state":"unmeasured"}`) so a carrier-present result can
never be read as a decoded route. The still-unmeasured encoding is filed as a
follow-up (see "Recorded unknowns").

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/routes.rs` (extended): the `MissionType` enum and the
  pure `classify_mission_type(name)` directory-name rule — `M<digits>` is
  campaign, `IA<digits>` Instant Action, `MP<digits>` multiplayer and a prefix
  without digits (or anything else) is `Other`, never a silent campaign entry.
- `tools/cs_inspect/src/routes.rs` (extended): the `routes --coverage` mode
  (F31-D). It walks `ZBD/<group>/<mission>`, classifies each mission, reads its
  `zrdr.zbd`, dispatches it through the production F13-B `discover_container`
  and checks that a located mission program names `aiv.zrd`. It reports per
  mission type and fails closed (exit 3) on a missing archive or carrier; exit
  4 without an installation, 2 on bad/conflicting input, 1 on a runtime
  failure. `--cs-path` wins over `CS_GAME_DIR`.
- `crates/cs_sim/tests/accept_f31_d_navigation_coverage.rs` (new): the AC04
  minimum scenario through the production `follow_route`.
- `tools/cs_inspect/tests/accept_f31_d_route_coverage.rs` (new): the synthetic
  and retail coverage acceptance tests.
- `tools/cs_inspect/tests/evidence_report_f31_d.rs` (new): the evidence
  harness.
- This file.

**One observable failure:** before F31-D no production path reported whether
the original installation's mission directories — across campaign, Instant
Action and multiplayer — carry the AI-navigation control data F31's runtime
follower must consume, and no `accept_f31_d_` test exercised a moving waypoint
plus an origin shift through the production follower. Removing the carrier
check makes `accept_f31_d_coverage_fails_closed_on_a_missing_carrier_or_archive`
fail (a hidden carrier reports exit 0); removing the frame-origin application
makes `accept_f31_d_moving_waypoint_and_origin_shift_do_not_reset_progress_or_false_arrive`
fail (a local-position read is a false arrival).

## The designed behavior

- **Mission types are a directory-name classification, not a decode.**
  `classify_mission_type` matches `M`/`IA`/`MP` followed by at least one
  ASCII digit, the same mission-directory shape F13-B's `mission_scope` and
  F14-E's campaign layout already walk. It is `inferred` evidence at the level
  of a member name, never a claim about contents.
- **Coverage fails closed.** The audit's denominator is every
  `ZBD/<group>/<mission>` directory, not a filtered list. A mission missing its
  `zrdr.zbd`, a non-reader archive or a missing `aiv.zrd` member stays a row
  with `program_present:false`/`carrier_present:false` and makes the run exit
  3; an installation with no mission directory is exit 3, never an empty pass.
- **The audit reads no original bytes into the report.** A row carries the
  program path and the SHA-256 of the archive only; the carrier check is a
  located-program member name.
- **AC04 through production code.** `follow_route` samples the route frame
  once per tick; the new test starts the aircraft at a node's authored *local*
  position under a displaced frame and requires no arrival (an ignored frame
  would falsely arrive), then shifts the frame again and requires the set-owned
  progress to stay monotonic and reach both moved waypoints.
- **No Bevy, no clock.** `cs_content` and `cs_sim` keep their F31-A/B/C
  boundaries; the boundary tool links both crates.

## Retail corpus result (`cs-inspect routes --coverage --cs-path $CS_GAME_DIR`)

Read-only `$CS_GAME_DIR`; no file written inside it. Exit 0, `coverage:true`:

| Count | Value |
| --- | --- |
| mission directories (`ZBD/<group>/<mission>`) | 53 |
| campaign (`M##`) | 24 |
| Instant Action (`IA#`) | 8 |
| multiplayer (`MP#`) | 21 |
| other | 0 |
| carrying `aiv.zrd` | 53 |
| not carrying it | 0 |

Every campaign, Instant Action and multiplayer mission directory of the
owner's installation dispatches as a reader archive and locates the observed
`aiv.zrd` AI-navigation control member. This is the coverage the task asked
for, at the level the data supports today.

## Recorded unknowns (recorded, not guessed; filed as follow-ups)

- **The original route encoding, node layout, units and trigger rule are not
  measured.** The audit validates the carrier member, not a route. Affected
  content: every AI aircraft that will follow an original route once missions
  are decoded. **Filed as #455 / `F31-ROUTE-ENCODING` (measure and decode the
  original AI route encoding carried by `aiv.zrd`)** with `create_tasks`; it
  gates any "original route followed" claim and depends on #52 (F13-D, the
  mission-language decode plan).
- **`aiv.zrd`'s semantics are inferred from its name.** F13-B records mission
  control member names (`aiv.zrd`, `objectives.zrd`, `targets.zrd`) as
  `inferred`; whether `aiv.zrd` is the route carrier or only one input to it is
  not decoded here. The report and this note label it as an observed name.
- **Whether the Instant Action and multiplayer directories are launchable
  scenarios stays unmeasured** (F14-D #388). F31-D counts and checks their
  mission archives; it makes no launchability claim.
- **Lateral route rejoin through the integrated flight loop is open** (#451,
  recorded in `2026-10-01-f31-ecs-integrated-follower-gap.md`). F31-D's AC04
  test drives the F31-B/C kinematic follower, as the stage's tests do.

## Mutation probes (tests fail when the behavior is removed)

Each probe was applied to the working tree, the named test was run, and the
tree was restored (working tree verified clean). No probe was committed.

1. `ReferenceFrameSample::world_position` made to ignore `origin_m` →
   `accept_f31_d_moving_waypoint_and_origin_shift_do_not_reset_progress_or_false_arrive`
   `FAILED` (`left: 1, right: 0` on the tick-0 false-arrival assertion).
2. The `aiv.zrd` carrier check replaced with an unconditional `true` →
   `accept_f31_d_coverage_fails_closed_on_a_missing_carrier_or_archive`
   `FAILED` (a hidden carrier reported exit 0 instead of 3).
3. `classify_mission_type` made to return `Campaign` always →
   `accept_f31_d_mission_type_classifies_the_observed_directory_families`
   `FAILED` (`left: Campaign, right: InstantAction`).

## Commands run (final tree)

```
cargo fmt --all -- --check                                              -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                         -> 0 (no failures)
cargo test --workspace --locked -- accept_f31_d_ --include-ignored      -> 0 (9 tests selected, all passed)
```

The 9 selected tests are 1 unit in `cs_content`
(`mission_type_classifies_the_observed_directory_families`), 1 unit in
`cs_inspect` (`coverage_summary_never_passes_an_empty_denominator`), 4 synthetic
and 1 retail integration in `cs_inspect/tests/accept_f31_d_route_coverage.rs`,
and 2 in `cs_sim/tests/accept_f31_d_navigation_coverage.rs`. The retail test is
`#[ignore = "requires CS_GAME_DIR"]`, so CI skips it; the implementing and
reviewing agents run it with `--include-ignored` and `CS_GAME_DIR` set.

The consumer trace (`cargo run -p cs_inspect -- routes --coverage --cs-path
$CS_GAME_DIR`):

```
{"schema":"cs-inspect-routes-coverage/v1","source":"<install>","retail":true,
 "route_encoding":{"state":"unmeasured","claim_id":"f31d.route_encoding","reason":"..."},
 "carrier_member":"aiv.zrd","mission_count":53,"covered":53,"coverage":true,
 "types":[{"type":"campaign","mission_count":24,"covered":24},
          {"type":"instant_action","mission_count":8,"covered":8},
          {"type":"multiplayer","mission_count":21,"covered":21},
          {"type":"other","mission_count":0,"covered":0}], ...}
```

## Evidence

`private/evidence/F31-D/acceptance.json` is derived entirely from the recorded
acceptance log, production discovery of `$CS_GAME_DIR`, the shipped binary's
`routes --coverage` output, `rustc --version` and `Cargo.lock`; it passes
`tools/validate_evidence.py --require-pass` and is committed as
`docs/findings/evidence/F31-D.json`. `unknowns` is empty on purpose: this
task's own acceptance is complete, and the product-coverage limits above are
moved, never deleted — they are quoted in the report's `review.method`, written
up here and filed as follow-ups. This stage awards at most **checked**; it makes
no visual, audible or ordinary-play claim and no original-route claim.

The report's `assertions` array lists every selected `accept_f31_d_` test,
including the two unit tests whose libtest names carry a module path
(`routes::tests::...`), so it matches the nine tests the acceptance run
executed.

## Review status and identity

Implemented by `deepseek-1`. At the time of submission this branch has **not**
been independently reviewed; a Rally review claim is expected to run the four
checks above (including `--include-ignored` with `CS_GAME_DIR` set), reproduce
the mutation probes, regenerate the evidence report on the reviewed and rebased
commit, and record the actual implementer and reviewer identities and whether
the reviewer's context was fresh. This record makes no original-reference
claim, so no agent review is offered as original evidence, and no agent may
self-award `verified_original` or `release_approved`.

## Sources

- `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (`### F31-D`,
  acceptance tests AC01–AC04, non-negotiable behavior, "Research boundary"),
  `docs/contracts/FLIGHT-PHYSICS.md`, `docs/contracts/CLI-EVIDENCE.md`.
- `crates/cs_content/src/routes.rs` (F31-A record, F31-D mission types),
  `crates/cs_sim/src/ai/navigation.rs` (F31-A/B follower, F31-C frame
  sampling), `tools/cs_inspect/src/routes.rs` (F31-A/C/D consumer).
- `docs/findings/2026-09-29-f13-b-locate-and-classify-programs.md` (mission
  scope, reader members, the `aiv.zrd` name observation),
  `docs/findings/2026-09-29-f14-d-retail-baseline-inventory.md` (IA/MP
  directories, #388), `docs/findings/2026-09-29-f14-e-*` (the campaign layout
  and evidence-harness precedent),
  `docs/findings/2026-10-01-f31-c-original-routes-and-moving-frames.md`
  (limitation 5 assigns retail coverage here),
  `docs/findings/2026-10-01-f31-ecs-integrated-follower-gap.md` (#451).
