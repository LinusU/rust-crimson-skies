# Flight and physics implementation contract

The formulas below are a **designed starting model**. They are not asserted to be Crimson Skies source equations. F26 calibrates and may revise them through an approved design update. Their purpose is to prevent agents from assembling incompatible force laws or integrating a body twice.

## Inputs and outputs

`FlightInput { throttle:[0,1], pitch:[-1,1], roll:[-1,1], yaw:[-1,1], boost:bool }`

`FlightState { world_pose, linear_velocity_mps, angular_velocity_radps, engine_state, damage_state, control_mode }`

`FlightTuning { model_kind, mass_properties, engine_curve, drag_parameters, lift_curve, angular_response, stall_behavior, assist_profile }`

`FlightOutput { world_force_N, world_torque_Nm, instrument_state, accepted_boost_consumption, diagnostics }`

Reject nonfinite inputs at boundaries. Clamp human controls to their declared ranges; do not silently clamp corrupted tuning into plausible values. Commands belong to one simulation tick and actor generation.

## Coordinate convention

Body forward is -Z, right +X and up +Y. Let `v_air = v_world - wind_world`; transform to body space. Use a tested sign convention for angle of attack and sideslip. Define a small, named numerical epsilon only to avoid singularities, not to impose minimum flying speed.

A suitable initial force decomposition is thrust along body forward, drag opposite air-relative velocity and lift perpendicular to airflow in the body-up plane. With `q = 0.5 * rho * speed^2`, `L = q*S*CL(alpha)` and `D = q*S*CD(alpha)`, all coefficients/area/density are explicit tuning. Low-speed lift smoothly approaches zero. A simple finite-stall curve is preferable to discontinuous `if speed < stall then fall` behavior. If original tuning does not map to physical coefficients, fit a clearly documented empirical model instead of pretending extracted values are SI coefficients.

Do not add both a hand-written gravity force and Avian global gravity. Do not apply drag twice through both a custom law and an undocumented rigid-body damping setting. Tuning tests expose every contribution.

## Attitude control

One consistent initial option is rate command plus bounded feedback torque: desired rates derive from control inputs and speed/damage authority; torque corrects angular rate error with specified gains and damping. Feed-forward inertia compensation is permitted only if mass properties and torque units are consistent. A PID integrator, if used, needs anti-windup and reset on ownership transitions. Never rotate Transform directly to meet the desired attitude while a dynamic rigid body also integrates torque.

A bank/level assist is optional, declared and logged. It must not secretly recover every stall or make gravity disappear. Flight tuning must permit the authored obstacle routes and combat pacing; realism is subordinate to verified game behavior.

## Boost and special models

Boost is equipment/state with capacity, accepted activation, thrust modification, consumption and recovery determined by data. Pressing a button while boost is unavailable does not consume capacity. A visual exhaust trail cannot be its authority.

Exceptional airframes implement the same input/output boundary but can use a different control law. Do not use the word autogyro as permission to invent helicopter hover. Record a separate reference maneuver envelope.

## Collision and ballistic tests

Synthetic tests define exact mass, forces and geometry. For high-speed tests choose speed*dt larger than the obstacle thickness so a discrete endpoint-only implementation provably fails. Check relative movement: a target can cross the projectile path between ticks. A closest-hit policy must choose earliest time-of-impact and stable tie-breakers. Apply damage once even if several collision features report the same hit.

For interaction triggers use a swept center/shape appropriate to the original rule. Store movement continuity. A teleport clears its segment; an origin shift translates both endpoints consistently. Contact normal direction, reflection and local damage-zone mapping require unit tests.

## Calibration acceptance

Record at least acceleration, coast-down, turn, roll, pitch loop, stall/recovery, engine-out and damaged control for each relevant airframe. Use several loadouts. Reference traces state initial conditions, input timing, camera/instrument measurement error and difficulty. Select tolerances before fitting the final run; hold out at least one maneuver. A synthetic physics pass cannot promote faithful handling.
