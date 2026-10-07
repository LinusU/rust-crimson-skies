# F29-D.1: the measured airframe damage vocabulary and its declared shape

Date: 2026-10-07. Task: #521 (F29-D.1) "Bind the measured original airframe
damage-region vocabulary into F29's declared graph shape"
(`specs/F29-damage-zones-armor-destruction-and-bailout.md`, sections
`### F29-B` and `### F29-D`). Shared contract:
`docs/contracts/STATE-TRANSACTIONS.md`.
Capabilities used: `retail` (read access to `$CS_GAME_DIR`), so this task
produced an evidence report: `private/evidence/F29-D.1/acceptance.json`,
validated with `tools/validate_evidence.py --require-pass` and copied to
`docs/findings/evidence/F29-D.1.json`.

## Files and the one observable failure (listed before editing)

The owner paths were listed in the task description; recorded here in the
form the sheet asks for:

- `crates/cs_content/src/damage.rs` (extend): `AirframeContainerSource`,
  `AirframeRegionGroup`, `MeasuredDamageRegion`, `MeasuredWreckMaterial`,
  `DiscardedDamageName`, `AirframeDamageVocabulary`,
  `observe_airframe_damage_vocabulary`, `synthetic_airframe_damage_vocabulary`,
  `AirframeRegionShape`, `DeclaredDamageGraph::declare_airframe_regions`, four
  new `DamageSchemaError` variants and two new error enums.
- `crates/cs_content/tests/accept_f29_d1_airframe_damage_vocabulary.rs`
  (**new**): 5 `accept_f29_d1_*` tests (4 unignored, 1 retail).
- `crates/cs_content/tests/evidence_report_f29_d1.rs` (**new**): the evidence
  harness (not named `accept_*`).
- This file.

No protected path, no `cs_formats`/`cs_sim`/`cs_app` change, no binary, no
original text committed.

**One observable failure, before the change:** the declared damage-graph
schema knew nothing about the original's vocabulary. `DeclaredDamageGraph::
try_new` accepted any node set, so an airframe graph could declare three
region slots, five or none, and nothing could refuse it: there was no
measured number to refuse against, and no production path in this crate that
read `ZBD/planes.zbd`'s node or material names. The four names and the eleven
`<prefix>_damage` materials existed only in prose — the task description and
the F11-D2 `support\cockpit.gw` lead — so a graph "declaring regions" would
have been authored design with no source.

## What was measured (first-hand, this machine, `retail`)

Production path, one function:
`cs_content::damage::observe_airframe_damage_vocabulary($CS_GAME_DIR)` →
`cs_assets::install::discover` (the inventory and its per-file digest) →
the byte read **checked against the inventoried SHA-256** (a file that does
not hash to the installation's own row is refused, never measured) →
`cs_formats::gamez::read_gamez_nodes`, `read_gamez_materials` and
`read_gamez_meshes` over the same bytes with one `ParseContext`, each
proving its own section boundary. No `strings`, no scan, no table carried by
the test: the acceptance suite calls that function and asserts what it
returned.

| measured | value |
| --- | --- |
| container | `ZBD/planes.zbd`, catalog key `install_file/zbd_2f_planes.zbd` |
| container SHA-256 | `45da54a8e1886a8182e84bef03eb5e099481e483d79e458e7db5356538fbc21b` |
| distinct damage-region node names | **4** — `leftwingdamage`, `nosedamage`, `rightwingdamage`, `taildamage` |
| region records | **44**, each name stored **11** times |
| region groups | **11**, one per shared parent slot; every group's parent is stored as `damageindicator` and every group holds all four names |
| ancestry | each group's ancestry contains **exactly one** of F11-D2's eleven measured airframe roots, and all eleven are covered |
| `zone_id` of the 44 region records | `255` on every one — the stored default; nothing measured assigns a zone |
| distinct `<prefix>_damage` materials | **11** — `agyro_ avenger_ bal_ bldhwk_ brigand_ de_ firebrand_ fury_ ke_ pm_ whawk_damage` |
| material records naming them | **1** present material record per material |
| airframe bindings | **1** binding per material: exactly one mesh in exactly one airframe group's subtree references it, and the eleven bindings cover the eleven groups one each |
| per-airframe binding | measured, not inferred: `player_pfighter`→`de_damage`, `player_bhawk`→`bldhwk_damage`, `player_fbrand`→`firebrand_damage`, `player_brigand`→`brigand_damage`, `player_fury`→`fury_damage`, `player_autogyro`→`agyro_damage`, `player_avenger`→`avenger_damage`, `player_kestrel`→`ke_damage`, `player_peacemaker`→`pm_damage`, `player_warhawk`→`whawk_damage`, `player_balmoral`→`bal_damage` |
| damage-marker names **not** selected | nodes `damageindicator` ×11, `player_damage_off` ×14, `player_damage_on` ×12; textures `damage1.tif`, `damage2.tif` |

Two selection rules are disclosed rather than hidden, and both keep what they
left out in `AirframeDamageVocabulary::discarded` so the boundary is part of
the record: a **node** is a region candidate when its stored name ends with
`damage`, a **texture** is a wreck candidate when its stem ends with
`_damage`. Nothing else is filtered; case is folded byte-wise so a non-ASCII
name cannot panic the rule.

**Spans.** Each region record carries the span of the whole info-array slot
its name was read from (`info_offset + index · NODE_SLOT_BYTES`, length
`NODE_SLOT_BYTES`), each material the span of the whole 44-byte texture-name
record; both use only sizes and table starts `cs_formats` publishes. The
name field's inner offset inside a record is *not* published, so it is not
duplicated here — the acceptance test instead reads every name back out of
its own span against the container's bytes, which fails loudly if either the
layout or the arithmetic drifts. Every span carries the installation digest
and the container's own digest as its member digest.

A one-off pass over the same node reader during this task printed
`zone_id == 255` for **all 3317** records of the container, not only the 44.
That count is *not* what the committed record carries (the record stores
`zone_id` per selected region node only), so it is a lead for whoever
measures the zone field, not a claim of this task.

## What is explicitly not measured

- **A name is not a role.** Nothing measured says `nosedamage` is a damage
  zone, what its topology or parent's role is, or how a hit routes to it.
  The record stores names, slots, counts and spans; it assigns no kind, no
  integrity and no edge to any name.
- **No numbers.** `crimson.exe`, `mcp.dll`, `ifc21.dll` and `strings.dll`
  carry no recoverable integrity, armor, multiplier or overkill value (the
  task measured that: zero matches for `damage|armou?r` outside one input
  command). Nothing in this change infers one.
- **`zone_id`'s domain stays unknown.** The 44 selected records store the
  default; whether the field ever distinguishes zones in this container is
  unobserved, and `zone_id`'s domain beyond `{255 = none, 1, 2}` is
  unrecovered.
- **No original mission event** exists for a destroyed or bailed-out plane
  (F13-C: the mission opcode table is empty), so nothing here claims one.

## The declared lowering (shape only)

`AirframeRegionShape` is what the observation lowers into: four region part
slots and at most one wreck presentation slot per airframe, recorded on the
graph by

```rust
DeclaredDamageGraph::declare_airframe_regions(regions, wreck, observation)
```

- Both bounds are **read from `observation`**: `region_count()` (the distinct
  measured region names) and `max_wreck_materials_per_airframe()` (the
  largest number of distinct wreck materials one measured group binds). No
  `4` and no `1` appears in the check.
- A slot is a [`DamageNodeKey`], so a later binding is by identity and never
  by array position; a repeated key is refused instead of inflating the count
  the check compares.
- The shape carries `observation`'s own `Provenance`, so the declared record
  stays traceable to the measurement it was validated against.
- Refusals, each naming its subject: `RegionCountMismatch` ("airframe
  `<subject>` declares N damage-region slots, but its measured vocabulary
  holds M"), `WreckSlotOverflow`, `DuplicateShapeSlot`,
  `RegionShapeOnNonAircraft`.

The synthetic fixture is deliberately parameterised by count: a vocabulary
measuring **three** regions makes four a refusal, so a check that hard-coded
`4` — or that accepted any count — cannot pass the unignored suite.

## Why `crates/cs_sim/src/damage/graph.rs` is unchanged

It was an owner path and is untouched, on purpose:

1. `cs_sim` may depend only on `cs_types` and `cs_script`
   (`docs/01-ARCHITECTURE.md`), so a runtime check there could not read the
   installation: it would take a bare `usize` from its caller with no
   provenance — an argument, not a measurement.
2. The only caller would be `cs_app::damage::lower_graph`, outside this
   task's owner paths and part of F29's boundary stage. Adding a second,
   unwired constructor to `DamageGraph` would be public API nothing reaches,
   which reviewers (rightly) reject.

The refusal therefore lives beside the measured record, where the number and
its provenance come from one place. **Affected content:** the runtime half of
the region shape. **Resolving task:** the F29 boundary stage that lowers a
declared airframe graph (F29-B/F29-D wiring); if the runtime graph enforces
the shape too, it should receive `AirframeDamageVocabulary::region_count()`
through that boundary rather than hold its own copy.

## Tests

Every test drives production code: the reader, the record and
`declare_airframe_regions`. No test repeats an expected value out of a
fixture it also wrote.

| test | what it pins |
| --- | --- |
| `accept_f29_d1_region_shape_reads_its_counts_from_the_observation` | a three-region observation accepts 3 and refuses 4 with the exact by-name message; a four-region one accepts 4 and refuses 3 and 5 |
| `accept_f29_d1_region_shape_refuses_more_wreck_slots_than_one_airframe_binds` | 1 wreck slot accepted, 2 refused; an observation measuring no wreck material accepts 0 and refuses 1 |
| `accept_f29_d1_region_shape_refuses_duplicate_slots_and_non_aircraft_graphs` | a repeated slot key is refused by name; a world-object graph is refused by name; an accepted shape carries the observation's provenance |
| `accept_f29_d1_observation_names_an_installation_without_the_container` | the reader refuses an installation whose inventory holds no container, rather than measuring whatever it found |
| `accept_f29_d1_retail_the_installation_names_four_regions_and_eleven_wreck_materials` (`#[ignore = "requires CS_GAME_DIR"]`) | the whole measurement above: container key and digest, `Origin::Installation` + `observed_tool` provenance, 4 names × 11, 11 groups × 4 against F11-D2's roster, 11 materials with their per-airframe bindings, the discarded list, every span read back from the container's bytes, and the accepted/refused shape counts |

Without `CS_GAME_DIR` the retail test fails loudly (its own `game_dir()`
panics with a named reason); it never skips or passes vacuously.

## Mutation probes

Each probe edited one production line, ran the task selection, recorded the
failing test and restored the file from a saved copy (`git diff` was empty
after every restore):

| probe | edit | result |
| --- | --- | --- |
| the count check disappears | `if declared != observed` → `if false` | `accept_f29_d1_region_shape_reads_its_counts_from_the_observation` failed ("the lowering accepted a region count its observation never measured") |
| the count is a constant | `let observed = observation.region_count()` → `let observed = 4` | the same test failed on the three-region observation |
| the wreck ceiling is permissive | `max_wreck_materials_per_airframe()` → `usize::MAX` | `accept_f29_d1_region_shape_refuses_more_wreck_slots_than_one_airframe_binds` failed |
| the selection rule is loosened | node rule `ends_with(damage)` → `contains(damage)` | the retail test failed: `left` held 7 names (`damageindicator`, `player_damage_off`, `player_damage_on` included), `right` the measured 4 |

## Follow-ups left open

- **No production consumer yet.** Nothing outside the suite builds a
  `DeclaredDamageGraph` from installation data, so neither the record nor
  its refusal is reachable from the running binary. That is the standing
  status of the whole declared schema (`declared_synthetic_airframe_damage`
  is its only builder today) and the same open item F11-D2 recorded for its
  roster: **affected content** every airframe's damage graph.
  **Resolving task:** the F29 zones stage together with F11-C's name-path
  binding rules, which is also where the roster's `required_roles` gap
  (`support\cockpit.gw`) is open.
- **Region slots are reservations.** The shape says how many region part
  slots an airframe declares and by which keys; which `DamageNodeKind` (if
  any) a region becomes, and whether the original modelled these four nodes
  as zones at all, is unmeasured and must not be decided by a name.
- **Independent review is required.** Per the 2026-09-28 owner directive the
  review of this fidelity/measurement work should be a different agent
  instance or model with a fresh context, and its identity — with whether
  the context was fresh — must be recorded. No agent review replaces the
  owner's approval and none awards more than `checked`.
- **Evidence class.** Everything here is `observed_tool` over original
  *files*: `retail` is file access, not proof the original executable ran.
  No original run, no human play, no visual or audio evidence; nothing
  claims `verified_original` or `release_approved`.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-B`,
  `### F29-D`, non-negotiable 1, AC02).
- `docs/contracts/STATE-TRANSACTIONS.md`, `docs/contracts/IDENTITY-CONTENT.md`
  (stable ids, provenance, explicit unknowns, no positional resolution),
  `docs/contracts/CLI-EVIDENCE.md`.
- `docs/findings/2026-10-02-f11-d-2-airframe-roster-discovery.md` (the
  eleven-airframe roster the per-airframe claims are checked against, and the
  `support\cockpit.gw` lead whose names this task measured *inside*
  `planes.zbd`).
- `docs/findings/2026-10-02-gamez-node-array-layout.md` (the node array the
  region names are read from) and
  `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` (the
  texture-name and material tables the wreck materials are read from).
- `crates/cs_formats/src/gamez/{nodes,materials,reader}.rs`,
  `crates/cs_assets/src/install.rs`, `crates/cs_content/src/catalog/baseline.rs`
  (`install_file_key`).
