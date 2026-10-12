# F37-C-FU1: evaluator seed path and reward catalog

Both items are engine designs. Nothing here is measured from the original game.

## Seed path, end to end

1. The run's root seed: `cs_app::environment::RunSeeds::mission_seed`, held by the
   stage's `EnvironmentSession`. It is derived from the installation fingerprint
   (`root_seed_from`) or `--seed`, never from ambient entropy.
2. `cs_script::runtime::evaluator_seed(root, mission_id)`: the root xor the 64-bit
   FNV-1a of the mission's content id text, so two missions of one run draw
   independent streams.
3. `MissionState::new(program, session, root_seed)` stores that seed; the stream is
   `SplitMix64::for_domain(seed, "MSN_EVAL")`.
4. `MissionStateSnapshot::seed` (snapshot version 3) carries the seed and
   `rng_draws` the stream position; a restore rebuilds the stream and replays the
   draws (`MAX_RNG_REPLAY_DRAWS`).

The session generation no longer feeds the stream. `MissionSession::launch` and
`launch_with_countdown` keep the generation counter as their root for synthetic
callers that have no run seed; `MissionHost` uses `launch_seeded`. A restart under
the same root seed replays the same stream by design.

## Reward catalog

`MissionHostSeed::rewards` is the catalog `MissionHost` passes to the session.
`cs_app::campaign::mission_reward_catalog` resolves it from the unlocks of the
edge grants of the declared campaign nodes bound to the mission. No campaign is
declared for an original mission yet (F43-D), so `stage_for` carries an empty
catalog: a `GrantReward` in an original control program surfaces as
`HostFault::UnknownReward` instead of being granted.

## Limitation

Whether the original granted rewards through campaign edges, and what ids it
used, is unknown; the catalog source is a design until F43-D measures it.
