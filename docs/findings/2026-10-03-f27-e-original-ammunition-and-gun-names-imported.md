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
  nine fast tests over the production importer and three `#[ignore = "requires
  CS_GAME_DIR"]` tests that re-measure every id from the installation.
- This file.

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
  interaction options, each with its own claim id and reason. The lowering
  boundary refuses such a record, which is the correct outcome: a session must
  not fire ammunition whose damage nobody measured.
- **The caliber belongs to the gun, so the type declares none.** The original
  enumerates caliber and type as *one* vocabulary (five calibers against four
  types), so a per-type caliber would be a fabrication; the caliber this stage
  reads is the gun's own label row, kept as the original's text.
- **The markup code is split off, never interpreted.** Every ammunition-name
  row and every gun caliber row carries `[COUR9]`; the gun *long* names carry
  none and the blurbs carry `[CSB9I]`. `ORIGINAL_TEXT_MARKUP` documents the
  first, the tests pin all three, and no style rule is invented anywhere.
- **A missing or empty row is refused by id, never skipped.** `import` requires
  all 36 rows (4 types x 3 name rows + 4 descriptions + 3 empty-ammunition rows
  + 5 guns x 3 rows + 2 empty-gun rows). A silently shorter catalogue would make
  "four types, five guns" unfalsifiable, which is the whole failure F27-D
  measured.
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
  `.txt` (file offset `0x400`, 59 840 bytes, Shannon entropy **7.997**),
  `.text` (entropy 6.630) and `.txt2` (entropy 6.297), and **145 945 of its
  `.rsrc` section's 147 456 raw bytes are zero**. The only plaintext in the file is the C
  runtime, the Win32 import names and SafeDisc's own diagnostics ("Insert
  replication gold master in CDROM drive", `SAFEDISC_ERROR_%08lx`, the Spanish
  and German CD-warning strings). `crimson.icd` (2 580 578 bytes) is a second
  MZ image at entropy **7.811** with zero matches for
  `caliber|ammo|armour|piercing|incend|heat|gun|rocket|shell`. The damage table is
  in the decrypted image, i.e. only in a running original. **Affected content:**
  every damage amount F27 simulates. **Resolving task:** an owner-supplied
  original-run reference capture (#358 `REF-OWNER-FIRST-CAPTURE`, blocked;
  protocol #357), or a task filed against a decoded image.
- **Which gun each airframe mounts** (`f27.d.limit.gun_group_assignment`).
  `ZBD/planes.zbd` reads: 3 317 GameZ nodes, 562 distinct names, and the
  gun-bearing ones are `bgun0..bgun3`, `fgun`, `hgun`, `hgun2`, `rgun`,
  `gungauge`, `balmoral_turret0..3`, `bturret0..3`, `brigturret`, `brigturret2 `
  (with its trailing space), `brigand_turret1/2`, `fire_turret1/2`,
  `hell_turret1/2`, `hturret`, `hturret2`, `kestrel_turret1/2`. **None of the
  twenty `IDS_*GUNS` group names appears**, and only `rgun` carries a side, so
  the mesh data cannot place `INNERWINGGUNS 3061` and its ten siblings on a
  side. Unchanged from F27-D.
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

`AmmunitionAudit::run` against the real surface (F27-D's audit) still reports
`undeclared_ammunition_type` and eleven `uncovered_gun_group` findings on
`main`: the audit is F27-D's, it is not on `main` yet, and this stage does not
weaken its closure checks. What this stage removes is the *guess* those findings
were standing in for: the four ammunition ids now have measured names,
abbreviations and descriptions, so when the audit lands it compares against the
original's own vocabulary rather than against four labels.

Not claimed: `verified_original`, `release_approved`, any gameplay behavior, any
visual or audible result. What is claimed is **observed_tool**: a measured fact
about shipped files, read by production readers, pinned to
`install_sha256 b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
and `content_sha256 a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`.
