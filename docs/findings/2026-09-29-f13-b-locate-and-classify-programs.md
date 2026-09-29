# F13-B: locate and classify loading, mission and animation programs

Date: 2026-09-29. Task: F13-B "Locate and classify loading, mission and
animation programs"
(`specs/F13-mission-language-discovery-and-compatibility-closure.md`, section
`### F13-B`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capability
used: `retail` (read-only `$CS_GAME_DIR`). Evidence report:
`private/evidence/F13-B/acceptance.json`, committed as
`docs/findings/evidence/F13-B.json`.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/script_raw/ledger.rs` (new): `ProgramLocator`,
  `LedgerError`, `OpcodeEntry`, `OpcodeLedger`, `ReachedOpcode`,
  `ProgramError`, `walk_program` and `MAX_OPCODE_BYTES`. The typed place a
  reachable opcode's meaning is recorded, and the fail-closed cursor.
- `crates/cs_formats/src/script_raw/discovery.rs` (new): `ProgramKind`,
  `LocatedProgram` (`walk`, `mission_label`), `DiscoveryFinding`,
  `ContainerDiscovery`, `mission_scope`, `discover_container`,
  `ANIMATION_HEADER_BYTES` and the name/path classification rules.
- `crates/cs_formats/src/script_raw/mod.rs`: module declarations and
  re-exports only.
- `tools/cs_inspect/src/script_discovery.rs` (new): the `scripts` command
  (`--coverage`) and its tests.
- Wiring only: `tools/cs_inspect/src/lib.rs` (`pub mod script_discovery;` and
  one doc paragraph), `tools/cs_inspect/src/main.rs` (dispatch one subcommand
  to the owned code, help text and the command lists).
- `crates/cs_formats/tests/script_raw/main.rs`: the `accept_f13_b_*` tests.

**One observable failure:** an empty opcode ledger that skipped an unnamed
opcode would let a program run past an instruction nobody measured. With the
ledger lookup in `walk_program` removed, the first reached counter is accepted
and `accept_f13_b_unknown_opcode_at_reached_pc_fails_with_mission_and_location`
fails: no error carries the mission `zbd/c1/m02` and the locator. The
mirrored failure — `mission_scope` accepting a group or content-root archive —
is `accept_f13_b_mission_scope_is_the_mission_directory`.

## What the discovery locates

`discover_container(label, path, bytes)` routes one candidate container through
the F06 two-key `dispatch` and returns a `ContainerDiscovery`: the located
programs plus the findings it could not turn into a program. It never fails
and never drops a container.

| Family | Program unit | Kind | Confidence | Support |
| --- | --- | --- | --- | --- |
| interp (`interp.zbd`) | one body per F07 script | `loading` | documented | F07 / FORMAT-NOTES INTERP layout |
| reader (`zrdr.zbd`) member | one readable member | `mission` / `mission_animation` / `camera_animation` / `reader_entry` | inferred | member name, then mission path |
| animation | one payload after the 8-byte header | `camera_animation` / `mission_animation` / `unknown` | inferred / unknown | basename only |
| texture, sound, gamez | — | — | excluded | no committed evidence; `DiscoveryFinding::Excluded` |
| dispatch refused | — | — | — | `DiscoveryFinding::DispatchRefused` with the code |

- **Mission scope.** A path is scoped to a mission only when its logical key
  is exactly `zbd/<group>/<mission>/<archive>` (case-insensitive, `/`-joined).
  `zbd/c1/m02/zrdr.zbd` and `zbd/c1/m02/mis_anim.zbd` are mission
  `zbd/c1/m02`; `zbd/c1/zrdr.zbd`, `zbd/interp.zbd` and a deeper path are not.
- **Reader members.** `cam_anim.zrd` is a camera-animation record and
  `mis_anim.zrd` a mission-animation record by name. An observed mission
  control name (`aiv.zrd`, `objectives.zrd`, `targets.zrd`) and every other
  member of a mission's archive are `mission` programs at `inferred`
  confidence; an unnamed member of a group archive stays a `reader_entry`.
- **No bytes are copied.** A located program is a `ProgramLocator`
  (container label, optional member name, byte span) plus the role. The
  `scripts` report and the inventory contain no container bytes.
- **The ledger ships empty.** The public research establishes no Crimson Skies
  mission opcode table (spec F13 "Research boundary"), so `OpcodeLedger::new()`
  names nothing and `walk_program` refuses a program at its first reached
  counter. `word_bytes` (the assumed instruction unit) and `budget` are
  explicit inputs, never hidden constants: the fleet has not measured either.

## The F13-B minimum scenario (AC02)

`LocatedProgram::walk(&OpcodeLedger::new(), 4, 4096)` on the retail
`ZBD/C1/M02/zrdr.zbd` `objectives.zrd` program stops at program counter
`0x0..0x4` with `ProgramError::UnknownOpcode { mission: "zbd/c1/m02",
locator, pc, opcode }`. The same walk refuses a loading body and a
mission-animation payload with the same code, and `Display` renders
``mission `zbd/c1/m02`: unknown opcode 0x… at program counter 0x0..0x4 in
container `retail-zrdr` member `objectives.zrd` at 0x0..0x…``. Tests:
`accept_f13_b_unknown_opcode_at_reached_pc_fails_with_mission_and_location`
(synthetic) and `accept_f13_b_retail_locates_mission_programs` (retail).

## Retail corpus result (`cs-inspect scripts --coverage`)

`$CS_GAME_DIR` (`CS_GAME_DIR` read-only; no file written inside it):

| Count | Value |
| --- | --- |
| `.zbd` containers routed | 184 |
| script containers (interp + reader + animation) | 124 (1 + 62 + 61) |
| families excluded from the search (gamez + sound + texture) | 60 |
| located programs | 1452 |
| loading | 98 (one per `interp.zbd` script) |
| mission | 629 |
| mission_animation | 106 (53 `mis_anim.zbd` containers + 53 members) |
| camera_animation | 16 (8 `cam_anim.zbd` containers + 8 members) |
| reader_entry | 603 |
| unknown | 0 |
| discovery findings over script containers | 0 |

Every container routes and every script container locates at least one
program, so `--coverage` passes and the command exits 0.

Observed reader member names (a name rule, not a decode):

- mission `zrdr.zbd`: always `aiv.zrd`, `dzones.zrd`, `egen.zrd`,
  `location.zrd`, `map.zrd`, `mis_anim.zrd`, `net.zrd`, `objectives.zrd`,
  `targets.zrd`, `weather.zrd`, plus mission-specific members;
- world-group `zrdr.zbd`: always `cam_anim.zrd`, `declient.zrd`,
  `fogvol.zrd`, `landings.zrd`, `templates.zrd`, plus scenery members.

## Recorded unknowns (not guessed)

- **The mission-language opcode table is not measured.** F13-B locates
  programs; it does not decode them. The ledger is empty and the walk refuses
  every program at its first counter. F13-C has to resolve each reachable
  instruction with an isolated probe.
- **The instruction unit is not measured.** `word_bytes` is an argument. The
  4-byte little-endian word used in tests and by the CLI contract is an
  assumption to be measured, not a format claim.
- **The INTERP bodies' relationship to the mission language is not
  established.** They are `loading` programs because F07 decodes the
  container, not because the corpus proved they share the mission VM.
- **The animation payloads' semantics are not established.** A
  `cam_anim.zbd` / `mis_anim.zbd` program is a byte range named by basename;
  its event/instruction encoding is unmeasured.
- **The reader archive header layout stays undocumented** (F06): dispatch
  routes `zrdr.zbd` by role, validates its version-one trailer index and
  reads the members; the leading header bytes are not interpreted.
- **Whether the excluded GameZ family holds script data stays unmeasured.**
  It is listed but not searched; a later stage has to check it.
- **`aim`/`aiv` and the mission control members' encodings are unknown.**
  Only their names and locations are recorded.
- **No original run was observed.** The classification is name/path inference
  from the installation's bytes; it is `inferred`, never `verified_original`.

## Test inventory (`accept_f13_b_*`)

`crates/cs_formats/tests/script_raw/main.rs` (7 unignored + 1 retail) and
`tools/cs_inspect/src/script_discovery.rs` (3 unignored + 1 retail):

| Test | Covers |
| --- | --- |
| `unknown_opcode_at_reached_pc_fails_with_mission_and_location` | AC02 minimum scenario, synthetic |
| `ledger_names_reached_opcodes_and_fails_closed` | ledger lookup, duplicate/empty claims, fail-closed |
| `walk_refuses_empty_truncated_and_unbounded_programs` | empty, bad width, partial word, budget |
| `mission_scope_is_the_mission_directory` | mission path rule and case-insensitivity |
| `classifies_loading_and_animation_programs` | INTERP documented loading, animation by name, walk |
| `reader_members_are_classified_by_name_and_mission_scope` | member name rules, mission scope, group archive |
| `refusals_stay_visible_without_guessing_programs` | dispatch refusal, exclusion, refused index |
| `retail_locates_mission_programs` | AC02 over `$CS_GAME_DIR` |
| `cli_locates_loading_mission_and_animation_programs` | `scripts` report and coverage |
| `cli_coverage_fails_on_a_container_that_hides_its_programs` | fail-closed coverage |
| `cli_refuses_bad_input_and_missing_installation` | exit 2/4 and `--out` inside the install |
| `retail_cli_locates_every_campaign_program` | corpus counts and AC02 over `$CS_GAME_DIR` |

## Mutation probes

- ledger lookup removed from `walk_program` →
  `accept_f13_b_unknown_opcode_at_reached_pc_fails_with_mission_and_location`
  fails;
- `mission_scope` accepting any depth →
  `accept_f13_b_mission_scope_is_the_mission_directory` fails;
- animation containers not carrying their mission →
  `accept_f13_b_classifies_loading_and_animation_programs` fails;
- `--coverage` ignoring findings →
  `accept_f13_b_cli_coverage_fails_on_a_container_that_hides_its_programs`
  fails.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f13_b_ --include-ignored
```

## Sources

`specs/F13-mission-language-discovery-and-compatibility-closure.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`, the
F06-A/F06-D findings (families and corpus counts), the F07-A/F07-D findings
(INTERP layout and loading classification), and the read-only `$CS_GAME_DIR`
listing.

## Reviewer note (2026-09-29)

The F13-B review found one defect in the fail-closed walk: the instruction
budget was checked *after* a word was decoded, so a budget of zero still
accepted the first word of a non-empty program. `walk_program` now checks the
budget before each instruction, so the documented cap holds exactly and a zero
budget decodes nothing; the regression is asserted in
`accept_f13_b_walk_refuses_empty_truncated_and_unbounded_programs`. No other
code change was needed. The reviewer regenerated
`docs/findings/evidence/F13-B.json` on the reviewed tree and recorded the
implementer (`glm-1/deepseek-1`) and reviewer (`deepseek-1`, fresh session)
identities in it.
