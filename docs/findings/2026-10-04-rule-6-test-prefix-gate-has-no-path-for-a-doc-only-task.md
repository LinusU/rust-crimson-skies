# Rule 6's test-prefix gate has no satisfiable path for an authorized documentation-only task

Date: 2026-10-04. Task: #615, opened by bunny-alpha-1 while reviewing #614
(F54-X9, "AGENTS.md: 'could not execute process (never executed)' means rerun
once, never 'the run was green'"). Measurements 1-4 were taken on
`origin/main` = `bf72340cf97a5eeb46acc20bc9d0811a4bd3711c` with rustc 1.98.1 /
cargo 1.98.1 on the implementing host; #614 has since landed as `47b1e547`, so
this finding is rebased onto it and the re-measurement on that base is noted
under measurement 4. Capabilities used: ordinary build/test only — no
`CS_GAME_DIR` read, no `retail`/`gpu`/`audio`, no human play. This is a finding
about the project's own agent contract on a multi-agent machine; it says
nothing about the original game.

## The gap, in one sentence

`AGENTS.md` rule 6 requires every task to have a test prefix that resolves to a
running test, and a task whose authorized paths contain no test file cannot
produce one, so such a task cannot pass the gate its own contract defines.

## The task shape makes the prefix unconditional

The owner-authored shape for a task description is in the repository, in the
protected `docs/TASK-SPLITTING.md`. Every task description must be
self-contained and state, among other things (lines 11-18), "the owner paths it
may change", "a unique positive test prefix, e.g. `accept_f38_b_01_`", and "a
discriminating acceptance case and what does not count as passing".

There is no documentation-only exception on that list, and the same file's
"Discovered follow-up work" section applies the same list to follow-up tasks
created with `create_tasks`. So the prefix requirement reaches a
documentation-only task through three independent owner-controlled documents
(rule 6, the contract's testing section, and this task shape), and no
agent-created task description can opt out of any of them.

The same file also states the procedure this task follows, in "Owner-controlled
plan changes" (lines 26-28):

> `specs/`, `missions/M*.md`, `docs/contracts/`, `docs/research/`, `schemas/`,
> this file, `AGENTS.md` and `.github/` are protected: Rally refuses to merge a
> branch that changes them. If a specification is wrong or contradicts the
> original data, record the evidence in `docs/findings/` and call `block_task`
> explaining what needs to change. The owner updates the plan.

## The rule as written

`AGENTS.md`, rule 6:

> **Tests are real.** Every task has a test prefix, e.g. `accept_f05_b_`. At
> least one test with that prefix must exist, call production code, run and
> pass, and must fail when the implementation is removed. Never weaken, skip or
> delete tests or lints to get green. Zero matching tests is a failure.

`docs/contracts/CLI-EVIDENCE.md` repeats it twice, in the testing section
(line 54: "The task-specific test prefix must resolve to at least one real
test.") and in the test-discovery section (line 64: "`cargo test --workspace
--locked -- <prefix> --include-ignored` must execute and pass at least one
test, and each discovered test must pass when run alone with `--exact`"). Rule
6 also carries an escape hatch, as a sub-bullet:

> **Python-only maintenance.** A task that the owner explicitly authorizes to
> change only Python tools or documentation uses the nonempty Python test
> selection its description names, e.g. `python3 -m unittest discover -s
> tools/tests -p '<file>' -v`. The selection must exercise the production tool
> or documents and fail when the change is removed. Do not add a Rust test that
> merely wraps it to satisfy a prefix.

So the escape hatch exists, but it is conditioned on two things an
implementation-only task cannot supply for a prose edit: a selection **named by
the task description**, and a selection that **fails when the change is
removed**.

## Measurement 1: the production gate refuses the prefix

The repository enforces rule 6 in production code:
`tools/cs_xtask/src/test_select.rs`, `classify_selection`, turns
`parsed.passed == 0` into `SelectError::Empty`. Run on current `main`:

```
$ cargo run -q -p cs_xtask --locked -- test-select --prefix accept_f54_x9_
cs-xtask: prefix "accept_f54_x9_" selected no test; a task test prefix must resolve to at least one test that runs
exit 1
```

## Measurement 2: the plain selection is a green run with zero tests

The command rule 6 prints as the fourth check is not a gate either. On the same
tree:

```
$ cargo test --workspace --locked -- accept_f54_x9_ --include-ignored
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 85 filtered out; finished in 0.00s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
... (every test target reports 0 passed)
exit 0
```

Every harness reports `0 passed` and the workspace exits 0. A selection that
runs nothing cannot fail, so it certifies nothing about the edit. This is why
`cs_xtask test-select` exists; for a documentation-only task both the enforced
gate and the printed one are unusable.

## Measurement 3: the diff admits no test at all

#614's authorized path list is `AGENTS.md` only, so no test file can be added
to that branch even if a test were the right answer. Measured:

```
$ base=$(git merge-base FETCH_HEAD origin/main)   # FETCH_HEAD = rally/614-agents-md-...
$ git diff --stat "$base" FETCH_HEAD
 AGENTS.md | 6 ++++++
 1 file changed, 6 insertions(+)
```

Six lines of prose in one protected file. There is nowhere on that branch to put
a test.

## Measurement 4: the only readable Python checker cannot fail on this edit

`python3 -m unittest discover -s tools/tests -p 'test_plan_sync.py'` is the
repository's only check that reads `AGENTS.md` at all. Its entire `AGENTS.md`
assertion is one `assertIn`:

```python
# tools/tests/test_plan_sync.py:194-196
for path in ('AGENTS.md', 'docs/00-SCOPE.md', 'docs/TASK-SPLITTING.md'):
    self.assertIn('specs/README.md' if path != 'AGENTS.md' else 'allowProtectedChanges',
                  (ROOT / path).read_text(), path)
```

It checks that `AGENTS.md` still mentions `allowProtectedChanges`. It asserts
nothing about the text a documentation edit adds. Measured both ways, by
applying #614's own diff and then reverting it:

```
$ git apply x614.patch          # AGENTS.md +6 lines, the F54-X9 paragraph present
$ python3 -m unittest discover -s tools/tests -p 'test_plan_sync.py'
Ran 6 tests ... OK              exit 0

$ git checkout -- AGENTS.md     # paragraph removed again
$ python3 -m unittest discover -s tools/tests -p 'test_plan_sync.py'
Ran 6 tests ... OK              exit 0
```

6/6 both times. The selection is not insensitive to the *file* — it would catch
a truncated or unreadable `AGENTS.md` — but it is completely insensitive to the
*edit*. Rule 6 requires a selection that "fail[s] when the change is removed",
and this one does not, so naming it does not satisfy the sub-bullet as written.

#614 landed while this task was open, as `47b1e547` ("docs(agents): split the
vanished-harness rule into three paragraphs"), so the paragraph is now on `main`
and the experiment reproduces in one step: delete the F54-X9 paragraphs from
`AGENTS.md` on `47b1e547` and run the selection. Measured there — the string
`never executed` is gone from the file, and the selection still reports 6/6 OK,
exit 0.

## Measurement 5: this task fails its own gate the same way

#615's own prefix cannot be satisfied either, which is the point of the task.
On the tree carrying this finding, with the whole workspace green (349 test
targets, 3373 passed, 0 failed, 377 ignored without `CS_GAME_DIR`):

```
$ cargo run -q -p cs_xtask --locked -- test-select --prefix accept_t615_
cs-xtask: prefix "accept_t615_" selected no test; a task test prefix must resolve to at least one test that runs
exit 1

$ cargo test --workspace --locked -- accept_t615_ --include-ignored
summed "0 passed" over all 349 targets
exit 0

$ python3 -m unittest discover -s tools/tests -p 'test_plan_sync.py' -v
Ran 6 tests ... OK              exit 0
```

Both available gates are green-or-nothing here, and the one that is not green
is the one the contract points at. That is why this finding ships with no test
and #615 is blocked rather than submitted: the honest state is reported, not
papered over with a test that asserts prose.

## Why an agent cannot close this from the repository

Every fix changes a path AGENTS.md rule 2 reserves for the owner, and #615 has
`allowProtectedChanges: false`:

* rule 6's own carve-out lives in `AGENTS.md` (protected);
* the matching contract sentences live in `docs/contracts/CLI-EVIDENCE.md`
  lines 54 and 64 (protected);
* naming a selection per task is a change to the task-description shape in
  `docs/TASK-SPLITTING.md` lines 11-18 (protected).

There is no fourth place. `docs/findings/` can record the gap (this file), but a
finding is evidence, not a contract: it cannot satisfy a gate that a protected
document states. Adding a Rust test that asserts the prose of a documentation
edit is the shim that both rule 6 and the owner directive of 2026-09-28
explicitly forbid, and it must not be used to make this look merged.

## What the owner has to choose

Three coherent fixes, all requiring an owner edit:

1. **Name the selection in the task-description shape.** Every authorized
   documentation-only task description carries a nonempty Python selection, and
   that selection is the satisfied gate. No contract change; needs a line in
   `docs/TASK-SPLITTING.md` next to "a unique positive test prefix" so it is not
   left to each implementer. Weakness: it still requires a selection that can
   actually fail on the edit, and measurement 4 shows the existing one cannot.
   Also does not cover a doc-only task with no readable checker at all.
2. **Carve rule 6 out for authorized documentation-only tasks.** State that when
   the authorized path list contains no test file, the gate is satisfied by the
   owner's own acceptance criteria read against the diff, and that the reviewer
   records the zero-test selection in the `complete_review` notes. This is the
   smaller change and it is how #614 actually had to be reviewed.
3. **Both.** The carve-out for the contract, and the named selection wherever a
   real one exists.

Three things worth deciding at the same time as the choice itself:

* `docs/contracts/CLI-EVIDENCE.md` lines 54 and 64 state the prefix rule with
  no documentation-only exception. If rule 6 gains a carve-out and the contract
  does not, the two disagree and the next agent has to guess which governs.
* `docs/TASK-SPLITTING.md` lines 11-18 ask every task description for "a unique
  positive test prefix" with no exception either, so the same carve-out has to
  reach that list or new tasks will keep asking for a prefix that cannot exist.
* Every future `AGENTS.md` edit has this shape, so this is not a one-off for
  #614; the owner has asked for several sentences in that file.

## What was not done, deliberately

No Rust test was added. No selection was renamed, widened or made tolerant. No
protected path was touched. This finding records the gap and stops there, which
is the procedure `docs/TASK-SPLITTING.md` lines 26-28 prescribes for a
specification that cannot be satisfied as written: record the evidence in
`docs/findings/`, call `block_task` explaining what needs to change, and let the
owner update the plan. The decision belongs to the owner, and #615 stays blocked
until it is made.
