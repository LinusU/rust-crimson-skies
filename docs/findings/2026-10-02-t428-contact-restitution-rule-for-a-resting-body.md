# #428: the contact/restitution rule for a body resting against geometry an overlay moves

Date: 2026-10-02. Task: "Establish the contact/restitution rule for a body
resting against geometry an overlay moves" — a follow-up filed by F18-C
(task #87), recorded in
`docs/findings/2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md`.

Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`
(F23's owner paths; this task changes no spec). Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md`. Capabilities used: ordinary build/test
only — no `CS_GAME_DIR` read, no evidence report required, nothing
`verified_original`.

## The question, and the answer

F18-C measured a body pressed against a door panel creeping at 0.0124 m/tick,
identically in all three of its arms — panel displaced, panel despawned, panel
untouched — and filed this task asking whether that is intended contact
behaviour, a restitution default, or a defect.

**It is a defect, and it is in the contact rather than in the overlay. What
F18-C recorded is not a body held in place by the panel it opened; it is a body
that never came to rest in the first place, drifting at the rate its contact
left it.**

The residual a contact leaves behind is **constant, non-zero, and not
restitution**. In a world with zero gravity and no drag, nothing retires it, so
it carries the body for as long as the world runs. That is the whole defect, and
`cs_app::physics::resting` is the rule that closes it.

F18-C's attribution needs one correction, and it is a small one: its finding
says the residual is "ordinary contact behaviour". It is not ordinary — it is
the specific, measurable, unretired output of Avian's solver on this pinned
pair. Its *attribution* was right (the contact is the cause, not the overlay,
not the collider's movement); its *characterisation* was wrong, and it is
corrected below.

## The measurement, on the pinned pair

`bevy 0.19.1`, `avian3d 0.7.0`, `SubstepCount(2)` (F23-D's declared count),
120 Hz, `Gravity::ZERO`, no drag. A 250 kg half-metre box, a 0.4 m wall.

### What a contact leaves

| impact speed | velocity after the contact resolves | as a fraction of impact |
| --- | --- | --- |
| 5 m/s | 0.156 m/s | 3.1 % |
| 15 m/s | 0.610 m/s | 4.1 % |
| 30 m/s | 0.981 m/s | 3.3 % |
| 60 m/s | 1.759 m/s | 2.9 % |
| 100 m/s | 2.830 m/s | 2.8 % |

Three properties of that residual are what the rule is built on, and each was
measured rather than assumed.

**1. It is not restitution.** Avian's `Restitution::default()` is coefficient
`0.0` (`avian3d-0.7.0/src/dynamics/rigid_body/physics_material.rs:320`), and
binding `Restitution::new(0.0)` explicitly on the wall reproduced the numbers
above **bit for bit**. Zero restitution is also the *correct* rule for this
game: a body must not rebound off a door panel. Binding
`Friction::new(0.0)` made the residual *larger* (1.037 m/s against 0.981), so
friction is damping part of the leak rather than causing it.

**2. It is not the substep budget.** 1, 2, 4 and 8 solver substeps measured
0.933, 0.981, 1.219 and 0.820 m/s at the same 30 m/s impact — noise around one
value, not a trend.

**3. It never decays.** The contact resolves in a *single* tick: measured
`|Δv| = 30.39 m/s` on the resolving tick, then `|Δv| = 0` — once `6e-8`, float
noise — on every tick after it, for 960 ticks. The velocity that survives is
*constant to float precision* from then on.

Property 3 is the rule. A velocity that does not change while the body is in a
world with zero gravity and no drag is not being produced by anything, so what
is left is the solver's own approximation error. Measured over 240 ticks, that
carries the body 0.25 m — and there is nothing to stop it.

It is also why the body never sleeps: Avian's `SleepThreshold` is 0.15 m/s
(`SleepThreshold { linear: 0.15, angular: 0.15 }`), and the measured residual
is six times that at a 30 m/s impact and nineteen times at 100 m/s. The engine
would settle the body if the residual were smaller; it is not.

### The three arms, and why they agreed

F18-C's arms are re-measured and confirmed, and the reason they agree is now
stated: the leak happens *before* any arm runs. Measured — the wall displaced
4 m away, the wall despawned, and the wall untouched, after the body has been
pressed into it and the contact resolved: **all three drift identically, to
within 1e-6 m**, and none of them resumes the arrival velocity. So there was
never a moment at which the overlay could have released the body, because the
body was not being held by the panel — it was already drifting, and the panel's
whereabouts are irrelevant to a drift nothing is producing.

### Where a resting body ends up

Two consequences of the leak's size, both measured, both of which the rule has
to answer:

* The leak carries the body about **1.3 cm away** from the surface it struck
  (at 30 m/s), which is further than Avian's own contact tolerance. So the pair
  stops being a contact at all and the body comes to rest in open space. This is
  why the resting mark cannot mean "this body is touching geometry": the
  measured answer is that a body at rest after a collision usually is not.
* The leak is *proportional to impact speed*, so a fast body's resting pose is
  further from its wall than a slow body's. At 100 m/s the leak is 2.83 m/s and
  moves the body ~2.4 cm before the rule catches it. The tick count that catches
  it therefore matters (below).

## The rule

`cs_app::physics::resting`, installed by
`RestingBodiesPlugin` in both compositions that simulate bodies
(`PhysicsBodiesPlugin`, and `cs_app::world::world_app`). It runs in
`FixedPostUpdate` after `PhysicsSystems::StepSimulation` — the same slot the
contact reporter reads, so it sees this tick's contacts and this tick's
post-step velocity.

For every **dynamic** body in a **touching** contact with **immovable** world
geometry: a per-tick change in linear velocity at or below
`RESTING_STILL_EPSILON_M_S` extends an unchanged run; any larger change resets
it. `RESTING_STILL_TICKS` consecutive unchanged ticks retire the body's linear
and angular velocity and mark it `RestingContact`. A marked body is held at zero
on every tick it is still touching, and leaves the mark when it is touching
nothing or when something is measurably driving it
(`RESTING_RELEASE_TICKS` consecutive ticks of *changing* velocity).

It writes **velocity only**. The pose is never touched: a crashed body stops
where the contact left it, which is the whole point of the rule and the reason
it is not a teleport.

Nothing in the rule is conditional on an overlay. It cannot see one. A body at
rest before a door opens is at rest after it — measured, and that is the fact
F18-C's stage needed pinned.

### The two declared constants, and where their numbers come from

**`RESTING_STILL_EPSILON_M_S = 1e-3`.** The residual is *exactly* constant: over
960 ticks the measured per-tick change was `0.0`, once `6e-8`, never more. One
millimetre per second per tick is five orders of magnitude above that noise and,
at 120 Hz, an acceleration of 0.12 m/s² — below any force the game applies (even
a single millinewton on a 250 kg body is 4e-6 m/s² per tick). It separates
"nothing is acting on this body" from "something is" with a wide margin on both
sides.

**`RESTING_STILL_TICKS = 4`.** The count is not about waiting for the residual
to settle — it is constant from the tick it appears. It is about *where* the
body is left, and the measurement moves with it:

| ticks | 30 m/s impact | 100 m/s impact |
| --- | --- | --- |
| 1 | 3.4 mm clear | 10.4 mm clear |
| 2 | 6.6 mm clear | 25.5 mm clear |
| **4** | **12.8 mm clear** | **55.7 mm clear** |
| 6 | 19.1 mm clear | **1.76 m past the wall** |
| 8 | 25.4 mm clear | **1.76 m past the wall** |

The **upper bound is forbidden by the measurement**. At 100 m/s a body covers
0.83 m per tick against the wall's 0.4 m thickness, so by six ticks it is still
moving when the count completes and the rule retires it 1.76 m *through* the
wall it struck. Six and above are wrong, not merely eager, and a body "at rest"
inside geometry is the same render/collision mismatch this class of work exists
to prevent.

The **lower bound is a judgement, and is recorded as one rather than dressed up
as a measurement**: one, two and four all leave the body beside the wall at both
speeds, differing only in clearance, which is Avian's contact tolerance behaving
as designed rather than a property of this count. Four is chosen because the
contact's transient is a single tick and four leaves three ticks of margin
against it at 120 Hz — 1/30 s of simulated time, short enough that "the body
stopped" is not perceptible, long enough that no single solver impulse is
mistaken for a settled state. A reviewer preferring one or two has this table to
check against, and the pinned test's fast arm reports a wrong-long count
immediately.

**`RESTING_RELEASE_TICKS = 24`.** Added by review; see "The defect review found"
below. It is the dwell after which a marked body stops being held because
something is measurably driving it.

## The defect review found: the hold could freeze a body the game is driving

**Found in review, not in the first pass, and it was in the rule this task
ships rather than in the measurement it rests on.**

The rule as first written held a marked body at zero for as long as it kept
touching geometry, with no way out while the contact lasted. The module
documentation described a release path (`RESTING_RELEASE_TICKS`) that the code
did not contain, and the one test that claimed to check "the rule is not a
brake" ran on a body that was **not marked and not touching** — a 30 m/s striker
comes to rest 1.3 cm clear of the wall, so it was never held by the rule at all.
The assertion passed and proved nothing.

### Measured, with the unconditional hold

| case (a body marked at rest against a wall) | measured |
| --- | --- |
| 3 kN into the wall | speed stays exactly `0.0` for 60 ticks, pose moves `0.12 mm` — correct: the wall holds it |
| 3 kN along the face | released on tick 4, because the body leaves the contact — not the rule |
| 0.5 m/s velocity write every tick | pose advances at 0.5 m/s while `LinearVelocity` reads `0.0` for all 60 ticks |
| **3 kN into the wall and 2 kN along it (wedged)** | **frozen: `0.010 m` in 60 ticks, speed `0.0` throughout, mark never withdrawn** |

The last row is the defect. A body pressed into geometry cannot lose the
contact, so the unconditional hold has no exit at all: the game pushes, the rule
rewrites the velocity every tick, and the body never moves.

### The release signal is velocity *change*, and that is forced, not preferred

The obvious test — "is the body's speed above the epsilon?" — **cannot work**,
and the failure is measured. A resting body in contact is handed a non-zero
speed every tick by the solver's own soft-constraint bias: `0.0638 m/s` on the
depot's door panel, constant in the plain wall fixture. A speed test releases
every resting body on its next tick. Measured directly: with a speed-based
release the depot probe cycled mark → drift → re-mark and the `retired`
counter reached three for one body.

Velocity **change** separates the two, because the bias does not stay constant
— it *decays*. On the depot panel it falls `0.0638 → 0.0036 m/s` over **19
consecutive ticks** as the soft constraint bleeds the initial overlap off, and
only then stops changing at all (the plain wall fixture is 1 tick, because its
overlap is shallow). Anything actually driving the body changes its velocity
every tick by `a·dt`: 3 kN on 250 kg at 120 Hz is `0.1 m/s` per tick, a hundred
times the epsilon.

**So the dwell has to outlast the decay, and 24 is 19 plus five ticks of margin.**
The margin exists because the decay's length is a property of how deeply the pair
overlaps and this repository has not measured every geometry that could overlap
more deeply. The cost of the margin is named rather than hidden: a marked body
that gameplay starts driving is released after 24 ticks, **1/5 s** of simulated
time.

Measured after the fix: the wedged body is released at tick 24 and slides
`0.104 m` over the same 60 ticks, ten times the frozen pose change.

### Two corrections to this finding, found in the same review

* **"The resting mark was withdrawn exactly once, on the tick the panel left"**,
  below, is wrong: it is never withdrawn for the depot pair. Measured — after the
  overlay applies, the probe keeps its mark, `released` stays `0`, and the world
  contact log still reports the probe against `depot.door`. The panel keeps a
  speculative contact with the body after it has slid 2 m along `z`, so the mark
  is still resting *on the record*. The pinned test already asserted this
  correctly; only this prose was wrong.
* **The "3 cm" figure** quoted by the "the rule is not a brake" test was measured
  on a body the rule was not holding (1.3 cm, in fact). The test now asserts its
  premise and drives a body that is genuinely held.

## The F18-C pair: what the door opening actually does

Measured on the depot world with the world composition, the production probe, and
the door overlay declared. The probe starts *between* the trigger volume
(`x ∈ [-4.5, -3.5]`) and the panel (`x ≥ -0.5`), so nothing fires the overlay on
the way in; it strikes the closed panel and comes to rest at
`x = -0.6483`. The overlay is then requested through the mission's own branch
producer (`request_overlay`), the same hand-off the contact stream uses.

After the door opens and 240 ticks pass:

* the overlay log reports one `Applied`;
* the panel's **collided** half moved by the authored offset `[0, 0, 2]`;
* the body **did not move at all** — its pose is bit-identical to the pose it had
  before the overlay, and its velocity stays below `3.2e-6 m/s` throughout;
* the resting mark is **not** withdrawn. This corrects an earlier draft of this
  section, which claimed it was "withdrawn exactly once, on the tick the panel
  left": measured, `released` stays `0` and the mark survives, because the panel
  keeps a speculative contact with the body after sliding 2 m along `z` and the
  world contact log still reports the pair. The body is held by nothing and
  moves by nothing either way; the mark records that its residual was retired
  and nothing has given it a velocity since, which is what its own doc says it
  means.

**This is the answer to "the door opened and the body on it moved."** The two
facts are now separately recorded and they agree: the door opened (the overlay
log says so, and the collided half proves it), and the body stayed where it was,
because it had already come to rest and was never held by the panel.

## Files

* `crates/cs_app/src/physics/resting.rs` (new): the rule, the two declared
  constants, the `RestingContact` mark, the `RestingReports` counters, and
  `RestingBodiesPlugin`.
* `crates/cs_app/src/physics/contacts.rs` (edited): `PhysicsBodiesPlugin`
  installs `RestingBodiesPlugin` — three lines, no logic.
* `crates/cs_app/src/physics/mod.rs` (edited): module declaration, re-exports,
  module docs.
* `crates/cs_app/src/world/fixture.rs` (edited): `world_app` installs
  `RestingBodiesPlugin`, with the reason it does so in a comment — eight lines,
  no logic.
* `crates/cs_app/tests/physics/resting.rs` (new) and
  `crates/cs_app/tests/physics/main.rs` (edited): the nine `accept_t428_`
  acceptance tests and their module declaration.
* This file.

**One observable failure:** a body that struck a wall still carries a non-zero
velocity after the contact, and in a world with no gravity and no drag it goes
on carrying it forever — so a door that opens onto it changes nothing, and "the
door opened" and "the body moved" cannot be told apart from outside. That is
`accept_t428_the_residual_never_decays_so_a_body_in_a_world_with_no_drag_drifts_forever`,
and it is the failure this task exists to make impossible.

## Test sensitivity (mutation matrix)

Nine mutations were applied, `cargo test -p cs_app --test physics --
accept_t428_ --include-ignored` was run, and the source was restored each time.
**Seven of the nine are caught.** The two that survive are recorded below rather
than papered over.

| mutation | tests that failed |
| --- | --- |
| the retire path never zeroes the velocities (M1) | `..._comes_to_rest_and_holds_its_pose`, `..._holds_only_what_geometry_holds...` |
| a marked body is not held at zero (M2) | `..._a_door_opening_does_not_move_a_body_that_already_came_to_rest` |
| the sensor filter is deleted (M3) | `..._a_body_crossing_a_trigger_volume_is_never_retired...` |
| a **kinematic** body is treated as dynamic (M5) | `..._covers_only_live_dynamic_bodies_and_never_disturbs_a_sleeping_one` |
| the pruning pass and its counter decrement are deleted (M6) | `..._covers_only_live_dynamic_bodies_and_never_disturbs_a_sleeping_one` |
| **`ContactPairFlags::TOUCHING` is ignored (M4)** | **none — see below** |
| **`RESTING_STILL_TICKS` is raised to 6 or 8 (M7)** | `..._comes_to_rest_and_holds_its_pose` (6 and 8 caught; 1 and 2 are not) |
| **the release-on-drive branch is removed (M10)** | `..._a_marked_body_is_released_when_gameplay_drives_it...` — added by review |
| **`RESTING_RELEASE_TICKS` is lowered to 4 (M11)** | `..._a_door_opening_does_not_move_a_body_that_already_came_to_rest` — added by review; 4 is inside the measured 19-tick decay, so the depot probe is released while it is resting |

### The one that survives

**M4 — the `TOUCHING` filter is masked by the geometry it is checked against.**
Deleting the check leaves every test green, because the pairs it would have
excluded are all sensor pairs, which the *next* clause excludes anyway, or
pairs that never exist for long enough to matter.

The measured reason is narrow and is recorded in the source: a pair exists as
soon as two colliders' AABBs overlap, which is *before* the shapes touch. On the
pinned pair a 250 kg body flying at 30 m/s into a wall is already in a
**non-touching** pair at tick 9, at `x = -0.500`; the pair starts touching on
tick 10. So the filter does real work for exactly the tick or two between "the
broad phase thinks these might touch" and "they do", and a body arriving at
speed passes through that window without ever being misjudged — because it is
still moving, its velocity is changing every tick, and the unchanged run never
completes. The filter's error case needs a body that is *slow and approaching*,
and no fixture in this stage is both.

The filter is kept because it is the record's own claim about what "resting
against geometry" means, and because the window is real even if these tests do
not reach it. A reviewer should read it as documented, not as covered.

### What M7 does and does not pin

The mutation matrix for `RESTING_STILL_TICKS` is asymmetric, and the test was
built to match rather than to flatter:

* **6 and 8 fail** `..._comes_to_rest_and_holds_its_pose`, on the 100 m/s arm:
  a body retired 1.76 m *through* the wall is not resting beside it, and the test
  asserts the resting pose is beside the wall rather than merely at rest.
* **1 and 2 pass.** Nothing in the measured behaviour separates them from four —
  see the table above — and the constant's own doc comment says so and calls the
  lower bound a judgement. A reviewer who wants the count pinned to one tick
  would be asking for a measurement that does not exist.

`RESTING_RELEASE_TICKS` is the asymmetric case that shows the measurement doing
real work: **4 fails and 24 passes**, and the reason is measured rather than
tuned. The depot pair's solver bias takes 19 ticks to stop changing, so a dwell
inside that window releases a body that is in fact resting and hands back the
drift the rule exists to remove. There is no value below 20 that the pinned door
test accepts, which is the constant's lower bound earned.

## Designed rule, not original data

Every number above is measured from **this** engine, not read from the 2000 PC
original. Whether the original settled a body against geometry this way, let it
sleep, applied a restitution of zero, or left it drifting is **unknown**. The
resting rule is newly authored project design, and nothing here is
`verified_original`.

## Known limitations that gate later stages (not silently dropped)

* **The residual is proportional to impact speed, and the resting pose inherits
  that.** Measured: 1.3 cm of clearance at 30 m/s, 5.6 cm at 100 m/s, measured
  from the wall's face. A fast body comes to rest visibly clear of what it hit.
  Affected content: any object that strikes world geometry above walking speed
  and is then inspected at rest. The rule bounds the drift, it does not correct
  the pose to the surface — that would be a different rule (a
  penetration-resolution pass) and is not written here.
* **A body at rest after a collision is usually *not* touching what it struck**
  (1.3 cm clear, against a 1 mm contact tolerance). So `RestingContact` means
  "at rest, and its contact residual was retired" — not "held by geometry". Any
  consumer that needs the second must ask the geometry, not the mark. The mark's
  doc comment says this.
* **A world with gravity or drag would need a different rule.** Measured
  nothing here: every measurement is `Gravity::ZERO` with no drag, which is what
  F23's flight world declares. Under gravity a body resting on a surface has a
  normal force holding it and the unchanged-velocity test would behave
  differently. Affected content: any future ground-contact staging (F34's ground
  vehicles, F29's wreckage). The rule is written for the world this game
  actually declares, and says so.
* **The rule only sees immovable geometry and dynamic bodies.** A kinematic
  actor is left alone by design (its velocity is gameplay's), and two dynamic
  bodies leaning on each other are not a resting case this rule models.
  Affected content: debris piles, multi-body stacks. F29/F34.
* **A body held at rest is not sleeping**, because the mark is a velocity write
  and a velocity write wakes a body. Measured: a body resting against a wall
  does fall asleep on its own at tick 60, and the rule neither wakes it nor
  drops its claim — the sleeping body is skipped rather than written to. But a
  body the rule keeps re-zeroing because its contact keeps nudging it never
  reaches the engine's own sleep threshold. Affected content: how long a parked
  aircraft stays fully simulated. F24.
* **The `TOUCHING` filter's failure case is not covered by a test** (M4 above).
  Affected content: a slow body approaching geometry it has not yet reached.
  Documented in the source and above.
* **A *constant* external velocity write on a marked, still-touching body is not
  released**, because the release signal is velocity *change* and a constant
  write produces none. Measured: a 0.5 m/s write every tick moves the body at
  0.5 m/s while `LinearVelocity` reads `0.0` for all 60 ticks. The write cannot
  be told apart from the solver's own constant bias, which is the same signal
  the rule must not mistake for gameplay — this is the deliberate cost of the
  choice above, not an oversight. Affected content: any later stage that drives a
  resting dynamic body by writing velocity rather than through the force queue
  (`ForceRequest`, which the rule does release — measured at the dwell). The
  declared drive path in this repository is the force queue; kinematic movers are
  excluded from the rule by design. A drive-side signal (a flag the drive path
  sets) would close it and is not written here.
* **The decay the release dwell must outlast is 19 ticks on the pair measured
  here.** A geometry whose overlap is deeper would decay for longer and would
  need a larger dwell; the constant carries five ticks of margin and no
  measurement of a deeper overlap. Affected content: any future world whose
  contact pairs overlap more deeply than the depot panel. The count is declared
  and measured, not derived.

## Evidence

Ordinary build/test only; no `CS_GAME_DIR` read and no evidence report is
required for this task. Commands run locally:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # 254 binaries, 0 failed
cargo test --workspace --locked -- accept_t428_ --include-ignored
#   9 tests run, 9 passed (crates/cs_app/tests/physics)
```

The nine acceptance tests are listed with their failure sensitivity in the
mutation matrix above.

## Sources

No external sources were consulted. Every Avian, Bevy and parry statement above
was read from the pinned sources in the local cargo registry and then *measured*
by running the fixture: the residual table, the three-arm agreement, the
substep sweep, the material sweep, the resting-pose table and the sleep tick are
all outputs of the runs above, not readings from the engine's source. The
attribution being corrected is
`docs/findings/2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md`.