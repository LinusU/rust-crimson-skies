# F13-A: script-container inventory and disassembly-neutral evidence schema

Date: 2026-09-29. Task: F13-A "Build script inventory and disassembly-neutral
evidence schema"
(`specs/F13-mission-language-discovery-and-compatibility-closure.md`, section
`### F13-A`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capability
used: ordinary build/test only. `CS_GAME_DIR` was not read for this stage and
no evidence report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/script_raw/mod.rs` (new): module doc, the two
  submodules and re-exports.
- `crates/cs_formats/src/script_raw/evidence.rs` (new): `ByteSpan`,
  `ResearchMethod`, `Confidence`, `LocatorKind`, `EvidenceLocator`,
  `EvidenceError`, `ScriptEvidence` (`new`, `establishes_semantics`) and
  `MAX_NOTE_BYTES`.
- `crates/cs_formats/src/script_raw/inventory.rs` (new): `ScriptSource`,
  `ScriptRole`, `Candidacy`, `DispatchOutcome`, `FormatDiscriminator`,
  `RecordKind`, `ReferenceKind`, `RecordReference`, `InstructionStatus`,
  `InstructionClaimError`, `ScriptRecord` (`establish_instructions`),
  `InventoryFinding`, `ScriptContainerEntry`, `InventoryStats`,
  `ScriptInventory` and the entrypoint `inventory_scripts`.
- `crates/cs_formats/tests/script_raw/main.rs` (new; the integration-test
  target `script_raw`): the `accept_f13_a_*` tests.
- Wiring only: `crates/cs_formats/src/lib.rs` (`pub mod script_raw;` and one
  doc paragraph).

**Not created in this stage:** `tools/cs_inspect/src/script_discovery.rs`.
Running the inventory over the installation and reporting it is F13-B's
`retail` work; a command with nothing but synthetic input would only
duplicate the tests (the same reasoning F06-A recorded for `cs-inspect zbd`).

**One observable failure:** an inventory that reports only what a reader
decodes hides bytes. With the unclaimed-range records removed from
`inventory_scripts`, the trailing bytes after the last INTERP script (which
carry a printable run) appear in no record, and
`accept_f13_a_inventories_every_candidate_without_claiming_instructions`
fails in its coverage check. The mirrored failure — a scan lead promoted to
"instructions" — is
`accept_f13_a_only_semantic_evidence_establishes_instructions`.

## What the inventory records

For every container handed to `inventory_scripts` (provenance label,
installation-relative path, bytes) one `ScriptContainerEntry`:

| Field | Source |
| --- | --- |
| dispatch outcome | the F06 two-key `zbd::dispatch`; a refusal keeps its code |
| candidacy and likely role | from the dispatched family (table below) |
| records, each with byte range, record kind, format discriminator, references, instruction status, evidence | the F07 INTERP reader for INTERP containers; one opaque record otherwise |
| findings | a reader refusal, a lead overflow |

| Family | Candidacy | Role | Confidence | Why |
| --- | --- | --- | --- | --- |
| interp | candidate | `loading_script` | documented | F07 / FORMAT-NOTES "INTERP observed subset" |
| animation, `cam_anim.zbd` | candidate | `camera_animation` | inferred | name only (F06-A role rules) |
| animation, `mis_anim.zbd` | candidate | `mission_animation` | inferred | name only |
| reader (`zrdr.zbd`) | candidate | `reader_file` | inferred | name only |
| texture, sound, gamez | excluded (listed, not searched) | — | — | no committed evidence ties these families to script, animation-event or binding records |
| dispatch refused | candidate | `unknown` | unknown | an unroutable container is not evidence that it holds no script |

## Design decisions

- **No record is ever instructions by default.** `InstructionStatus` has three
  states: `container_structure` (a decoded INTERP header or index entry),
  `unestablished` (every body, with the reason) and `established`. Only
  `ScriptRecord::establish_instructions` reaches the last one, and it refuses
  a scan, any evidence below `documented`, a structure record and evidence
  that points outside the record. The inventory never calls it (AC01).
- **INTERP bodies stay unestablished.** F07 decoded them as loading commands.
  Whether they belong to the mission language is exactly what F13 has to find
  out, so "decoded by a reader" and "is an instruction stream" are kept
  separate (spec deliverable: INTERP loading scripts, reader files, camera and
  mission animation are distinct until evidence proves relationships).
- **Every byte is in a record.** A decoded INTERP container gets an
  `unclaimed` record for each range its header, index and scripts do not
  cover; a container the reader refuses becomes one `opaque` record with the
  refusal as a finding. Undecoded containers are one `opaque` record each.
- **Scans find leads, not meanings.** Printable-ASCII runs of at least four
  bytes are recorded as `string_lead` references at `lead` confidence, capped
  at 256 per record with the overflow counted (`leads_truncated`). References
  are spans: no container bytes, names included, are copied into the
  inventory, so a report built on it cannot leak original text.
- **The evidence schema is disassembly-neutral.** `ScriptEvidence` is method,
  confidence, locator and a note of at most 280 bytes. Static analysis of the
  executable is recorded as a module and an RVA plus a summary; an original
  run as a capture id and tick; a decode as a container span; a document as a
  citation. Each method must use its own locator kind, a scan is capped at
  `lead` and a document at `documented`. There is no field for code bytes and
  no `verified_original` confidence (spec non-negotiable #2, AGENTS rule 8).
- **The opcode ledger is not defined here.** Opcode spelling, reachability and
  the `UnsupportedMission` trace (AC02, AC03) need the program format F13-B
  has to find first; defining them now would guess the unit of an opcode.

## Test inventory (`accept_f13_a_*`, `crates/cs_formats/tests/script_raw/main.rs`)

| Test | Covers |
| --- | --- |
| `inventories_every_candidate_without_claiming_instructions` | AC01 over six authored containers: roles, exclusion, a refused dispatch, full byte coverage, INTERP split, unclaimed tail with its lead, zero established records |
| `refused_interp_stays_visible_as_opaque_record` | failure case: F07 reader refusal kept as a finding plus one opaque record |
| `header_mismatch_is_an_unknown_candidate` | failure case: wrong bytes at the INTERP path are a refused, unknown-role candidate |
| `only_semantic_evidence_establishes_instructions` | scan and inferred evidence refused, structure refused, out-of-record evidence refused, then an accepted claim |
| `evidence_schema_is_disassembly_neutral` | locator/method agreement, confidence ceilings, note cap, empty note |
| `string_leads_are_capped_and_counted` | lead cap and overflow finding, minimum run length |
| `inventory_order_is_stable` | case-insensitive path order independent of input order |

## Mutation probes

Each mutation was applied to `inventory.rs`, the tests run, and the file
restored:

- gap records disabled → `inventories_every_candidate_without_claiming_instructions` fails (coverage);
- the evidence check in `establish_instructions` disabled → `only_semantic_evidence_establishes_instructions` fails;
- texture/sound/gamez turned into candidates → `inventories_every_candidate_without_claiming_instructions` fails.

## Recorded unknowns

- Where the mission programs, animation event records and native binding
  tables actually live is unknown. Candidacy here comes from family and name
  alone; F13-B (`retail`) has to confirm or refute each role against the
  installation, including whether any excluded family (GameZ in particular)
  holds script data.
- Containers outside `zbd/` (the executable's PE image, ROF members) are
  accepted by `inventory_scripts` but are refused by the ZBD dispatch and
  listed as unknown-role opaque candidates; no reader exists for them in this
  crate yet. Locating binding tables in them is F13-B/F13-C work.
- The text encoding of string leads is unknown; they are byte spans only.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f13_a_ --include-ignored
```

## Sources

`specs/F13-mission-language-discovery-and-compatibility-closure.md`,
`docs/contracts/SCRIPT-MISSION.md`, the F06-A findings (family and role
rules), the F07-A/F07-D findings (INTERP layout and loading classification).
