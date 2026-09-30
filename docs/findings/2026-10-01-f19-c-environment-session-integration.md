# F19-C: environment session integration (task #90)

Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage `### F19-C`,
AC03. Code: `crates/cs_app/src/environment/session.rs`; tests:
`crates/cs_app/tests/environment/session.rs` (`accept_f19_c_`).

## What was wired

`EnvironmentSession` owns the authored definition, one `EnvironmentClock` and
the run's `RunSeeds`.

* **Wind:** `wind()` / `flight_environment(base)` read the state the clock
  installed at its last tick and delegate to `AuthoritativeWind`, which uses
  the single conversion in `cs_sim::environment` (task #434). An explicit
  unknown wind is returned as `WindUnavailable::Unknown`; there is no
  still-air fallback.
* **Sight range:** `sight_range_m()` returns the authored gameplay visibility
  or `VisibilityUnavailable::Unknown { claim_id, reason }`. It never reads fog.
* **Randomness (AC03):** `RunSeeds` keeps the mission seed and the cosmetic
  weather seed apart; `mission_ai_stream()` is
  `SplitMix64::for_domain(mission_seed, AI_NAVIGATION_DOMAIN)` and takes no
  weather input. The test interleaves AI draws with weather ticks, effect
  resolution and particle generation under six weather seeds, including one
  chosen so the cosmetic stream equals the AI stream.
* **Retry:** `restart()` rebuilds the clock from the definition at tick zero.

## Not claimed / unknown

* No AI code consumes the sight range yet. `AI_NAVIGATION_DOMAIN` is still the
  F31-A reserved tie-break stream, and `Navigator::decide` draws nothing. The
  AC03 test therefore pins the stream derivation, not an AI behavior; F31 must
  keep its draws on `RunSeeds::mission_ai_stream`.
* No projectile consumer exists in `cs_sim` (F27-B); `ProjectileMotion` still
  reads `AuthoritativeWind` directly.
* No original wind, visibility or weather data was read. Synthetic fixtures
  only; AC04 stays with F19-D.
