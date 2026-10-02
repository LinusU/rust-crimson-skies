# #464: what does the original pay for a stunt, and does a repeat traversal pay again?

Date: 2026-10-02. Task: #464 "Measure the original one-time vs repeatable stunt
reward rules and payout amounts". Feature sheet:
`specs/F42-stunts-fame-photos-and-optional-achievement-events.md`, stage
`### F42-D` (the retail audit stage). Shared contract:
`docs/contracts/CLI-EVIDENCE.md` (evidence) and
`docs/contracts/STATE-TRANSACTIONS.md` (the boundary this feeds).
Predecessors: `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`
(#463 measured *where* a stunt is spelled and named #464 as the task that answers
the payout/repeat half) and `docs/findings/2026-10-02-t465-ai-stunt-earning.md`
(#465 measured *who* may earn a completion and named the same unresolved payout
and repeat policy).

Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and ordinary
build/test. `gpu` and `audio` were available and **not used**: nothing is
rendered and nothing is played.

**Nothing here is `verified_original`.** No original run happened. `retail` here
is read access to files; the objective, scenario and reader bytes are the only
evidence, and what the 2000 PC original *did* with them — what it credited and
whether it paid twice — is not established. Format and mission-semantics work
should be reviewed by a different agent instance with a fresh context, and no
agent review replaces the owner's approval.

Installation fingerprint:
`b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` (the F02-B
`install::fingerprint` over the whole manifest).

## The question, stated so it can be answered or not

"One-time vs repeatable reward rules and payout amounts" is a question about the
original's **runtime**, and files are data, not behaviour. So the task has two
halves and only one is answerable from files:

1. **Does any file record what a stunt pays, or whether a completion may
   repeat?** That is measurable: it is a statement about the content's key
   vocabulary and about the one numeric score table the installation carries.
2. **If no file records it, what *is* the surface the rule would have to appear
   on?** Also measurable, and it decides what a reimplementation may safely
   assume and what it may not.

Both halves are measured below, and the answer is deliberately narrow: no
original data file this task read records a payout or a repeat policy for a
stunt. That is a statement about the data, not about the original's runtime.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/stunts.rs` (extend, an F42 owner path): the reward
  surface half — `vocabulary_keys`, `ScoreEntry`, `score_entries`,
  `RetailScoreTable`, `RewardRow`, `RetailStuntRewardSurvey`, the
  `REWARD_KEY_VOCABULARY` / `REPEAT_KEY_VOCABULARY` search vocabularies, and the
  key inventory now carried by `StuntCompletionCondition`.
- `crates/cs_app/src/stunts.rs` (extend, an F42 owner path): the survey
  `survey_retail_stunt_reward` and its named refusals
  (`StuntRewardSurveyError`).
- `crates/cs_app/tests/accept_f42_d_reward.rs` (new, F42 test path): the
  unignored authored-installation tests and the one `#[ignore]`d retail test.
- `crates/cs_app/tests/evidence_report_t464.rs` (new, F42 test path): the
  evidence harness; it is not part of the acceptance suite.
- `docs/findings/2026-10-02-t464-stunt-reward-and-repeat.md` (this file) and
  `docs/findings/evidence/T464.json`.

**One observable failure:** a survey that answered `reward_is_measured() ==
false` and nothing else would be indistinguishable from a reader that never
looked, and a finding that concluded "the original paid nothing / never paid
twice" from an absence of keys would be claiming a runtime rule from data that
carries no rule at all. Both defects produce the same plausible, wrong prose.
That is why the survey has to **report what it found**: the complete objective
key inventory, every stunt completion block's complete key surface, the
payout/repeat scans *derived from that inventory* (so an authored payout key
would be found), and the one numeric score table — so that "no payout is
recorded" is a measurement with numbers behind it. The unignored tests author a
`payout` key, a `repeatable` key and a `score_*` table, and fail if any of the
scans is hard-wired to "none".

## What was measured

Every number below comes from the owner's installation through read-only access
and is reproducible from this branch over `$CS_GAME_DIR`. The corpus is **every
reader archive in the installation**: **62** `zrdr.zbd` containers, of which
**53** carry objective records and **53** carry an objective state machine.

### 1. The objective records carry no payout and no repeat key: six keys in all

Across **332** objective records in the 53 `targets.zrd` members the complete
key vocabulary is **six** keys:

| key | occurrences |
| --- | ---: |
| `description` | 331 |
| `nodes` | 331 |
| `help_label` | 294 |
| `category_label` | 146 |
| `objective` | 93 |
| `other_target` | 52 |

None of them names a payout, a score, a reward or a repeat policy. (`objective`
and `other_target` are the marker keys #465 measured; they carry no value.)

### 2. The objective state machine: 1 338 blocks, 55 keys, none a payout or repeat

Over **1 338** numbered `OBJECTIVE<N>` blocks the union of keys inside a block is
**55** keys. None equals any entry of the declared reward or repeat vocabulary.
The union over both surfaces — the six record keys and the 55 block keys — is
**61** measured keys (`objective_keys()`), and the payout scan
(`reward_keys()`) and repeat scan (`repeat_keys()`) derived from it are both
**empty**. The scan is exact-match on purpose: a substring scan would read
`DANGER_ZONES_COMPLETION_COUNT` as a repeat count, which is exactly the false
positive a payout census must not have.

### 3. The 31 stunt completion blocks: a closed, 11-key surface that pays nothing

The original's own stunt completion condition is **`DANGER_ZONES_COMPLETED`**,
carried by **31** blocks in **7** readers
(`C2/M02`, `C2/M03`, `C3/M01`, `C4/M02`, `C5/M01`, `C5/M02`, `C5/M04`). Its
value is a list of `dzpath<N>` world-zone names: 27 blocks name one zone and four
name 2–6. Six blocks also carry `DANGER_ZONES_COMPLETION_COUNT`
(four times `1`, once `3`, once `4`) — the number of listed zones that completes
the objective, **not** a number of times it may pay.

The complete key surface of those 31 blocks — the closed set a payout would have
to appear on — is exactly **11** keys:

| key | blocks |
| --- | ---: |
| `DANGER_ZONES_COMPLETED` | 31 |
| `COMPLETED_SOUND_GROUP` | 22 |
| `WAKE_OBJECTIVE_WHEN_I_COMPLETE` | 22 |
| `KILL_OBJECTIVE_WHEN_I_COMPLETE` | 15 |
| `BEGIN_DORMANT` | 14 |
| `NAP_OBJECTIVE_WHEN_I_COMPLETE` | 14 |
| `REMOVE_OBJECTIVE_TARGET` | 13 |
| `ADD_OBJECTIVE_TARGET` | 7 |
| `DANGER_ZONES_COMPLETION_COUNT` | 6 |
| `IDENTITY` | 5 |
| `SET_HELP_LABEL` | 1 |

Not one is a payout and not one is a repeat policy. A block that paid a reward
or stated whether it repeats would have to carry that key here, and does not.

### 4. The installation's only numeric score table is a five-key multiplayer match scoreboard

Exactly **one** reader archive carries a `player.zrd` score table: the
installation's global `zbd/zrdr.zbd`. Its complete `score_*` surface is **five**
keys, and nothing else in the table names a stunt, a danger zone or a photo:

| key | stored word | signed |
| --- | ---: | ---: |
| `score_kill` | 2 | 2 |
| `score_return_flag` | 8 | 8 |
| `score_zep` | 10 | 10 |
| `score_enemy_flag` | 10 | 10 |
| `score_suicide` | 4294967294 | −2 |

The one negative-looking entry is the suicide penalty; the raw word is stored
unsigned, so `ScoreEntry::signed()` is a documented reinterpretation, not a
decoded schema. This is a capture-the-flag match scoreboard: `return_flag` and
`enemy_flag` are its vocabulary. No single-player stunt, fame or cash entry
exists in it.

### 5. Cross-check: no reader member anywhere spells a reward or repeat setting

As a bounded supporting probe, a raw text-token pass over **every `.zrd` member
of all 62 reader archives** (1 293 members, all decoded to their end, zero
refusals) matched the reward/repeat words `fame, cash, money, reward, bonus,
payout, prize, medal, achievement, unlock, repeat, repeatable, one_time,
recurring, respawn` as whole tokens or affixes. The only hits are **two
`respawn`-prefixed settings in the global `player.zrd`** — `respawn_rad` and
`respawn_el`, spawn geometry, not a repeat rule. This probe is not part of the
production survey (which audits the objective data and the score table); it is
recorded because it extends the "no file records a payout" measurement past the
objective members and past the members this task decodes, and because it names
the only near-miss.

## The answer, in one paragraph

What the original paid for a stunt, and whether a second traversal paid again,
is **not recorded in any of the 1 293 `.zrd` members of the 62 reader archives
this task read**. The objective records and the
objective state machine carry no payout and no repeat key; the 31 blocks that
carry the original's own stunt completion condition have a closed 11-key surface
with neither; the installation's only numeric score table is a five-key
multiplayer match scoreboard with no stunt, fame or cash entry; and a raw text
pass over every reader member finds no reward or repeat spelling outside two
spawn-geometry settings. The original's data model *can* express a count on a
stunt block (`DANGER_ZONES_COMPLETION_COUNT`), so "there is no count anywhere" is
not the claim — the claim is that the only count is how many zones complete the
objective. What a completion credited is runtime behaviour, and the only thing
that can settle it is an original run. Until then the reimplementation must not
invent a payout or a one-time/repeatable rule and present it as the original's.

## What is **not** measured, and is therefore not a field

- **What a stunt paid** — fame, cash, a medal, an unlock: none is recorded in any
  file this task read, and the survey reports `reward_is_measured() == false`;
- **whether a repeat traversal paid again** — no file states a repeat policy, and
  the survey reports `repeat_is_measured() == false`;
- **the zone-crossing predicate** — whether a crossing is detected at all is
  runtime geometry (not in these files);
- **the direction and clearance rules** of a traversal (the F42 rules / F42-A);
- **who a crossing is credited to** — the earning authority, measured by #465 and
  still unrecorded;
- **whether any single-player stunt ever touched the `score_*` table** — the table
  is a multiplayer match scoreboard; whether single-player matches used it is not
  recorded;
- **fame counters and linked scrapbook media** (F43 / F47): outside this task.

## Evidence

Ordinary build/test plus read-only `retail` access. The acceptance run's log and
a second production observation (the reward census over every reader archive)
are recorded under `private/evidence/T464/`; the report is committed as
`docs/findings/evidence/T464.json` and checked with
`tools/validate_evidence.py --require-pass`. The claim is **`implemented`**: the
reward/repeat surface is measured, and the rule itself is recorded as
unmeasured.

## Known limitations that gate later stages (not silently dropped)

- **The original's payout for a stunt is unmeasured.** Affected content: every
  stunt in the installation — the instant-action fly-through targets (#463) and
  the 31 campaign danger-zone conditions measured here. Resolving task: an
  original run (REF capture), which only the owner can supply. Until then F42-C
  may not apply a fame/cash amount or a repeat rule as if it were the original's.
- **The original's one-time vs repeatable rule is unmeasured**, likewise. The
  reimplementation may choose a designed rule, but it must be labelled designed,
  and this finding is the evidence that no file supplied it.
- **No original run happened.** Nothing here is evidence of the original's
  runtime behaviour: not that a player crossing fires a stunt, not that a
  completion paid anything, not that a second pass did or did not pay again.
- **The `player.zrd` name is duplicated** in the global reader (the score table
  and an unrelated `ANIMATION_DEFINITIONS` record). The score measurement is the
  declaration-order match the production reader selects; the duplicate is
  recorded here so a future reader change cannot silently swap them.

## What was built

* `crates/cs_content/src/stunts.rs` — the content half:
  - `StuntCompletionCondition` now carries the **complete** key inventory of the
    block it lives in, with `keys()`, `reward_keys()` and `repeat_keys()`;
  - `REWARD_KEY_VOCABULARY` (16 keys) and `REPEAT_KEY_VOCABULARY` (10 keys), the
    **declared search vocabularies** (not claims about the original's spelling),
    and `vocabulary_keys`, the exact-match scan over a complete key inventory;
  - `ScoreEntry` with its raw and signed readings, `score_entries` (the
    `score_*` reader, which unwraps the measured one-element member wrapper) and
    `RetailScoreTable`;
  - `RetailRewardRow` and `RetailStuntRewardSurvey` with `objective_records()`,
    `objective_blocks()`, `objective_keys()`, `reward_keys()`, `repeat_keys()`,
    `stunt_conditions()`, `stunt_block_keys()`, `score_tables()`,
    `score_entries()`, `reward_is_measured()` and `repeat_is_measured()`.
* `crates/cs_app/src/stunts.rs` — the boundary: `survey_retail_stunt_reward`
  (one production discovery, every `*/zrdr.zbd` in the inventory,
  `discover_container` per archive, the `.zrd` decoder, provenance spans per
  member) and `StuntRewardSurveyError` with named refusals (`Discovery`,
  `NoObjectiveReaders`, `Read`, `Decode { container, member, code, offset }`).
* `crates/cs_app/tests/accept_f42_d_reward.rs` — four unignored tests and one
  `#[ignore]`d retail test, all on the production path (authored `.zrd` documents
  and reader archives through `cs_app::stunts::survey_retail_stunt_reward`).
* `crates/cs_app/tests/evidence_report_t464.rs` — the evidence harness, with a
  **second production observation**: the survey re-run over the installation and
  rendered as `reward-census.json` (every row with its key inventory, its stunt
  conditions and its complete score table, plus the derived scans).

## Test sensitivity (each mutation was applied, run and reverted)

| Removed behaviour | Tests that failed |
| --- | --- |
| the payout/repeat scans hard-wired to report none | `..._the_vocabulary_scan_finds_an_authored_payout_and_repeat_key`, `..._the_survey_measures_the_reward_surface_of_every_reader` |
| the `score_*` reader's origin filter (every key read as a score) | `..._the_score_table_reads_only_score_keys_and_both_signs`, `..._the_survey_measures_the_reward_surface_of_every_reader` |
| the stunt-block key inventory (a block's `keys()` always empty) | `..._the_vocabulary_scan_finds_an_authored_payout_and_repeat_key`, `..._the_survey_measures_the_reward_surface_of_every_reader`, the retail test |
| the member-wrapper unwrap in `score_entries` (flat-only reading) | `..._the_score_table_reads_only_score_keys_and_both_signs`, `..._the_survey_measures_the_reward_surface_of_every_reader`, the retail test |
| the `NoObjectiveReaders` refusal | `..._every_reward_refusal_names_the_container_and_member` |

Each row was applied to the production code, run, and reverted; the named tests
failed and the rest passed in every case.

The fourth row is also the defect the retail run caught during development: the
real `player.zrd` wraps its record in the same one-element list as
`objectives.zrd`, and the first implementation read the member flat-only and
measured an empty table. That is what the retail run exists to catch, and the
authored fixtures were corrected to the measured wrapped shape at the same time
so the unignored tests exercise the same path.

## Checks run

* `cargo fmt --all -- --check` = 0,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  = 0, `cargo test --workspace --locked` = 0, and
  `cargo test --workspace --locked -- accept_f42_d_reward_ --include-ignored` = 0
  (5 discovered, 5 passed: 4 unignored for CI and 1
  `#[ignore] = "requires CS_GAME_DIR"]` run locally over `$CS_GAME_DIR`).
* The evidence report is committed as `docs/findings/evidence/T464.json` and
  validates with
  `python3 tools/validate_evidence.py private/evidence/T464/acceptance.json
  --artifact-root private/evidence/T464 --require-pass`.

## Sources used

- `specs/F42-stunts-fame-photos-and-optional-achievement-events.md` (F42-D) and
  `docs/findings/2026-10-01-f42-a-traversal-predicates-and-reward-identity.md`
  (the reward/repeat unknown) and
  `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md` and
  `docs/findings/2026-10-02-t465-ai-stunt-earning.md` (the encoding and authority
  this survey joins to).
- `crates/cs_formats/src/zbd/{trailer,reader_archive}.rs`,
  `crates/cs_formats/src/script_raw/discovery.rs` (the production reader-archive
  discovery) and `docs/findings/2026-10-02-f09-palette-original-faction-palettes.md`
  (the `.zrd` grammar).
- The owner's installation, read-only, over `$CS_GAME_DIR`: all 62 reader
  archives (`ZBD/**/zrdr.zbd`) and their `targets.zrd`, `objectives.zrd` and
  `player.zrd` members, plus the raw text pass over all 1 293 `.zrd` members.
  Installation fingerprint above.

**No original data is committed.** The numbers here are counts, offsets, spans
and digests; no extracted `.zrd`, no string table, no mesh and no screenshot is
in the repository, and every private output went to `private/`, outside it.
