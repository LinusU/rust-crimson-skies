# F04-D: the reader's two loose rules — one modelled, one refused

Date: 2026-10-06. Task #700 `F04-D-order-reader-loose`, the follow-up #341's
finding left open and #685 named as a limitation
(`docs/findings/2026-10-06-f04-d-reader-member-lookup.md` section E, items 1
and 2).

## Provenance

The two rules are the ones the owner's static analysis of the pinned executable
recorded: `docs/findings/2026-10-05-f04-d-original-lookup-order.md` section A,
reader open `0x579c60`, loose pass `0x579710` → `0x59d170`, directory list built
at startup `0x4a6ff0` and on world/mission load `0x463cb0`.

This task adds **no** new observation of the original. What it adds is a
decision about each rule, production code for the one that can be implemented
from what is known, a refusal for the one that cannot, and a measurement of
what this installation can say about either. **Both orders remain
code-derived**; nothing here is `verified_original`, `READER_LOOKUP_ORDER_STATUS`
and the new `READER_LOOSE_ORDER_STATUS` stay `inferred`,
`PRECEDENCE_ORDER_STATUS` stays `designed`.

## A. The decision, rule by rule

| original (section A) | decision | why |
| --- | --- | --- |
| **loose directory fallback**: only when *no* archive holds the name, the loose pass searches the most recently added directory first, then `zbd` | **modelled** | it is an ordering rule, not a comparison; everything it needs is the directory list the finding already gives, plus the host filesystem |
| **loose-file override**: a loose file of the same basename that is newer (`CompareFileTime >= 1`) overrides the archive copy | **refused, not modelled** | deciding it needs a timestamp on the *archive* side, and the only candidate the bytes offer — the index entry's trailing `u64` — is unknown (#692, finding section F). Comparing it against the loose file's host modification time would be inventing the rule's argument. |

So the affected content of the second decision is every reader member a loose
file of the same basename could shadow. It is a **new collision class** — a loose
file and an archive member are different origins of the same basename — and it
is handled as one: `ReaderResolution::origin` says which of the two served a
lookup ([`ReaderOrigin`]), and a lookup that finds both is **refused**
(`ReaderLookupError::LooseOverrideUndecided`) rather than answered. The
refusal is raised whether the loose file is newer or older; two tests differ
only in the loose file's host modification time (now, and 1980) and both must
refuse, because reading the host clock would answer one and refuse the other.

## B. What `cs_assets::vfs::reader` does now

* `original_loose_reader_directories(world, mission)` builds the original's
  list **in the order the executable appends it**: `zbd` and
  `data/common/zrdr` at startup, then `data/common`, `data/<w>`,
  `data/<w>/nets` and `data/<w>/<m>` on a world/mission load.
* `ReaderMounts::add_loose_directory` (or `add_original_loose_directories`)
  registers them in that **append** order; `loose_search_order()` is the
  original's **search** order, most recently added first and `zbd` last, and
  `resolve` uses that.
* `resolve` runs the loose pass **only when no mounted archive the context admits
  holds the basename**. The first **regular** file of the basename in the search
  order serves it, its bytes are read and hashed, and the resolution's
  `SourceSpan` names the file itself (no member key, offset 0, whole file) —
  the same convention `SourceSpan` already documents for a container that is
  itself the source.
* Every registered directory is probed and reported in the trace, in search
  order, with one of: `absent` (the directory does not exist — the original
  counts only existing directories), `miss` (it exists and holds no such file),
  `not_a_directory` (the **declared directory** is not one, or sits below
  something that is not one), `not_regular` (the entry of that name *inside* a
  real directory is a link or not a regular file), `unreadable` (an I/O failure
  after a candidate was already found), `selected`, `shadowed`,
  `undecidable_shadow`.
* **A probe failure refuses the lookup only while no candidate exists.** Until
  the pass holds the file the original would open, the answer still depends on
  the directory being probed, so an I/O failure there is
  `ReaderLookupError::LooseUnreadable`. Once a candidate exists the answer no
  longer depends on any lower-priority directory, so the failure is recorded as
  `unreadable` and the search continues: an I/O error in a directory that holds
  nothing must not un-decide a lookup the original decides from the file already
  found. The candidate's **own** bytes are still read under that rule, so a
  candidate that cannot be read coherently refuses rather than being reported.
* `original_loose_reader_directories(None, Some(mission))` is **refused**
  (`ReaderLooseError::MissionWithoutWorld`), not silently answered with the
  startup list: the original spells a mission directory below its world
  directory, so dropping it would answer a different lookup than the one asked.
* `ReaderMounts::read` re-reads a loose resolution from the host and checks it
  against the digest the resolution recorded (`loose_digest_mismatch`,
  `unknown_loose_directory`, `stale_resolution`, `loose_origin`). A loose file
  is never read through an archive, and an archive member is never read through
  a loose directory.
* **Case is not folded.** `original_loose_reader_directories` uses the spelling
  it is given. The finding spells the loose paths in lower case, this
  installation's `ZBD` directories are upper case, and a case-sensitive host
  makes `data/c1c` and `data/C1C` different directories. The retail tests
  therefore measure **both** spellings rather than pick one.

Two engine guards are deliberate and labelled as such: a symbolic link is never
followed (the guard `mount_directory` already applies) and a non-regular entry
is never served. Neither is claimed to be the original's behaviour; the
original's Windows host would have opened and compared the linked file.

## C. What retail data says, through the production path

Both retail tests are `#[ignore = "requires CS_GAME_DIR"]` and register the
original's directories through `add_original_loose_directories`.

**No loose reader file exists in this installation.** Measured over the
installation's declared reader member names (442 distinct, case-folded) and
every registered directory of every declared world group and mission, in both
spellings: **0** of the `data/...` directories exists and the default `zbd`
directory holds no loose reader file of any declared name — it holds the
archives. 636 directory spellings were probed.

**Registering the loose directories changes no retail answer.** With them
registered, all **221** root archive members resolve and every one is served by
the archive (`ReaderOrigin::Archive`); no lookup reports `selected` or
`undecidable_shadow`.

That is why **retail data cannot exercise either rule**, and the measurement is
what makes the modelled fallback's status `inferred`-only rather than anything
stronger: the fallback is code-derived and synthetic-pinned, not observed here.

## D. What stays unknown

Recorded in the evidence report's `unknowns` array, **not** dropped
(`docs/findings/evidence/T700.json` is validated **without** `--require-pass`),
each naming the affected content and what would resolve it. Seven items:

1. the loose-file override is not modelled and the lookup that would need it is
   refused — the archive-side timestamp is unknown (**#692**);
2. *which* loose file the original compares is not pinned by the finding (the
   loose pass is described as running only when no archive holds the name, while
   the override needs a loose file): this task records the most recently added
   regular file as the candidate and refuses — further static work or an
   owner capture (**#358**);
3. loose **file** names match case-exactly here while the original's Windows
   filesystem was case-insensitive, and the loose **directory** spellings keep
   the caller's case — needs a host whose loose directories exist;
4. the never-followed-link and never-serve-a-non-regular-entry guards are this
   engine's decisions, not measured original behaviour;
5. when a loose directory cannot be probed decides the lookup and when it does
   not (`LooseUnreadable` before any candidate, the `unreadable` outcome after
   one), and the split between `not_a_directory` and `not_regular` — these are
   **this engine's** refusal rules, not claims about the original, which has no
   measured behaviour here;
6. the loose directories belong to a context by the caller's declaration only —
   they carry no `MountScope`, so the mission binding stays with **#687**;
7. both orders are code-derived: `READER_LOOSE_ORDER_STATUS` is `inferred`,
   `READER_LOOSE_OVERRIDE_STATUS` is `unknown`, `PRECEDENCE_ORDER_STATUS` stays
   `designed`. Settling them needs owner-supplied capture (**#358**).

**The unmodelled override gates any claim that reader resolution reproduces the
original.** A resolution served from an archive is the original's answer only
when no loose file of that basename exists; where one does, production says so
by refusing rather than guessing, and no consumer may read the refused lookup as
a modelled outcome.

## E. Tests

`crates/cs_assets/tests/accept_f04_d_order_reader_loose.rs`, prefix
`accept_f04_d_order_reader_loose_`.

Synthetic (CI runs them): the fallback serving a name no archive holds, with the
span, origin and read-back pinned; the most recently added directory winning over
five others of the same name, with the search order shown to be the reverse of
the append order; the fallback skipped when an archive holds the name (and the
same set still serving the name no archive holds); an archive member and a loose
file of the same basename refusing the lookup, with the refusal naming the
candidate, the archive and the comparison it cannot make; the override refused
identically for a far newer and a far older loose file; the two order statuses
and the untouched designed status; the original's directory list in its append
order for the startup, world and mission cases, and a world-group spelling
refused; `not_found` listing every directory and what each held; a non-regular
entry reported and never served (and never shadowing an archive member); a
symbolic link never followed; and a changed file, a stale extent, an
unregistered directory and a loose origin read through an archive all refused.

Also from review (2026-10-06, `bunny-alpha-1`, four defects the implementer's
suite did not reach): a declared loose directory that is not one — or sits below
something that is not one — reported apart from a non-regular entry, and a real
file elsewhere still served; an I/O failure in a directory the answer no longer
depends on recorded rather than refused, and the same failure **before** any
candidate refusing; a mission named without a world refused with
`MissionWithoutWorld`, registering nothing; and a broken sibling directory in the
test tree not being able to make an intact directory unservable.

Retail (`--include-ignored`, needs `CS_GAME_DIR`): no registered loose directory
holds any declared member name, in both spellings, with only `zbd` existing; and
every root archive member still resolving from the archive with the loose
directories registered, in both spellings.

Sensitivity was checked by mutation rather than asserted. Reversing the loose
search order to append order fails five tests (the most-recently-added one, both
override-refusal ones and two trace-order ones). Removing the loose pass
altogether fails eight. Removing the `is_symlink` guard from the file probe fails
the link test. Letting the loose fallback win over an archive member ("the loose
file always wins") fails the two override-refusal tests, and suppressing the
override refusal so the archive member is served anyway fails the same two. So a
suite that passed with the fallback removed, with the order reversed, with the
link guard gone, or with either wrong copy served would be a false green.

Each mutation was reverted and the suite re-run green afterwards.

## F. Evidence

`evidence_report_t700_writes_the_acceptance_report` is the evidence harness
(`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test. It re-derives the
survey through production code (`original_loose_reader_directories`,
`add_original_loose_directories`, `mount_reader_archive`, `ReaderMounts::resolve`)
and writes `private/evidence/T700/reader-loose.json` (installation spellings,
directory existence and counts only) plus `acceptance.json`. The committed copy
is `docs/findings/evidence/T700.json`.

Validated **without** `--require-pass`: the flag rejects a report whose
`unknowns` is non-empty, and section D is exactly what this stage left open. With
the flag the validator exits 3 with "Unresolved issues" — the expected result. An
empty `unknowns` would be a report claiming this stage closed questions it did
not.

## G. Review

Implementer `bunny-alpha-1/bunny-alpha-1` (Rally #700 implement claim). Reviewer
`bunny-alpha-1/bunny-alpha-1` (Rally #700 review claim, 2026-10-06) — the **same
agent instance** as the implementer, so this is **not independent review** and
must not be read as such. It found and fixed four real defects (section E) that
the implementer's suite did not reach; the ownership decision, the evidence
report and the limitation list are unchanged and were verified rather than
rewritten.

The owner policy asks for a different agent instance or model for format and
mission semantics, and this is that kind of work. Nothing here claims otherwise:
no original run, no owner capture, no `verified_original`.