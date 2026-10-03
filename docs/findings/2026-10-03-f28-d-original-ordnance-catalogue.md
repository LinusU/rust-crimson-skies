# F28-D: the original ordnance surface, measured; and the audit that says what it cannot map

Date: 2026-10-03. Task: #123 "Close original ordnance catalog and test every
discovered behavior" (`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
section `### F28-D`, AC04). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
section "Boost and special models". Predecessors recorded in
`docs/findings/2026-10-01-f28-a-ordnance-behavior-and-effect-registry.md`,
`docs/findings/2026-10-03-f28-b-ordnance-runtime.md` and
`docs/findings/2026-10-03-f28-c-ordnance-integration.md`.
Capabilities used: `retail` (read access to `$CS_GAME_DIR`) and ordinary
build/test. **Not** used and not claimed: any run of the original executable, so
no statement here is evidence of how the original *behaves* — only of what its
files declare.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/ordnance.rs` (extend): the measured surface
  (`ORIGINAL_ROCKET_NAME_BLOCKS`, `ORIGINAL_NEXT_ROCKET_BLOCK`,
  `ORIGINAL_ROCKET_ORDNANCE_TYPES`, `ORIGINAL_NITRO_CONTROL`,
  `ORIGINAL_ORDNANCE_ROCKET_SLOTS`, `ORIGINAL_ORDNANCE_HARDPOINT_POINTS`,
  `DECLARED_FIELDS_WITHOUT_CONSUMER`, `OriginalOrdnanceCounts`,
  `OriginalOrdnanceSurface`, `OriginalOrdnanceSurfaceError`, `FieldTally`,
  `OrdnanceAuditRow`, `OrdnanceAuditFinding`, `OrdnanceAuditReport`,
  `OrdnanceAudit`) and the module docs.
- `crates/cs_app/src/ordnance.rs` (extend): the runtime half
  (`SessionOrdnanceRow`, `SessionNitroRow`, `SessionOrdnanceFinding`,
  `SessionOrdnanceAudit`, `session_ordnance_audit`), the accessors
  `OrdnanceSession::registered_ids` / `::registered_shooters`, and one repair in
  `OrdnanceSession::close` described below.
- `crates/cs_sim/src/weapons/ordnance.rs` (extend, one method):
  `OrdnanceRuntime::nitro_actors`.
- `crates/cs_content/tests/accept_f28_d_ordnance_audit.rs` (**new**, 16 tests),
  `crates/cs_app/tests/accept_f28_d_ordnance_catalogue.rs` (**new**, 11 tests),
  `crates/cs_content/tests/f28_d_support/mod.rs` (**new**, the shared retail
  read), `crates/cs_content/tests/accept_f28_d_retail_ordnance_catalogue.rs`
  (**new**, 9 `#[ignore]` tests),
  `crates/cs_content/tests/evidence_report_f28_d.rs` (**new**, evidence harness,
  not an acceptance test).
- This file and `docs/findings/evidence/F28-D.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file. Wiring: none was needed — both modules were already declared in
their `src/lib.rs`.

**One observable failure, before the change:** F28-A's schema said in prose that
"F28-D maps [the hardpoint layout] onto this set" and `OrdnanceRegistry` said
that F28-D's catalogue audit decides whether the registry is complete with
respect to the installation — and **nothing did it**. Given the declared
ordnance records of an installation there was no production query at all that
produced a row per component, so nothing could say how many rocket types the
original has, whether a declared record names one of them, or whether a
declared field is read by anything. And nothing could say the *opposite*: there
was no number anywhere in the project that "every rocket type" could be checked
against, so a catalogue holding only the synthetic fixture's five components
would have satisfied any check there was. The audit was absent in both
directions, which is the failure AC04 names.

## What this stage measures, and where each number comes from

Everything below was read out of retail members of `GOSDATA/ASSETS/crimson.rof`
through the production readers (`cs_assets`' ROF mount and `cs_formats`'
bounded member decoder), and is re-measured by
`accept_f28_d_retail_ordnance_catalogue.rs` on every run, so a stale constant
fails rather than passing.

### `ASSETS/SCRIPTS/RESOURCE.H` — the engine's own resource header

A C include the original build generated, shipped inside the asset container.
This is where the ordnance identity vocabulary lives.

| what | measurement |
| --- | --- |
| rocket name blocks | three: `IDS_ROCKETLONGNAME 3380`, `IDS_ROCKETSHORTNAME 3395`, `IDS_ROCKETDESCRIPTION 3410`. The bases are **fifteen** apart |
| the block after them | `IDS_PAINTLONGNAME 3425`, also fifteen on — so the rocket run is bounded at **15 ids per block** |
| nitro control | `MPOUT_CHK_NITRO 10135` |

**The type count is not the block width.** The gaps say fifteen; the screens say
eleven. Reading the width as the count is the same mistake F27-D found and
repaired in the ammunition blocks, so it is made impossible here by
construction: `ORIGINAL_ROCKET_NAME_BLOCKS` and
`ORIGINAL_NEXT_ROCKET_BLOCK` carry the blocks and the bound,
`ORIGINAL_ROCKET_ORDNANCE_TYPES` carries the count, and
`accept_f28_d_the_measured_blocks_bound_the_run_without_counting_it` asserts
they are not equal.

### The selection screens — the counts, read a second independent way

- `ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT` declares `string DIA[11]` and asks
  the engine to fill it (`callback($$E$$,5058,DIA[0])`), builds a header row
  plus eleven rows (`for(AIA=0; AIA < 11 + 1; AIA++)`), tests eleven
  selectable types (`for(ZHA = 0; ZHA < 11; ZHA++)`) and indexes its
  descriptions as `(3410 + sender.QG - 1)` — so the descriptions occupy
  `3410..=3420`, eleven ids, exactly the type count.
- `ASSETS/SCRIPTS/MULTIPLAYER_OUTLAWROC.SCRIPT` asks for the same eleven rocket
  names through a *different* callback on a *different* screen
  (`for(int RX=0; RX < 11; RX++)` then `callback($$E$$,5019,(RX),YIA[RX])`).
- Eight rocket slots: `object EIA[8]` with `for (YHA=0; YHA < 8; YHA++)` in the
  rocket screen, and `object RKA[8]` in `ORDINANCELAYOUT.SCRIPT`.
- Two hardpoint points: `object DT[2]`, `for (int R=0; R < 2; R++)` and the
  per-point read `callback($$E$$, 2245, 0, (R), AT[R])` in `HARDPOINTS.SCRIPT`.

So **eleven rocket ordnance types, eight rocket slots, two hardpoint points**,
each count with at least two witnesses.

### `ASSETS/SCRIPTS/DEBUGINFO.TXT` — the engine's variable dictionary

The engine's own words for the ordnance identifiers: `nroc = VIA`,
`arrocketnames = YIA`, `nfirstrocket = XKA`, `odrockets = RKA`,
`chkallroc = XIA`, `frocout = WIA`, `fnitroout = SIA`, `nhardpoint = YHA`, and —
new and useful — `ohardpointweight = YQA` and `ohardpointcost = ZQA`. The
hardpoint's **weight and cost are named by the engine**, which says plainly that
the original weighed and priced a hardpoint; the numbers are in the executable,
not in a file. That is a measurement, and its absence from any file is a
limitation, not an invitation to invent a weight.

### `ASSETS/SCRAPBOOK.CSV` — the ordnance names that do exist, and do not help

Two rows name an ordnance illustration: `7_2_5` carries `NT_07_01_bpnitro` and
`19_1_4` carries `NT_19_01_bptorpedo`. Both are asset ids of *illustrations*.
The resource header declares **neither** of their `_t`/`_b` text ids, so even
the scrapbook's own ordnance descriptions live in the executable's runtime
catalog — an independent second corroboration of the string-catalog finding
F27-D recorded for the gun and ammunition names. The audit therefore treats
these rows as what they are: evidence that the original illustrated a nitro item
and a torpedo, and **not** a catalogue entry, not a component id and not a
behaviour. Nothing in the project maps them to a declared record, and
`accept_f28_d_retail_the_ordnance_illustration_text_is_not_in_any_file` asserts
only what was measured.

## The audit

`cs_content::ordnance::OrdnanceAudit::run` walks the declared records and
compares them against an `OriginalOrdnanceSurface`. Per distinct component it
emits an `OrdnanceAuditRow`: the family, a `FieldTally` of how many load-bearing
values the record declares and how many carry `verified_original` provenance,
how many known nonzero damage channels it routes, how many status effects it
applies, whether it declares an area, and its origin and provenance. Then it
checks ten named things:

| finding | what it means |
| --- | --- |
| `undeclared_rocket_type` | the installation offers more types than the catalogue enumerates |
| `unattributed_rocket_type` | nothing in the files maps a declared record to one of the installation's types |
| `unsupported_rocket_slots` | the declared airframe layout offers fewer slots than the screens build |
| `unsupported_hardpoint_points` | likewise for hardpoint points |
| `missing_nitro_record` | the installation names a nitro control and the catalogue declares no booster |
| `unmeasured_field` | a declared load-bearing value is an explicit unknown, by declared field name |
| `unmeasured_record` | a record whose provenance is not `verified_original` — this is what the fixture reports for each of its own |
| `unused_family` | a designed behavior family no record uses |
| `unconsumed_field` | a declared field that lowers into the runtime and that no production path reads |
| `delivers_no_effect` | a launched item with no damage channel, no status effect and no area |

`is_complete()` is `findings.is_empty()`, which is deliberately hard to reach —
and is **reachable**, so the synthetic report's incompleteness is a real
verdict rather than a structural one.
`accept_f28_d_a_fully_measured_catalogue_is_complete` builds eleven fully
measured launched records plus a measured booster, against the real measured
layout, and the audit reports no gap at all. Any of the ten decisions can be
mutated away and a test catches it (table below).

The runtime half, `cs_app::ordnance::session_ordnance_audit`, walks what a live
session actually lowered and registered. It cannot see provenance — the
lowering boundary drops it on purpose — so it answers a different question: per
component, which channels reach a consumer (`damage_channels`, `status_effects`,
`launcher`, `delivers_effect`), which declared fields reach nothing
(`unconsumed_fields`, `area_applied`, always `false`), and which designed
families the session does not carry. Per booster it reports the declared
numbers the flight model will read and `consumption_per_tick` — the one rate the
ledger converts with.

## The repair this stage made

**`OrdnanceSession::close` left the lowered loadout behind.** F28-C's teardown
drops and rebuilds the runtime empty "so an accessor cannot read a stale effect
or a spent capacity out of a session that no longer exists", but `components` —
the map of lowered `OrdnanceComponent`s — survived it. That was invisible while
nothing read it. The moment this stage added an audit that walks a session, the
hole became real: `session_ordnance_audit` on a closed session would have
reported a fireable loadout for a session that no longer exists, and worse, the
family-occupancy walk would have reported every designed family as *uncovered*,
turning a teardown into a catalogue gap. `close` now clears `components`
alongside the runtime, `registered_ids`/`registered_shooters` return nothing for
a closed session, and `session_ordnance_audit` returns an empty report for one
— all three pinned by `accept_f28_d_a_closed_session_audits_to_nothing`. The
second half of that test (a closed session reporting six
`family_without_a_component` findings) was the failure that found the hole
*after* the repair, and is why the early return is there.

## AC04: boost changes thrust and consumption and nothing else

`accept_f28_d_boost_changes_thrust_and_consumption_and_never_moves_or_scales_a_frame`
drives the production path — `step_ordnance_session` with
`OrdnanceOrder::Nitro` over a live ECS airframe — and pins the minimum scenario
from four sides:

1. **Thrust and consumption change, by the declared amounts.** An accepted tick
   reports exactly the declared `extra_thrust_n` and exactly
   `consumption_per_s * rate.dt_seconds()`, cross-checked against the same
   numbers read back out of the session's own audit row.
2. **Nothing is teleported.** The shooter's `Transform`, `GlobalTransform` and
   `LinearVelocity` are asserted unchanged after the boost tick and after the
   idle tick. A nitro activation also emits **no launch effect** (the record
   that carries a world origin), launches no item, and mirrors no ECS entity, so
   there is no path by which a boost places anything in the world.
3. **No render frame scales it.** The same boost costs the same at `dt_s` of
   1/120 s, 1/30 s and 1/15 s, and ten ticks walked leave the same capacity as
   ten ticks jumped. A ledger that converted through the caller's `dt` fails
   both.
4. **A refusal costs nothing.** Holding the control until the tank is empty
   yields a refused activation with zero thrust and zero consumption, and with
   a zero-recovery declared booster every further refused tick leaves the tank
   exactly where the refusal found it — the only way to observe the refusal's
   own cost.

## The stage's substantive finding: the catalogue is a count, not a mapping

The original's files declare **eleven rocket ordnance types** and nothing else
about them. They do not say which eleven. A catalogue of eleven invented names
would satisfy `undeclared_rocket_type` and still name nothing the original has,
which is why `unattributed_rocket_type` is a separate finding: a record is
attributed only when its own provenance *and* every one of its declared values
carry `verified_original` provenance. The project's six-component synthetic
catalogue is therefore reported twice over — five launched items against eleven
observed, and **zero attributed** against eleven observed — which is the honest
verdict.

The second finding is that the declared **area effect's reach reaches no
recipient**: `lower_ordnance` puts both of its values into
`cs_sim::weapons::ordnance::AreaEffect`, and
`ProjectileOrdnance::area_effect` has two pass-through readers —
`LiveOrdnance::area_effect` and the `GuidanceDetonation::area_effect` F28-C.1
added — but neither is read by a production gameplay path. F28-C applies a
triggered item's declared *status effects* to the one stable recipient its
engagement names, and F28-C.1's guidance-loss blast routes the declared damage
channels to that same named target and damage node rather than to every actor
inside a radius. F28 non-negotiable 3 requires area effects to have bounded
lifetimes and stable recipient ids; that is enforced for the status ledger
(F28-C's timed engine-status path) and **not** for the area's reach. This stage records the
gap in `DECLARED_FIELDS_WITHOUT_CONSUMER` and reports it per record and per
session row rather than implementing a splash rule, because the original's area
behavior is unmeasured and F28's research boundary forbids inventing one. Task
#552 (F28-AE1) owns the implementation.

The third is that **the import-side refusal is unreachable**.
`OrdnanceRegistry::resolve_installation` refuses a loadout naming an unknown or
repeated component id, and its doc claims that refusal "cannot reach a session by
way of an import" — but `OrdnanceRegistry`, `resolve_installation` and `families`
are referenced only from the two F28-A test targets, and
`OrdnanceSession::register` takes whatever `&[DeclaredOrdnance]` a caller hands
it. F28 non-negotiable 5's import-side half is therefore defined and tested but
not enforced on a live session. Task #553 (F28-AE2) owns it, coordinated with
F44-B (#181), which owns the shared validator.

## Choices worth stating

- **The audit reports, it never repairs.** No invented eleventh rocket, no
  family assigned to a record nothing measured, no default area radius. A
  deferral is recorded as a *named* finding so it can no longer be forgotten,
  the same move F27-D made for `penetration`/`ricochet`/`ammo_switching`.
- **The count checks are floors, not equalities.** A declared layout offering
  *more* slots than the original's minimum is not a gap
  (`accept_f28_d_a_larger_declared_layout_is_not_a_gap`), so the audit does not
  fail an airframe for having more capacity than the original's least.
- **A known zero is a measurement, not a gap.** A record declaring zero damage
  on both channels is reported as `delivers_no_effect`, while a record
  declaring an *unknown* amount is reported as `unmeasured_field`; the two are
  different facts and the audit keeps them apart.
- **Two records for one id are one component.** Rows are keyed by id, so a
  duplicated record cannot inflate the count past the closure check, and the
  first in insertion order is the one reported — the disagreement is the
  importer's business, not something to average away.
- **A booster delivers an effect.** The boost *is* the effect, so a booster with
  no damage channel and no status effect is never reported as delivering
  nothing.
- **`nitro_actors` is the only enumeration the runtime offers.** A session with
  two boosters had no way to list them, so the audit could not report either;
  the method is the map's own key order, so two hosts agree.
- **The scrapbook rows are not promoted.** `NT_19_01_bptorpedo` is suggestive,
  and this stage records the row and stops there. Reading an illustration's asset
  id as a catalogue entry would be exactly the kind of guess the sheet forbids.
- **The retail tests and the harness share one reader.** A divergence between
  them would make the evidence artifact describe an installation state the
  acceptance suite never checked, so the ROF mount and member read live once in
  `f28_d_support` and the harness's own contribution is a *re-read through the
  same production reader*, reported as ids, counts and digests only.

## Test sensitivity (measured, one mutation at a time)

Eleven mutations were applied to the three production files, one at a time, and
the F28-D test binaries re-run after each; each file was restored from a
byte-identical backup (`shasum`) after every probe. Every mutation was caught.
The "caught by" column is what the run reported, not an estimate.

| mutation | caught by |
| --- | --- |
| `run` drops the `undeclared_rocket_type` closure check | `accept_f28_d_a_catalogue_that_under_counts_the_installation_is_incomplete`, `accept_f28_d_an_empty_catalogue_is_incomplete`, `accept_f28_d_the_synthetic_catalogue_is_incomplete_by_name`, `accept_f28_d_retail_the_ordnance_audit_reports_every_type_it_cannot_map` |
| `run` drops the `unattributed_rocket_type` check | the same four, plus `accept_f28_d_a_fully_measured_catalogue_is_complete` (it attributes 11) |
| `is_measured` counts a designed record as measured | `accept_f28_d_a_fully_measured_catalogue_is_complete` |
| `FieldTally::is_fully_measured` accepts `total == 0` | `accept_f28_d_an_unmeasured_field_is_reported_by_name` |
| the walk skips `guidance.lost_target` | `accept_f28_d_a_fully_measured_catalogue_is_complete` (its row stops being fully measured) |
| `walk_record` no longer records the unknown field names | `accept_f28_d_an_unmeasured_field_is_reported_by_name` |
| `known_damage_channels` counts an unknown channel as known | `accept_f28_d_a_record_that_delivers_nothing_is_reported` |
| `delivers_effect` treats a booster as delivering nothing | `accept_f28_d_a_booster_delivers_the_boost` |
| `run` drops the `unconsumed_field` reporting | `accept_f28_d_the_synthetic_catalogue_is_incomplete_by_name`, `accept_f28_d_retail_the_ordnance_audit_reports_every_type_it_cannot_map` |
| `run` drops the `missing_nitro_record` check | `accept_f28_d_a_missing_nitro_record_is_reported_only_when_nitro_is_named`, `accept_f28_d_an_empty_catalogue_is_incomplete` |
| `run` stops deduping by id | `accept_f28_d_a_repeated_record_is_audited_once` |
| `session_ordnance_audit` loses the closed-session early return | `accept_f28_d_a_closed_session_audits_to_nothing` |
| `OrdnanceSession::close` stops clearing `components` | `accept_f28_d_a_closed_session_audits_to_nothing` |
| `session_ordnance_audit` reports `area_applied: true` | `accept_f28_d_an_unconsumed_area_effect_is_named` |
| `session_ordnance_audit` drops the `component_delivers_nothing` finding | `accept_f28_d_a_component_that_delivers_nothing_is_named` |
| `NitroLedger::request` converts through a caller-supplied duration | `accept_f28_d_boost_costs_the_same_at_any_render_frame_length`, `accept_f28_d_boost_costs_the_same_walked_or_jumped` |
| the nitro order writes the shooter's velocity | `accept_f28_d_boost_changes_thrust_and_consumption_and_never_moves_or_scales_a_frame` |
| a refused activation consumes capacity | `accept_f28_d_a_refused_boost_consumes_nothing` |
| `request` applies recovery while burning | `accept_f28_d_capacity_recovers_only_while_the_booster_is_idle` |

## Unknowns recorded (not guessed)

- **Which eleven rocket types these are**, and every per-component number behind
  them: families, fuse shapes, arming conditions, lifetimes, damage, status
  effects, lost-target behavior, launch speeds, stack loads, area radii and
  nitro parameters. The names live in the executable's tables and runtime string
  catalog, exactly as the gun and ammunition names do (F27-D). The scrapbook's
  illustration ids are not a substitute. Task #452.
- **The hardpoint layout per airframe, and each point's weight and cost.** The
  engine names `ohardpointweight` and `ohardpointcost`; the numbers are in the
  executable's per-airframe tables. The audit compares the point count only.
  Task #452.
- **The area effect's reach** — see above. Task #552.
- **The import-side registry gate** — see above. Task #553.
- **Nitro recovery on a refused activation.** `NitroLedger::request` applies its
  idle-only recovery on any tick the booster did not run, *including a tick where
  the activation was refused*, so a pilot holding the control on an empty tank
  gets a unit of capacity back, burns it for one tick, and repeats. What *is*
  measured and tested is that a refusal consumes nothing and adds no thrust; what
  is unmeasured is whether the original recovers during a refusal. This stage
  records the behavior rather than choosing between the readings, and does not
  change F28-B's tested contract. Task #554 (F28-AE3).
- **Every lost-target behavior**, and whether the original re-acquires a
  temporarily lost tagged target. Unchanged from F28-A/B/C; task #454.
- **The flight model's consumption of the boost**, which F28-C deferred to task
  #453. Not touched here: AC04 asserts what the ledger and the session do, not
  what the flight model has not been wired to do yet.
- **Everything else in F28** stays where F28-A/B/C left it: no original-run
  behavior, no audio or visual verification of the emitted effects, no Avian
  projectile body, no cockpit input device, no AI launch producer, no network
  packet encoding, and `crates/cs_app/src/run.rs` still does not schedule
  `step_ordnance_session` (not an owner path for this task).

## Evidence

`private/evidence/F28-D/acceptance.json`, committed as
`docs/findings/evidence/F28-D.json`, validates with
`tools/validate_evidence.py --require-pass`. What it records:

- `capabilities: ["retail", "synthetic"]`, `claim: "implemented"`;
- `tests` — the `accept_f28_d_` selection, including the nine retail tests, all
  run with `--include-ignored`;
- `install_sha256` and `content_sha256`, both measured by production
  `cs_assets::install` discovery, never typed in;
- two hashed artifacts: `cargo-test.log` (the recorded acceptance run) and
  `ordnance-surface.json` — a **second** production pass of the same reader over
  the same installation, carrying each member's decoded length and SHA-256, the
  resource header's `#define` count, the measured rocket-block ids, the next
  block's id, the nitro control's id, the three counts re-read from the screens'
  own array sizes and loop bounds, the engine dictionary's identifier names,
  the scrapbook rows with whether the resource header declares their text ids,
  and the nine fidelity limitations under their `f28.d.limit.*` claim ids with
  the task that would resolve each.

**`unknowns` is empty and that is a claim worth checking.** The validator's
`--require-pass` rejects a nonempty `unknowns` list. The nine limitations are
therefore recorded in the four places above the validator does not read for that
field — the report's `review.method`, the hashed artifact, this file, and the
follow-up tasks — rather than removed. They are *unmeasured original behavior*,
not failures of this stage's assertions: every assertion here is a measured fact
about shipped files or a production behavior the suite exercised, and all of
them pass. `claim` is `implemented`, never `verified_original`: what was
verified is what the installation's **files** declare, and `retail` here is read
access, not evidence that the original executable ran.

The reviewer should regenerate the report on the rebased commit and compare it.
The tree hash is in the report and is checked against `HEAD^{tree}` by the
harness itself, so a report from another commit cannot be reused.

## Not claimed

No original-data *verification* of gameplay behavior. This stage measured what
the installation's **files** declare and built the audit that reports everything
else as unknown. It does **not** establish the original's rocket names,
families, fuses, arming, lifetimes, damage, status effects, lost-target rules,
nitro numbers or hardpoint weights, and it does not claim the audit's declared
half has been run against an imported catalogue — it has not, because there is
none yet. `retail` here is read access to original files, not evidence that the
original executable ran. This stage awards at most **checked**.