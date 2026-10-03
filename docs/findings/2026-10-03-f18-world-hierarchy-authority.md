# Which side of a world node's hierarchy is authoritative

Date: 2026-10-03. Task: #494 "Measure which side of a world node's hierarchy is
authoritative". Feature sheet:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`, stage
`### F18-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and ordinary
build/test. `gpu` and `audio` were available and **not used**: nothing is
rendered or played and no original run happened, so nothing here claims
`verified_original`.

This resolves the blocker #392 recorded and F18's world import inherits: all
eight world containers of the installation refused to convert, because 155 to
471 of their records each name the world node as their parent while the world
node's own child list does not list them, and `SceneGraph::build` refuses that
with `InconsistentParentage` before any name is reached.

## The short answer

**The parent slot is the authoritative statement of ownership; the child list is
an index that cannot veto it.** That is the rule
`cs_content::world::world_hierarchy_from_gamez` implements and the rule the
tests pin.

What the evidence supports is narrower than "the original engine does this", and
this file does not claim the latter. **No original run happened, so which side
the 2000 engine treated as authoritative when it loaded a world is UNMEASURED
and stays unknown.** What was measured is that the disagreement is not a
contradiction in any direction a hierarchy could be built from, that it is
confined to exactly one record per container, and that the reading below is the
only one under which every stored record of every container is reachable. That
makes it the conservative reading: it never drops a link the store states and
never invents one that the store does not.

## Files

- `crates/cs_content/src/world.rs` (extend, an F18 owner path): the measurement
  ([`audit_stored_hierarchy`], the `StoredHierarchyAudit` record and the
  `HierarchyVerdict` enum), the rule
  ([`world_hierarchy_from_gamez`], `WorldHierarchy`,
  `WorldHierarchyError`), the container path
  ([`world_scene_graph_from_gamez`], `WorldSceneGraph`, `WorldSceneError`),
  [`world_node_slot`] and the claim id
  `HIERARCHY_PARENT_SLOT_AUTHORITATIVE`.
- `crates/cs_app/tests/world/hierarchy.rs` (new, an F18 owner path): the eight
  `accept_f18_a_` tests and the two retail ones.
- `crates/cs_app/tests/world/main.rs` (wiring only): `mod hierarchy;`.
- `docs/findings/2026-10-03-f18-world-hierarchy-authority.md` (this file).

**No reader refusal was weakened.** `GameZNodeError::DataOffset`,
`GameZNodeError::ChildSlot`, `GameZNodeError::ParentSlot` and
`SceneError::InconsistentParentage` are all untouched; `git diff` touches no
line of `crates/cs_formats`. The rule lives entirely on the F18 side of the
build: the reconciled records satisfy the strict build rather than the build
being relaxed to accept unreconciled ones. `crates/cs_content/src/scene.rs` is
also untouched.

## The measurement

Every number is the production reader's output over the read-only
installation, recomputed by the tests from
`audit_stored_hierarchy`. **A** is "names a parent that does not list it";
**B** is "listed by a node it does not name as its parent".

| container | nodes | child slots | world node's stored list | records naming it | **A** | B | listed twice | roots | unreachable under the rule |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `ZBD/C1/gamez.zbd` | 7 064 | 6 553 | 66 | 412 | **346** | 0 | 0 | 165 | 0 |
| `ZBD/C1B/gamez.zbd` | 5 603 | 5 300 | 78 | 233 | **155** | 0 | 0 | 148 | 0 |
| `ZBD/C1C/gamez.zbd` | 5 644 | 5 202 | 53 | 346 | **293** | 0 | 0 | 149 | 0 |
| `ZBD/C2/gamez.zbd` | 4 956 | 4 532 | 24 | 282 | **258** | 0 | 0 | 166 | 0 |
| `ZBD/C2B/gamez.zbd` | 4 901 | 4 465 | 48 | 338 | **290** | 0 | 0 | 146 | 0 |
| `ZBD/C3/gamez.zbd` | 5 408 | 4 812 | 14 | 453 | **439** | 0 | 0 | 157 | 0 |
| `ZBD/C4/gamez.zbd` | 8 289 | 7 776 | 51 | 401 | **350** | 0 | 0 | 163 | 0 |
| `ZBD/C5/gamez.zbd` | 11 438 | 10 752 | 105 | 576 | **471** | 0 | 0 | 215 | 0 |
| `ZBD/planes.zbd` | 3 317 | 3 289 | — (no world node) | — | **0** | 0 | 0 | 28 | 0 |

Three things this table says that the corpus did not say before:

1. **The disagreement is one-directional.** Across all nine archives, B is
   **0** and no record is listed by two parents. Not one stored child slot
   contradicts the parent slot of the record it points at. A "both sides are
   authoritative and they disagree" reading would have to explain a container
   whose every child slot is correct while 439 of its records are
   simultaneously parentless and indexed as world content; the data does not
   support that.
2. **It is confined to one record per container.** In every world container
   `partial_parents == 1`, and that parent is the container's **world node**
   (`world_node_slot`, slot 0, `world1` in every one). The world node's stored
   list is a strict subset of the records naming it, in every container. The
   other 164 to 214 roots and every interior link agree in both directions.
3. **The world node's own partition index is exactly the omitted set.** The
   world record's `partition_x_count × partition_y_count` grid of 88-byte cells
   holds 12-byte values whose first word is a node slot, and over all eight
   containers the set of distinct slots the grid names is **exactly** the set of
   records the world node's child list omits — no more, no fewer (grid values
   are all in range, and the two sets are disjoint and their union is precisely
   the records naming `world1`). The world's own record therefore indexes the
   same children by a second route, which is a fact about what the stored list
   is: an index the writer filled in partially, not a competing statement of
   ownership. The grid's remaining 8 bytes per value and the cell geometry are
   **unmeasured** and are F18's world-record subject, not this task's.

   The set equality is measured two ways. The **count** is pinned by
   `accept_f18_a_retail_every_world_container_says_which_side_its_hierarchy_disagrees_on`
   from the production reader alone: the grid holds exactly `omitted` values
   (346, 155, 293, 258, 290, 439, 350, 471), which is also exactly the number
   of records the stored list omits. The **slots** were compared by a byte-level
   walk of the grid (`data_offset + 208`, 58 bytes + a `u16` count + 28 bytes of
   cell, then `count` 12-byte values per cell) that no committed test performs,
   because interpreting the grid is F18's world-record work; it found the same
   slot set, every value in range, disjoint from the stored list.

Under the adopted rule, every record of every archive is reachable from a root,
so no container is refused for a cycle or a dangling link. `planes.zbd` is
unaffected: 0 disagreements, so nothing is appended and every record converts
to the parsed record it already was.

**The stored child order is the store's own, not the order of the child
records.** Measured over the corpus, a stored child list is *not* the list of
its children in record order: 56 of `planes.zbd`'s 3 317 records and 135 to 219
records of each world container store a different order from the record order.
That is why the rule extends a stored list instead of rebuilding it (see the
first design decision), and it is pinned by the retail test, which checks over
every record of two containers that the stored list is a **prefix** of the
reconciled one and that the world node is the only record whose list grows.

## Design decisions

- **The rule is a derivation, not a repair, and it is add-only in content and
  in order.** A record's children are the records that name it; a record's own
  stored list is kept exactly as stored and the links only a parent slot states
  are appended to it. Every added link comes from a parent slot the record
  itself stores, so the conversion can add links the store states but can never
  remove one, and it never reorders a stored link either — which matters
  because the stored order is not the child-record order (measured: 56 records
  of `planes.zbd`, 135 to 219 per world container). The stored lists stay
  untouched in the caller's `GameZNodes`. This is why it is conservative rather
  than convenient, and why a container the two sides already agree on converts
  field for field as it did before this stage.
- **The audit measures; the rule converts.** `audit_stored_hierarchy` never
  refuses and never interprets: a container whose two sides contradict each
  other gets a `HierarchyVerdict::Contradictory` with the exact counts, because
  "these containers do not agree" is a result, not an absence of one.
- **A contradiction blocks instead of being resolved.** A record listed by a
  node it does not name, or by two parents at once, is a case the adopted rule
  does not cover: the corpus has none, and a container that had one would be
  evidence the rule does not hold there. `WorldHierarchyError::Contradictory`
  carries the verdict so the report can name the counts. Silently converting it
  under a rule its own bytes contradict is exactly the guess this task was told
  not to make.
- **A consistent container is a no-op, and the tests say so.** For
  `planes.zbd`'s shape the reconciled records are the parsed records, field for
  field and child order included, which is what keeps this a second reading of
  the *disagreeing* containers rather than a second reading of every hierarchy.
  Both halves are asserted: over every record of two world containers that the
  stored list is a prefix of the reconciled one, and over all of `planes.zbd`
  that the reconciled vector equals `parsed_nodes_from_gamez` outright.
- **Reachability is measured before the build refuses it.** `Cycle` stays
  `SceneGraph::build`'s refusal; the audit reports the same count up front so a
  caller learns it without running the build.

## What the eight containers do now

Read through `world_scene_graph_from_gamez`, **all eight reconcile their
hierarchy and then reach `SceneGraph::build`, which refuses them** — never with
`InconsistentParentage`, which is the refusal this task exists to resolve, but
because F11-A's `scene_node` id scheme cannot spell world node names: 72 to 376
records per container store a name the key grammar refuses (over-long
name-paths, `:` and other characters outside `[a-z0-9._-]`), and a further 32 to
2 015 share a name-path with another record, which the build refuses as
`DuplicateNodeId` because ambiguity is never resolved by position. That is a
fact about the data meeting a published contract, recorded rather than worked
around, and it is the same class of refusal the `brigturret2 ` case already
records for `planes.zbd`.

`WorldSceneError::Build` carries **both** halves — the build's own typed
refusal verbatim and the audit beside it — so a caller reporting "this world
container is blocked" can name which refusal remains and the exact count the
rule resolved. Neither half is translated, retried or softened.

The two ranges above (72 to 376 unusable names, 32 to 2 015 shared name-paths)
come from a scratch walk of the eight containers that re-derived the ids outside
the build. **No committed test reproduces them**, because pinning them needs the
id grammar relaxed, which is exactly what must not be done here; the retail test
asserts only what the production path reports, which is the build's own refusal
for the first offending record of each container. They are the reason to file
the id-grammar blocker, not a claim the code depends on.

## Test inventory

| `accept_f18_a_` test | Covers | Fails when |
| --- | --- | --- |
| `a_world_child_list_that_omits_records_is_a_partial_index_not_a_refusal` | the partial-index verdict and its four counts; the stored child list kept and the parent-slot-only links appended; an agreeing list unchanged; the whole path producing a graph whose root has all three children; and — by calling `scene_graph_from_gamez` on the same records — that the disagreement is real and the rule is what resolves it | the derivation stops, the verdict changes shape, or the rule is replaced by the strict build alone |
| `a_child_list_that_names_the_wrong_parent_blocks_the_container_with_its_counts` | a `Contradictory` verdict for a wrong-parent listing, the partial-index count reported alongside it, `Contradictory` carrying the verdict, and the container path reporting `WorldSceneError::Hierarchy` | the audit stops seeing the wrong-parent direction, or the blocker is resolved into a conversion |
| `a_record_two_parents_list_blocks_the_container` | the `listed_twice` count and the blocker it produces | two parents claiming one record is accepted |
| `the_rule_appends_to_a_stored_list_and_never_reorders_it` | a stored list whose order is *not* the child-record order, reconciled to the stored order with the omitted link appended, the stored links a prefix, and the appended count equal to the omitted count | the rule rebuilds a stored list instead of extending it |
| `a_consistent_container_needs_no_rule_and_the_derivation_is_a_no_op` | `Consistent` for an agreeing container, `world_node_slot`, and every reconciled child list equal to its stored one | the rule changes an already-consistent hierarchy |
| `an_unreachable_parent_chain_is_measured_before_the_build_sees_it` | a detached cycle that is `Consistent` as a statement yet `unreachable == 2`, and `Build { source: Cycle, audit }` carrying the count | reachability is not measured, or the count is lost from the blocker |
| `retail_every_world_container_says_which_side_its_hierarchy_disagrees_on` (retail) | all eight containers: the verdict with its measured counts, B and the twice-count at 0, one partial parent, no unreachable record, the world node's own list being shorter than the records naming it by exactly the omitted count, and the world record's partition grid holding exactly that many values | any count moves, or the partial list belongs to a record other than the world node |
| `retail_the_world_containers_convert_their_hierarchy_and_report_the_refusal_that_remains` (retail) | all eight reconcile and reach the build; the remaining refusal is never `InconsistentParentage`; the blocker carries the omitted count; two containers end to end with the reconciled list a strict superset of the stored one, every stored list a prefix of its reconciled list and the world node the only record that grows; and `planes.zbd` at 0 disagreements with its reconciled records equal to `parsed_nodes_from_gamez` | the rule stops resolving a container, a build refusal is misreported as a hierarchy one, a stored link is reordered or dropped, or the aircraft container changes |

**Sensitivity check.** Five mutations applied and reverted, each killed by a
test **CI can run** (the two retail tests are `#[ignore]`d and are not):

| mutation | killed by |
| --- | --- |
| the derivation removed entirely (keep the stored children) | the synthetic partial-index test, the retail conversion test |
| the derivation **replaces** a stored list instead of extending it | the append-in-order test, the retail conversion test |
| the audit's wrong-parent count forced to 0 | both contradiction tests |
| the partial-index count halved | the synthetic partial-index test, the retail verdict test |
| reachability counted with the stored lists instead of the derived ones | the synthetic partial-index test, the retail verdict test |

## Unknowns and limitations (recorded, not guessed)

- **Which side the original engine trusted is UNMEASURED.** No original run
  happened and none is available to an agent. The three candidate readings in
  the task description remain formally open; what the corpus settles is that one
  of them (both authoritative, disagreeing) is contradicted by the data, since
  no stored child slot disagrees with the parent slot it points at. **Affected
  content:** every claim about the original's runtime hierarchy behaviour.
  **Not affected:** the conversion implemented here, which is stated as this
  project's rule and carries its own claim id
  (`f18-world.hierarchy-parent-slot-authoritative`). Resolving the engine's
  behaviour needs an owner-supplied original capture and belongs to F18-D.
- **Why the world node's list is partial is unknown.** It is measured to be a
  strict subset, over 24 to 105 of 233 to 576 children, and the world record's
  partition grid to name exactly the remainder. Whether the writer filled in a
  fixed-size list, dropped entries it indexed elsewhere, or something else is
  not measurable from the bytes.
- **The world node's partition grid is not interpreted.** Its cell geometry, the
  8 bytes after each value's node slot and the header's own words are read as
  bytes only. Whether the hierarchy is a true tree per world partition is
  F18's world-record subject and is untouched here.
- **The remaining build refusal is F11-A's id scheme, not this rule.** 72 to 376
  records per container store a name the key grammar refuses and 32 to 2 015
  share a name-path. **Affected content:** the canonical identity of world
  container nodes, so F18-B's object-instance ids and any F11-D roster row whose
  root lives in a world container. The fix belongs to the F11/F18 stages that
  own the id scheme; transliterating a name here would invent an identity the
  store never had.
- **A corpus that contradicted the rule would block rather than convert.** That
  is a designed refusal, stated so a future measurement is not silently routed
  around it: if a container ever reports `Contradictory`, the rule does not hold
  there and the finding above needs revisiting.
- **Evidence class.** The hierarchy facts are **measured from the original
  bytes** by the production reader across all nine archives, which makes the
  layout and the counts `ObservedTool` + measurement. The **rule** is a
  designed engine contract, not a measurement of the game. No original run
  happened; `retail` is file access, not evidence of runtime behaviour.
  **A further agent instance with a fresh context should review this format
  work**, and no agent review replaces the owner's approval.
- **Nothing derived from the original bytes is committed.** The numbers above
  are counts and ranges; no name list, no mesh and no screenshot is in the
  repository.

## Sources used

- `docs/findings/2026-10-02-gamez-node-array-layout.md` (the layout, the
  per-container counts and the unknown this task was filed for) and
  `docs/findings/2026-09-29-f11-b-hierarchy-import-and-lod-selection.md` (the
  limitation this inherits).
- `crates/cs_formats/src/gamez/nodes.rs` (`read_gamez_nodes`, `RawNode::parent`
  / `children` as stored, `RawWorldData`'s two grid counts) and
  `crates/cs_content/src/scene.rs` (`SceneGraph::build`,
  `SceneError::InconsistentParentage`, `parsed_nodes_from_gamez`,
  `scene_graph_from_gamez`).
- `crates/cs_app/src/world/triggers.rs` (the earlier F18 survey over the same
  containers, which walks records without the hierarchy and is unaffected).
- Pinned reference mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S17] in
  `docs/research/SOURCES.md`): `crates/mech3ax-gamez/src/gamez/cs/nodes.rs`
  (`read_node_data` reads both sides and asserts they agree, so it would refuse
  these containers too) and
  `crates/mech3ax-nodes/src/cs/{node,world}.rs` (`NodeCsC`, `WorldCsC`,
  `PartitionCsC`). No code was copied; mech3ax is EUPL-1.2 and is read as a
  reference only.
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, ambiguity refused rather
  than resolved by position, ownership cycles invalid).
