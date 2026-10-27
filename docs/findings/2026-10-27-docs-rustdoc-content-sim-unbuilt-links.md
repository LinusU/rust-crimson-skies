# DOCS-RUSTDOC-CONTENT-SIM: links that named things the code does not have

Date: 2026-10-27. Task: DOCS-RUSTDOC-CONTENT-SIM "Fix remaining rustdoc
warnings in cs_content and cs_sim" (Rally #2124), under the parent #413
(DOCS-RUSTDOC-WARNINGS) rule that a link to something never built is a
finding, not a reworded comment. Capability used: none — every check is
`synthetic`; no original data is read and nothing here is a claim about the
original game.

## `DeclaredObjectiveProgram::with_recovery` — promised, never built

`cs_content::objectives`' module doc (the F39-D "Support" bullet) and the
`DeclaredObjectiveProgram` struct doc both said an `installation` record starts
`DeclaredSupport::Original` "with **no** recovery and is therefore unplayable
until an importer attaches one through `DeclaredObjectiveProgram::with_recovery`".

No `with_recovery` exists — not on `DeclaredObjectiveProgram`, not anywhere in
the crate graph. The API the docs promised — a way to attach a recovery that
makes an original-derived program playable — was never written, and the actual
semantics deliberately run the other way:

- `DeclaredSupport::is_playable` is `true` only for `Authored`
  (`crates/cs_content/src/objectives.rs:1171`); an `Original` program cannot
  become playable by attaching anything.
- `DeclaredObjectiveProgram::with_measured_record`
  (`crates/cs_content/src/objectives.rs:2463`) attaches F39-D's measured
  declaration-site census *and keeps* `DeclaredSupport::Original`: "a
  declaration site is not a rule". Attaching the measurement is what lets a
  refusal name the numbers; it is not a recovery.

The recovery the docs gestured at is the unmeasured mission-language
instruction table (F13-B/C) plus the lowering that would run an original
program — the same gate `docs/contracts/SCRIPT-MISSION.md` already states
("if the actual program … cannot be decoded, the mission remains
Unsupported"). Both doc sites now name `with_measured_record` and say the
recovery is still unbuilt, so the doc no longer promises an API a caller
cannot call.

A follow-up task, if the project wants one, is the real importer-side
recovery path (attach measured semantics and promote `Original` to playable);
nothing in this record asserts whether that is F13-B/C's own deliverable or a
separate task.
