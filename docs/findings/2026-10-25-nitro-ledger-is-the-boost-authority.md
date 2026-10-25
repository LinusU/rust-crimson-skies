# Nitro ledger and flight boost: one authority (task #453)

**Decision.** The declared `cs_sim::weapons::ordnance::NitroParameters` is the
only authority for an airframe's boost terms. `flight::BoostParameters` is not
declared a second time; it is lowered from the nitro record by
`NitroParameters::boost_parameters()` (`thrust_n` = `extra_thrust_n`,
`consumption_per_s` = `consumption_per_s`, same capacity units).

**Where it is stated.** `NitroParameters::boost_parameters` in
`crates/cs_sim/src/weapons/ordnance.rs` is the single source; a caller that
flies a nitro-equipped airframe sets `AirframeTuning::boost` from it and feeds
the flight model `FlightInput::boost = NitroTick::active` and
`FlightState::boost_available = NitroLedger::boost_available()`.

**What stays separate.** `NitroLedger::request` is still the only place
capacity moves and the only judge of accepted activation, recovery and the
refusal reasons. The flight model keeps computing thrust and reporting
`accepted_boost_consumption` for the tick it is handed.

**AC04 property.** Neither `BoostParameters` nor `NitroTick` has a pose,
velocity or `Duration`; the ledger converts through the declared `TickRate`.
Pinned by `accept_f28_f24_nitro_boost_*` in `crates/cs_sim/tests`:
an accepted activation raises thrust by exactly `NitroTick::extra_thrust_n`
and consumption by exactly `consumed_units`, leaving lift, drag and gravity
unchanged; a refusal consumes no capacity and adds no thrust.

**Open.** Airframe tuning records (`cs_content`) can still declare
`boost.*` fields themselves (`airframe_tuning_from_declared` in
`cs_app::physics::flight`). Wiring the app boundary so an airframe with a
declared nitro booster takes its boost from the nitro record, and refusing a
disagreeing tuning boost, is not done here (no original airframe-to-booster
mapping is known); it belongs to the F28-C/F24-C runtime wiring.
