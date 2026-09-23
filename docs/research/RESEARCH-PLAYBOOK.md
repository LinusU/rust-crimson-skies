# Closing unknowns without inventing answers

## Bounded investigation unit

One unit examines one file family, one instruction/native call, one transform convention or one gameplay rule. Input is a concrete hypothesis, source locator and affected content ids. Output is a short evidence report plus a discriminating test. A report that just says "seems fine" is rejected.

Record competing hypotheses before testing. For ROF lengths, compare actual stored stream boundary and decoded size, not a filename. For mission timers, hold kills constant while varying elapsed time and vice versa. For trigger direction, approach the same geometry from different sides. For coordinate scale, compare several distances and motion/instruments. For damage, use controlled weapons/zones and retain uncertainty.

## Measurement hygiene

Record edition/patch/locale, difficulty, loadout, initial condition, input sequence and relevant options. Use reproducible captures of original behavior where possible. A community guide's best strategy is not a complete branch specification; contradictory behavior can be due to edition, difficulty, random seed or author uncertainty. Keep contradictions visible until resolved.

Reference executable observation is a separate owner-run research activity. The new engine never needs to execute the old binary. Do not bundle protected executables, decompiled implementation, DRM tools or original assets into reports.

## Escalation triggers

Stop a slice and record a blocker after two independent hypotheses fail or the model cannot explain a field/branch. Do not fill the gap with a guessed struct, generic mission, success-returning stub or infinite retries. Call Rally's `block_task` so the owner can assign a stronger agent or a human measurement session. Independent tasks remain available, but the affected original content stays unsupported.

## Corpus expansion

After the initial inventory, create additional bounded work orders for each newly discovered variant, opcode/native binding, airframe configuration, IA preset and multiplayer mode. `docs/TASK-SPLITTING.md` defines the safe process. Do not assume the initial 65 feature headings are a complete opcode catalog. The fixed release criteria require complete coverage of the discovered original corpus even if that increases task count.

## Definition of a closed unknown

The observed representation/behavior is specified, source-scoped and independently checked; parser/runtime implementation uses it; a failing-before/passing-after regression exists; affected dependency closures become ready; the evidence is fresh for current code/data. Merely finding an explanatory web page or compiling a parser is not closure.
