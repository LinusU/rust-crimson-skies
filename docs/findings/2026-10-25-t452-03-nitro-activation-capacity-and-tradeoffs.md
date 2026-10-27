# T452.03: nitro activation, capacity, consumption, recovery and tradeoffs (static)

Date: 2026-10-27. Task: #2129. Predecessors: `2026-10-03-f28-d-original-ordnance-catalogue.md`,
`2026-10-11-f28-ae3-nitro-refused-recovery.md`, `2026-10-25-nitro-ledger-is-the-boost-authority.md`.
Capabilities: `retail` and the decrypted executable `$CS_ENGINE_IMAGE` (base `0x400000`). Static
analysis only: nothing here is an original run, so nothing is `verified_original`; awards at most
**checked**. No `engine-facts.toml` rows (the task does not list that file); the pins are byte
windows in `crates/cs_content/tests/accept_t452_03_nitro_rule.rs`, and the measured record is
`cs_content::ordnance::ORIGINAL_NITRO`.

## Where nitro lives

Nitro is **not** a hardpoint ordnance record. No file in the installation carries its numbers
(`nitro_boost`/`nitro_charge`/`nitro_backplate`/`nitrogauge` in `PLANES.ZBD` are model and HUD node
names). It is actor state:

| field (actor) | meaning | evidence |
| --- | --- | --- |
| `+0x946` byte | the airframe has a booster; copied from the plane record `+0x150` (`0x475c94`) which the plane parser sets from the plane-record key `nitro` (`0x47a79c..0x47a7b9`, non-zero) | static |
| `+0x947` byte | the booster is running | static |
| `+0x8b4` float | capacity | static |
| `+0x8b8` float | current charge | static |
| `+0x8bc` float | consumption per delta | static |
| `+0x8c0` float | recovery per delta | static |
| `+0x28c` float | elapsed-while-running timer, cleared on start (`0x4b219a`) | static |

## Capacity, consumption, recovery (VA `0x4b02c4`, actor construction)

`mov eax, 0x41f00000` stored to `+0x8b4` and `+0x8b8`; `+0x8bc = 0x40800000`; `+0x8c0 = 0x3f800000`.

| value | bytes | number | provenance |
| --- | --- | --- | --- |
| capacity, and initial charge (full) | `00 00 f0 41` | **30.0** | static |
| consumption per delta while running | `00 00 80 40` | **4.0** | static |
| recovery per delta, every step | `00 00 80 3f` | **1.0** | static |

A search of the whole disassembled `.text` for direct stores to `+0x8b4`, `+0x8bc`, `+0x8c0` finds
only this site (struct block copies were not excluded), so the pool is the same for every airframe
with `nitro` set. The plane key `fuel` (→ actor `+0x130`) is a different quantity and not claimed
as capacity.

## Per-step update (`0x49f810..0x49f8a4`)

With `dt` the global at `0x9ad744`:

1. running: `charge -= dt * 4.0`
2. always: `charge += dt * 1.0` — recovery is **not** suspended while running (net −3.0 per delta)
3. clamp: `charge > capacity` → capacity; `charge < 0` → 0
4. `capacity * 0.05` (`0x6034d8`, `0x3d4ccccd`) `> charge` → `0x4b2110(actor, 0)` (cut the booster)

The HUD gauge reads `charge / capacity` (`0x49f8bc`).

## Activation rule — neither "while held" nor a fixed burn

Player decision (`0x487e91..0x487efc`, only for an airframe with `+0x946`; input `0x12` is read
through `0x537c10` unless `[0x71d55c]` is non-zero, mask `& 3`):

1. `capacity * 0.05 > charge` → cut.
2. else `capacity * 0.99` (`0x6080a8`, `0x3f7d70a4`) `> charge` → keep running if already running,
   otherwise **no start**.
3. else (charge ≥ 99% of capacity) → control pressed starts it; otherwise nothing.

So: **start needs a nearly full tank (≥ 99%) and the control; once running it continues until the
charge falls below 5% of capacity.** Nothing in this block cuts it when the control is released. The
burn is therefore a charge-bounded run, not `DeclaredNitroActivationRule::WhileHeld` nor
`FixedTicks`. `DeclaredNitroActivationRule` is not changed (cs_sim and cs_app match on it and are
outside this task's owner paths); `OriginalNitro::player_command` is the measured rule and a
follow-up should add a variant (`UntilCharge { start_min, stop_below }`) with the lowering.
Burn length from full at a steady delta: 10 deltas (30 → 27 → … → 3 → 0), runs of a given charge
follow from `step_charge`. Refusal: the set-running routine `0x4b2110` refuses a start while actor
status `+0x2dc` has bit 1 (value 2) set (`0x4b2136`), and a status clearing that bit calls it with 0
(`0x4b16f1`). That is the only recovery-on-refusal-related fact found; claim
`f28.d.limit.nitro_recovery` (recovery on a *refused request while the tank is empty*) stays
unmeasured: the recovery in step 2 is unconditional, so a refused tick recovers like any other, but
that is the code's consequence, not an observed run.

## Tradeoffs and extra thrust

While `+0x947` is set:

* speed/thrust step `0x48fc40` (`0x48fcac..0x48fcc4`) uses **1.8** (`0x3fe66666`) in place of the
  engine-record value `actor+0x128` (set from the plane record's `engine` entry, `0x47acf8..0x47ad01`)
  and multiplies the result of `0x41ada0` by **0.8** (`0x3f4ccccd`); otherwise `+0x128` and `1.0`.
* a second consumer (`0x4b1c51..0x4b1c70`) replaces a computed ratio with **1.25** (`0x608b9c`) and
  stores **1.17** (`0x3f95c28f`) to a local.

These are the measured nitro terms, but their units and targets are not decoded, so
`DeclaredNitroParameters::extra_thrust_n` and `authority_multiplier` stay **unknown**: 1.8 is a
replacement of an engine-record value (not newtons), 0.8 and the 1.25/1.17 pair are factors on
quantities the finding did not identify. No drag/handling penalty can be asserted from them.

## Still unknown

* The unit of `dt` (`0x9ad744`): seconds is assumed from status timers (`+0x2e0 -= dt`).
* What `+0x128`, the 0.8 result factor and the 1.25/1.17 pair scale (thrust, speed cap, turn rate).
* The meaning of the `& 3` mask of input `0x12` (held bit versus edge bit).
* Other callers of `0x4b2110`: AI (`0x420928` starts, `0x4899d1` cuts, keyed by the per-behaviour
  table at `0x71b214`, stride 28), `0x49229c` and `0x492adb` (re-apply the current state). The AI
  `nitro_evade` behaviour and the visual `nitro_boost`/`nitro_decay` nodes (actor `+0x280/+0x284`)
  were not followed. The gate on `+0x288`/`+0x27c` in `0x4b2136..0x4b2153` was not decoded.
* Whether a cut by status bit 1 (`+0x2dc & 2`) is the FLASH status of
  `2026-10-19-t1601-status-bit-consumers.md`.

## Not claimed

No original-run behaviour; no `cs_sim`/`cs_app` change; no number outside the table above.
