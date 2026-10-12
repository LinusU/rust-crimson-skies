# T575: pairing the session `ActorId` with the script `ActorId`

Date: 2026-10-12. Task #575 (follow-up of F33-C/F33-D). Test prefix
`accept_t575_` in `crates/cs_app/tests/accept_t575_script_actor_binding.rs`.
Capability: `retail` for the measurement (read-only), ordinary build/test for
the rest. No `verified_original` claim.

## Measured: the original's records name actors by text

`survey_mission_control_programs` over the owner's installation (install
fingerprint `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`)
gives the argument shapes of every directive key of the measured control
records. The keys that name placed actors carry **text only**, with no integer
in any shape:

| key family | sites | shape |
| --- | ---: | --- |
| `ADD_OBJECTIVE_TARGET` | 76 | text names / lists of text names |
| `REMOVE_OBJECTIVE_TARGET` | 153 | text names / lists of text names |
| `ADD_OTHER_TARGET`, `REMOVE_OTHER_TARGET` | 16, 27 | text names |
| `INACTIVE<n>` | 1335 | 1-3 text names |

Every integer the other keys carry is an objective index
(`WAKE_OBJECTIVE_WHEN_I_COMPLETE`, `KILL_…`, `TICK_DEPENDS_ON_OBJ`), a count,
a flag or a coordinate. **No retail record carries a numeric actor id**, so
`cs_script::ir::ActorId(u32)` is an engine-side key, not an original byte.

## Defined: the pairing is declared by the host that places the actor

`cs_app::roster::ScriptActorBindings` pairs one session-qualified
`cs_sim::damage::ActorId` with one `ir::ActorId`, optionally recording the
authored text name it was placed under. It refuses a foreign session
generation and a duplicate on either side, and is per session (a retry builds
a new table). `translate` turns the F33-C `AllyConsumerLog` callbacks into a
`ScriptActorBatch` (`registered`, `lifecycles`, `unbound`) which feeds
`MissionSession::advance_observed` unchanged. `lifecycle_of_ally_event` is the
total inverse of `cs_sim::allies`' classification (the three loss kinds are
all `Destroyed`). A callback for an unbound actor is returned in `unbound`,
never dropped or given an invented id. A bailout reaches the script side as
`PilotBailout`, which writes no `ActorState` (F37-E1).

This is a **designed** pairing, not a measured one.

## Unknowns (not guessed)

- How the original resolves a text name to an object is measured only for the
  record spelling; which objects a name binds to (and so which names the host
  must pass to `bind`) is the placement/lowering task's input, not read here.
- `TRAVELERS` leads with an integer at 6 of 75 sites
  (`[int,text,…]`); its meaning is unmeasured and it is **not** read as an
  actor id.
- Whether the original keys any runtime table by a numeric actor handle
  (the native object pointer is not an id in a record) is unmeasured.
- Nothing yet calls `bind` from a spawn path: the mission host that places
  actors must declare each pairing. This is follow-up work for the task that
  wires placed actors into the session.
