# F16-A: units, typed time and coordinate adapters

Date: 2026-09-29. Task: F16-A "Define units, typed time and coordinate
adapters" (`specs/F16-coordinates-units-origin-management-and-clocks.md`,
section `### F16-A`). Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md`. Required capability: ordinary build/test
(the machine also has `retail`, `gpu` and `audio`; **none was used** — this
stage reads no original data, renders nothing and plays nothing).

## Files and the one observable failure (listed before editing)

- `crates/cs_types/src/space.rs` (new): `Meters`, `Radians`, `Winding` (+
  `CANONICAL_FRONT`, `flipped`, `label`), `SpaceError` (`NonFinite`,
  `NotUnit`), `UNIT_LENGTH_TOLERANCE`, `QUATERNION_LENGTH_TOLERANCE`,
  `WorldPosition` (`try_new`, `x`/`y`/`z`, `to_array`), `LocalPosition`
  (`try_new`, `ZERO`, `x`/`y`/`z`, `to_array`), `UnitVec3` (`try_new`,
  `x`/`y`/`z`, `to_array`), `Quaternion` (`try_new`, `from_axis_angle`,
  `rotate`, `components`), plus the `accept_f16_a_*` unit tests for the
  canonical convention and the boundary rejections.
- `crates/cs_content/src/coordinates.rs` (new): `Axis`, `Sign`, `SourceAxis`,
  `AngleUnit`, `RotationSense`, `SourceError` (`EmptyLabel`,
  `RepeatedSourceAxis`, `NonFiniteMetersPerUnit`, `NonPositiveMetersPerUnit`,
  `WindingReferenceAxisMismatch`), `SourceConvention` (`new`, `axes`,
  `winding_reference`, `meters_per_unit`, `angle_unit`, `rotation_sense`,
  `front_face`, `is_orientation_preserving`, `reverses_vertex_order`),
  `CoordinateSource` (`new`, `label`, `convention`, `origin`, `provenance`,
  `declared`), `SourceAdapter` (`new`, `declared`, `source`,
  `position_to/from_canonical`, `direction_to/from_canonical`,
  `normal_to/from_canonical`, `rotation_to/from_canonical`,
  `distance_to/from_canonical`, `angle_to/from_canonical`,
  `winding_to/from_canonical`, `reverses_vertex_order`), the declared
  round-trip tolerances, and the `accept_f16_a_*` unit tests for declaration
  validation.
- `crates/cs_app/src/origin.rs` (new): `OriginEpoch`, `OriginError`
  (`EpochExhausted`, `Space`), `WorldOrigin` (`new`, `epoch`, `position`,
  `local_of`, `world_of`, `rebased`), `local_round_trip_tolerance_m`,
  `LOCAL_ABSOLUTE_TOLERANCE_M`, `OriginChange` (`Rebase`, `Teleport`,
  `preserves_swept_continuity`) and its `accept_f16_a_*` unit tests.
- `crates/cs_sim/src/time.rs` (new): `TimeError` (`ZeroTickRate`,
  `NoSpeedUpAuthority`, `ClockPaused`, `TickOverflow`), `TickRate` (`new`,
  `ticks_per_second`, `dt_seconds`), `TimeDomain`, `PausePolicy`,
  `SpeedUpPolicy`, `ClockPolicy` (`single_player_simulation`,
  `multiplayer_simulation`, `ui_wall`, `media_unscaled`,
  `authoritative_gameplay`), `SimClock` (`new`, `with_tick`, `policy`,
  `rate`, `tick`, `is_paused`, `set_paused`, `advance`,
  `advance_fixed_ticks`) and its `accept_f16_a_*` unit tests.
- `crates/cs_content/tests/accept_f16_a_source_adapters.rs` (new): the AC01
  minimum scenario — round-trip a position, a normal and a quaternion through
  **every** declared source adapter within the declared tolerances — plus the
  forward-mapping, winding, failure-case and provenance tests.
- Wiring only (no logic): `crates/cs_types/src/lib.rs` gains
  `pub mod space;`, `crates/cs_content/src/lib.rs` gains
  `pub mod coordinates;` plus a doc paragraph, `crates/cs_app/src/lib.rs`
  gains `pub mod origin;` plus a doc paragraph, `crates/cs_sim/src/lib.rs`
  gains `pub mod time;` plus a doc paragraph.

**One observable failure:** if `SourceAdapter::rotation_to_canonical`
dropped the orientation sign (mapping the vector part with the axis map
alone), `accept_f16_a_forward_mapping_matches_the_declared_convention`
fails: the left-handed fixture's +90° yaw lands as −90° about canonical +Y,
and `accept_f16_a_round_trip_position_normal_quaternion_through_every_adapter`
alone would *not* catch it, because a wrong-but-consistent pair of
conversions still round-trips. Mutation probes (below) confirm it.

## Design decisions

- **The canonical convention is declared, not discovered.** Right-handed,
  +Y up, forward -Z, SI units, radians and CCW front faces are the spec's
  project convention (`F16` "Deliverable and interfaces"). Nothing here
  asserts anything about the original game: no original handedness, axis
  order, scale or angle unit has been measured with the three independent
  landmarks rule 1 requires, and that measurement is F16-D's.
- **A source convention is data, and adapters are derived from it.**
  `SourceConvention` declares a signed axis permutation
  (`[SourceAxis; 3]`: canonical X/Y/Z each read one source axis with a
  sign), the length scale (`meters_per_unit`), the angle unit, the rotation
  sense (right- or left-hand rule) and the front-face rule
  (`front_face` measured w.r.t. `winding_reference`). Every conversion is
  computed from that declaration, so a future measured format is one
  declaration plus provenance, not a second hand-written conversion path
  ("map into the canonical convention exactly once").
- **Validation refuses the declarations that cannot be mapped.** The axis
  map must be a permutation (no axis dropped or read twice), the scale must
  be finite and strictly positive, the winding reference axis must be the
  source axis that feeds canonical Z (otherwise a planar triangle's source
  winding has no canonical counterpart), and the label must be non-empty.
  Conversion boundaries reject NaN/infinity and non-unit normals and
  quaternions (`FLIGHT-PHYSICS`: "Reject nonfinite inputs at boundaries").
- **Positions, distances and angles.** Positions map through the signed
  permutation and the scale, distances through the scale alone and angles
  through the angle unit; normals and directions are unit vectors and map
  through the permutation alone (the scale is a single positive scalar, so
  there is no per-axis stretch that would need inverse-transpose).
- **Rotations carry two independent declarations.** A source quaternion
  `(x, y, z, w)` is interpreted under its declared `rotation_sense`
  (a left-hand-rule `+θ` about an axis is a right-hand-rule `−θ`), then its
  vector part is transformed as `det · M · v` — for an improper map
  (`det = −1`) the rotation axis is an axial vector, so it picks up the
  extra sign while the scalar part is unchanged. Both steps are involutions,
  so the round trip is stable *and* the forward test pins the absolute
  mapping (the round trip alone could not distinguish a consistently wrong
  pair).
- **Winding has a label and an order, and they are separate.**
  `winding_to_canonical` only reflects the declared front-face rule (it
  flips iff the source declares CW front faces), while
  `reverses_vertex_order` — `(front CW) XOR (improper map) XOR (winding
  reference maps to −Z)` — is what a consumer must apply when copying
  vertices, so that geometric winding and label agree and front faces stay
  canonical-CCW. The derivation is recorded in the module doc; the
  acceptance test checks it geometrically with real cross products rather
  than re-running the implementation's own algebra.
- **Three declared sources, none claiming original provenance.** An
  identity self-map (`Designed`), a right-handed Z-up degrees fixture and a
  left-handed Z-up centimeter/degrees fixture (both `SyntheticFixture`),
  each with `Provenance::designed`. They are *designed* declarations that
  exercise every branch (proper and improper maps, both rotation senses,
  both angle units, scale ≠ 1, both winding labels, vertex-order reversal
  both ways); they are attributed to no original file. A test asserts that
  no declared source claims `Origin::Installation`, so an
  `accept_f16_a_`-green build can never be read as "the original convention
  is known".
- **f64 world, f32 local, declared tolerance.** `WorldPosition` is f64
  relative to a declared `WorldOrigin` epoch; `LocalPosition` is f32 for the
  physics/render frame. The round-trip tolerance is a declared function of
  the value (`local_round_trip_tolerance_m`): f32 rounding of the local
  vector, f64 rounding of the origin addition and a 1 µm floor, so the test
  states a bound instead of an epsilon picked to fit.
- **Rebase and teleport are different types, not a comment.**
  `OriginChange::Rebase` preserves swept continuity and world identity;
  `OriginChange::Teleport` invalidates prior sweep segments (non-negotiable
  5). The transaction itself is F16-B; stage A only makes the distinction
  unrepresentable-as-a-bool and tests it.
- **Time is integer ticks with a fixed rate, accumulated in nanoseconds.**
  `SimClock::advance(Duration)` adds nanoseconds as integers
  (`carry + ns·hz`, divide by 10⁹), so 30, 60 and 144 fps runs over the same
  wall time advance **exactly** the same tick count — no float accumulator
  can drift across frame boundaries. Pause is a per-subsystem policy
  (`ClockPolicy::pause`): a `Freeze` clock returns zero ticks and does not
  bank paused time, a `KeepRunning` clock (UI wall, unscaled media) keeps
  advancing. Speed-up is the other explicit axis: only
  `SpeedUpPolicy::AdvanceFixedTicks` may inject ticks, so the multiplayer
  policy refuses local speed-up with `TimeError::NoSpeedUpAuthority`
  (non-negotiable 4).
- **The four time domains are distinct types, not one clock's modes.**
  `TimeDomain::{Simulation, UiWall, MediaUnscaled, AuthoritativeGameplay}`
  each get an explicit `ClockPolicy`; the acceptance test asserts they are
  pairwise distinct and that pause/authority follow the sheet's rules.

## Test inventory (`accept_f16_a_`)

20 tests, all selecting production code: 13 unit tests inside the four
owner modules and 7 in the integration test file.

| Test | Covers |
| --- | --- |
| `cs_types::space::tests::accept_f16_a_space_values_reject_nonfinite_and_nonunit_inputs` | boundary rejection: NaN/∞ positions, directions, rotations; non-unit normal/quaternion refused with its measured length instead of being renormalized |
| `cs_types::space::tests::accept_f16_a_canonical_rotation_is_right_handed_y_up_forward_negative_z` | the canonical convention itself: +90° about +Y maps +X → −Z, +90° about +X maps +Y → +Z, `Winding::CANONICAL_FRONT` is CCW, `flipped` is an involution |
| `cs_content::coordinates::tests::accept_f16_a_invalid_source_declarations_are_refused` | permutation, zero/negative/NaN/∞ scale, winding-reference validation, empty label |
| `cs_app::origin::tests::accept_f16_a_local_world_round_trip_within_declared_tolerance` | world → f32 local → world inside the *declared* (value-dependent) tolerance; the origin maps to exactly zero |
| `cs_app::origin::tests::accept_f16_a_rebase_opens_a_new_epoch_and_keeps_world_positions` | world identity survives an origin change; the epoch advances exactly once; a pre-rebase local coordinate no longer describes the point |
| `cs_app::origin::tests::accept_f16_a_rebase_and_teleport_are_distinct_and_declare_swept_continuity` | non-negotiable 5 at the type level |
| `cs_app::origin::tests::accept_f16_a_origin_boundary_overflow_is_refused` | f32/f64 overflow and epoch wrap are refused, never produced |
| `cs_sim::time::tests::accept_f16_a_equal_wall_time_at_30_60_144_fps_advances_identical_ticks` | AC03's clock half: identical ticks for identical wall time at three frame rates (exactly 120 ticks for 2 s at 60 Hz) |
| `cs_sim::time::tests::accept_f16_a_pause_freezes_simulation_and_objective_ticks` | AC04's clock half: `Freeze` clocks (simulation, multiplayer simulation, authoritative gameplay) return zero ticks while paused; UI wall time keeps running |
| `cs_sim::time::tests::accept_f16_a_paused_wall_time_is_never_banked` | pause then resume must not dump a burst of never-simulated ticks |
| `cs_sim::time::tests::accept_f16_a_multiplayer_policy_refuses_local_speed_up` | non-negotiable 4: `NoLocalAuthority` rejects injected ticks with a named error and mutates nothing; a paused freezing clock refuses them too |
| `cs_sim::time::tests::accept_f16_a_clock_policies_are_explicit_per_time_domain` | non-negotiable 3: four distinct domains, explicit pause and speed-up per subsystem, single-player speed-up not leaking into multiplayer |
| `cs_sim::time::tests::accept_f16_a_zero_tick_rate_and_tick_overflow_are_refused` | failure cases of the fixed-rate clock, including a refused advance leaving the tick counter unchanged |
| `crates/cs_content/tests/accept_f16_a_source_adapters.rs::accept_f16_a_round_trip_position_normal_quaternion_through_every_adapter` | **AC01 minimum scenario** over every declared adapter, with the declared tolerances asserted usable (finite, positive, < 1) |
| `…accept_f16_a_round_trip_distance_angle_and_winding_through_every_adapter` | the rest of the deliverable's quantity list |
| `…accept_f16_a_forward_mapping_matches_the_declared_convention` | absolute hand-computed mappings (positions, degrees→radians, cm→m, both rotation senses, orientation, reversal, winding labels), so a wrong-but-consistent pair cannot pass |
| `…accept_f16_a_winding_labels_and_vertex_order_agree_with_the_geometry` | geometric cross-product check of `winding_to_canonical` + `reverses_vertex_order` for both windings of a real triangle per adapter, plus "front faces land on canonical front" |
| `…accept_f16_a_winding_rule_holds_for_every_valid_convention` | **added in review**: the same three checks over every convention `SourceConvention::new` accepts (6 permutations × 8 sign patterns × 2 front-face rules = 96 declarations, 192 triangles), so the winding/orientation formulas are no longer pinned only by the three declarations the registry happens to hold |
| `…accept_f16_a_nonfinite_and_nonunit_inputs_are_refused_at_the_adapter_boundary` | adapter-boundary failures with their exact field names and measured lengths |
| `…accept_f16_a_declared_sources_are_registered_provenanced_and_never_claim_original` | registry completeness, unique labels, `Designed`/`SyntheticFixture` origins, **no `Origin::Installation` claim**, and declaration validation through the public constructors |

## Mutation probes (implementation neutered → tests fail; all reverted and
byte-compared)

| # | Edit | Result |
| --- | --- | --- |
| 1 | `rotation_to_canonical` drops the orientation sign | `cs_content` selection FAILED (exit 101): `…forward_mapping…`, `…winding_labels…` |
| 2 | `winding_to_canonical` returns its input unchanged | FAILED (101): `…forward_mapping…`, `…winding_labels…` |
| 3 | `reverses_vertex_order` always `false` | FAILED (101): `…forward_mapping…`, `…winding_labels…` |
| 4 | `SimClock::advance` ignores the pause policy | FAILED (101): `…pause_freezes…`, `…paused_wall_time…` |
| 5 | `advance_fixed_ticks` skips the authority check | FAILED (101): `…multiplayer_policy_refuses_local_speed_up` |
| 6 | `local_round_trip_tolerance_m` returns `0.0` | FAILED (101): `…local_world_round_trip_within_declared_tolerance` |
| 7 | position conversion uses `unit_scale` instead of `meters_per_unit` | FAILED (101): `…forward_mapping…`, `…round_trip_position…` |
| 8 | `check_finite` removed from `position_to_canonical` | FAILED (101): `…nonfinite_and_nonunit_inputs…` (the diagnostic must name `position[0]`, not `world.x`) |

An **organic** failure also proved test sensitivity during development: the
first implementation of `Quaternion::rotate` mis-weighted the quadratic term
(`2w·q_v×(q_v×v)` instead of `2·q_v×(q_v×v)`), and
`accept_f16_a_canonical_rotation_is_right_handed_y_up_forward_negative_z`
failed with `NotUnit { length: 1.042… }` before any probe was run. The
formula was corrected, not the test.

After restoring, `grep -rn "MUTATION PROBE" crates/` prints nothing and
`cargo fmt --all -- --check` is clean.

## Reviewer correction (2026-09-29, F16-A review)

The review added `accept_f16_a_winding_rule_holds_for_every_valid_convention`,
which enumerates every convention `SourceConvention::new` accepts instead of
only the three the registry happens to declare (6 permutations × 8 sign
patterns × 2 front-face rules). It found that
`SourceConvention::is_orientation_preserving` returned the wrong answer for
**18 of the 48 axis maps**.

- **Rule, as stated in `coordinates.rs`'s module documentation:** `det(M)` is
  the permutation's parity times the *product* of its signs.
- **Rule, as implemented:** `parity_even == (all three signs positive)`. Those
  agree when 0, 1 or 3 rows are negated; with **exactly two** negated rows the
  product is `+1` while "all positive" is `false`, so the answers disagree.
  Checked mechanically over all 48 maps: 18 mismatches, all of them the
  two-negated-row case.
- **Effect on production behaviour:** for such a declaration
  `SourceAdapter::orientation` took the wrong sign, so
  `rotation_to_canonical` / `rotation_from_canonical` flipped the quaternion's
  vector part, and `reverses_vertex_order` reversed (or failed to reverse)
  copied geometry. `winding_to_canonical` is unaffected: it depends only on the
  declared front-face label.
- **Why the original 19 tests stayed green:** none of the three declared
  sources has two negated rows (they negate 0, 1 and 1 rows), so the defect was
  invisible until the declaration space was enumerated. `SourceConvention::new`
  is public and accepts such maps, so a measured format at F16-D would have hit
  it.
- **Fix:** count the negated rows and use `negated_rows.is_multiple_of(2)` as
  the sign product, which is what the module documentation already said.
  Reverting only that change makes the new test fail (exit 101) while the other
  19 stay green — the coverage was genuinely missing, not a sign that the
  earlier assertions were wrong.

Eight further reviewer mutation probes (rotation orientation sign, the
`winding_to_canonical` flip, `reverses_vertex_order`, the pause policy in
`advance`, the speed-up authority check, the epoch increment, the declared
local round-trip tolerance, and unit-length rejection) each failed the
`accept_f16_a_` selection as required; the working tree was byte-identical to
the committed state after every probe.

## Recorded unknowns (recorded, not guessed)

- **Original handedness, axis order, scale and angle units are unmeasured.**
  No declared source is attributed to an original file; rule 1's three
  independent landmarks are F16-D's measurement, and this stage's fixtures
  are designed declarations only. `docs/research/FINDINGS.md` still lists
  "Coordinate handedness, scale and angle units" as an open question for
  F16/F26, and `docs/findings/2026-09-28-f10-a-lossless-mesh-ir-and-strip-fixtures.md`
  still records "Front-face winding and handedness: unknown for CS content".
  Nothing in this stage answers either.
- **Origin-shift transactions are not implemented here.** `OriginChange`
  names the distinction and `WorldOrigin` provides the typed conversion;
  the rebase-during-flight transaction, the atomic multi-subsystem rebasing
  and the sweep-segment handling are F16-B, and the wiring into every
  spatial subsystem is F16-C.
- **Which per-format adapters exist is F16-D's.** The registry currently
  holds designed fixtures; a measured format joins it with its own
  `Origin`/`Provenance` and evidence.
- **Pause/acceleration policies are designed defaults.** The sheet requires
  them to be explicit per subsystem, not that these particular pairings are
  original behaviour; F16-D's behavioral probes and the original mission
  work can revise them through an approved design update.

None of these is a new task: F16-B, F16-C and F16-D already cover them, so
`create_tasks` was not used.

## Commands run

All commands from the repository root on branch
`rally/65-define-units-typed-time-and-coordinate-a`, Rust 1.98.1, rebased on
`origin/main` (`2213421`) at handover; the work started from `96e9dc2`.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f16_a_ --include-ignored` | 0 (**19 tests**, all passing) |
| `python3 /tmp/mutate_f16a.py` (mutation probes 1–5) | 0: every probe's run exited 101 and every file was restored byte-for-byte |
| `python3 /tmp/mutate_f16a2.py` (mutation probes 6–8) | 0: same |

No command needed `CS_GAME_DIR`; `CS_CAPABILITIES` (`retail,gpu,audio`) was
not exercised by this stage. All four checks were re-run after the final
rebase onto `origin/main` (`2213421`) with the same exit codes, and the
19-test selection was re-counted then.

The **review** re-ran all four commands on the reviewed tree after the
correction above: `fmt` 0, `clippy -D warnings` 0, `cargo test --workspace
--locked` 0, and the prefix selection 0 with **20 tests** (the original 19 plus
`accept_f16_a_winding_rule_holds_for_every_valid_convention`).

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_types/src/lib.rs`: `pub mod space;`.
- `crates/cs_content/src/lib.rs`: `pub mod coordinates;` plus a doc
  paragraph.
- `crates/cs_app/src/lib.rs`: `pub mod origin;` plus a doc paragraph.
- `crates/cs_sim/src/lib.rs`: `pub mod time;` plus a doc paragraph (the
  crate doc's "no implementation yet" line now names the time module).

No `Cargo.toml` change was needed: `cs_types` stays dependency-free, and
the three consumer crates already depend on it. No protected path, original
datum or binary file is involved.

