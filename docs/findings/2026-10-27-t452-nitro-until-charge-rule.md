# T452: the measured charge-bounded nitro rule is now a `NitroActivationRule`

Task #2155. Source of the numbers: `2026-10-25-t452-03-nitro-activation-capacity-and-tradeoffs.md`
(static, engine image `0x487e91..0x487efc`, `0x4b2110`) and
`cs_content::ordnance::ORIGINAL_NITRO`. No `verified_original` is awarded.

## What changed

* `cs_sim::weapons::ordnance::NitroActivationRule::UntilCharge { start_min_fraction, stop_below_fraction }`,
  validated by `NitroParameters::try_new` (finite, `0 < start <= 1`, `0 <= stop < start`;
  `OrdnanceDefinitionError::InvalidNitroChargeFractions`).
* `NitroLedger::request`: a start needs the control and `charge >= start * capacity`; an
  accepted start latches a burn that releasing the control does not stop; a tick runs only if
  the charge at its start is `>= stop * capacity` (the original cuts on `charge < 5%`, so
  equality keeps running). A start below the threshold on a non-empty tank is refused with
  the new `NitroRefusal::BelowStartCharge` and consumes nothing; an empty tank still reports
  `CapacityExhausted`. Idle recovery and `NitroRefusedRecovery` are unchanged. A multi-tick
  jump consumes exactly what the same ticks walked consume, including across the cut.
* `cs_content::ordnance::DeclaredNitroActivationRule::UntilCharge` (both fractions
  `Resolved<f64>`, known values validated) and `cs_app::ordnance::lower_nitro_activation`,
  which refuses an unknown fraction by field name.

## Still measured-unknown

The extra thrust and authority tradeoff stay under `t452.limit.nitro_tradeoff`;
`NitroTradeoffs::UNMEASURED` is untouched.

## Not done here

The task text names `cs_app::ordnance::original_catalogue` / `OriginalNitroCatalogue`, the
`t452.limit.nitro_activation_rule` claim and
`docs/findings/2026-10-27-t452-original-ordnance-catalogue-closure.md`. None of them exists on
`main` at the time of this change, so there was nothing to re-point; whoever lands the
catalogue should declare the original nitro with `UntilCharge { 0.99, 0.05 }` and drop that
claim.

## Tests

`accept_t452_nitro_*` in `crates/cs_sim/tests/accept_t452_nitro.rs` and
`crates/cs_app/tests/accept_f28_d_ordnance_catalogue.rs`.
