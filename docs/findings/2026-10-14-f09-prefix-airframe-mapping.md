# F09-PREFIX: the livery prefix to airframe mapping, measured from the GameZ scene records

Date: 2026-10-14. Task: #386 (key `F09-PREFIX`) "Establish the original
prefix-to-airframe mapping from original data". Feature sheet:
`specs/F09-bm-multilayer-liveries-and-paint-composition.md` (the F09-D
limitation this resolves) and
`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md` (whose node
array, #392, and roster discovery, #399, this builds on). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`; evidence contract
`docs/contracts/CLI-EVIDENCE.md`. Test prefix: `accept_f09_prefix_`.
Capabilities used: **`retail`** (read-only `$CS_GAME_DIR`) plus ordinary
build/test. Evidence: `docs/findings/evidence/F09-PREFIX.json`; private
artifacts stay in `private/evidence/F09-PREFIX/`.

This closes the F09-D boundary "Prefix -> airframe": the eleven stock-livery
prefixes were file-name stems of `GRAPHICS/<FACTION>/<PREFIX>_<PART>.bm` and
nothing in the original data had been read to say which airframe each names
(F09 non-negotiable #3). All eleven now resolve, each from a measured chain
inside the shared aircraft archive `ZBD/planes.zbd`.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/livery_airframes.rs` (new, the production path):
  `CLAIM_PREFIX_AIRFRAME`, `CLAIM_PREFIX_UNRESOLVED`, `AirframeCandidate`,
  `PrefixRootEvidence`, `PrefixAirframeMapping`, `PrefixAirframeMap`,
  `PrefixAirframeError`, `resolve_prefix_airframes` and the private
  `match_head`/`Observation` walk.
- `crates/cs_content/src/lib.rs` (wiring only): `pub mod livery_airframes;`.
- `crates/cs_content/tests/livery_airframes.rs` (new): the synthetic section
  builders, the three synthetic `accept_f09_prefix_*` tests, the `#[ignore]`d
  retail acceptance test, the evidence harness
  `evidence_report_f09_prefix_writes_the_acceptance_report` (deliberately not
  named with the task prefix) and the harness helpers.
- `docs/findings/evidence/F09-PREFIX.json` (this stage's evidence copy) and
  this file.

**One observable failure:** before this stage no production path could say
which airframe `KES_` (or any other prefix) names; a consumer could only
guess from name similarity, which AGENTS.md rule 4 forbids. With this stage,
`accept_f09_prefix_retail_every_stock_prefix_resolves_to_its_declared_airframe`
fails on the first prefix that stops resolving, resolves to a different
airframe, moves its evidence counts or anchors its span anywhere but the
container record that spells the prefix, and the synthetic
`accept_f09_prefix_resolves_by_contained_evidence_and_reports_the_span` fails
on the first change to the matching or containment rules that CI can run.

## Design decisions

- **The chain is one measured walk, every link a production reader's own
  fact.** Candidate root node (`read_gamez_nodes`) → subtree child links →
  `mesh_index` → mesh record (`read_gamez_meshes`) → stored material
  references → material record → texture-name table entry
  (`read_gamez_materials`). No link is derived from a file name or from name
  similarity, which is exactly what the F09-D limitation forbade.
- **Both inputs are caller data, and the engine ships no idiom.** The
  candidate airframes are `AirframeCandidate`s (the retail test builds them
  from the production roster discovery, `airframe_roster_declarations` +
  `discover_airframe_roster`, over `ZBD/interp.zbd`); the prefix list is
  caller data too (the retail test takes it from the production
  `StockLiveryCatalog::discover` over `GOSDATA/ASSETS/crimson.rof`). A wrong
  idiom produces no mappings, which is falsifiable; a shipped table would be
  a claim with no provenance. This follows the roster discovery's own
  precedent (`docs/findings/2026-10-02-f11-d-2-airframe-roster-discovery.md`).
- **The matching rule is the stock livery's own separator.** A container
  texture name matches a prefix when its ASCII-case-folded spelling starts
  with the folded prefix plus `_` — the `<PREFIX>_<PART>` separator
  `parse_livery_spelling` already uses. The fold is a designed bridge between
  the two measured spellings (uppercase in the BM library, lowercase in the
  container's texture table); the separator is what keeps `firebrand_…` from
  matching `fir`, `brig_dash` from matching `bri`, `ke_damage` from matching
  `kes` and `warhawk_canopy` from matching `war`. A caller list with a folded
  collision is refused (`DuplicatePrefix`), never silently merged.
- **The resolution rule is evidence containment, never position or
  plurality.** A prefix resolves to the unique naming candidate whose set of
  distinct matched texture names is a **superset of every other naming
  candidate's set**. Equal sets or incomparable sets leave no unique such
  candidate and stay `Resolved::Unknown` with a reason that names every
  naming candidate and how much exclusive evidence each has. The rule is a
  **Designed decision procedure over measured evidence** (the identity value
  itself is always measured); it never breaks a tie by position, count of
  references or name similarity. Raw reference counts deliberately do not
  decide: containment runs over distinct names, so a texture referenced by
  many meshes cannot outweigh exclusive evidence.
- **Both candidate idioms are refused, not repaired.** A candidate whose
  slot is past the node array (`CandidateNodeMissing`), whose stored node
  name disagrees with the declared one (`CandidateRootNameMismatch`), or
  that claims a slot another candidate holds (`DuplicateCandidateRoot`) is a
  caller error with its own variant. The retail test locates each declared
  root by the production `escape_scene_node_name` of its stored name, so the
  F11-E1 escaping cannot silently misaddress a candidate.
- **A resolved mapping's span is the bytes that spell the prefix.** The
  `SourceSpan` covers the 44-byte texture-name record of the winning
  candidate's matched entry with the **smallest container offset**
  (deterministic), carrying the installation fingerprint and the container's
  own content hash as `member_sha256`, class `observed_tool`. The identity
  travels in the `Resolved::Known` value, the losing candidates in
  `PrefixAirframeMapping::naming`, so nothing is hidden by the single span.
- **Unresolvable links are counted, never skipped.** A `mesh_index` past the
  mesh array or an all-zero slot raises `unresolved_meshes`; a material
  reference past the material table raises `unresolved_materials`; a child
  slot past the node array is counted the same way. A cyclic or
  self-listing child structure cannot loop the walk (visited set) and cannot
  double-count evidence.
- **The scope is the declared airframes of the shared archive.** The 28
  roots of `ZBD/planes.zbd` include the eleven bare-named scene variants the
  same script loads a second time (`bloodhawk`, `kestrel`, …) plus
  non-airframe roots (`rope_ladder`, `chuteman`, …). F11-D2 established
  that those bare variants are scene airframes, **not** declared airframes,
  so they are not candidates; the map answers "which declared airframe does
  this prefix name", which is the F09-D question. See the limitation below
  about a caller that offers them anyway.

## The measured mapping

Production resolution over `$CS_GAME_DIR`, installation fingerprint
`b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`,
container `ZBD/planes.zbd` content hash
`45da54a8e1886a8182e84bef03eb5e099481e483d79e458e7db5356538fbc21b`.
The artifact `prefix-airframe-map.json` (hashed by the evidence record)
carries every row, evidence count and anchor; **no original texture name is
committed anywhere**, only ids, counts and offsets.

| Prefix | Resolved airframe | Root slot | Distinct names | Referencing meshes | Anchor (offset+length) | Other naming candidates (distinct names) |
| --- | --- | --- | --- | --- | --- | --- |
| `AGYRO` | `airframe/player_autogyro` | 19 | 12 | 22 | 5672+44 | `player_fury`: 1, `player_avenger`: 1 |
| `BAL` | `airframe/player_balmoral` | 25 | 11 | 35 | 9544+44 | — |
| `BLO` | `airframe/player_bhawk` | 1 | 8 | 28 | 2724+44 | — |
| `BRI` | `airframe/player_brigand` | 632 | 14 | 25 | 4924+44 | — |
| `DEV` | `airframe/player_pfighter` | 44 | 14 | 31 | 1316+44 | — |
| `FIR` | `airframe/player_fbrand` | 41 | 9 | 20 | 3868+44 | — |
| `FUR` | `airframe/player_fury` | 39 | 10 | 26 | 5760+44 | — |
| `HEL` | `airframe/player_avenger` | 23 | 7 | 29 | 6948+44 | — |
| `KES` | `airframe/player_kestrel` | 14 | 9 | 27 | 7344+44 | — |
| `PEA` | `airframe/player_peacemaker` | 10 | 10 | 26 | 8268+44 | — |
| `WAR` | `airframe/player_warhawk` | 635 | 10 | 29 | 8884+44 | — |

All eleven prefixes resolve; the map is total over the measured library and
injective over the eleven airframes. Every `unresolved_meshes` and
`unresolved_materials` count in the whole run is **0**, which is the corpus
cross-check the F14-D node-to-mesh verdicts also report for this container.

Two measured facts worth stating:

- **The one cross-reference is `AGYRO`.** Its cockpit texture (one distinct
  name, one referencing mesh) is also referenced by `player_avenger` and by
  `player_fury` — the only case where a prefix's texture appears under a
  second declared airframe. Both carry strictly less evidence than
  `player_autogyro`'s twelve names, so containment resolves uniquely and the
  retail test pins all three numbers so the leak cannot appear, move or grow
  silently. The other ten prefixes name exactly one declared subtree.
- **`DEV` resolves through the nested root.** `player_pfighter` is not
  parentless (the build script attaches it under `player`), but its subtree
  is its own and carries all fourteen `dev_` names; the resolution walks a
  subtree, not a forest root, so the F11-D2 measured nesting is no obstacle.

## Tests

| Test | Covers | Fails when |
| --- | --- | --- |
| `accept_f09_prefix_resolves_by_contained_evidence_and_reports_the_span` (synthetic, CI) | containment resolution with its span (offset 100, length 44, both hashes, claim id, `observed_tool`); equal evidence → unknown with both candidates named; incomparable evidence → unknown naming each side's exclusive count; a lone naming candidate; an absent prefix → unknown naming the folded head; the per-candidate evidence counts; the prefix lookup's case folding | the containment rule changes, a tie is broken, the anchor is not the smallest matched record, an absent prefix stops being unknown, or the evidence counts move |
| `accept_f09_prefix_walk_is_cycle_safe_and_counts_unresolvable_links` (synthetic, CI) | a duplicate child slot and a back edge walked once; a mesh index past the array and a material index past the table each counted once; the readable evidence still resolves | the visited guard disappears, an unresolved link is silently skipped, or a resolvable chain stops resolving |
| `accept_f09_prefix_refuses_inconsistent_caller_data` (synthetic, CI) | every `PrefixAirframeError` arm a corpus can produce: slot past the node array, stale root name, doubly claimed slot, folded duplicate prefix, empty prefix | a refusal becomes a silent repair or a wrong mapping |
| `accept_f09_prefix_retail_every_stock_prefix_resolves_to_its_declared_airframe` (`#[ignore]`, retail) | the whole table above with the production roster discovery as candidates and the production stock-livery catalog as prefixes; every span's hashes, container and covered bytes spelling its prefix; the pinned distinct-name counts; the AGYRO cross-reference triple; zero unresolved links; total and injective resolution | the corpus changes, a prefix stops resolving or resolves elsewhere, a span moves off the record that spells the prefix, the candidate idiom stops finding a root, or the leak is repaired |

Without `$CS_GAME_DIR` the retail test panics rather than passing; CI runs
the three synthetic tests. The task selection
`cargo test --workspace --locked -- accept_f09_prefix_ --include-ignored`
executes four tests, all passing, on this branch.

**Sensitivity check.** Six mutations applied, run and reverted; each is
killed by a test **CI can run** except where noted:

| Mutation | Killed by |
| --- | --- |
| the `_` separator dropped from the match head | `…reports_the_span` (the WAR ambiguity arm: `warhawk_canopy` starts matching) |
| the prefix not folded into the match head | `…reports_the_span` and `…unresolvable_links` (no lowercase texture name matches an uppercase head) |
| the containment winner rule collapses to "the first naming candidate" | `…reports_the_span` (KES/WAR/FUR arms) |
| the anchor keeps the largest matched offset instead of the smallest | `…reports_the_span` (the KES anchor) |
| unresolved links not counted | `…unresolvable_links` |
| the candidate root-name check removed | `…inconsistent_caller_data` |

No probe text remains in the tree.

## Unknowns and limitations (all recorded, none guessed)

- **The containment rule is a designed decision procedure.** The measured
  corpus needed it only for the `AGYRO` cockpit cross-reference, where the
  losing candidates' evidence is a strict subset of the winner's; every
  other prefix names one subtree only. A future container whose evidence is
  genuinely equal or incomparable resolves to `Unknown` by construction.
  **Affected content:** any prefix map over a corpus with shared textures
  between incomparable airframes. **Resolving task:** none required for this
  corpus; a container that produces an unknown reports it with both sides'
  evidence rather than a guess.
- **The candidate scope decides the twins, and it is the caller's.** The
  bare-named scene variants (`bloodhawk`, `kestrel`, …) reference the same
  prefix families as their `player_` twins with largely equal evidence; a
  caller that offered both as candidates would get `Unknown` (equal
  evidence) for those prefixes rather than a wrong winner. This stage scoped
  candidates to the **declared** airframes because the roster discovery
  measured that the bare variants are scene airframes, not declared ones.
  **Affected content:** which scene root a runtime actually paints when the
  same model is instantiated twice. **Resolving tasks:** F11-C/F11-E
  hierarchy import plus the F17-B renderer; the identity question F09-D
  asked is answered regardless.
- **The map covers the shared aircraft archive only.** Mission-only
  airframes in the eight per-chapter `gamez.zbd` containers are not
  candidates because no roster declares them yet (the F11-D2 recorded
  boundary, still open for the `LoadGameGen <model> <alias>` idiom).
  **Affected content:** a stock livery for a mission-only airframe, if the
  library ever holds one — the measured eleven prefixes all resolve in
  `ZBD/planes.zbd`. **Resolving task:** the follow-up per-chapter roster
  discovery stage.
- **The per-part association is not established here.** The BM members name
  parts (`KES_WING`, …) and the container names textures per surface; this
  stage binds a **prefix** to an **airframe**, which is what the F09-D
  limitation asked. Which scene node a part name paints is still open — the
  walk in this module is the instrument for it (texture name → mesh slot →
  node), but a binding rule and its consumer are F17-C's follow-up wiring,
  and nothing here claims them. **Affected content:** every painted
  multi-part surface. **Resolving tasks:** F17-C's recorded
  `livery_surface_association` boundary plus its wiring.
- **The anchor span covers the whole 44-byte texture-name record**, not only
  the name field inside it; the record is the smallest addressable original
  unit this reader exposes (`GameZTextureName::data_offset`/`data_end`), and
  the retail test checks the covered bytes contain the folded prefix.
- **Evidence class.** The layout is the pinned reference's (mech3ax v0.6.0,
  commit `d3521a9721be731d365504568ddcd78e3f9846bb`, [S02]/[S17]) and the
  mapping was measured from the original bytes through production readers,
  which is `observed_tool`. `retail` is file access: no original executable
  run happened and nothing here claims `verified_original`. The mapping
  closes the F09-D boundary that gated F09 non-negotiable #3 for stock
  liveries; livery **appearance** claims stay with F17-D as before.
- **Independent review is requested.** AGENTS.md asks for a different agent
  instance or model with a fresh context for format/mission semantics and
  fidelity-claim work; this stage qualifies and no agent review replaces
  the owner's approval.
- **Review round (2026-10-14).** Rally assigned the review of this branch to
  a fresh review session of `bunny-alpha-1` (model
  `opencode-go/mimo-v2.6-flash`): it started with no implementation context,
  but it is the **same agent name** as the implementer, so the
  different-instance preference above could not be satisfied here — recorded
  honestly, and no agent review awards more than `checked`. The reviewer
  reran two independent mutation probes (separator dropped from the match
  head; containment winner rule collapsed to the first naming candidate),
  each killed by a CI-runnable `accept_f09_prefix_` test; reran the four
  checks (fmt, clippy, the workspace suite, the task selection with
  `--include-ignored`, 4/4 green including the retail test); and
  regenerated `docs/findings/evidence/F09-PREFIX.json` on the reviewed tree
  per `CLI-EVIDENCE.md`. The regenerated `prefix-airframe-map.json` hashes
  identically to the implementing round's (`c850f33c…`), so the measured
  table above reproduced byte-for-byte on the reviewed commit.
- **Nothing derived from the original bytes is committed.** Ids, counts,
  offsets, hashes and this table's numbers; no texture name, no mesh, no
  screenshot.

## Sources used

- `specs/F09-bm-multilayer-liveries-and-paint-composition.md` (the
  non-negotiable #3 this serves) and the F09-D stage of the same sheet.
- `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md` (the node
  array and roster contracts this builds on).
- `docs/contracts/IDENTITY-CONTENT.md` (provenance, explicit unknowns,
  evidence-backed aliases, no positional resolution) and
  `docs/contracts/CLI-EVIDENCE.md` (the evidence record).
- `docs/findings/2026-09-29-f09-d-stock-liveries-and-combinations.md` (the
  measured prefix list and the boundary this resolves).
- `docs/findings/2026-10-02-gamez-node-array-layout.md` (#392: the node
  array, the mesh-slot rule and the `planes.zbd` forest).
- `docs/findings/2026-10-02-f11-d-2-airframe-roster-discovery.md` (the
  declared-airframe idiom and the bare-variant scoping this reuses).
- `docs/findings/2026-10-13-t593-f14-d-node-mesh-code.md` and
  `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` (the
  node-to-mesh-to-material verdicts the walk quotes).
- `crates/cs_formats/src/gamez/{nodes,reader,materials}.rs` and
  `crates/cs_content/src/{livery,scene}.rs`,
  `crates/cs_content/src/catalog/baseline.rs` (the production readers and
  idioms).
