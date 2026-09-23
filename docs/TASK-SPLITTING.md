# Splitting work and revising the plan

The initial 332 tasks in Rally are a **feature/stage DAG**, not a claim that an unknown mission VM or entire UI can be implemented safely in one agent session. Each implementation attempt should change one format variant, one focused behavior or one UI path. F13/F38 and the content catalog will reveal additional concrete work units.

## Splitting a claimed task

When a task is too broad, call Rally's `split_task` with the subtasks instead of producing a giant change. Rally queues the subtasks and brings the original task back as the integration step once they are all done; it then still has to meet its **full original acceptance criteria**. Do not redefine done, edit the specification or drop acceptance cases to make a task fit.

Examples of a good split: one original opcode/native family per subtask; one newly discovered GameZ record variant; one airframe's calibration probes; one IA preset; one multiplayer mode variant.

Give subtasks keys like `F38-B.01`, `F38-B.02`, keeping `F38-B` as the parent. Each subtask description must be self-contained (the agent that picks it up starts with a fresh context) and state:

- the spec section and shared contract it implements,
- its dependencies (Rally `dependsOn`, by key),
- the owner paths it may change,
- the capabilities it needs (`retail`, `gpu`, `audio`, ...),
- a unique positive test prefix, e.g. `accept_f38_b_01_`,
- a discriminating acceptance case and what does not count as passing.

## Discovered follow-up work

Work you discover that is not part of the current task (a new format variant, an unrelated bug, a missing consumer) goes into Rally with `create_tasks`, with the same fields as above. Do not fix it in the current branch.

## Owner-controlled plan changes

`specs/`, `missions/M*.md`, `docs/contracts/`, `docs/research/`, `schemas/`, this file, `AGENTS.md` and `.github/` are protected: Rally refuses to merge a branch that changes them. If a specification is wrong or contradicts the original data, record the evidence in `docs/findings/` and call `block_task` explaining what needs to change. The owner updates the plan.

The initial complete-product acceptance requirements remain binding even as the task count grows. A final release must not depend only on whichever subset of tasks happened to be implemented.
