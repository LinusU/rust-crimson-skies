# F03-A: the owner-path list contradicts Rust module wiring

Date: 2026-09-23. Task: #13 / F03-A "Implement checked reader and structured
errors". Recorded during review, per `AGENTS.md` rule 2 and
`docs/TASK-SPLITTING.md` §"Owner-controlled plan changes": the spec is wrong,
the evidence goes here, and the task is blocked for the owner rather than
merged with a waived acceptance criterion.

## The contradiction

`specs/F03-bounded-binary-parsing-primitives.md` line 5 and task #13 both give
these owner paths:

> `crates/cs_formats/src/io.rs`; `crates/cs_formats/src/error.rs`;
> `crates/cs_formats/tests/`; `tests/`; `docs/findings/`

and the task's "Done when" says **"Only owner paths changed."**

Implementing the stage cannot satisfy that:

1. On `origin/main`, `crates/cs_formats/src/lib.rs` is a three-line doc comment
   ("No implementation yet") with **no module declarations**
   (`git show origin/main:crates/cs_formats/src/lib.rs`).
2. `crates/cs_formats/` contains only `Cargo.toml` and `src/lib.rs`
   (`git ls-tree origin/main crates/cs_formats/`); there is **no `build.rs`
   anywhere** under `crates/` (`ls crates/*/build.rs` → none), so nothing else
   can declare the modules.
3. Rust compiles `src/io.rs` / `src/error.rs` only when the crate root says
   `pub mod io; pub mod error;`. Without those two lines the files are dead
   text: `cs_formats` exposes no reader and no `ParseError`.
4. The sheet requires tests that "call production code, not parallel
   test-only implementations" (`specs/F03`, Acceptance tests paragraph). The
   only ways around the crate root defeat that requirement:

   | alternative | why it does not work |
   |---|---|
   | `#[path = "../src/io.rs"] mod io;` in each test binary | compiles the sources *outside* the `cs_formats` crate, one copy per test binary — a test-only compilation path, and the crate itself still exposes nothing for F03-C ("integrate into all parser entrypoints") or F05+ to consume |
   | put the reader in `lib.rs` instead | still modifies `lib.rs`, and abandons `io.rs`/`error.rs`, which the owner paths require to exist |
   | leave `io.rs`/`error.rs` undeclared | dead files; no production code for the tests to call; the acceptance scenario is unmeetable |

So the F03-A acceptance criterion "Only owner paths changed" is **unsatisfiable
by any implementation**. The branch as submitted touches one file outside the
list: `crates/cs_formats/src/lib.rs`, +17 lines (module declarations, two
`pub use` re-exports, doc comment) and no logic.

## The gap is systematic, not a one-off

No feature sheet lists a crate root among its owner paths:

```sh
grep -n "Owner paths" specs/*.md | grep -i lib.rs   # zero matches
```

Every stage that adds a module to an existing crate has the same
contradiction: F01 (`crates/cs_types/src/evidence.rs`), F02 (`install.rs`),
F04 (`crates/cs_assets/src/vfs/`), F05 (`crates/cs_formats/src/rof.rs`), F06
(`zbd/`), F07 (`interp.rs`), F08 (`texture/`), F14 (`content.rs`,
`catalog/`) … F00's sheet was the exception — it owned all of `crates/`,
which is how `src/lib.rs` came to exist at all.

## What the owner needs to change (owner-protected files; an agent cannot)

Pick one, then unblock #13:

1. add the crate roots (e.g. `crates/cs_formats/src/lib.rs`) to the owner
   paths of the sheets that add modules to those crates, **or**
2. add a global rule to `AGENTS.md`: module declarations and re-exports in a
   crate's own `src/lib.rs` are implicitly in scope for any task that owns
   files inside that crate (wiring only — no logic), **or**
3. direct F03-A somewhere else entirely.

## Status of the branch (for whoever resumes the review)

`rally/13-implement-checked-reader-and-structured` at
`849e337ea24a4e8e02a0b50f89a5c56c770af446` is otherwise review-complete; the
lib.rs question is the **only** deviation the reviewer found.

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings`, `cargo test --workspace --locked`,
  `cargo test --workspace --locked -- accept_f03_a_ --include-ignored`
  (8 tests) — all exit 0 on the reviewer's machine.
- Reviewer sensitivity probes (each reverted afterwards, tree left clean):
  disabling the bounds check in `Reader::take`, dropping the `0x00`
  terminator requirement in `read_bounded_cstr`, rebasing `sub_reader` offsets
  to sub-range-local, and decoding `u16` big-endian each make
  `cargo test -p cs_formats -- accept_f03_a_` exit 101 with named failures.
- No protected path touched, no original data (`CS_GAME_DIR` never read), no
  binary files, no stubbed success, no guessed layout or constant.
- Recorded unknowns (retail string encoding per format; `SourceSpan`
  installation hash left to a consuming stage) are already documented in
  `docs/findings/2026-09-23-f03-a-checked-reader-and-structured-errors.md`.
