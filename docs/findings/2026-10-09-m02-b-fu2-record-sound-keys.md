# M02-B-FU2: the five record-level sound keys, measured in the retail executable

Date: 2026-10-09. Task: `M02-B-FU2` "Measure the five M02 record-level sound
keys and admit them to the record vocabulary" (Rally #801,
`missions/M02.md`, work order `M02-B`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail` (`$CS_GAME_DIR`
read-only, never written) and the owner-supplied decrypted engine image
(`$CS_ENGINE_IMAGE`, read-only, never committed). Evidence report:
`private/evidence/M02-B-FU2/acceptance.json` (validated with
`tools/validate_evidence.py --require-pass`), committed as
`docs/findings/evidence/M02-B-FU2.json`; its artifacts are the acceptance log
and `m02-b-fu2-record-sounds.json`, both under `private/`.

**Everything about the original program below is static code evidence** from
disassembling the owner's decrypted executable. The original program was never
run for this task. Nothing here is `verified_original` runtime behaviour (AGENTS
rule 8), and where a semantic could not be reduced from the code alone it is
recorded as **unknown**, not guessed (AGENTS rule 4).

This is the same method, and the same image, as the M01-LC directive family
(`2026-10-06-m01-lc-directive-{a,b,c,d}-*.md`), whose finding D first located
the seven mission-level `*_SOUND` record keys. Two things differ: the image is
now read at its owner-supplied path outside the installation (AGENTS rule 10,
owner note 2026-10-05 / Rally #798, #804), and this task measures **M02's five
of the seven** — the two M02 does not spell (`OBJECTIVES_WON_SOUND`,
`OBJECTIVES_LOST_SOUND`) are deliberately **not** admitted; see Unknowns.

## Provenance

| Item | Value |
| --- | --- |
| File | `$CS_ENGINE_IMAGE` (`crimson.decrypted.exe`), outside `$CS_GAME_DIR` since Rally #798 |
| Format | PE32, image base `0x400000` |
| Size | 2 580 480 bytes |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` — byte-identical to the directive family's image, so both readings describe one binary, and equal to `cs_content::coordinates::ORIGINAL_IMAGE_SHA256`, which the acceptance test re-hashes on every run |
| Address convention | virtual address (VA); the section table measures `.text` at RVA `0x1000` (VA `0x401000`) = file `0x1000` and `.data` at RVA `0x219000` (VA `0x619000`) = file `0x219000`, so for both sections this note reads, `file offset = VA − 0x400000` |
| Parser | the mission-file parse block of `0x466b70` (`mov ebx, ecx` = mission object, `xor edi, edi`), the same routine finding A measured |
| Name → handle | `0x596120(name)` — re-read here: returns 0 when the global table pointer `[0x639c7c]` or the name is null, otherwise walks the list at `[0x9ad6dc]` comparing per node through `0x596af0`, falling back to `0x599a50(name)` |
| Record lookup | `0x57a090(name, container)` — the named-child lookup the parse calls for every record key |
| Objective-completion tail | `0x46a490` (`CZMission::Update`, `this` in ebx, `edi` = 0), the completion path at `0x46a95b`…`0x46a9b5` |
| Mission end | `0x463c30(mission, ended, arg)`, `this` in esi |
| Tools | `r2` `pd` over the measured sites, `python3` for the section-table walk, the string search, the 32-bit immediate-reference search and the byte assertions the acceptance test now performs |
| M02 data read | production `SourceContext::control_program` over `ZBD/C1/M02/zrdr.zbd`, the member the measured rule chooses |

## What M02's record spells, and how the keys are classified now

M02's control member declares 50 numbered blocks and, outside them, **ten**
record-level keys: the five shape-measured fields (`MISSION_TIMER`,
`PLAYER_INIT` and the three animation lists) and these five sound keys, each
**exactly once**, each carrying exactly one text value — the sound-group name
(the names themselves are original content and are not reproduced here).

`cs_content::mission_control` now classifies them through two vocabularies
instead of one plus a refusal list:

| Key | Mission-object field | Parse site | Consumer site | Measured consumer |
| --- | --- | --- | --- | --- |
| `PRIMARY_COMPLETE_SOUND` | `+0xc64` | `0x466ced` | `0x46a9a0` | objective completion, presentation class **1** |
| `SECONDARY_COMPLETE_SOUND` | `+0xc68` | `0x466d1a` | `0x46a994` | objective completion, class **2** |
| `TERTIARY_COMPLETE_SOUND` | `+0xc6c` | `0x466d47` | `0x46a988` | objective completion, class **3** |
| `MISSION_WON_SOUND` | `+0xc78` | `0x466dce` | `0x463c5b` | mission end, won branch |
| `MISSION_LOST_SOUND` | `+0xc7c` | `0x466dfb` | `0x463c86` | mission end, lost branch |

Every one of the five strings is referenced **exactly once** in the image, by
the `push imm32` in its own parse block — no second reader of any key's name,
and (searching the four-byte displacement over `.text`) no second reader of any
of the five fields beyond the parse's own two writes and the consumer above.

## The parse block, uniform across all five keys

Each key is parsed by the same seven-instruction shape, measured at the parse
site in the table (`push` at `+0x0`, and the recorded offsets from it):

| Offset | Bytes | Measured meaning |
| --- | --- | --- |
| `+0x0` | `68 <imm32>` | `push` the key's own string (the `imm32` is the string's VA; the test reads the bytes there back as the key plus its terminator) |
| `+0x6` | `89 bb <field>` | the field's default: **0**, written before the lookup |
| `+0xc` | `e8 rel32` → `0x57a090` | look the key up among the record's children by name |
| `+0x13` | `83 c4 08` … | null check: a record that does not spell the key keeps the 0 |
| `+0x1f` | `e8 rel32` → `0x596120` | resolve the found child's text through the name → handle lookup |
| `+0x27` | `89 83 <field>` | store the handle into the same field |

So a key the record does not spell stores `0` (the consumers test for `0` and
play nothing), and a key it does spell stores whatever
`0x596120(text)` returns — a **runtime** handle. No tag check guards the text
child, which is the parse fragility finding D already recorded as unknown; it
is not turned into a refusal here, because the original does not refuse either.

## Selector 1: the objective-class chain (`CZMission::Update`)

```
0x46a97d  8b 06             mov  eax, [esi]        ; the objective's class
0x46a97f  48                dec  eax
0x46a980  74 1e             je   0x46a9a0          ; class 1 → +0xc64
0x46a982  48                dec  eax
0x46a983  74 0f             je   0x46a994          ; class 2 → +0xc68
0x46a985  48                dec  eax
0x46a986  75 2d             jne  0x46a9b5          ; anything but 3 reads nothing
0x46a988  8b 83 6c 0c 00 00 mov  eax, [ebx+0xc6c] ; class 3 → +0xc6c
```

Each arm then tests the handle against `edi` (0, measured in this function) and,
if non-null, `push eax; mov ecx, 0x71b438; call 0x46cc50` — the
objective-completed channel, which calls `0x46caf0(mgr, handle, 1.0f, 3, 1.0f)`.
**Measured:** the three class handles are mutually exclusive, an objective of
class 0 (no `IDENTITY` class) or of a class past 3 plays no class sound at all,
and a null handle plays nothing. The acceptance test derives the three arm
addresses from these instruction bytes rather than from the table, so the class
each key carries is checked against the original.

The same completion tail plays the objective's own `COMPLETED_SOUND_GROUP`
handle first (`0x46a95b`, field `+0x550`) — a *different* key, outside this
task's five (finding D).

## Selector 2: the mission-end won flag (`0x463c30`)

```
0x463c49  39 8e 58 0c 00 00  cmp  [esi+0xc58], ecx  ; the mission's won flag
0x463c4f  c7 86 54 0c 00 00 01 00 00 00
                             mov  [esi+0xc54], 1     ; mission-ended flag
0x463c59  74 2b             je   0x463c86           ; won clear → lost handle
0x463c5b  8b 86 78 0c 00 00 mov  eax, [esi+0xc78]   ; won set → MISSION_WON_SOUND
0x463c61  3b c1             cmp  eax, ecx            ; null plays nothing
0x463c86  8b 86 7c 0c 00 00 mov  eax, [esi+0xc7c]   ; MISSION_LOST_SOUND
0x463c8c  eb d3             jmp  0x463c61            ; join the same play call
```

A non-null handle is played as `0x46caf0(mgr=0x71b438, handle, 1.0f, 1, 0)`.
**Measured:** the won flag selects between the two handles, both are read only
here, and a null handle plays nothing. The acceptance test derives both arm
addresses from these bytes: the `je` operand for the lost arm and the
fall-through of the `je` for the won arm.

**Measured, and worth stating plainly:** the branch that *plays* is decided by
the won flag alone, not by the recorded result. Finding F37-D-FU2 measured that
when both mission flags are set the code takes the loss branch for delay and
sound while the recorded result is still success — so `MISSION_LOST_SOUND`
sounding is not evidence of a recorded loss.

## What M02-B-FU2 adds, in owner paths

- `crates/cs_content/src/mission_control.rs` — `CONTROL_RECORD_SOUND_KEY_VOCABULARY`,
  `ControlRecordSound`, `RecordSoundConsumer`, `MeasuredRecordSound`,
  `RecordSoundDisposition` and `record_sound_disposition`, plus a
  `ControlRecordField::Sound` variant so the five keys are **classified** by
  the same record walk as every other record key (their shapes land in
  `record_field_shapes`, and `record_sounds` / `record_sound_shapes` are the
  sound view of those two lists), and a second `FieldSupport` level,
  `effect_measured`, which is what distinguishes them from the shape-only
  fields. The record-level refusal list (`unclassified_record_keys`) now names
  only keys outside **both** vocabularies. `CONTROL_RECORD_KEY_VOCABULARY`
  keeps its five shape-measured fields unchanged, so every M01 assertion about
  it — including the exact `record_fields() == vocabulary` equality on M01's
  record and on the authored fixture — still holds without a single M01 test
  being touched. The classification adds **no field** to
  `MeasuredControlRecord`, so its size (and with it
  `cs_app::mission_control::ControlProgram`'s variant) is unchanged and the
  workspace's `clippy::large_enum_variant` gate stays green without boxing
  another crate's data.
- `crates/cs_app/tests/campaign/m02_b.rs` — the two assertions this task's
  premise changes (M02's sound keys were "unclassified and outside the
  vocabulary"): the retail vocabulary test now asserts they are classified,
  counted once each and measured with a consumer, and that M02's unclassified
  list is **empty**; the synthetic partition test keeps its refusal arm by
  moving it to a key outside both vocabularies
  (`A_RECORD_KEY_NOBODY_HAS_MEASURED`) and adds the classification arm for
  `MISSION_WON_SOUND`. Its other assertions (50 blocks, 190 sites, 36 keys,
  the 2+34+0 partition, the outcome keys) are untouched.
- `crates/cs_app/tests/campaign/m02_b_fu2.rs` — this task's three
  `accept_m02_b_fu2_*` tests, below.
- `crates/cs_app/tests/campaign/evidence.rs` — the test lists and
  `evidence_report_m02_b_fu2_writes_the_acceptance_report`, which writes
  `private/evidence/M02-B-FU2/{cargo-test.log,m02-b-fu2-record-sounds.json,
  acceptance.json}` and is validated with
  `tools/validate_evidence.py --require-pass`; the committed copy is
  `docs/findings/evidence/M02-B-FU2.json`. Two text changes ride along:
  one clause of M02-B's own report text is corrected (it claimed the five
  keys "stay outside the measured vocabulary", which after this task is true
  only of `CONTROL_RECORD_KEY_VOCABULARY`, so it now says exactly that and
  names #801 — and, after #808, that task as well), and both harnesses —
  M02-B's and this task's — stop wrapping
  `str_array`'s already-bracketed output in a second pair of brackets, which
  wrote an empty `unclassified_record_keys` as `[[]]` (M02's five keys were
  written as a nested array the same way; no committed artifact carried
  either, since those lists live in the `private/` artifact).
- Wiring only (AGENTS rule 1): `crates/cs_app/tests/campaign/main.rs` (one
  `mod m02_b_fu2;` and a doc paragraph), `crates/cs_app/tests/campaign/m02_b.rs`
  (`control_binding` / `control_document` widened to `pub(crate)` for the
  follow-up's seam, no logic change).
- No `Cargo.toml` or `Cargo.lock` change: this stage adds no dependency and no
  crate edge. `missions/bindings/` is unchanged.

## Test inventory (`accept_m02_b_fu2_*`, 3 tests)

| Test | Kind | What it pins |
| --- | --- | --- |
| `accept_m02_b_fu2_m02s_five_record_sound_keys_are_measured_with_their_consumers` | retail (`CS_GAME_DIR`) | an independent re-walk of the decoded member classifies its ten record-level keys into exactly the two vocabularies and leaves nothing unclassified; the production measurement reports the same partition with each sound key counted once, in parse order; every disposition is `Measured`, names its consumer (classes 1/2/3 and mission end), cites this finding, carries residual unknowns and distinct addresses; every sound key's measured value shape is `[text]` |
| `accept_m02_b_fu2_the_image_parses_and_consumes_each_sound_key_where_production_says` | engine image (`CS_ENGINE_IMAGE`) | the image is read through `cs_content::coordinates::load_engine_image`, which refuses an unset variable, an unreadable file and bytes that do not hash to the recorded digest (#798); at each `parse_site`: `push imm32` of the key's own NUL-terminated string, zero default into `field_offset`, `0x57a090` named-child lookup, `0x596120` name→handle, handle stored into `field_offset`; at each `consumer_site`: the field is read back through the measured addressing mode; both selectors re-derived from the instruction bytes (class chain arms = the three consumer sites in class order; won-flag `je` target = `MISSION_LOST_SOUND`'s site, fall-through = `MISSION_WON_SOUND`'s, and the lost arm joins the play call) |
| `accept_m02_b_fu2_the_sound_vocabulary_is_entirely_measured_and_answers_for_nothing_else` | synthetic (CI) | every vocabulary key answers `Some(Measured)` with a stable consumer code and different parse/consumer sites — a vocabulary key without a measurement would answer `Refused` and fail here; `MISSION_TIMER`, the four other shape-measured fields, `OBJECTIVES_WON_SOUND`, `OBJECTIVES_LOST_SOUND` and a key no original spells all answer `None`; the consumers partition as 3 classes + 2 mission ends |

Every test calls production code (`record_sound_disposition`,
`MeasuredControlRecord::record_sounds` / `record_sound_shapes` /
`unclassified_record_keys`, `SourceContext::control_program`,
`control_document`'s production discovery). The retail tests are
`#[ignore = "requires CS_GAME_DIR"]`, the image test
`#[ignore = "requires CS_ENGINE_IMAGE"]`; CI runs the synthetic one.

## Mutation probes

Applied one at a time against the final design, each reverted before the next;
the tree carried none of them afterwards (`git status --porcelain` clean but
for this note). Selection: `cargo test --locked --test campaign -- <selection>
--include-ignored`.

| Mutation | Observed result |
| --- | --- |
| `ControlRecordField::from_key` refuses the sound vocabulary (`_ => None`) | **3 of 17 fail**: M02-B-FU2's retail test, M02-B's vocabulary test and M02-B's synthetic partition test — the five keys fall back into `unclassified_record_keys` |
| `record_sound_disposition` answers `None` for every key | **5 of 17 fail**: all three M02-B-FU2 tests plus M02-B's vocabulary test and its synthetic partition test — every `Measured` expectation breaks |
| `MISSION_WON_SOUND`'s `consumer_site` moved one instruction (`0x463c5b` → `0x463c5c`) | **1 fails**: the image test, at *"MISSION_WON_SOUND: the consumer loads the field"* (`left: 134`, i.e. `0x86`, where the measured site holds `0x8b`) — the consumer read fires before the won-arm derivation |
| the class-1 and class-2 `consumer_site` values swapped | **1 fails**: the image test, at *"PRIMARY_COMPLETE_SOUND: the consumer reads the field the parser wrote"* (`left: 3176` = `+0xc68`, `right: 3172` = `+0xc64`) — the swapped site reads the other key's field |

After the last probe the file was restored from a copy taken before the first
(`private/scratch/`), and the three-task selection was re-run green:
`3 passed; 0 failed`.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m02_b_fu2_ --include-ignored` | 0 (3 tests) |
| `python3 tools/validate_evidence.py private/evidence/M02-B-FU2/acceptance.json --artifact-root private/evidence/M02-B-FU2 --require-pass` | 0 (`structurally_valid: true`) |

## Recorded unknowns (not guessed)

- **What sound a name resolves to.** Every one of the five keys is a
  name → handle lookup whose result is runtime state; no shipped file declares
  a name's sound. The measurement says what the original does with the handle,
  never which sound plays. Affects: all five keys. Resolved by: an ordinary-play
  reference capture (M02-C), which is blocked on the owner.
- **The mission won flag's writers.** Measured here only as the selector
  mission end reads (`[mission+0xc58]`); its writers belong to findings A/B and
  F37-D-FU2.
- **The presentation class names.** The dispatch reads an integer `1`/`2`/`3`
  out of the objective's class field; calling them PRIMARY/SECONDARY/TERTIARY is
  finding B's reading of `IDENTITY`, carried as a reading.
- **`OBJECTIVES_WON_SOUND` and `OBJECTIVES_LOST_SOUND` were this task's
  follow-up, since measured and admitted.** They are the other two of the
  original's seven mission-level sound keys (fields `+0xc70`/`+0xc74`,
  consumers at `0x46afd2`/`0x46af9f` per finding D). M02 spells neither and
  this task's five are M02's five, so admitting them stayed outside this
  slice; RECORD-OBJECTIVES-SOUND (#808) then measured the pair — an
  end-of-tick outcome consumer — and admitted both to
  `CONTROL_RECORD_SOUND_KEY_VOCABULARY`, which now carries all seven.
- **No engine operation plays these keys.** `Measured` is a statement about the
  original, not support: nothing in this change lowers or emits a sound.
  M02's record has lowered completely since `M02-B-FU1` (#800) carried the
  `KILL_OBJECTIVE_WHEN_I_COMPLETE` index lists as one list argument each —
  that is a lowering result, not an implemented effect, and the campaign
  gate still needs every row.
- **No original run, and nothing `verified_original`.** Every claim above is
  static code evidence from one byte sequence (`43540fc9…`), read twice — once
  by the directive family on 2026-10-06, once here — from two different paths.
  The work-order ↔ mission join remains M02-A's inference. No reference capture
  exists for M02 (REF-OWNER-FIRST-CAPTURE is blocked on the owner).
- **A non-text value beside one of these keys is not a refusal.** The original's
  parse reads the child's payload without a tag check (finding D's parse
  fragility). Refusing on shape would invent a rule the original does not have,
  so a non-`[text]` value would be *counted with its measured shape* instead;
  no retail record measured here spells one.

## Review round 1 (Rally #801)

Reviewer: `bunny-alpha-1/bunny-alpha-1`, Rally review claim of
2026-10-09T01:09Z in a separate, fresh session. **The reviewing agent is the
same Rally agent identity that implemented this task, so this review is not
independent evidence** (AGENTS.md: a review by the same agent that implemented
the work is not independent evidence); the reviewer's session context was
fresh, and no agent review replaces the owner's human approval. What the
review changed:

### 1. The branch had silently dropped M03-B's wiring and evidence harness (fixed)

`crates/cs_app/tests/campaign/main.rs` no longer declared `mod m03_b;`, and
`crates/cs_app/tests/campaign/evidence.rs` no longer held
`RETAIL_TESTS_M03_B`, `SYNTHETIC_TESTS_M03_B`,
`evidence_report_m03_b_writes_the_acceptance_report` or `parse_m03_b_suite`.
`m03_b.rs` was still on the disk, so nothing looked amiss: all six retail and
two synthetic `accept_m03_b_*` tests and M03-B's report harness simply ceased
to exist as far as `cargo test` was concerned, and neither this task's
selection nor its workspace run could notice the loss. Both were restored
exactly as `origin/main` has them (commit *Restore M03-B's campaign wiring and
evidence harness*), and `cargo test --workspace --locked -- accept_m03_b_
--include-ignored` runs the restored suite green again. A scan of
`git diff origin/main...HEAD` for removed `fn accept_`, `#[test]`, `mod` and
`*_TESTS` lines found no other deletion.

### 2. `ControlRecordSound::TertiarComplete` did not spell the key it named (fixed)

The variant that stands for `TERTIARY_COMPLETE_SOUND` was spelled
`TertiarComplete`, which is not a word; renamed to `TertiaryComplete` before
the vocabulary becomes API other crates match on. The key string itself, the
variant order and every measurement are unchanged.

### 3. The measurements were re-read out of the image by an independent script (confirmed)

Besides running this task's own image test, the reviewer read
`$CS_ENGINE_IMAGE` with a script written from this document's table alone: the
image hashes to `43540fc9…`; each of the five parse sites is a `push imm32` of
that key's own NUL-terminated string, writes the zero default into the recorded
`field_offset`, calls `0x57a090` and `0x596120` by `rel32` and stores the
handle into the same field; each key's string VA is referenced exactly once in
`.text` (by its own parse site); each consumer site is a `mov` from the
recorded field, base `ebx` (`0x83`) in `CZMission::Update` and `esi` (`0x86`)
at mission end; the class chain at `0x46a97d` is `8b 06 48 74 1e 48 74 0f 48
75 2d`, whose arms land on the three class consumer sites in class order; and
the mission-end `cmp [esi+0xc58]` / `je 0x463c86` selects the lost handle with
the won one falling through, the lost arm joining the play call at `0x463c61`.
Every assertion reproduced.

### Checks run by the reviewer (rebased head, clean tree)

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m02_b_fu2_ --include-ignored` | 0 (3 tests) |
| `cargo test --workspace --locked -- accept_m03_b_ --include-ignored` | 0 (8 tests, restored by this review) |
| `cargo test --workspace --locked -- accept_m01_lc_ --include-ignored` | 101 — 161 passed, 3 failed, and the three are #798's, not this task's: `m01_lc_campaign_airframe_pose_retail_m01_binds_from_the_measured_bytes` (`ImageAbsent`), `m01_lc_player_config_retail_m01_binds_player_and_names_unknowns` and `m01_lc_player_airframe_source_retail_m01_has_no_airframe_key_and_the_scenario_does` (both `the campaign airframe is measured engine state`) all reach `mission_start::engine_state_source`, which looks the image up in the installation inventory that no longer carries it since the owner moved `crimson.decrypted.exe` out of `$CS_GAME_DIR` (Rally #798, in review, names that file as one of its sites); this branch touches neither `mission_start` nor the install manifest. Every census and record-field test in the selection passed — `accept_m01_lc_lowering_adapter_the_census_reports_complete_rows_and_keeps_the_gate`, `accept_m01_lc_directive_d_the_block_site_key_census_and_identity_classes_match_the_document` and all 19 `accept_m01_lc_mission_program_*` including `…_record_fields_are_classified_and_an_unknown_key_stays_unclassified` |
| `cargo test --workspace --locked -- accept_m02_b_ --include-ignored` | 0 (17 tests: M02-B's own plus this task's three) |
| `python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py'` | 0 (27 tests) |
| `python3 tools/validate_evidence.py … --require-pass` | 0 |

The committed evidence copy
(`docs/findings/evidence/M02-B-FU2.json`) was regenerated by the reviewing
agent on the reviewed commit with its own `CS_EVIDENCE_REVIEWER` identity, per
`docs/contracts/CLI-EVIDENCE.md`.

## Review round 2 (Rally #801, after the failed landing)

Reviewer: `Devin SWE-2/swe2-max-1` (SWE-2 Max), Rally review claim of
2026-10-09T04:22Z in a separate, fresh session. **Unlike round 1, this
reviewer is a different agent identity and a different model from the
implementer** (`bunny-alpha-1`, opencode mimo-v2.6-flash), so this round is
independent review in the AGENTS.md sense — still not the owner's human
approval, and nothing here is `verified_original`.

Round 1's approval at `219b5f8d` could not land: between its push and the
landing attempt, main gained M02-B-FU1 (#800, which *closed* the
`KILL_OBJECTIVE_WHEN_I_COMPLETE` lowering gap), M04-B and the T796 evidence
work, and the automatic rebase conflicted. This round rebased the branch by
hand and re-verified it.

### 1. The rebase conflict, resolved

`crates/cs_app/tests/campaign/evidence.rs` and `main.rs` were the conflicted
files (main's M04-B and M02-B-FU1 harnesses now occupy the ground the branch
was written on). The resolutions, all checked against the surrounding code:

- the accidental M03-B harness deletion (round 1's finding) resolved to its
  restored state — `RETAIL_TESTS_M03_B`, `SYNTHETIC_TESTS_M03_B`,
  `evidence_report_m03_b_*` and `parse_m03_b_suite` are present, and the
  late "Restore M03-B" commit on the branch became a one-line no-op;
- M04-B's harness (`evidence_report_m04_b_*`, `RETAIL_TESTS_M04_B`,
  `parse_m04_b_suite`) and M02-B-FU1's (`RETAIL_TESTS_M02_B_FU1`,
  `SYNTHETIC_TESTS_M02_B_FU1`, `evidence_report_m02_b_fu1_*`,
  `parse_m02_b_fu1_suite`) are kept whole beside this task's FU2 harness;
- the M02-B harness's NOT-CLAIMED text carries both updates: M02-B-FU1's
  "the record lowers completely, which is lowering evidence only" *and* this
  task's "the five sound keys are outside `CONTROL_RECORD_KEY_VOCABULARY`
  here — M02-B-FU2 (#801) admits them to their own measured vocabulary";
- the `unclassified_record_keys` single-bracket fix and the whole FU2
  harness are unchanged. `git diff origin/main..HEAD` removes nothing from
  main beyond the lines this task's premise requires.

### 2. Stale claims the rebase made false (fixed)

FU1's landing falsified "M02's control record still does not lower / M02
stays Unsupported / the campaign gate stays closed on M02's account". The
claim is corrected in the FU2 harness's `review.method` text and doc
comment, the M02-B harness's doc comment, and this document's unknowns — to
"M02's record lowers completely since M02-B-FU1 (#800), which is lowering
evidence only, not an implemented effect; the campaign gate still needs
every row". Two *pre-existing* stale copies of the same claim on main —
`m02_b.rs`'s module doc and `main.rs`'s M02-B paragraph — were left alone
(not this branch's change) and filed as follow-up task #820.

The `.data` row of the provenance table is also corrected: its
section-table entry is RVA `0x219000` (VA `0x619000`), not "VA `0x219000`";
the `file offset = VA − 0x400000` rule was and stays right. The same
mislabel in `m02_b_fu2.rs`'s `IMAGE_BASE` comment is fixed.
`unclassified_record_keys`' doc in `campaign_bindings.rs` now names both
vocabularies.

### 3. The measurements re-read a second time, independently

This reviewer read `$CS_ENGINE_IMAGE` with a script written from this
document's tables, before trusting the suite: SHA-256
`43540fc9…` confirmed; the section table confirmed `.text` RVA `0x1000` /
file `0x1000` and `.data` RVA `0x219000` / file `0x219000`; every parse site
is `push imm32` of that key's own NUL-terminated string, `89 bb` zero
default into the recorded field, `rel32` calls to `0x57a090` and `0x596120`,
`89 83` handle store into the same field; each key's string VA is referenced
exactly once in `.text`; the class chain at `0x46a97d` is
`8b 06 48 74 1e 48 74 0f 48 75 2d` with arms landing on the three class
sites in class order; the mission-end `cmp [esi+0xc58]` / `je 0x463c86`
selects the lost handle with the won one falling through, the lost arm's
`eb d3` joining the play call at `0x463c61`. The field-displacement sweep
found two extra byte matches per `+0xc68`/`+0xc78` (`0x44dc04`, `0x56d615`,
`0x4f8420`, `0x506189`) — each is a coincidental `rel32` call/jump operand
or a `push` of the small constant, none is a `[base+disp32]` field read, so
"no second reader" stands. Every assertion reproduced.

### Checks run by this reviewer (rebased head)

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m02_b_fu2_ --include-ignored` | 0 (3 tests) |
| `cargo test --workspace --locked -- accept_m02_b_ --include-ignored` | 0 (18 tests: M02-B, M02-T3, M02-B-FU1, M02-B-FU2) |
| `cargo test --workspace --locked -- accept_m03_b_ --include-ignored` | 0 (8 tests, still restored) |
| `cargo test --workspace --locked -- accept_m04_b_ --include-ignored` | 0 (8 tests, main's harness intact) |
| `cargo test --workspace --locked -- accept_m01_lc_ --include-ignored` | 101 — 157 passed, 3 failed, all three the #798 image-relocation fallout round 1 recorded (`m01_lc_campaign_airframe_pose_retail_m01_binds_from_the_measured_bytes`, `m01_lc_player_airframe_source_retail_m01_has_no_airframe_key_and_the_scenario_does`, `m01_lc_player_config_retail_m01_binds_player_and_names_unknowns`): `engine_state_source` looks the image up in the installation manifest that no longer carries it, a code path and test files this branch's diff does not touch |
| `python3 tools/validate_evidence.py … --require-pass` | 0 |

The evidence copy was regenerated again on this round's reviewed commit with
this reviewer's `CS_EVIDENCE_REVIEWER` identity.

### 4. Second rebase, onto the post-#798 main; the image read moves to `load_engine_image`

While CI ran on `bf9554e0`, main gained eight more commits: #798's
engine-image relocation (`load_engine_image` in `cs_content::coordinates`
becomes the one reader of `$CS_ENGINE_IMAGE`, with named
Unset/Unreadable/DigestMismatch refusals, and every existing image-reading
test moved onto it) and M10-B's acceptance work (which touches the same
`campaign/evidence.rs` / `campaign/main.rs` this branch extends). The rebase
onto `32b9d823` auto-merged both shared files; the result was verified by
hand — M10-B's harness (`evidence_report_m10_b_*`, `RETAIL_TESTS_M10_B`,
`parse_m10_b_suite`) and this task's FU2 harness coexist, and `mod m10_b` /
`mod m02_b_fu2` are both declared.

On that base, `m02_b_fu2.rs`'s hand-rolled `CS_ENGINE_IMAGE` read plus inline
`sha256` assert would have shipped as the tree's only non-canonical image
reader — the same check, but a second path. The test now calls
`load_engine_image()` and uses `EngineImage::bytes`: the digest refusal moves
into the loader (where it is a named error, not a bare assertion), and no
behaviour the suite pins changed — the same byte assertions re-verify the
same measured addresses.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m02_b_fu2_ --include-ignored` | 0 (3 tests) |
| `cargo test --workspace --locked -- accept_m02_b_ --include-ignored` | 0 (18 tests) |
| `cargo test --workspace --locked -- accept_m03_b_ --include-ignored` | 0 (8 tests) |
| `cargo test --workspace --locked -- accept_m04_b_ --include-ignored` | 0 (8 tests) |
| `cargo test --workspace --locked -- accept_m01_lc_ --include-ignored` | 0 (202 tests — the three round-2 failures were #798's image-relocation fallout and are resolved by the landed loader work; nothing on this branch touches them) |
| `python3 tools/validate_evidence.py … --require-pass` | 0 |

The evidence copy was regenerated once more on that round's final reviewed
commit (candidate_tree `b772a348`) with the same reviewer identity.

## Review round 3 (Rally #801, landed through the stacked sibling)

Reviewer: `Devin SWE-2/swe2-max-1` (SWE-2 Max), Rally review claim of
2026-10-09T07:18Z in a separate, fresh session — the same reviewer identity
as round 2, still independent of the implementer in the AGENTS.md sense, and
still not the owner's human approval.

Round 2's approval at `10cf265a` could not land either. While it queued,
main gained M02-B-FU3 (#802) and then RECORD-OBJECTIVES-SOUND (#808) — whose
branch had been built **on top of this task's stack** and therefore carried
rebased copies of every implementation commit plus its own seven-key
extension (`OBJECTIVES_WON_SOUND`, `OBJECTIVES_LOST_SOUND`, measured as the
end-of-tick outcome pair). When #808 landed, this task's content reached
main through that stack: the vocabulary, the dispositions, the suite, the
harness and this document all landed there, reviewed in that task's rounds.

What the stacked landing did **not** carry were this review's later
corrections, because #808's branch point predates them. The round-3 rebase
of `rally/801-…` onto the post-#808 main (`756b862e`) therefore reduced to
exactly the residual delta — a literal fifteen-commit rebase would have
re-fought every deliberately-diverged hunk, so the branch was rebuilt as
`origin/main` plus these fixes, each re-verified against the merged tree:

- `m02_b_fu2.rs`: the hand-rolled `CS_ENGINE_IMAGE` read plus inline
  `sha256`/`ORIGINAL_IMAGE_SHA256` check is replaced by the canonical
  `load_engine_image()` reader (#798's convention), and its `IMAGE_BASE`
  comment's `.data` row is corrected to RVA `0x219000` (VA `0x619000`) as in
  the table above;
- `evidence.rs`: three stale claims corrected — the M02-B harness doc and
  `review.method` now say the record lowers completely since M02-B-FU1 and
  that the sound keys live in `CONTROL_RECORD_SOUND_KEY_VOCABULARY` (all
  seven, naming #801 and #808), and the FU2 harness's own `review.method`
  and doc comment stop claiming "still does not lower … stays Unsupported";
- `campaign_bindings.rs`: `unclassified_record_keys`' doc names both
  vocabularies;
- `main.rs`: the `accept_m02_b_` prefix covers four suites (`m02_b`,
  `m02_t3`, `m02_b_fu2`, `m02_b_fu3`), not three;
- this document: the provenance table's `.data` label, the image-test row's
  loader description, the unknowns' `OBJECTIVES_*_SOUND` bullet (admitted by
  #808) and the "still does not lower" claim, plus these review-round
  records.

Nothing else of this branch was missing from main: `mission_control.rs`'s
seven-key vocabulary, `m02_b.rs`'s membership assertions, the FU2 harness
and constants, and the `unclassified_record_keys` single-array artifact all
landed with the stack. The one verification repeated by hand this round:
each of the five keys' `record_sound_disposition` returns `Measured` with
its recorded consumer, field offset, parse site and consumer site on the
merged `mission_control.rs`.

### Checks run by this reviewer (merged head)

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m02_b_fu2_ --include-ignored` | 0 (3 tests) |
| `cargo test --workspace --locked -- accept_m02_b_ --include-ignored` | 0 (21 tests: M02-B, M02-T3, M02-B-FU1, M02-B-FU2, M02-B-FU3) |
| `cargo test -p cs_app --test campaign --locked -- accept_m03_b_ --include-ignored` | 0 (8 tests) |
| `cargo test --workspace --locked -- accept_m04_b_ --include-ignored` | 0 (8 tests) |
| `cargo test --workspace --locked -- accept_m01_lc_ --include-ignored` | 0 (203 tests) |
| `python3 tools/validate_evidence.py … --require-pass` | 0 |

The evidence copy was regenerated on this round's reviewed head with the
same reviewer identity extended to name this round; `review.identity` in the
committed JSON records the full chain (implementer, round 1, the #808
stack's re-runs, rounds 2–3).

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`SourceContext::read`, `SourceContext::control_program`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`;
`$CS_ENGINE_IMAGE` read-only (section table, string and 32-bit-immediate
searches, `r2` disassembly of `0x466b70`, `0x463c30`, `0x46a490`,
`0x46a95b`…`0x46a9b5`, `0x46caf0`, `0x46cc50`, `0x46cc70`, `0x596120`,
`0x57a090`); `missions/M02.md`; `docs/contracts/SCRIPT-MISSION.md`;
`docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-08-m02-b-compatibility-gaps.md`;
`docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`;
`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`;
`docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md`;
`docs/findings/2026-10-07-f37-d-fu2-mission-terminal-precedence-and-tick-ordering.md`;
`crates/cs_app/tests/campaign/m02_b.rs`; `crates/cs_content/src/mission_control.rs`.
