# F28-A audit: the guidance-loss contract against the original (per-weapon lost-target behavior)

Date: 2026-10-25. Task: #454. Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
non-negotiable behavior 4. Predecessors: `docs/findings/2026-10-01-f28-a-ordnance-behavior-and-effect-registry.md`,
`docs/findings/2026-10-11-f28-c-2-lost-target-blast-measured.md` (which measured the
projectile step and the blast; this audit does not repeat it). Capabilities used: `retail`
(`$CS_GAME_DIR`, and the owner-supplied decrypted executable `$CS_ENGINE_IMAGE`, base
`0x400000`). Not used and not claimed: any run of the original. Everything below is what
the files and the image *contain*, so nothing here is `verified_original`.

Files: `crates/cs_content/src/ordnance.rs` (extend: `OriginalRocketBlast::range`,
`OriginalLostTargetRule`, `OriginalRangeEnd`, `OriginalRocketBlast::lost_target_rule`),
`crates/cs_content/tests/accept_t454_guidance_loss.rs` (new, module of `tests/main.rs`),
`crates/cs_content/tests/evidence_report_t454.rs` (evidence harness), `docs/findings/evidence/T454.json`
and this file. No `cs_sim` change: see "What the contract needs" below.

## The answers

### Per weapon: what a lost target does — measured

The original has one rule, not one per weapon. A projectile's target is a pointer at
projectile `+0x70`; "lost" is that pointer being zero (the removed-object sweep `0x5ac0c0`
zeroes it, pin `0x5ac0ec`). With a zero pointer the step skips the steering call
(`0x5af7f9`: `cmp [esi+0x70], ebx; je`) and the `DETONATION_DISTANCE` proximity branch
(`0x5b021b`). The rocket keeps its last heading; only the `DETONATION_TIME` timer and the end
of `RANGE` can detonate it. What differs per weapon is only which of those the *record*
declares. `OriginalRocketBlast::lost_target_rule` derives it; the twelve guided (`LOCK_ON`)
records of `weapons.zrd` are:

| record | proximity trigger lost (m) | timer (s) | `RANGE` (m) | end of range |
| --- | --- | --- | --- | --- |
| wep_04 | 30 | none | 900 | detonates |
| wep_05 | 15 | none | 1000 | detonates |
| wep_06 | 15 | none | 1000 | detonates |
| wep_07 | 50 | none | 1000 | detonates |
| wep_08 | 35 | none | 1000 | detonates |
| wep_09 | 30 | none | 900 | detonates |
| wep_10 | 15 | none | 1000 | detonates |
| wep_11 | 15 | none | 10000 | detonates |
| wep_14 (torpedo) | 1 | none | 1200 | detonates |
| wep_15 (flare) | none | 2 | none declared | detonates |
| wep_24 | 15 | none | 1000 | detonates |
| wep_27 | 20 | none | 900 | detonates |

"Detonates" at the end of range is the step's rule for a locked-on weapon without `EXPIRES`
(`0x5b01f9`..`0x5b0211`); no retail rocket declares `EXPIRES` (the 41-key census of
`accept_t548_`), so the other branch (`EXPIRES` without `DETONATE_AT_RANGE`) does not occur.
In terms of the Rust contract: the original is none of the three designed
`LostTargetBehavior` values as a stored choice — it is "coast, then detonate on the timer or
at range end" for all twelve, and `Detonate` (immediate) is not an original behavior.

Source spans: record keys `ZBD/zrdr.zbd` `weapons.zrd` via `read_original_rocket_blasts`;
code `0x5ac0c0`..`0x5ac115` (sweep), `0x5af7f9` (steering guard), `0x5b021b`/`0x5b029a`
(proximity skip, timer). Independent probe: the data table comes from the record reader,
the code claims from a byte scan of the image (below); they share no code.

### Re-acquisition after a temporary loss — absent in the projectile module, not excluded elsewhere

Byte scan of the image (`accept_t454_image_has_no_target_store_in_the_projectile_module`,
every `89 /r disp8` and `c7 /0 disp8` store to `[reg+0x70]` in `0x5ab000..0x5b2000`): exactly
three, `0x5ac0f1` (the sweep's clear), `0x5af610` (the launch zeroing, `mov [edi+0x70], ebx`
with `ebx = 0`) and `0x5ad95e` (a constant in an unrelated record). The per-frame step
(`0x5af720`) and the steering routine (`0x5af960`) only read the pointer. A linear
disassembly (Capstone 5.0.7, not committed) of the whole `.text` found no function that walks
an actor's projectile list (`[actor+0x88]`, the walk the sweep uses) and stores to `+0x70`.
So once the pointer is cleared nothing in the projectile module sets it again: loss is
terminal there, matching `GuidanceTracker::lose`. **Not excluded:** the launch zeroes the
pointer and no store in the module assigns a target, so the *first* assignment is made by
code this audit did not locate; a re-acquiring writer outside the module cannot be ruled
out from this scan (linear disassembly of a mixed code/data image can desynchronise). That
is the unknown `t454.limit.target_assignment`. Affected content: all twelve guided records.

### Per weapon or per swarm — per projectile, as far as measured

Each projectile has its own `+0x70`; the sweep tests and clears every projectile of every
actor individually. A lost target therefore affects exactly the projectiles that held the
removed object, one by one. Whether `CLUSTER_SIZE` records launch one projectile per
sub-munition or one with an expansion was not followed (`t454.limit.swarm`).

### Despawn versus destruction versus session change

The sweep is run "when an object is removed" (`0x5ac0c0`'s callers were not enumerated), so
it is the same rule whether the target was destroyed or despawned. A session change is not
a code path found in the projectile module: unmeasured (`t454.limit.session_change`).

## What the contract needs

Nothing changes in `GuidanceRule`/`GuidanceTracker`: terminal-per-item loss is what the
module shows, and re-acquisition is not evidenced, so no declared option is added (the task
forbids one without evidence). The designed `Detonate`/`Coast`/`Disarm` values stay declared
options and none of them is claimed to be the original's. The original's "coast then timer or
range end" is expressible with the existing `Coast` plus the item's declared lifetime; mapping
the twelve rules into a catalogue is F28-D's concern and is **not** done here because the
metre values need no conversion but the lifetime in ticks needs a speed (`VELOCITY`), whose
audit is separate.

## Still unknown

| unknown | claim id | affects |
| --- | --- | --- |
| where a target is first assigned; whether any external routine re-assigns | `t454.limit.target_assignment` | the twelve guided records |
| in-flight guided rockets on a session change or mission end | `t454.limit.session_change` | every guided rocket |
| what removes a projectile at the end of `RANGE`; the flare declares no `RANGE` | `t454.limit.range_end_removal` | wep_15, any non-detonating range end |
| `CLUSTER_SIZE` launch shape | `t454.limit.swarm` | records declaring `CLUSTER_SIZE` |

## Not claimed

No original-run behavior, no change to the declared catalogue, no new `cs_sim` rule. Awards
at most **checked**; no agent review replaces the owner's approval.
