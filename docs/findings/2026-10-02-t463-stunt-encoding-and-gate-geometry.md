# #463: the original stunt encoding and gate geometry, measured

Date: 2026-10-02. Task: #463 "Measure the original stunt encoding and gate
geometry for F42-D". Feature sheet:
`specs/F42-stunts-fame-photos-and-optional-achievement-events.md`, stage
`### F42-D`. Shared contract: `docs/contracts/CLI-EVIDENCE.md` (evidence) and
`docs/contracts/STATE-TRANSACTIONS.md` (the boundary this encoding feeds).
Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and
ordinary build/test. `gpu` and `audio` were available and **not used**: nothing
is rendered and nothing is played.

**Nothing here is `verified_original`.** No original run happened. `retail` here
is read access to files; the scenario and world bytes are the only evidence, and
what the 2000 PC original *did* with them is not established. A different agent
instance with a fresh context should review this format work, and no agent
review replaces the owner's approval.

Installation fingerprint:
`b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` (the
F02-B `install::fingerprint` over the whole manifest, which the survey carries
as `install_sha256`).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/stunts.rs` (extend, an F42 owner path): the `.zrd`
  decoder (`decode_zrd`, `ZrdValue`, `ZrdDecodeError`), the field reader
  (`zrd_field`), the scenario extractors (`scenario_mission_type`,
  `scenario_zone_bindings`, `scenario_fly_through_targets`) and the records
  `ScenarioFlyThroughTarget`, `StuntEncodingSpan`, `RetailStuntGate`,
  `RetailStuntEncodingSurvey`.
- `crates/cs_app/src/stunts.rs` (extend, an F42 owner path): the survey
  `survey_retail_stunt_encoding` and its named refusals
  (`StuntEncodingSurveyError`), which join the scenario to task #427's
  production world-box survey.
- `crates/cs_app/tests/accept_f42_d_stunt_encoding.rs` (new, F42 test path):
  the four unignored tests and the one `#[ignore]`d retail test.
- `crates/cs_app/tests/evidence_report_t463.rs` (new, F42 test path): the
  evidence harness; it is not part of the acceptance suite.
- `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md` (this
  file) and `docs/findings/evidence/T463.json`.

**One observable failure:** the encoding's whole value is *which* world zone a
fly-through objective means, and that joins a **label** in one member
(`ia.zrd`'s `dzones`) to a **box** in another file (`gamez.zbd`, measured by
#427). A reader that invented the label→node direction, or that mistook an
ordinary objective for a stunt gate, would produce a **plausible but wrong
corpus** with the same provenance fields. That is why the label direction, the
target selector and the two `.zrd` record shapes are all pinned, and why the
whole survey is exercised over an authored installation in CI, not only over
retail.

## What was measured

### 1. Where a stunt lives: an instant-action scenario, not a mission

The eight instant-action scenarios are the reader archives
`ZBD/<group>/ia1/zrdr.zbd`, one per world group (`c1`, `c1b`, `c1c`, `c2`,
`c2b`, `c3`, `c4`, `c5`). Each declares its mode and its zone bindings in the
member `ia.zrd`, and its objectives in the member `targets.zrd` — the same
member name the campaign mission readers use, but a different carrier.

### 2. The `.zrd` grammar, and the two record shapes inside it

The `.zrd` members are the typed tree F09 measured
(`docs/findings/2026-10-02-f09-palette-original-faction-palettes.md`): a `u32`
tag then a payload — tag `1` int, `2` float, `3` text (`u32` length + bytes),
`4` list (`u32` count followed by **`count - 1`** children). The decoder in
`cs_content::stunts` is a second, public copy of that grammar because F09's
reader in `cs_content::livery` is private to that module.

Inside the tree the original writes a record in **two** measured shapes, and
`zrd_field` reads both:

- **flat alternating** `[key, value, key, value, …]`, the shape `ia.zrd`'s root
  uses (`["mission_type", ["stunt_flying"], "dzones", [[…], …], …]`). The value
  is often itself a one-element list even when the value is scalar, which is why
  `scenario_mission_type` reads a bare text *and* a one-element list.
- **a list of `[key, value]` pairs**, the shape every `targets.zrd` objective
  uses (`[["description", "…"], ["nodes", ["dz1"]], ["category_label",
  "MSG_OBJ_DZ"], ["help_label", "MSG_OBJ_FLYTHROUGH"]]`).

A fly-through danger-zone target is selected by its own `category_label`/
`help_label` pair (`MSG_OBJ_DZ` / `MSG_OBJ_FLYTHROUGH`), never by position, so
an ordinary objective (a zeppelin, a building) in the same file is not a gate.

### 3. The measured corpus: 54 fly-through danger-zone targets

Measured across the eight scenarios:

| world group | `mission_type` | fly-through targets | `dzones` bindings |
| --- | --- | ---: | ---: |
| `c1` | `dogfight_squadron` | 5 | 5 |
| `c1b` | `stunt_flying` | 5 | 5 |
| `c1c` | `zeppelin_run` | 0 | 0 |
| `c2` | `stunt_flying` | 9 | 9 |
| `c2b` | `zeppelin_run` | 0 | 0 |
| `c3` | `dogfight_squadron` | 4 | 4 |
| `c4` | `stunt_flying` | 14 | 14 |
| `c5` | `stunt_flying` | 17 | 17 |

**54** targets in total, in six world groups; `c1c` and `c2b` author none. **45**
sit in a scenario the original marks `stunt_flying` (`c1b`, `c2`, `c4`, `c5`);
`c1` and `c3` author fly-through objectives under `dogfight_squadron`. This is a
measurement of the *encoding*, not of which mode the game calls a "stunt" — the
45/54 split is exactly what the bytes say.

### 4. The join: every target names a world node task #427 measured

`ia.zrd`'s `dzones` is a list of two-element lists `[<world node>, <label>]`,
e.g. `["dzpath1", "dz1"]`. The first element is the `dzpath<N>` node whose box
#427 measured; the second is the scenario-local label a target's `nodes` names.
The survey resolves the label, then looks the `(world, node)` pair up in #427's
`RetailTriggerVolumeSurvey`. **All 54 resolved to a measured box**: the label
direction is the measured one, and no target names a node the world container
does not carry. Examples the tests pin: `c2`'s `sghangar` label binds
`dzpath1`; `c5` uses `dz1`…`dz16` and `dz18`, skipping `dz17`.

Each row carries its provenance: the container key
(`zbd/<group>/ia1/zrdr.zbd`), that container's SHA-256, the member
(`targets.zrd`), the member's byte span and the installation fingerprint, so
every number can be traced back to the bytes.

## A correction to #427's first reading of the campaign `dzones.zrd`

Task #427 (`docs/findings/2026-10-02-t427-retail-trigger-volume-thickness.md`,
"The second measurement this task found") concluded that the campaign mission
member `dzones.zrd` is "not a length-prefixed value list", because a grammar
that reads the word after the `4` tag as an item **count** (= number of children)
fits the first four values and then breaks. That grammar is the wrong list
convention: F09 measured the count as `children + 1`. Re-tested with the
measured `count - 1` grammar, **all 23** `dzones.zrd` members parse exactly to
their last byte. The 23 reader archives that carry one are the 22 campaign
missions (`ZBD/<group>/M<NN>/zrdr.zbd`) **plus** `ZBD/C5/IA1/zrdr.zbd`, an
instant-action scenario; they run 155–826 bytes each, 9 197 bytes in total. The
alternative grammar that reads the word after a `4` tag as the child count
consumes **0 of the 23** exactly, so #427's own review result reproduces on this
corpus. So the framing of `dzones.zrd` *is* consistent with the same
value-list grammar as the instant-action `ia.zrd`; what is still unmeasured is
its **semantics** — which zones a mission uses and what its objective numbers
mean — which remains the follow-up task's question (owner `crates/cs_formats/`,
prefix `accept_t427_dzones_`). This record does not change #427's own survey or
its `zone_declarations_are_decoded() == false` state; it corrects only the
framing claim and points the follow-up at the grammar that parses.

## What is **not** measured, and is therefore not a field

The scenario bytes carry a mode, a zone binding, a target and a localized
description. They do **not** carry, and this task does **not** invent:

- **a direction rule** — the sequence or heading a completion must satisfy
  (`RetailStuntEncodingSurvey::direction_rule_is_measured() == false`);
- **a clearance rule** — the rim margin or altitude a completion must keep
  (`clearance_rule_is_measured() == false`);
- **a payout or a linked scrapbook photo** (`reward_is_measured() == false`);
- **a repeat policy** — whether a second pass pays again
  (`repeat_is_measured() == false`).

Those fields stay explicit unknowns on the declared `cs_content::stunts` record
and refusal points in the lowering boundary (`cs_app::stunts::lower_stunt`). The
survey reports the unmeasured flags rather than filling any of them in.

## Evidence

Ordinary build/test plus read-only `retail` access. Commands run locally over
the recorded candidate tree:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # exit 0
cargo test --workspace --locked -- accept_f42_d_ --include-ignored
#   5 tests run, 5 passed (crates/cs_app/tests/accept_f42_d_stunt_encoding.rs)
#   of which 4 unignored (CI) and 1 #[ignore = "requires CS_GAME_DIR"]
```

The acceptance run's log and a second production observation (the survey's own
census of the 54 rows, their boxes and spans) are recorded under
`private/evidence/T463/`; the report is committed as
`docs/findings/evidence/T463.json` and checked with
`tools/validate_evidence.py --require-pass`. The claim is **`implemented`**: the
encoding and geometry are measured, but the semantics of the traversal rule and
the payout are not.

## Known limitations that gate later stages (not silently dropped)

- **The traversal and payout rules are unmeasured.** Affected content: the
  completion predicate and the reward of every stunt. Resolving tasks: the F42-A
  rule work and #464 (repeat/payout), #465 (AI earning).
- **No original run happened.** Nothing here is evidence of the original's
  runtime behaviour: not that a `stunt_flying` scenario awards anything, not
  that crossing a `dzpath` fires the objective, not that the label→node join is
  the one the executable uses. A capture from an actual original run is the only
  thing that can settle those, and it needs the owner.
- **`unk140` remains a correlation, not a decode** (inherited from #427): the
  box *values* are measured, their *interpretation* is not.
- **The campaign `dzones.zrd` semantics are unmeasured** (the framing
  correction above does not read a mission's zone set).

## Review (2026-10-02)

Reviewer: **`deepseek-1/deepseek-1`, the same agent identity that implemented
the task**, in a later Rally session with a fresh context (the implement claim
of 2026-10-02T16:49Z, this review claim of 2026-10-02T17:28Z). Per AGENTS.md a
same-agent review is **not independent evidence**, and no agent review replaces
the owner's approval. What follows is a fresh-session check of the code, the
corpus and the evidence, nothing more.

The review re-ran all four checks on the branch before these review commits
(the code tree `6608b55122f33ad981d5b13aad266cb81878eb47`, the tree of code
commit `0104487`; the review changed no code):
`cargo fmt --all -- --check` = 0,
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
= 0, `cargo test --workspace --locked` = 0, and
`cargo test --workspace --locked -- accept_f42_d_ --include-ignored` = 0
(5 discovered, 5 passed, 0 failed, 0 ignored). The evidence report was
regenerated on this reviewed tree and validates with
`tools/validate_evidence.py --require-pass`; its two artifacts' SHA-256 match
`private/evidence/T463/`.

The corpus was re-measured with a **from-scratch parser written for the review
and sharing no code with the workspace** (reader-archive trailer index and a
recursive `count - 1` `.zrd` reader). Every number the record asserts
reproduces:

| assertion | record | review's own parse |
| --- | --- | --- |
| fly-through danger-zone targets | 54 | 54 |
| of them `stunt_flying` | 45 | 45 |
| per world | `c1` 5, `c1b` 5, `c1c` 0, `c2` 9, `c2b` 0, `c3` 4, `c4` 14, `c5` 17 | identical |
| mission types | `c1`/`c3` `dogfight_squadron`, `c1c`/`c2b` `zeppelin_run`, else `stunt_flying` | identical |
| label→node direction | `[world node, label]`, every target resolved | 54 of 54 |
| `dzones.zrd` members | 23, 155–826 bytes, 9 197 total | identical |
| `dzones.zrd` under `count - 1` | 23 of 23 parse exactly | identical |
| `dzones.zrd` under `count = children` | 0 of 23 parse exactly | identical |

The direction row matters most: **every** selected target's `nodes` label
matches a `dzones` entry whose *first* element is a `dzpath<N>` world node (and
not the reverse), so the join #427's world boxes are looked up through is the
measured one, not an assumed one.

### What the review changed

1. **A clarification, not a correction, to the `dzones.zrd` count.** The "23
   campaign mission readers" are really **22 campaign missions plus
   `ZBD/C5/IA1/zrdr.zbd`**; the member count, byte total and grammar result are
   unchanged and the correction above is unaffected. A reader reproducing the
   count from `M*/` alone would otherwise find 22 and think the record wrong.

### What the review did not change, and why

* **The target selector that unions the two labels.** It is deliberately the
  union of `MSG_OBJ_DZ` and `MSG_OBJ_FLYTHROUGH`, and the retail corpus carries
  no target with only one of them (the `#[ignore]`d test asserts every selected
  row has both), so the union and the intersection select the same 54. A union
  is the looser reading; it is kept because the retail bytes do not discriminate
  between them and the selector is not what this stage measures.
* **The two unmeasured rules and the payout stay unmeasured.** Nothing in the
  scenario bytes carries a direction rule, a clearance rule, a reward or a
  repeat policy; the survey reports them as unmeasured rather than filling a
  guess.
* **No `verified_original`.** No original run happened, so nothing here is
  evidence of the original's runtime behaviour.

## Sources used

- `specs/F42-stunts-fame-photos-and-optional-achievement-events.md` (F42-D) and
  `docs/findings/2026-10-01-f42-a-traversal-predicates-and-reward-identity.md`
  (the F42-A unknowns this task answers for the encoding half).
- `docs/findings/2026-10-02-t427-retail-trigger-volume-thickness.md` (the world
  box measurement this survey joins to, and the framing claim corrected above).
- `docs/findings/2026-10-02-f09-palette-original-faction-palettes.md` (the
  `.zrd` grammar) and `crates/cs_content/src/livery.rs` (its private reader).
- `crates/cs_formats/src/zbd/{trailer,reader_archive}.rs`,
  `crates/cs_formats/src/script_raw/discovery.rs` and
  `crates/cs_app/src/world/triggers.rs` (the production reader-archive
  discovery and the node survey the join uses).
- The owner's installation, read-only, over `$CS_GAME_DIR`: the eight
  `ZBD/<group>/ia1/zrdr.zbd` reader archives and all 23 `dzones.zrd` members
  (the 22 campaign mission readers and `ZBD/C5/IA1/zrdr.zbd`). Installation
  fingerprint above.

**No original data is committed.** The numbers here are counts, offsets, spans
and digests; no extracted `.zrd`, no string table, no mesh and no screenshot is
in the repository, and every private output went to `private/`, outside it.

## Update (task #533, 2026-10-10)

Task #533 decided the fly-through selector's label rule in favour of the
**union** of the two measured labels: `scenario_fly_through_targets` now selects
a record when its `category_label` is `MSG_OBJ_DZ` **or** its `help_label` is
`MSG_OBJ_FLYTHROUGH`. What that changes and confirms in this document, with the
affected records named:

* **§3's corpus is confirmed unchanged**: the eight instant-action scenarios
  author **54** fly-through targets, **45** of them `stunt_flying`, with the
  same per-world table and the same resolved boxes — every instant-action
  labelled record carries both labels, so the stricter rule and the union read
  the same rows. This document's review section said "the retail corpus carries
  no target with only one of them"; that was measured over the eight
  instant-action scenarios and stays true of them, and over the whole
  installation three campaign records carry only the help label (see below).
* **The selector's rule, as this document's review section described it ("the
  union … the retail corpus carries no target with only one of them"), is now
  the implemented rule** for the whole installation. The three records the old
  `?`-on-`category_label` selector dropped are `ZBD/C1/M02`'s `MSG_OBJ_ZEPHANGER`
  at `h3_marker`, `ZBD/C4/M03`'s `MSG_TRGT_DEVILSHORN` at `dz2` and
  `ZBD/C5/M02`'s `MSG_TRGT_PHQ` at `dz1`.

The decision, its evidence and the re-measured counts are in
`docs/findings/2026-10-10-t533-fly-through-label-rule.md`.
