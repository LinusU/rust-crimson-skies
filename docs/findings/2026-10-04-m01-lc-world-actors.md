# #632: which `zrdr.zbd` member drives which actor

Date: 2026-10-04. Task: #632 (`M01-LC-WORLD-ACTORS`), "Measure world-actor
program semantics for the zrdr.zbd animation members" — the step
`VS-M01-RUNTIME` (#359) waits on. Feature sheets:
`specs/F20-object-animation-and-authored-destruction-states.md` (stage
`### F20-D`) and `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`
(stage `### F34-D`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and ordinary
build/test. `gpu` and `audio` were available and **not used**: nothing is
rendered or played, and no original run happened, so nothing here is
`verified_original`.

## Files

- `crates/cs_app/src/animation/programs.rs` (new, an F20 owner path): the
  `.zrd` record readers, the object-selector vocabulary, the binding resolver,
  `WorldNodeNames` and `UnmeasuredFieldFamily`.
- `crates/cs_app/src/animation/mod.rs` (wiring only): `pub mod programs;`, its
  re-exports and the module-documentation paragraph. The review also dropped the
  duplicate `STARTUP_MEMBER` re-export here and made `programs` reuse
  `carrier::STARTUP_MEMBER`, which the rebase onto `main` made a compile error.
- `crates/cs_app/tests/accept_m01_lc_world_actors.rs` (new, an F20 owner path):
  the eleven `accept_m01_lc_world_actors_` tests, seven synthetic and four
  retail.
- This file.

**No reader refusal was weakened.** `git diff` touches no line of
`crates/cs_formats`, `crates/cs_types`, `crates/cs_assets` or `cs_content`; the
`.zrd` grammar is the existing `cs_content::stunts::decode_zrd` (F09/#463's
measured tags `1`/`2`/`3`/`4`) and the member walk is the existing
`cs_formats::script_raw::discover_container` (F13-B) plus the F06 version-one
trailer reader.

## The answer

**A mission does not name a member at startup. It names an *animation*, and the
animation name is resolved to a member afterwards.** The chain is three measured
records:

```
zbd/<group>/<mission>/zrdr.zbd :: startanims.zrd
    NEW_GAME_START | LOAD_GAME_START  ->  ["<ANIMATION_NAME>", ...]
        |
        v
<archive> :: <member>.zrd :: ANIMATION_DEFINITIONS / ANIMATION_DEFINITION
    ANIMATION_NAME  ->  the animation this definition implements
    NAME            ->  the node names it drives            (1 104 definitions)
    NAME1           ->  (state name, node path) pairs       (429 definitions)
    ACTIVATION      ->  ON_CALL | ON_STARTUP | WEAPON_OR_COLLIDE_HIT
        |
        v
zbd/<group>/gamez.zbd :: object3d records  (the nodes those names select)
```

The `Acceptance` criterion of this task had two branches. The first — resolve the
members a measured mission requires into a `DeclaredWorldActorProgram` — is
**not** met and cannot be: `cs_content::world_actors` is declared-designed and a
definition record states no motion, no socket, no pickup, no tick rate and no
faction. Inventing them would be exactly the guess AGENTS rule 4 forbids. The
**second** branch is what this task delivers: the members resolve to a **measured
binding** — archive key, member name, the member's own `SourceSpan`, the object
selectors and the activation — and all **seventeen** field families that stand
between that binding and a declared world-actor program are **named** with their
own claim ids by `UnmeasuredFieldFamily`, each claiming the stored field names it
covers in `UnmeasuredFieldFamily::field_names()`.

## Measurements (production readers over the read-only installation)

### The startup event table

Every one of the **53** mission reader archives carries `startanims.zrd`, and
every one of them has the **same shape**: a one-element root holding a flat record
of exactly two fields, `NEW_GAME_START` and `LOAD_GAME_START`, each a list of
one-element name lists. Nothing else appears — no third event key in any mission.

| | value |
| --- | --- |
| mission scopes with a reader archive | 53 |
| of those, carrying `startanims.zrd` | **53** |
| distinct event keys | 2 (`NEW_GAME_START`, `LOAD_GAME_START`) |
| animation names fired, summed | **195** |
| distinct animation names | **71** |
| `LOAD_GAME_START` arity 0 / 1 / 2 | 29 / 21 / 3 missions |
| `NEW_GAME_START` arity | 1–16, mode 1 (20 missions) |

So the campaign's whole startup animation surface is **71 names**, not 195: the
same names recur across missions. `player_setup` fires on 42 of 53
`LOAD_GAME_START`s, `generic_intro` on 12 `NEW_GAME_START`s and
`pzep_engines_start` on 17.

The task description names `STARTUP_ANIMATIONS`; **no member of the
installation declares a record by that name.** The startup surface is the
`startanims.zrd` member's two event keys, and that is what the reader reads.

### The definition record

| | value |
| --- | --- |
| reader archives read (shared root + 8 world groups + 53 missions) | **62** |
| members declaring `ANIMATION_DEFINITIONS` | **446** |
| inline `ANIMATION_DEFINITION` entries | **1 533** |
| `ANIMATION_DEFINITION_FILE` references | **828** |
| distinct animation names declared | **918** |
| names declared by more than one definition | **49** |
| definitions naming their objects through `NAME` | 1 104 |
| … through `NAME1` | 429 |
| … through neither | 0 |
| … through **both** | 0 |
| definitions naming an object family with `*` | **428** |
| wildcard spellings carrying a character after the `*` | **39** (187 selectors: 32 spellings end in `**`, 7 narrow) |
| definitions with no `ACTIVATION` field | 183 |
| distinct fields stored and not interpreted | **17** |

`ACTIVATION`, over those 1 533 definitions: `ON_CALL` **1 215**, `ON_STARTUP`
**132**, `WEAPON_OR_COLLIDE_HIT` **3**, absent **183**. The three-value vocabulary
is a measurement of *this* installation, not a closed set, so `Activation::Other`
retains an unseen spelling and `UnmeasuredFieldFamily::ActivationVocabulary`
names the possibility of more.

`SEQUENCE_DEFINITION` is repeatable (1 to 42 sequences per definition) and its
statements name **38** distinct keys. The most used are `CALL_ANIMATION` (4 178),
`OBJECT_ACTIVE_STATE` (4 126), `ACTIVATION` (1 914), `CALL_SEQUENCE` (1 864),
`OBJECT_MOTION_FROM_TO` (1 398), `OBJECT_MOTION` (1 042), `PUFFER_STATE` (879),
`SOUND` (689), `LOOP` (668), `OBJECT_OPACITY_FROM_TO` (570),
`STOP_ANIMATION` (477), `INVALIDATE_ANIMATION` (429), `OBJECT_ROTATE_STATE`
(424), `IF`/`ENDIF` (401 each), `OBJECT_MOTION_SI_SCRIPT` (396).

**Two sequences of `ZBD/C4/M03/zrdr.zrd::zep_dock.zrd` end in bare authored
words** — `generic`, `crash`, `is`, `used` in one, and a second prose tail ending
in `instead` in the other. A flat key/value walk reads four words as two entries
whose "kind" is the first of each pair. The reader **keeps** them (the test pins
`generic`, `is` and `instead` as retained entry kinds), so a consumer sees a
statement list longer than the statements are. This is the concrete reason the
statement vocabulary is filed as measured-but-not-interpreted rather than as a
closed enumeration, and it is why the census asserts **38** kinds knowing that
three of them are prose.

### The seventeen fields a definition stores without this reader interpreting them

`DeclaredAnimationDefinition::uninterpreted_fields()` names them per definition;
the vocabulary is closed over the installation and each name is claimed by
exactly one family, which the acceptance tests check in both directions:

| stored field | definitions | family |
| --- | --- | --- |
| `RESET_TIME` | 1 509 | `reset_fields` |
| `RESET_STATE` | 648 | `reset_fields` |
| `SAVE_LOG` | 571 | `persistence_flags` |
| `LOCAL_NODES_ONLY` | 457 | `world_membership` |
| `ANIMATION_ROOT_NAME` | 208 | `animation_root_name` |
| `HEALTH` | 184 | `damage_coupling` |
| `NETWORK_LOG` | 128 | `persistence_flags` |
| `DAMAGE_SEQUENCE` | 102 | `damage_coupling` |
| `AUTO_RESET_NODE_STATES` | 96 | `reset_fields` |
| `PERSIST_LOG` | 62 | `persistence_flags` |
| `EXECUTION_PRIORITY` | 52 | `execution_order` |
| `EXECUTION_BY_RENDER` | 34 | `execution_order` |
| `AUTO_ADD_TO_WORLD` | 31 | `world_membership` |
| `EXECUTION_BY_RANGE` | 31 | `execution_order` |
| `PROXIMITY_DAMAGE` | 15 | `damage_coupling` |
| `COPY_NODE_DATA` | 7 | `damage_coupling` |
| `EXECUTION` | 1 | `execution_order` |

Three of these were **unnamed** in the first reading of this task, which the
review found and fixed: `ANIMATION_ROOT_NAME` (208 definitions — the root an
animation applies to, which is exactly the binding this task is about),
`AUTO_RESET_NODE_STATES` (96) and the single bare `EXECUTION` (1). The first now
has a family of its own; the other two join `reset_fields` and `execution_order`.
`ANIMATION_ROOT_NAME` in particular is the field a consumer would want next: it
names the root a definition applies to, and whether it is a node name, a node path
or a state — and how it relates to the `NAME`/`NAME1` objects it travels with — is
unmeasured, so nothing is resolved from it.

### The two object fields, and what they mean

`NAME` is a **flat list of node names** (`wv_tailhook`, `piratezep`, `litehouse`).

`NAME1` is a **list of pairs**: a state name bound to the node path that state
drives. Measured example, from `ZBD/C1C/M01/zrdr.zbd::wv_tailhook.zrd`:

```
NAME1 = [ "wv_hookup_lights", ["workersvoyagezep", "hookup_lights"] ]
NAME1 = [ "deploy_bwzep_rbroad1*", ["beowulfzep", "rbroad1*"],
          "deploy_bwzep_rbroad2*", ["beowulfzep", "rbroad2*"], … ]
```

This is the closest these records come to saying "this animation moves that
object", and it was not in the task's brief — it was found by reading `NAME1`'s
nesting rather than its text leaves, which is why the first draft of the reader
(in which `NAME1` was read as a flat name list) was rewritten. It is measured:
the field's entries alternate a text with a **list** in every one of the 429
`NAME1` definitions, and an odd entry count appears in none of them.

Note that the wildcard appears in the **state** name too
(`deploy_bwzep_rbroad1*`), not only in node names: the original authors families
of both.

`ACTIVATION_PREREQUISITE` is a nested pair, `[REQUIRED | OPTIONS, body]`, where
an `OPTIONS` body leads with `MINIMUM_TO_SATISFY` and its count. Its condition
is one of three measured names — `OBJECT_ACTIVE_LIST`, `OBJECT_INACTIVE_LIST`,
`ANIMATION_LIST` — and an object-list condition names **node paths**. One path
step is stored either as a bare text (`phut_healthy`) or as a one-element list
(`["panelleftb1"]`); both spellings occur and the reader accepts both, because
normalising them away would be the reader's choice and not the data's.

### `*` is a prefix wildcard over a node name

Two measurements make this a reading rather than a guess:

1. `*` occurs in **428** definitions' object names and in **no** stored scene-node
   name of `ZBD/C1C/gamez.zbd`, `ZBD/C2/gamez.zbd`, `ZBD/C5/gamez.zbd` or
   `ZBD/planes.zbd`. A node name cannot contain it, so it can only be the
   original's own wildcard.
2. The families it names exist, numbered, in the world container that places them:

| stored object name | first declared by | `ZBD/C1C` | `ZBD/C2` | `ZBD/C5` | `ZBD/C1` | `ZBD/C3` | `ZBD/C4` |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `g_engine*` | `ZBD/zrdr.zbd::spruce_destroy1.zrd` (also `spruce_destroy2.zrd`) | **0** | **8** | **8** | 0 | 0 | 0 |
| `aagun**` | `ZBD/zrdr.zbd::aa_gun.zrd` | 0 | 0 | 0 | 5 | 7 | 5 |
| `fuel_truck0*` | `ZBD/C1/zrdr.zbd::fueltruck.zrd` (also `ZBD/C2/M02::car_truck_pickford.zrd`, `ZBD/C3/M02::fueltruck.zrd`) | 0 | 1 | 0 | 2 | 1 | 0 |
| `lkgasbag0*` | `ZBD/zrdr.zbd::locklear_gasbag.zrd` | 0 | 0 | 0 | 5 | 0 | 0 |
| `maagun**` | `ZBD/zrdr.zbd::maa_gun.zrd` | 0 | 0 | 2 | 0 | 0 | 0 |
| `tcargun0*` | `ZBD/C4/zrdr.zbd::rockyxpress.zrd` (also `traingun.zrd`) | 0 | 0 | 0 | 0 | 0 | 3 |

A reader that treated `*` as part of a name would select **zero** everywhere; one
that treated it as a literal would select in every container. Only the prefix
rule gives the measured answer, and the rule is still this project's: it is
filed as `f20-anim.object-selector-wildcard-is-a-node-name-prefix`
(`Origin::Designed`), and `ObjectSelector::rule_provenance()` returns that
provenance so a reader can see which half is measured and which half is not.

A **zero is a measurement**, not a gap: `g_engine*` selecting nothing in C1C is
what says the spruce-goose engine family is not placed in that world. The reader
reports `Some(0)` for that and `None` only where a node-name table genuinely
cannot answer.

### … but only when nothing narrows it

The prefix rule is measured for a suffix of **wildcard characters**, which is the
empty suffix and a repeated `**`: the installation stores 39 spellings with a
character after their first `*`, and 32 of them end in `**` (`aagun**`,
`maagun**`, `leng**`, `reng**`, `noseballgun**`, …), which is the wildcard twice
and narrows nothing — `aagun**` above is one of them, and it resolves 5 / 7 / 5
across `C1`/`C3`/`C4` exactly as `aagun*` would.

The remaining **seven** carry a character that may: `lbroad*1`, `lbroad*2`,
`rbroad*1`, `rbroad*2` in `ZBD/zrdr.zbd::beowulf_broadsides.zrd` and
`docksteam*#`, `sstack*#`, `torch*#` in `ZBD/C5/zrdr.zbd::steam.zrd` — 31
selectors in all. A trailing `1` may pick one numbered sibling (`lbroad1`,
`lbroad2` and `lbroad3` all exist, so a prefix match over-selects) and a trailing
`#` may be the original's own second wildcard, whose meaning the corpus never
establishes.

The reader keeps the suffix (`ObjectSelector::wildcard_suffix()`) and reports such
a selector as `SelectorMatch::UnmeasuredSuffix` — no count at all — because
counting the prefix alone would be a silent over-selection wearing a
measurement's clothes. That gap is
`UnmeasuredFieldFamily::SelectorWildcardSuffix`
(`f20-anim.object-selector-wildcard-suffix-unmeasured`). None of the families
whose placement this document leans on is one of the seven, so no measured
relation above changes.

### M01's closure, end to end

`ZBD/C1C/M01/zrdr.zbd` is the archive `missions/bindings/M01.json` names. It
declares **12** members: `aiv.zrd`, `egen.zrd`, `location.zrd`, `map.zrd`,
`mis_anim.zrd`, `net.zrd`, `objectives.zrd`, `startanims.zrd`, `weather.zrd`,
`zeppelins.zrd`, `placezeps.zrd`, `wv_tailhook.zrd`.

Its `startanims.zrd` fires **6** animations on `NEW_GAME_START` and **1** on
`LOAD_GAME_START`. Reading the mission archive, its world group
`ZBD/C1C/zrdr.zbd` and the shared `ZBD/zrdr.zbd`, **all seven resolve to exactly
one definition** — none ambiguous, none unresolved:

| event | animation | archive | member | object |
| --- | --- | --- | --- | --- |
| `NEW_GAME_START` | `generic_intro` | `zbd/zrdr.zbd` | `generic_intro.zrd` | `camera1` |
| `NEW_GAME_START` | **`wv_hookup_state`** | **`zbd/c1c/m01/zrdr.zbd`** | **`wv_tailhook.zrd`** | **`wv_tailhook`** |
| `NEW_GAME_START` | `pzep_engines_start` | `zbd/zrdr.zbd` | `pirate_zep_nacelles.zrd` | `piratezep` |
| `NEW_GAME_START` | `wvzep_engines_start` | `zbd/zrdr.zbd` | `wv_zep_nacelles.zrd` | `workersvoyagezep` |
| `NEW_GAME_START` | `bszep_engines_start` | `zbd/zrdr.zbd` | `bswan_zep_nacelles.zrd` | `blackswanzep` |
| `NEW_GAME_START` | `call_add_jack` | `zbd/zrdr.zbd` | `passengers.zrd` | `apassengers` |
| `LOAD_GAME_START` | `player_setup` | `zbd/zrdr.zbd` | `player_setup.zrd` | `camera1` |

**Exactly one of the seven is mission-scoped**: `wv_hookup_state`, the mission's
own tailhook machinery. The other six come from the shared root archive, and
five of those six drive one of the mission's three capital ships. That is the
binding `VS-M01-RUNTIME` needed: the mission's world actors are named by the
shared zeppelin-nacelle members, and the mission's own `wv_tailhook.zrd` supplies
the one mission-specific machine.

Every one of those seven object names resolves to **exactly one** record of
`ZBD/C1C/gamez.zbd` (5 644 records). `camera1` is selected twice — by
`generic_intro` and by `player_setup` — which is why "seven animations, six
distinct objects" is the honest count.

**The spawn half.** `ZBD/C1C/M01/zrdr.zbd::placezeps.zrd` declares three
definitions with `ACTIVATION = ON_STARTUP` and objects `piratezep`,
`workersvoyagezep` and `blackswanzep` — the same three records the
`ON_CALL` startup animations drive. So M01's placed capital ships are named in
two places with one set of names, and the `ON_STARTUP` activation is the
placement statement. The shared and group archives also carry `ON_STARTUP`
definitions; their objects are placed elsewhere, which is why the resolution
test is scoped to the mission archive rather than to the whole closure.

### `player` and `pickup_cpilot` are not world records

`wv_tailhook.zrd` binds the states `wv_hookup_player` and `wv_unhook_player` to
the object `player`, and `wv_pickup_copilot` to `pickup_cpilot`. Neither name is
a record of any world container: both are records of `ZBD/planes.zbd` (3 317
records, 562 distinct names), the shared object/aircraft container, which also
holds 23 records named `healthy`. A world-container-only resolver would report
them as selecting nothing, and a resolver that fell back to the object container
for every unresolved name would invent a fallback this measurement does not
support. `WorldNodeNames` is therefore **one container, chosen by the caller**,
and the missing container is reported as a zero rather than patched.

## Design decisions

- **One archive, one scope, one order — and no winner.** `WorldActorProgramBinding`
  searches the mission archive, then the world group, then the shared root. That
  order decides which sites an **ambiguous** name reports and nothing else:
  `BindingResolution::Ambiguous` keeps every declaring site and
  `BindingResolution::single()` returns `None`. 49 of the installation's 918
  animation names are declared more than once (`speed_cue` in seven world
  groups, `cg1zep_engines_start` in two shared members), so a "first hit wins"
  resolver would have been wrong on real data. The three failures stay distinct:
  `resolved()`, `ambiguous()` and `unresolved()`.
- **The record name is checked, never assumed.** `startanims.zrd`'s first text is
  `NEW_GAME_START`, not a record name, so a reader that trusted the first key
  would file the startup table under a record it never declares. The definition
  reader checks that the root's first field really is
  `ANIMATION_DEFINITIONS` and refuses otherwise.
- **A sequence's `NAME` is its name, not a statement.** Every measured sequence
  states one and most state the same `callback_sequence`. Counting it as a
  statement kind would report a `NAME` "kind" that no animation system executes.
- **Every field is read or named.** `DeclaredAnimationDefinition::uninterpreted_fields`
  lists by name every field the reader stores and does not interpret
  (`RESET_TIME`, `RESET_STATE`, `SAVE_LOG`, `HEALTH`, `DAMAGE_SEQUENCE`,
  `AUTO_ADD_TO_WORLD`, `EXECUTION_PRIORITY`, …). There is no third case: a
  definition cannot carry a field that was silently dropped, and a test asserts
  both directions — the fields the fixture states and does not interpret, and that
  a fully-read definition names none.
- **A path is not matched on its last step.** `NAME1` paths and prerequisite
  paths need the container's parent/child hierarchy. `WorldNodeNames` carries
  only record names, so `resolve_path` returns `SelectorMatch::Path` with the
  stored path. Matching the terminal step on its own would report a hit in the
  wrong world; that is a refusal, not a default.
- **A gap's provenance is designed and locates nothing.**
  `UnmeasuredFieldFamily::absence_provenance()` is `Provenance::designed` with
  `source: None`, and `is_measured()` is `false` for every family: each one is a
  statement that something has not been established, so none can be reported as
  installation-derived. A test asserts that over all fifteen.

## Review record

**Implementer:** bunny-2 (task #632, session of 2026-10-04).
**Reviewer:** bunny-2 again, in a fresh session with no context from the
implementing one. **This is same-agent review and therefore not independent
evidence** — it is recorded as such here and in the `complete_review` notes
because AGENTS asks for a different agent instance or model on format and
mission-semantics work. What the review could do, it did: re-derive every corpus
number in this document from the original bytes with the production readers
rather than trusting the implementer's counts, and check each documented relation
against the code.

**What the review confirmed.** All nine of the original tests ran and passed
(including the four retail ones), and the measurements below held on
re-measurement: 53 mission reader archives and 62 reader archives in total, 195
startup entries over 71 distinct names, exactly the two event keys, 446
definition members, 1 533 definitions, 828 authoring-path references, 1 215 /
132 / 3 / 183 activations, 38 statement keys, 1 104 `NAME` and 429 `NAME1`
definitions with none carrying both or neither, 918 distinct animation names of
which 49 are declared more than once, 428 wildcard definitions, 1 to 42 sequences
per definition, `ZBD/planes.zbd` holding 3 317 records with 562 distinct names
(including one `player` and one `pickup_cpilot`), and all six rows of the wildcard
table below. The `NAME1`-is-pairs finding, the "a sequence's `NAME` is its name"
rule and the refusal to pick a winner for an ambiguous name all survived
re-reading.

**What the review fixed** (all in the task's owner paths, no protected path, no
reader refusal weakened):

1. **`selected_objects` truncated the answer.** It documented "the node names a
   resolved startup animation drives" but yielded `node_names().next()`, so a
   definition naming two objects reported one. It now yields every name. A caller
   counting a startup animation's actors no longer loses the ones behind the
   first.
2. **Three stored field families were unnamed**, which is a hole in the task's
   second acceptance branch: `ANIMATION_ROOT_NAME` (208 definitions),
   `AUTO_RESET_NODE_STATES` (96) and one bare `EXECUTION`. All seventeen
   uninterpreted field names are now claimed by exactly one family, and that is
   machine-checked: `UnmeasuredFieldFamily::field_names()` lists the stored
   names, a synthetic test checks the coverage without the installation, and the
   retail census checks the vocabulary against the corpus.
3. **A narrowing wildcard suffix was silently dropped.** Thirty-nine spellings
   carry a character after their `*` — 32 end in `**` (the wildcard twice, which
   narrows nothing and keeps the measured prefix rule) and seven carry a
   narrowing character (`lbroad*1`, `lbroad*2`, `rbroad*1`, `rbroad*2`,
   `docksteam*#`, `sstack*#`, `torch*#`, in 31 selectors). The reader read all of
   them up to the `*` and reported a count, applying the prefix rule to seven
   spellings the corpus never measures that way: `lbroad1`, `lbroad2` and
   `lbroad3` all exist, so a prefix match there over-selects silently.
   `ObjectSelector::wildcard_suffix()` now keeps the narrowing text,
   `WorldNodeNames::resolve` reports `SelectorMatch::UnmeasuredSuffix` instead of
   a guessed count, and the suffix has its own family and claim.
4. **A shadowed object field was silently dropped.** No definition of the
   installation states both `NAME` and `NAME1`, so the reader's choice of one was
   harmless — but a record that did would lose a shape *and* name neither field,
   which is exactly the silent drop the module promises never happens. The
   shadowed field is now named in `uninterpreted_fields()`.
5. **`read_prerequisite` returned `Result<Option<_>>` and never `None`,** which
   reads as "this prerequisite may be absent" on a function whose absence is
   already modelled by `DeclaredAnimationDefinition::prerequisite()`. It returns
   the value now.
6. **Documentation corrections** in this file: the prose tail is two sequences of
   `zep_dock.zrd`, not one; the wildcard table named a single declaring member
   for spellings several members declare; and the uninterpreted-field and
   activation numbers are now in tables above.
7. **A duplicate constant, found by the rebase.** `main` now carries
   `cs_app::animation::carrier` (the animation-carrier survey), which defines the
   same measured member name this module defined for itself, so the two
   `pub use` lists in `animation/mod.rs` collided and the crate did not compile
   after the rebase. `programs` now re-exports `carrier::STARTUP_MEMBER` instead
   of defining a second constant with the same value — one measured fact, one
   definition — and the duplicate re-export is gone. Nothing in this task's
   measurements changed.

The review also **added pins** for measurements this document recorded but no test
checked: 918/49 names and their ambiguity, `speed_cue` declared seven times, the
1 104/429/0/0 shape split, the activation counts including the 183 absent, 428
wildcard definitions, 1–42 sequences, the seventeen uninterpreted field names,
and both wildcard-suffix shapes (7 narrowing spellings in 31 selectors, 32
repeated-`**` spellings in 156). Two new synthetic tests cover the suffix and the
shadowed field, and one pins the field-name coverage against the families.

## Test inventory

| `accept_m01_lc_world_actors_` test | Covers | Fails when |
| --- | --- | --- |
| `the_startup_table_reads_two_events_and_names_no_member` | both events read and distinct; each event's names in order; an undeclared event answering `None` rather than an empty list; the flattened walk crossing the event boundary in stored order | the table stops being an event table, an absent event becomes an empty one, or the walk loses order |
| `a_definition_member_reads_objects_activation_and_statements` | the member and record identity; all three definitions; the `NAME` shape; an `ACTIVATION` holding no name read as **no** activation; the `NAME1` **state/path pair** with its terminal step; `ON_STARTUP` read rather than defaulted and an absent `ACTIVATION` read as absent; the `OPTIONS` prerequisite's requirement, count, condition and paths; the sequence name not counted as a statement; a statement's own field order; both sequences of a repeatable field; the uninterpreted-field list in both directions; `startup_definitions`, `sequence_kinds` and `definitions_of` | the object field stops naming which shape it used, a pair stops pairing, an activation is invented or defaulted, the prerequisite's nesting stops being read, `NAME` is counted as a statement, or a field is dropped instead of named |
| `a_resolved_binding_names_its_member_and_its_span` | a single declaration resolving with its archive, member, animation name and selector; the same name in two members staying **ambiguous** with both sites in search order and no winner; a name nobody declares staying **unresolved**; the three failure lists staying distinct; the byte span's offset and member key; the `ON_STARTUP` declaration reported as a placement statement; `selected_objects` contributing only resolved bindings | the resolver picks a winner, collapses ambiguous into resolved, loses the span, or drops the event order |
| `a_wildcard_selects_a_prefix_and_a_path_is_left_unmeasured` | a literal name selecting exactly one record; a prefix without `*` compared whole; `aagun**` selecting three; the wildcard's segment and its designed claim id/provenance; a **zero** reported as a measurement; a node path reported unmeasured with its stored spelling; a wildcard path step still unmeasured; the empty, prefix-less and empty-step selectors refused | `*` is treated as a literal, a path is matched on its terminal step, a zero becomes a gap, or a degenerate selector is accepted |
| `every_unmeasured_field_family_is_named_and_never_measured` | all seventeen families with unique labels and **unique claim ids**, a validated `ClaimId`, a designed `absence_provenance` with no source, `is_measured() == false`, a reason longer than a label and a usable content id; the statement, state-binding, stored-unit, wildcard-suffix and root-name claims named individually | two families start sharing a claim id, a gap is filed as installation-derived, or a family loses its reason |
| `every_uninterpreted_field_name_is_claimed_by_a_family` | the seventeen stored-but-uninterpreted field names each claimed by at least one family, each family's own `field_names()` list, the four field-less families claiming nothing, and the exact owner set of `NAME` (the two wildcard readings) and `NAME1` (both wildcards, the node path and the state binding) | a stored field becomes unnamed because a family stopped covering it, a family claims a field the reader reads in full, or two families start overlapping where none should |
| `a_wildcard_suffix_is_kept_and_reported_unmeasured` | `lbroad*1`'s narrowing suffix kept and reported `UnmeasuredSuffix` with no count; `docksteam*#` likewise; a repeated `**` narrowing nothing, so `aagun**` counting exactly what `aagun*` counts; a bare name carrying no wildcard text; the suffix family's own claim id and a measured spelling in its reason | a narrowing suffix is applied or dropped, or a repeated `**` is reported as unmeasurable |
| `a_shadowed_object_field_is_named_not_dropped` | a definition carrying both `NAME` and `NAME1`: the last stored field is the shape the reader keeps, and the shadowed field is named in `uninterpreted_fields()` instead of being dropped | a second object field is silently discarded, which is exactly the drop the module promises never happens |
| `retail_m01_startup_animations_resolve_to_their_members` (retail) | `ZBD/C1C/M01/zrdr.zbd`'s 12 members; the three searched archives in order; the startup member present; 7 bindings, 6 on `NEW_GAME_START` and 1 on `LOAD_GAME_START`; **zero** unresolved and zero ambiguous; all seven archive/member pairs; the one mission-scoped member (`wv_tailhook.zrd`); the three `placezeps.zrd` placements naming the same three zeppelins; every placement's `ON_STARTUP` | a binding stops resolving, a member moves, the mission-scoped member changes, or a placement is lost |
| `retail_the_selected_objects_resolve_in_the_c1c_world` (retail) | `ZBD/C1C/gamez.zbd`'s 5 644 records; each of the seven selected objects resolving to **exactly one** record, in binding order; the three mission placements resolving too | an object stops resolving, resolves twice, or the placement scope widens to the shared closure |
| `retail_a_wildcard_resolves_only_where_its_family_is_placed` (retail) | `g_engine*` declared by the shared root; resolving 0 in C1C and 8 in C2 and C5; the rule being prefix matching and nothing else; no stored node name of C1C, C2, C5 or `ZBD/planes.zbd` carrying `*` | the wildcard resolves where the family is absent, or a node name carries the character |
| `retail_the_startup_and_activation_vocabulary_is_measured` (retail) | 62 reader archives and 53 mission scopes; 53 startup members; exactly the two event keys; 195 entries and 71 distinct names; exactly the three activation values in their 1 215 / 132 / 3 / 183-absent buckets; 38 statement kinds including the three authored-prose entries; 446 definition members, 1 533 definitions and 828 authoring-path references; 428 wildcard definitions; 918 distinct declared names with 49 declared twice and `speed_cue` seven times; the 1 104 / 429 / 0 / 0 object-shape split; 1–42 sequences per definition; the seventeen stored-but-uninterpreted field names with a family claiming each; both wildcard-suffix shapes (7 narrowing spellings in 31 selectors, 31 `**` spellings in 156); an unseen activation retained and never folded into a measured one | any measured count moves, a value outside the three is refused instead of retained, a definition member that cannot be read is skipped instead of failing, a non-definition member is read as one, or a stored field loses its family |

**Sensitivity.** Eight mutations were applied to `programs.rs` by the implementer
and five more by the review, the non-retail selection (`--skip retail`) was
re-run and the source restored each time. **All thirteen are killed by a test CI
can run**, so none of them relies on the `#[ignore]`d cases:

| mutation | killed by |
| --- | --- |
| a sequence's own `NAME` is counted as a statement | the definition test's statement count and `sequence_kinds` |
| `NAME1` is read as a flat name list instead of state/path pairs | the definition test's `StateBindings` arm |
| an `ACTIVATION` that holds no name becomes an activation | the definition test's nameless-activation assertion |
| an absent `ACTIVATION` defaults to `ON_STARTUP` | the definition test and the binding test (the definition becomes a placement statement) |
| the prerequisite is read as a `/`-joined path instead of the nested shape | the definition test's prerequisite assertions |
| the resolver takes the first hit instead of reporting ambiguity | the binding test's ambiguity assertion |
| a node path is resolved on its terminal step instead of refused | the wildcard test's `is_unmeasured` assertion |
| an empty object selector is accepted | the wildcard test's three refusal assertions |
| `selected_objects` yields only the first node name | the binding test's `selected_objects` and the retail C1C world's seven selected objects |
| a narrowing wildcard suffix is applied as a prefix | the wildcard-suffix test's `UnmeasuredSuffix` assertions |
| a repeated `**` is reported as unmeasured | the wildcard-suffix test's `aagun**` count and its equality with `aagun*` |
| a shadowed second object field is dropped instead of named | the shadowed-field test's `uninterpreted_fields` |
| a family stops claiming a stored field | the field-coverage test's `UNINTERPRETED` loop |

The behaviours only the retail cases check are the corpus counts themselves
(1 533 definitions, 195 startup entries, 71 names, 918 declared names, 38
statement kinds, the seven M01 bindings, the seventeen field names); every
**rule** above is covered without the installation.

## Unknowns and limitations (recorded, not guessed)

- **The members do not resolve to a `DeclaredWorldActorProgram`, and this task
  does not make them.** A definition record states no kind, no motion, no
  sockets, no pickups, no faction and no tick rate. All seventeen families are
  named with claim ids and reasons. **Affected content:** every claim that M01's
  world actors move, spawn or carry anything. **Resolving task:** F34-D (#150),
  which needs the statement semantics and #436's unit first.
- **No statement is interpreted.** `SEQUENCE_DEFINITION` names 38 statement keys
  and this reads none of their payload, so no pose, route, spawn or gameplay
  transition is recovered. `OBJECT_MOTION_FROM_TO`'s `ROTATE_FROM`/`ROTATE_TO`/
  `RUN_TIME` and `OBJECT_MOTION_SI_SCRIPT` are the fields a route would come
  from and both are unmeasured, as is the unit of `RUN_TIME`.
  **Affected content:** every F34 route and every F20 transform claim.
  **Resolving task:** the F20-C/F20-D statement work.
- **`NAME1` state bindings are read, not resolved.** The pairing
  (state name ↔ node path) is measured; whether the engine resolved it by name at
  load time or by a build-time reference, and whether the state name is itself
  addressable, are not. **Affected content:** every claim that a *named* state
  moved a *named* object at runtime. **Resolving task:** the same statement work.
- **A node path is not resolved.** `NAME1` paths and prerequisite paths need the
  container hierarchy; `WorldNodeNames` carries record names only. **Affected
  content:** the 429 `NAME1` definitions and every prerequisite's node list.
- **Wildcard matching is a rule, not a measurement.** Filed as `Origin::Designed`
  with its own claim id. **Affected content:** every claim that a family of
  objects was addressed as one. **Resolving task:** an original run, or the
  original's own source.
- **A narrowing wildcard suffix is unmeasured.** Seven spellings carry a character
  after their `*` that is not itself a wildcard (`lbroad*1`, `rbroad*2`,
  `docksteam*#`, …), so no count is reported for them at all.
  **Affected content:** the 31 selectors using those seven spellings — the
  beowulf/bhat/bswan/dante/vostok broadside families and three C5 steam-family
  selectors. **Resolving task:** the same original-run or source evidence as the
  prefix rule.
- **`ANIMATION_ROOT_NAME` is unmeasured.** 208 definitions store the root an
  animation applies to; whether it is a node name, a node path or a state, and
  how it relates to the `NAME`/`NAME1` objects, is unknown, so no root node is
  resolved. **Affected content:** every claim about what an animation is rooted
  on. **Resolving task:** the same statement work.
- **`ACTIVATION` may have values outside the three measured.** An unseen value is
  retained by spelling, never refused and never folded into `ON_CALL`.
- **The statement vocabulary is not closed.** Two sequences carry authored prose
  that reads as two entries each; the reader keeps them.
- **`ANIMATION_DEFINITION_FILE` paths resolve to no container.** They are the
  original's authoring paths (`..\data\common\zrdr\planes\player.zrd`); the
  basenames match member names, the directories are not installation-relative.
- **Placement members are not read.** `zeppelins.zrd`'s
  `node`/`position`/`yaw`/`pitch`/`max_speed`/`max_accel` are F33-D's
  undecoded carrier; `position`'s unit is #436's measurement. **Affected
  content:** every spawn position and route of a placed actor. **Resolving
  tasks:** #436, and a task that decodes the placement member.
- **Only C1C/M01 is resolved by a test.** The other 52 scopes are censused (the
  two event keys, the three activation values, the statement keys) but not
  resolved member by member. **Affected content:** the per-mission bindings.
  **Resolving task:** the F50 per-mission closure.
- **Evidence class.** The layout, the counts and the relations above are
  `ObservedTool` + measurement: they were read out of the original bytes by the
  production readers. Every **rule** carries its own claim id. No original
  executable was run: `retail` is file access, not evidence of runtime
  behaviour. The review recorded above was **same-agent** (bunny-2 reviewing
  bunny-2's own work, in a fresh session), so it is **not independent evidence**
  and a different agent instance or model should still review this format work.
  No agent review replaces the owner's approval.
- **Nothing derived from the original bytes is committed.** The numbers above are
  counts, dimensions and relations; no member payload, no name list beyond what a
  reviewer needs to check a binding, and no screenshot is in the repository.

## Follow-ups filed

Not filed as tasks: the remaining questions above name their resolving tasks and
are recorded here, because filing them would duplicate work those tasks already
own. The one genuinely new dependency this task exposes is the statement
semantics behind `OBJECT_MOTION_FROM_TO` / `OBJECT_MOTION` /
`OBJECT_MOTION_SI_SCRIPT`, which F34-D needs and no stage owns yet.

## Sources used

- `crates/cs_content/src/stunts.rs` (`decode_zrd`, `zrd_flat_fields`, the
  `ZRD_TAG_*` grammar F09/#463 measured) and `objectives.rs` (the flat-record
  reading rule and why a shape-agnostic walk invents keys).
- `crates/cs_formats/src/script_raw/discovery.rs` (`discover_container`,
  `ProgramLocator`, `mission_scope`) and `crates/cs_formats/src/zbd/trailer.rs`
  (the version-one member index).
- `crates/cs_formats/src/gamez/nodes.rs` (`read_gamez_nodes`, `RawNode::name`) and
  `crates/cs_app/src/world/retail.rs` (the production world-container read).
- `docs/findings/2026-10-02-f20-d-animation-family-validation.md` (the carrier
  validation this task's sibling measured) and
  `docs/findings/2026-10-02-t464-stunt-reward-and-repeat.md` (the observation
  that the shared reader carries `player.zrd` twice, once under an unrelated
  `ANIMATION_DEFINITIONS` record — the reason an animation name is not assumed
  to be unique).
- `docs/findings/2026-10-04-m01-lc-world-import.md` and
  `2026-10-04-m01-lc-world-scene-ids.md` (the world container's records and their
  stored names).
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, `Provenance`, evidence
  classes).

## Commands run

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_m01_lc_world_actors_ --include-ignored
#   9 tests: 5 run and pass, 4 retail run and pass (about 4 minutes; each does
#   one production discovery pass over the installation)
```