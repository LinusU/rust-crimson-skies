# F04-D: the archive bindings and the mission level in the VFS layout

Date: 2026-10-06. Task #687 `F04-D-order-archives`, the second of the three
follow-ups section G of
`docs/findings/2026-10-05-f04-d-original-lookup-order.md` filed (#685 reader
members, #686 ROF before loose for GOS requests, **#687 mission level,
gamez / planes / anim bindings and the texture rule**).

## Provenance

The bindings this task records are the ones the owner's static analysis of the
pinned executable recorded (finding sections B, C and E). Nothing here is a new
observation of the original: **the bindings are code-derived, not a runtime
capture**, and nothing here is `verified_original`.
[`BINDING_ORDER_STATUS`](../crates/cs_assets/src/vfs/binding.rs) is
`ClaimStatus::Inferred` for that reason, and the designed
`PRECEDENCE_ORDER_STATUS` stays `designed` — a binding names one explicit path
and consults no precedence class.

What this task adds is the **production path** that was missing: which archives
a world/mission load opens, at which level, and a mission-level mount for the
level the designed layout had no place for.

## A. What was missing

| Original (sections B, C, E) | VFS before this task |
| --- | --- |
| `gamez.zbd` per **world group**, `planes.zbd` from the ZBD **root**, `cam_anim.zbd` (world) then `mis_anim.zbd` (mission) loaded in that order, none falling back | nothing named these archives; a consumer picked its own spelling, so any name it invented was a guess |
| exactly **one** texture archive per world, from `zbd\<world>` with the shared `zbd` fallback | `cs_content::textures` selects the tier (#352) but nothing in the layout recorded that the search is *one archive, two directories* |
| a **mission level**: `ZBD/<world group>/<mission>` holding `mis_anim.zbd` and the mission's own `zrdr.zbd` | `SessionBuilder::mount_installation` mounts the whole tree plus one `world-<n>` mount per world group; mission-level archives had **no mount at all** |

The designed `cs_content::catalog::baseline::GEOMETRY_CONTAINER_PATTERN`
already said `"ZBD/<world group>/gamez.zbd and ZBD/planes.zbd"`, so the GameZ
pair was consistent with the finding before this task. It was a *string* in a
report, not a binding a load could be held to, and it did not cover the
animation pair, the mission level or the texture rule.

## B. What `cs_assets::vfs::binding` does now

[`WorldLayout::for_context`] records the bindings of one `ResolveContext`:

| Role | Container | Level | Family | Script order |
| --- | --- | --- | --- | --- |
| `camera_animation` | `<world>/cam_anim.zbd` | world | animation | 0 |
| `mission_animation` | `<world>/<mission>/mis_anim.zbd` | mission | animation | 1 |
| `texture_archive` | *searched*, see below | world | texture | 2 |
| `gamez` | `<world>/gamez.zbd` | world | gamez | 3 |
| `planes` | `<shared>/planes.zbd` | root | gamez | 4 |

* The **mission animation** binding exists only when the context names a
  mission; a world-only load binds three archives, not four.
* A context naming **no world group** is refused (`NoWorldGroup`, carrying the
  mission when there was one): every binding lives below a world group
  directory, so the original's `MISSION_DIR` is only ever used below
  `CAMPAIGN_DIR`.
* `BindingLevel::depth` is the directory depth below `ZBD` (root 0, world 1,
  mission 2). A level is **not** a precedence rank: the original opens these by
  explicit path, so a mission-level archive is not "higher priority" than a
  world-level one, it is a different file opened in addition to it.
* `has_directory_fallback` is `false` for all four named archives and `true` for
  the texture role — the only level fallback among them.
* `WorldLayout::missing(root)` lists the bindings this installation does not
  ship — the named archives, the texture archive the selection rule has bound
  and the two directories the texture search walks — so a layout naming an
  absent archive is visible instead of failing later. It is a **host listing**:
  no archive bytes are read, nothing is written.

### The texture archive

[`TextureBinding`] records the **search** — the world's own directory first,
then the shared `ZBD` directory — because that is a layout fact: an archive can
only come from the selected world or from `ZBD`, never from another world. The
**choice of file** is not re-implemented here: it is task #352's measured
budget-and-tier rule in `cs_content::textures`, which *depends on* `cs_assets`
and so cannot be called from it. A binding therefore starts empty, takes the
file that rule selected (`bind`), and refuses a second one
(`TextureArchiveAlreadyBound`) — the original opens exactly one archive per
world, and a name it does not hold falls through to `rimage.zbd`, never to
another tier. `bind` also refuses a file that is not **in** one of the two
searched directories (`TextureArchiveOutsideSearch`), because the original
never looks in another world's directory; the comparison is the case-folded
logical key of the archive's own directory, exactly as a mount compares
members, so `ZBD\C1C\RTexture15.zbd` binds for `zbd/c1c` and
`zbd/c1/rtexture2.zbd` is refused. Nothing in this crate claims which tier a
world picks.

### The mission level as mounts

[`mission_directories`] derives the mission level from **discovery**
(`Diagnosis::directories`), not from the campaign mission-name tables (which are
campaign-type dependent): every directory directly below a discovered world
group is a mission directory, original spelling kept. A directory that carries
no regular file is still a mission directory, so a mission whose archives were
all removed stays reportable.

[`SessionBuilder::mount_installation_missions`] mounts each of them under the
new `MISSION_NAMESPACE` (`mission`), bound to its world group **and** its own
mission, at `PrecedenceClass::MissionWorld` as a retail source. A mission mount
is a namespace of its own because a mission archive is keyed by the member name
of its **own** directory — `mission/default/mis_anim.zbd`, the way the original
opens it — while inside a world group's mount the same file is spelled
`M01/mis_anim.zbd`; keeping the levels apart means no lookup has to decide
between two levels of the installation by precedence class, which is the
*designed* order the original does not use. The original's own answer for reader
members is **mount order** — root, then mission, then world — implemented by
task #685 in `cs_assets::vfs::reader`; here each mission mount is bound to its
own world and mission, so a sibling mission is *skipped*
(`SkipReason::ScopeMismatch`) rather than tied, and two mission mounts never
present an equal-priority choice for the designed order to decide.

`mount_installation` itself is unchanged and still mounts the world level only;
its documentation now says so and points at this method, so nothing binds a
mission archive a caller did not ask for.

A mission directory whose own name is not a valid `MissionScope` label — for
example `_wip`, which starts with a separator the label rules refuse — is
refused with `SessionError::MissionScope` carrying the directory in the
installation's spelling, because no context could ever be admitted to such a
mount. The refusal is immediate and names that directory; the mounts added
before it stay, as for any other mount failure. No retail mission directory has
such a name, so this is the defensive path, not a measured one — and a directory
that merely *looks* unusual is not affected: `M01.2` folds to `m01.2`, which is
a valid label and mounts like any other mission directory.

## C. What retail data says, through the new production path

All four retail tests are `#[ignore = "requires CS_GAME_DIR"]`, read the
installation read-only and go through production discovery, `WorldLayout` and
the mount builder; none reads the archive tree with a test-only parser.

* **`retail_world_groups_bind_the_measured_archives`**: all eight world groups,
  and each of their mission directories, bind only archives the installation
  actually ships (`missing` is empty for every layout it builds).
* **`retail_gamez_is_never_per_mission_and_mis_anim_always_is`**: **no** mission
  directory ships a `gamez.zbd` (the per-world-group-only half of the GameZ
  rule, measured), **every** mission directory ships a `mis_anim.zbd`, and no
  mission directory ships a `cam_anim.zbd` (it is the world's).
* **`retail_each_world_searches_its_own_directory_then_the_shared_one`**: each
  world's texture search is `[<world>, <shared>]` in that order, the shared
  directory holds `rimage.zbd`, and every world group ships the unnumbered
  `texture.zbd` the measured walk ends at.
* **`retail_the_mission_level_mounts_every_mission_directory`**: all **53**
  mission directories are mounted under `mission`, each bound to a world group
  and a mission, and every mission's `mis_anim.zbd` reads back byte-for-byte
  from its own directory.

The container a layout composes is spelled from the context's `MissionScope`
label, which is lower case (`ia1`), while the installation spells the directory
`IA1`; the comparison is therefore the case-folded logical key, exactly as a
mount compares members, and `mission_directories` keeps the walk's own spelling.

## D. Recorded unknowns and limitations

Each item names the affected content and the task that owns it, and each is
recorded in the machine-readable report's `unknowns` array, **not** dropped:

* **The bindings are code-derived only.** No run of the original observed them,
  so `BINDING_ORDER_STATUS` is `Inferred` and `PRECEDENCE_ORDER_STATUS` stays
  `Designed`. Settling it against a run needs owner-supplied capture (**#358**)
  or further static work. Affects every world and mission load.
* **Which texture tier binds is not here.** The budget, the descending
  `rtextureN`/`textureN` walk and the software renderer's `texture.zbd` are task
  **#352** in `cs_content::textures`; that rule is not wired into a world load
  (**#688**). This task records the search and holds the file the rule selected.
  Affects every world's texture archive.
* **The reader loose-file override and the loose directory fallback are still
  unmodelled** (**#700**), and the index entry's unexplained bytes are **#692**.
  Affects every reader member lookup.
* **Which archives a mission that is not a directory of a world group would use
  is not settled.** The finding's mission-name tables are campaign-type
  dependent; this task uses the directories the installation has, so a mission
  with no directory binds nothing beyond its world's archives.
* **`missing` is a host listing, not a parse.** It proves a binding's file
  exists, not that the file is a readable container of the expected family; that
  is what `cs_formats`' family gate and `mount_reader_archive` do, and the ZBD
  containers of the families bound here are read by the F05/F06/F08 tasks.

## E. Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/vfs/binding.rs` (owner path): the module, its types
  (`ArchiveFamily`, `BindingLevel`, `BindingRole`, `BoundArchive`,
  `TextureBinding`, `WorldLayout`, `MissionDirectory`, `BindingError`,
  `BINDING_ORDER_STATUS`) and `mission_directories`.
- `crates/cs_assets/src/vfs/session.rs`: `MISSION_NAMESPACE`,
  `mount_installation_missions`, `SessionError::MissionScope`, and the
  `mount_installation` documentation.
- Wiring only: the module declaration, re-exports and one paragraph of
  `crates/cs_assets/src/vfs/mod.rs`; the one new `SessionError` arm in
  `crates/cs_assets/src/rof.rs` (a match on `SessionError`, no logic).
- `crates/cs_assets/tests/accept_f04_d_order_archives.rs`: the acceptance suite
  and the evidence harness.

**One observable failure:** before this stage nothing in the workspace named the
archives a world/mission load opens, so a consumer that picked a spelling picked
a guess, and a mission-level archive such as `zbd/c1c/m01/mis_anim.zbd` had no
mount and could not be resolved at all. That is what
`accept_f04_d_order_archives_the_mission_level_is_mounted_for_its_own_mission`
fails on without the new mount, and what
`accept_f04_d_order_archives_the_bindings_are_named_in_the_scripts_order` fails
on without the bindings.

## F. Tests

`crates/cs_assets/tests/accept_f04_d_order_archives.rs`, prefix
`accept_f04_d_order_archives_`: **twelve synthetic and four retail**, sixteen
in all (`TASK_TESTS`).

Synthetic (CI runs them): the four bindings in script order with their
containers and levels; `gamez` per world group and `planes` from the root across
three worlds; the mission level present only for a mission, absent without one,
two missions binding two archives, and a mission without a world refused with
its name; only the texture role falling back, to the shared directory and never
another world; exactly one texture archive bound per world, a second tier refused
and the bound one unchanged, **and a file from outside the two searched
directories refused**; every role reachable by its label and every family and
level labelled; the bindings reported `inferred` with the designed precedence
status untouched; `missing` naming exactly what an installation lacks, **bound
texture archive included**; mission directories derived from discovery (a
table-unknown `ZZ9` is a mission, a directory holding no regular file is one, a
directory two levels deeper is not); the mission level mounted for its own
mission, **as a retail mission/world mount**, with a sibling mission and another
world refused it and the world's own archives still resolving from
`WORLD_NAMESPACE`; a mission directory that is no valid scope label refused
naming it while a dotted one mounts; and the world layout alone binding no
mission.

Retail (`--include-ignored`, needs `CS_GAME_DIR`): the four tests of section C,
each also passing alone with `--exact`.

Sensitivity was checked by mutation, not asserted (seven mutations of the
production code, each reverted; the tree matches the commit):

| Mutation | Task tests that fail |
| --- | --- |
| `bind` accepts any file, ignoring the two searched directories | `one_texture_archive_is_bound_per_world` |
| `missing` ignores the bound texture archive | `a_binding_this_installation_lacks_is_listed` |
| `mission_directories` restricted to the campaign mission-name table | `mission_directories_come_from_discovery_not_a_name_table` |
| the mission mount classed `Shared` | `the_mission_level_is_mounted_for_its_own_mission` |
| the mission mount not declared `retail` | `the_mission_level_is_mounted_for_its_own_mission` |
| an invalid mission directory skipped instead of refused | `a_mission_directory_that_is_no_scope_label_is_refused` |
| `gamez.zbd` bound at the mission level | `the_bindings_are_named_in_the_scripts_order`, `gamez_is_per_world_group_and_planes_is_the_root`, `a_binding_this_installation_lacks_is_listed` |

`evidence_report_t687_writes_the_acceptance_report` is the evidence harness
(`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test: it re-derives every
world group and mission directory's bindings through production code, writes
`private/evidence/T687/bindings.json` (installation spellings, roles, levels and
containers only) next to the recorded test log, then `acceptance.json` for
`tools/validate_evidence.py`. The committed copy is
`docs/findings/evidence/T687.json`.

It is validated **without** `--require-pass`. That flag rejects a report whose
`unknowns` is non-empty, and section D's limitations are exactly what this stage
left open, so the report lists them (with the affected content and the resolving
task) and the flag exits 3 with "Unresolved issues" — the expected result.

## G. Scope note

The texture *file* choice stays in `cs_content::textures` (#352) and the reader
order stays in `cs_assets::vfs::reader` (#685): this task adds the layout and
the mission level, and references both rather than duplicating either. Nothing
here parses an archive; naming archives is this crate's job and reading them is
the format crates'.

Like #685, this stage has no consumer outside `cs_assets` and this suite yet:
what a world load opens is still the world's own wiring (#688), and the tool
`cs-inspect resolve` still mounts the world level only. That is deliberate
scope, not an oversight — F04's owner paths include
`tools/cs_inspect/src/resolve.rs`, so a follow-up may mount the mission level
there to make the new namespace reachable from the command line.

## H. Review corrections (2026-10-06)

Reviewer `bunny-2/bunny-2` — **the same agent instance that implemented this
stage, with its context, so this is not independent review** (see the identity
note in the handover). Everything below was fixed on the task branch, not left
as a comment:

1. **`TextureBinding::bind` did not hold the search it records.** It refused a
   second archive but accepted any path, so `zbd/c1/rtexture2.zbd` could be
   bound to `zbd/c1c` — a name the original never opens, and a rule the
   documentation stated but nothing enforced. Now refused with
   `TextureArchiveOutsideSearch`, compared by logical key, with a test.
2. **`WorldLayout::missing` skipped the texture archive.** Its contract is "the
   spellings this load binds that do not exist"; a bound texture archive is one
   of them and was not listed, so a rule that selected an archive this
   installation does not ship passed as complete. Now listed, with a test.
3. **`SessionError::MissionScope`'s documentation named a wrong example.**
   `M01.2` folds to `m01.2`, which **is** a valid `MissionScope` label, so the
   refused case it described cannot happen. Corrected to `_wip`, and the refusal
   — previously untested production behavior — is now pinned, including that a
   dotted directory still mounts.
4. **The `MISSION_NAMESPACE` rationale was wrong.** It claimed two mounts in one
   namespace holding one member name "would be an equal-priority collision the
   designed order cannot decide". `observe_collisions` groups by **basename
   across all mounts** regardless of namespace, and each mission mount is bound
   to its own world and mission, so a sibling is skipped and no ambiguity
   arises either way. The real reason (a mission archive is keyed by the member
   name of its own directory, so no lookup decides between two levels by the
   designed precedence) is now what the code and this finding say.
5. **Two documented behaviors had no test**: a mission directory holding no
   regular file is still a mission directory (the fixture wrote a `.keep` file
   into it, so an implementation that required a file would have passed), and a
   mission mount is a `MissionWorld` **retail** source (dropping either changes
   F04 non-negotiable behavior 2 — a retail decision that must stay blocked
   while the precedence order is only `designed` — with nothing failing). The
   fixture directory is now genuinely empty and both properties are asserted.

The reviewer's own mutation sweep (section F) found no gap in the original
sensitivity claim, but corrected its size: binding `gamez.zbd` at the mission
level fails **three** task tests, not two — `gamez_is_per_world_group_and_planes_is_the_root`
catches the level as well.
