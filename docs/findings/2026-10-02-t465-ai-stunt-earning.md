# #465: can an AI aircraft or another non-player authority earn an original stunt?

Date: 2026-10-02. Task: #465 "Measure whether an AI aircraft or a non-player
authority can earn an original stunt". Feature sheet:
`specs/F42-stunts-fame-photos-and-optional-achievement-events.md`, stage
`### F42-D` (the retail audit stage; task #463 measured the encoding half of
the same sheet and named #465 as the task that answers this half). Shared
contract: `docs/contracts/CLI-EVIDENCE.md` (evidence) and
`docs/contracts/STATE-TRANSACTIONS.md` (the boundary this feeds). Predecessor:
`docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`; the
unknown this task resolves is recorded there ("the traversal and payout rules
are unmeasured … #464 (repeat/payout), #465 (AI earning)") and in
`docs/findings/2026-10-01-f42-a-traversal-predicates-and-reward-identity.md`
("Whether an AI aircraft can earn a stunt is unmeasured").

Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and
ordinary build/test. `gpu` and `audio` were available and **not used**: nothing
is rendered and nothing is played.

**Nothing here is `verified_original`.** No original run happened. `retail` here
is read access to files; the scenario, objective and world bytes are the only
evidence, and what the 2000 PC original *did* with them is not established. A
different agent instance with a fresh context should review this format work,
and no agent review replaces the owner's approval.

Installation fingerprint:
`b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` (the F02-B
`install::fingerprint` over the whole manifest).

## The question, stated so it can be answered or not

"Can an AI aircraft or another non-player authority earn an original stunt?" is
a question about the original's **runtime**, and the installation's files are
data, not behaviour. So the task has two halves and only one of them can be
answered from files:

1. **Does any file in the original record an *earning authority* for a stunt?**
   That is measurable: it is a statement about the objective encoding.
2. **If no file records it, what *is* in the data about non-player aircraft and
   about non-player subjects?** Also measurable, and it decides what a
   reimplementation may safely assume and what it may not.

Both halves are measured below, and the answer is deliberately narrow: the
original's data expresses *no* actor for a stunt (danger zone) completion, and
the *one* actor-scoped completion condition it does have (`TRAVELERS`) does name
non-player actors — but never for a danger zone. Whether an AI crossing can
therefore earn a stunt is **runtime behaviour no file records**, and it needs an
original run.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/stunts.rs` (extend, an F42 owner path): the earning-authority
  half — `zrd_flat_fields`, `objective_record_keys`, `scenario_non_player_aircraft`,
  `objective_state_machine`, and the records `ScenarioEnemyGroup`,
  `ScenarioAce`, `ScenarioNonPlayerAircraft`, `StuntCompletionCondition`,
  `TravellerSubject`, `TravellerCondition`, `ObjectiveStateMachine`,
  `RetailObjectiveAuthorityRow`, `RetailStuntAuthoritySurvey`.
- `crates/cs_app/src/stunts.rs` (extend, an F42 owner path): the survey
  `survey_retail_stunt_authority` and its named refusals
  (`StuntAuthoritySurveyError`).
- `crates/cs_app/tests/accept_f42_d_ai_authority.rs` (new, F42 test path): the
  unignored authored-installation tests and the one `#[ignore]`d retail test.
- `crates/cs_app/tests/evidence_report_t465.rs` (new, F42 test path): the
  evidence harness; it is not part of the acceptance suite.
- `docs/findings/2026-10-02-t465-ai-stunt-earning.md` (this file) and
  `docs/findings/evidence/T465.json`.

**One observable failure:** a survey that reported the answer as a constant
(`earning_authority_is_measured() == false` and nothing else) would be
indistinguishable from a reader that never looked, and an audit that concluded
"the original never lets an AI earn a stunt" from a *key inventory* would be
claiming a runtime rule from data that carries no rule at all. Both defects
produce the same plausible, wrong prose. That is why the survey has to **report
what it found**: the complete objective-key inventory, the `TRAVELERS` subject
census with its non-player subjects, the danger-zone conditions and the
scenario's declared aircraft — so that "no authority is recorded" is a
*measurement with numbers behind it* and so that a reader which mistook the
actor vocabulary for a rule (or missed a non-player subject) fails a test.

## What was measured

Every number below comes from the owner's installation through read-only access,
and every one is reproducible from the code in this branch over
`$CS_GAME_DIR`. The corpus is **every reader archive in the installation**: 62
`zrdr.zbd` containers, of which 53 carry objective records, 53 carry an
objective state machine, 40 carry numbered `OBJECTIVE<N>` blocks, 8 carry an
instant-action scenario descriptor and 23 carry a detection-zone member.

### 1. The objective records carry no actor: six keys, and none of them is one

Across **332** objective records in the 53 `targets.zrd` members the complete
key vocabulary is **six** keys:

| key | occurrences |
| --- | ---: |
| `description` | 331 |
| `nodes` | 331 |
| `category_label` | 146 |
| `help_label` | 294 |
| `objective` | 93 |
| `other_target` | 52 |

`objective` and `other_target` are **marker** keys — a one-element list with no
value. There is no key naming a subject, an owner, a team, a squadron, an
aircraft or a pilot. **67** of the records are fly-through danger-zone targets by
either measured label and **64** by #463's stricter selector (the three-record
difference is corrected below), **46** are team-scoped (`MSG_OBJ_TEAM_1` /
`MSG_OBJ_TEAM_2`, all of them in the multiplayer readers). So the original's
objective *record* never says who may complete it, and where the original does
have an owning authority in this layer it is a **team**, never an aircraft.

### 2. The objective state machine: 1 338 numbered blocks, 55 keys, one of them names an actor

The `objectives.zrd` member (measured: a one-element list wrapping one flat
alternating record) carries `MISSION_TIMER`, `PLAYER_INIT` and
`OBJECTIVE<N>` blocks. Over **1 338** blocks in 40 readers the union of keys
inside a block is **55** keys. Three matter here:

* **`DANGER_ZONES_COMPLETED`** — 31 blocks. It is the original's own stunt
  completion condition: its value is a list of `dzpath<N>` world zone names,
  e.g. C2/M03's `OBJECTIVE17` is
  `['DANGER_ZONES_COMPLETED', ['dzpath1'], 'REMOVE_OBJECTIVE_TARGET', ['sghangar'], 'ADD_OBJECTIVE_TARGET', ['dz2'], …]`
  — the danger zone completes, the flown gate's target is removed, the next gate
  is added. **`DANGER_ZONES_COMPLETED` takes zone names and nothing else: no
  subject, no owner, no authority.** 6 of the 31 blocks add
  `DANGER_ZONES_COMPLETION_COUNT` (four times `1`, once `3`, once `4`).
* **`TRAVELERS`** — 75 blocks in 20 readers. This is the only actor-scoped
  completion condition in the whole objective machine, and its first field is the
  subject. Measured census: **`player` 43**, a **named non-player actor 26** (25
  distinct names: `secfury_5`, `secfury_6`, `geminizep`, `blackhatzep`,
  `dantezep`, `autogyro_1`, `stihellhound_5_eg0`, `devastator_1/2`,
  `wingman_2/3`, `bhatbrigand_5_1` … `bhatbrigand_5_14`), and an **indexed
  subject 6** (a bare integer). Its second field is the relation
  (`APPROACHING` 54, `LEAVING` 21).
* **`SET_AI_NET` / `SET_AI_TEAM` / `SET_AI_`** — 41 / 6 / 1 blocks. These *give*
  AI aircraft a net or a team; they never consume one. They are setup, not an
  earning rule.

So the original's data model can express "**a named non-player actor** did
something" — that is what the 26 named `TRAVELERS` subjects are — and the same
model expresses a stunt as "**a zone** was completed" with no actor at all.

### 3. No stunt condition ever names a non-player subject

Of the 75 `TRAVELERS` conditions, exactly **3** name a danger-zone label as their
target, and **all three name `player`** as the subject:

| reader | objective | condition |
| --- | --- | --- |
| `ZBD/C5/M01` | `OBJECTIVE2` | `['player', 'APPROACHING', 'dz1', 1000.0, 1]` |
| `ZBD/C5/M02` | `OBJECTIVE15` | `['player', 'APPROACHING', 'dz1', 4000.0, 1]` |
| `ZBD/C5/M02` | `OBJECTIVE20` | `['player', 'APPROACHING', 'dz1', 500.0, 1]` |

Combined with §2's `DANGER_ZONES_COMPLETED` (zone names only), the measured
statement is: **in every measured record and condition in the installation that
concerns a danger zone, the actor is `player` or absent.** No measured record
lets an AI aircraft be the subject of a danger-zone condition.

### 4. Every stunt scenario also declares non-player aircraft

This is why the question matters for a reimplementation. The instant-action
scenario descriptor (`ia.zrd`) that carries `mission_type` and the `dzones`
label→node bindings also declares, at its root:

* `player_plane` — the player's aircraft (7 of the 8 scenarios; `c2b` names
  none);
* `num_wingmen` — **3** in the same 7 (`c2b` names none);
* `group1` … `group4` — four enemy groups, each with `num_enemies`,
  `enemy_name`, `enemy_plane`, `enemy_skill` (`novice` / `veteran` / `ace`),
  present in **all eight** scenarios, summing 12–18 aircraft per scenario;
* `ace_name` / `ace_plane` / `ace_skill` — a named ace, in all eight.

The four scenarios the original marks `stunt_flying` — `c1b`, `c2`, `c4`, `c5`,
which between them declare all **45** of #463's stunt-flying fly-through targets —
each place **3 wingmen and 12–18 enemies plus an ace in the same record that
declares their stunt gates**:

| world | `mission_type` | player plane | wingmen | enemy groups | ace plane | summed enemies |
| --- | --- | --- | ---: | --- | --- | ---: |
| `c1b` | `stunt_flying` | Firebrand | 3 | 4/3/3/2 | Fury | 12 |
| `c2` | `stunt_flying` | Brigand | 3 | 4/4/3/2 | Bloodhawk | 13 |
| `c4` | `stunt_flying` | Peacemaker | 3 | 6/5/4/3 | Bloodhawk | 18 |
| `c5` | `stunt_flying` | Kestrel | 3 | 6/5/4/3 | Peacemaker | 18 |

AI aircraft are therefore part of the measured content of every stunt scenario.
A reimplementation that credits *any* aircraft crossing a stunt zone would credit
AI passages — which is what F42-A's `StuntAuthority::AiFlight` refusal
(`cs_sim::stunts`) prevents. The data **justifies keeping that refusal** as a
safe default; it does **not** establish it as the original's rule.

## The answer, in one paragraph

Whether an AI aircraft or another non-player authority can earn an original
stunt is **not recorded anywhere in the installation**: no objective record and
no objective-machine condition that concerns a danger zone names any actor other
than `player`, and the stunt condition itself (`DANGER_ZONES_COMPLETED`) names
only zones. The original's objective model *can* name a non-player actor as the
subject of a completion condition (26 measured `TRAVELERS` subjects), so "only
the player ever completes anything" is **not** a safe inference either. The
earning authority of a stunt is a property of the original's *runtime*, and the
only thing that can settle it is an original run. Until then the measured
content says: stunt scenarios contain AI aircraft, every measured stunt
condition is player-named or anonymous, and the reimplementation's AI refusal is
a **designed** choice with a measured hazard behind it — never a measured
original rule.

## A correction this task found in #463's fly-through selector

`cs_content::stunts::scenario_fly_through_targets` selects a fly-through target
by reading **both** halves of the measured label pair and requiring both to be
present (its `?` on `category_label` refuses a record that carries only the help
label). Over the **eight instant-action scenarios** every fly-through record
carries both labels, so the selector and the looser union read the same **54**
rows — which is why #463's record could not see the difference. Over the **whole
installation** they disagree by exactly **three** records, all of them campaign
missions, all of them carrying `help_label = MSG_OBJ_FLYTHROUGH` and **no**
`category_label` at all:

| reader | objective | node |
| --- | --- | --- |
| `ZBD/C1/M02` | `MSG_OBJ_ZEPHANGER` | `h3_marker` |
| `ZBD/C4/M03` | `MSG_TRGT_DEVILSHORN` | `dz2` |
| `ZBD/C5/M02` | `MSG_TRGT_PHQ` | `dz1` |

So the measured counts are **67** fly-through records by either label and **64**
by #463's stricter selector. Both numbers are in this branch's survey
(`fly_through_labelled_objectives()` and `fly_through_objectives()`), the
retail test pins both and names the three readers, and **#463's selector is left
unchanged**: which reading the reimplementation should use is a fidelity
decision over content this task did not audit, and the risk is now recorded here
and as a follow-up task rather than silently resolved in another task's
measurement.

## What is **not** measured, and is therefore not a field

- **The zone-crossing predicate itself** — whether a crossing is detected at all
  is not in any of these files (it is runtime geometry);
- **the direction and clearance rules** of a traversal (the F42-A rules; task
  #464 and the F42 rule work);
- **the payout, the repeat policy and the linked photo** (task #464, F47);
- **which aircraft the original credits for a zone crossing** — the open
  question this task bounds;
- **`TRAVELERS`' semantics beyond its shape** — that the first field is the
  subject and the second the relation is *measured as a field order* over 75
  blocks; that `APPROACHING` means "approaches" is not established here and is
  not claimed (an integer subject is not even decoded: whether it indexes the
  scenario's actor name table is a correlation, not a decode);
- **`aiv.zrd`'s aircraft table and `egen.zrd`'s nets** — read while searching for
  an authority, and deliberately **not** decoded: their record layouts have ~90
  opaque fields each, so any statement about them would be a guess.

## Evidence

Ordinary build/test plus read-only `retail` access. Commands run locally over
the recorded candidate tree are listed in the "Checks run" section appended when
the slice is done; the acceptance run's log and a second production observation
(the authority census over every reader archive) are recorded under
`private/evidence/T465/`, the report is committed as
`docs/findings/evidence/T465.json` and checked with
`tools/validate_evidence.py --require-pass`. The claim is **`implemented`**: the
earning-authority surface is measured, and the rule itself is recorded as
unmeasured.

## Known limitations that gate later stages (not silently dropped)

- **The original's earning authority for a stunt is unmeasured.** Affected
  content: every stunt in the installation — the 54 instant-action fly-through
  targets (#463) and the campaign danger-zone conditions measured here.
  Resolving task: an original run (REF capture), which only the owner can
  supply; until then `cs_sim::stunts::StuntAuthority::AiFlight` stays a designed
  refusal and F42-C may not treat it as an original rule.
- **No original run happened.** Nothing here is evidence of the original's
  runtime behaviour: not that `DANGER_ZONES_COMPLETED` fires on a player
  crossing, not that `TRAVELERS`' first field is a subject, not that an AI
  crossing cannot complete a zone. A capture from an actual original run is the
  only thing that can settle those, and it needs the owner.
- **`TRAVELERS`' integer subject is not decoded** — the correlation that it
  indexes an actor name table (the tables were read) is recorded, not asserted.
- **The campaign `dzones.zrd` semantics remain unmeasured** (`objective_numbers`
  joins, `disable`, `nosnapshot`), as `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`
  left them; task #513 owns that member's framing.
- **The fly-through selector's label rule is undecided** (three campaign records
  carry the help label and no category label). Affected content: those three
  records and every downstream stunt count built on the stricter selector.
  Resolving task: #533 (`T465-flythrough-selector`), filed by this task.

## What was built

* `crates/cs_content/src/stunts.rs` — the content half:
  - `zrd_flat_fields`, the **flat-only** record reader (with the reason a
    shape-agnostic walk would invent vocabulary out of values like
    `INACTIVE1 ["fuel_truck01", "tank"]`);
  - `objective_record_keys` / `objective_record_count` / `team_scoped_objectives` /
    `is_fly_through_labelled` / `fly_through_labelled_objectives` — the objective
    record census, including the looser fly-through reading that keeps #463's
    three dropped records visible;
  - `objective_record` (the measured one-element wrapper) and
    `objective_state_machine`, with `ObjectiveStateMachine`,
    `StuntCompletionCondition` (zones and a required count, **no subject**),
    `TravellerSubject` (`Player` / `Named` / `Indexed` / `Unreadable`) and
    `TravellerCondition`;
  - `scenario_non_player_aircraft` with `ScenarioEnemyGroup`, `ScenarioAce` and
    `ScenarioNonPlayerAircraft`;
  - `RetailObjectiveAuthorityRow`, `RetailObjectiveCorpus`,
    `RetailObjectiveMachine`, `RetailScenarioAuthority`, `TravellerSubjectCensus`
    and `RetailStuntAuthoritySurvey` — with `objective_keys()` (the complete
    vocabulary over both surfaces), `keys_naming_an_authority()` (derived from
    that vocabulary against the declared `AUTHORITY_KEY_VOCABULARY`),
    `stunt_conditions()`, `traveller_subject_census()`,
    `non_player_subjects()`, `traveller_conditions_naming_a_danger_zone()`,
    `non_player_danger_zone_conditions()`, `scenarios_declaring_non_player_aircraft()`,
    `stunt_flying_scenarios()` and `earning_authority_is_measured()`.
* `crates/cs_app/src/stunts.rs` — the boundary: `survey_retail_stunt_authority`
  (one production discovery, every `*/zrdr.zbd` in the inventory,
  `discover_container` per archive, the `.zrd` decoder, provenance spans per
  member) and `StuntAuthoritySurveyError` with named refusals
  (`Discovery`, `NoWorldGroups`, `NoScenarioReaders`, `Read`,
  `Decode { container, member, code, offset }`).
* `crates/cs_app/tests/accept_f42_d_ai_authority.rs` — six unignored tests and
  one `#[ignore]`d retail test, all on the production path (authored `.zrd`
  documents and reader archives through
  `cs_app::stunts::survey_retail_stunt_authority`).
* `crates/cs_app/tests/evidence_report_t465.rs` — the evidence harness, with a
  **second production observation**: the survey re-run over the installation and
  rendered as `authority-census.json` (every row with its key inventory, its
  stunt conditions, its traveller conditions and its declared aircraft).

## Test sensitivity (each mutation was applied, run and reverted)

| Removed behaviour | Tests that failed |
| --- | --- |
| the authority scan (`keys_naming_an_authority` hard-wired to report none) | `..._the_survey_measures_the_authority_surface_of_every_reader` |
| the subject discrimination (every `TRAVELERS` subject read as `player`) | `..._the_state_machine_reads_stunt_conditions_and_traveller_subjects`, `..._the_survey_measures_the_authority_surface_of_every_reader` |
| the danger-zone label test (`target_is_danger_zone_label` always false) | the same two |
| `ScenarioNonPlayerAircraft::is_declared` (always false) | `..._the_scenario_reads_its_declared_non_player_aircraft`, `..._the_survey_measures_the_authority_surface_of_every_reader` |
| the flat-only rule (`zrd_flat_fields` reading a pair-shaped value as a key) | `..._the_flat_reader_reads_pairs_and_never_invents_a_key` |

The first row is the reason the authored installation contains a `player_only`
objective key: a scan that always answered "no authority is recorded" would pass
every measured assertion over the retail corpus and still be worthless.

## Defects the review found and fixed

The review of this task was performed by the same agent identity that wrote it
(`bunny-2/bunny-2`, in a later session with a **fresh context**, but the **same
model**). Per `AGENTS.md` that is **not independent evidence**, and no agent
review replaces the owner's approval; the numbers below were additionally
re-measured from scratch by a separate agent instance parsing the installation
without reading this branch's Rust. Three real defects were found and fixed:

1. **The committed evidence artifact was not valid JSON.** The harness rendered
   every list with a bare `.join(", ")`, so an empty list wrote `"key": ,` and a
   non-empty one wrote `"key": {..}, {..}` — both malformed. The census is
   committed as evidence, `tools/validate_evidence.py` only checks its digest,
   and nothing else parsed it, so the defect was invisible to every check in
   this repository. All list rendering now goes through helpers that own their
   brackets, the harness asserts the document's balance and emptiness grammar
   before the write is trusted, and the harness additionally refuses a list of
   the wrong element shape (a double-wrapped array is *valid* JSON and would
   still slip past a grammar check).
2. **Two census fields were hard-wired to `[]`.**
   `keys_naming_an_authority` and `non_player_danger_zone_conditions` were
   written as literals rather than measured — exactly the "a scan that always
   answers *none*" failure this task's own one-observable-failure note is about,
   committed as if it were a measurement. Both are now computed from the survey.
   Both still measure empty over the owner's installation, and that is now a
   result rather than a constant.
3. **This document claimed 61 reader archives** where the corpus is 62 (and
   where the acceptance test, the survey and the installation itself all say
   62). Corrected in both places. Every other number in this document was
   re-checked against the regenerated census and is unchanged.

## Checks run

* `cargo fmt --all -- --check` = 0,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  = 0, `cargo test --workspace --locked` = 0, and
  `cargo test --workspace --locked -- accept_f42_d_ai_ --include-ignored` = 0
  (7 discovered, 7 passed: 6 unignored for CI and 1
  `#[ignore] = "requires CS_GAME_DIR"]` run locally over `$CS_GAME_DIR`).
* The evidence report is committed as `docs/findings/evidence/T465.json` and
  validates with
  `python3 tools/validate_evidence.py private/evidence/T465/acceptance.json
  --artifact-root private/evidence/T465 --require-pass`.

## Sources used

- `specs/F42-stunts-fame-photos-and-optional-achievement-events.md` (F42-D) and
  `docs/findings/2026-10-01-f42-a-traversal-predicates-and-reward-identity.md`
  (the F42-A unknown this task answers) and
  `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md` (the
  encoding this survey joins to).
- `crates/cs_formats/src/zbd/{trailer,reader_archive}.rs`,
  `crates/cs_formats/src/script_raw/discovery.rs` (the production reader-archive
  discovery) and `docs/findings/2026-10-02-f09-palette-original-faction-palettes.md`
  (the `.zrd` grammar).
- The owner's installation, read-only, over `$CS_GAME_DIR`: all 62 reader
  archives (`ZBD/**/zrdr.zbd`) and their `targets.zrd`, `objectives.zrd`,
  `dzones.zrd` and `ia.zrd` members. Installation fingerprint above.

**No original data is committed.** The numbers here are counts, offsets, spans
and digests; no extracted `.zrd`, no string table, no mesh and no screenshot is
in the repository, and every private output went to `private/`, outside it.

## Update (task #533, 2026-10-10)

Task #533 resolved the "Known limitations" bullet this document filed ("The
fly-through selector's label rule is undecided … Resolving task: #533"): the
reimplementation now uses the **union** of the two measured labels — a record is
a fly-through target when its `category_label` is `MSG_OBJ_DZ` **or** its
`help_label` is `MSG_OBJ_FLYTHROUGH`. What that changes and confirms in this
document, with the affected records named:

* **§1's sentence is corrected in meaning**: "**67** of the records are
  fly-through danger-zone targets by either measured label and **64** by #463's
  stricter selector" — 64 was the old selector's count and the strict rule no
  longer exists. The selector now counts **67**, equal to the label-only
  reading, because every labelled record names a node. The three records that
  made the difference are `ZBD/C1/M02`'s `MSG_OBJ_ZEPHANGER` at `h3_marker`,
  `ZBD/C4/M03`'s `MSG_TRGT_DEVILSHORN` at `dz2` and `ZBD/C5/M02`'s
  `MSG_TRGT_PHQ` at `dz1`; each is wired into its mission's objective machine
  (`ADD_OBJECTIVE_TARGET`/`REMOVE_OBJECTIVE_TARGET`, and `TRAVELERS`
  conditions on C5/M02's `dz1`) exactly like the both-label campaign records.
* **Every other measured number in this document is confirmed unchanged**: the
  62 reader archives, 332 objective records, the six-key vocabulary
  (146/294 category/help), 46 team-scoped records, 1 338 blocks, 31
  `DANGER_ZONES_COMPLETED` and 75 `TRAVELERS` conditions with their subject
  census, and all eight scenarios' declared aircraft — the label rule touches
  only the fly-through count, and only over campaign readers.

The decision, its evidence and the re-measured counts are in
`docs/findings/2026-10-10-t533-fly-through-label-rule.md`.