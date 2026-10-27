# T2117 — the end of a projectile's life: RANGE end, teardown, CLUSTER_SIZE

Follow-up of #454 (claims `t454.limit.range_end_removal`,
`t454.limit.session_change`, `t454.limit.swarm`; finding
`2026-10-25-f28-guidance-loss-per-weapon-audit.md`).

Every statement is **static analysis** of the owner-supplied decrypted
executable (`$CS_ENGINE_IMAGE`, base `0x400000`; linear disassembly with
Capstone in skip-data mode, not committed). Nothing is original-run behaviour;
claim level `implemented`/checked, never `verified_original`. The byte windows
and caller inventories are pinned by `accept_t2117_image_pins_the_lifetime_end_and_teardown`
(`crates/cs_content/tests/accept_t2117_guided_rocket_lifetime_end.rs`), the
record facts by `accept_t2117_flare_declares_no_range_and_cluster_census`.

## Vocabulary

The weapon table (`[0xa1d78c]`, stride `0x214`) holds one record per weapon;
each owns a projectile list at `+0x88`. A projectile node (stride `0x694`, from
the pool `[0xa1d794]`, free list `[0xa1d798]`) carries a state at `+0x20`:
`1` live, `0` dead, `2` set by the special-case path `0x5b0970`. The per-frame
driver `0x5ac4e0` calls the step `0x5af720` for every record, then the cleanup
walk `0x5af900`. The weapon record's `+0x1c` is `RANGE`, `+0x20` its square; the
node's `+0x664` is the distance flown and `+0x668` its age in seconds.

## `t454.limit.range_end_removal` — measured

The step (`0x5b01e6`) compares the distance flown with record `+0x1c`. Once it
is no longer below `RANGE`:

| weapon flags | outcome |
| --- | --- |
| `LOCK_ON` (`0x8000`) and `EXPIRES` (`0x80`) clear | detonates: `0x5b02ba` → `0x5ac3a0`, then state `0` |
| `LOCK_ON` with `EXPIRES` set, or not `LOCK_ON` | `DETONATE_AT_RANGE` (`0x2000000`) set: detonates as above; clear: **silent removal** at `0x5b0318` (state `0`, no explosion) |

(The flag-to-key mapping comes from the record parse, `DETONATE_AT_RANGE` at
`0x5adbde`, `EXPIRES` at `0x5adc8c`; each key sets or clears its bit by value.)
All twelve guided records declare no `EXPIRES` (#454), so all twelve detonate at
the end of `RANGE`: the silent path exists in the code but no guided retail record
takes it.

A node with state `0` is skipped by the step (`0x5af73c`). The cleanup walk
(`0x5af917`) hands it to `0x5b08d0`, which on its first call stamps the node with
the next global frame number and returns `0`; one frame later it frees the node's
two effect handles (`+0x10`, `+0x14`), detaches the model (`0x4cd6d0`) and returns
the node to the pool (`0x5aed40`), and the walk unlinks it. So a removal is
**invisible for one frame, then final**.

### The flare `wep_15` declares no `RANGE`

The record parse starts with `+0x1c = 500.0f` (`0x5ad80b`); the `RANGE` key
overwrites it only when present (`0x5adf90`..`0x5adfb8`). The census test shows
`wep_15` is the only guided record without the key. The flare therefore has an
end of range at **500 m**, not none. Its `DETONATION_TIME` of 2 s (#454) is reached
first at any plausible speed, so its end of range is the timer; the 500 m default is
the fallback and detonates (flare is `LOCK_ON`, no `EXPIRES`).

## `t454.limit.session_change` — measured

The teardown `0x5ae820` (entries `0x5ae810` from `0x5ad610`, `0x5ae950` from
`0x464a96`; no other caller, no data reference) calls the stop `0x5ae690(0)`
first. The stop walks each record's `+0x160` list and destroys its entries
(`0x5ae6cc`), then walks every pool node and destroys the visual at slot `+0xc`
(`0x5ae711`, detaching with `0x4cd6d0` when `+0x54 == 1`), then calls `0x5b16d0`.
The teardown then frees the weapon table and the pool and zeroes their globals
(`0x5ae8a4`). `0x5ad4e0` (callers `0x4a7fe3`, `0x5b57c3`) zeroes the same globals
on initialisation.

Consequences, as far as the code shows: a rocket in flight at the teardown is
**discarded**, not detonated, not damaged and not handed on — no
`0x5ac3a0`, no projectile callback (`+0x688` is called only in the step,
`0x5af757`; no `+0x688` displacement lies in `0x5ae690..0x5ae960`). The new
session builds its table and pool from empty (`0x5ae5ec` calls the stop with 1).

## `t454.limit.swarm` — partly measured

`CLUSTER_SIZE` is parsed into the weapon descriptor (`[weapon+0x210]`) at `+0x0c`
(`0x4baa00`). The launch routine `0x5aef40` (six direct callers, #2116) never
loads the descriptor pointer and makes exactly one node per call. The field's
readers found are the getter `0x4bad90` (callers `0x444340`, `0x4b25b1`: the
ammunition stack and its display, matching the `CLUSTER_SIZE` = rocket count of
`weapon_counts.rs`) and a byte test at `0x4ab16b` (a non-zero value enables the
mount). So `CLUSTER_SIZE` is an ammunition count, not a number of nodes per
launch. 15 rocket records declare it (`wep_04`..`wep_15`, `wep_24`, `wep_26`,
`wep_27`).

## Still unknown (recorded, not claimed)

| unknown | claim id | affects |
| --- | --- | --- |
| a reader of descriptor `+0x0c` through a cached pointer is outside the pattern scan (`mov r,[x+0x210]` followed within twelve instructions by `[r+0xc]`) | `t2117.limit.cluster_size_meaning` | `wep_04`..`wep_15`, `wep_24`, `wep_26`, `wep_27` |
| that `0x5ae810` / `0x5ae950` are the mission-end and session-change paths | `t2117.limit.teardown_callers` | every guided rocket in flight at a mission end |

Static evidence only; claim level `implemented`. No agent review replaces the
owner's approval.
