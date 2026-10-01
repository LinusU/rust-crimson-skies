# F55-A: lobby state and revision protocol — design notes and what stays open

Date: 2026-10-02. Task: F55-A "Define lobby state and revision protocol"
(`specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`). Ordinary
build/test only. Everything is newly authored engine design; no original
multiplayer option, scenario id or rule is known or claimed.

## Files and the observable failure

- `crates/cs_net/src/lobby.rs`: `Lobby` (host-authoritative record),
  `LobbyRules` + `Revision` + `RulesDigest`, `HostAction` / `PeerRequest` /
  `LobbyCommand`, readiness bound to a revision (`Ready`), `ReadyRevocation` /
  `RevokeReason`, `Lobby::admit` with distinct `JoinError`s, two-step
  acknowledged launch (`begin_launch` / `acknowledge_launch` / `cancel_launch`),
  `finish_match`, `remove_peer` + `HostLoss`, bounded `Callsign`/`ChatText`,
  redacted `Secret`, and the `LoadoutValidator` hook.
- `crates/cs_app/src/ui/lobby/mod.rs`: `LobbyView`, the client projection of
  host events into `Notice`s (reason display, mute list, bounded notice list).
- Tests: `crates/cs_net/tests/accept_f55_a_lobby_protocol.rs` (15),
  `crates/cs_app/tests/accept_f55_a_lobby_ui.rs` (2).
- Wiring edits: `pub mod lobby;` plus a doc paragraph in `cs_net/src/lib.rs`;
  `pub mod lobby;` in the `ui` module of `cs_app/src/lib.rs`.
- Observable failure without the implementation: nothing revokes readiness,
  so after the host bans a weapon a ready client whose loadout carries it
  stays ready and can launch (`accept_f55_a_host_ban_revokes_ready_and_
  reports_reason`).

## Decisions

- **Revision semantics.** Scenario / team-mode change revokes everybody; a ban
  revokes only members carrying the component (it only narrows the rules, so
  other loadouts stay valid and their readiness is carried to the new
  revision); an unban or late-join change revokes nobody. A command that does
  not change the rules does not bump the revision.
- **Ready names the revision it acknowledges**; `SetReady` on an old revision
  is `StaleRevision`, so a client racing a ban cannot ready into unseen rules.
- **Launch is bound to revision and digest** and acknowledged by every other
  member; the last ack commits. Rules/readiness are locked while pending; a
  leaving member or host cancel returns to gathering.
- **Access and capacity are outside the revisioned rules** (`Admission`):
  changing them cannot invalidate a loadout.
- **Join check order**: password, compatibility (`evaluate_hello`), phase/late
  join, capacity, callsign. A refused join allocates no peer id.
- **Host loss** is an explicit `HostLoss` policy (`EndSession` or
  `PauseThenEnd { ticks }`); host migration stays out of scope.
- `RulesDigest` is 64-bit FNV-1a over a canonical form: a staleness check, not
  a security primitive.

## Open / not claimed (resolving stages)

- **Lobby wire envelope.** `cs_net::message` (F54-A, not owned here) has no
  lobby payload variants; `LobbyCommand`/`LobbyEvent`/`LaunchRequest` are
  typed but are not yet carried by `ClientPayload`/`ServerPayload`, and have
  no codec. F55-B/F54-C wire this; lobby messages must be `Reliable` and
  idempotent by `EventId`.
- **Blueprint validation is a hook.** `cs_net` cannot depend on content crates,
  so budgets/constraints (F44) arrive through `LoadoutValidator`; the
  production implementation over `cs_content::construction` is F55-B work.
  Until then the lobby alone does not prove a loadout is within budget.
- **Password handling** compares a stored `Secret`; hashing, rate limiting and
  session identity belong to F58-A.
- **Original option tables unknown.** Scenario ids, team modes, late-join and
  respawn rules of the original PC game are not known (F56-A discovers them);
  `MultiplayerScenario` ids and the ban component kinds here are designed.
- **Discovery vs Internet, LAN broadcast, pilot voice, ping display, per-mode
  late-join table**: not in this stage (F55-B/C, F56-A).
- **`cs_content/src/lobby.rs`** is not created: no content-side lobby data is
  known yet.
- Chat mute is client-local (`LobbyView`); rate limiting of chat is unspecified
  and left to F55-B.
