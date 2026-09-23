# Crimson Skies: agent contract

Build the **2000 PC game Crimson Skies**, not the Xbox sequel High Road to Revenge, as an independent reimplementation in Rust + Bevy + Avian that loads the owner's original game data.

Work is coordinated by **Rally** (the `rally` MCP server). One session does one piece of work: `request_work`, follow the returned `steps`, hand over with `submit_for_review` / `complete_review` / `save_checkpoint` / `split_task` / `block_task`, stop.

## What to read

Do not load the whole repository into context. For a task `F05-B` read, in this order:

1. The Rally task description and its history (checkpoint and review notes, owner notes).
2. Its feature sheet `specs/F05-*.md` (for missions `missions/M01.md`): the whole sheet, and closely the `### F05-B` section.
3. The one shared contract the task names in `docs/contracts/`.
4. The existing code in the task's owner paths.
5. For import/format/research work also `docs/research/FORMAT-NOTES.md`, `docs/research/RESEARCH-PLAYBOOK.md` and the relevant `docs/research/SOURCES.md` entries.

`docs/00-SCOPE.md` and `docs/01-ARCHITECTURE.md` describe the whole product and crate layout.

## Rules

1. **One bounded slice.** Implement only this task's stage. Stay inside the owner paths listed in the task. If it is bigger than one format variant, one focused behavior or one UI path, use `split_task` (see `docs/TASK-SPLITTING.md`) and keep all acceptance criteria.
2. **The plan is not yours to change.** Never edit `specs/`, `missions/M*.md`, `docs/contracts/`, `docs/research/`, `docs/templates/`, `docs/TASK-SPLITTING.md`, `schemas/`, `fixtures/synthetic/`, `tools/*.py`, `.github/`, `opencode.json` or this file. Rally refuses to merge branches that touch them. Research discoveries go into `docs/findings/`. If the spec is wrong, record the evidence there and `block_task`.
3. **Original data is read-only and never committed.** The installation is at `$CS_GAME_DIR`. Never write inside it. No game assets, executables, extracted scripts, commercial fonts, manuals, decompiled code or screenshots of original content in Git. Private outputs go in `private/` (ignored by Git).
4. **Unknown means unknown.** Unknown formats, units, opcodes and game rules are recorded as unknown with evidence and the affected content. Do not guess a layout, ignore an opcode, fabricate a tuning table or replace an original mission with a generic fight.
5. **Missing data or capabilities block; they never pass.** If the task needs a capability you do not have, call `block_task` and say exactly what is needed. Synthetic tests do not prove retail compatibility. A parsed asset needs a working runtime consumer.
6. **Tests are real.** Every task has a test prefix, e.g. `accept_f05_b_`. At least one test with that prefix must exist, call production code, run and pass, and must fail when the implementation is removed. Never weaken, skip or delete tests or lints to get green. Zero matching tests is a failure.
7. **Canonical contracts.** One physics pose owner, integer simulation ticks, stable content/actor ids and session generations. No Bevy/renderer dependency in `cs_types`, `cs_formats`, `cs_assets`, `cs_content`, `cs_script` or `cs_net`. No game state hidden in UI code.
8. **Never self-award `verified_original` or `release_approved`.** A merged task is **checked**, not "recreated".
9. **Git hygiene.** Only work on the Rally task branch. Never push to `main`, never force-push another branch, never rewrite someone else's history. Small commits with imperative messages that describe one change.
10. **Shell access is not a sandbox.** Do not touch files outside this checkout except reading `$CS_GAME_DIR`. Do not download proprietary media, install global software or start background jobs that outlive your session.

## Environment

| Variable | Meaning |
| --- | --- |
| `CS_GAME_DIR` | Absolute path to the read-only original installation. Unset: you have no `retail` capability. |
| `CS_CAPABILITIES` | Comma-separated capabilities this machine can really provide, e.g. `retail,gpu,audio`. Anything not listed is unavailable. |

`human_play`, `human_review` and `network_real` are never available to an agent: they need the owner. Tasks that require them are blocked until the owner can supply the evidence.

## Checks before every push

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- <task-test-prefix> --include-ignored   # must run >= 1 test, all passing
```

CI runs the first three on every push. CI has no original data: tests that need it are marked `#[ignore = "requires CS_GAME_DIR"]` and must fail loudly (not pass) when run without `CS_GAME_DIR`. You run them locally with `--include-ignored`; so does the reviewer.

Bevy builds are slow. Call Rally `heartbeat` during long builds and CI waits.

## Evidence-bound tasks

A task that needs any capability besides plain build/test (`retail`, `gpu`, `audio`, ...) must produce an evidence report as described in `docs/contracts/CLI-EVIDENCE.md`: the acceptance harness writes `private/evidence/<TASK-ID>/acceptance.json` (schema `schemas/evidence.schema.json`), you validate it with `tools/validate_evidence.py` and commit a copy as `docs/findings/evidence/<TASK-ID>.json`. Never write a report by hand or fabricate one to unblock a task.

## Handing over

The `submit_for_review` summary must list: what changed, files, commands run with their exit codes, the task tests discovered and executed, evidence produced, unmet criteria and sources used.

## Reviewing

Reviews come first in Rally. As a reviewer you own getting the branch right: check it against the spec section and this file, fix problems yourself, rerun every check above (including the ignored task tests with `CS_GAME_DIR`), then follow the steps to `complete_review`. Look especially for tests that do not exercise production code, stubs that return success, guessed layouts or constants, touched protected paths and binary files without provenance.

## Hard failure conditions

An original mission cannot be marked ready while it contains unknown reachable instructions, missing gameplay assets, placeholder victory logic or unverified critical native calls. A full release cannot be approved without actual original-data, visual, audible and ordinary-play evidence and the owner's approval.
