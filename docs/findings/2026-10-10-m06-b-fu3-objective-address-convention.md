# M06-B-FU3: one objective-address convention, reconciled across the M02–M06 suites

Date: 2026-10-10. Task: `M06-B-FU3` (Rally #819), follow-up of `M02-B-FU3`
(#802) and the four campaign acceptance suites `M02-B`, `M03-B`, `M04-B`,
`M06-B`. Capabilities used: `retail` (`$CS_GAME_DIR` read-only) and the
owner's decrypted engine image (`$CS_ENGINE_IMAGE`, read-only, never
committed — the same image `2026-10-09-m02-b-fu3-out-of-range-wake-address.md`
measured, SHA-256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`).

Nothing here is `verified_original` (AGENTS.md rule 8): the executable
evidence cited is **static code evidence** read out of the decrypted image
by #802; no original program was run in this task, and the runtime behaviour
that image does not decide stays recorded as unknown.

## The convention

**A spelled cross-objective address is the one-based number of a numbered
`OBJECTIVE<N>` block; the original's parse decrements it to the zero-based
record index `address − 1`.** The valid range is `[1, objectives]`; an
address outside it is refused by name
(`cs_sim::objectives::address::resolve_objective_address`, rule
`OUT_OF_RANGE_OBJECTIVE_ADDRESS`), never clamped and never silently
accepted. `address_of` is the inverse.

## The evidence, re-derived here

The convention is not asserted by fiat; it is the parse the decrypted
executable runs, measured by #802 and re-stated for this finding:

| Address (VA) | Site | What it shows |
| --- | --- | --- |
| `0x468c40` | `WAKE_OBJECTIVE(_WHEN_I_COMPLETE)` store loop | `mov child; dec; store` per spelled integer into the `+0x1c` array |
| `0x468cf0` | `NAP_OBJECTIVE_WHEN_I_COMPLETE` target | `dec` before storing `+0xd0` |
| `0x4679fc` | `TICK_DEPENDS_ON_OBJ` | `dec` before storing `+0x10` |
| `0x467a21` | `DEDG` (control) | two integers stored **without** `dec` — the decrement is specific to objective addresses, not an integer encoding |
| `0x468d18` | NAP's seconds child (control) | stored without `dec` — a nap's child1 is a delay, never an address |
| `0x467956` | record allocation | one `0x5e4`-byte record per numbered block, so the record array is indexed `0..count` |
| `0x469043` | `+0xc48` | the objective count is the numbered-block count |
| `0x469af0` | the wake walk | walks the stored (decremented) indices against the record array with **no bounds check** — an out-of-range stored index is executed, so the spelled side must be in range |

Independent corroboration, also noted by #802: F39-E2's corpus closure over
all 1338 retail blocks found every spelled target inside the declared block
numbers with `dangling_sites == 0` — under a zero-based reading the corpus
would never spell `0` and several records would address one past their own
end.

## What was wrong, and what changed

Two of the four suites had merged reading a spelled integer as the
**zero-based record index itself**. On retail data that misattributes every
edge to the block one number lower than the site names, and calls an
address equal to the block count dangling. The corrections are in the
assertions, not deletions:

* **`m02_b.rs` — `accept_m02_b_the_objective_graph_…`.** The walk sorted
  spelled integers into `edges`/`dangling` by `index < blocks.len()` and
  reported `OBJECTIVE13`'s spelled `50` as M02's one dangling address.
  Reconciled: `dangling` is now required empty and `OBJECTIVE13 → 50` is
  pinned as a real edge to `OBJECTIVE50`, the last block — the
  discriminating address a zero-based reading cannot explain. The latch
  edges were re-derived on the record: `INSTANTWIN` (block 15) is named by
  `OBJECTIVE14`'s `NAP [15, 22.0]` (a nap on a spelled delay, previously
  described as a wake from the block `50`-indexing shifted it onto);
  `INSTANTLOSS` blocks 24 and 36 are named by `OBJECTIVE3`'s nine-target
  kill plus `OBJECTIVE19`'s nap, and by `OBJECTIVE7`'s nap respectively.
* **`m04_b.rs` — `accept_m04_b_the_terminal_blocks_are_gated_…`.** The
  `incoming(target)`/`gates` walk compared `addresses(d)` against
  `target − 1`. Reconciled to `target` directly. The latch edges it now
  pins: `INSTANTWIN` (block 32) ← block **31**'s `NAP [32]` (was listed as
  block 44's `NAP [31]`); `INSTANTLOSS` (block 41) ← block **27**'s
  `NAP [41]` (was listed as block 20's `WAKE [22, 40, 50]`); nothing gates
  on block 41, and block 42's `TICK_DEPENDS_ON_OBJ` targets block 40 — the
  block `OBJECTIVE20` wakes (was described as a gate on the latch).
* **`m03_b.rs`, `m06_b.rs` — already correct.** Both compare spelled
  addresses against the block number directly and their pins are
  unchanged; `m06_b.rs`'s cross-reference comment now names this task.

The two stale findings that recorded the wrong reading are corrected in
place, in the established "Corrected by #N" style: the dangling-address
bullet and test row of `2026-10-08-m02-b-compatibility-gaps.md`, and the
latch table, its paragraph and the test row of
`2026-10-08-m04-b-compatibility-gaps.md`. The ambiguous wording the
misreading grew from is pinned where it lives: `dec` is now defined as the
decrement of a one-based block number at
`2026-10-06-m01-lc-directive-a-objective-directive-parser.md` ("Outcome,
identity and dependency") and `…-directive-b-objective-lifecycle-target-semantics.md`.

## The discriminating record facts

The convention is pinned against data a zero-based reading cannot explain.
**Each of the four reconciled records spells an address equal to its own
block count** — in range exactly under the one-based rule, one past the
last index under the other:

| Record | Blocks | Site spelling the count | Resolves to |
| --- | --- | --- | --- |
| `zbd/c1/m02` | 50 | `OBJECTIVE13` `WAKE … [14, 50]` | record 49 = `OBJECTIVE50` |
| `zbd/c1b/m03` | 55 | `OBJECTIVE21` `NAP … [55]` | record 54 = `OBJECTIVE55` |
| `zbd/c1/m04` | 52 | `OBJECTIVE23` `WAKE … [37, 52]` | record 51 = `OBJECTIVE52` |
| `zbd/c2/m01` | 82 | `OBJECTIVE67` `WAKE … [68, 71, 82]` | record 81 = `OBJECTIVE82` |

M06 carries a second discriminator, already pinned by its own suite and
re-pinned by `accept_objaddr_`: **nothing in its record spells `50`**
although block 50 naps `51`, so under an index reading the `INSTANTLOSS`
latch (block 51) would have no completion edge at all.

Every spelled address in all four records resolves through the production
rule (`walk` counts: M02 63 over 42 sites, M03 81 over 55 sites under the
eight measured `dec` keys — M03's two `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`
sites stay out of the address walk because their decrement is unmeasured —
M04 58 over 50, M06 107 over 70).

## Files

- `crates/cs_app/tests/campaign/objaddr.rs` — new suite (2 tests).
- `crates/cs_app/tests/campaign/main.rs` — wiring only (`mod objaddr;`).
- `crates/cs_app/tests/campaign/m02_b.rs`, `m04_b.rs`, `m06_b.rs` — the
  reconciled assertions and comments above.
- `crates/cs_app/tests/campaign/evidence.rs`,
  `crates/cs_app/tests/campaign/evidence/m06_b_fu3.rs` — the evidence
  harness and its module wiring.
- `docs/findings/2026-10-08-m02-b-compatibility-gaps.md`,
  `…-m04-b-compatibility-gaps.md`,
  `2026-10-06-m01-lc-directive-a-….md`,
  `…-directive-b-….md` — the corrections above.
- `docs/findings/evidence/M06-B-FU3.json` — the committed copy of the
  acceptance report (artifacts stay in `private/evidence/M06-B-FU3/`).

## Test inventory (`accept_objaddr_`, 2 tests)

| Test | Capability | What it pins |
| --- | --- | --- |
| `accept_objaddr_a_spelled_address_is_the_one_based_block_number` | synthetic (CI) | `resolve_objective_address`: `1 → record 0`, `objectives → record objectives − 1`, every interior address maps to `a − 1` and round-trips through `address_of`; `0` and `objectives + 1` refuse by name with the spelled address and the count in `AddressRefusal` |
| `accept_objaddr_every_reconciled_record_spells_its_own_block_count` | retail | per record (`zbd/c1/m02`, `zbd/c1b/m03`, `zbd/c1/m04`, `zbd/c2/m01`): the measured rule finds the control member, blocks number `1..=count` contiguously, every spelled address resolves, and the site spelling the count itself is named with its block and key; plus M06's second discriminator (nothing spells 50, block 50 naps 51) |

Both call production code (`read_control_member`, `objective_record`,
`zrd_flat_fields`, `objective_block_number`, `resolve_objective_address`,
`address_of`). The retail test re-derives its addresses from `$CS_GAME_DIR`
by an independent grammar walk; no expected value is read from the
assertion it checks.

## Residual unknowns

* What the original *observes* when a genuinely out-of-range stored index
  reaches `0x469af0` — a crash, a silently wrong state or nothing visible —
  is not decidable from the code (#802 records it); no retail record spells
  such an address, and this port refuses it by name.
* `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` (M03 blocks 13/14) is not among the
  eight keys measured as `dec`'d; its integer children are unmeasured and
  its sites stay refused as unknown host calls — unchanged here.
* Whether the console `objective`/`objective_status` commands agree with
  the directive parse is #802's open question, not this task's.

## Checks

Run on this branch; see `docs/findings/evidence/M06-B-FU3.json` for the
recorded acceptance run.
