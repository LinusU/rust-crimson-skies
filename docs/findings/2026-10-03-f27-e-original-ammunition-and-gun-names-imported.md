# F27-E: the original's ammunition and gun names are in a shipped file, and this stage imports them

Date: 2026-10-03. Task: #545 "Import the original ammunition records once their
names are measured" (F27-E, follow-up to #120 / F27-D). Sheet:
`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, F27
non-negotiable 1 ("Enumerate all actual ammunition ids from original data") and
AC04's closure target. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
Predecessor: `docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md`.

Capabilities used: `retail` (read-only access to `$CS_GAME_DIR`) and ordinary
build/test. **Not** used and not claimed: any run of the original executable, so
nothing here is evidence of how the original *behaves* — only of what its files
declare.

## The headline: F27-D's blocking claim was wrong, and this stage corrects it

F27-D recorded, as `f27.d.limit.ammo_names` and `f27.d.limit.gun_set`:

> the ammunition screen never names a type — it asks the engine for the array …
> `crimson.exe` carries **no** `RT_STRING` resource at all … and `strings.dll`'s
> `RT_STRING` blocks run `7..=1072` … and **does not contain block 211** … So
> the id the scripts use is not a Win32 string-table id in either numbering,
> and the catalog behind `callback($$E$$,5067,10585,…)` is a different one
> entirely.

Both halves of that are **first-hand measurements that hold**, and both are
**incomplete**: the installation ships a **third** PE image with the game's UI
strings that neither F27-D nor its reviewer opened.
`GOSDATA/ASSETS/BINARIES/langui.dll` (282 624 bytes, SHA-256
`357e6bb05f1d2872a00e0976fdde44561cd5bb6a56d9f555d85d0ff1481faf49`) carries
**101** `RT_STRING` blocks holding **1 616** counted rows (ids `0..=40175`, no
undecodable unit and no duplicate id), and the ids `RESOURCE.H` declares line up
with it **exactly**:

| declared block | first id | measured |
| --- | --- | --- |
| `IDS_AMMOLONGNAME` | 3350 | four names at `3350..=3353`, then `3354` = "None" |
| `IDS_AMMOSHORTNAME` | 3360 | four names at `3360..=3363`, then `3364` = "None" |
| `IDS_AMMOABBRNAME` | 3365 | four names at `3365..=3368`, then `3369` = "None" |
| `IDS_AMMODESCRIPTION` | 3370 | four descriptions at `3370..=3373`, then `3374` = "No Information Available" |
| `IDS_GUNLONGNAME` | 3310 | five names at `3310..=3314`, then `3315` = "No Gun" |
| `IDS_GUNSHORTNAME` | 3320 | five caliber labels at `3320..=3324`, then `3325` = "No Gun" |
| `IDS_GUNDESCRIPTION` | 3330 | five blurbs at `3330..=3334`, then `3335` = "No Information Available" |

Seven independent four- and five-wide alignments that land on the declared ids
with the right shape (four types + one "None", five guns + one "No Gun") are not
a coincidence, and they settle the question F27-D could not: **the id the
scripts use is a Win32 `RT_STRING` id**, in `langui.dll`, numbered by the
production rule `cs_formats::pe_resources::string_id(block, index) =
(block - 1) * 16 + index` that #374 already measured and defended.

### What the four ammunition types are

Measured through the production `cs_content::config::StringCatalog` over
`langui.dll`, in the blocks' own order:

| selection | long name (`3350+i`) | short name (`3360+i`) | abbreviation (`3365+i`) |
| --- | --- | --- | --- |
| 1 | Slugs | Slug | Slug |
| 2 | Dum-Dum Bullets | Dum-dum | DD |
| 3 | Armor-Piercing Bullets | Armor-piercing | AP |
| 4 | Explosive Bullets | Explosive | EX |

The four descriptions (`3370..=3373`) state, in the original's own words, what
each type does: standard lead bullets damage armor and internal components
equally; `DD` rounds have a split head that flattens on impact and spread
damage over a broader area; `AP` rounds have a hardened tip for shredding armor
and **"tend to punch clean through unarmored surfaces, inflicting very little
damage"**; `EX` bullets carry shaped charges that detonate on any hard surface.

### The five guns, and the calibers

`IDS_GUNDESCRIPTION 3330..=3334` names exactly five guns, one per caliber
`.30 .40 .50 .60 .70`, each with a manufacturer and a model. `IDS_GUNSHORTNAME
3320..=3324` gives the caliber rows this stage reads as
[`DeclaredCaliber`]: `" .30-cal."` … `" .70-cal."` — verbatim, each with the
measured leading space, and **not** parsed into a numeric bore.

### A second, independent vocabulary: the engine's own identifier names

`strings.dll` carries 880 ASCII `MSG_*` identifiers, of which 20 are the
gun-ammunition vocabulary, five calibers times four types:

```
MSG_WEAP_{30,40,50,60,70}CAL_{APIERCING,DUMDUM,MAGNESIUM,SLUG}
```

`30CAL … 70CAL` is the five calibers this stage measured from `langui.dll`, and
`APIERCING, DUMDUM, SLUG` are the display types 3, 2 and 1. **The fourth
identifier is `MAGNESIUM` where the display name is "Explosive"**, and the
identifiers are in alphabetical order, so **nothing states which identifier
names which display type**. This stage records the identifier set and does not
map it. (`MSG_WEAP_APIERCING_ROCKET` and `MSG_WEAP_INCENDIARY_ROCKET` are
rockets, F28's subject, not gun ammunition.)

## What this stage changed

- `crates/cs_content/src/weapons.rs` (extend): the measured id tables
  (`ORIGINAL_AMMUNITION_LONG_NAME_IDS` and its three siblings,
  `ORIGINAL_GUN_LONG_NAME_IDS` and its two siblings, the two "None" and two
  "No Gun" rows, the counts), `OriginalStringTable` (the catalog the importer
  is handed), `OriginalMeasuredText` (splits the shipped `[TAG]` markup code off
  and keeps it), `OriginalImportError` (four refusals, each by id),
  `OriginalAmmunitionIdentity` / `OriginalGunIdentity` /
  `OriginalGunAmmunitionCatalogue` and `::import`, the four claim ids, and
  `OriginalAmmunitionIdentity::declared` /
  `OriginalGunAmmunitionCatalogue::declared_ammunition`.
- `crates/cs_content/tests/accept_f27_e_original_ammunition_catalogue.rs` (new):
  ten fast tests over the production importer and F27-D's audit, and four
  `#[ignore = "requires CS_GAME_DIR"]` tests that re-measure every id from the
  installation.
- `crates/cs_content/tests/evidence_report_f27_e.rs` (new): the evidence
  harness.
- This file and `docs/findings/evidence/F27-E.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original asset, no
binary file. **`crates/cs_sim/src/weapons/guns.rs` and
`crates/cs_app/src/weapons.rs` are untouched**: see "Why no runtime record".

### The one observable failure, before the change

F27-A's schema said the original set "is enumerated by F27-D from the
installation", F27-D measured the *shape* of four types and five guns, and
**nothing in the project could name one of them.** Given a `StringCatalog`, there
was no production query anywhere that turned `RESOURCE.H`'s declared ids into
text, so `DeclaredAmmunition`'s four ids were arbitrary labels over a
`Resolved::Unknown` caliber and two `Resolved::Unknown` damage channels: a
catalogue with any four names at all would have satisfied every check there
was. The names were measured but unreachable, which is the failure F27
non-negotiable 1 names.

## Choices worth stating

- **The declared records carry no invented number.** Every imported
  `DeclaredAmmunition` has `Origin::Installation { source }` over the language
  image's own byte span, a **known** identity (four measured names) and an
  **unknown** caliber and unknown damage on both channels and all five
  interaction options, each with its own claim id and reason. What keeps that
  from becoming a firing round is *not* a lowering refusal:
  `cs_app::weapons::lower_ammunition` lowers only the id and consults neither the
  damage profile nor the caliber, so it succeeds here. What keeps it out of a
  session is that **nothing pairs the four types with a gun** — no
  `DeclaredGunDefinition` can be built without a measured mount (below), and no
  `DeclaredLoadout` names them — which is exactly what the audit reports as
  `unpaired` on every row.
- **The caliber belongs to the gun, so the type declares none.** The original
  enumerates caliber and type as *one* vocabulary (five calibers against four
  types), so a per-type caliber would be a fabrication; the caliber this stage
  reads is the gun's own label row, kept as the original's text.
- **The markup code is split off, never interpreted.** Every ammunition-name
  row and every gun caliber row carries `[COUR9]`; the gun *long* names carry
  none and the blurbs carry `[CSB9I]`. `ORIGINAL_TEXT_MARKUP` documents the
  first, the tests pin all three, and no style rule is invented anywhere.
- **A missing or empty row is refused by id, never skipped.** `import` requires
  all **31 identity rows** (4 types x 3 name rows + 4 descriptions + 5 guns x 3
  rows) and refuses each by id. The five *empty-slot* rows the blocks leave
  behind (`3354`, `3364`, `3369`, `3315`, `3325`) are measured too and the
  retail test reads all 36 rows of the installation's catalog, but `import` does
  not require them: they are the loadout's "None"/"No Gun" placeholders, not an
  identity. A silently shorter catalogue would make "four types, five guns"
  unfalsifiable, which is the whole failure F27-D measured.
- **The prose is measured but not committed.** This file and the tests name the
  type labels, the abbreviations and the caliber labels — 21 short factual
  strings, and F27 non-negotiable 1 asks for exactly that vocabulary. The four
  description sentences, the five gun names and the engine blurbs are asserted
  only by length (`> 20` code units) and are identified by id, following #374's
  rule that original string *text* stays out of the repository. F27-D committed
  `RESOURCE.H`'s macro identifiers, which are identifiers; these are display
  labels, and the owner may rule that even those should be id-only.

## Test sensitivity

The importer's value is that it can fail, so each of its decisions was removed
one at a time (source patched in place, restored after each run) and the whole
`accept_f27_e_` selection re-run with `--include-ignored`. Every mutation was
caught; the "caught by" column is what the run reported.

| mutation | caught by |
| --- | --- |
| `OriginalMeasuredText::measure` stops splitting the `[TAG]` code | 5: `..._the_markup_code_is_split_off_and_kept`, `..._a_measured_catalog_imports_four_types_and_five_guns`, `..._an_empty_row_is_refused_by_id`, `..._a_type_identity_is_queryable_by_selection`, `..._retail_the_original_names_four_ammunition_types_and_five_guns` |
| the importer stops reading `IDS_AMMOABBRNAME` | 4: the same set without the markup test |
| every gun's caliber becomes the first caliber instead of its own row | 1: `..._a_measured_catalog_imports_four_types_and_five_guns` |
| a missing row falls back to a placeholder instead of being refused | 2: `..._a_missing_row_is_refused_by_id`, `..._an_empty_catalog_is_refused_on_its_first_row` |

The first three mutations are also caught by the **retail** test, so the
importer's shape is pinned against the installation and not only against a
synthetic catalog.

## Unknowns recorded (not guessed)

- **Per-type damage amounts** (`f27.d.limit.ammo_names`, the damage half).
  `crimson.exe` is a **C-Dilla/SafeDisc-protected image**: its code sections are
  `.txt` (file offset `0x400`, 59 904 raw bytes, Shannon entropy **7.997**),
  `.text` (entropy 6.630) and `.txt2` (entropy 6.297), and **145 945 of its
  `.rsrc` section's 146 944 raw bytes** (virtual size 146 552) **are zero**. The only plaintext in the file is the C
  runtime, the Win32 import names and SafeDisc's own diagnostics ("Insert
  replication gold master in CDROM drive", `SAFEDISC_ERROR_%08lx`, the Spanish
  and German CD-warning strings). `crimson.icd` (2 580 578 bytes) is a second
  MZ image at entropy **7.811** whose only case-insensitive hits for
  `caliber|ammo|armour|piercing|incend|heat|gun|rocket|shell` are the Win32
  import name `ShellExecuteA` and one three-byte coincidence inside encrypted
  bytes; no word of that list appears as game content. The damage table is
  in the decrypted image, i.e. only in a running original. **Affected content:**
  every damage amount F27 simulates. **Resolving task:** an owner-supplied
  original-run reference capture (#358 `REF-OWNER-FIRST-CAPTURE`, blocked;
  protocol #357), or a task filed against a decoded image.
- **Which gun each airframe mounts** (`f27.d.limit.gun_group_assignment`).
  Two independent negatives, one measured by this stage:
  - The shipped language image **does** name the groups, under the same ids
    F27-D read from `RESOURCE.H`: `3060..=3079` carry `[TREB13B]`-marked display
    names (`3061` "Inner Wing Guns" … `3079` "Middle Wing Guns") and **`3080`
    (`NOSETURRET`) is an empty row**. None of the *eleven uncovered* groups'
    names says which side or which airframe — `INNERWINGGUNS 3061` is "Inner
    Wing Guns", with no left or right — so the display names do not close the
    gap; they only show that the row exists and carries no side. (The test
    `accept_f27_e_retail_the_shipped_image_names_nineteen_gun_groups_and_leaves_the_twentieth_empty`
    pins exactly that: nineteen named, one empty, at `ORIGINAL_GUN_GROUP_NAMES_LAST_ID`.)
  - `ZBD/planes.zbd` holds no group name either: no `IDS_*GUNS` name appears
    anywhere in the container's bytes (a byte scan of the 6 083 868-byte file
    finds `INNERWINGGUNS`, `OUTERWINGGUNS`, `CENTERGUNS`, `MIDDLEWINGGUNS`,
    `NOSETURRET`, `WINGGUNS` and `GUNS` **zero** times each, while `bgun`,
    `fgun`, `hgun`, `rgun` and `gungauge` do occur). Read through the production
    GameZ reader the container is 3 317 nodes over 562 distinct names, of which
    the gun-bearing ones are `bgun0..bgun3`, `fgun`, `hgun`, `hgun2`, `rgun`,
    `gungauge` and the `*_turret*` parts; only `rgun` carries a side.
  So the per-airframe tables in the executable remain the only place a side
  could come from. Unchanged from F27-D, with the display names now checked too.
- **Convergence** (`f27.d.limit.convergence`) and **the inherited-velocity
  rule** (`f27.d.limit.inheritance`) are original *behavior*; no shipped file
  declares them. `cs_sim::weapons::MountTransform::forward` still carries the
  resolved direction and no convergence geometry is invented.
- **Penetration, ricochet and in-flight ammo switching**
  (`f27.d.limit.interaction_rules`) stay declared and unapplied.
  `InteractionRules::deferred` still names `DEFERRAL_STAGE` (`"F27-D"`) for all
  three; this stage **does not** re-point it at itself, because F27-E resolves
  none of them, and re-filing a deferral to the stage that could not resolve it
  would make the machine-readable contract less true. Every imported record
  carries all three as `Resolved::Unknown` under `f27.e.ammunition-behavior`, so
  the deferral is now visible per record as well as in the schema.

## Why no runtime record, and what this stage does *not* claim

`DeclaredGunDefinition` needs a `mount` (`DamageNodeKey`), a
`DeclaredGunMountKind` and an optional `scene_binding`; **all three are
per-airframe data this stage cannot measure** (see the gun-group unknown above),
and F27 non-negotiable 2 forbids inventing them. So this stage builds no
`DeclaredGunDefinition` for the original's five guns: it imports their
**identity** — name rows, caliber — and leaves the runtime record to the stage
that measures the mounts. That is also why `cs_sim` and `cs_app` are untouched;
a runtime record with a designed mount would be a guess wearing a measured
name.

With the audit available, the verdict this stage can and cannot claim is worth
stating precisely. F27-D's `AmmunitionAudit::run` compares **counts**: it reports
`undeclared_ammunition_type` only while `declared < observed`, so four declared
records close that finding whatever their labels say, and the test proves the
check is live by dropping one record and watching the finding return. What this
stage actually changed is that those four records now **are** the original's own
vocabulary — measured names, abbreviations and descriptions — instead of four
labels over unknown values. The finding is gone; the reason it is *meaningful*
is the measurement, not the audit.

`AmmunitionAudit::run` against the real surface still reports
`uncovered_gun_group` for the eleven groups only the executable's per-airframe
tables can place, plus per type `unmeasured_caliber`, `no_damage_consumer` and
`unpaired`, and `is_complete()` stays false. Nothing in F27-D's closure checks
was weakened to reach that verdict: the audit still fails, on the gaps this
stage could not measure.

Not claimed: `verified_original`, `release_approved`, any gameplay behavior, any
visual or audible result. What is claimed is **observed_tool**: a measured fact
about shipped files, read by production readers, pinned to
`install_sha256 b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
and `content_sha256 a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`.

## Evidence

`private/evidence/F27-E/acceptance.json`, committed as
`docs/findings/evidence/F27-E.json`, validates with
`python3 tools/validate_evidence.py private/evidence/F27-E/acceptance.json
--artifact-root private/evidence/F27-E --require-pass` (exit 0,
`structurally_valid: true`, `artifact_count: 2`). It records:

- `capabilities: ["retail", "synthetic"]`, `claim: "implemented"` — never
  `verified_original`;
- `tests: {discovered: 14, executed: 14, passed: 14, failed: 0, ignored: 0}`
  over the fourteen `accept_f27_e_` tests, the four retail ones run with
  `--include-ignored`;
- `install_sha256 b4e780ab…c631978` and `content_sha256 a0223506…62c12d`, both
  from production `cs_assets::install` discovery, never typed in;
- two hashed artifacts: `cargo-test.log` (the recorded acceptance run) and
  `ammunition-vocabulary.json` — a **second** production pass of the same
  readers over the same installation, carrying `langui.dll`'s digest, length and
  `StringCatalog` accounting (101 blocks, 1616 rows, no undecodable unit, no
  duplicate id), every declared id with its **code-unit length and markup code
  but never its text**, the twenty gun-group rows as id + header label +
  named-or-not, what the production importer made of the catalog
  (4 types, 5 guns, 4 declared records, 0 with a declared caliber), and
  `strings.dll`'s 20 `MSG_WEAP_<cal>CAL_<type>` identifiers (counted apart from
  the 37 `MSG_WEAP_*` names that image carries, the rest being rockets and other
  ordnance) as the corroborating vocabulary;
- the five `f27.d.limit.*` items this stage does **not** resolve, each with the
  content it gates and what would resolve it, inside the hashed artifact and
  inside the report's `review.method`.

`unknowns` is empty and that is a claim worth checking. The validator's
`--require-pass` rejects a nonempty `unknowns` list, and every one of the five
remaining items is *unmeasured original behavior* rather than a failure of this
stage's assertions — every assertion here is a measured fact about a shipped
file or a production behavior the suite exercised, and all of them pass. They
are recorded in the four places above instead of removed.

The committed report is **regenerated by the reviewer** on the rebased tree it
reviewed (`candidate_tree` = the tree of that commit), not the implementer's
earlier one: the report must match the tree the suite ran on, and this stage's
code changed in review. The commit that adds this copy is therefore
documentation-only on top of the measured tree, as
`docs/contracts/CLI-EVIDENCE.md` describes.

## Follow-up filed

`#547 (F27-E.1)` carries what this stage could not measure: the per-type damage
amounts, the per-airframe gun-group assignment, and — once an owner-supplied
capture exists — the convergence, inherited-velocity and interaction rules.

## What F27-D's audit now says (added after #120 merged)

`#120` (F27-D) landed on `main` while this stage was in flight, so the audit
this task's acceptance criterion names is now available and this branch runs it:
`accept_f27_e_the_audit_type_closure_holds_and_the_rest_is_named` feeds the four
imported records and F27-D's measured surface (the production
`ORIGINAL_GUN_GROUPS` and its counts) to `AmmunitionAudit::run` and reads the
verdict.

- **`undeclared_ammunition_type` is gone.** F27-D measured "observed 4,
  declared 0"; against the same surface the imported catalogue declares **4**, so
  the audit reports no type it cannot map. The check counts records (see "Why no
  runtime record" above for what that does and does not prove).
- **The rest stays, named.** `uncovered_gun_group` (the eleven groups only the
  executable's per-airframe tables can place), and per type `unmeasured_caliber`,
  `no_damage_consumer` and `unpaired` — no gun is paired because no
  `DeclaredGunDefinition` may be built without a measured mount.
  `is_complete()` is therefore still false, which is the correct answer.
- **Every type has a row**, addressable by id through `AmmunitionAuditReport::row`,
  so an audit over the imported catalogue can answer what a type is.

Nothing in F27-D's closure checks was weakened to reach that verdict: the audit
still fails, on the gaps this stage could not measure.

## Documentation and measurements corrected in review

Review (a fresh session, same agent identity — see the handover) re-measured this
stage's claims from the installation and corrected the following, all of which
were on the branch as submitted:

1. **"The names are readable from no file" was false for the gun groups too.**
   `ORIGINAL_GUN_GROUPS`' doc said the engine's *display* names for `3061..=3080`
   "are readable from no file", and `OriginalGunLoadout`'s said the ammunition
   names "live in the executable's own tables, which no agent can read". Both are
   corrected: the shipped image names nineteen of the twenty groups at their own
   ids (`3080` empty), and the ammunition names are imported here.
2. **"`import` requires all 36 rows"** — it requires the **31** identity rows. The
   five empty-slot rows are measured and asserted by the retail test but are not
   identity rows and are not required.
3. **"The lowering boundary refuses such a record"** — `cs_app::weapons::lower_ammunition`
   checks only the id namespace and *succeeds*. What keeps the imported types out
   of a session is that no gun and no loadout pairs them, which the audit reports
   as `unpaired`.
4. **The audit closure was credited to the names.** `AmmunitionAudit::run`
   compares counts, so four records close `undeclared_ammunition_type` whatever
   they are called; a negative case now proves the check is live.
5. **Two executable measurements were off by a little**: `crimson.exe`'s `.txt`
   section is 59 904 raw bytes (not 59 840) and its `.rsrc` section is 146 944 raw
   bytes with 146 552 virtual (not 147 456 raw). The 145 945 zero bytes are right.
   `crimson.icd` does not have "zero matches" for the word list — `ShellExecuteA`
   (an import) and one three-byte coincidence inside encrypted bytes match; no
   word of the list appears as game content.
6. **The evidence artifact counted the wrong set**: `gun_ammunition_identifiers`
   was the whole `MSG_WEAP_*` set (37 names, including rockets) while the claim
   was the **20** `<caliber>CAL_<type>` names; the two are now counted apart, and
   the harness asserts the count is 20.
7. **The synthetic catalog was not shaped like the measured one.** Its doc said
   "every required id carries a `[COUR9]`-prefixed row", but the installation
   writes no code on the gun long names and a different code (`[CSB9I]`) on the
   gun blurbs. The fast half now reproduces all three shapes and asserts that a
   bare row imports without a markup and a blurb keeps its own code.

Review also added, because the ids are F27-D's and nothing pinned them together:
cross-checks that this stage's four name blocks and both counts equal F27-D's
`ORIGINAL_AMMO_NAME_BLOCKS`, `ORIGINAL_AMMUNITION_TYPES` and
`ORIGINAL_SELECTABLE_GUNS`, and the use of F27-D's five measured counts in the
audit surface instead of bare literals.

Review re-ran the sensitivity check on the changed code, on top of the four
mutations below: removing the markup split is caught by **6** tests (the same
five plus the new gun-group test), and **removing F27-D's closure check from
`AmmunitionAudit::run` is caught by the new negative case** — before this review
that check could have been deleted with the suite still green, because the only
assertion on it was that the finding was absent.
