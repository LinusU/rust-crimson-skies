# F29-C: wiring the damage consumers

Date: 2026-10-02. Task: F29-C "Wire destruction visuals, debris, scoring and
bailout" (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, section
`### F29-C`). Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no
render, no audio, so no `private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/damage/resolver.rs` (extend): the read accessor
  `DamageResolver::graph`, so a consumer maps a part identity to what it
  drives without a second copy of the graph.
- `crates/cs_app/src/damage.rs` (extend): the F29-C consumer seam —
  `apply_damage_state`, the `DamageConsumerEvent` / `DamageConsumerRefusal` /
  `DamageConsumerLog` / `DamageConsumerReport` / `DamageConsumerOutcome`
  records and the private `apply_visual` / `apply_mount` helpers. Module docs
  updated.
- `crates/cs_app/tests/accept_f29_c_damage_consumers.rs` (**new**): 8
  `accept_f29_c_*` tests over the declared → lowered → resolver → consumer
  path.
- This file.

No protected path, no `cs_content` change, no original data, no binary.

**One observable failure, before the change:** F29-A/F29-B made the resolver
the authority — `PartState`, `SystemState` and the `SystemDisabled` /
`PartTransition` events — but nothing consumed that authority. A destroyed
weapon mount left `FireResolver`'s per-mount disabled set untouched, so the
mount kept firing through its own wreck; and the visual damage record
(`AirframeDamageState`) never heard about the destruction, because the
resolution produced events a scene pass would have had to replay. A gate
reading the event stream has to reconstruct "is this part destroyed *now*"
from history, which drifts after a reload and cannot answer an unresolved
pool.

## The designed behavior

`apply_damage_state(resolver, actor, fire, visuals) -> DamageConsumerOutcome`
is a **state-driven** pass, not an event replay. For every node of the actor's
registered graph it reads the resolver's authoritative `part_state` and
rewrites the two consumers the sheet's minimum scenario names:

| node state | visual consumer (`AirframeDamageState`) | weapon gate (`FireResolver`) |
| --- | --- | --- |
| `Destroyed` | records the part destroyed | disables the carrier's mount |
| `Intact` / `Damaged` | repairs the part if it was destroyed | re-enables the mount |
| `Unknown` (unresolved pool) | asserted nowhere; refused by name | asserted nowhere; refused by name |

This is F29 non-negotiable 1 ("Damaged visuals consume authoritative state"):
the visuals follow the resolver, never a particle or material, and never the
event history. It also makes the pass **convergent** — running it twice, after
a reload, or against a stale consumer changes nothing the second time. A
stale disable the authoritative state disagrees with is cleared, which is the
retry/teardown behavior the stage asks for; a foreign session generation is
refused instead of applied.

A destroyed part with no scene binding is simply skipped (no visual node to
present). A declared-but-unresolved scene binding
(`Resolved::Unknown` / `Some(Resolved::Known)` holding a non-`scene_node`
id) is refused with its claim; the visual consumer never guesses a scene node.
A destroyed `SystemKind::Weapon` carrier whose actor has no weapon state is
refused once with `UnarmedActor`, and one whose mount carries no gun is
refused with `UnmountedMount`; neither writes a disable for a gun that does
not exist. Every refusal is returned in `DamageConsumerLog`, the same
error-propagation channel `apply_damage_events` uses for colliders.

Only `SystemKind::Weapon` is wired here. `Propulsion`'s consumer is the
flight-authority gate (`cs_sim::flight`), and debris, scoring
(`DamageEventKind::KillAwarded`) and the bailout mission transition
(`LifecycleKind::PilotBailout`) need consumers that live *outside this task's
owner paths* — `cs_sim/src/net_state.rs`, `targeting.rs`,
`objectives/counters.rs`, `cs_sim/src/flight` and the scene/debris pass. They
are filed as follow-ups rather than guessed (see below).

**Designed behavior, not original data.** Whether the original disabled
firing from a destroyed mount, whether a mount's wreck kept any visual state,
and what consumers debris, scoring and bailout had are **unmeasured** (F29
"Research boundary"; the manual and guides name no damage equations). The
rule here — a destroyed weapon carrier's mount leaves the firing gate and a
destroyed part's scene node is presented destroyed — is this engine's
designed behavior. **Affected content:** every airframe and world object whose
graph declares a weapon carrier or a scene binding. **Resolving tasks:** F29-D
keeps the original-family gate; nothing here claims an original behaviour.

## Tests

Every test drives production code — `lower_graph` / `lower_policy`, the real
`DamageResolver`, `cs_app::weapons::lower_gun`, `FireResolver` and
`AirframeDamageState` — never a test-only bridge.

| test | what it pins |
| --- | --- |
| `accept_f29_c_destroying_a_mount_disables_firing_and_updates_its_visual_state` (minimum) | destroying `gun_mount_1` disables the gate and records the visual once; a `FireIntent` on the disabled mount is refused with `MountDisabled` and consumes no round; an intact actor is a no-op |
| `accept_f29_c_a_scratched_mount_still_fires_and_keeps_its_visual` | `Damaged` is not `Destroyed`: the gate stays open, nothing is presented destroyed, and the shot really consumes a round |
| `accept_f29_c_the_consumer_pass_is_convergent_and_re_enables_a_stale_disable` | a stale disable is cleared by the authoritative state; applying the same destroyed state twice changes nothing the second time |
| `accept_f29_c_a_destroyed_engine_updates_its_visual_but_not_the_weapon_gate` | the engine's own visual is destroyed, the gun is untouched and still fires — each carrier drives only its own consumer |
| `accept_f29_c_a_foreign_or_unknown_actor_is_refused_and_changes_nothing` | a foreign session generation and an unregistered actor are refused by name, no consumer changes |
| `accept_f29_c_an_unarmed_actor_and_an_unmounted_carrier_are_reported` | no weapon state → `UnarmedActor` once (not per part); a graph carrier the gate has no gun for → `UnmountedMount`, not a silent disable |
| `accept_f29_c_unresolvable_visual_bindings_are_refused_not_guessed` | `Resolved::Unknown` and non-`scene_node` bindings are refused with their claim/id; nothing is guessed into the visual record |
| `accept_f29_c_an_unresolved_pool_asserts_neither_visual_nor_mount` | an unresolved integrity is refused by name; neither the visual nor the gate is asserted, and the gun still fires |

## Mutation probes

Each probe edited one production line, ran the F29-C selection, recorded the
failing tests and restored the file from a saved copy:

| probe | edit | result |
| --- | --- | --- |
| the gate ignores a destroyed mount | `apply_mount`'s `if destroyed` disabled via `if false && destroyed` | `accept_f29_c_destroying_a_mount_disables_firing_and_updates_its_visual_state` and `accept_f29_c_the_consumer_pass_is_convergent_and_re_enables_a_stale_disable` failed (`mounts_disabled` 0) |
| the visuals ignore a destroyed part | `apply_visual`'s `if destroyed` disabled via `if false && destroyed` | the minimum, the engine-visual test and the convergence test failed (`visuals_destroyed` 0) |

## Follow-ups left open (filed with `create_tasks`)

- **Propulsion consumer.** A destroyed `SystemKind::Propulsion` carrier should
  reach the flight-authority gate (`cs_sim::flight`). Its consumer lives
  outside this task's owner paths, so it is a follow-up, not guessed here.
- **Debris.** A destruction should spawn authored debris/wreck visuals; the
  debris pass and its asset bindings are another stage's.
- **Scoring.** `DamageEventKind::KillAwarded` needs its score/reward consumer
  (progression/reward transactions), outside this stage's owner paths.
- **Bailout mission transition.** `LifecycleKind::PilotBailout` needs its
  distinct mission transition (`objectives` / mission removal); F29-D
  validates it against original data and requires `retail`.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-C`,
  AC03), `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-10-02-f29-b-zones-armor-and-system-disablement.md`
  (the queryable `PartState` / `SystemState` this pass reads, and its
  "Consumer wiring" follow-up).
- `docs/findings/2026-10-02-f29-damage-zone-collider-call.md` (the F29-B/C
  collider bridge whose refusal-log pattern this pass follows).
- `crates/cs_sim/src/damage/{resolver,graph,events}.rs`,
  `crates/cs_sim/src/weapons/guns.rs`,
  `crates/cs_app/src/damage.rs`, `crates/cs_app/src/scene.rs`.
