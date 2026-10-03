# F11-E1: what a `scene_node` id does with an authored name the key grammar cannot spell

Date: 2026-10-03. Task: #493 "Decide how a scene_node id treats a stored name
outside the key grammar" (feature sheet
`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, stages F11-A's
id scheme and the F11-B reader this stage unblocks). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capability used: **`retail`** (read-only
access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` and `audio` were
available and **not used**: nothing is rendered or played and no original run
happened, so nothing here claims `verified_original`.

This resolves the limitation
`docs/findings/2026-10-02-gamez-node-array-layout.md` recorded under "The
hierarchy in `planes.zbd`, and the one refusal": the shared aircraft container
now converts end to end, so F11-B's ECS import and F11-D's roster audit can run
over it.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/scene.rs` (extend, an F11 owner path):
  `SCENE_NODE_KEY_ESCAPE`, `escape_scene_node_name`,
  `unescape_scene_node_name`, `SceneNodeId::for_escaped_path` (replaces
  `for_path`), `SceneNodeId::authored_names`, the second path the DFS in
  `SceneGraph::build` carries, and the roster discovery's root-id derivation.
  The module doc, [`SceneRootRef::new`], [`ParsedNode::name`],
  [`SceneGraph::build`], [`SceneError::NodeId`] and
  [`parsed_nodes_from_gamez`] say what the rule is and what it is not.
- `crates/cs_content/tests/scene.rs` (extend, an F11 owner path): the four
  `accept_f11_e1_*` tests below, the three existing tests that asserted the old
  refusal, and — because the container's verdict changed — the F11-D2 retail
  acceptance test and its evidence harness.
- `docs/findings/2026-10-03-f11-e1-scene-node-id-escaping.md` (this file).

**One observable failure:** without the escaping, `SceneGraph::build` refuses the
real `ZBD/planes.zbd` at node 640 (`SceneError::NodeId`), so
`scene_graph_from_gamez` fails and the retail test's
`.expect("the real container must convert …")` panics. Removing the *reading*
half instead breaks the other direction: `SceneNodeId::authored_names` stops
agreeing with [`SceneNode::path`] for all 3 317 nodes, and the three synthetic
tests fail. Both directions are asserted, so a mutation that keeps only half of
the rule dies.

## The decision, and the three options not taken

The task offered three rules and picked none. What the evidence supports is the
first one — **a documented, reversible escaping of the characters the key
grammar cannot spell, with the stored name carried outside the id**:

| stored byte | in the key | why |
| --- | --- | --- |
| ASCII letter, digit, `_` | unchanged | the grammar spells it; nothing may rewrite an id that was already right |
| ASCII letter in upper case | unchanged | [`ContentId`] folds case centrally (`M01` and `m01` are one item); see the loss below |
| `-` | `-2d` | the escape character has to be escapable, or the reading is ambiguous |
| `.` | `-2e` | `.` is this scheme's name-path separator, so a name spelling one itself is indistinguishable from a nested path |
| anything else (e.g. `0x20`) | `-` + two lowercase hex digits (`-20`) | the grammar refuses it |

Reading left to right: at a `-`, exactly two following lowercase hex digits are
one byte and nothing else is an escape. That is total, injective and
reversible, and it is the reading half implemented as
`SceneNodeId::authored_names`, which is public so the reversibility claim is
checkable from outside the module.

**Why the escape character is `-`, measured:** over all **56 620** stored names
of the nine GameZ archives the grammar's punctuation is `_` **7 514** times, `.`
**1 599** times and `-` **twice** (`terpat01-128` in `C4` and `terpat04-128` in
`C2`, both roots of world containers). Any injective escaping must reserve one
character the grammar allows, so the choice is between rewriting the ids of
almost every node in the installation or of two. `_` was the obvious
candidate and was rejected on that measurement: it changes
`scene_node/…player_bhawk` to `…player_5fbhawk` and would have moved every id
F11-D2's roster discovery had already derived, F11-C's sockets and the F11-B
ECS bindings with it. `.` is already reserved as the separator and is escaped for
that reason, not for its frequency.

**Option 2 (an authored disambiguator) is not available and would not be
wanted.** The store holds no disambiguator: the only per-node words left are
`node_index` (not unique — 120 values repeat in `planes.zbd`) and the array slot
(position). F11-A's rule is that ambiguity is never resolved by position, and
adding an authored field is a format question the corpus cannot answer.

**Option 3 (trim, documented as lossy) was rejected because it is strictly
worse than an injective rule.** Trimming `brigturret2 ` would make two nodes
that differ only in trailing space derive one id — an ambiguity manufactured by
the reader — and it discards the stored byte, so the id could no longer say what
the file said. The escaping loses nothing at all, and the stored name is still on
the record.

### The one documented loss

Case. `ContentId::from_source` lowercases, so two stored names differing only in
case derive one id and are refused as `SceneError::DuplicateNodeId` — refused,
not silently merged. This is the repository-wide identity rule rather than
something this decision introduced, and escaping case would actively break F11-D2:
the roster discovery matches a script-declared root name against the stored node
name case-insensitively, and two spellings that ContentId folds are one id there
on purpose. The three measured spellings in the corpus are `Default` (41 nodes in
`C5`), `G` (1) and `glide_bombC5` (1); all in world containers.

## Measurement: the real container, before and after

The reader is unchanged — no transliteration, trimming or escaping was added to
`cs_formats`; a name crosses over byte for byte, as
`docs/findings/2026-10-02-gamez-node-array-layout.md` required.

`ZBD/planes.zbd`, read through `cs_formats::gamez::read_gamez_nodes` and
converted with `cs_content::scene::scene_graph_from_gamez` over the F16-A
`canonical` adapter:

| | before | after |
| --- | --- | --- |
| records converted (`parsed_nodes_from_gamez`) | 3 317 | 3 317 |
| `SceneGraph::build` | `SceneError::NodeId { node: 640, source: BadKeyCharacter { ch: ' ' } }` | 3 317 nodes, 28 roots |
| ids whose authored path reads back | — | **3 317 of 3 317**, zero mismatches |

The six affected nodes, by stored slot, stored name and derived id — the
authored path is `player_brigand.cockpit1.brigand_turret2. …`, written out here
in full because these are exactly the ids the retail test pins:

| slot | stored name | id key under `container.zbd.planes` |
| --- | --- | --- |
| 640 | `brigturret2 ` | `player_brigand.cockpit1.brigand_turret2.brigturret2-20` |
| 641 | `g938` | `player_brigand.cockpit1.brigand_turret2.brigturret2-20.g938` |
| 642 | `hgun2` | `player_brigand.cockpit1.brigand_turret2.brigturret2-20.hgun2` |
| 643 | `hfirepoint2` | `player_brigand.cockpit1.brigand_turret2.brigturret2-20.hgun2.hfirepoint2` |
| 644 | `g65` | `player_brigand.cockpit1.brigand_turret2.brigturret2-20.hgun2.g65` |
| 645 | `hfirepoint2b` | `player_brigand.cockpit1.brigand_turret2.brigturret2-20.hgun2.hfirepoint2b` |

640 stores the space; 641–645 inherit it through the parent link. Every one of
the 3 317 ids reads back to the name-path the file holds, and `planes.zbd`
holds **no** name with a `-`, a `.` or an upper-case letter, so those six ids
are the only ones this rule moves in that container.

### What the escaping changes outside `planes.zbd`

Measured over the other eight archives (none of which converts yet, see the
limitations):

- **1 599** `.` occurrences, in that many names (each holds exactly one), of
  which **245 are roots** — 26 in `C1`, 23 in `C1B`, 25 in `C1C`, 21 in `C2`, 22
  in `C2B`, 25 in `C3`, 27 in `C4`, 76 in `C5`. Those 245 are the concrete cost
  of not escaping `.`: a root named `lightpole.flt` would spell its key with a
  separator inside it, so `SceneRootRef::new` would refuse it as
  `SceneError::NotARootNode`. `planes.zbd` has **0** dotted names, so nothing in
  the shared archive moves for this.
- 120 `\` and 24 `:` characters, all inside one intermediate name per affected
  container, `z:\crimsonrun\data\common\vessels\` (12 nodes in `C1B`, 12 in
  `C3`); it becomes
  `z-3a-5ccrimsonrun-5cdata-5ccommon-5cvessels-5c`.
- the two `-` names listed above, both of them roots, which become
  `terpat01-2d128` and `terpat04-2d128`.

## The knock-on effect on F11-D2's audit, measured

`crates/cs_content/tests/scene.rs` asserted that *every* container refuses, and
that assertion became false, so the F11-D2 retail acceptance test and its
evidence harness now answer the audit's graph source from each container's own
measured verdict (`retail_container_verdicts`, replacing
`retail_container_blockers`). Measured over the real installation:

| | before | after |
| --- | --- | --- |
| mapped containers | 0 | **1** (`install_file/zbd_2f_planes.zbd`: 3 317 nodes, 28 roots, 11 airframes, no container-level gap) |
| refused containers | 9 | 8 (the world containers, each with its own `InconsistentParentage` reason) |
| mapped airframes / roots | 0 / 0 | **10 / 10** |
| blocked airframes | 11 | **1** (`player_pfighter` → `AirframeBlocker::RootMissing`) |
| mapped sockets | 0 | 0 — no evidence-backed binding rule exists yet |
| blockers (container + airframe) | 20 | 9 (eight container refusals and one wrong root reference) |
| gaps | not recorded before this change | 21 |

`player_pfighter` is the one row the store contradicts, and now that the
container converts the audit can say so precisely: the script created it as a
root, the store nests node 44 under the script's own parentless `player` node
1418, so the id the row names — `zbd_2f_planes.zbd.player_pfighter` — is not a
node the graph holds (`zbd_2f_planes.zbd.player.player_pfighter` is). The row is
**not** repointed at `player`: a reference is checked, never resolved to the
nearest match.

The ten mapped rows carry the measured subtree sizes 199, 202, 215, 202, 174,
215, 216, 211, 208 and 235 nodes, each reported as `AuditGap::MissingRole` for
the declared `cockpit` role because no binding rule binds a node yet. That is a
real shortfall, reported as one, not a pass.

## Test inventory

| `accept_f11_e1_*` test | Covers | Fails when |
| --- | --- | --- |
| `an_unspellable_authored_name_is_escaped_into_its_id_and_reads_back` | the escape table over the three spellings the installation stores (`brigturret2 `, `z:\crimsonrun\data\common\vessels\`, a `-`, a `.`); that a name the grammar can already spell is untouched; that every spelling is a legal key; the real container's shape over the production reader and writer (slots 640–645's analogue), each id read back to the stored name-path; the stored name and the authored path unchanged on the record; every node of the converted container reading back | the escaping is removed, a name the grammar spells is rewritten, the record's stored name is trimmed or escaped, or the reading half disagrees with the authored path |
| `the_escape_is_injective_and_a_case_clash_is_still_refused` | `brigturret2 `, `brigturret2-20` and `brigturret2-2d20` deriving three distinct ids (a naive "replace the space" spelling would give the first two one); the exact three keys; two roots differing only in case still refused as `DuplicateNodeId`; `authored_names` returning `None` for another container and for three malformed escapes, and the names of a well-formed nested id | the escape character is not escaped itself, the case clash is silently merged, or a malformed key reads back as a plausible name |
| `a_dotted_root_name_is_a_root_and_a_nested_node_is_not` | `ap_lightpole.flt` spelled `ap_lightpole-2eflt` and accepted by `SceneRootRef::new`, while a nested id is still refused as `NotARootNode` (the corpus has 245 dotted roots) | `.` is escaped as something else, or the root check stops being a check on separators |
| `retail_planes_node_array_builds_every_node_id` (retail) | the real `planes.zbd`: the whole container converts (3 317 nodes, 28 roots); every id reads back; the six affected nodes by slot, stored name and exact key; and that an unaffected root keeps the key it always had | the conversion is refused, an id moves, a name is rewritten, or a seventh node is affected |

**Sensitivity check.** Mutations applied and reverted, each killed by a test CI
runs (only the fourth is `#[ignore]`d):

| mutation | killed by |
| --- | --- |
| escaping removed (the old behaviour: the joined authored path straight into the key) | the retail test and the first synthetic test |
| the escape character changed to `_` | `an_unspellable_…` (the `r_aileron1` and `player_bhawk` cases) and `retail_planes_…` |
| `.` no longer escaped | `a_dotted_root_name_…`, and the first test's escape table |
| the reading half made to skip the container prefix, or to accept an uppercase hex digit | `the_escape_is_injective_…` |
| `unescape_scene_node_name` losing the `String::from_utf8` check (returning a lossy string) | `the_escape_is_injective_…` and the first test's round trip |
| a name trimmed instead of escaped | the first test and the retail test |

## Unknowns and limitations (all recorded, none guessed)

- **The world containers still refuse**, with `SceneError::InconsistentParentage`,
  and that is unchanged by this task: 155–471 nodes per container are absent from
  their parent's child list. **Affected content:** every world container's
  hierarchy, so F18's world import and any airframe that lives in one.
  **Unmeasured:** whether the child lists are a partial index or the parent slots
  are authoritative. Filed as a follow-up; the reference reads both and asserts
  they agree, so it would refuse them too.
- **A name-path longer than `MAX_CONTENT_KEY_LEN` (128 bytes) is still refused**,
  as `SceneError::NodeId { source: KeyTooLong }`. **Affected content:** the world
  containers, whose longest authored name-paths measure **136 bytes** in six
  archives (`pzhookpoint.hoop.hook_seg.mid_seg.top_seg.hook_base.pzhookup_crane.
  underneath.rock_zeppelin.tilt_zeppelin.move_zeppelin.piratezep.world1`) and
  **150 bytes** in `C1C`; `planes.zbd`'s longest is 82 bytes, so its longest
  derived key is 104. **This is a second, separate decision** — whether a long
  path is shortened, folded or refused — and it is F18's to take together with
  the parentage question, not this stage's. A follow-up task is filed.
- **The escape is only as good as the key grammar it fits into.** A name holding
  a `NUL`, a `/` or a non-ASCII byte cannot be stored in the 36-byte field as the
  reference reads it (the reader requires ASCII in the name prefix), so the rule
  is stated over bytes rather than over the corpus's actual repertoire; a
  non-ASCII name would round trip as its UTF-8 bytes escaped, which is why
  `unescape_scene_node_name` checks the decoded bytes are valid UTF-8.
- **`authored_names` needs the container id** to know where the name-path starts,
  because an id does not carry it. A caller holding only the id cannot recover
  the path; `SceneNode::path()` remains the direct answer for a node it already
  has.
- **The committed F11-D2 evidence artifact predates this change.**
  `docs/findings/evidence/F11-D2.json` was generated from candidate tree
  `67e2b789…` and records `mapped_roots: 0`, `refused_containers: 9` and a
  limitation naming node 640's trailing space — all falsified by this task. The
  harness itself is updated and was run end to end here: it now writes
  `mapped_roots: 10`, `refused_containers: 8`, `blockers: 9`, `gaps: 21` and
  validates with `tools/validate_evidence.py --require-pass`. **The committed
  copy was deliberately not regenerated in this session**, because
  `review.identity` must name the reviewer and the harness's default text names
  the previous implementer; a follow-up task regenerates it with a reviewer's own
  identity, as `docs/contracts/CLI-EVIDENCE.md` requires.
- **Evidence class.** The escaping rule is designed engine contract: it is a
  choice about how *this* repository spells ids, not a measurement of the
  original game. The measurements above (which bytes the corpus holds, which six
  nodes carry the space, what the audit reports now) are from the original bytes
  through production readers, which makes them `ObservedTool`. No original run
  happened, `retail` is file access and not evidence of runtime behaviour, and no
  agent review replaces the owner's approval. **A further agent instance with a
  fresh context should review this id-scheme change**, because it changes a
  published identity contract.
- **Nothing derived from the original bytes is committed.** The numbers above are
  counts, offsets, ids and digests; no name list, no mesh, no material and no
  screenshot is in the repository.

## Sources used

- `docs/findings/2026-10-02-gamez-node-array-layout.md` (the measurement, the
  refusal this task replaces, and the requirement that a name crosses over
  unchanged).
- `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md` (the
  `scene_node` id scheme, the `DuplicateNodeId` rule, and the central
  case-folding normalization).
- `docs/findings/2026-10-02-f11-d-2-airframe-roster-discovery.md` (the roster,
  the per-airframe blockers and the evidence protocol whose numbers moved).
- `crates/cs_types/src/content.rs` (`normalize_key`, `normalize_grammar`,
  `MAX_CONTENT_KEY_LEN`, the case-folding rule) and
  `crates/cs_content/src/scene.rs` (`ParsedNode`, `SceneGraph::build`,
  `SceneRootRef`, `AirframeRoster::audit`).
- `crates/cs_formats/src/gamez/nodes.rs` and `reader.rs` (the reader this task
  did not touch, and the container header both sections share).