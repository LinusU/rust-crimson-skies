# UI and multiplayer state contracts

## UI transition discipline

Use an explicit state table for every screen and Back/Cancel path. A UI action requests a domain transaction; it does not directly edit campaign cash, ownership or objective fields. Focus and accessibility state reset sensibly on entry. Dropdown selection is a content id, never a transient row number.

Test these complete paths: missing install -> choose install -> main; new profile -> cabin -> briefing -> flight check -> loading -> mission -> success -> scrapbook; construction edit -> cancel; invalid loadout -> correct -> launch; mission failure -> retry; pause -> settings -> resume; IA customize -> finish -> main; lobby join -> ready -> match -> results -> lobby; missing content -> diagnosis -> selection.

Back from a draft discards or explicitly confirms changes. Load failures preserve valid selections. Switching screen cannot leave a joystick trigger bound simultaneously to a button and a gun. Authored images are aspect-fit with logical hotspot coordinates transformed by the same scale/offset as the image.

## Network ownership table

Server owns ActorId allocation, physics truth, weapon acceptance, hit/damage, faction/interaction, mission program, score and result. Client owns only local input requests, UI and predicted cosmetics. Lobby host changes rules through a validated server action. No deserialized packet directly calls an arbitrary host binding.

Wire ids are stable typed numeric/string keys with bounded lengths. Protocol messages carry session epoch and sequence/tick. Epoch mismatch rejects stale packets. Content/rules mismatch rejects launch before expensive asset loading.

## Reliability and prediction

Lifecycle, rules, dialogue/objective-critical events and outcomes are reliable/idempotent. Motion snapshots are sequenced and may be dropped. Inputs have sequence acknowledgment and bounded acceptance windows. Reliable delivery does not replace application idempotency because reconnect/retry can replay requests.

Interpolation buffers separate actor generations. Local prediction never commits damage or rewards. Reconciliation has a tested error budget and hard reset threshold. Full deterministic Avian rollback is not assumed. Explicitly record limitations and choose bounded correction if complete rollback state is unavailable.

## Modes and compatibility

Discover original **PC** mode/scenario ids and full option tables. Do not infer this list from High Road to Revenge, a demo or a generic deathmatch library. Every original rule requires a declared state machine including ties, disconnections, ownership, respawn and end conditions. Where legacy transport behaviors cannot be preserved, document the new-engine behavior as designed rather than inventing historical equivalence.

The complete release includes the original multiplayer content over the new protocol. Legacy executable interoperability, proprietary matchmaking, host migration, voice chat, cooperative campaign and split-screen are optional separately scoped additions, not implied requirements.
