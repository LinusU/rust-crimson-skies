# F28-AE3: nitro capacity recovery on a refused activation

Claim `f28.d.limit.nitro_recovery` stays **unmeasured**. No file in the
installation names the rule, so no value is chosen for the original.

What changed: `DeclaredNitroParameters::recovery_on_refused` is a `Resolved`
option (`recovers` / `withheld`) with no default. `lower_ordnance` refuses an
unresolved value by the name `nitro.recovery_on_refused`, and the audit walk
lists it as an unmeasured field. `cs_sim` carries the lowered choice as
`NitroRefusedRecovery`. `NitroParameters::try_new` keeps the previous idle-only
behaviour (`Recovers`); only the declared path states the choice.

Unchanged: a refused activation consumes nothing and adds no thrust, under
either choice (`accept_f28_ae3_*`).

Resolving evidence needed: an original run that holds the nitro control on an
empty tank and records capacity over time (owner-supplied capture).
