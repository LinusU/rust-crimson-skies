# #709: the airframe's stored nose is `−Z`, and the playtest's Bloodhawk was drawn tail first

Date: 2026-10-06. Task: #709 `PLAYTEST-NOSE-FIX`. Owner feedback (`human_play`,
owner, 2026-10-06): in `cs --playtest --cs-path "$CS_GAME_DIR" --world c1c` "the
plane is flying backwards". Capabilities used: **`retail`** (read the owner's
installation read-only) and **`gpu`** (real Metal captures). Nothing here is
`verified_original`: no original run happened, and the original engine's own
airframe orientation is still an open question for an original run.

## What the code did, and why it was wrong

`crates/cs_app/src/playtest_retail.rs` applied one half turn about `+Y` to every
drawn airframe part, on the claim that the stored airframe's nose is `+Z`. That
claim was inferred from the **propeller**: "the propeller disc lies in the stored
`x`/`y` plane and its node sits at `z = +4.80`, the fuselage mesh's own maximum".

The inference is unsafe, and for `bloodhawk` it is false: the propeller is at the
*tail*. The half turn put `−Z` (the real nose) at the back, so the drawn aircraft
travelled nose away from its direction of travel.

## The measurement: what else in the container says

Read with production code only — `read_playtest_sources` → `aircraft_graph` →
`SceneGraph::build` (the same readers the playtest spawns through) — over all
**eleven scene airframes** of `ZBD/planes.zbd`. Two authored anchors per
airframe, neither of which is the propeller:

* the **cockpit**: the authored `pilot` / `pilot_pos` (and `pf_canopy` /
  `canopy`) node;
* the **tail**: the authored rudder and elevator surfaces — what the container's
  own authored `tail` node groups where it groups any (it binds a mesh and has no
  surface children in `firebrand`, `autogyro` and `piratefighter`), and the same
  surfaces parented under `leftwing` / `rightwing` / `uprights` / `nearest` where
  the modeler put them elsewhere.

Composed `z`, canonical metres, airframe frame; the parent in brackets is what
makes each surface a *tail* surface rather than a name:

| airframe | cockpit `z` | rudder `z` (parent) | elevator `z` (parent) | propeller `z` | tail aft of cockpit? |
| --- | --- | --- | --- | --- | --- |
| `bloodhawk` | −1.630 (`pf_canopy` −1.163) | +3.032 (`tail`) | **−4.195 / −4.201** (`nose`) | **+4.802** | yes — the `tail`-grouped rudder |
| `warhawk` | +5.012 | +2.806 (wingtip, `leftwing`/`rightwing`) | +9.130 (`tail`) | −4.553 | yes — the `tail`-grouped elevators |
| `fury` | +1.000 | +1.788 (`leftwing`/`rightwing`) | +3.777 (`tail`) | −3.300 | yes — both |
| `kestrel` | −1.453 | +6.593 (`tail`) | +5.302 (`tail`), +0.190 (`nose`) | −3.562 | yes |
| `avenger` | −0.488 | +5.531 (`uprights`) | +5.142 (`tail`) | −4.586 | yes — both |
| `balmoral` | −1.435 | +13.731 (`uprights`) | +12.526 (`tail`) | −2.747 | yes — both |
| `brigand` | +0.500 | +3.214 (`tail`) | +2.971 (`tail`) | −2.100 | yes — both |
| `firebrand` | −1.186 | +3.654 (`leftwing`/`rightwing`) | +4.505 (`nearest`) | **+5.141** | yes — both |
| `peacemaker` | −0.677 | +1.641 (`leftwing`/`rightwing`) | +6.879 (`tail`) | **+3.600** | yes — both |
| `autogyro` | −0.441 | — (none) | +3.412 / +1.718 (`nearest`) | −1.730 | yes |
| `piratefighter` | 0.000 (`pilot_pos`), canopy +0.492 | +3.514 (`leftwing`/`rightwing`) | **−3.724 / −3.664** (`leftwing`/`rightwing`) | **+3.865** | yes — the rudder |

Readings:

* **In all eleven the tail surfaces compose aft of the cockpit** — the `tail`
  node's own group where it has one, the rudder or the elevators wherever else
  the modeler parented them where it does not. That is one convention: the
  container's tail is at `+Z`, so its nose is at `−Z`. This is the rule the fix
  encodes, and it is one rule rather than a per-aircraft constant.
* **Two airframes also carry forward horizontal surfaces ahead of the cockpit**:
  `bloodhawk` (`−4.195`, parented under its authored `nose` node) and
  `piratefighter` (`−3.724`, under the wings). Both are the two whose propeller
  composes behind their rudder, so those surfaces are the front ones of those
  layouts; the direction question is settled by their rudders, which stay aft
  (+3.032, +3.514). Likewise `warhawk`'s wingtip "rudders" (+2.806) are ahead of
  its aft-set cockpit (+5.012) while its `tail` elevators are at +9.130, and
  `kestrel`'s `nose` node groups an elevator at +0.190 that is still aft of its
  cockpit — so the **group names alone are not used as a positional anchor**; the
  cockpit-versus-tail comparison is.
* **The propeller is not usable as an orientation signal**, which is the point.
  Its composed `z` runs from −4.586 to +5.141: it sits at the negative end for
  seven airframes and behind the rudder for `bloodhawk` (+4.802 vs +3.032),
  `piratefighter` (+3.865 vs +3.514) and `firebrand` (+5.141 vs +3.654). Reading
  their propeller as "the nose" turns those around, which is exactly what happened
  to `bloodhawk`.
* **`bloodhawk`'s markers corroborate**: `exhaust1/2` at `z = +1.320`/`+1.289`
  behind the cockpit, the four gun markers `firepoint1..4` at
  `z = −4.709 … −6.237` ahead of it, and `pf_canopy` at `z = −1.163`.
* **Corroboration, not the argument:** the `cf_light` marker (whose meaning is
  unmeasured) sits at the positive-`z` end in every airframe (`+3.0 … +9.0`).

Recorded because it does **not** fit cleanly, and is therefore not used as
evidence either way: `bloodhawk`'s `cockpit_camera` marker is at `z = −0.200`,
i.e. behind its `pilot` node at `z = −1.630`, while `warhawk`'s is ahead of its
pilot (+3.296 vs +5.012); and `autogyro`'s eight `firepoint*` markers are behind
its cockpit (`z = +2.16 … +2.53`).

## The fix: one rule

```rust
pub const STORED_AIRCRAFT_NOSE_AXIS: [f32; 3] = [0.0, 0.0, -1.0];   // measured
pub fn nose_mapping(stored_nose: [f32; 3]) -> Result<Quat, PlaytestError>
```

`nose_mapping` is the **yaw about `+Y`** that carries a stored nose axis onto
`cs_sim::flight::BODY_FORWARD` (`[0, 0, −1]`): for the measured `−Z` it is the
identity (nothing is turned), for `+Z` it is exactly the old half turn, and a
non-finite or vertical axis is refused as `PlaytestError::NoseAxis` rather than
guessed. It is applied once per aircraft to the composed transform of every drawn
part (`spawn_pose` → `RetailContent::visual_rotation` → `AircraftPartAsset::oriented`
in the free flight, and the scene parent in the capture scene), so the area and
the camera path are untouched.

Files: `crates/cs_app/src/playtest_retail.rs` (constant, rule, `spawn_pose`, the
module and error docs), `crates/cs_app/src/playtest/retail.rs` (`visual_rotation`
doc), `crates/cs_app/tests/playtest_retail.rs` (the spawn test now pins the rule
instead of the half turn), `crates/cs_app/tests/playtest_retail/nose.rs` (new),
`docs/PLAYTEST.md`, `docs/PLAYTEST-RETAIL.md`, this finding.

This finding **supersedes** the propeller-based nose reading in
`docs/findings/2026-10-05-t648-playtest-retail-scene.md` and
`docs/findings/2026-10-05-t665-full-aircraft.md`; those records stay as what those
tasks measured at the time.

## The tests (`accept_playtest_nose_`, all four pass)

| test | needs retail/GPU | measured |
| --- | --- | --- |
| `a_synthetic_nose_on_either_stored_axis_is_drawn_leading` | no | for stored `−Z` **and** `+Z`: `nose_mapping` puts the nose on `BODY_FORWARD`, is a yaw (`q·Y = Y`), is a rotation, and the production `AircraftPartAsset::oriented` placement of a nose reference 2 m along the stored axis lands at `body_forward · 2`; the measured constant maps to `Quat::IDENTITY`; a vertical and a non-finite axis are refused as `NoseAxis` |
| `the_drawn_nose_leads_the_tail_in_level_cruise` | retail | 240 fixed ticks of level cruise, speed 64.245 m/s, heading −Z; drawn `pf_canopy` `(0, 0.828, −1.163)` vs drawn `l_rudder1` `(0, −1.009, +3.032)`, `along_velocity = +267.426` |
| `the_airship_is_not_mirrored_by_the_read` | retail | all **793** composed transforms of the pinned `piratezep` subtree have a positive determinant; `front_door_left = (−9.698, −32.416, −4.142)` and `front_door_right = (+9.698, −32.416, −4.142)` — the authored sides are kept and the pair straddles the hull centreline symmetrically |
| `gpu_chase_capture_shows_the_nose_leading` | retail + GPU | three views captured through `capture_playtest_views`; see below |

**Failure check.** With the production `spawn_pose` temporarily restored to
`main`'s half turn (the only difference on that path), the cruise test fails:

```
PLAYTEST-NOSE-FIX ticks=240 speed=64.245 nose_reference=Vec3(1.017032e-7, 0.8282425, 1.163349)
  tail_reference=Vec3(-2.6509497e-7, -1.0087719, -3.032333) along_velocity=-271.596
panicked: ... must lie ahead of the tail ... got -271.596 — the aircraft is travelling tail first
```

and with the fix it passes with `+267.426`. The half turn is what `main` has
(`AIRCRAFT_NOSE_AXIS = [0, 1, 0]`, unchanged since before the owner's run at
`70fff350`), so the criterion "fails on main" is met on that path.

## GPU frames (paths and hashes only; nothing original is committed)

Both halves are the production `capture_playtest_views` over
`spawn_playtest_scene` with `PlaytestConfig::documented()` (640 × 480, view
`chase` = abeam-and-outboard of the spawn), written under the workspace's
git-ignored `private/`:

| | before (`main`, half turn) | after (this branch) |
| --- | --- | --- |
| driver | `accept_playtest_retail_retail_c1c_area_and_bloodhawk_mesh_spawn_and_capture` on `1a41452b` | `accept_playtest_nose_gpu_chase_capture_shows_the_nose_leading` |
| chase | `private/evidence/PLAYTEST-NOSE-FIX/before/playtest-retail-c1c-chase.png` — `9914346b643f27df1fbf83b9633641ec2ee230a26cf1bb5d58cd65f8fbbe11d9` | `private/evidence/PLAYTEST-NOSE-FIX/after/playtest-retail-c1c-chase.png` — `4f69ab1a3742a3018e0c3502ba66fb581c96ebbae006994233ca6d3ba955ecd1` |
| quarter | `…/before/playtest-retail-c1c-quarter.png` — `f164f92484e5d7f4044046f2c4675e0bf16c3aa671a7b25b7d1bc2ab52744b3c` | `…/after/playtest-retail-c1c-quarter.png` — `e33ebd4a4d0b90335e3eff617c11e6e7b17727d4e1f63c13be1970b50248dce3` |
| overview | `…/before/playtest-retail-c1c-overview.png` — `c97c699bc2bc69e185c319659f24c2d794860b8154bc62fd3ce725998d622c40` | `…/after/playtest-retail-c1c-overview.png` — `3705533eb18beebdb4e481f902407a0a053d9071a01faa7ce53cd10ba248a392` |

The frames differ exactly where the aircraft is: the measured aircraft pixels of
the `chase` view are **11 627 before** and **11 359 after** (the environment is
the same scene), and the two frames are the same aircraft turned 180° about the
vertical axis. The `chase` eye sits abeam and outboard of the spawn — slightly
behind it along the flight direction — so each frame is the aircraft's side
profile with its travel direction fixed in the frame; in the after frame the
canopy and the forward gun markers are on the leading side and the rudder and the
rear propeller disc on the trailing one, while in the before frame those two ends
are exchanged. Which end leads is the numeric claim of the cruise test above
(`along_velocity = +267.426`), not the picture; the pictures are for the owner's
eyes.

## Mirroring (acceptance 4)

The area and the aircraft go through the same coordinate adapter
(`playtest_adapter`, the canonical self-map: identity axis map, right-handed,
`+Y` up), and the nose mapping is a quaternion — a rotation, never a flip. Both
sides are measured rather than asserted from the code: see the table above —
793 of 793 composed area transforms have a positive determinant, and the
authored left/right door pair keeps its sides. **Result: the airship is not
mirrored**, and the fix mirrors nothing.

## For PLAYTEST-PROP-SPIN (#710)

The drawn propeller is not at the nose. Measured: `bloodhawk`'s `staticprop1`
node composes to `(0, −0.019, +4.802)`, behind `lrudder1` at `+3.032`. A spin
therefore has to be about the body's `Z` axis **at that rear pivot** — the disc
turns in place at the tail — and not about a nose-mounted hub. The spin axis
direction (which way the blade turns) is unmeasured and still needs its own
evidence; this finding fixes only where the disc is.

## Not claimed

* Not `verified_original`, not `human_play`: the owner's report is a human
  observation of *our* build; nothing here shows how the original engine orients
  an airframe.
* The rule rests on the container's own authored node names (`tail`, `pilot`,
  `pf_canopy`) being what they say they are, which is a reading of the bytes by
  the production readers — `ObservedTool` — not a decode of the original's
  orientation code.
* Still provisional: one aircraft (`bloodhawk`), a static propeller, a designed
  spawn and camera set, and the `ORIGINAL ASSETS / DEVELOPMENT FREE FLIGHT /
  PROVISIONAL TUNING` label unchanged.

## Commands run

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_playtest_nose_ --include-ignored
```

Sources: the owner's playtest report on task #709; `ZBD/planes.zbd` and
`ZBD/C1C/gamez.zbd` through the production readers; `specs/F11-*` (scene
hierarchy, LOD) for the graph contract; `docs/contracts/CLI-EVIDENCE.md`.
