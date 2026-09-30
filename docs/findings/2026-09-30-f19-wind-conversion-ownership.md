# Where the authoritative wind conversion lives (task #434 `F19-WIND-CONVERSION-OWNER`)

Task: #434, opened from
`docs/findings/2026-09-30-f19-b-sky-fog-light-and-weather-effects.md`, section
"An architectural seam this stage ran into: the wind conversion lives above the
simulation". Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`,
non-negotiable behavior 2 and acceptance case AC02; shared contract
`docs/contracts/FLIGHT-PHYSICS.md`, "Coordinate convention"
(`v_air = v_world - wind_world`); crate ownership `docs/01-ARCHITECTURE.md`.

## The decision

The task asked the owner to pick one of three options. I picked **option 1,
moved down into `cs_sim`**, and the reasoning is recorded here because the
choice constrains F27-B.

* `cs_sim::environment` owns `air_relative_velocity_m_s`,
  `world_velocity_from_air_m_s` and `airspeed_m_s`.
* `FlightEnvironment::air_relative_velocity_m_s` is the accessor every consumer
  inside `cs_sim` is meant to call.
* `FlightModel::compute` and the exceptional `autogyro` law convert through it,
  so the code that *applies* a wind no longer has a private subtraction.
* `cs_app::environment::air` re-exports the two conversion functions and
  `AuthoritativeWind::air_relative` / `::world_velocity` / `::airspeed_m_s`
  delegate to them. `AuthoritativeWind` and `ProjectileMotion` stay in
  `cs_app`, and so does the unknown-wind refusal.

Rejected:

* **Option 2 (`cs_types`)** would put physics semantics in the crate that owns
  ids and immutable cross-boundary records, and it would have to be reachable
  from `cs_sim` *and* `cs_app` — the module already sits next to the consumers,
  so a lower crate buys reachability the code does not need.
* **Option 3 (explicit duplication)** was never attractive here: the duplication
  would have been one subtraction reachable from two crates, which a later
  weapon implementation could silently copy a third time. It remains the
  fallback only if a future constraint makes `cs_sim` the wrong owner.

## Why the record stayed above the simulation

`AuthoritativeWind::from_state` reads
`cs_content::environment::EnvironmentState::wind`, and `docs/01-ARCHITECTURE.md`
allows `cs_sim` to depend on `cs_types` and `cs_script` only. So the binding
from an authored environment state to a wind — and with it
`WindUnavailable::Unknown { claim_id, reason }`, the refusal of an unmeasured
wind — has to live above `cs_sim` for as long as that dependency rule stands.
`ClaimId` itself is a `cs_types` record, so the error type could have moved;
the constructor cannot.

This is a split of *ownership*, not a second implementation. The record
(`EnvironmentState::wind`) was already shared: one field every consumer reads.
What was not shared was the conversion, and that is what moved.

## What F27-B must do

`crates/cs_sim/src/weapons/` will need the same subtraction for swept
ballistics. It must call
`FlightEnvironment::air_relative_velocity_m_s(world_velocity_m_s)` or
`cs_sim::environment::air_relative_velocity_m_s(world, wind)`, and it must not
write `a[i] - b[i]` over a wind again. If a future consumer needs the wind as a
value rather than an argument, the type to introduce is one in
`cs_sim::environment` — not a second subtraction.

## Test sensitivity

`crates/cs_sim/tests/accept_f19_b_authoritative_wind_conversion.rs` and the
three unit tests in `cs_sim/src/environment.rs` compare the airspeed the real
`FlightModel` and the real exceptional law report against
`airspeed_m_s` of the same world velocity and wind, over still air, a headwind,
a tailwind, a crosswind and the timeline gust, plus the AC02 regression (a wind
change moves aircraft airspeed and a projectile's world velocity by the wind's
own difference). Reverting the conversion's sign in `air_relative_velocity_m_s`
fails two of the three integration tests and both conversion unit tests. The
existing F19-B acceptance tests in `crates/cs_app/tests/environment/air.rs` were
left untouched and still pass, which is the AC02 regression the task required
to keep passing.

## What is not claimed

Nothing here is `verified_original`, and nothing is new evidence: this task
moves code and corrects wording. The sign convention and the subtraction are
the designed `FLIGHT-PHYSICS` contract, not a recovered original rule. No
original wind value, unit, profile or tuning was read; a projectile's drag,
ballistics and wind-shear behaviour remain unmeasured and unmodelled, as
recorded in the F19-B findings. No crate dependency was added or inverted:
`cs_app -> cs_sim` is unchanged and `cs_sim` still depends only on `cs_types`
and `cs_script`.