# F04-D: reader members in the VFS — basename keys and the mount order

Date: 2026-10-06. Task #685 `F04-D-order-reader-members`, the first of the
three follow-ups section G of
`docs/findings/2026-10-05-f04-d-original-lookup-order.md` filed (#685 reader
members, #686 ROF before loose for GOS requests, #687 mission level, gamez /
planes / anim bindings and the texture rule). Reviewed and corrected by
`bunny-2/bunny-2`; see section G for what the review changed.

## Provenance

The order this task implements is the one the owner's static analysis of the
pinned executable recorded (finding sections A and E), and the five cases it
decides were re-measured on this installation by
`accept_f04_d_original_order_retail_mission_shadows_world_in_five_cases`
(#341). What this task adds is the **production mount and lookup** that was
missing, and its own retail read of the archives through that production path.
Nothing here is a new observation of the original: **the order is
code-derived, not a runtime capture**, and nothing here is
`verified_original`.

## A. What was missing

| Original (sections A, E) | VFS before this task |
| --- | --- |
| requested name reduced to its **basename** | members indexed by their full container-relative path (`Mount::member`) |
| archives searched in mount order **[root, mission, world]** | no precedence expresses a root-first order (`PrecedenceClass` ranks mission/world **above** shared) |
| **first** archive holding the name serves it | `Vfs::resolve` would answer `Ambiguous` or `UnmeasuredOrder` between two archives holding one name |
| **first** index entry inside one archive | `MountBuilder::add_member` **refuses** a duplicate key, so the root archive's two `player.zrd` entries could not be mounted at all |
| nothing mounted a `zrdr.zbd` reader archive | `read_reader_archive` existed; no code turned its members into mounts |

So reader member collisions were not merely undecided: they were
**unrepresentable**. `MountError::DuplicateMember` is the right rule for two
sources competing at equal precedence (spec F04 non-negotiable behavior 3),
but the original's own first-hit scan inside **one** archive is a different
rule, and applying the equal-priority rule there refuses a container the
original reads.

## B. What `cs_assets::vfs::reader` does now

`mount_reader_archive(builder, path, level)` reads one `zrdr.zbd` through the
production reader chain (`dispatch` → `read_version_one_index` →
`read_reader_archive`), so the family gate, the index and the bounds check are
the ones every other ZBD container goes through. Then:

* every declared entry becomes a [`ReaderMember`] row — offset, length,
  SHA-256 of exactly those bytes where the listing could hand them out —
  **including** entries no name can reach, with the reason recorded
  ([`Unreachable`]: `failed_bounds`, `non_utf8_name`, `invalid_name`,
  `duplicate_name`). The refusals are ordered by how much they depend on: the
  bounds check first (an entry with no bytes has no name to serve), then the
  spelling, then the first-hit rule;
* an entry a name **can** reach also becomes a mount member, spelled with the
  name the archive declares, keyed by its case-folded form — which is the
  **basename keying** the original does, because the member's name is a bare
  file name;
* `ReaderMounts::resolve(context, key)` reduces the requested path to its
  **basename**, skips archives the context's world/mission/locale/mod does not
  admit, then searches the rest in the original's mount order —
  [`ReaderLevel::rank`]: **root (2), mission (1), world (0)** — and the first
  archive that holds the name serves it. Archives searched after the winner
  are still reported, as `shadowed(entry_index)`, so "the world copy was there
  and the mission copy won" is a fact a diagnostic can read;
* inside the winning archive the **first** index entry whose name matches
  case-insensitively serves it; a later entry of the same name is a row with
  `duplicate_name`, never a mount member and never dropped;
* `ReaderArchive::read` / `ReaderMounts::read` slice the member's extent out
  of the bytes the archive was built from and check it against the digest
  recorded at mount time. The mount has no directory backing, so
  `Vfs::read_all` on a reader member reports `no backing` rather than
  returning container bytes as if they were host files — the same split a ROF
  mount has.

The mount order is used **instead of** insertion order deliberately: the test
adds the archives world-first and still gets the root answer.

## C. Why this is not `Vfs::resolve`

The designed precedence classes rank a mission- or world-specific source
*above* a shared one; the original's reader order is the other way round —
the root archive shadows both — and it decides by **mount order**, not by
rank. No `PrecedenceClass` value expresses "root first", and inventing one
would silently rewrite the designed baseline every other namespace relies on.

So the reader lookup is its own path with its own trace
([`ReaderTrace`], `attempts` + `order_status`) and its own status constant
[`READER_LOOKUP_ORDER_STATUS`]. That constant is
`ClaimStatus::Inferred`: the order is read off the original executable's code,
which is real evidence, but it was not observed in a run of the original, so
it is never `verified_original`. `PRECEDENCE_ORDER_STATUS` is **not**
consulted by a reader lookup and stays `designed`
(`accept_f04_d_order_reader_the_designed_precedence_status_is_untouched`).

A reader mount is refused outside the `reader` namespace
(`foreign_namespace`), because a lookup that only searches that key space
could never serve a mount elsewhere — better a refusal than a silently
unreachable mount. A request that **specializes a variant** is refused the same
way (`foreign_variant`): a reader archive declares one copy of each name and
indexes it at `AssetVariant::default`, so answering with that copy would return
bytes the caller did not ask for, and the retail variant vocabulary is unknown
(`AssetVariant::default` is an engine label). That refusal is the review's; see
section G.

## D. What retail data says, through the new production path

Both retail tests are `#[ignore = "requires CS_GAME_DIR"]` and mount the real
archives through `mount_reader_archive`; neither reads the member set with a
test-only parser.

**Mission shadows world in exactly five cases** — the same five
`accept_f04_d_order_reader_retail_mission_shadows_world_in_five_cases`
measures, now resolved through `ReaderMounts::resolve`:

| mission archive | member | level served |
| --- | --- | --- |
| `ZBD/C1C/IA1/zrdr.zbd` | `targets.zrd` | mission |
| `ZBD/C1C/MP1/zrdr.zbd` | `targets.zrd` | mission |
| `ZBD/C1C/MP3/zrdr.zbd` | `targets.zrd` | mission |
| `ZBD/C2/M01/zrdr.zbd` | `security_destroy.zrd` | mission |
| `ZBD/C3/M02/zrdr.zbd` | `fueltruck.zrd` | mission |

Each case asserts the served digest equals the **mission** copy's, differs
from the **world** copy's, and that the world archive is reported `shadowed`
— so the order, not an absent member, is what decided. Mounting the world
archive alone serves the world copy, which is what makes this shadowing.

**The root `player.zrd` duplicate** (finding section F), resolved through the
same path: entries **22** and **100** both stay rows; entry 22 (3414 B,
sha256 `a8cc7547…`) is what `player.zrd` serves, entry 100 (34711 B) is
`duplicate_name { served_by: 22 }`, and the read-back of entry 22's bytes
hashes to the span the resolution named. The root archive declares **221**
entries, mounts **220** members and has exactly **one** unreachable entry.

**Root-first is not exercised by retail data.** No world or mission member
name occurs in the root archive at all (finding section F, re-checked here),
so there is no retail case where the root copy shadows another one. That half
of the order is therefore **code-derived only** — pinned by the synthetic test
`accept_f04_d_order_reader_root_archive_shadows_mission_and_world`, which is
the honest state of the evidence, not a claim of a measured retail shadowing.

## E. What stays unknown

Unchanged by this task, and unchanged by the new code. Each item names the
affected content and the task that owns it, and each is recorded in the
machine-readable report's `unknowns` array, **not** dropped:

* the **loose-file override** (section A: a loose file of the same basename
  that is newer wins over the archive copy) — not modelled; the finding also
  records that the candidate for the comparison, the index entry's trailing
  `u64`, is never observed being read. Affects every reader member.
  Owner: **#700** `F04-D-order-reader-loose` (filed by this review; §A's two
  rules had no task), with **#692** for the timestamp;
* the **loose directory fallback** (only when no archive holds the name) —
  not modelled. Affects reader lookups that find no archive copy. Owner:
  **#700**. No such loose directory exists in retail, so no test can exercise
  it from the installation;
* what the index entry's `u32 word` (`2` in all 1293 entries) and its
  trailing `u64` mean — still `Unknown` in `cs_formats`, untouched. Owner:
  **#692**;
* whether the world/mission **archive** set is exactly the campaign layout
  this installation has, and the **mission level** itself:
  `SessionBuilder::mount_installation` still binds no mission, so which
  archives are mounted for a mission is the caller's decision. Owner: **#687**;
* the order itself is code-derived only: no run of the original observed it,
  so `READER_LOOKUP_ORDER_STATUS` is `Inferred` and `PRECEDENCE_ORDER_STATUS`
  stays `Designed`. Settling it against a run needs owner-supplied capture
  (**#358**) or further static work.

**No retail archive reaches a `ReaderMountError`.** All 62 `zrdr.zbd` declare
1293 entries and every one passes dispatch, the index read, the bounds check
and the spelling check (reviewer re-measured over `$CS_GAME_DIR`: 1293
entries, no name containing a path separator, no non-UTF-8 name, no extent
outside its container, `word == 2` in all of them). Every mount refusal is
therefore a guard exercised by synthetic archives, which the module now says
in its own documentation.

## F. Tests

`crates/cs_assets/tests/accept_f04_d_order_reader_members.rs`, prefix
`accept_f04_d_order_reader_`.

Synthetic (CI runs them): root-first order under a world-first registration
order; mission over world when the root misses; basename reduction of a
requested spelling with directories; case-insensitive first-entry matching and
the duplicate row; a name no key can spell archived as `invalid_name`; an
extent outside the archive and a non-UTF-8 name archived as `failed_bounds` /
`non_utf8_name` without hiding their siblings; a scope-mismatched archive
reported `skipped` with the next archive serving, and the same mount list
answering differently under the admitted context; an archive of another world
never searched; `not_found` listing every archive it searched; a mount outside
the reader namespace refused; an empty archive; a foreign / stale / moved-span
read refused; a row that does not describe the archive's bytes refused by the
read guard; a key of another namespace refused; a request that specializes a
variant refused while the plain request is still served; two archives sharing one
mount id refused by the read path; and the designed precedence status untouched.

Sensitivity was checked by mutation, not asserted: reversing
`ReaderLevel::rank` fails three tests; dropping the basename reduction fails
the basename test; ignoring `MountScope::admit` fails the two scope tests;
short-circuiting the digest check fails the read-guard test; and clearing
`Unreachable::FailedBounds` fails the unreadable-entry test. A `.rev()` in
`ReaderArchive::member` does **not** fail anything, because the duplicate entry
is already marked unreachable before the scan — which is the intended
structure, not a gap.

Retail (`--include-ignored`, needs `CS_GAME_DIR`): the five shadowing cases
with their digests, and the root `player.zrd` duplicate with both entries,
their lengths, their digests and the read-back.

`evidence_report_t685_writes_the_acceptance_report` is the evidence harness
(`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test: it re-mounts all
62 declared `zrdr.zbd` through production code and writes
`private/evidence/T685/reader-lookup.json` (installation spellings, member
counts, digests and the five shadowing cases) next to the recorded test log,
then `acceptance.json` for `tools/validate_evidence.py`. The committed copy is
`docs/findings/evidence/T685.json`.

It is validated **without** `--require-pass`. That flag rejects a report whose
`unknowns` is non-empty, and section E's limitations are exactly what this stage
left open, so the report lists them (with the affected content and the resolving
task) and the flag exits 3 with "Unresolved issues" — the expected result. The
first version of this report declared `"unknowns": []` and was validated with
`--require-pass`; that was an empty list produced to satisfy the validator, which
the owner directive of 2026-09-28 forbids, and this review replaced it.

## G. What the review changed

Implementer `bunny-2/bunny-2`; reviewer `bunny-2/bunny-2` — **the same agent
instance, and its context was not fresh** (the implementing session wrote
`reader.rs`, the suite and this file). It is therefore *not* independent
evidence of anything about the original; the owner policy asks for a different
agent or model for format and mission semantics, and this is that kind of work.

The retail measurements were nonetheless re-derived independently from
`$CS_GAME_DIR` (a separate parser over the raw index trailers, not the
production code under test) and agree with the pinned claims: 62 archives, 1293
entries, root 221 declared / 220 mountable, `player.zrd` at entries 22 (3414 B,
sha256 `a8cc7547…`) and 100 (34711 B, `f9bd5306…`), the five mission-over-world
cases with differing digests, no name with a path separator, no non-UTF-8 name,
no extent outside its container and `word == 2` in every entry.

Two production defects were found and fixed:

1. **A panic on a non-default `AssetKey` variant.** `ReaderMounts::resolve`
   built its mount lookup key with the *requested* variant, while
   `mount_reader_archive` indexes every member at `AssetVariant::default`, so
   the `.expect("the selected archive holds the key…")` after the scan panicked
   for any legal key carrying a variant (reproduced with
   `AssetKey::from_spelling("reader", "targets.zrd", "night")`). The lookup now
   refuses such a request with `ReaderLookupError::ForeignVariant` before
   searching, rather than serving the default copy's bytes as an answer to a
   different question. Pinned by
   `accept_f04_d_order_reader_a_request_that_specializes_a_variant_is_refused`.
2. **An ambiguous mount id was resolved by taking the first match.**
   `ReaderMounts::read` finds its archive by mount id, so two archives
   registered under one id made it return one archive's bytes under another's
   provenance. It now refuses with `ReaderReadError::AmbiguousArchive`
   (`archives_named` makes the ambiguity visible). Pinned by
   `accept_f04_d_order_reader_two_archives_under_one_mount_id_are_refused`.

Also: the module documents that no retail archive reaches a `ReaderMountError`,
and the report's `unknowns` are the five items of section E instead of an empty
list. **#700** was filed for the loose-file override and the loose-directory
fallback, which section A of #341's finding described and no task owned.

Still worth a fresh, independent reviewer's eye: whether refusing a specialized
variant is the right boundary rather than serving the default copy, and whether
a reader archive with no production caller yet (the acceptance suite and the
evidence harness are its only consumers today) should be reachable from
`cs-inspect` before it is used by a mission.