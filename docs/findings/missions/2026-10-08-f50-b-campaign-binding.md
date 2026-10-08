# F50-B: bind the discovered campaign identities and run the prerequisite closures

Date: 2026-10-08. Task: F50-B "Bind discovered campaign identities and
prerequisite closures" (Rally #205, `specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
section `### F50-B`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`;
evidence contract: `docs/contracts/CLI-EVIDENCE.md`. Capabilities used:
`retail` (read-only `$CS_GAME_DIR`, never written) and `synthetic` (nothing
executed). Implementer: **bunny-alpha-2** (OpenCode Space Bunny Alpha, Rally
#205 implement claim of 2026-10-08T12:10Z). Review is assigned by Rally after
this hand-over; this document records the implementer's side only.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/campaign_bindings.rs` (production, owner path):
  `SourceContext::bind_campaign`, `assemble_campaign`, `BoundCampaign`, plus a
  module-doc section "The whole campaign (F50-B)".
- `crates/cs_app/tests/campaign/f50_b.rs` (new): the nine `accept_f50_b_*`
  tests.
- `crates/cs_app/tests/campaign/main.rs` (wiring): `mod f50_b;` and two doc
  paragraphs that were no longer true once this stage existed.
- `crates/cs_app/tests/campaign/evidence.rs` (owner path):
  `RETAIL_TESTS_F50_B`, `SYNTHETIC_TESTS_F50_B`,
  `evidence_report_f50_b_writes_the_acceptance_report`, its two helpers, and
  the module doc's list of stages.
- `missions/bindings/README.md` (owner path): a section describing what the
  whole-campaign call does and does not produce here, and a closing paragraph
  that no longer sent F50-B its work.
- `docs/findings/missions/` (this file) and `docs/findings/evidence/F50-B.json`
  (the committed copy of the acceptance report).

No wiring outside the owner paths was needed: `cs_app` already depended on
`cs_content` (F50-A), so no `Cargo.toml` or `Cargo.lock` change.

**One observable failure.** If `bind_campaign` filtered its work orders down to
the ones whose identities resolved — the exact "filtering to the working
subset" that spec F50 non-negotiable behavior 5 forbids — `assemble_campaign`
reports the left-out work order as a missing input and all four retail tests
fail with
`declared work order M09 was supplied no source binding, so it would stay a
placeholder instead of a bound record`. That is the failure the stage exists
to prevent: a mission that disappears rather than staying red. (Measured as
mutation 4 below, then reverted.)

## What was added

```rust
impl SourceContext {
    pub fn bind_campaign(&self, inventory: &CampaignInventory)
        -> Result<BoundCampaign, SourceBindingError>;
}

pub fn assemble_campaign(
    inventory: &CampaignInventory,
    sources: &[SourceBinding],
) -> Result<CampaignBindings, SourceBindingError>;

pub struct BoundCampaign {
    pub bindings: CampaignBindings,
    pub sources: Vec<SourceBinding>,
}
```

`bind_campaign` is the whole-campaign production path: **one** read of the
installation, one `SourceContext::bind` per declared work order in inventory
order, then one assembly over the frozen denominator. `assemble_campaign` is
the assembly rule lifted into a function of its own so it can be exercised
without an installation — the five synthetic tests are its tests.

`BoundCampaign` carries both halves because they answer different questions.
`bindings` is what coverage, closure and readiness are measured against.
`sources` is what those records were derived from, in inventory order, each
carrying the installation fingerprint it was read under, the campaign position
its discovery title resolved to, the identities it located and the byte ranges
it cites. Profile continuity and "which position is the last one" are
answerable from `sources` without deriving the campaign twice; nothing is
re-derived, both halves come out of one call.

### The three refusals

`assemble_campaign` refuses everything that would make a declared work order
*silently* lighter:

| Refusal | Reason |
| --- | --- |
| a binding naming a work order the inventory does not declare | the denominator would grow without a `declare` call |
| two bindings naming one work order | a placeholder is bound exactly once |
| a declared work order supplied no binding | its placeholder would survive and read as an ordinary unresolved mission, indistinguishable from a mission whose identity merely did not resolve |

What it deliberately does **not** inspect is whether an identity resolved. A
source binding with no mission id, world group or program still replaces its
placeholder through `SourceBinding::to_mission_binding`, with an explicitly
unresolved `mission_identity` cell; the other six categories and all 23
prerequisite rows stay unresolved because this stage reads original data, it
does not implement subsystems.

## What was measured on `$CS_GAME_DIR`

`install_sha256`
`c14a876f4457d8710dee7986333ab636122c9549cf72b646fd69cbe7e72c5352`,
`content_sha256`
`148a24b7b0506812e8f1ee13d8d3137a05926abebbe10161994e8c4cd300c35e`, the same
installation the M01-A … M24-A stages read (their *asset* digests and spans are
unchanged; see the fingerprint note at the end). Derived by
`SourceContext::read` + `SourceContext::bind_campaign` over the committed
`missions/bindings/campaign-inventory.tsv`, recorded verbatim in
`private/evidence/F50-B/campaign-binding.json`.

| Fact | Value |
| --- | --- |
| declared work orders / recorded missions / declared cells | 24 / 24 / 168 |
| missions whose five critical dependencies all resolve | 17 |
| `mission_identity` cells complete / unknown / **missing** | 17 / 151 / **0** |
| work orders whose identity stays unresolved | M09, M11, M14, M15, M20, M22, M23 |
| prerequisite subsystem rows | 552, all unresolved, 0 resolved, 0 unsupported |
| progression known / unknown | 0 / 24 |
| `CoverageReport::is_ready()` | **false** |
| closure reports / roots / missions reached | 24 / 24 / 24 |
| cells and subsystem rows the closures account for | 168 and 552 (exactly the campaign's totals) |
| retail campaign positions in the installation | 24, last one bound by M24 (`campaign_position` 23) |
| installation fingerprints across the 24 source bindings | exactly one |

The seven unresolved work orders are the ones whose declared discovery title
the installation carries in neither display form — the same seven
`missions/bindings/README.md` already lists as `Uncarried`. This stage does
not resolve them and does not guess: their identity cells stay `Unknown` with
their refusal in `unresolved_critical`, they stay in the denominator, and they
keep the campaign unready. Which retail mission each of them names is still
open (see `docs/findings/2026-10-01-m05-a-source-binding.md`).

## Test inventory (9 tests, prefix `accept_f50_b_`)

Five synthetic (unignored, so CI runs them) — these are `assemble_campaign`'s
tests:

| Test | What it pins |
| --- | --- |
| `accept_f50_b_a_declared_campaign_assembles_one_record_per_work_order` | one record per declared work order, no placeholder left, 21 cells with 0 missing, an unresolvable identity is an `Unknown` cell (never `Missing`, never `Complete`), a resolved one is `Complete`, coverage totals balance, campaign not ready |
| `accept_f50_b_the_assembled_campaign_is_ordered_by_the_denominator_not_by_arrival` | reversing the input changes neither `missions()` order nor closure roots |
| `accept_f50_b_a_work_order_the_inventory_does_not_declare_is_refused` | the refusal names the offending label and the denominator |
| `accept_f50_b_a_repeated_work_order_is_refused` | a second binding for one work order is refused, naming it |
| `accept_f50_b_a_declared_work_order_with_no_binding_is_refused` | a left-out declared work order is a missing input, naming it |

Four retail (`#[ignore = "requires CS_GAME_DIR"]`):

| Test | What it pins |
| --- | --- |
| `accept_f50_b_the_whole_campaign_binds_from_one_installation` | one source binding and one record per declared work order, in inventory order, all under production discovery's fingerprint; **per work order**: `unresolved_critical().is_empty()` ⟺ the `mission_identity` cell is `Complete`, and the recorded mission id is the one the source located |
| `accept_f50_b_the_prerequisite_closures_of_the_bound_campaign_omit_nothing` | 24 closures, one root each, each with 7 cells and 23 prerequisite rows, each cell equal to what the record holds, roots and reached unions = all 24, totals = 168 cells and 552 rows, no closure complete |
| `accept_f50_b_unresolved_identities_stay_unresolved_and_the_campaign_stays_unready` | the coverage totals above, `missing_cells == 0`, every subsystem row unresolved, 24 unknown progressions, `is_ready() == false`, and the unresolved set pinned to M09/M11/M14/M15/M20/M22/M23 with a reason to re-measure if it changes |
| `accept_f50_b_the_campaign_is_walked_from_m01_to_m24_under_one_profile_and_ends_at_the_last_retail_position` | the AC02-shaped walk: 24 work orders in declared order M01 … M24, every one recorded and non-placeholder, exactly one installation fingerprint across the walk, and M24 bound to the installation's last campaign position |

The four retail tests share one `SourceContext` and one `BoundCampaign` through
`OnceLock`, so "profile continuity" is both structural (one call, one context)
and asserted (one fingerprint, equal to production discovery).

### Mutation probes (implementation changed, test must fail, then reverted)

| # | Mutation | Result |
| --- | --- | --- |
| 1 | drop `assemble_campaign`'s "declared work order supplied no binding" check | `a_declared_work_order_with_no_binding_is_refused` FAILED (8 passed) |
| 2 | drop the undeclared-label check | `a_work_order_the_inventory_does_not_declare_is_refused` FAILED (8 passed) — the label still reaches `CampaignBindings::bind`, but the refusal no longer says what would have gone wrong |
| 3 | drop the duplicate check | `a_repeated_work_order_is_refused` FAILED (8 passed) — the duplicate is still refused by `CampaignBindings::bind`'s `AlreadyBound`, so this mutation shows the test pins the *specific* refusal, not that a duplicate would have been accepted |
| 4 | `bind_campaign` keeps only the work orders whose critical dependencies resolve | all four retail tests FAILED (5 passed), each with `declared work order M09 was supplied no source binding …` |

Each mutation was applied to `crates/cs_content/src/campaign_bindings.rs`,
the suite run with `cargo test -p cs_app --test campaign --locked --
accept_f50_b_ --include-ignored`, and the file restored from a copy in
`private/scratch/` (Git-ignored); the final state is byte-identical to the
committed one (`git diff` shows only the stage's 161 added lines).

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f50_b_ --include-ignored` | 0 (9 tests, all passing) |
| `python3 tools/validate_evidence.py private/evidence/F50-B/acceptance.json --artifact-root private/evidence/F50-B --require-pass` | 0 |
| `python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py'` | 0 (27 tests) |

## Evidence

`private/evidence/F50-B/acceptance.json`, copied unchanged to
`docs/findings/evidence/F50-B.json`. Capabilities `["retail", "synthetic"]`;
9 discovered / 9 executed / 9 passed / 0 failed; claim `implemented`;
`candidate_tree` `4f0aa11aed083130c7a468828510bc887c7a7222`, the tree of the
commit the suite and the harness ran on — the only later deltas are this
report's own copy under `docs/findings/evidence/` and this document, neither
of which the acceptance suite reads. Artifacts: `cargo-test.log`
(`810670b08912789fc214c5bd2161cb5df4ca21fa57078a2cc4cb172c980f9a03`) and
`campaign-binding.json`
(`5364bf8c97fd4e3b9edab4aaa1359530ecb81784dda6e16c58afff66750f7286`), both
staying in `private/`.

## What this stage does not do

- **It does not bind the campaign progression.** The successor relation lives
  in original scripts nobody has measured; the directory layout and the
  localized row blocks give an *order*, not a successor rule, and an order is
  not a progression (a mission may be optional, skippable or branched). Every
  assembled mission therefore keeps `Progression::Unknown`, `progression_known`
  is 0, and `is_ready()` is false. `Progression::Known` stays for the stage
  that measures the rule.
- **It does not play anything.** The AC02-shaped minimum scenario ("play M01
  through M24 in progression order with profile continuity and final ending")
  is met here at the binding level: the declared walk covers M01 … M24 in
  order under one profile and ends on the retail campaign's last position. A
  played run, a profile carried *between* missions and a measured progression
  are F50-C's probes and F50-D's ordinary-play evidence; F50-B has no
  `human_play` capability and claims none.
- **It does not resolve M09, M11, M14, M15, M20, M22 and M23.** Their titles
  are uncarried; the identities stay unknown with their refusal.
- **It creates no `missions/bindings/M*.json`.** The per-mission records stay
  the output of `M01-A` … `M24-A`; this stage produces no file there.
- **It awards nothing.** The claim in the evidence report is `implemented`;
  a Rally merge would award `checked` at most, and no agent review replaces
  the owner's human approval.

## A pre-existing failure found on the way (Rally #779)

While running the retail suite,
`the_committed_record_is_what_the_installation_derives` turned out to fail for
**all 17** missions that have a committed record, on this installation:

```
cargo test -p cs_app --test campaign --locked \
  -- the_committed_record_is_what_the_installation_derives --include-ignored
# test result: FAILED. 0 passed; 17 failed
```

For `missions/bindings/M01.json` the only differing field is `install_sha256`
(`b4e780ab…` committed vs `c14a876f…` derived); every span, digest, id and
`unknowns` entry is byte-identical. So either the installation changed in a way
the fingerprint records but that touches neither cited asset, or the
fingerprint algorithm changed on `main` after those records were committed.
Either way it is not this stage's work: F50-B touches none of the committed
records, the fingerprint code or the installation, and all 17 tests are
`#[ignore]`d so CI never sees them. Filed as **Rally #779**
(`M0X-A-FU-INSTALL-FINGERPRINT`) with the measurement above; it must establish
the cause before any record is regenerated.

Nothing in F50-B depends on those records: its campaign binding is derived
fresh from the installation on every run, and its own continuity assertion is
`SourceContext::install_sha256() == fingerprint(discover($CS_GAME_DIR).manifest)`,
which holds for the installation as it stands today.
