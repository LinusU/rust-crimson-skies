# F10-C.04: which texture archive a GameZ container's materials bind to

**Task:** Rally **#387** (`F10-C.04`), slice of **#43** (`F10-C`), the
"which archive" half of stage F10-C's material/texture dependency audit. Sheet:
`specs/F10-gamez-mesh-topology-and-material-records.md`, section `### F10-C`.
Contract: `docs/contracts/IDENTITY-CONTENT.md`. Test prefix: `accept_f10_c_04_`.
Capabilities used: ordinary build/test, and **`retail`** for the two
`#[ignore = "requires CS_GAME_DIR"]` tests. **No original game was run**: every
class in this document is `Inferred`, `Unknown` or `ObservedTool`, and none is
`verified_original`.

Read first: **#365**'s
`docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` (the material
section, the texture-name encoding, the exact-name rule and its deferred items
1–4), **#366**'s
`docs/findings/2026-09-29-f10-c-03-mesh-container-catalog-and-upload.md` (the
container-to-upload path and why it stops at the content boundary), and
`docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md` (the
`TextureCatalog` / `TextureArchive` side).

## The question, and why it was open

F10-C.02 measured a GameZ material's texture dependency exactly: the material
record stores a `u32` at offset 16, that `u32` indexes the container's own
texture-name table, and the name that table holds is looked up **by byte
equality** in exactly one archive — the one the audit's caller named. On the
installation, **no** GameZ material in any of the nine containers resolves:
0 of 3 543 world rows and 0 of 935 airframe rows.

The audit was built that way on purpose. `DependencyContext` takes the archive
from its caller and never searches, because "which texture archive a mission or
a world uses is not established" (F08-C's own open question). That left a
question with no answer anywhere in the tree:

> **Which archive does a GameZ container's materials bind to, and does the
> lookup succeed at all?**

`ZBD/planes.zbd` is the awkward case the task was filed for. It is mounted at
the installation root, shared by every world, and has no world of its own — so
its binding is a genuinely separate question from a world's. The audit's answer
was "whatever the caller passed", and the caller had nothing to pass.

## What was built, and what it deliberately does not do

`measure_bindings` (`crates/cs_content/src/mesh.rs`) measures the question. It
takes a container's material facts and a list of candidate archives and reports,
per candidate and per name reading, how many of the container's distinct stored
texture names that archive holds — then reports the **weakest decision those
numbers support**.

Two things it does not do, and both are the point:

* **It does not pick an archive for the audit.** `MeshDependencyAudit` is
  untouched. It still resolves by byte equality against the one archive its
  caller named, no case folding, no extension stripping, no alias, no fallback.
  A reading that reaches more names is *reported* as a measurement, never
  *applied* as a rule.
* **It does not resolve a tie it cannot settle.** A tie over **one** stored name
  set — several files holding one answer — is reported as a named binding. A tie
  **across** name sets is reported as naming nothing.

`TextureNameRule` separates the audit's rule from the four readings that only
measure:

| Reading | What it does to the container's stored name |
| --- | --- |
| `Exact` | nothing — this is the rule `MeshDependencyAudit` uses |
| `LastSuffixDropped` | cut at the **last** `.`: `Sky1.tif` → `sky1`, `buildingspotlighted.` → `buildingspotlighted` |
| `LastSuffixCaseFolded` | the above, ASCII lower case |
| `FirstDotDropped` | cut at the **first** `.`: differs from the last-dot reading on a name with a dot inside it |
| `FirstDotCaseFolded` | the above, ASCII lower case |

The two dot readings are kept apart because the corpus separates them:
`bldhwk_cowling..tif` (36 stored occurrences in `planes.zbd`, the container's
double-dot shape from F10-C.02) keeps a trailing dot under a last-suffix reading
and loses it under a first-dot one. ASCII-only lower casing is deliberate: a
stored name is bytes, and a Unicode case mapping would be a claim about a text
pipeline the format does not have. The **archive** side of the comparison is
never projected — what a texture package stores is what the archive stores.

### A candidate is a container, not a key

`TextureCandidate` carries the archive's **installation-relative path** beside
its `AssetKey`. This is not cosmetic. Every world group mounts its own
`texture.zbd` at the *same* key (`world/default/texture.zbd`); the key says
nothing about which world answered, the session's `ResolveContext` does. A
measurement identifying candidates by key alone cannot tell eight worlds'
archives apart, and would report a tie over eight different name sets as a tie
over one. The first draft of this task did exactly that, and the retail test
caught it.

## The measured state, per container

Nine GameZ containers, **49** texture archives, read through
`install::discover` → `SessionBuilder::mount_installation` →
`TextureCatalog` → `archive_names`, and through `read_gamez_meshes` +
`read_gamez_materials` for the containers. Everything below is a per-reading
count of **distinct stored names** the archive holds, produced by production
code.

The candidate set is the **49 archives the installation actually holds**, and
that is worth stating because getting it wrong is the error this section
supersedes. Each world group carries **six** texture archives: a primary
`texture.zbd` and **five** `rtexture*` resolution tiers. Four tier numbers
(`rtexture2`, `rtexture4`, `rtexture6`, `rtexture8`) are the same in every
group, but the fifth differs per world — `rtexture9` in `C2B`, `rtexture10` in
`C1C`, `rtexture11` in `C1B`, `rtexture12` in `C3`, and `rtexture14` or
`rtexture15` elsewhere. Naming only the four common ones yields 41 archives and
silently drops one real tier from every world. The tests enumerate each group's
`.zbd` members from the production inventory and let the catalog decide which of
them is a texture package, so no archive is left out by name; a member that is
not a texture package is a reported failure, and the test asserts that failures
and archives add up to the members offered. `crates/cs_formats/tests/texture/retail.rs`'s
`accept_f08_b_retail_zbd_texture_packages_read_and_decode_with_the_recorded_census`
independently records the same census of 49.

The measurements below do not depend on the tier numbering: the tiers of one
group hold an identical name set, so including the fifth changes no count and
only widens the tie by one file. The conclusions are the same either way. The
correction matters because a candidate set that is quietly missing a fifth of
each world's archives is a measurement of the wrong question, and the numbers
below should be reproducible from the installation rather than from a list
someone typed.

### The material-row census, every row counted exactly once

| Container | stored references | material rows | `textured` | `untextured` | other states | distinct names |
| --- | --- | --- | --- | --- | --- | --- |
| `ZBD/planes.zbd` | 19 810 | 935 | 926 | 9 | 0 | 221 |
| `ZBD/C1/gamez.zbd` | 22 678 | 561 | 552 | 9 | 0 | 551 |
| `ZBD/C1B/gamez.zbd` | 11 169 | 314 | 310 | 4 | 0 | 309 |
| `ZBD/C1C/gamez.zbd` | 10 173 | 279 | 275 | 4 | 0 | 274 |
| `ZBD/C2/gamez.zbd` | 16 202 | 488 | 480 | 8 | 0 | 479 |
| `ZBD/C2B/gamez.zbd` | 9 336 | 271 | 267 | 4 | 0 | 266 |
| `ZBD/C3/gamez.zbd` | 20 506 | 441 | 434 | 7 | 0 | 423 |
| `ZBD/C4/gamez.zbd` | 24 381 | 627 | 618 | 9 | 0 | 605 |
| `ZBD/C5/gamez.zbd` | 28 396 | 562 | 555 | 7 | 0 | 538 |
| **total** | **162 651** | **4 478** | **4 417** | **61** | **0** | **3 666** |

Three things are worth stating plainly, because they are the shape of the
answer and not incidental:

* **Zero rows in all nine containers are out of range.** No stored material
  index is past its container's table, no textured record names a texture index
  the container lacks, and no material carries a flag bit outside the pinned
  reference's `MaterialFlags`. Every row is either a complete flat colour or a
  row that names a stored texture. The census is a partition: rows counted = rows
  referenced.
* **Rows ≠ names.** `C1` has 552 textured rows and 551 distinct names; several
  materials point at one texture-table entry. A measurement that conflated the
  two would report the same coverage for containers with and without shared
  materials, so `ArchiveCoverage` carries both `names` and `rows` per reading.
* **`planes.zbd` is the outlier in names-per-row**: 926 textured rows over just
  221 distinct names, four materials per name on average. The airframe library
  reuses skins heavily; the world containers are close to one row per name.

### Under the exact rule: nothing binds, anywhere

| Container | distinct names | held by any of the 49 archives, `Exact` |
| --- | --- | --- |
| all nine except `C2`, `C3` | 221–605 | **0** |
| `ZBD/C2/gamez.zbd` | 479 | **5** |
| `ZBD/C3/gamez.zbd` | 423 | **5** |

The ten exact matches are `C2`'s and `C3`'s five extension-less lower-case
names each (`lflare1`, `lightmap`, …) — and each pair is the *same* five names,
so over the union of the world's name sets there are **5**, not 10. This is
F10-C.02's deferred item 1 restated through the binding lens, and it is the
honest state of the contract's rule on this corpus: **the exact-name lookup the
contract specifies resolves 10 of 4 478 material rows.**

`ZBD/C2` and `ZBD/C3` therefore do not get a `Unique` decision under `Exact` —
they get a 48-way tie at 5 of their names, over **8** name sets, which is
`Unknown`. A container that reaches 5 names out of 479 is not bound to
anything; a measurement that named a leader there would be reading noise as a
signal. (The 48 are every world archive; the install-root `rimage.zbd` reaches 0
and is the reported runner-up.)

### Under a reading that drops the extension and folds case: each world binds to its own group

Every world container's **own** group leads, by a wide margin, and that group's
six archives are exactly tied because they store **one identical name set**
(measured: all six archives of a world group hold the same names, so
`name_sets == 1`).

| Container | own group holds (`FirstDotCaseFolded`) | next best archive | unreconciled names |
| --- | --- | --- | --- |
| `ZBD/C1` | **549** of 551 | `C2` at 323 | `snow16x16.tif`, `pir_spinner.tif` |
| `ZBD/C1B` | **307** of 309 | `C1` at 274 | `snow16x16.tif`, `pir_spinner.tif` |
| `ZBD/C1C` | **272** of 274 | `C5` at 262 | `snow16x16.tif`, `pir_spinner.tif` |
| `ZBD/C2` | **477** of 479 | `C1` at 321 | `snow16x16.tif`, `pir_spinner.tif` |
| `ZBD/C2B` | **264** of 266 | `C2` at 238 | `snow16x16.tif`, `pir_spinner.tif` |
| `ZBD/C3` | **419** of 423 | `C1` at 334 | `snow16x16.tif`, `pir_spinner.tif` |
| `ZBD/C4` | **602** of 605 | `C1` at 365 | `snow16x16.tif`, `pir_spinner.tif`, `canopycorner.tif` |
| `ZBD/C5` | **535** of 538 | `C2` at 328 | `snow16x16.tif`, `pir_spinner.tif`, `barngrill.tif` |

**A world's GameZ container binds to its own world group's texture archives.**
The margin is large everywhere: `C4` holds 602 names and the next best archive
anywhere in the installation holds 365; `C2` 477 against 321. The decision is a
`Tied` with `name_sets == 1` over the group's six archives, and its evidence
class is **`ClaimStatus::Inferred`** — reasoned from a measurement, not observed
from a run.

`unreconciled` in that table is the count of names **no** archive in the
installation holds under **any** reading, which is not the number the leading
reading misses. `ZBD/C3` is the case that shows the difference: its own group
holds 419 of 423, so four names go unreached by the loosest reading, but two of
those are reached by a different reading, and only **two** are absent from the
whole installation. `ContainerBinding::unreconciled()` reports the second
number, and the difference between the two is a fact about the readings rather
than about the corpus.

Note what the `name_sets == 1` distinction buys. The six archives of a world
store identical names, so a decision that could not tell a tie over one name set
from a tie between answers would have to answer "we cannot say" for every world,
losing the finding. A tie over one set is several files over one answer, and it
is named as such.

### `ZBD/planes.zbd`: no archive is named, and that is the finding

| Reading | best count | candidates tied | name sets | class |
| --- | --- | --- | --- | --- |
| `Exact` | **0** | — | — | `Unknown` |
| `LastSuffixDropped` | 217 of 221 | 36 | 6 | `Unknown` |
| `LastSuffixCaseFolded` | 218 of 221 | 36 | 6 | `Unknown` |
| `FirstDotDropped` | 218 of 221 | 36 | 6 | `Unknown` |
| `FirstDotCaseFolded` | **219** of 221 | 36 | **6** | `Unknown` |

The airframe container's 221 names are held, at best, by **36 of the 49**
archives — six world groups' worth, all six of each — and the best archive
outside those six reaches **218**, one name behind. `rimage.zbd`, the one
archive at the installation root beside `planes.zbd`, holds **0 of 221 under
every reading**, so no *airframe* material binds to the UI set. That one fact
is settled, and it is settled by exclusion. It is a statement about the
airframe container's 221 names; the world's containers are not searched against
the UI set in that measurement, so nothing is claimed here about them.

So the honest answer to the task's question is:

> **The airframe materials bind to no single catalogued archive, and this
> measurement cannot say which. Six world groups' archive sets are equally good
> candidates, separated by a single name out of 221.**

This is `ClaimStatus::Unknown`, and it is **not** a gap in the measurement — it
is the measurement's answer. A world group's airframes are drawn while that world
is loaded, so the binding is a *runtime* fact about which world is active, not a
property of `planes.zbd`. Deciding it needs the original engine's behaviour
(an original-run capture) or an independent reference; it cannot be read out of
the container.

Two names in `planes.zbd` are held by **nothing** in the installation under any
reading: `pea_shadow.tif` and `canopycorner.tif`. The first-dot reading reaches
`bldhwk_cowling..tif` (the double-dot shape) where the last-suffix reading does
not, which is why the two dot readings are separate and why the first-dot one
scores 219 rather than 218.

## Answering the task's acceptance criteria

1. **A finding stating which archive each GameZ container's materials bind to,
   with the evidence class it has.** This document: § "Under a reading that
   drops the extension and folds case" (`Inferred`, per world group) and §
   "`ZBD/planes.zbd`" (`Unknown`). Eight of nine containers are named; the
   airframe container is not, with the reason measured rather than assumed.
2. **Any alias records added carry scope and test coverage.** **None were
   added.** The IDENTITY-CONTENT lookup contract permits "evidence-backed
   aliases … with scope and test coverage", and the bar is an *independent
   reference or an original-run capture*. The measurement shows a relaxed
   reading would reach all but 10 of the 3 666 distinct names (the two
   per-world names, `canopycorner.tif` or `barngrill.tif`, and
   `pea_shadow.tif`), which is exactly the kind of number that must not be
   produced by relaxing a rule. The readings are measurement instruments, each
   with a test that fails when its projection changes, and none of them is wired
   into a lookup.
3. **Every GameZ material row's state reported exactly as measured.** The census
   table above: 4 478 rows over 162 651 stored references, 4 417 textured, 61
   untextured, 0 in any other state, for all nine containers, through
   `ContainerMaterials::new`. Names no archive holds are named per container in
   the `unreconciled` column and reachable through
   `ContainerBinding::unreconciled()`.
4. **The report names which claims are still gated.** See below.

## Claims still gated

| # | Gated claim | Why it is gated | Resolving path |
| --- | --- | --- | --- |
| 1 | Any claim that a GameZ material is **bound** to a texture | The exact rule the contract specifies resolves 10 of 4 478 rows. The relaxed readings are measurements, not rules, and no alias is admitted without independent evidence | the **owner** — the engine's actual name-matching rule is a claim about the original engine; then an alias record with scope and tests |
| 2 | Any claim that an **airframe** material is bound to a texture | 36 archives tie at 217–219 of 221 names over 6 name sets, margin 1. The binding is a runtime fact about which world is loaded | an original-run capture (**owner**, `human_play` / `human_review`), or a consumer that knows the active world and passes it in — the latter is a *runtime* answer, not a static binding, and must be recorded as such |
| 3 | Any render-mesh fidelity claim | The three `MeshPresentationUnknown` codes (winding, UV origin, vertex colour) are open on every row, and F10-C.03 reports no row as `Ready` | **F17-B** |
| 4 | That the two dot readings reflect the engine's own name parsing | The corpus distinguishes them on one name (`bldhwk_cowling..tif`) and the reading that wins is a *measurement* of which spelling the archive happens to store | the same evidence as #1 |
| 5 | `snow16x16.tif` and `pir_spinner.tif` (every world), `canopycorner.tif` (`C4`), `barngrill.tif` (`C5`), `pea_shadow.tif` (`planes`) | Absent from the whole installation under every reading this code may try. Whether the original engine found them in a built-in resource is not established | **F10-D** (#46) on the private corpus; the owner for built-in resources |
| 6 | Every `verified_original` / `release_approved` claim | No original game was run. `retail` is file access, not evidence of runtime behaviour. Every class in this document is `Inferred`, `Unknown` or `ObservedTool` | the **owner** |

Items 1, 2, 4 and 6 are the same underlying gap seen from four sides, and they
are deliberately **not** resolved by a measurement. The owner directive for this
task is explicit: do not relax the exact-name rule to make a number smaller, and
if the answer turns out to be "the original engine also matched these loosely",
that is a finding about the original engine that needs an independent reference
or an original-run capture — not an inference from a count.

## Files, and the one observable failure

The gating is unchanged by the review, except that items 1 and 2 now rest on
measurements over the installation's full archive set rather than a subset of
it, and item 5's counts are asserted per container.

- `crates/cs_content/src/mesh.rs` (owner path): `TextureNameRule`, `RuleCounts`,
  `TextureCandidate`, `StoredTextureName`, `ContainerMaterials`,
  `TEXTURED_ROW_STATE`, `ArchiveCoverage`, `RunnerUp`, `TextureBinding`,
  `ContainerBinding`, `archive_names`, `measure_bindings`, `measure_one`,
  `decide`, `cut_at`, `ascii_lower`, the module doc section, the sixteen
  `accept_f10_c_04_` tests, and the F10-C.01/02/03 test and production code
  unchanged.
- `crates/cs_content/src/lib.rs` (wiring, allowed by AGENTS rule 1): one
  paragraph in the crate doc naming the new entry point. No logic.
- `docs/findings/2026-09-29-f10-c-04-gamez-texture-archive-binding.md` (this
  file).

**One observable failure:** identifying a candidate by its `AssetKey` instead of
its container path. Every world group mounts `texture.zbd` at the same key, so
`decide`'s name-set count collapses six different world name sets onto one and
reports the airframe's 36-way, 6-set tie as a 1-set tie — i.e. it claims the
airframes bind to a named answer. `accept_f10_c_04_retail_planes_binds_to_no_catalogued_archive`
fails on `name_sets > 1`. This is not hypothetical: the first draft of this task
did exactly this, and the two retail tests found it.

## Test inventory (`accept_f10_c_04_*`)

All sixteen in `crates/cs_content/src/mesh.rs`, all calling production code.

| Test | What it pins |
| --- | --- |
| `..._every_material_row_is_counted_exactly_once` | the census partitions the rows: one textured, one untextured, one `texture_index_out_of_range`, one `material_index_out_of_range`; and the walk reaches both stored reference levels (6 references, 4 distinct rows) with the reader's own count as the cross-check |
| `..._several_rows_naming_one_texture_are_counted_once_per_name` | rows and names are different numbers, and `ArchiveCoverage` reports both per reading |
| `..._the_exact_rule_reaches_nothing_and_decides_nothing` | the corpus naming difference on a fixture, **and** that the audit still resolves nothing: `Sky1.tif` against an archive storing `sky1` stays `missing_texture` |
| `..._a_tie_over_one_name_set_still_names_the_binding` | the world's tiers: both containers named, `name_sets == 1`, `Inferred` — and the two are named by path, with a comment that the keys are equal |
| `..._a_tie_across_name_sets_names_nothing` | the airframe shape: both reported, neither picked, `name_sets == 2`, `Unknown` |
| `..._one_strict_leader_is_named_with_its_margin` | the `Unique` decision carries the runner-up's own numbers |
| `..._a_container_with_no_textured_row_is_uncovered_not_empty` | all five readings report `Uncovered` for a flat-colour-only container, rather than a vacuous success |
| `..._the_readings_differ_only_where_the_corpus_differs` | the four name shapes: the double dot, no dot, the exact rule byte-preserving, and the archive's side never projected |
| `..._a_name_no_candidate_holds_is_reported_not_dropped` | `unreconciled()` names the absent textures in the container's order, and a reached name is not among them |
| `..._the_measurement_is_a_function_of_the_bytes` | two runs equal; the named leader does not depend on the caller's candidate order; one changed name changes the coverage |
| `..._an_empty_candidate_set_is_reported_not_indexed` | a caller with no archive in hand measures nothing and is told so: `Uncovered { candidates: 0 }` under all five readings, the names reported unreconciled, the census unaffected — a report, not an index panic |
| `..._a_repeated_container_spelling_does_not_collapse_a_tie` | two candidates under one spelling, two name sets: the tied set's name-set count is 2 and the decision `Unknown`. The count is taken by position, so a repeated label cannot read one name set twice and turn a tie between answers into a named binding |
| `..._retail_planes_binds_to_no_catalogued_archive` *(retail)* | the airframe measurement against **every** archive the installation holds: the census (926 textured of 935 rows, 9 untextured, 221 distinct names, 19 810 stored references), 49 candidates, `Exact` reaches 0 for all of them, `rimage.zbd` reaches 0 under **every** reading, and the loosest reading is a 36-way tie over 6 name sets at 219 of 221 with the best outsider at 218, classed `Unknown`. The two absent airframe names are named exactly |
| `..._retail_each_world_binds_to_its_own_group` *(retail)* | a world's container against its own six archives plus a sibling world's six: `name_sets == 1` over 549 of 551 names, all six named and all in `C1`, the sibling excluded by coverage, and the reader's own reference count agreeing |
| `..._retail_every_container_row_is_census_ed_once` *(retail)* | the census table of this document, container by container: stored references, distinct material rows, textured, untextured and distinct names for all nine containers, each row counted under exactly one state, totalling 162 651 references and 4 478 rows. It needs no mount and no archive, so the container's own half cannot drift with the archive set |
| `..._retail_each_container_is_measured_against_every_archive` *(retail)* | the per-world table above, over all 49 archives: each world's own group leads on 6 tied archives over 1 name set at the recorded count, the runner-up is the recorded count, and `Exact` is `Uncovered` everywhere except `C2` and `C3`, which get an 8-name-set tie at 5 names and stay `Unknown` |

### Sensitivity probes actually run

Each probe is a single textual change to production code, applied and reverted,
run against `cargo test -p cs_content --lib -- accept_f10_c_04_` (10 non-retail
tests). The counts below are what the runs reported.

| Probe | Result |
| --- | --- |
| untextured rows dropped from the census | **2 failed** |
| a tie's name-set count faked, and the tied set reordered into ranked order | **2 failed** |
| the first-dot readings collapse into last-suffix | **1 failed** |
| covered rows counted per name instead of per row | **1 failed** |
| only the mesh-record reference level walked | **1 failed** |
| the exact reading case-folds the stored name | **1 failed** |
| a tie resolved by picking the first candidate | **2 failed** |
| an unreachable name dropped instead of reported | **2 failed** |
| a tie always reported as one name set | **1 failed** |
| an unnamed binding claims `Inferred` | **1 failed** |
| every candidate offered as the binding | **2 failed** |

Two probes found real gaps in the first draft, both fixed here:

* **"only the mesh-record reference level walked" survived.** The census test
  used a fixture whose two levels reached the *same* material indices, so a walk
  that read only the mesh's 12-byte reference list reported the same rows. The
  fixture now has materials reachable from the polygon level only, and asserts
  the reader's own `unchecked_material_references` as the cross-check. The
  lesson is F10-B's, restated: a field the corpus does not vary must be covered
  by a fixture or not covered at all.
* **"a tie's name-set count faked" survived nothing, but the reordering probe
  did** — the tied set was being taken in ranked order, so a caller listing its
  archives differently got a different list. It is now taken in the caller's
  order, and `..._the_measurement_is_a_function_of_the_bytes` pins it.

### Review probes

Three more, run by the reviewer against the rebased branch. Each is a single
textual change, applied and reverted.

| Probe | Result |
| --- | --- |
| `decide`'s name-set count taken by a **name lookup** into the candidate list instead of by position | **1 failed** — `..._a_repeated_container_spelling_does_not_collapse_a_tie` reads one name set twice, reports `name_sets == 1` and claims a named binding over two different answers |
| the empty-candidate guard removed from `decide` | **1 failed** — `..._an_empty_candidate_set_is_reported_not_indexed` panics indexing the empty `ranked` list |
| the candidate enumeration narrowed back to the four tiers every group shares (`rtexture2/4/6/8`) | **3 failed** — all three retail tests, on the archive census (40 against 49) and on the world's own archive count (10 against 12) |

The third probe is the one that matters. It is the state this finding was
originally written from: a measurement that silently omitted one real
resolution tier from every world group. It passed the assertions written beside
it, because those assertions had been written from the same incomplete list.
The measurements it changes are the tie *widths* (36 candidates rather than 30,
6 archives per world rather than 5) and the archive census; the conclusions do
not move, because the omitted tier holds a name set identical to the four it
was left out of. The candidate enumeration is now derived from the production
inventory rather than named, and the census is asserted, so the omission cannot
come back unnoticed.

That is the general lesson, and it is the same one F10-B's teaches: **a
measurement's search space is part of its evidence.** A count taken over an
arbitrary subset of the candidates answers a different question, and nothing in
the numbers says which subset was used.

## Recorded unknowns

- **Whether the original engine matched a texture name with or without its
  extension, and with or without case.** Unchanged from F10-C.02 deferred item 1
  and not re-decided here. The measurement shows the corpus reconciles; it does
  not show the engine did.
- **Which archive a GameZ material binds to at runtime**, and whether that is a
  per-container property at all. For `planes.zbd` the measurement says the
  question cannot be answered statically. If a consumer resolves it from the
  active world group, that answer is a *runtime* fact and must be recorded with
  the session generation that produced it — not baked into a container record.
- **Which tier of a world group a mission loads**, and when the choice is made.
  All five tiers and the primary of a group hold identical names (measured), so
  the choice is invisible to a name lookup. F10-C.02 deferred item 4, unchanged.
- **The two names absent from the whole installation** (`pea_shadow.tif`,
  `canopycorner.tif` for the airframes; plus the world's per-container two or
  three) and whether a built-in resource supplies them.
- **Original-run behaviour of any kind.** No original game was run for this
  task.

## Sources

- The original installation at `$CS_GAME_DIR`, read-only, through
  `install::discover`, `SessionBuilder::mount_installation`, `TextureCatalog`,
  `archive_names`, `read_gamez_meshes` and `read_gamez_materials`. Every number
  in this document comes from production code over those files, and all of them
  are asserted by the four `accept_f10_c_04_retail_` tests: the census table by
  `..._retail_every_container_row_is_census_ed_once`, the per-world coverage
  table by `..._retail_each_container_is_measured_against_every_archive`, the
  airframe measurement by `..._retail_planes_binds_to_no_catalogued_archive`, and
  the archive census of 49 by both of the last two. The candidate archives are
  enumerated from the production inventory, so the search space is the
  installation's, not a list.
- `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` — the material
  worksheet, the texture-name encoding, the exact-name rule, deferred items 1–4.
- `docs/findings/2026-09-29-f10-c-03-mesh-container-catalog-and-upload.md` — the
  container-to-upload path and the presentation unknowns.
- `docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md` — the
  `TextureCatalog` / `TextureArchive` API this measurement reads.
- `docs/contracts/IDENTITY-CONTENT.md` — the lookup contract, the catalog
  element fields, and the `EvidenceClass` vocabulary used for
  `TextureBinding::evidence()`.

No code was copied from any reference; mech3ax is EUPL-1.2 and was read as a
reference only in earlier slices. No original game data is committed, and the
retail tests read `$CS_GAME_DIR` only.
