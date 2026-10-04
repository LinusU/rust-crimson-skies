# F39-E7: the contract's sixth distinction, and where a detached actor actually lives

Date: 2026-10-04. Task: F39-E7-DETACHED "Reconcile the objective condition
vocabulary with the contract's sixth distinction (detached)" (Rally #604). The
sheet `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
has no `### F39-E7` section, so the task-test prefix this stage uses is
`accept_f39_e7_`, stated here as the sheet states a prefix per stage. Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail`
(read-only `$CS_GAME_DIR`). Evidence report: `private/evidence/F39-E7/acceptance.json`,
committed as `docs/findings/evidence/F39-E7.json`.

## The question

`docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering") says:

> Conditions distinguish disabled, dead, captured, escaped, detached and
> despawned.

That is **six**. The engine declares **five**:

| vocabulary | entries | where |
| --- | --- | --- |
| `cs_script::ir::ActorState` (what a `Condition::ActorIs` compares against) | `Alive`, `Disabled`, `Dead`, `Captured`, `Escaped`, **`Detached`**, `Despawned` — all six | `crates/cs_script/src/ir.rs` |
| `cs_sim::objectives::counters::CountKind` (what a session counts) | `Destroyed`, `Disabled`, `Captured`, `Escaped`, `Despawned` — five | `crates/cs_sim/src/objectives/counters.rs` |
| `cs_content::objectives::DeclaredCountKind` (what a declared record may count) | the same five | `crates/cs_content/src/objectives.rs` |

So the contract's sentence is about the **condition** vocabulary, and
`ActorState` satisfies it literally: all six exist. What did **not** exist
anywhere was a statement of where the sixth lives — no `CountKind`, no producer,
no measured spelling, and no F39-A/B/C/D finding recording it as an unknown. The
F39 sheet's own non-negotiable 2 names the same five as the counters. Left
alone, the five-category vocabulary reads as though it satisfied the contract's
six.

## The answer, in one line

**A detached actor is an event in the original, not a counted category, and in
this engine it is a released payload that keeps its objective.** No mission and
no shared/world-group reader in the installation declares a counted condition or
an objective kind that spells a detached category, over a published
twelve-stem spelling family; the corpus *does* write detaches, in the
objective-block trigger vocabulary, where they **complete an objective** instead
of counting detached actors.

## Files and the one observable failure (listed before editing)

* `crates/cs_content/src/objectives.rs`: the F39-E7 section —
  `DETACH_STEM`, `DETACHED_SPELLING_STEMS`, `DetachedVocabularySurface`,
  `detached_spelling_family`, `DetachedSpellingSite`,
  `MeasuredDetachedVocabulary`, `measure_detached_vocabulary`,
  `DETACHED_TARGETS_MEMBER`, and the module docs above them.
* `crates/cs_sim/src/objectives/counters.rs`: `CountKind::producer`, the
  `CountKind` doc note about the missing sixth.
* `crates/cs_app/src/objectives.rs`: the F39-E7 section —
  `NO_LIFECYCLE_TRANSITION_REPORTS_THIS_CATEGORY`,
  `DETACHED_IS_AN_EVENT_NOT_A_COUNTED_CATEGORY`, `ContractDistinction`,
  `ContractDistinctionReading`, `contract_condition_distinctions`,
  `DetachedCensusError`, `ReaderScope`, `DetachedVocabularyRow`,
  `DetachedVocabularyCensus`, `DETACH_CONTRACT_STEM`,
  `survey_retail_detached_declarations`, and the `SCENARIO_TARGETS_MEMBER`
  import that walk needs.
* `crates/cs_app/tests/accept_f39_e7_detached_condition_vocabulary.rs` (new):
  6 unignored + 1 retail, prefix `accept_f39_e7_`.
* `crates/cs_app/tests/evidence_report_f39_e7.rs` (new): the evidence harness.
* Wiring only: none — every edited file is an owner path of the F39 sheet.

**One observable failure.** Nothing could answer "does the engine implement the
contract's six distinctions?", so nothing could fail when it stopped doing so.
The five-category vocabulary read as complete, the sixth was recorded nowhere,
and a reader of either the contract or `counters.rs` had to guess which of the
two lists was authoritative. The new query is observable: six rows, each naming
its vocabulary entry, its producer and — where it has none — one of two **named**
reasons; a seventh distinction added to the enum cannot go missing from the
table, and a category that gains a producer without being declared by the
original cannot quietly read as supported.

## What was measured, over the owner's installation

One pure walk, run by the census and by the acceptance suite's fixtures:
`cs_app::objectives::survey_retail_detached_declarations` opens **every**
`zrdr.zbd` reader archive in the installation, decodes its `objectives.zrd` with
the production `.zrd` reader and reads it through
`cs_content::objectives::measure_detached_vocabulary`, decoding the same
archive's `targets.zrd` when it declares one.

**This walk deliberately has a bigger denominator than F39-D's and F39-E4's.**
Both of those covered mission-scoped archives only (F13-B's rule: exactly
`zbd/<group>/<mission>`), which left the shared reader `zbd/zrdr.zbd` (220
members) and the nine world-group readers `zbd/<group>/zrdr.zbd` outside the
count — F39-D unknown #5, the item task #597 owns. A detach is exactly the kind
of thing a shared animation/world reader would spell, so those nine are read
here and reported in their own scope, never folded into the mission count.

**What the wider denominator measured for blocks: none of the nine declares an
objective record at all.** The shared and world-group readers hold animation,
sound and motion records; not one carries an `objectives.zrd` member, so they
contribute **no** name to the counted-condition or block-declaration surfaces. One
of them — `zbd/c1c/zrdr.zbd` — *does* declare a `targets.zrd`, so it does
contribute objective kinds, which is F39-E4's unknown #3 (its "sixth
`MSG_OBJ_DISABLE` record, outside this census") seen from this side. That is why
the two scopes are reported side by side instead of as one number: "62 archives
read", "53 declare objective records" and "54 declare objective kinds" are three
different facts.

| measurement | Value |
| --- | --- |
| installation SHA-256 | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| reader archives read | 62 |
| …mission-scoped | 53 |
| …shared / world-group | 9 |
| archives declaring an `objectives.zrd` | 53 — every mission-scoped reader, and no shared one |
| archives declaring a `targets.zrd` | 53 (52 mission-scoped — `zbd/c1c/m01` declares none — plus the shared `zbd/c1c/zrdr.zbd`) |
| counted-condition names | 3244 sites / 191 distinct |
| objective-kind names (`help_label`, `category_label`, `description`) | 758 sites / 170 distinct |
| block-declaration names (every text element of every `OBJECTIVE<n>`) | 3523 sites / 1284 distinct |

These are **this stage's own** numbers, with this stage's own surface
definitions. They are not F39-E4's census and do not widen it: E4 measured 226
distinct counted-condition names over the counted conditions *beside a
threshold*, mission-scoped, under its own segmentation; this walk reads every
stage element on a block that declares a threshold, over 62 archives, and also
counts the two other surfaces. Neither figure replaces the other, and the
finding quotes only its own.

### The three surfaces, and why they are kept apart

| surface | what it can declare | can a name here be a counted category? |
| --- | --- | --- |
| `CountedCondition` | the `INACTIVE<n>` conditions of a block that also declares `INACTIVE_COMPLETION_COUNT` — the only counter the records write | yes |
| `ObjectiveKind` | a `targets.zrd` record's `help_label`, `category_label`, `description` | no — a localized label, which is exactly why F39-E4 refused to make one a producer |
| `BlockDeclaration` | every other text element of every `OBJECTIVE<n>` block: `WAKE_ANIM`, `COMPLETED_SOUND_GROUP`, `SET_HELP_LABEL`, … | no — this is the vocabulary the objective's *triggers* are written in |

A stage that stands beside no threshold is **not** a counted condition and is
read as a plain declaration instead. The threshold is what makes a stage a
count, and this reader keeps the two apart rather than assuming a stage is one.

### The spelling rule

One rule, published: split a name on `_`, and a family claims it when one
**segment** begins with that family's stem, case-insensitively; the first family
in the list wins, so the answer cannot depend on iteration order.
`DETACHED_SPELLING_STEMS` is twelve stems — `DETACH`, `RELEASE`, `DROP`,
`EJECT`, `JETTISON`, `LAUNCH`, `LOOSE`, `FREE`, `UNDOCK`, `UNCOUPLE`,
`DISCONNECT`, `CASTOFF` — the contract's own word plus the release/drop/ejection
vocabulary the installation is measured to use for scripted detaches.

Segment-prefix rather than whole-string, because the measured labels carry a
`MSG_OBJ_` prefix; segment rather than substring, so no stem can swallow a state:
`redetached` and `undroppable` match nothing, and the ten measured part-state and
actor spellings (`healthy`, `healthy_part`, `healthy_balloon`, `panels`,
`reng11`, `gasbag3`, `piratezep`, `cargozep2`, `balmoral_1`, `sprucegoose`)
match nothing.

### The five categories, for the sixth's sake

| family stem | counted conditions | objective kinds | block declarations |
| --- | --- | --- | --- |
| `DETACH` | **0** | **0** | **0** |
| `RELEASE` | **0** | 1 | 0 |
| `DROP` | **0** | 0 | 7 |
| `EJECT` / `JETTISON` / `LOOSE` / `UNDOCK` / `UNCOUPLE` / `DISCONNECT` / `CASTOFF` | 0 | 0 | 0 |
| `LAUNCH` | 0 | 0 | 16 |
| `FREE` | 0 | 0 | 1 |

**Zero** sites spell `DETACH` on any of the three surfaces, over all 62
archives. Zero spell any other stem on the counted-condition surface. Exactly one
objective kind spells the family: `MSG_OBJ_RELEASE`, the `help_label` on
`cargozep1` in `zbd/c4/m03`. It is a localized label on a target, which F39-E4's
finding already rules is not a counted transition, and one site is not a
vocabulary.

The non-zero that keeps this absence from being vacuous: **24** block
declarations spell the release family, and all seven `DROP` sites are block
declarations of a numbered `OBJECTIVE<n>`:

| archive | block | name |
| --- | --- | --- |
| `zbd/c1c/m01` | `OBJECTIVE11` | `activate_dropoff_node` |
| `zbd/c1c/m01` | `OBJECTIVE11` | `wv_drop_copilot` |
| `zbd/c1c/m01` | `OBJECTIVE49` | `activate_dropoff_node` |
| `zbd/c2/m05` | `OBJECTIVE23` | `drop_paratroopers` |
| `zbd/c3/m01` | `OBJECTIVE21` | `enable_dropoff` |
| `zbd/c3/m01` | `OBJECTIVE22` | `disable_dropoff` |
| `zbd/c4/m03` | `OBJECTIVE2` | `dropped_blacke` |

The `LAUNCH` sites are spawn-group names (`launch_warhawk` ×9, `launch_brigand`
×6, `launch_autogyro` ×3, all in `zbd/c4/m04` — F34-C's own machinery), and the
one `FREE` site is `free_the_goose` on `zbd/c5/m03 OBJECTIVE37`, the Spruce
Goose. They are counted because they are in the family and they are on a
declared surface; naming them is what stops the family from being quietly
narrowed to make the answer come out zero.

The release/drop vocabulary that lives in the **shared** readers — `release_hook`
and its `snd_release_hook` sound group, the `warlaunchhook` animation,
`drop_smokescreen_canister` and the `dropit` object motion, all measured in
`zbd/zrdr.zbd` and `zbd/c4/zrdr.zbd` — is in those archives' **animation,
sound-group and motion records**, and none of the nine declares an objective
record, so none of that vocabulary reaches any of the three surfaces. That is the
structural reason it cannot be a counted category, and it is measured from the
archives' own member lists rather than inferred from a spelling.

**`zbd/c2/m05 OBJECTIVE23` is the clearest instance of the measured shape.** Its
whole block is:

```
OBJECTIVE23  BEGIN_DORMANT  WAKE_ANIM drop_paratroopers
             SET_AI_NET balmoral_1 M5Return
             REMOVE_OBJECTIVE_TARGET balmoral_1
             SET_HELP_LABEL balmoral_1
             NAP_OBJECTIVE_WHEN_I_COMPLETE  KILL_OBJECTIVE_WHEN_I_COMPLETE
```

No `INACTIVE<n>`, no `INACTIVE_COMPLETION_COUNT`. The paratrooper drop — the
clearest "detached actor" in the game — **completes** an objective and then naps,
kills and wakes other objectives. The corpus's paratrooper vocabulary
(`chuteman`, `chuteman_drop`, `chutemanparent`, `deploy_pchute`, `chuteopen`,
`chuteopen.zan`) lives in the same archive's **animation** records, and
`drop_paratroopers` is a `WAKE_ANIM`: a scripted event that wakes a block, not a
count over a category of detached actors. This is measured structure, not an
inference about what the drop does.

### A corroborating byte-level observation (not a production measurement)

`grep -ril detach "$CS_GAME_DIR"` over the whole installation returns **two**
files, and neither is game data:

* `dsetup32.dll` — `DLL_PROCESS_DETACH`, a Windows loader export name.
* `ifc21.dll` — `CImmDevice::detach_effects`, the DirectInput interface DLL.

The token `detach` appears in **no** mission record, in no `strings.dll` entry
and in no `crimson.exe` string. This is a byte-level grep recorded here as
corroboration of the census, with its sensitivity stated: the same grep finds
`MSG_OBJ_DESTROY` 114 times inside the same `zrdr.zbd` archives, so the archives
store these names in the clear and a missing token is a missing token. It is
**not** a production measurement — no test asserts it, because a whole-tree scan
of 625 MB of assets is minutes of work that the decoded census already does
properly — and it proves nothing about a name spelled in another language or a
non-ASCII encoding.

## The verdict, and the query that carries it

The task offered three outcomes. The measured answer is the second — a measured
absence with the mechanic named — and the two named verdicts are production
constants, not prose:

* `cs_app::objectives::DETACHED_IS_AN_EVENT_NOT_A_COUNTED_CATEGORY`
  (`"detached_is_an_event_not_a_counted_category"`) for `detached`.
* `cs_app::objectives::NO_LIFECYCLE_TRANSITION_REPORTS_THIS_CATEGORY`
  (`"no_lifecycle_transition_reports_this_category"`) for `disabled` and
  `escaped`.

`cs_app::objectives::contract_condition_distinctions()` is the single queryable
answer to "does the engine implement the contract's six distinctions?". It
resolves each of the six to:

| contract spelling | `ActorState` | `DeclaredCountKind` | `CountKind` | producer (`LifecycleKind`) | named reason |
| --- | --- | --- | --- | --- | --- |
| `disabled` | `Disabled` | `Disabled` | `Disabled` | — | `no_lifecycle_transition_reports_this_category` |
| `dead` | `Dead` | `Destroyed` | `Destroyed` | `Destroyed` | — |
| `captured` | `Captured` | `Captured` | `Captured` | `OwnershipCaptured` | — |
| `escaped` | `Escaped` | `Escaped` | `Escaped` | — | `no_lifecycle_transition_reports_this_category` |
| `detached` | `Detached` | — | — | — | `detached_is_an_event_not_a_counted_category` |
| `despawned` | `Despawned` | `Despawned` | `Despawned` | `Despawned` | — |

Every cell is read out of the enum it names — `declared_kind()` maps the
distinction, `lower_kind` is the production lowering, `producer` is
`CountKind::producer` — so the table cannot go stale without a compile error.
`ContractDistinction::ALL` is exhaustive by construction and the function
asserts the table's length against it, so a seventh distinction cannot go
missing from the answer.

The `producer` column is one derived function rather than five written-out arms:
`CountKind::producer` is `CountKind::from_lifecycle` asked the other way over
`LifecycleKind::ALL`, because the runtime's *only* producer for a category is the
`TickInput::lifecycles` transition `from_lifecycle` maps to it
(`cs_sim::objectives::runtime::ObjectiveRuntime`). `None` there is therefore the
whole structural answer for that category: no transition can count it, and only a
caller that names the category itself can record it. A test asserts the two
directions agree for all five categories and all five transitions, so the
inverse cannot drift from the forward map.

### Where a detached actor is represented instead

The named answer, and each part is production code today:

* **`cs_sim::world_actors::release::release_payload`** — "detached payloads:
  inherit source motion, keep identity". `PayloadSpec::objective` is
  `Option<SymbolId>`, "the objective this payload counts for, if any; kept
  across release", and `ReleasedPayload` carries it through. That is F34
  non-negotiable 4 ("Detached payloads inherit source motion and keep objective
  identity if relevant") implemented, and it is the reason a detach is not a
  counter category: the payload **keeps** its objective instead of leaving
  accounting, so nothing ends for a counter to observe.
* **`cs_content::animation::AttachmentOp::Detach { pose }`** — the authored
  detach with its inherited velocity (F20-C `AC03`), and its runtime twin
  `cs_sim::animated_object::AttachmentOp::Detach`. The corpus models the same
  thing as a child node motion (`chutemanparent`, `chuteopen`), which is what
  this op reads.
* **`cs_sim::collision::Wreckage::Detached`** — "detached, tumbling wreckage":
  the physical state after the release.
* **`cs_sim::damage::LifecycleKind::MissionRemoved`** — the transition for an
  actor that leaves mission accounting. `CountKind::from_lifecycle` answers
  `None` for it (and for `PilotBailout`), so it is the one transition that is
  measured to count toward no category, and it is terminal
  (`LifecycleKind::is_terminal`), which is why it cannot stand in for the sixth.

### What this task deliberately did **not** do

* **No `Detached` category was added.** The task forbids a category with no
  measured or authored producer, and the acceptance criterion requires a producer
  the way `Disabled`/`Escaped` do not have one. A `Detached` would need a new
  `LifecycleKind`, and nothing measured says the original reports a lifecycle
  transition for a detach: what it reports is a completed objective.
* **F39-E4's gate was not moved or restated.** Whether an original *record* may
  declare one of the five counted categories is decided in one place,
  `cs_content::objectives::original_count_category_refusal`, from its own corpus
  census. This task adds the sixth distinction the census was never asked about
  and widens nothing.
* **The contract was not edited.** `docs/contracts/` is protected, and the
  measured finding does not ask for an edit: the contract requires six
  distinctions, the engine keeps six *condition* states, and the sixth
  distinction's counted-category half is answered by name instead of by
  invention.

## The contrary hypotheses, and what would settle each

1. **The original declares a detached count somewhere this census cannot see.**
   Not refuted, and much narrower than F39-E4's version of the hypothesis: this
   walk reads 62 of 62 reader archives, and it measures that the nine it did not
   previously cover carry no objective record at all, so the wider denominator is
   closed for this question. What remains outside it: (a) the **compiled mission
   program** behind each record, which is undecoded (F13-B/C, F38 own the
   instruction table) and which is where a category could be named without a
   spelling in the data; (b) a mission record shipped in an archive this
   product's reader rules do not classify as a reader archive; (c) a name spelled
   outside the twelve published stems. Settled by: the instruction table, or an
   original run.
2. **`MSG_OBJ_RELEASE` on `zbd/c4/m03`'s `cargozep1` **is** the sixth category.**
   This is the strongest contrary reading in the data and it was weighed
   seriously: one mission spells "release" on a cargo zeppelin. It is refused for
   the reason F39-E4's finding gives for the disable labels, and the census adds
   the structural fact: the block that would have to consume it declares no
   threshold, and the release-family names that appear beside thresholds appear
   nowhere. A localized label is not a counted transition, and one site is not a
   vocabulary. Settled by: an original run of `zbd/c4/m03`.
3. **A counted condition's part-state spelling *is* a detached count.** `healthy`,
   `panels` and `healthy_part` are part states of engines and gasbags, and
   `zbd/c5/m03` pairs 112 `healthy` stages with four `MSG_OBJ_DISABLE` targets and
   `snd_MN3Cargo1Disabled` — F39-E4's unknown #2, untouched here. Reading a name as
   a rule is the inference F39-D refused for `WAKE`/`NAP`/`KILL` and F39-E1
   refused for the dormant lifecycle. Settled by the same evidence as theirs.
4. **The sixth distinction is about a *pilot* leaving an airframe, not about
   cargo.** Plausible — `LifecycleKind::PilotBailout` exists and is measured to
   count toward no category, and a bailout is a detachment of sorts. But the
   contract lists `dead` and `despawned` beside it and the sheet's non-negotiable
   4 is about protected actors, and nothing measured connects a bailout to an
   objective category either. It stays unmeasured; F32-D's combat-failure rules
   own the pilot's fate.
5. **The original's detached vocabulary is spelled in a language this stage
   cannot read.** The installation ships localized `MSG_*` labels resolved
   through `strings.dll`, which F39-E4's sources note is never decoded here; the
   *keys* are English ASCII and measured, but a label's text is not. A category
   named only in a label's localized text would be invisible to this census.
   Partly refuted: `help_label`/`category_label` values are `MSG_*` ids, not
   prose, and the ids themselves are ASCII. Settled by: decoding `strings.dll`.

## Test inventory (`accept_f39_e7_*`)

`crates/cs_app/tests/accept_f39_e7_detached_condition_vocabulary.rs`:

| Test | Covers |
| --- | --- |
| `accept_f39_e7_the_six_distinctions_resolve_in_the_contract_s_own_order` | six rows, the contract's six spellings in its order, exhaustiveness against `ContractDistinction::ALL` |
| `accept_f39_e7_each_distinction_names_a_vocabulary_entry_and_a_producer_or_a_named_reason` | the table above: five categories reached through the production lowering, exactly three producers, the three named reasons, and each producer round-tripping through `from_lifecycle` |
| `accept_f39_e7_the_producer_column_is_the_inverse_of_the_only_counting_path` | `producer()` against `from_lifecycle` for all five categories and all five transitions; exactly two categories unreported; `PilotBailout`/`MissionRemoved` count toward none |
| `accept_f39_e7_a_detached_actor_is_represented_by_a_release_that_keeps_its_objective` | the named location, as behaviour: `anchor_sample` → `release_payload` keeps the objective, inherits the carrier's velocity plus the ejection, and `MissionRemoved` is terminal and counts toward nothing |
| `accept_f39_e7_the_spelling_rule_is_a_segment_match_over_a_published_stem_list` | every stem finds itself and is distinct and case-insensitive; `MSG_OBJ_RELEASE`/`DETACHMENT`/`drop_paratroopers`/`activate_dropoff_node`/`free_the_goose` match; `redetached`, `undroppable`, empty and the ten measured part-state spellings do not |
| `accept_f39_e7_the_reader_keeps_the_three_surfaces_apart` | a hand-built record: a stage beside a threshold is counted, one beside no threshold is a declaration, a sound group and a `WAKE_ANIM` are declarations, only the three labels are objective kinds, the surfaces are disjoint, the family is attributed per surface, and an absent `targets.zrd` is a measured absence |
| `accept_f39_e7_the_installation_spell_no_detached_category_on_either_counting_surface` (retail) | the whole table above over `$CS_GAME_DIR`: the fingerprint, both denominators, the three surfaces' counts, zero `DETACH` sites on every surface, the single `MSG_OBJ_RELEASE` objective kind by name, and the seven `DROP` block declarations including `zbd/c2/m05 OBJECTIVE23 drop_paratroopers` |

## Measured sensitivity (mutation probes, all observed on this branch)

Each probe was applied, `cargo test -p cs_app --test
accept_f39_e7_detached_condition_vocabulary -- --include-ignored` was run, and the
file was restored. "Fails" names the tests that reported `FAILED`.

| probe | fails |
| --- | --- |
| `ContractDistinction::declared_kind` gives `Detached` a declared category | `each_distinction_names_a_vocabulary_entry_and_a_producer_or_a_named_reason` |
| `ContractDistinction::unproduced_reason` returns `None` for `Detached` | `each_distinction_names_a_vocabulary_entry_and_a_producer_or_a_named_reason` |
| `contract_condition_distinctions` drops the `Detached` row | `six_distinctions_resolve_in_the_contract_s_own_order`, `each_distinction_names_a_vocabulary_entry_and_a_producer_or_a_named_reason` |
| `CountKind::from_lifecycle` maps `MissionRemoved` to `Disabled` | `the_producer_column_is_the_inverse_of_the_only_counting_path`, `each_distinction_names_a_vocabulary_entry_and_a_producer_or_a_named_reason`, `a_detached_actor_is_represented_by_a_release_that_keeps_its_objective` |
| `release_payload` drops `objective` | `a_detached_actor_is_represented_by_a_release_that_keeps_its_objective` |
| `detached_spelling_family` matches anywhere in the name instead of at a segment's start | `the_spelling_rule_is_a_segment_match_over_a_published_stem_list`, and the retail census's per-stem counts |
| the reader counts a stage that stands beside no threshold | `the_reader_keeps_the_three_surfaces_apart`, and the retail census's counted-condition total |
| the reader reads `nodes` as an objective kind | `the_reader_keeps_the_three_surfaces_apart`, and the retail census's objective-kind total |
| `CountKind::producer` hardcoded as a written-out match instead of the derived search | **nothing** — and that is the point: what is pinned is its agreement with `from_lifecycle`, not its implementation, so a refactor that keeps the agreement is not a regression |

The last row is stated because a reviewer should know it: the six distinctions
are pinned through derived queries, so the tests would not catch a rewrite that
computes the same answer. They do catch every change to the *answer*, which is
what the six-item reconciliation claims.


## Public-API changes worth naming

`cs_content::objectives` gains the F39-E7 reader and its types (additive).
`cs_sim::objectives::counters::CountKind` gains one method, `producer`
(additive). `cs_app::objectives` gains the F39-E7 section, the
`SCENARIO_TARGETS_MEMBER` import and one new struct field on no existing type —
`survey_retail_objective_records` and `survey_retail_dormant_reveal` are
untouched, so F39-D's, F39-E1's and F39-E5's committed measurements stay exactly
as published.

## Unknown / deferred (not guessed)

1. **What a counted condition means.** Unchanged and still carried by F39-E1:
   the shapes, names and thresholds are measured; the rule that turns a part
   state into a completed objective is not. This stage did not widen it.
2. **The wider denominator beyond the reader archives.** This stage closed
   F39-D unknown #5 *for the objective-declaration question*, and closed it by
   measurement rather than by scope: all nine shared/world-group readers were
   read, none declares an objective record, and one (`zbd/c1c/zrdr.zbd`) does
   declare objective kinds. It did **not** widen any other census's
   denominator, and task #597 still owns the shared/world-group corpus for
   F39-E4's five categories.
3. **Whether an original record may ever count a detached category.** Unmeasured
   and unmeasurable from the files, because nothing in them declares one. If the
   compiled program names one, F39-E4's gate is where that lands: the census
   would find the spelling, and the gate would open **by measurement**.
4. **`LifecycleKind` has no transition for a detach.** Named as the structural
   reason, not filled in. Inventing a sixth transition would be a new unmeasured
   damage/event vocabulary, which is a separate, larger change than this stage.
5. **The census reads no opcode and no compiled program**, so nothing here is
   evidence of how the original behaves, only of what its files declare.
6. **The byte-level `detach` grep is corroboration, not a measurement**, and it
   is byte-level: a name spelled in a non-ASCII encoding would not be found.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f39_e7_ --include-ignored
cargo test --locked -p cs_app --test evidence_report_f39_e7 -- --ignored
python3 tools/validate_evidence.py private/evidence/F39-E7/acceptance.json \
  --artifact-root private/evidence/F39-E7 --require-pass
```

## Sources

`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
(non-negotiable 2), `docs/contracts/SCRIPT-MISSION.md` ("Objective event
ordering"), `docs/contracts/CLI-EVIDENCE.md`, the F39-E4 finding
(`docs/findings/2026-10-04-f39-e4-count-category-producers.md`, whose census
this stage reads beside rather than widens), the F39-E1 and F39-D findings
(unknowns #4, #5 and #7), F20-C's `AC03` and F34's non-negotiable 4 (both
shipped), the F13-B/C findings (the mission-scope rule and the undecoded
program), F12's string-catalog findings (a `MSG_*` label resolves through
`strings.dll`, never decoded here), F32-D's combat-failure findings (a pilot
bailout is not a kill), and the read-only `$CS_GAME_DIR` listing. No web source
was consulted and no original executable was run.
