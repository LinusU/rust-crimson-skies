# M10-B: M10's control program, measured, and why it does not validate yet

Date: 2026-10-09. Task: M10-B "Implement and regress mission-specific
compatibility gaps" (#286, `missions/M10.md`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail` (read-only
`$CS_GAME_DIR`). Implementer: **claude-2/claude-2** (Sonnet 5.5, 2026-10-09).
Reviewer: **bunny-alpha-2/bunny-alpha-2** (Rally review claim of 2026-10-09T04:22Z,
a different agent instance and model, fresh context). That review is independent
of the implementation, but it is an agent review of the code and the tests: it
is not independent original-reference evidence and not original-run evidence,
and no agent review replaces the owner's human approval.

## What changed

No production code. The census (`cs_app::mission_control`), the directive
dispositions and the record → `RawProgram` adapter are M01-LC's; this stage runs
them over M10 and pins the result in `crates/cs_app/tests/campaign/m10_b.rs`
(seven retail and two synthetic `accept_m10_b_*` tests) plus the evidence harness
in `evidence.rs`. Wiring: `campaign/main.rs` (`mod m10_b;`).

## Measured

| Fact | Value |
| --- | --- |
| Reader archive | `ZBD/C2/M05/zrdr.zbd`, 14 members, SHA-256 `df57933f…e7b4390` (the program M10-A bound) |
| Control member | `objectives.zrd`, the only member declaring numbered blocks |
| Blocks / sites / keys | 49 (numbered 1..=49, no gaps) / 232 / 36 |
| Record-level keys | no unclassified key, no unreadable block |
| Terminal outcomes | `INSTANTWIN` in block 28, `INSTANTLOSS` in block 12, one site each |
| Dispositions | 2 terminal, 34 measured, **0 refused** |
| Calls | all 232 sites bind to a host call; no unbound key |
| Conditions | 47 of 49 lower, **2 refused** (blocks 22 and 35) |

Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT -1`).
Block 28 is named by block 11 (a kill) and block 26 (a nap); block 12 by blocks
11 and 34 (naps). Block 11 therefore kills the win block and naps the loss block;
the tests pin that as spelled and make no claim about which fires first. All 52
spelled wake/kill/nap/gate addresses are in `1..=49` (the spelled value is the
block number, which the original `dec`s into an index).

Sheet priorities. The sheet names a protected neutral actor, attached payloads
and air and surface threats without naming a key; nothing in the record says
which actor is protected, neutral or attached, and none is guessed. What the
record spells: group depletion `DEDG` in blocks 26, 27, 34 (`[1,0] [1,2] [2,0]`);
`WAKEUP_ENEMIES` (21) and `WAKEUP_GENERATOR` (18, 47); 97 `INACTIVE<n>` sites with
10 `INACTIVE_COMPLETION_COUNT` thresholds; `SET_AI_NET` (23, one pair, so inside
the host-call bound), `SET_AI_TEAM` (36); and `cargozep2` as the anchor of both
`TRAVELERS` sites.

## The M10-specific gap

Two of 49 conditions refuse, so the lowered program fails validation, the
`objective_condition` and `call_arguments` rows are unmet and M10 is not ready:

**`TRAVELERS` counting mode** (block 22 `[2, APPROACHING, cargozep2, 200, 1]`,
block 35 `[2, LEAVING, cargozep2, 2000, 1, DELETE_ON_SUCCESS]`). A non-string
`child0` arms the counting mode, which accumulates a count into the objective
during evaluation (`+0x5b8`): a write, so no side-effect-free `Condition` can
carry it. This is the first mission measured here that spells the counting mode.
Open questions, all **unknown**: how the runtime should carry that counter;
what `child0 = 2` and the trailing `1` mean; and whether the original compares
the polarity word with `LEAVING` (it is spelled here for the first time; the
measured rule only distinguishes `APPROACHING` from everything else). Filed as
**M10-B-FU1** (#812).

## Not claimed

No playthrough, difficulty, optional or presentation row (M10-SUCCESS …
M10-DIFFICULTY) is covered: those need the runtime and human play (M10-C). The
meanings of the `INACTIVE` in-play bit, `DEDG` group ids and `IDENTITY` strings
stay as unknown as the M01-LC findings left them.

## Mutation checks

Each applied, run against `accept_m10_b_`, observed and reverted:

| Mutation | Result |
| --- | --- |
| `cs_content::mission_control::terminal_outcome_of`: `INSTANTWIN` → `None` | 3 failures (dispositions, lowering, synthetic terminal-block binding) |
| `m10_b.rs`: expected block count 49 → 48 | 3 failures (control program, terminal blocks, lowering) |

## Review

Reviewed by **bunny-alpha-2/bunny-alpha-2** on 2026-10-09 in a fresh session
(a different agent instance and model from the implementer), against
`missions/M10.md` (`M10-B`), `docs/contracts/SCRIPT-MISSION.md`,
`docs/contracts/CLI-EVIDENCE.md` and the diff. Review is independent of the
implementation; it is an agent review of the code and tests, not independent
original-reference evidence and not original-run evidence.

- Rebased onto `origin/main` (`26cc21a5`) without conflicts; the branch then
  carried the three stage commits plus the reviewer-identity commit.
- Checks on that rebased tree, 2026-10-09 04:29–04:40 UTC: `cargo fmt --all --
  --check` (0), `cargo clippy --workspace --all-targets --all-features --locked
  -- -D warnings` (0), `cargo test --workspace --locked` (0), `cargo test
  --workspace --locked -- accept_m10_b_ --include-ignored` (0: 9 discovered, 9
  executed, 9 passed, 0 failed, 0 ignored).
- Evidence regenerated from that run with the recipe in the `evidence.rs`
  header (`candidate_tree` `be666013…`, the tree of the commit tested), written
  to `private/evidence/M10-B/acceptance.json` and validated with
  `tools/validate_evidence.py --require-pass` (exit 0). The committed copy in
  `docs/findings/evidence/M10-B.json` is that file; the only field changes from
  the implementer's copy are `candidate_tree`, `created_at`, the artifact hash
  of this run's log, the `cwd`, and `review.identity` / `review.method`, which
  now name the reviewer instead of the hand-over placeholder.
- Both mutations above re-applied by the reviewer and reverted, each failing
  exactly the same 3 of the 9 tests the implementer recorded; the working tree
  was clean afterwards.
- `python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py'`
  → 27 tests, OK (M10-B is not in a Rally review snapshot yet, so its
  implementer/reviewer pair is an advisory note there, as the reader documents).
- No protected path, no original data and no binary file changed; the only
  files on the branch are the two test files, the `mod m10_b;` wiring line,
  this finding and the evidence copy.

Not fixed here, unchanged and still open: the `TRAVELERS` counting mode
(blocks 22 and 35) and the polarity word `LEAVING` are **M10-B-FU1** (#812), and
`M10` stays unready until they are measured.

