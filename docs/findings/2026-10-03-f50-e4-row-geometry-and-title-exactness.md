# F50-E4: a retail comparison test for the row geometry and title exactness

Date: 2026-10-02. Task: F50-E4 "Add a retail comparison test for the row
geometry and title exactness rules" (Rally #481, work order `F50-E4` of
`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`). Raised by
the M18-A review of Rally #309, recorded in
`docs/findings/2026-10-02-m18-a-source-binding.md`. Capabilities used:
`retail` (`$CS_GAME_DIR` read-only, never written), `synthetic` (nothing
executed). Implementer: **bunny-2** (OpenCode, Space Bunny Free, Rally #481
implement claim of 2026-10-02T22:04Z). Reviewer: the agent Rally assigns to
#481's review claim; this document is written before that review, so it records
the implementer's own run and no reviewer identity — see *Review identity*.

## What changed

No production code. `crates/cs_content/src/campaign_bindings.rs` is untouched;
the stage adds `crates/cs_app/tests/campaign/f50_e4.rs` (three
`accept_f50_e4_*` retail tests), `mod f50_e4;` in
`crates/cs_app/tests/campaign/main.rs` (wiring), the F50-E4 harness and its
test-name constant in `crates/cs_app/tests/campaign/evidence.rs`, this document
and `docs/findings/evidence/F50-E4.json`.

## The gap this closes

`cs_content::campaign_bindings` decides which localized rows may name a campaign
position with two rules:

1. `SourceContext::campaign_title_blocks` — a row run must be **exactly** as long
   as the campaign the directory layout declares;
2. `title_form` / `SourceContext::confirm_title` — a row carries a title **byte
   for byte**, either as its whole display text (`TitleForm::Verbatim`) or as
   the tail of a region-prefixed long name
   (`TitleForm::RegionPrefixedLongName`).

Every per-mission binding stage (M01-A … M24-A) proved these two rules on
*authored* values, through synthetic tests in the stage's own suite, and proved
their *consequences* against `$CS_GAME_DIR`. Nothing re-derived the string
table's own row geometry from the installation. While verifying M18-A,
mutating rule 2 to accept a tail that merely **starts with** the title left all
eight `accept_m18_a_*` retail tests green (`docs/findings/2026-10-02-m18-a-source-binding.md`,
*Mutation checks*). This stage closes that gap the only way that can
discriminate: by deriving the same facts a second time, from the installation,
in a test that shares **no helper** with the code under test.

The independent reading is deliberately not a copy:

| Fact | Production derives it by | This stage derives it by |
| --- | --- | --- |
| which rows carry display text | `present_string_ids`, filtering `self.strings.rows()` in place | building a `Vec<u32>` of ids, sorting and deduplicating it |
| the runs | `title_blocks`, scanning a `BTreeSet<u32>` iterator | scanning a sorted `&[u32]`, closing a run only when `next != last + 1` |
| the campaign length | `scan_campaign`, walking `ZBD/<chapter>/<mission>` | walking the same tree again in the test, with its own chapter-number parse |
| which titles a row carries | `confirm_title` searching *per title* over the rows | building the index *per row*: each row contributes the one or two titles it can carry, and the lookups run the other way |
| how a row carries a title | `title_form(display, title)` | this file's own `carried_by(display, title)` and `Form` enum, mapped onto `TitleForm` only after both sides have decided |

## What was measured on `$CS_GAME_DIR`

`install_sha256` `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
— the same installation as M01-A … M24-A. The localized table is
`GOSDATA/ASSETS/BINARIES/langui.dll`.

| Fact | Value |
| --- | --- |
| decoded `RT_STRING` rows | 1616 (101 blocks, ids 0…40175, one language, 1033) |
| rows holding no text | 369 |
| rows carrying display text | 1207 |
| maximal runs of those rows | 76 |
| campaign length, from the `ZBD` layout | 24 missions, chapter sizes `[5, 5, 5, 5, 4]` |
| runs exactly as long as the campaign | **two**: `3450..3473` (region-prefixed long names) and `3480..3503` (bare short names) |
| longest run | `40081..40170` (90 rows) |
| runs of 23, 25 and 26 rows | `10499..10521` (23), `1165..1189` and `40055..40079` (25), `109..134` (26) — so "about the campaign length" would report four or five runs where the exact rule reports two |

The whole 76-run geometry, as the independent reading measured it:

```
9..80 82..83 85..88 97..97 100..107 109..134 136..136 200..214 500..508 510..521 700..712
1001..1032 1035..1047 1052..1056 1060..1074 1076..1118 1120..1120 1122..1129 1131..1143
1149..1160 1165..1189 1191..1228 1251..1259 1300..1301 1400..1401 3000..3010 3020..3030
3040..3050 3060..3079 3100..3165 3170..3235 3240..3305 3307..3307 3310..3315 3320..3325
3330..3335 3350..3354 3360..3374 3380..3391 3395..3406 3410..3421 3425..3438 3450..3473
3480..3503 3600..3618 3650..3656 3660..3663 3670..3682 3695..3697 3700..3710 4000..4009
10001..10046 10048..10048 10050..10050 10052..10065 10067..10087 10089..10103 10105..10141
10143..10145 10499..10521 10524..10525 10532..10532 10534..10536 10539..10552 10554..10566
10576..10580 10588..10588 20000..20003 30001..30002 40000..40000 40002..40013 40015..40035
40037..40039 40041..40053 40055..40079 40081..40170
```

Run widths, as a multiset: `1×9, 2×5, 3×4, 4×3, 5×3, 6×3, 7×1, 8×2, 9×2, 10×1,
11×4, 12×6, 13×6, 14×3, 15×4, 19×1, 20×1, 21×2, 23×1, 24×2, 25×2, 26×1, 32×1,
37×1, 38×1, 43×1, 46×1, 66×3, 72×1, 90×1`. This table is also what an
independent Python re-reading of the PE `RT_STRING` tree produced before the
Rust test existed; the two agree exactly, and the Rust test now derives it from
the production reader and holds production to it.

## The near-miss set, and what the exactness rules do with it

219 candidate titles, every one derived in the test from the installation's own
campaign rows or from `missions/bindings/campaign-inventory.tsv`:

| Candidate source | Count | Independent outcome |
| --- | --- | --- |
| the 24 declared work-order titles | 24 | 17 confirmed (16 verbatim, 1 long name), 7 uncarried |
| each long name's tail, plus a strict prefix of it, plus `… .`, ` …`, lower-cased | 120 | the tail confirms as a long name; the four near misses are uncarried |
| each short name, plus a strict prefix of it, plus a trailing space, upper-cased | 96 | the short name confirms verbatim; the three near misses are uncarried |
| the five distinct region prefixes of the long-name run | 5 | 3 confirmed verbatim (rows 1221, 1222, 1223), 2 ambiguous (`Hawaii` at rows 1220 **and** 3652; `Manhattan` at 1224, 3653, 10084, 10561) |
| the shortest row carrying two `" - "` separators (row 3333) | 3 | the text after the *first* separator confirms as a long name; the middle and the last segments are uncarried |

Totals: **27 confirmed verbatim, 19 confirmed through a long name, 2 ambiguous,
171 uncarried.** Every one of the 219 agrees with
`SourceContext::confirm_title`, and every one of the 1207 present rows agrees
with `title_form`, row by row — 264 333 comparisons in the second test, all four
class counts and all three weakened-matcher counts pinned in the test itself.

Three facts about the installation make the exactness arms real rather than
formal:

- **The seven refused declared titles are near misses of real rows.** `M09`
  *Perils for Blake* is one edit from row 3488 *Peril for Blake*; `M11` *The
  Stolen Scarlet* one edit from 3490 *The Stolen Starlet*; `M14` *Clash of the
  Dreadnaughts* four from 3493; `M15` *The Fight for the Figaro* five from
  3494 *Fight for the FIGAROA*; `M20` *Unholy Alliance*, `M22` *Runaway Witness*
  and `M23` *Criminal Exodus* four each from 3499, 3501 and 3502. The test
  measures the distance with its own Levenshtein and requires it to be in 1…=5,
  so a refusal that had drifted into "nothing nearby at all" would fail.
- **A weakened matcher has something to take.** Over the same 219 titles a
  prefix matcher would confirm 27 of the uncarried ones, a substring matcher 72
  and a case-insensitive matcher 48. `confirm_title` refuses every one of them.
- **Six titles are carried twice** — once verbatim in `3480..3503` and once as a
  long name in `3450..3473` (`The Great Plane Robbery`, `Raid on the Rocky
  Express`, `Deceit at Devil's Horn`, `Rescue the Black Swan`, `Death on the
  Docks`, `Battle over Broadway`) — so the rule's *order*, verbatim first, is
  decided by real data. The seventh long-name-only case is `M05`, whose bare
  short name reads `Union Jack's Revenge` while the declared title is `The Union
  Jack's Revenge`, which is why exactly one declared title confirms through a
  long name.

## Test inventory (`accept_f50_e4_*`, 3 tests, all retail)

| Test | What it pins |
| --- | --- |
| `accept_f50_e4_the_row_runs_are_exactly_the_ones_an_independent_reading_finds` | the campaign length and chapter sizes re-derived from `ZBD`; the present set, the 76 runs, their maximality, disjointness and ascending order; both campaign-length runs; `campaign_title_blocks` held to them in **both** directions; every run that is not 24 rows refused; the truncated and extended versions of both campaign runs refused; and the 48 campaign rows' own byte ranges re-derived from the decoded units of their `RT_STRING` blocks and decoded back out of the image |
| `accept_f50_e4_the_confirmed_rows_are_exactly_the_exact_byte_matches` | `title_form` over **every** row against **every** candidate title; `confirm_title` against the independently inverted index for all 219 candidates, in each outcome class; the four outcome classes each exercised at least N times; 17 of 24 declared titles confirmed with one long-name-only and seven refused (named); the both-forms arm; and the five region prefixes, three confirmed and two ambiguous |
| `accept_f50_e4_a_fuzzy_matcher_would_confirm_a_near_miss_this_table_refuses` | the three weakened matchers, each measured over the table, each required to have at least ten near misses to take and each refused by `confirm_title`; and the seven refused declared titles, each within 1…=5 edits of a real row |

## Mutation checks

Fourteen mutations of `crates/cs_content/src/campaign_bindings.rs` were applied
one at a time and reverted before the next. Every one is caught:

| Mutation | Caught by |
| --- | --- |
| `title_form` accepts a tail that merely **starts with** the title | tests 2 and 3 (this is the mutation M18-A's suite missed) |
| `title_form` compares the tail case-insensitively | tests 2 and 3 |
| `title_form` compares the verbatim display case-insensitively | tests 2 and 3 |
| `title_form` compares the tail after `trim` | tests 2 and 3 |
| `title_form`'s verbatim arm removed | test 2 |
| `title_form` splits at the **last** separator instead of the first | tests 2 and 3 |
| `confirm_title` consults the long-name rows before the verbatim row | test 2 |
| `confirm_title` reports several verbatim rows as `Uncarried` instead of `Ambiguous` | test 2 |
| `campaign_title_blocks` drops the campaign-length filter | test 1 |
| `campaign_title_blocks` accepts a run **at least** as long as the campaign | test 1 |
| `campaign_title_blocks` reports only the first campaign-length block | test 1 |
| `present_string_ids` keeps rows that hold no text | test 1 |
| `present_string_ids` drops the rows that do hold text | test 1 |
| `title_blocks` merges runs across gaps | test 1 |

Test 1 is the geometry test, test 2 the confirmation test and test 3 the
weakened-matcher test. Two notes on how the sweep was run, so a reviewer can
repeat it: twelve mutations were applied through a script that refuses to write
unless its anchor matches the file exactly, and two (`confirm_title`'s arm order
and `campaign_title_blocks`' first-block truncation) had to be applied by hand
after the script's anchor for them did not match. A run whose anchor did *not*
match therefore reported "ok" — that run was of unmutated code and counts for
nothing; both mutations were then applied by hand and observed failing as the
table says.

## What is not claimed

- The row geometry and the title comparison are **measured facts about this
  installation's string table**, not original-game rules. `campaign_title_blocks`
  and `title_form` remain heuristics that this stage has checked against one
  installation, and an edition with a different campaign length would need a new
  measurement.
- The pins (1207 present rows, 76 runs, the two campaign-length runs, the
  widths, the 24 mission titles and their outcomes, the four outcome classes and
  the three weakened-matcher hauls) are properties of *this* installation. A
  different retail edition will need them re-derived; that is a deliberate cost,
  because a retail acceptance test that adapts to whatever it reads proves
  nothing.
- The title-to-directory join is still an inference (`ClaimStatus::Inferred`) and
  nothing here is `verified_original`. No original executable was run, so nothing
  in this document is evidence of the original's runtime behaviour, of how the
  original grouped its mission names, or of which row the original displayed.
- `present_string_ids`, `strip_font_tag` and `title_blocks` are private, so the
  tests reach them only through `campaign_title_blocks`. The mutations above were
  applied to the private bodies and observed through that door; a reviewer who
  wants the same observation from outside must do the same.
- The M18-A suite is untouched, as the task requires.

## Review identity

The evidence harness reads `CS_EVIDENCE_REVIEWER` at run time instead of
carrying a literal, so the committed report cannot contain a hand-over
placeholder: `docs/findings/evidence/F50-E4.json` records the implementer's run,
and the reviewing agent regenerates it on the reviewed tree with its own
identity. `tools/tests/test_evidence_review_identity.py` reads this report's
harness as the `runtime` shape and checks it against
`CS_EVIDENCE_REVIEWER`; note that this check is already red on `main` for six
other stages (T463, T464, T465, F30-D, F11-D2, F22-H), which is **not** this
task's slice and is not filed here as fixed. F50-E4 adds no Rally review facts to
`docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json`: that file
belongs to another task, and adding a `merge_event` for #481 before the merge
would be writing a review fact that does not exist yet.

## Checks

| Command | Exit |
| --- | --- |
| `cargo test --workspace --locked -- accept_f50_e4_ --include-ignored` | 0 (3 tests) |
| each of the three alone with `--exact --include-ignored` | 0, 0, 0 |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `tools/validate_evidence.py private/evidence/F50-E4/acceptance.json --artifact-root private/evidence/F50-E4 --require-pass` | 0 |

CI has no original data: all three tests are `#[ignore = "requires CS_GAME_DIR"]`
and are reported `ignored` there, exactly as M16-A-FU1's two are. The split the
CI contract depends on was confirmed locally: a plain
`cargo test --locked -p cs_app --test campaign` runs the rest of the campaign
suite (63 passed) and reports these three as `ignored, requires CS_GAME_DIR`.

`docs/findings/evidence/F50-E4.json` was written on the commit
`5d9ca646` and validated with `--require-pass`; its `candidate_tree` is that
commit's tree. The delta after it is this document and the pinning of the class
counts and weakened-matcher hauls, which are measurements of the same
installation, not new production code — a reviewer who re-runs the harness on
the reviewed tree gets the same numbers and a fresh `candidate_tree`.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_content::config::StringCatalog` and `cs_content::campaign_bindings`;
`schemas/evidence.schema.json`; `missions/bindings/campaign-inventory.tsv`;
`docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-02-m18-a-source-binding.md`;
`docs/findings/2026-10-02-m16-a-source-binding.md`;
`docs/findings/2026-10-02-m16-a-fu1-title-row-span.md`.
