# F64-D: verifying the original custom-aircraft reference and the explicitly scoped optional saves

Date: 2026-10-08. Task: F64-D "Verify original custom aircraft and explicitly
scoped optional saves" (#257),
`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, section
`### F64-D`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Evidence report: `docs/findings/evidence/F64-D.json` (copy of
`private/evidence/F64-D/acceptance.json`, validated with
`python3 tools/validate_evidence.py private/evidence/F64-D/acceptance.json
--artifact-root private/evidence/F64-D --require-pass`).

Owner paths used: `crates/cs_formats/src/legacy_profile/`,
`crates/cs_content/src/legacy_import.rs`, `crates/cs_app/src/ui/import.rs`,
`tests/` (`crates/cs_app/tests/{f64_d_support,accept_f64_d_optional_save_switch,accept_f64_d_retail_custom_aircraft_and_optional_saves,evidence_report_f64_d}`),
`docs/findings/`. **No wiring edits outside the owner paths**: every module
this stage touched was already declared by its crate.

Capabilities used: **`retail`** (read-only reads of `$CS_GAME_DIR`, plus the
production ROF mount over `GOSDATA/ASSETS/crimson.rof`) and ordinary
build/test. The original executable was never run; nothing here is a claim
about original runtime behaviour, and no original byte layout was read.

## Three things this stage found

1. **The switch was a string, not a setting.** The inventory declared
   `disable_switch: "cs.profile.legacy_save_import"` on all three save rows
   (F64-A) and the consumer read a `bool` its caller supplied (F64-C), but
   nothing in production declared that key as a profile setting, so AC04 —
   "optional old-save migration can be disabled" — could only be exercised by
   a test hand-building the boolean. It is now a real rule
   (`cs_content::legacy_import::legacy_save_import_rule`, `SettingApply::Live`,
   labels `on`/`off`, default `on`), which is what the settings module means by
   "the feature that owns a setting owns its rule", and it is read back with
   `legacy_save_import_enabled(&SettingsState)`.
2. **The consumer asked the layout capability first, so the switch was
   invisible in production.** `run_attempt` refused with `no_measured_layout`
   as soon as a class had no layout — which is *every* class today — and only
   then called `plan_import`, where the switch lives. Turning the enhancement
   off therefore changed nothing a player could see. The class-level decision
   now runs first, still propagating the producer's own
   `ImportRefusal::EnhancementDisabled` whole; `plan_import` keeps its own
   check for callers that bypass the flow.
3. **A recorded `referenced_by` path is an asset spelling, not a file at the
   installation root.** This stage's first draft asserted that
   `ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT` appears in the 229-file manifest
   and failed against the real installation: it does not. The path resolves as
   a member of the shipped base container
   (`GOSDATA/ASSETS/crimson.rof#ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT`)
   through the production ROF mount. The support reader now resolves a
   reference either way and records **which** of the two it was
   (`ReferenceSource::InstallFile` vs `ReferenceSource::ContainerMember`), and
   the test fails when nothing resolves the reference at all.

## What the tests pin (`accept_f64_d_*`, 5 tests)

| test | sheet criterion / behaviour |
| --- | --- |
| `..._optional_save_migration_can_be_disabled_while_new_campaigns_still_work` | **AC04, the stage's minimum scenario**: profile created, switch taken from its default to a committed `off` through `ProfileSession::set_setting`, an optional save refused **by name** with the switch, the required custom-aircraft class still importing, a campaign run started and committed on the same profile, a reopen that reads the stored `off` back, an unusable value refused without moving it, a second campaign still starting |
| `..._the_switch_is_consulted_before_the_missing_layout_capability` | the ordering: switch off + no layout → `enhancement_disabled` naming the switch; switch on + no layout → `no_measured_layout`; required class + switch off → `no_measured_layout` |
| `..._the_named_switch_is_the_declared_profile_setting` | the scoping: the rule's key *is* the inventory's `disable_switch`; exactly the three save classes are optional and each carries it; the rule is valid, live, default `on`; a stored `off` reads as off, an unusable stored value recovers to the default and is reported, and a build whose catalog omits the rule still reads the value this feature's rule names |
| `..._retail_the_custom_aircraft_reference_resolves_to_the_shipped_construction_screen` | `retail`, `#[ignore]`: the recorded reference resolves (member of the inventoried base container), the shipped member is the whole construction screen, and all three saved-plane slot markers F64-B measured are still in it |
| `..._retail_optional_saves_are_switchable_and_no_shipped_file_reaches_a_report` | `retail`, `#[ignore]`: no shipped file matches a runtime-created save/plane shape; all 179 in-cap files offered as a save class are refused with `enhancement_disabled` when the switch is off and `no_measured_layout` when it is on; the required class with the switch off gives `no_measured_layout`; sources re-read identically and the probe destination is untouched |

### Mutation checks (the tests fail when the implementation is removed)

1. The early switch check in `crates/cs_app/src/ui/import.rs` disabled
   (`&& false`): `accept_f64_d_the_switch_is_consulted_before_the_missing_layout_capability`
   failed (`left: "no_measured_layout"`, `right: "enhancement_disabled"`) and
   the retail test failed with every offer reporting
   `switch off gave no_measured_layout`.
2. `legacy_save_import_enabled` stubbed to `true`:
   `accept_f64_d_the_named_switch_is_the_declared_profile_setting`
   (`assertion failed: !legacy_save_import_enabled(&off)`) and
   `accept_f64_d_optional_save_migration_can_be_disabled_while_new_campaigns_still_work`
   ("the store's live value is what the import reads") failed.

Both mutations were reverted; the committed tree is the restored one.

## The retail measurement (`switch-trace.json`)

Re-derivable from the artifact, all measured on this installation
(`install_sha256 c14a876f…`, `content_sha256 148a24b7…`):

- 229 inventoried files; 179 within the 4 MiB designed import cap, 50 refused
  at the proposal constructor; **0** files match `SavedGames`, `Planes\`,
  `.sav`, `Status.dat`, `Mission.` or `Persist.` — the originals are
  runtime-created, so no byte layout has ever been read.
- The 1 recorded original-content reference resolves (member of the shipped,
  inventoried base container) with length and SHA-256 recorded; the member
  still holds `object HMA[4]`, the per-slot plane label and the `2243` fill
  callback.
- The declared setting, taken through one production transaction: created
  profile reads `on` (declared default) → `set_setting(…, "off")` is
  `AppliedLive` → campaign run `f64d.evidence.run` starts and commits while it
  is off → a reopened session reads `off` back and still carries the run.
- The consumer census over every shipped file: 179 × `enhancement_disabled`
  with the switch off, 179 × `no_measured_layout` with it on, 1 ×
  `no_measured_layout` for the required class with the switch off; 0
  destination writes, 0 sources changed.

## Deliberately not done

- **No front-end dialog.** `crates/cs_app/src/ui/front_end/` is not an owner
  path; drawing the refusal is follow-up **#757**.
- **No persistence of a confirmed import.** `crates/cs_app/src/profile.rs` is
  not an owner path; the settings transaction this stage uses is the store's
  existing public API, and writing a `ConfirmedImport` is follow-up **#756**.
- **No byte-layout or original-runtime claims.** No save or plane file ships,
  none has been read, and the original executable was never run; the inventory
  rows' `evidence` stay `Unknown` and every `unknowns` entry of the report is
  empty only because *this* report's own measurements all resolved.
- **No default change.** The rule's default is `on`, a designed value
  consistent with F64-C's recorded production default; the import still needs
  the explicit owner action and is still refused under
  `LayoutAdmission::MeasuredOnly`.

## Reviewer regeneration (2026-10-08)

Per `docs/contracts/CLI-EVIDENCE.md` the reviewer regenerates the report on
the reviewed commit. The reviewer is `bunny-alpha-2` in a **fresh session
context** (the implementer was the same agent name in an earlier, separate
session — recorded here so the review's independence is auditable), on the
branch rebased onto `a97dd67a` (head `676fe307`, tree `ba4b650f`):

- Acceptance run of record, reviewer side:
  `cargo test --workspace --locked -- accept_f64_d_ --include-ignored` → exit
  0, **5 discovered / 5 executed / 5 passed** (3 synthetic, 2 retail), logged
  to `private/evidence/F64-D/cargo-test.log`.
- The harness was re-run against that log and installation; the regenerated
  `private/evidence/F64-D/acceptance.json` passes
  `tools/validate_evidence.py … --require-pass` (exit 0) and so does the
  re-committed copy `docs/findings/evidence/F64-D.json`. Compared with the
  implementer's report, **every measurement is unchanged** — 229 inventoried
  files (179 in cap, 50 over), 1 of 1 reference resolving as a container
  member, 179/179/1 refusal census, 0 destination writes, the persisted `off`
  and the still-running campaign run; only `candidate_tree`, `created_at`,
  the two artifact hashes and `review.identity` differ. `candidate_tree`
  `ba4b650f` is the tree of `676fe307`, the commit whose code was tested; the
  commits above it carry only this report copy and this note.
- Independent mutation checks by the reviewer, both reverted (tree clean
  afterwards, tests green again): (1) the early switch check in
  `crates/cs_app/src/ui/import.rs` disabled →
  `accept_f64_d_the_switch_is_consulted_before_the_missing_layout_capability`
  fails (`left: "no_measured_layout"`, `right: "enhancement_disabled"`);
  (2) `legacy_save_import_enabled` stubbed to return `true` → all three
  synthetic `accept_f64_d_*` tests fail.
- Review outcome: no code or test fix was needed; the change stayed inside
  the owner paths and no protected path is touched.

## Follow-ups

- Existing: **#756** (persist a `ConfirmedImport`), **#757** (draw the report
  in the front-end dialog), **#460** (assemble the production settings
  catalog at session start — until it runs, `legacy_save_import_rule()` is
  declared but no runtime session declares it, exactly like the locale rule).
- New: **#790** (`F64-E`) — the inventory's `referenced_by` is `&[&str]`
  with no field saying *where* a path resolves (installation root or
  container member), a distinction this stage had to measure by trial.
  Typing that field would keep the next reader from asserting the wrong one.
