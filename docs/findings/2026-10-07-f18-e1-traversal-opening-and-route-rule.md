# F18-E.1: the measured traversal-opening and route rule, and what it locates in the F18 world audit

Date: 2026-10-07. Task: #732 (`F18-E.1`), the follow-up #436 (`F18-E`)
filed against its own limitation. Feature sheet:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md` (AC04's
traversal half; the deliverable line "preserve tunnels, arches, building
openings, hangars and stunt passages"). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`.

## Provenance and method

Capabilities: `retail` (read-only `$CS_GAME_DIR`) and ordinary build/test. No
`gpu` was needed — nothing is rendered by this rule. **No original run happened,
and nothing here is `verified_original`.** Every input is a file measurement or
a static analysis whose class is `observed_tool`.

This task measured **no new bytes of its own**. What it did was evaluate the
candidate evidence in #732's description, find that the measurement it needed
already existed in two merged tasks, bind them together, and record precisely
what remains absent. The two halves:

1. **a located class.** F42-D (task #463,
   `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`) measured
   that an instant-action scenario's `targets.zrd` declares a fly-through
   danger-zone objective selected by its own `category_label`/`help_label` pair
   (`MSG_OBJ_DZ`/`MSG_OBJ_FLYTHROUGH`), that `ia.zrd`'s `dzones` binds that
   objective's label to a world node, and that **all 54** such targets resolve to
   a `dzpath<N>` node whose stored box task #427 measured
   (`docs/findings/2026-10-02-t427-retail-trigger-volume-thickness.md`).
2. **an absent rule for everything else.** Three measurements say the corpus
   holds no opening of the other four classes and no route at all: what
   `read_gamez_nodes` decodes per record (name, flags, mesh binding, hierarchy
   slot, area partition, three stored boxes — no field states an opening and no
   field is a path); what the owner-supplied decrypted image does with a world
   record's name (its **only** name-keyed consumer of one is the four-byte
   `fvol` prefix, `strncmp` at VA `0x44e087`, tasks #716/#727,
   `docs/findings/2026-10-07-f18-grid-collision-origin.md`); and that the
   corpus's only route carrier is a mission reader's `aiv.zrd`, present in every
   mission of every mission type (F31-D) whose **encoding is unmeasured** —
   task #455, `F31-ROUTE-ENCODING`.

## The rule

**A `StuntPassage` is located where, and only where, four measurements meet:**

| step | measurement | source |
| --- | --- | --- |
| 1 | the scenario's `targets.zrd` declares a fly-through danger-zone objective, selected by `category_label = MSG_OBJ_DZ` and `help_label = MSG_OBJ_FLYTHROUGH` | F42-D / #463 |
| 2 | `ia.zrd`'s `dzones` binds the objective's label to a world node (the measured label direction) | F42-D / #463 |
| 3 | that node's stored box and mesh binding | #427's box survey, read by the production node reader |
| 4 | clearance = the box's **narrowest stored extent** × the group's own `vertex_scale_to_m` (the measured metre, #677/#436, carried by the F18-E census) | this task |

Nothing in the chain matches a node name to a class. An authored label such as
`sghangar` is a *scenario-local* label that binds a zone; it is not a statement
that a hangar opening exists, so `Hangar` stays unlocated — which is exactly the
guess #732's description and AGENTS rule 4 forbid.

The four other classes are reported **absent from the corpus with that
measurement**, and no route is stated with its own. Both statements are
`pub const` strings in `cs_app::world::audit`
(`OPENING_CLASS_ABSENT_FROM_THE_CORPUS`, `ROUTE_ABSENT_FROM_THE_CORPUS`) so a
test asserts against the same text the code emits rather than a re-spelling.

## What the rule locates, over the owner's installation

54 openings of class `StuntPassage`, one per fly-through danger-zone target,
across six of the eight groups — the same 54 F42-D measured, reached here
through the production classification:

| group | `c1` | `c1b` | `c1c` | `c2` | `c2b` | `c3` | `c4` | `c5` |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| located `StuntPassage` | 5 | 5 | **0** | 9 | **0** | 4 | 14 | 17 |
| unlocated classes | 5 | 5 | 5 | 5 | 5 | 4 | 4 | 4 |

`c1c` and `c2b` are the two groups whose instant-action scenario is
`zeppelin_run` and declares **0** fly-through targets; their `StuntPassage` row
carries the measured reason naming their own scenario container
(`zbd/c1c/ia1/zrdr.zbd`, `zbd/c2b/ia1/zrdr.zbd`) rather than a bare zero.

**No `TraversalRoute` is stated in any group.** The world container stores no
path, and the corpus's only route carrier (`aiv.zrd`) has an unmeasured encoding
(task #455). The audit says so as
`WorldAuditGap::NoRouteInMeasuredCorpus { world, located_openings, affected,
measured }`, once per group, with `affected` = that group's missions.

## What changed

* `crates/cs_content/src/world.rs`
  * `UnlocatedOpening { class, measured }` — one unlocated class plus the
    measurement behind it; `WorldAuditError::BlankOpeningReason` refuses an
    empty one at both the constructor and the census boundary, so
    "unlocated and silent" is not a representable state.
  * `OPENING_SEARCH_UNSUPPLIED` — the reason a class carries when the census
    carries no measured opening search at all, so a caller that searched for
    nothing still gets a reason instead of nothing.
  * `RouteSearch { Unstated { measured }, Unsought { measured } }` on the census.
  * `WorldAuditGap::NoRouteInMeasuredCorpus` — the successor to
    `NoRouteMeasured` for every group a measured rule reached;
    `NoRouteMeasured` itself is unchanged and now only fires for a census that
    was **never** searched (the shortfall safety net), which is what keeps the
    synthetic arm honest.
  * `census_verdict` consults both, and every `StuntOpeningAudit` it produces
    carries one measured reason per unlocated class.
* `crates/cs_app/src/world/audit.rs`
  * `locate_stunt_passages(world, stunts, vertex_scale_to_m)` — the
    classification above, public so a caller can check it against its own
    evidence; `traversal_evidence` builds the per-class reasons and the
    `RouteSearch::Unstated` verdict from it.
  * `class_absent`, `route_absent`, `stunt_passage_absent`,
    `stunt_passage_unmeasured` produce the cited reason texts.
  * `survey_world_groups` runs `cs_app::stunts::survey_retail_stunt_encoding`
    **once** for the corpus and keeps its failure as a message (every affected
    row then quotes it) rather than aborting the world survey. Cost: two extra
    production discoveries per world survey, so the retail F18-D audit went from
    about 40 s to about 70 s on this host — still well inside the sibling retail
    suites (286–392 s).
* `crates/cs_app/src/world/mod.rs` — wiring-only re-exports (AGENTS rule 1).
* `crates/cs_app/tests/world/{audit.rs,audit/evidence.rs}` — the re-pinned
  verdicts and the two `accept_f18_e1_` tests.

`vertex_scale_to_m` stays `Some(1.0)` everywhere; no field of any record here
was renamed, reinterpreted or dropped.

## Acceptance criteria, one by one

| criterion | state |
| --- | --- |
| every `OpeningClass` located with a measured clearance, or unlocated **with the measured reason**, never a silent zero | **met.** `StuntPassage` located in six groups with `Some(clearance_m)`; all other rows carry a non-empty `measured`, refused if blank (`BlankOpeningReason`) |
| at least one real `TraversalRoute`/`StuntOpening` from measured evidence, or the finding names precisely why the corpus and references hold none | **met on the first branch** for `StuntOpening` (54, table above); **met on the second branch** for `TraversalRoute` — `ROUTE_ABSENT_FROM_THE_CORPUS` cites the container's decoded fields, the image's `fvol`-only name consumer and `aiv.zrd`'s unmeasured encoding under #455 |
| `WorldAuditGap::NoRouteMeasured` gone for the groups the rule covers; any remaining gap names affected content | **met.** All eight retail groups report `NoRouteInMeasuredCorpus` naming their missions; `NoRouteMeasured` now fires only for a census no rule ever reached |
| pinned `accept_f18_d_*` / `accept_f18_e_*` updated deliberately; synthetic `RouteWithoutFacts` still fires | **met.** The two retail F18-D/F18-E tests were rewritten to the new verdict with named reasons; `accept_f18_d_an_unlocated_opening_or_route_is_reported_instead_of_assumed` keeps its `RouteWithoutFacts` arms untouched and `accept_f18_e1_…` adds an arm showing `RouteWithoutFacts` still wins when the facts are missing even though the route search measured |
| every classification cites the measured evidence; nothing classified by node-name pattern matching alone | **met.** The chain above has no name→class step; `is_detection_zone_name` only *addresses* the node the scenario already named |
| `vertex_scale_to_m` stays measured; nothing claims `verified_original` | **met.** `Some(1.0)` asserted in both retail tests; this document and every reason string say `observed_tool` |

## Test inventory

| test | covers | fails when |
| --- | --- | --- |
| `accept_f18_e1_a_located_stunt_passage_and_an_absent_route_carry_their_measurement` (synthetic, CI) | the production classification over authored gates (box → narrowest extent × caller unit → clearance, per-world, unresolved targets skipped); the `NoRouteInMeasuredCorpus` gap with affected content; `NoRouteMeasured` absent for a searched census and present for an unsearched one; `OPENING_SEARCH_UNSUPPLIED` for a census that searched nothing; `BlankOpeningReason` refused; `RouteWithoutFacts` still wins; a declared-but-unresolved stunt passage reported as a **shortfall** and only a group that declared none as an absence | the classification, the gap selection, the reason default, the blank-reason refusal or the absence/shortfall distinction is removed or weakened |
| `accept_f18_e1_retail_stunt_passages_are_located_and_every_absence_is_measured` (`#[ignore]`, retail) | one survey + one audit over all eight groups: the 5/5/0/9/0/4/14/17 split and the total 54; each located opening's mesh index resolves to a **stored** mesh of that group's own container and its clearance is finite and positive; every unlocated class carries a non-empty reason that is not `OPENING_SEARCH_UNSUPPLIED`, the four corpus-absent ones byte-identical to `class_absent`, the `c1c`/`c2b` ones naming their own scenario container; exactly one measured route gap per group, no shortfall gap anywhere | any count moves, a reason goes blank or turns generic, the wrong class is located, or `NoRouteMeasured` comes back |
| re-pinned: `accept_f18_d_retail_every_discovered_world_group_is_visited_and_compared`, `accept_f18_e_retail_placement_is_decoded_and_the_scale_is_the_measured_metre`, `accept_f18_d_an_unlocated_opening_or_route_is_reported_instead_of_assumed`, `accept_f18_d_the_audit_visits_every_group_and_compares_its_representative_geometry`, `accept_f18_d_world_group_records_refuse_contradictions_and_impossible_values` | the old verdicts, changed deliberately to the new one | the audit's verdict moves again without these being re-pinned |

## Sensitivity, run rather than assumed

Removing the route-search classification (one line: `if let
RouteSearch::Unstated` → `Unsought` in `census_verdict`) made
`accept_f18_e1_a_located_stunt_passage_and_an_absent_route_carry_their_measurement`
**FAIL** on its own gap assertion; the tree was restored with `git checkout
crates/cs_content/src/world.rs` and every check below re-run afterwards.

Two more arms were run by the reviewer of this task (2026-10-07), each restored
byte-for-byte from a saved copy afterwards (`shasum` verified):

* `locate_stunt_passages` returning an empty vector — the synthetic
  `accept_f18_e1_a_` **FAIL**s on its classification assertion (`left: []`);
* the *wiring* of that function in `traversal_evidence` replaced by
  `openings = Vec::new()` — the retail
  `accept_f18_e1_retail_stunt_passages_are_located_and_every_absence_is_measured`
  **FAIL**s at `world/c1`, because the census carries no located opening. The
  synthetic arm does **not** see this one (it assembles its own census), so the
  wiring is covered by the retail arm, which reviewers run with
  `--include-ignored`.

## Review (2026-10-07)

Implementer: `bunny-alpha-2` (Rally session of 2026-10-07, the four commits this
branch carries). Reviewer: `bunny-alpha-2` — the **same agent identity** in a
fresh session, so this is *not* the independent fresh-instance review AGENTS.md
prefers for fidelity claims; it is recorded here rather than glossed over, and
the owner's human approval still stands. What the review changed:

* `stunt_passage_absent` now separates the two states its two counts describe.
  A group whose scenario declares fly-through targets none of which resolved to
  a measured box used to read "so the corpus holds none of this class in this
  group" — an **unknown** (an unbound label or a boxless node, which F42-D
  reports as its own gap) stated as a measured absence, which AGENTS rule 4
  forbids. It now states the shortfall; only `declared == 0` says the corpus
  holds none. Over retail nothing changes: `c1c`/`c2b` declare 0 and the other
  six groups locate the class, so no group reaches the new text.
* The `RouteSearch::Unsought` doc said the audit surfaces `measured`; the audit
  keeps the shortfall gap [`WorldAuditGap::NoRouteMeasured`] and does not quote
  it. The doc now says that, and points at `WorldGroupCensus::route_search` for
  the caller that wants the text.

Review re-runs after those changes (all exit 0): `cargo fmt --all -- --check`;
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`;
`cargo test --workspace --locked`;
`cargo test --workspace --locked -- accept_f18_e1_ --include-ignored` (2
passed); `cargo test -p cs_app --test world -- accept_f18_d_ accept_f18_e_
--include-ignored` (9 passed).

## Commands run

```sh
cargo fmt --all -- --check                                            # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked   # exit 0, 428 test suites green
cargo test --workspace --locked -- accept_f18_e1_ --include-ignored
#   exit 0: 2 tests discovered, executed and passed (1 synthetic + 1 retail, 123 s)
cargo test -p cs_app --test world -- accept_f18_d_ accept_f18_e_ --include-ignored
#   exit 0: 9 passed (4 synthetic, 3 retail, 2 GPU) — the re-pinned verdicts
cargo test -p cs_app --test accept_f20_d_validation -- --include-ignored
#   32 passed; the only 3 failures are the pre-existing evidence harnesses
#   refusing for a missing CS_EVIDENCE_DIR, untouched by this task
```

## Affected content and open limitations

* **Located:** 54 stunt passages — the fly-through danger-zone objectives of the
  eight instant-action scenarios in `c1`, `c1b`, `c2`, `c3`, `c4`, `c5`.
* **Measured absent (all eight groups):** every tunnel, arch, building opening
  and hangar, and **every traversal route**. The F18 deliverable's five nouns are
  therefore located for one class and accounted for with evidence for four;
  `WorldGroupAuditReport::is_complete()` stays **false** over retail, which is
  the honest verdict.
* **Resolving task for routes:** **#455** (`F31-ROUTE-ENCODING`) — measure the
  original `aiv.zrd` route encoding. Only then can a `TraversalRoute` between
  authored points be stated instead of inferred; this audit will report it as
  soon as the census carries one, and `NoRouteMeasured` will still catch a
  census that states none after the facts exist.
* **Resolving task for the campaign half of the zones:** **#513** measured the
  framing of the campaign missions' `dzones.zrd` but not its meaning (which
  zones a mission uses, what `objective_numbers` indexes). Until that is
  measured, a `dzpath<N>` node that no instant-action scenario declares as a
  fly-through target is **not** located as an opening here — 26 of the 80
  numbered zones are in that state.
* **Not regenerated:** `docs/findings/evidence/F18-D.json` (F18-D's own
  harness). Its `census.json` artifact now differs from what this change
  produces, exactly as it already differed after #436 changed the placement
  verdict; regenerating it belongs to F18-D's harness, whose protocol and owner
  paths this task does not own. No number in that file was edited here.

## Sources used

- `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`,
  `docs/findings/2026-10-02-t427-retail-trigger-volume-thickness.md`,
  `docs/findings/2026-10-07-f18-grid-collision-origin.md`,
  `docs/findings/2026-10-01-f31-d-route-coverage-in-every-mission-type.md`,
  `docs/findings/2026-10-07-f18-e-world-placement-and-scale.md`.
- `crates/cs_app/src/stunts.rs` (`survey_retail_stunt_encoding`),
  `crates/cs_app/src/world/triggers.rs` (`survey_retail_trigger_volumes`),
  `crates/cs_content/src/stunts.rs` (the `.zrd` scenario extractors),
  `crates/cs_formats/src/gamez/nodes.rs` (what a record stores).
- `$CS_GAME_DIR` read-only; `$CS_GAME_DIR/crimson.decrypted.exe` sha256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`
  quoted through the two findings above, not re-analysed here.
