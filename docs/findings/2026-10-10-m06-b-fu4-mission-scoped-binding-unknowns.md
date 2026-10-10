# M06-B-FU4: the passenger-identity verdict now lives in M06's binding record

Date: 2026-10-10. Task: M06-B-FU4 (Rally #1184) "Record M06's
passenger-identity verdict inside the binding record's unknowns", the
follow-up of M06-B-FU2 (#818,
`docs/findings/2026-10-10-m06-b-fu2-passenger-identity-binding.md`).
Owner paths used: `crates/cs_content/src/campaign_bindings.rs`,
`missions/bindings/` (the `M06.json` record and the `README.md` bullet),
`crates/cs_app/tests/campaign/` (the `m06_b_fu4.rs` suite, its `main.rs` /
`evidence.rs` module lines and `evidence/m06_b_fu4.rs`), `docs/findings/`.
Capabilities used: `retail` (`$CS_GAME_DIR`, read-only, never written).
Implementer: **opencode-go/mimo-v2.6-flash** (Rally #1184 implement claim of
2026-10-10, agent bunny-alpha-1).

## Verdict

The measured verdict of M06-B-FU2 — **the shipped data binds no passenger or
extraction entity to M06** — is now carried by `missions/bindings/M06.json`
itself, as the record's last `unknowns` entry, in the same voice as its
siblings: it names the limitation, the reason (the archive's only
passenger-named string is the shared `location.zrd` teleport entry
`Passenger_hangar`, and no directive, target, actor, startup animation, world
node or message id names one, measured by M06-B-FU2) and what settles it (an
original reference run under M06-C, or the owner's ruling). The entry reaches
M06's record alone: every other mission's record and the shared checklist are
unchanged.

## What changed

* **Production** (`crates/cs_content/src/campaign_bindings.rs`): a new
  mission-scoped unknown table `MISSION_SCOPED_UNKNOWNS`, keyed by work-order
  label, and a public accessor `mission_scoped_unknowns(&MissionLabel)` that
  `SourceContext::bind` consults after the global `SOURCE_BINDING_UNKNOWNS`
  and before the conditional title entries. The global table gained no
  passenger line — an entry there would assert the limitation for every
  mission, which is false for the missions whose data settles it (M01's
  startup fires the crew animation `call_add_jack`).
* **Record** (`missions/bindings/M06.json`): regenerated through production
  code — `regenerate_m06_b_fu4_writes_the_committed_record_through_production`
  (env-gated, `#[ignore]`d, deliberately not named with the acceptance prefix)
  re-derives the record with `SourceContext::read` + `SourceContext::bind` +
  `SourceBinding::to_json` and writes it; the diff is exactly one appended
  `unknowns` entry. Re-running it leaves the file byte-identical, and
  `accept_m06_a_the_committed_record_is_what_the_installation_derives` (9 of 9
  green) byte-compares the committed file with the derivation as before.
  `missions/bindings/README.md`'s M06 bullet records the regeneration.
* **Tests** (`crates/cs_app/tests/campaign/m06_b_fu4.rs`, two tests under the
  new `accept_m06_b_fu4_` prefix, plus the regeneration harness):
  * `…_m06s_record_names_the_passenger_identity_limitation_and_m01s_does_not`
    (retail): derives M06's and M01's records through production code and
    pins both directions — M06's derived record (and the committed file)
    carries the entry with every phrase above, and M01's carries no passenger
    unknown; before asserting, it re-measures *why* M01 is the contrast
    through production decoding: exactly one member of M01's archive
    (`ZBD/C1C/M01/zrdr.zbd`'s `startanims.zrd`) is a startup event table
    firing `NEW_GAME_START` together with `call_add_jack`, and the library
    member `passengers.zrd` declares that animation driving the world node
    `apassengers`, `ON_CALL`. It also checks the sibling committed records
    (`M01`, `M02`, `M03`, `M07`) name no passenger limitation.
  * `…_a_mission_scoped_unknown_reaches_only_the_work_order_it_was_measured_for`
    (synthetic, runs in CI): over the whole declared inventory — M06's entry
    is present and in the siblings' voice; all 23 other work orders receive
    none. Moving the entry into the shared table fails this test.
* **Evidence harness** (`evidence/m06_b_fu4.rs`): the report cites the
  recorded acceptance log and the derived M06 binding
  (`m06-binding.json`, produced by `source_binding_for`, which asserts the
  five critical dependencies are resolved and the limitation present before
  writing).
* No `Cargo.toml` or `Cargo.lock` change; no protected path touched.

## Why the mechanism is a keyed table

The task allowed a hook, table or argument. A keyed table was chosen because
the limitation is a *fact about one work order's data*, recorded at the one
site that already owns per-work-order checklist state: `bind` appends the
work order's entries after the shared ones, so the record format, the schema
(`schemas/mission-binding.schema.json`, unchanged) and every consumer of
`SourceBinding::unknowns` see ordinary checklist entries. `is_verified` keeps
`verified` false exactly as for the shared entries, and `bind_campaign`
inherits the scoping for free because it binds each work order through the
same `bind`.

## Recorded unknowns (not guessed)

- **Nothing was added to or removed from the global checklist.** The 13 shared
  entries are unchanged for every mission; M06's record differs from its
  siblings' by exactly the one measured entry.
- **The verdict itself stays as measured by M06-B-FU2** — the string-level
  teleport reading, the ON_CALL crew library and the census carrier set are
  that task's pins (`accept_m06_b_fu2_*`, 4 of 4 re-run green here); this task
  only records the verdict where the record can carry it.
- **The entry stays until it is settled:** an original reference run under
  M06-C (blocked on the owner) or the owner's ruling that the priority's cue
  describes no passenger mechanic. Whoever settles it removes the entry and
  regenerates the record with the same harness.
- **Nothing here is `verified_original`** (AGENTS.md rule 8): no original
  executable was run, and M06's record keeps `verified` false.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m06_b_fu4_ --include-ignored` | 0 (2 tests: 1 retail + 1 synthetic) |
| `cargo test --workspace --locked -- accept_m06_a_ --include-ignored` | 0 (9 tests, incl. the record-equality pin) |
| `cargo test --workspace --locked -- accept_m06_b_fu2_ --include-ignored` | 0 (4 tests) |

## Evidence

This task needs the `retail` capability, so it follows
`docs/contracts/CLI-EVIDENCE.md`: the acceptance run is tee'd into
`private/evidence/M06-B-FU4/cargo-test.log`,
`evidence_report_m06_b_fu4_writes_the_acceptance_report` writes
`private/evidence/M06-B-FU4/acceptance.json` from that log, the candidate
tree, the toolchain and production discovery of `$CS_GAME_DIR` (nothing typed
by hand), `tools/validate_evidence.py … --require-pass` validates it, and the
report is committed as `docs/findings/evidence/M06-B-FU4.json`.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_content::campaign_bindings::{SourceContext, mission_scoped_unknowns}`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::{decode_zrd, zrd_flat_fields}`;
`missions/M06.md`; `missions/bindings/M06.json`;
`missions/bindings/campaign-inventory.tsv`;
`docs/findings/2026-10-10-m06-b-fu2-passenger-identity-binding.md`;
`docs/findings/2026-10-01-m06-a-source-binding.md`;
`docs/contracts/CLI-EVIDENCE.md`;
`crates/cs_app/tests/campaign/m06_a.rs`;
`crates/cs_app/tests/campaign/m06_b_fu2.rs`.
