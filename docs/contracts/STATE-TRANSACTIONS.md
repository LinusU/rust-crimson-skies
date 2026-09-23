# Session, interaction, progression and save transactions

## Session reset

`begin_session(config)` allocates a new SessionId, verifies content readiness, stages all initial state and commits it only when required resources are ready. A failed load does not consume campaign money or progress. `end_session` cancels IO, scripts, timers, input ownership, audio loops and entity bindings for that generation. Persistent profile data receives only an explicit outcome transaction.

Retry restores the authored initial state, not a mutated copy of the just-failed world. Results, previous targets and delayed callbacks are always generation-qualified. No actor identifier is reused within a session.

## Interaction transaction

`InteractionId` binds initiating actor, target, mission authorization and session. It progresses through approaching/eligible/latching/transferring/completed or aborted. Eligibility predicates read the same canonical poses as collision. During latch a dedicated control owner is active. Completion validates that both actors and the mission phase still authorize the operation.

Aircraft swap is atomic: create/identify destination actor; apply transfer policy; bind pilot and player input; bind camera/HUD/audio; establish dynamic/kinematic state; invalidate old target/self references; publish `PlayerAircraftChanged`; release/despawn old actor as authored. Roll back or abort if the destination cannot be created. No intermediate frame may control both planes.

## Outcome and economy transaction

`OutcomeId = (profile_id, campaign_run_id, session_id, terminal_event_id)` or an equally stable persisted identity. Processing checks eligibility and whether it was already applied, computes all cash/unlock/record changes in memory, validates constraints, writes one atomic profile revision, then publishes a UI acknowledgment. A crash before acknowledgment can safely replay the same outcome without double reward.

Purchase/sell uses a draft and expected profile revision. Validate current availability, money and weight before writing. Conflicting revisions fail and refresh the view; they do not overwrite unrelated progression. Currency uses integer minor game units with a documented display mapping.

## Persistence

Write a new complete revision to a temporary file in the same filesystem; flush and fsync where available; preserve a valid previous version; rename; fsync directory where supported. Validate revision and checksum/schema when recovering. Do not combine arbitrary fields from two partially written files. Windows replacement semantics require an actual platform test rather than assuming POSIX rename behavior.

Profile ids are allocated from a persistent high-water mark or UUID equivalent. Display names can change and collide without changing identity. Deleting a profile does not recycle its id. Test recovery when the newest id file or active-profile pointer is missing.

## Mid-mission saves

Fresh-engine post-mission progress is required. Mid-mission suspend/checkpoints are an optional designed enhancement unless the actual original content requires them. If implemented, snapshot all program, actor, RNG, timer, interaction and ownership state consistently; otherwise disable that UI. Never claim mid-mission support from saving only position and health.
