# F38-D.1: UI host-call families re-keyed by native, three hosts bound from owner static evidence

Date: 2026-10-14. Task: **F38-D.1** "Re-key UI host-call families by native and
bind goslocalize, gospageinit and mail from owner static evidence" (Rally #694,
split from F38-D #159). Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
Capabilities used: **`retail`** (read-only `$CS_GAME_DIR`) and ordinary
build/test. Evidence report: `private/evidence/F38-D.1/acceptance.json`,
committed as `docs/findings/evidence/F38-D.1.json`.

## Source and provenance

All semantic claims below come from the **owner note on Rally #159**
(2026-10-05, "OWNER UPDATE"), itself static analysis of the owner-supplied
decrypted executable (`crimson.decrypted.exe`, sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, file offset
= VA − 0x400000 below 0x643000; script names from
`GOSDATA/ASSETS/crimson.rof` `ASSETS/SCRIPTS/DEBUGINFO.TXT`, which the game
loads at `0x406e40`). Claim class: **static code evidence, never
`verified_original`**. No original run was observed; no code, bytes or original
script text are committed here — the census below is counts, ids and spellings
derived from production measurement.

## Files (owner paths only) and the one observable failure

- `crates/cs_formats/src/script_raw/ui_host_calls.rs`: the measured native
  table `UiNative` (+ wire codes, DebugInfo labels, `$$X$$` tokens, handler
  addresses), `HostKind` (family key host: native / mail / unmeasured),
  `HostCallSite::native` resolved by the scanner, and the corpus re-keyed to
  **(host, dispatch value)** including computed-dispatch (`None`-id) families.
- `crates/cs_formats/src/script_raw/mod.rs`: re-exports (wiring only).
- `crates/cs_script/src/bindings/observed.rs`: `MeasuredNative`, `MeasuredHost`,
  protocol-modelled `MeasuredCall` (observed arities + per-position variation
  kept as `None`), `MeasuredMeaning`, the reworked `classify` that binds
  goslocalize / gospageinit / `mail` and refuses everything else with its
  native and handler address, and the (host, value) family table.
- `crates/cs_script/src/bindings/located.rs`: `SiteRow.host`, the re-keyed
  `judge`, `SiteVerdict::Refused { spelling, reason }`, and
  `SiteAudit::unmeasured_families()` — the audit names each unmeasured family
  by native and id.
- `crates/cs_script/src/bindings/mod.rs`: crossing-table doc update (wiring
  only).
- Tests: `accept_f38_d1_native_rekey.rs` (4 synthetic),
  `accept_f38_d1_retail_native_census.rs` (2 retail), plus the re-keyed
  `accept_f38_b_*`, `accept_f38_c_*` suites and their evidence harnesses.
- This file and `docs/findings/evidence/F38-D.1.json`.

**Observable failure (task AC):** before this task the family key was
`(dispatch form, integer id)`: `goscallback#6` and `uicontrol#6` merged into
one `callback#6` family, and F38-C's audit refused 994 retail sites as
`no_family` because their family's own sites "disagreed" on arity or argument
shape. Now the key keeps the native, variation is modelled as protocol, and
those 994 sites are judged (bound or refused) against the protocol their own
corpus measured.

## The measured native table (owner note, Rally #159)

`callback($$X$$, <id>, …)` calls the native the DebugInfo map names for `$$X$$`:

| `$$X$$` | DebugInfo name | handler | meaning measured? |
| --- | --- | --- | --- |
| `$$A$$` | goscallback | `0x407670` | no — per-id meanings open on #159 |
| `$$E$$` | uidata | `0x4093a0` | no — per-id meanings open on #159 |
| `$$UB$$` | uicontrol | `0x404960` | no — per-id meanings open on #159 |
| `$$JI$$` | gospageinit | `0x403680` | **yes** — first argument is the page record (`0x404940` looks it up); no integer id |
| `$$NB$$` | goslocalize | `0x404630` | **yes** — loads string `<id>` from the resource module set by `setresource` (langui.dll) (`0x4050c0`→`0x4050e0`, written by `0x403350`) |

`mail(<message>, <recipient>)` is not a native call: it is GOS
script-to-script messaging delivered to the recipient script's `gui_mailbox`
`getmessage()` switch (owner note §4), so its "host" is the message bus itself
and its meaning is the receiving script's mailbox case.

A site whose `$$X$$` token is outside this table is keyed `unmeasured`, counted
and refused with `HostNotMeasured`; it is never folded into a native family.

## The protocol model

The natives take a mode/index argument, so one (host, value) legitimately
appears with several arities and argument shapes (owner note §3). The family is
therefore a **protocol**: `MeasuredCall` keeps the observed arity *set* and a
per-position shape where `None` marks variation, and `judge` accepts a site
whose count is in the observed set and whose shapes fit the uniform positions.
Nothing resolves a disagreement by picking one reading. `RowError` no longer
has the `DisagreeingArity` / `DisagreeingArgumentShape` refusals — they encoded
the fixed-signature model the owner note corrected — and the `no_native_id`
verdict is gone: a computed dispatch belongs to its host's computed-dispatch
family instead of having no family at all. One consistency refusal remains
(protocol review of this task): a row whose shape table is narrower than its
own widest observed arity is refused (`RowError::ProtocolWidthMismatch`),
because the audit checks a site's positions against that table and a shorter
table would silently leave a wide site's trailing positions unchecked; the
producer always grows the table to the widest site it measured, so no
corpus row hits it.

## Re-keyed retail census (`$CS_GAME_DIR`, read-only, production measurement)

Per native (sites, distinct integer ids, computed-dispatch sites) — the owner
census of #159 re-measured through the production scanner, exact match:

| Native | sites | distinct integer ids | computed-dispatch sites |
| --- | --- | --- | --- |
| goscallback | 111 | 57 | 1 |
| uidata | 369 | 169 | 12 |
| goslocalize | 174 | 143 | 10 |
| uicontrol | 44 | 16 | 0 |
| gospageinit | 11 | 0 | 11 |
| **sum** | **709** | **385** | **34** |

Totals and the audit verdicts:

| Count | Value |
| --- | --- |
| UI script programs | 61 |
| sites total | 1812 |
| callback sites attributable to the five natives | **709** |
| distinct (native, id) pairs | **385** (id 6 under two natives) |
| mail sites / mail families | 1102 / 91 |
| unmeasured-target sites / families | **1** / 1 |
| families total | 481 (389 native + 91 mail + 1 unmeasured) |
| bound families / sites | 236 / **1287** (174 goslocalize + 11 gospageinit + 1102 mail) |
| refused sites | **525** (524 goscallback/uidata/uicontrol + 1 unmeasured-target) |
| unmeasured families (named by native + id) | 245 (58 goscallback + 170 uidata + 16 uicontrol + 1 unmeasured) |
| `no_family` / `shape_mismatch` / `arity_mismatch` | 0 / 0 / 0 |
| unjudged call-shaped heads | 3188 |
| `campaign_ready` | **false** |

**Census surprise, pinned not smoothed over:** the owner census's 709 callback
sites are exactly the sites attributable to the five measured natives; the
corpus contains **one further callback site** whose `$$X$$` token is outside
the table (MULTIPLAYERGAMESLIST.SCRIPT, computed dispatch, no arguments). It
keeps its own `unmeasured#computed` family and refuses; it is visible in every
count above rather than being attributed to a native by guess.

## Why F38-C's 994 `no_family` sites were not a corpus inconsistency

F38-C keyed families by `(form, id)` and refused any family whose sites
disagreed on arity or an argument's shape; 994 retail sites therefore had *no*
family. Those disagreements are the protocol: goslocalize/uidata/goscallback
ids are called with different mode arguments and optional output targets, so
the same id legitimately spells 0…n arguments and different shapes at a
position. Under the (host, value) protocol model every one of those sites has a
family and a verdict: 1287 bind (the measured hosts), 525 refuse (the
unmeasured natives and the unmeasured target), 0 are dropped.

## What this does not establish (unresolved, not removed)

1. **The per-value meanings of 57 goscallback, 169 uidata and 16 uicontrol
   ids** (245 families with the computed-dispatch and unmeasured-target
   families) are still unmeasured; each resolves to a known handler through
   the dispatch tables in the #159 note and is measurable statically, but no
   session has measured them. While any of them is reachable,
   `campaign_ready` stays false and `SiteAudit::unmeasured_families()` names
   each one. Resolving task: the F38-D parent (#159) / further owner-side
   static analysis.
2. **One callback site has no measured native** (the census surprise above);
   its `$$X$$` token is outside the five-native table and its meaning is
   unknown. Resolving task: #159.
3. **3188 call-shaped heads remain unjudged** (F38-B's measured boundary):
   `initialize`, `getmessage`, `script_run` and siblings are counted, not
   judged, and keep the gate closed. Resolving task: #159.
4. **The mission language is not decoded** (F13-D): nothing here covers a
   mission instruction or a mission native call; these are the UI script
   programs only.
5. **The differential trace (`bindings::differential`) still identifies a
   declared step by (form, value)**, not (host, value): a scenario that
   declared the same id under two natives would compare them as one call
   identity. The family table and audit are re-keyed; the differential's
   identity is filed as follow-up task **#1411**, outside this slice's
   acceptance criteria.
6. **Binding here is a measured meaning, not a runtime operation.** A bound UI
   family is deliberately **not** registered in the mission
   `HostBindingRegistry` (still empty): the mission IR has no UI operation and
   a mission program referencing a UI host call must still fail validation
   before flight (AC01). Executing these natives is UI-host work that does not
   exist yet.
7. The producer→consumer crossing is still written out in tests and harnesses
   (no crate may depend on both sides); a production home for it is outside
   this task's owner paths, as F38-B/C recorded.

## Tests (`accept_f38_d1_*`)

Synthetic: the family key keeps the native (id 6 under `$$A$$`/`$$UB$$` is two
families, spellings differ, each site judged by its own native; a true
duplicate is still refused); the measured hosts bind with their meanings
(including computed ids) and a all-measured corpus is `campaign_ready` —
proving the gate is a measurement, not a constant false; unmeasured families
are named by native + id, refuse with their handler addresses, and never
appear bound; protocol variation is judged with zero `no_family`.
Retail (`#[ignore = "requires CS_GAME_DIR"]`): the census tables above,
re-measured through production readers and pinned, and the audit verdicts with
`campaign_ready` false.

Mutation sensitivity: dropping the native from the key fails
`accept_f38_d1_the_family_key_keeps_the_native` (the pair collapses or the
dedup refuses it, the spellings collide, or one site is judged by the other
native); removing the goslocalize rule fails both the synthetic binding test
and the retail bound count (1287 → 1113); flipping any unmeasured id to bound
fails the synthetic refusal test and the retail refused count (525).
