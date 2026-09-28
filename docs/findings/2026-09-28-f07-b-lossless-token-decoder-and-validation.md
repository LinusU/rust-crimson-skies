# F07-B: lossless token decoder and validation

Date: 2026-09-28. Task: F07-B "Implement lossless token decoder and
validation" (`specs/F07-interp-loading-script-container.md`, section
`### F07-B`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
Capabilities used: ordinary build/test only. No `CS_GAME_DIR` read, so no
evidence report is required for this stage (F07-D is the retail stage).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/interp.rs`: `InterpToken`, `InterpLine`,
  `InterpScript`, `DecodedInterp`, `InterpFinding`, `decode_interp`, and the
  private `read_header_and_index`, `index_entries`, `walk_scripts`,
  `walk_script`, `check_arguments`, `decoded_record_bytes`,
  `Walk`/`WalkedScript`/`WalkedLine`. Five new `InterpError` variants
  (`ScriptOffset`, `Unterminated`, `LineOverrun`, `ArgumentCount`,
  `UnterminatedArguments`) with `code()` / `container()` / `offset()` /
  `script()` entries and `Display` arms. `read_interp` was refactored onto
  the shared header/index helpers; its behaviour and its seven
  `accept_f07_a_*` tests are unchanged.
- `crates/cs_formats/src/lib.rs` (wiring only): the new re-exports and one
  module-doc sentence.
- `crates/cs_formats/tests/interp.rs`: the `Image` layout builder, `line`,
  `body`, `decode`, `tokens` and `index_end` helpers and eleven
  `accept_f07_b_*` tests.
- `tools/cs_inspect/src/interp.rs` (new): the `interp` subcommand, its JSON
  report, its exit-code mapping and three `accept_f07_b_*` unit tests.
- `tools/cs_inspect/src/lib.rs`, `src/main.rs`, `Cargo.toml` and the root
  `Cargo.lock` (wiring only): `pub mod interp;`, the subcommand dispatch, the
  help text, and the `cs_formats` dependency edge the new module needs. The
  `Cargo.lock` change is that one edge and nothing else.
- This file.

**Not created in this stage:** `crates/cs_content/src/loading.rs`. The
loading plan, its producer/consumer wiring and the dependency tracing are
F07-C; there is no plan type to normalize yet. Recorded below as a scope note
for F07-C rather than as a missing owner path.

**One observable failure:** a decoder that does not bound a script to its own
extent reads the next script's bytes as its own lines. On a two-script
container whose first script has no terminator, `read_interp` reports the
first script with two lines, the second of which holds the neighbour's data;
`decode_interp` must refuse with
`InterpError::Unterminated { index: 0, offset: <the neighbour's offset>,
limit: <the same> }` and leave the neighbour undecoded
(`accept_f07_b_missing_script_terminator_is_refused`, which asserts both
halves).

## Design decisions

- **Validation is a second entrypoint, not a change to the raw reader.**
  `read_interp` keeps F07-A's contract exactly: it records what is stored and
  does not judge it. `decode_interp` walks the same bytes and adds the rules.
  The acceptance tests use the raw reader as a *control* on the same bytes
  (the swallowed neighbour, the wrong argument count, the unterminated tail
  all parse raw and are refused by the decoder), which is what makes the new
  rules discriminating rather than decorative.
- **A script's extent is the next strictly greater `script_offset`,** computed
  from the set of offsets the index declares — not from index order. An index
  whose entries are not in offset order still bounds every script correctly
  (`accept_f07_b_extents_come_from_the_offsets_not_the_index_order`). Two
  entries sharing one offset both get the next greater offset as their bound,
  so neither can read into the scripts that follow them.
- **A claimed extent ends at the terminator, not at the bound.** The bytes
  between a script's terminator and the next script's start belong to no
  script, so they are reported as `InterpFinding::Unclaimed` rather than
  counted as claimed. `InterpScript::limit()` (the bound it was validated
  against) and `InterpScript::end()` (the byte after its terminator) are both
  exposed so a consumer sees the difference.
- **The argument-count rule has two halves.** The observed behaviour is a
  `0x00` count matching `argument_count` ([S07],
  `docs/research/FORMAT-NOTES.md`). The decoder checks the count *and*
  requires the data to end with a `0x00`, because the end of the last
  argument is stored nowhere else: without it, decoding would have to invent
  a boundary or drop bytes, which is the failure mode non-negotiable #1
  exists to prevent. This is stricter than the observed check alone. If
  F07-D measures retail data that relies on the looser rule, this belongs in a
  finding rather than a refusal — that decision needs the corpus, not a
  guess.
- **Three phases in one attempt**, the shape F05-B established for
  `read_tree`: walk and validate (nothing booked), book every decoded script,
  line and token in one reservation, then build. So a refused container
  leaves the allocation ledger exactly as it found it and the same context can
  retry — checked for each refusal in the tests, and the booking itself is
  pinned: the exact charge fits, one byte less does not, and two decodes need
  twice the charge (`accept_f07_b_decoded_records_are_booked_once_and_refusals_charge_nothing`).
- **Truncation and an unterminated script are different failures.** A
  container that simply stops is a `ParseError`
  (`interp.scripts[0].lines[1].size`); a container that continues but has left
  the script's extent is `InterpError::Unterminated`. Both are refused, with
  the right class for each cause.
- **Unmeasured shapes are findings, not failures.** Unclaimed regions, a
  shared `script_offset`, a 120-byte name field with no `0x00` and non-zero
  name padding are reported on the result. Nothing in the observed format
  says they are errors, and refusing a container over one would be a guess in
  the other direction. A shared offset is a finding rather than a refusal
  specifically because it is also an *origin* question (non-negotiable #2):
  both entries keep their index position, so nothing collapses.
- **No encoding is claimed anywhere.** Names and arguments stay `&[u8]`. The
  `cs-inspect interp` report renders them as `{"length": n, "hex": "..."}`
  rather than as text, because a report that printed them as `str` would be
  asserting an encoding this stage has not measured. The "encoding" check
  `docs/research/FORMAT-NOTES.md` asks for therefore stays unknown, and is
  listed as an unknown below rather than approximated with UTF-8 validation.
- **The decoder classifies nothing.** `InterpLine::head()` returns the first
  token's bytes and says in its documentation that this stage does not decide
  whether that is a command, a verb or a value. No token is registered as an
  opcode, nothing resolves a world and nothing reports a loaded state.

## Tests

`cargo test --workspace --locked -- accept_f07_b_ --include-ignored` — 14
tests, all passing; each also passes alone with `--exact`.

### `crates/cs_formats/tests/interp.rs` (11)

| Test | Covers |
| --- | --- |
| `golden_fixture_decodes_every_token_losslessly` | AC01 on the generated fixture: header, name, extent, both lines, every token's bytes *and* its absolute offset cross-checked against the fixture bytes, re-joining the tokens reproduces each stored data, `findings()` empty |
| `missing_script_terminator_is_refused` | **AC02, first half.** Two scripts, the first with no terminator: the raw reader decodes the neighbour's line (control), the decoder refuses with `Unterminated` at the neighbour's offset and charges nothing; plus the truncation case, refused as `UnexpectedEof` at `interp.scripts[0].lines[1].size` |
| `inconsistent_argument_count_is_refused` | **AC02, second half.** Five disagreements (too many, too few, zero against an empty argument, an embedded `\0`, `u32::MAX`), each refused with both numbers and its data offset; the raw reader accepts all of them |
| `script_extents_are_validated_independently` | a line whose data crosses its extent (`LineOverrun`) and a `script_offset` inside the index (`ScriptOffset`) |
| `extents_come_from_the_offsets_not_the_index_order` | an unsorted index still bounds each script; index order is preserved |
| `unterminated_last_argument_is_refused` | data with no final `0x00` is refused; the raw split yields the unterminated tail (control) |
| `tokens_survive_spaces_empty_and_binary_arguments` | `"a b","c"` ≠ `"a","b c"`, two empty arguments counted as two, a non-UTF-8 argument kept as its bytes, every line's tokens rebuilt losslessly |
| `equal_names_keep_distinct_origins` | **AC03 (origin half).** Two scripts with equal names keep index, entry and script offsets; a shared offset is reported with both entries and both still decode |
| `unclaimed_regions_are_retained_as_findings` | the gap after the index, the gap between two scripts and the tail after the last script, each located in offset order |
| `name_field_anomalies_are_reported_not_guessed` | a 120-byte field with no `0x00` keeps the whole field as the name; non-zero padding is located |
| `decoded_records_are_booked_once_and_refusals_charge_nothing` | the exact charge fits, one byte less is refused with nothing charged, the context retries, and a second decode needs a second charge |

### `tools/cs_inspect/src/interp.rs` (3)

| Test | Covers |
| --- | --- |
| `cli_decodes_a_container_and_reports_its_tokens` | the command end to end: exit 0, the report is well-formed JSON, the header, the script's origin, extent and terminator, both arguments as offsets and hex, the head offset, no findings, and `--raw` adding the F07-A record (also shape-checked) |
| `cli_refuses_a_container_that_fails_validation` | a wrong argument count exits 3 with the code in the diagnostic and no report; a missing `--file` and an unreadable path exit 2; non-INTERP bytes exit 3 with the signature diagnostic |
| `cli_reports_unclaimed_regions_and_exits_nonzero` | a container that decodes but leaves a tail reports the region, writes the report to `--out` and exits 3, so an anomaly is never a clean pass |

### Mutation probes (each reverted)

- `check_arguments`' count comparison disabled →
  `inconsistent_argument_count_is_refused` fails.
- The extent bound replaced by the container length →
  `missing_script_terminator_is_refused` and
  `extents_come_from_the_offsets_not_the_index_order` fail.
- The unclaimed-region computation disabled →
  `unclaimed_regions_are_retained_as_findings` fails.
- A stray `}` restored in the `--raw` report row →
  `cli_decodes_a_container_and_reports_its_tokens` fails on the
  well-formedness check. This one was a real defect found by running the
  binary on a fixture: the first `--raw` report closed each line's object
  before appending the raw view, so it was not parseable JSON while every
  substring assertion still passed. The report is now shape-checked, and
  `cs-inspect interp --file fixtures/synthetic/synthetic.interp --raw`
  parses as JSON.

## Commands run

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | pass, no failures |
| `cargo test --workspace --locked -- accept_f07_b_ --include-ignored` | 14 tests, all pass |
| each of the 14 with `--exact` | all pass alone |
| `cargo run -p cs_inspect --bin cs-inspect -- interp --file <authored container with a tail>` | exit 3 with the unclaimed-region report, as designed |
| `cargo run -p cs_inspect --bin cs-inspect -- interp --file fixtures/synthetic/synthetic.interp --raw` | exit 0, report parses as JSON |

## Unknowns (not guessed)

- **Whether retail `interp.zbd` matches this layout at all** is still
  unmeasured; the layout is from the pinned mech3ax source [S07] and the
  authored fixture. F07-D measures it with the real file.
- **Whether retail data ends every line with a `0x00`.** The decoder refuses
  data that does not, which is stricter than the observed NUL-count check. If
  the corpus shows trailing argument bytes without a delimiter, this rule has
  to move to a finding; that is F07-D's call, on evidence.
- **Whether retail scripts share a `script_offset`, overlap, or leave
  unreferenced regions.** Reported as findings rather than refused, so no
  retail data is lost; F07-D decides whether any of them is an error.
- **Whether name padding is always zero and whether a name field is always
  `0x00`-terminated.** Both are reported as findings; nothing reads padding as
  part of a name.
- **The encoding of names and arguments.** Unchanged from F07-A and still
  unknown: no `String` is built and no UTF-8 check is applied. The
  `docs/research/FORMAT-NOTES.md` request to validate encoding cannot be
  satisfied without knowing the encoding.
- **The meaning of the `timestamp` word** (units, epoch). Still exposed only
  as `raw_timestamp`, and the report labels it `timestamp_metadata_only`
  because non-negotiable #5 says it is never a cache identifier.
- **Whether anything after the zero terminator word belongs to the script.**
  F07-A recorded this as unknown; the decoder still reads the terminator as
  one word and reports whatever follows as unclaimed bytes.

Every one of these was already the scope of F07-D, so no new task was filed.
F07-C inherits the decoded container as its input: `DecodedInterp` is what the
loading plan will normalize, and nothing else in this stage touches
`crates/cs_content/`.

## Sources

`specs/F07-interp-loading-script-container.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`,
`docs/research/FORMAT-NOTES.md` ("INTERP observed subset" [S07]),
`docs/research/SOURCES.md` (S07), `fixtures/synthetic/README.md`,
`fixtures/synthetic/expected.json`, `tools/make_synthetic_fixtures.py`, and
`docs/findings/2026-09-28-f07-a-interp-raw-records-and-golden-fixture.md`
(which listed exactly this stage's outstanding rules).
