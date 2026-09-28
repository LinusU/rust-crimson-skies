# F03-D: the truncation and fuzz corpus, and the resource limits it ran under

Date: 2026-09-28. Task: F03-D "Run truncation and fuzz corpus with recorded
resource limits" (`specs/F03-bounded-binary-parsing-primitives.md`). Capabilities
used: ordinary build/test only; `CS_GAME_DIR` was not read and no original
data appears in this change.

## What was built

No production code changed: `crates/cs_formats/src/io.rs`,
`crates/cs_formats/src/error.rs` and `src/lib.rs` are byte-identical to the
F03-C merge (`git diff` shows only the files below). The corpus found no
regression to repair, so nothing was weakened to make a test pass.

| file | contents |
|---|---|
| `crates/cs_formats/tests/common/nested.rs` | the synthetic grammar (leaf / bounded cstr / nested range / table), a SplitMix64 stream per seed, the `decode_node` + `decode_all` driver that runs through `ParseContext::parse`, an emitter of *valid* buffers, ten handcrafted hostile vectors, and the recorded-limit constants |
| `crates/cs_formats/tests/accept_f03_d_fuzz_nested_range_and_string.rs` | three `accept_f03_d_*` tests: per-case invariants over the whole corpus, corpus-coverage assertions, and the pinned hostile vectors |
| `crates/cs_formats/tests/accept_f03_d_truncation_corpus.rs` | one `accept_f03_d_*` test: every emitted buffer truncated at every byte boundary |
| `crates/cs_formats/tests/common/mod.rs` | wiring only: `pub mod nested;` |

## The grammar (synthetic, EvidenceClass `Designed`)

```text
node   := kind:u8, payload          (variant = kind & 0b11)
kind 0 := leaf  : len:u16, utf8[len]
kind 1 := cstr  : max:u8, field[max] containing one 0x00
kind 2 := range : len:u16, child nodes filling exactly len bytes
kind 3 := table : count:u32, elem:u8, payload[count * elem]
```

It exists to touch exactly the operations F03-D fuzzes — `Reader::sub_reader`
(nested ranges), `Reader::read_str` / `Reader::read_bounded_cstr` (strings),
`AllocationBudget::reserve` (table extents) and `RecursionBudget::enter`
(nesting) — through the F03-C entrypoint. Whether any *retail* container is
NUL-terminated, length-prefixed or fixed-padded remains **unknown** and stays
with the per-format stages (F05+), exactly as F03-A and F03-C recorded.

Two structural properties carry the evidence:

1. **Every node consumes exactly the bytes it declares**, and the driver keeps
   reading nodes until its range is empty (asserted after each attempt). So a
   successful decode accounts for every input byte — no trailing byte is
   skipped as padding (non-negotiable #4).
2. **Every emitted buffer is a single root node whose extent is the whole
   buffer** (`stats.roots == 1`). Truncating it therefore always cuts inside
   the root's declared extent, which is what makes "refused at every boundary"
   a theorem about the emitter rather than a hope about the reader.

## Recorded resource limits

These are the limits every `accept_f03_d_*` case ran with. All of them are
*designed* test budgets (EvidenceClass `Designed`), never observed original
values:

| limit | value | constant | how it is pinned |
|---|---|---|---|
| allocation budget per attempt | 8192 bytes | `RECORDED_ALLOCATION_LIMIT` | a table reserving exactly 8192 bytes is accepted, 8193 is refused unallocated |
| recursion ceiling per attempt | 8 nodes | `RECORDED_MAX_DEPTH` | a 12-level chain is refused at level 9; an 8-level chain parses |
| fuzz random seeds | 256 (`0..256`) | `FUZZ_RANDOM_SEEDS` | `assert_eq!(cases, 10 + 32 + 256)` so the corpus cannot silently shrink |
| largest raw random input | 96 bytes | `FUZZ_MAX_INPUT` | length drawn from the same seed stream |
| emitted buffers | 32 seeds, ≤ 12 nodes, ≤ 64 bytes of children per range | `EMIT_SEEDS`, `EMIT_NODES`, `EMIT_MAX_BYTES` | each parses, then each is truncated at every boundary |

Every case gets a **fresh** `ParseContext`, so the recorded limits are
per-attempt and a failed attempt must leave `used() == 0` and `depth() == 0` —
which is asserted after every one of the 1396 decode attempts below (298 fuzz
cases, plus 32 emitted buffers and their 1066 truncated prefixes).

## Observed results (production code unchanged)

`cargo test --workspace --locked -- accept_f03_d_ --include-ignored --nocapture`:

```text
f03-d fuzz: 298 cases (35 accepted, 263 refused), 63 nodes, variants [12, 10, 25, 16],
  38 bounded fields, error kinds {"allocation_budget_exceeded": 49, "invalid_encoding": 2,
  "missing_terminator": 14, "recursion_depth_exceeded": 1, "unexpected_eof": 197},
  peak depth 8/8, peak allocation 8192/8192 bytes
f03-d truncation: 32 emitted buffers, 1066 truncated boundaries refused with
  unexpected_eof, variants [11, 10, 18, 14]
```

Both peaks are reached *exactly* and never crossed: the corpus fills the
recorded allocation budget to the byte (crafted exact-fit vector) and reaches
the recursion ceiling (crafted 12-level chain), and no attempt exceeds either.
All four node variants are decoded in both halves of the corpus, and all five
refusal classes this grammar can produce are observed.

`length_overflow` is the one `ParseErrorKind` the corpus never produces: a
`u32` count times a `u8` element size is at most 2³⁹, which fits both `u64`
and 64-bit `usize`, so that branch is unreachable *from this grammar*. That is
recorded here rather than papered over with a fake overflow vector; the
`u32::MAX`-count and `offset + length` overflow paths are pinned by the F03-B
tests (`accept_f03_b_*`). Supported targets are 64-bit; on a 32-bit target the
`u32::MAX` table vector would report `LengthOverflow` (its 68 719 476 720 bytes
do not fit a 32-bit `usize`) instead of `AllocationBudgetExceeded`, which the
hostile-vector test would then flag.

## Sensitivity evidence

Four mutations applied to `crates/cs_formats/src/io.rs` and reverted with
`git checkout --` afterwards (verified with `git diff --quiet`: the file is
byte-identical to the merge base after every probe). Each ran
`cargo test -p cs_formats --locked --no-fail-fast -- accept_f03_d_`:

| mutation | result |
|---|---|
| `Reader::take`: `if available < len` → `if false && available < len` | exit **101**; **4 of 4** FAILED — the fuzz binaries panic with `range end index 1 out of range for slice of length 0`, the truncation sweep stops accepting its own prefixes |
| `AllocationBudget::charge`: `if bytes > available` → `if false && …` | exit **101**; **3 of 4** FAILED (hostile vectors, per-case invariants, coverage); the truncation test passes because emitted tables stay inside the limit either way |
| `RecursionBudget::enter`: `if depth < self.max_depth` → `if true` | exit **101**; **3 of 4** FAILED (the 12-level chain parses instead of being refused, and the observed peak goes past 8) |
| `ParseContext::parse`: drop `self.allocation.rollback_to(mark)` | exit **101**; **3 of 4** FAILED — only the coverage test passes; the "table reserves 64 bytes, then its sibling is truncated" vector keeps its charge, and the truncation sweep's refused prefixes keep theirs |

The "one observable failure" named before editing holds: without the bound
check in `Reader::take`, `accept_f03_d_truncation_corpus_fails_at_every_boundary`
fails at the first truncated boundary instead of reporting `UnexpectedEof`.

## Commands run (all exit 0 unless noted)

| command | exit |
|---|---|
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f03_d_ --include-ignored` | 0 (**4** tests: 3 in the fuzz binary, 1 in the truncation binary) |
| `cargo run -p cs_xtask --locked -- test-select --prefix accept_f03_d_` | 0 (4 selected, 4 re-ran `--exact`, 4 passed) |
| the four mutation probes above | 101 / FAILED (expected) |

## Design decisions (engineered, not observed)

1. **No new dependency.** The seeded generator is six lines of SplitMix64 in
   the test module, fully specified and reproducible from the seed printed
   next to any failing assertion. `cs_formats` still depends only on
   `cs_types`, and `Cargo.lock` is untouched.
2. **Fuzz and truncation share one decoder.** The random-bytes corpus proves
   the decoder never panics and never escapes its budgets on hostile input;
   the emitted corpus proves it also *fails closed* when the very same decoder
   is starved of bytes. Two independent decoders would prove neither.
3. **Refusal classes are asserted, not counted by luck.** The ten crafted
   vectors guarantee `unexpected_eof`, `invalid_encoding`, `missing_terminator`,
   `allocation_budget_exceeded` and `recursion_depth_exceeded` even if the
   random half of the corpus changed shape, so the coverage test cannot go
   vacuous while staying green.
4. **The limits are boundaries in both directions.** Peak depth must equal 8
   and peak charge must equal 8192: a ceiling that is never reached would hide
   a budget that is too tight to matter, and a peak above it would mean a case
   escaped the limit.

## Open point for a later stage (recorded, not guessed)

Unchanged from F03-C: `cs_formats` still has no format-specific parser, so the
corpus is the consumer of `ParseContext::parse` for now. Nothing here certifies
retail behaviour — no installation hash, no original container, no `SourceSpan`
conversion. Synthetic tests do not prove retail compatibility (EvidenceClass
`Designed`); the retail consumers arrive with F05 (rof), F06 (zbd), F07
(interp) and F08 (texture).
