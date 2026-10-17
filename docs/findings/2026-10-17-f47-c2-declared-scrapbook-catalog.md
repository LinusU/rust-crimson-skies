# F47-C2: the declared original scrapbook catalog

Date: 2026-10-17. Task: #773 / F47-C2 "Declare the original scrapbook catalog
the runtime projects" — the `### F47-D` audit's follow-up
(`specs/F47-scrapbook-records-mementos-and-mission-replay.md`). Shared
contracts: [`STATE-TRANSACTIONS`](../contracts/STATE-TRANSACTIONS.md) and
[`CLI-EVIDENCE`](../contracts/CLI-EVIDENCE.md). Required capabilities:
ordinary build/test plus `retail` — `CS_GAME_DIR` present and used. Task test
prefix `accept_f47_e_`. Evidence: `docs/findings/evidence/F47-C2.json`,
produced by `crates/cs_content/tests/evidence_report_f47_c2.rs` per
`docs/contracts/CLI-EVIDENCE.md`.

Implemented by Devin SWE-2 (Rally implement claim, session of 2026-10-17).
This file is the implementer's record; it is not a review and awards no
`verified_original`/`release_approved`. `retail` below means read access to
the owner's installation, never a run of the original executable.

## One observable failure (listed before editing)

F47-D's audit measured `declared: 0`, `undeclared_items: 461`: the runtime had
**no** production `ScrapbookCatalog` — F47-A's entries are authored fixtures,
F47-B persists whatever it is handed, F47-C projects whatever it is given, and
the F47-C1 screen's `FlowSetup::scrapbook` input had no production builder.
Concretely, with this stage's production code removed:

* `cs_content::scrapbook::declare` does not exist, so nothing turns a
  `DiscoveredScrapbook` into entries a `ScrapbookUi` can project;
* `cs_content::scrapbook::resource_names` does not exist, so `TitleResID`'s
  `IDS_*` names have no production join to string-table numbers;
* `cs_app::ui::scrapbook::declared_inputs` does not exist, so the front end
  cannot build `ScrapbookInputs` from an installation.

`accept_f47_e_every_discovered_item_becomes_a_declared_entry` fails first.

## Files

Owner paths only (`crates/cs_content/src/scrapbook.rs`,
`crates/cs_app/src/ui/scrapbook/`, `tests/`, `docs/findings/`); no
`Cargo.toml` or `Cargo.lock` change was needed.

* `crates/cs_content/src/scrapbook.rs` —
  `DiscoveredScrapbook::items()` (table-order iterator), `DeclareError`,
  `DeclaredScrapbook { catalog, unresolved_titles }`,
  `resource_names(install_root)` and
  `declare(discovered, resource_id)`. `declare` takes the name→id join as a
  resolver so the synthetic tests need no fixture install; `resource_names`
  is the production read of that join.
* `crates/cs_app/src/ui/scrapbook/declared.rs` (new) + `mod.rs` (re-export +
  one doc paragraph): `declared_inputs(install_root, page_size)` — the
  production chain `discover` → `resource_names` → `declare` →
  `ScrapbookInputs::try_new`, so a mount, member, header or build refusal is
  an error, never an empty catalog.
* `crates/cs_content/tests/accept_f47_e_declared_catalog.rs` (new): six
  synthetic tests plus the retail pin.
* `crates/cs_app/tests/accept_f47_e_declared_inputs.rs` (new): a missing
  installation is `DeclaredScrapbookError::Discover`, and the retail leg
  builds inputs for the screen.
* `crates/cs_content/tests/accept_f47_d_scrapbook_audit.rs` (edited as the
  task instructs): the retail audit now declares through the production
  chain — `declared` 0→461, `undeclared_items` 461→0, `unlock_unknown` 461 —
  and stays `!is_complete()` for the honest reason (unmeasured rules, not
  missing entries).
* `crates/cs_content/tests/evidence_report_f47_c2.rs` (new): the evidence
  harness, deliberately **not** named `accept_f47_e_*`.
* `docs/findings/evidence/F47-C2.json` (new): the validated acceptance
  report; this file.

## What the declaration states — and only that

One `ScrapbookEntry` per `DiscoveredItem`, keyed
`scrapbook_item/<install_file_key(entry_key)>`, so it joins F14-D.8's
baseline rows and F47-D's discovery on identity (asserted equal as a set on
retail).

| field | what is declared | why |
| --- | --- | --- |
| `title` | `string_resource/<n>` when `TitleResID` parses as decimal (`"0"` included) or its `IDS_*` name resolves through the resource headers; the name itself, escaped into the key, when no header defines it | the field is a resource **name**, not a number; see the header measurement below |
| `image` | `image/<install_file_key(member)>` of `DiscoveredItem::capture`, `None` when absent | the archive's own state, never a placeholder |
| `unlock` | `Resolved::Unknown` under claim `f47.c2.declared_scrapbook.unlock_unmeasured` | `Objective` is parsed but its meaning is unmeasured; an unknown rule never unlocks — verified: `catalog.unlocked(always-true)` is empty |
| `kind` | `EntryKind::Page` for every record | `Objective` is the only candidate discriminator and it is unmeasured; no `Memento` without evidence (`ORIGINAL_MEMENTO_RECORDS == 0`) |
| `visibility` | `EntryVisibility::Shown` | whether the original hides locked entries is unmeasured; shown-but-never-unlocked is what the table proves |
| `replay` | `None` | the table documents no mission field (`ORIGINAL_REPLAY_FIELDS == 0`) |
| `text` | not carried | `ScrapbookEntry` has no body-text slot; `TextResID` is parsed but cannot be projected — recorded as a limitation |

## What the installation measures

Everything below is the production chain over `$CS_GAME_DIR`, pinned by
`accept_f47_e_retail_installation_declares_the_whole_table` and re-derived in
the evidence harness.

| measurement | value |
| --- | --- |
| declared entries | **461** — one per discovered item |
| `audit.declared` / `declared_matched` / `undeclared_items` | **461 / 461 / 0** (was 0 / 0 / 461) |
| `audit.unlock_known` / `unlock_unknown` | 0 / **461** |
| entries with `image` / `image: None` | **294** / **167** (the `Snap_*` gap) |
| `TitleResID` null spellings → `string_resource/0` | **383** (379 `"0"` + 4 empty) |
| named `TitleResID` resolved → `string_resource/<n>` | **66** |
| named `TitleResID` unresolved | **12** |
| `audit.is_complete()` | **false** — the unlock rules are unmeasured, not the entries missing |

### The resource-name join, measured

`TitleResID`/`TextResID` are `IDS_*` **names**. The naive join —
`ASSETS/SCRIPTS/RESOURCE.H` — resolves almost none of them: enumerating the
container's `*.H` members shows the archive holds **two** resource headers,
and the scrapbook names live mostly in the second:

| member | `IDS_SB_*`/`IDS_NT_*` defines |
| --- | --- |
| `ASSETS/SCRIPTS/RESOURCE.H` | 24 |
| `ASSETS/SCRIPTS/RESRC1.H` | 172 |

`resource_names` merges both members and applies
`ResourceHeader::resource_id`'s own rule across them: a name defined more
than once anywhere, or whose one define names no id, is absent rather than
guessed.

### Twelve dead references

Twelve `TitleResID` names are defined in **no** member of either installed
container (`crimson.rof`, `crimptch.rof`) — verified by the acceptance test's
assertion and independently re-verified in the evidence harness by a
byte-level scan of both headers (not trusting `resource_names`'s negative):

```
IDS_SB_08_01_MANIFEST_T     IDS_SB_16_01_SPARKSNOTE_T
IDS_SB_09_01_TELEGRAM1_T    IDS_SB_21_01_BILLBILL_T
IDS_SB_09_01_TELEGRAM2_T    IDS_SB_21_01_CONLETTER_T
IDS_SB_12_01_FENCE_T        IDS_SB_21_01_CRAWNOTE_T
IDS_SB_14_01_TELEGRAM3_T    IDS_SB_22_01_COMICLETTER_T
                            IDS_SB_22_01_RICHLETTER_T
                            IDS_SB_24_01_CRAWNOTE2_T
```

The resource-name→id mapping is a compile-time `.H` construct, so the
original could not resolve them either: the table ships twelve dead
references. Their entries keep the escaped name in the title key
(`string_resource/ids_5f_…`) and are reported in
`DeclaredScrapbook::unresolved_titles`; the screen shows them title-less
rather than with text they never had.

## What the screen now shows

`declared_inputs(root, 18)` hands the F47-C1 screen a 461-entry catalog in
the table's own order: 66 real titles resolvable through the locale chain,
294 pictures, 167 entries honestly picture-less, **zero** unlocks — every
entry is `Shown` but `is_unlocked` is false, so the book displays as fully
locked. That is the measured state: the declaration does not award the
player a single page.

## Unknowns and recorded choices

* `unlock` — unknown for all 461 (see table above). **Resolves in #774 /
  F47-D.2** (`Objective` meaning).
* `replay` — none declared; no table field names a target. **#774 /
  F47-D.2** plus the campaign bindings.
* `EntryKind` — `Page` for every record; no measured discriminator.
  **#774 / F47-D.2** may revisit.
* `EntryVisibility` — `Shown` declared, not measured. **#774 / F47-D.2**.
* `TextResID` — dropped: the schema has no body-text slot. A schema
  extension or text join is follow-up work, not this stage.
* Empty `TitleResID` (4 records) → `string_resource/0`: both null spellings
  declared as the table's null resource; the schema cannot carry an absent
  title, and the choice is recorded here.
* Whether resource `0` resolves to real text in the shipped string image is
  unmeasured — if it does, 383 entries share the original's null-resource
  string, which is still the field's own transcription.
