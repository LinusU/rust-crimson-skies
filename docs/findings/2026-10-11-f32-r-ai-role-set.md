# F32-R: the original's AI role set — searched and empty; the seven roles are enforced as designed vocabulary

Date: 2026-10-11. Task: #569 `F32-ROLES`. Follows
`2026-10-03-f32-d-original-ai-roles-and-difficulty.md`. Capabilities: `retail`
(read access to the installation) and the owner-supplied decrypted executable
named by `$CS_ENGINE_IMAGE` (read-only). No original run.

## Searched

* Installed data: the engine's identifier lists (`ASSETS/SCRIPTS/DEBUGINFO.TXT`,
  `ASSETS/SCRIPTS/RESOURCE.H`, read through the production ROF mount) and the
  key vocabulary of the eight scenario descriptors. The other 59 `.SCRIPT`
  members are interface scripts. None contains a role spelling or a key
  containing `role` or `behav`.
* The executable image (SHA-256 `43540fc9…37d75`, the image every
  `M01-LC-DIRECTIVE-*` finding cites; 2 580 480 bytes), case-insensitive
  byte search. Zero occurrences of: `escort`, `intercept`, `retreat`,
  `torpedo run`/`torpedo_run`, `bomber run`/`bomber_run`, `bombing`,
  `fighter attack`/`fighter_attack`, `withdraw`, `evasion`, `role`.
  `evade` occurs, as a runtime AI mode label and as a maneuver name.

## Measured (counts only, no text committed)

* The image carries an AI debug overlay with a per-vehicle **mode** line:
  nine mode labels (`ORIGINAL_AI_MODE_LABEL_COUNT`), beside four `N/A`
  placeholders. They describe a runtime state — crash avoidance, an evasive
  maneuver step, danger-zone approach and navigation, being stunned,
  evading, pursuing, laying off and patrolling — not a mission-facing
  classification. Their strings start at file offset 2 228 704.
* The same overlay names five vehicle dynamics classes, and a block of seventeen
  maneuver names precedes it (just before the overlay's text).
* The node format comment inside the image (cited by
  `2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`) lists
  per-AI-vehicle fields including `primary_target`, `pattern`,
  `signature_maneuvers` and nine pilot attributes, none of them a role.

## Conclusion

The seven roles (fighter attack, bomber run, torpedo run, escort,
interception, evasion, retreat) are not measured original vocabulary. The
original's closest structure is the mode machine above, which has no
bomber/torpedo/escort/intercept/retreat state. Whether a mission-facing role
exists in compiled code that does not spell it is **unknown**; the image was
searched for strings, not disassembled for a role table.

## What changed

`cs_content::ai`: `ORIGINAL_AI_ROLE_SET_MEASURED = false`;
`DeclaredRoleProfile::try_new` refuses a profile with an installation origin
(`RoleClaimsOriginalOrigin`) or with provenance that is not designed
(`RoleProvenanceNotDesigned`); `DeclaredCombatRules::try_new` refuses a record
that holds roles but carries non-designed provenance (`RulesRolesNotDesigned`).
`cs_sim::ai::combat`: the mirrored flag and `CombatRole::original_counterpart_measured`
(always `false`). The `f32.d.limit.ai_roles` entry in `UNRESOLVED` is updated.

Tests: `accept_f32_r_*` in `cs_content::ai` (five synthetic including the three
failure cases and a cross-crate agreement check, one retail test needing
`CS_GAME_DIR` and `CS_ENGINE_IMAGE`) and in `crates/cs_sim/tests/ai/`.

## Open

* Disassembly of the mode machine's consumer, to see whether a mission-facing
  role exists unnamed: needs executable analysis.
* The same nine-attribute list may name the nine ace stat slots (task #568
  recorded the image as unsearched); filed as a follow-up.
