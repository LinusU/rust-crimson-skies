# F11-D: the private roster audit, and what it flags

Date: 2026-09-30. Task: F11-D "Validate every discovered airframe including
mission-only types" (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`,
section `### F11-D`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Required capability: **`retail`** — this stage read the owner's original
installation (`$CS_GAME_DIR`) with the production discovery, the production
GameZ readers and the new production roster audit. No `gpu`, `audio`,
`human_play` or `human_review` was used or available: this stage renders
nothing, plays nothing and was not reviewed by a human.

AC04 is *"Private roster audit maps every root, part, mount and cockpit binding
**or flags a blocker**"*. The honest result over the real installation is the
second arm, and this document says exactly what is blocked, by how much, and
which task resolves it.

## Scope decision (taken before editing)

This stage builds the audit instrument and runs it over the real corpus. It
does **not** add a format reader, a roster source or a gameplay rule: the
GameZ node-array reader is owned by no F11 stage and is filed as **#392**,
and no airframe roster has been discovered from original data at all (that is
the new follow-up task filed with this stage). Guessing either would be
exactly the "developer placeholder" the sheet forbids.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/scene.rs` (extend): the roster audit —
  `RosterEntry`, `ForcedMissionAssignment`, `AirframeRoster`, `AirframeRoster::audit`,
  `SceneContainerRef`, `ContainerBlocker`, `AirframeBlocker`, `AuditGap`,
  `ContainerAudit`/`ContainerOutcome`, `RosterAuditReport`, `RosterError`,
  plus `SceneGraph::subtree` (the one production definition of "the nodes
  under this root").
- `crates/cs_content/tests/scene.rs` (extend): the three `accept_f11_d_*`
  synthetic tests, the `#[ignore]`d retail AC04 test and the evidence harness.
- `crates/cs_app/src/scene.rs` (wiring/de-duplication only, no new behaviour):
  `import_airframe` now calls `SceneGraph::subtree` instead of carrying its own
  copy of the subtree walk.
- `docs/findings/2026-09-30-f11-d-private-roster-audit.md` and
  `docs/findings/evidence/F11-D.json` (this file and the evidence copy).

**One observable failure:** if the audit ignored the graph source's typed error
and reported a mapping anyway, then
`accept_f11_d_retail_the_private_installation_roster_audit_flags_every_container`
fails on the real installation — measured, not predicted — with
`assertion 'left == right' failed  left: 9  right: 0` on
`report.mapped_containers().count()`, and the two synthetic tests fail too.

## Design decisions

- **The audit's input is declared, its verdict is computed.**
  `AirframeRoster::audit(containers, graph_of)` takes the declared roster (one
  `RosterEntry` per discovered airframe: its catalog id, the checked
  `SceneRootRef` it references, its `RosterAvailability` and the
  `required_roles` it is declared to bind) plus the set of discovered
  `SceneContainerRef`s. `graph_of` is the seam: a caller that can convert a
  container hands over the `SceneGraph`; a caller that cannot hands over a
  typed `ContainerBlocker` carrying the measured `node_array_size` and
  `nodes_offset`. "Blocked" and "mapped" are therefore the same verdict with a
  different input, not two different reports — when #392 lands the same call
  maps instead of blocking and no audit code changes.
- **A blocker quotes numbers, not prose.** `ContainerBlocker::NodeArrayUndecoded`
  carries the container id, the header's `node_array_size` and the header's
  `nodes_offset`, and its `Display` prints them. The retail verdict is
  "9 containers, 56,620 stored node records, 0 decoded", which is a statement
  someone can act on rather than a message to interpret.
- **Every airframe in a blocked container inherits that blocker.** An airframe
  also gets `RootUndiscovered`, `RootMissing` and `ContainerNotAudited` when
  its own row is the problem. Nothing falls back to "the first root": a
  reference to a root the converted container does not hold is
  `RootMissing`, not the nearest match (F11 deliverable: roots are referenced,
  never selected by position).
- **Required roles are declared, never inferred.** The audit checks that each
  role the evidence says an airframe has is really bound to a node, and says
  nothing about a role nobody claimed. An empty `required_roles` therefore
  audits "this root's bindings", not "this root is complete" — and the
  difference shows up as a missing mapping, not as a silent pass.
- **Roster availability and forced mission assignments are separate
  discoveries.** `ForcedMissionAssignment` records "this mission puts this air
  in the air"; `RosterAvailability` records what a player may select. There is
  no path from one to the other: an airframe that two missions force and whose
  availability nobody discovered keeps the explicit
  `RosterEntry::undiscovered_availability()` unknown, and the audit reports
  `AvailabilityUndiscovered` **and** `ForcedAssignmentOnly` for it
  (F11 non-negotiable behavior 3). `AirframeRoster::new` refuses an assignment
  naming an airframe no row audits, so a forced plane cannot hide a hole in the
  roster.
- **A partial decode is a gap, not a pass.** `SceneContainerRef` carries the
  header's `node_array_size`, and a converted graph whose node count disagrees
  produces `AuditGap::NodeCountMismatch`. The correspondence between
  `node_array_size` and the number of node records is *ObservedTool* evidence
  (pinned reference header comments), not a measured original fact, so this can
  only ever *flag*; it is recorded as such below.
- **A rule that bound nothing belongs to the container, not to an airframe.**
  `SceneGraph::unmatched_bindings()` is a container-level property — the report
  does not know which root a rule was aimed at — so `AuditGap::UnmatchedRule`
  is a `ContainerAudit` gap and no airframe claims it mapped cleanly by
  accident.
- **The pose in a report is the scene's pose.** `MappedSocket::pose` is a copy
  of the node's one composed `CanonicalTransform`, the same value
  `visual_transform`, `collision_transform` and `PartSocket::pose` return, so a
  weapon origin read out of an audit report cannot drift from collision
  (F11 non-negotiable behavior 4).
- **An empty audit is never a pass.** `RosterAuditReport::is_complete()`
  requires at least one container and one airframe, zero blockers and zero
  gaps. The retail run audits nine containers and zero airframes and is
  therefore explicitly not complete — which is the point.
- **`SceneGraph::subtree` is now single-owner.** `import_airframe` in
  `cs_app` and the audit in `cs_content` used to answer "which nodes are under
  this root" in two places. The content layer owns the answer; the app asks.

## What the retail run measured

`$CS_GAME_DIR`, production discovery, both production GameZ readers, the
production `install_file_key` normalizer and `AirframeRoster::audit`
(`private/evidence/F11-D/roster-census.json`, hashed by
`docs/findings/evidence/F11-D.json`):

| container | `node_array_size` | `nodes_offset` | present meshes |
| --- | --- | --- | --- |
| `zbd/c1/gamez.zbd` | 7,064 | 4,326,296 | 2,237 |
| `zbd/c1b/gamez.zbd` | 5,603 | 1,924,148 | 1,305 |
| `zbd/c1c/gamez.zbd` | 5,644 | 1,964,684 | 1,518 |
| `zbd/c2/gamez.zbd` | 4,956 | 3,111,828 | 1,765 |
| `zbd/c2b/gamez.zbd` | 4,901 | 1,658,700 | 1,365 |
| `zbd/c3/gamez.zbd` | 5,408 | 3,661,748 | 1,901 |
| `zbd/c4/gamez.zbd` | 8,289 | 5,107,144 | 2,431 |
| `zbd/c5/gamez.zbd` | 11,438 | 5,259,292 | 2,851 |
| `zbd/planes.zbd` | 3,317 | 4,881,228 | 1,766 |
| **total** | **56,620** | | **16,139** |

Each `nodes_offset` was cross-checked against the value the pinned mech3ax
v0.6.0 reference records for that archive, and each `node_array_size` /
`nodes_offset` pair was read twice — once by `read_gamez_meshes` and once by
`read_gamez_materials` — so the census cannot be a reader disagreeing with
itself.

The verdict: **9 containers audited, 9 blocked, 0 roots mapped, 0 parts, 0
mounts and 0 cockpit bindings mapped, 0 airframes discovered, 56,620 stored
node records undecoded.** `is_complete() == false`.

Two things follow from the table, and both matter for the task title
(*"including mission-only types"*):

1. The shared airframe archive is the **smallest** scene corpus in the
   installation: `planes.zbd` holds 3,317 stored node records against 53,303 in
   the eight per-chapter `gamez.zbd` archives — a ratio of more than 16:1. Any
   airframe that appears only in missions lives in one of those per-chapter
   containers, not in the shared archive, so a roster audit that only looked at
   `planes.zbd` would miss the majority of the airframe-like scene data in the
   game. That is why the audit's unit of coverage is the *container set*, not
   one container, and why the census is nine rows.
2. The per-chapter containers are mission/world scenes, so nothing in the
   table separates "an airframe a mission spawns" from "a world prop". That
   separation needs the node names, which need the node array.

## Test inventory

| `accept_f11_d_*` test | Covers | Fails when |
| --- | --- | --- |
| `roster_audit_maps_every_root_part_mount_and_cockpit_binding` (cs_content/tests/scene.rs) | **AC04's mapping arm**: two containers (one converted, one whose node array is undecoded) and five roster rows. Both reachable roots are mapped with their exact socket sets in stable-id order, per-role counts for all seven roles, the gun's zone/animation/provenance, the gun's pose equal to `world_transform`/`collision_transform`/`PartSocket::pose`, the container census and its `UnmatchedRule` gap, the blocked container's measured facts quoted in its `Display`, `alpha` complete (every declared role bound, availability evidenced, no gaps), `beta` mapped but **not** selectable with `AvailabilityUndiscovered` + `ForcedAssignmentOnly` + its pod's `UnknownRole`, the three airframe blockers (`ContainerUndecoded` with the container's numbers, `RootUndiscovered`, `RootMissing`), the report totals, and the roster's own two-directional assignment queries | sockets are bound by position instead of identity, a mapped socket loses its provenance/zone/animation/pose, an unmeasured role is mapped instead of reported, a forced assignment is promoted to selectable, a blocker drops the container's numbers, a blocked airframe is reported as mapped, a missing root falls back to another root, or the report totals stop reconciling |
| `roster_audit_reports_each_shortfall_instead_of_a_pass` | **AC04's negative arm**: a container whose header declares 99 stored node records where three were decoded (`NodeCountMismatch`), a socket with an evidenced gameplay role but an unevidenced collision role (`UnknownRole` quoting the *unresolved* claim, not the rule's), two `MissingRole` gaps for roles nothing bound, an airframe in a container the audit never covered (`ContainerNotAudited`), an airframe in a container whose conversion was refused (`ContainerUndecoded` carrying the refusal), and the totals with `mapped_socket_count() == 0` | a partial decode rounds up to a pass, an unevidenced role is defaulted, a required role is silently satisfied, an uncovered container is skipped, a refusal is swallowed, or any of these still reports `is_complete()` |
| `roster_records_refuse_contradictions` | construction-time refusals: a non-airframe row, a non-mission assignment, a non-airframe assignment, a role required twice, the same airframe audited twice, the same assignment recorded twice, two missions forcing one airframe (the normal case, accepted), a mission forcing an airframe **no row audits** (refused), and an empty roster auditing nothing | a roster accepts a contradiction, an assignment can name a plane outside the roster, or an audit of nothing reads as complete |
| `retail_the_private_installation_roster_audit_flags_every_container` (`#[ignore = "requires CS_GAME_DIR"]`) | **AC04 over the real installation**: production discovery, both production GameZ readers, the nine-archive census, the reference `nodes_offset` cross-check, the 56,620-record total, the mission-only ratio, and the blocked verdict with every container's measured facts | the corpus changes, a reader mis-walks a container, the two readers disagree, the audit invents a mapping, a blocker loses its numbers, or the report claims completeness |

The evidence harness (`evidence_report_f11_d_writes_the_acceptance_report`,
deliberately **not** named with the task prefix) derives
`private/evidence/F11-D/acceptance.json` and `roster-census.json` from the
recorded acceptance log, production discovery of `$CS_GAME_DIR` and the same
production census; it refuses a stale `CS_CANDIDATE_TREE` and a missing log,
and writes a failing report when the acceptance run failed.

## Sensitivity check (mutations applied and reverted while implementing, all
in the same session; every message below is copied from the actual run)

| Mutation | Result |
| --- | --- |
| the audit ignores the graph source's typed error and reports a mapping anyway | the retail test fails: `assertion 'left == right' failed  left: 9  right: 0` on `report.mapped_containers().count()`; the two synthetic tests also fail, and where a roster row still points into that container the audit trips its own consistency check (`an audited container with no blocker was produced from a graph: NodeArrayUndecoded { … stored_nodes: 733, nodes_offset: 9216 }`) rather than reporting a pass |
| `ContainerBlocker::NodeArrayUndecoded`'s `Display` stops quoting the measured numbers | the retail test fails: `zbd/c1/gamez.zbd: the blocker must quote the measured facts, got install_file/zbd_2f_c1_2f_gamez.zbd has an undecoded node array`; the synthetic mapping test fails on the same text |
| `is_complete()` drops its "something was actually audited" clause | `roster_records_refuse_contradictions` fails: `an audit that looked at nothing must not read as a pass`. The retail test does **not** fail under this mutation, because its nine container blockers already keep `is_complete()` false — recorded here rather than claimed as coverage |
| a forced mission assignment marks the airframe selectable | `roster_audit_maps_every_root_part_mount_and_cockpit_binding` fails: `a forced mission assignment is not proof of selectability` |
| `UnknownRole` cites the *rule's* claim instead of the unresolved one | `roster_audit_reports_each_shortfall_instead_of_a_pass` fails on the gap list: left `claim_id: ClaimId("f11a.test.binding"), reason: "unresolved"`, right `claim_id: ClaimId("f11d.test.odd-collision-unmeasured")` with the real reason |
| the required-role check is dropped | `roster_audit_reports_each_shortfall_instead_of_a_pass` fails: the expected `MissingRole { … Cockpit }` and `MissingRole { … Gun }` are absent from the list |
| `NodeCountMismatch` is dropped | `roster_audit_reports_each_shortfall_instead_of_a_pass` fails: `left: []`, `right: [NodeCountMismatch { container: "fix_partial", declared: 99, decoded: 3 }]` |
| `MappedSocket::pose` takes the node's *local* transform | **this mutation initially did not fail** — the first fixture had identity transforms on every parent, so local and world coincided. The fixture now mounts both gun mounts under a rotated, offset wing, and the test asserts the two poses differ; the mutation then fails on `the fixture mounts the gun under a rotated, offset wing, so a socket that copied the local transform would differ from the composed pose` |
| `SceneGraph::subtree` ignores the root and returns the whole container | `roster_audit_reports_each_shortfall_instead_of_a_pass` fails (`left: 2  right: 3` node count) and `roster_audit_maps_every_root_part_mount_and_cockpit_binding` fails (`seven on alpha, three on beta — left: 20  right: 10`), because the second root's sockets leak into the first airframe |

The tests are not vacuous. Two honest notes on the coverage: the empty-roster
clause of `is_complete()` is covered by the synthetic test only, and
`mapped_root_count()` is covered by the synthetic mapping test only (with every
retail container blocked there is no mapping to count, so a mutation there is
indistinguishable from the original).


## Unknowns and limitations (all recorded, none guessed)

- **No production path decodes a GameZ node array**, so the roster audit maps
  no root, part, mount or cockpit binding from the original installation: the
  nine measured GameZ archives declare **56,620** stored node records and none
  of them is decoded. **Affected content:** every airframe in the game, the
  shared roster in `planes.zbd` and every mission-only airframe in the
  per-chapter `gamez.zbd` archives. **Resolving task:** **#392** "Read the
  GameZ node array into `ParsedNode` records" (needs the owner to grant
  `crates/cs_formats/` owner paths). This limitation gates every
  scene-hierarchy and roster fidelity claim and **survives this task being
  marked done**.
- **No airframe catalog element has been discovered from original data**, so
  the roster the audit takes is empty and no roster row can be audited. A
  model name in an archive is not a discovered airframe, and a file on disk is
  not a roster row. **Affected content:** the player-selectable roster, the
  forced mission assignments, and every airframe's mount and cockpit bindings.
  **Resolving task:** the roster-discovery follow-up filed with this stage
  (**#399**, "Discover the airframe roster and its selectability from original
  data", depends on #392); it needs a decoded node array before a node name
  can be bound to an `airframe/<key>` element.
- **Roster availability is a designed vocabulary with no measured original
  meaning.** Which modes let a player choose which airframe has not been
  observed, so `RosterAvailability` carries a value only where some discovery
  recorded one, and `MissionOnly` is an engine-authored label rather than a
  transcription of a mode's selection list. **Affected content:** the
  selectable roster in every mode. **Resolving tasks:** the F49 instant-action
  preset and F22 mode stages, together with roster discovery.
- **`node_array_size` versus the number of node records is *ObservedTool*
  evidence, not a measured original fact.** `SceneGraph` holds one node per
  stored record, and `AuditGap::NodeCountMismatch` compares the two counts; the
  correspondence comes from the pinned reference's header comments, so the
  check can flag a partial decode but must not be read as proof that a mismatch
  always means one. **Affected content:** every converted container. **Resolving
  task:** #392, and the re-measurement that follows it.
- **The audit is a library path.** `AirframeRoster::audit` is exercised by the
  acceptance suite and by the evidence harness's consumer trace, but nothing in
  the running binary and no `cs-inspect` subcommand invokes it yet, so there is
  no reachability evidence beyond the tests. **Affected content:** the audit's
  own reachability. **Resolving task:** **#398** (F11-E) "Insert the airframe
  scene request from the real producer", which should refuse an airframe the
  audit could not map, and a `cs-inspect` subcommand when the owner wants one.
- **Nothing here says which authored node is a gun, an engine, a control
  surface, a camera anchor, a damage zone or a cockpit in the original game.**
  The role vocabulary and the name-path binding mechanism are designed engine
  contracts (F11-A); the audit checks that a *declared* mapping is complete and
  honest, and cannot supply the mapping. **Affected content:** every part
  binding of every airframe. **Resolving task:** the F29 zones/armor/system
  disablement stage together with roster discovery over a decoded node array.
- **The node flag bits, the `zone_id` domain and the meaning of a LOD record's
  `level` remain unmeasured** (inherited from F11-A/F11-B and unchanged here).
  The audit carries `zone_id` through as raw data and never interprets it.
  **Resolving task:** the evidence stage that reads the flag bits.
- **Fixture scope** — the three unignored tests are synthetic and newly
  authored. Only the `#[ignore]`d retail test reads original data, and nothing
  derived from it beyond counts, offsets and digests is committed.

## Sources used

- `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md` (F11-D section,
  the deliverable, non-negotiable behaviors 1–5, AC01–AC04).
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, provenance, explicit
  unknowns, one pose owner, cycles invalid in ownership hierarchies, catalog
  rows that may not exclude failed entries).
- `docs/contracts/CLI-EVIDENCE.md` and `schemas/evidence.schema.json` (the
  evidence record and its validator).
- `docs/findings/2026-09-30-f11-c-part-socket-and-damage-visual-wiring.md`
  (the socket table this stage maps, and the recorded #392 blocker).
- `docs/findings/2026-09-29-f11-b-hierarchy-import-and-lod-selection.md` and
  `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md` (the hierarchy,
  the id scheme, the binding mechanism and the recorded unknowns).
- `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` (the 40-byte header,
  `node_array_size`/`nodes_offset`, and the reference's recorded per-archive
  offsets, which the retail census cross-checks).
- `cs_formats::gamez` (the two production readers used for the census),
  `cs_assets::install` (production discovery and the two installation
  fingerprints) and `cs_content::catalog::baseline::install_file_key` (the
  production container-key normalizer).
