# F38-A: host-binding registry and raw-call lowering

Code: `crates/cs_script/src/bindings/mod.rs`; tests:
`crates/cs_script/tests/accept_f38_a_host_bindings.rs` (synthetic only).

## What exists

- `HostBindingRegistry`: exact-name map of `BindingSpec` (family, argument
  domains, `Lowering`, repeatability, provenance). Empty by default; it ships
  **no original call names**.
- `lower_program(registry, RawProgram) -> Result<MissionProgram, Vec<BindingError>>`:
  unknown names, wrong arity/type, out-of-domain arguments and oversize names
  are all reported with mission, objective, call index and source span. No
  program is produced on error, so nothing reaches `validate` or flight; a
  call is never lowered to `Action::Unknown` or a no-op.

## Design decisions

- `cs_content` may not depend on `cs_script` (`docs/01-ARCHITECTURE.md`), so
  the raw input types live beside the registry in `cs_script`; a future
  format/content adapter in `cs_formats`/`cs_content` must hand its decoded
  calls over as `RawCall` values via the consumer (`cs_sim` or a tool).
  No `script_adapter` module was added: there is no decoded original call
  stream to adapt yet (F13-C signatures unresolved).
- Only three lowerings exist (`SetVariable`, `Finish`, `GrantReward`), the
  actions F37-A's IR can express. Families such as actor lifecycle, docking or
  cues need new IR actions first.

## Recorded unknowns (not guessed)

- Original call names, signatures, argument domains and numeric conversions:
  unknown until F13-C/F38-B measure them. Provenance `Observed` is available
  but nothing in this slice uses it.
- Cancellation semantics and the repeatability of any original call: unknown;
  `Repeatability` is recorded but not yet enforced by the runtime.
- AC02 (differential traces), AC04 (coverage audit) and the source-to-IR map
  for archive/member offsets are F38-B/C/D.
