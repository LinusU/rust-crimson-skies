# F15-E: AC04 over a mission dependency closure, and how narrow that closure still is

Date: 2026-10-14. Task #402 `F15-E`, the follow-up to #64 `F15-D` recorded in
`docs/findings/2026-09-30-t64-f15-d-cold-warm-restart.md` and in
`docs/findings/2026-10-02-f15-e-mission-closure-blocked.md` (spec
`specs/F15-asynchronous-asset-loading-and-private-cache.md`, AC04; the stage
itself is defined by the task description, which has no `F15-E` section in
the spec). Capability used: `retail` (read-only `$CS_GAME_DIR`). No `gpu`,
`audio`, `human_play` or `human_review` claim is made, and nothing is marked
`verified_original`.

## Outcome

Spec F15 **AC04** — *warm and cold loads produce equal content hashes and
gameplay state* — is now measured on the reference installation over **every
campaign mission's dependency closure**, resolved through the production
dependency-closure algorithm of `docs/contracts/IDENTITY-CONTENT.md` rather
than a hand-picked asset list. All 24 campaign missions pass the full
cold/warm/restart cycle: identical per-item payload digests, identical
`ReadyBundle::closure_hash` and identical gameplay state in all three
passes. The eight-world texture-archive result of F15-D still passes
unchanged (re-run in this stage's session, 6/6 tests).

The stage also measures, and names here, how narrow that closure still is:
on this installation every mission closure reaches exactly **three catalog
nodes over one unique byte source**, none of them ready, none with a runtime
consumer. That is a measured fact of today's product state, not a defect of
this measurement; the resolving work is named at the end.

## What was measured (input fingerprints)

Reference installation `$CS_GAME_DIR`:

* `install_sha256`:
  `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
  (production `cs_assets::install::fingerprint`, re-checked by the baseline
  builder and discovery in the evidence harness);
* `content_sha256`:
  `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`
  (production `content_fingerprint`);
* reference mission `mission/ch1-m01`, catalog closure hash
  `593a2312c560ccb7aa103f7081d431ac1ade44d4e144b5dfebe821a242a9ba48` —
  byte-identical between the in-process production walker and the
  `cs-inspect closure` CLI report, and identical to the hash the 2026-10-02
  block recorded, so the closure's identity is stable across the F14-D
  collection landings;
* per-mission catalog closure hashes and per-item payload digests for all 24
  missions are carried in the committed evidence artifact
  `docs/findings/evidence/F15-E.json` (its `cycle.json` contents), measured
  on the candidate tree recorded there.

## What the stage does

`crates/cs_assets/tests/accept_f15_e_mission_closure_cold_warm_restart.rs`:

1. **Resolution (AC1).** Every launchable campaign mission's closure is
   computed by `cs_content::catalog::retail_baseline` +
   `Closure::compute` — the production catalog builder and the
   `IDENTITY-CONTENT` closure walker that the production `cs-inspect
   closure` CLI runs — and the reference mission's closure is cross-checked
   against the CLI's own report (hash string, every node id, node count).
   Every reached node's bytes are then resolved through the mounted
   original installation (the designed baseline's install mount,
   `SessionBuilder::mount_installation`'s install half) and the resolved
   member, offset and length are asserted equal to the catalog row's own
   origin span, so the bytes loaded are the bytes the closure names.
2. **The cycle (AC2).** Per mission, the reached spans are loaded three
   times over one private store through the real production path —
   `LoadingSession` over `CacheStore`, pumped item by item, converting with
   the real reader-archive structural decode
   (`common::derive_reader_archive_listing`: F06 two-key dispatch →
   version-one trailer index → reader-archive reader, the pipeline the
   F13-B discovery uses; converter identity `zbd-reader-archive-listing`
   1/IR 1) — cold against an empty store, warm against the re-opened store
   (required to be served *entirely* from verified entries) and again after
   a second close/re-open. All three passes must deliver equal payload
   digests, an equal `ReadyBundle::closure_hash` and equal gameplay state
   (the `(content id, asset key)` binding each delivered entity presents,
   read back out of a real Bevy `World`).
3. **Naming (AC3).** The suite prints, per mission and per closure node,
   whether the node's bytes were delivered and at which level, or why it is
   not served, and asserts the accounting is total. A node with no runtime
   consumer is printed as a **finding**, never counted as a pass.

`crates/cs_assets/tests/evidence_report_f15_e.rs` (not part of the task
selection) re-measures the cycle independently through the production
`CacheStore`, re-derives the closure with the production walker and writes
`private/evidence/F15-E/acceptance.json`; a copy is committed as
`docs/findings/evidence/F15-E.json`.

## The closure as measured, and the members named as not served

On this installation each of the 24 campaign mission closures reaches
exactly:

* `mission/ch<chapter>-m<nn>` — the mission row;
* `script/<world>-m<nn>-zrdr` — its reader-archive program row;
* `install_file/<archive>` — the install-file row.

All three nodes name the **same bytes**: the mission's own reader archive
`ZBD/<world group>/<M nn>/zrdr.zbd` (91 171 bytes for M01). The three
nodes are therefore deduplicated into one load item per mission, headed by
the install-file row's identity; the deduplication is measured from the
rows' own origin spans and recorded per node.

Named, never silently skipped (this is the complete list of closure members
the stage cannot serve at their own kind level; all 72 nodes, 24×3, are
reported in the test output):

* **Every mission-kind node** (`mission/...`): no mission-program decoder
  with a runtime consumer exists; the row is `unparsed`, `not_normalized`
  and `missing_runtime_consumer` in the catalog. The bytes are served at
  the container-structural level only.
* **Every script-kind node** (`script/...`): the reader family's entry
  encodings are `Undeclared` in the committed research
  (`cs_formats::zbd::reader_archive`), so member *content* is retained
  verbatim and not decoded; the derived asset is the archive's member
  index, not an interpreted program. The row is likewise
  `missing_runtime_consumer`.
* **Every node of every closure** has an empty `runtime_consumers` list and
  is catalog-`unready`; `is_retail_ready` stays `false`, and this stage
  awards no readiness.
* **The closure does not reach the populated content collections.** The
  F14-D collections are now in the catalog (88 496 rows, 20 kinds — worlds,
  scene nodes, meshes, materials, images, sounds, airframes, factions,
  paint masks, stunts, scrapbook items, video, engines, guns, multiplayer
  rules), but mission rows still carry exactly one static edge — to their
  program row — so `cs-inspect closure --mission …` reaches no world,
  geometry, audio or UI content. The mission→content dependency edges and
  the script adapter's load-command candidate sets are what would widen
  this closure.

## What resolves the remaining gaps (not this stage)

* Measured mission→content static dependency edges (the campaign bindings
  already name each mission's world group as a critical dependency; wiring
  that, and other measured dependencies, into the catalog's mission rows is
  catalog-baseline work following F50-B `#205` / M01-B `#259`).
* The script adapter's conservative dynamic candidate sets: the F07-D
  classification data exists (`docs/findings/2026-09-28-f07-d-retail-opcode-classes.txt`)
  but the production `LoadCommandTable` stays caller-supplied and empty, so
  the adapter expands nothing.
* A mission-program decoder with a runtime consumer (the F37/F38 line) is
  what would turn the `script`/`mission` nodes into served content.

Until those land, the measured closure of a mission is its reader archive
alone; this stage measures AC04 over exactly that, names the boundary, and
gates no fidelity or release claim.

## Consumer trace of the measured load

`cs-inspect closure` (CLI, cross-checked) → `cs_content::catalog`
(`retail_baseline` + `Closure::compute`) → per-span `LoadItem` keyed in the
`install` namespace by the row's own container path, content id the
catalog's node id → `ContentSession` (designed install mount, resolved span
asserted equal to the row's origin span) → `SessionIo` →
`cs_formats` reader-archive pipeline (`derive_reader_archive_listing`) →
`CacheStore` (`CacheKey` = installation hash + resolved span +
`zbd-reader-archive-listing` 1/IR 1) → `ReadyBundle::closure_hash` →
`LoadedItemBinding` entities in a Bevy `World`.

## Evidence

* `docs/findings/evidence/F15-E.json` — the committed evidence report
  (`claim: implemented`, capabilities `["retail"]`, validated with
  `tools/validate_evidence.py --artifact-root private/evidence/F15-E
  --require-pass`); its `cycle.json`-derived artifact carries the per-node
  served/not-served accounting and the per-mission payload digests.
* `private/evidence/F15-E/` (outside Git) — the acceptance log, the
  `cycle.json` artifact and the working cache the harness used.
* Task selection: `cargo test --workspace --locked -- accept_f15_e_
  --include-ignored` executes
  `accept_f15_e_mission_closures_resolve_through_the_production_closure_and_cli`
  and
  `accept_f15_e_retail_mission_closure_cold_warm_restart_deliver_equal_content_and_gameplay_state`,
  both passing; a non-selection smoke guard
  (`f15_e_derive_reader_listing_reads_the_reference_mission_program_archive`)
  pins the production reader pipeline on the reference archive (12 members).

## Sources

* Task #402 (`F15-E`) description and history, including the 2026-10-02
  block and the owner's 2026-10-05 unblock with F14-D.3..D.8 as
  dependencies (all landed).
* `docs/contracts/IDENTITY-CONTENT.md` (dependency-closure algorithm),
  `docs/contracts/CLI-EVIDENCE.md` (evidence record).
* `docs/findings/2026-09-30-t64-f15-d-cold-warm-restart.md`,
  `docs/findings/2026-10-02-f15-e-mission-closure-blocked.md`,
  `docs/findings/2026-09-28-f07-d-retail-opcode-classes.md`.
* Production output of `cs-inspect closure` and `cs-inspect catalog` over
  `$CS_GAME_DIR` on this branch (closure report and catalog JSON measured
  in `private/` scratch, ids and hashes only).

## Review additions (2026-10-14)

The reviewer regenerated the evidence report on the rebased, reviewed commit
(`docs/findings/evidence/F15-E.json`, `candidate_tree` matching that tree) per
`docs/contracts/CLI-EVIDENCE.md`; the re-measured closure hashes and payload
digests were identical to the implementer's run, confirming the measurement is
stable across the rebase. The reviewer also added a **degenerate-closure guard**
to the resolution test: every campaign mission's closure must still reach a
`mission`, a `script` and an `install_file` node, so a closure walker that
stopped expanding the mission's edges can never make this stage measure the
launchable root alone and pass vacuously. The three reached kinds are a lower
bound, not an exact count, so the mission-to-content edges that F50-B / M01-B
will add are not blocked by it.
