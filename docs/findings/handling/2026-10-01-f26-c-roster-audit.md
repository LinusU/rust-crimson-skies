# F26-C: roster-wide handling audit

Task F26-C (#103). Code: `crates/cs_sim/src/probes/audit.rs`,
`tools/cs_inspect/src/handling.rs` (`cs-inspect handling --audit`); tests:
`crates/cs_sim/tests/accept_f26_c_roster_audit.rs` and
`accept_f26_c_*` in `handling.rs`.

## What exists

`audit_roster` flies every `RosterRow` (airframe id, configuration label,
optional tuning, optional envelope, loadout, damage) through `ProbeRunner` and
`compare`. Each row is `pass`, `fail` or `unavailable`, with per-entry signed
deviations, the assist profile with its tuning provenance, and the combined
probe hash. `fail` (any out-of-envelope entry) outranks `unavailable`. Only
`pass` counts for `RosterAudit::all_pass`; an empty roster is an error, never
all-pass. A missing envelope, missing tuning, refused run, unmeasurable
maneuver or an envelope that skips a sheet maneuver is `unavailable`. Rows are
independent; a refusal is recorded on its row only.

## Unknowns and limits

- **The roster is unknown.** No retail airframe list, tuning or reference
  envelope has been imported, so the audit takes caller-supplied rows and the
  CLI audits the declared synthetic roster only. Enumerating every supported
  airframe, custom-loadout extreme and forced mission configuration (sheet rule
  5) needs the retail tuning import and F26-D's envelopes. Until then the
  coverage claim is not made.
- **Assists are reported, not enumerated.** Only the `AssistProfile` fields the
  tuning carries are shown (enabled flag, bank/level gain). No other modern
  assist exists in `cs_sim` yet; any added later must appear here.
- **Stall recovery is not measurable on the stock synthetic airframe** (see the
  F26-B note); the audit reports it `unavailable`. A row can reach `pass` only
  with a tuning that recovers, e.g. a 0.8 rad stall angle and 0.8 residual
  (used in the tests; authored, not fitted).
- Quantity definitions and probe horizons remain the authored F26-B ones and
  are not verified against an original capture.
- The `handling --audit` exit code is 0 whenever a report is produced; an
  unavailable or failing row is report content. A consumer must read `all_pass`.
