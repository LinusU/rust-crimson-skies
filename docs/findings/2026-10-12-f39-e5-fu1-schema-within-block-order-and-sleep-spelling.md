# F39-E5-FU1: the declared schema accepts the within-block conflict the engine order decides; the SLEEP spelling stays out of the declared vocabulary

Date: 2026-10-12. Task: #1376 `F39-E5-FU1` (follow-up to #1362
`M01-B-OBJ-1-FU1`, parent #1358 `M01-B-OBJ-1`). Test prefix
`accept_f39_e5_fu1_`. Capabilities used: none beyond a code change — every
measurement cited here was taken under #1358 (`retail` read of `$CS_GAME_DIR`
and read-only static analysis of `CS_ENGINE_IMAGE`) and is recorded, with
addresses, in
`docs/findings/2026-10-12-m01-b-obj-1-completion-effects-and-tick-dependency.md`
and its closing note
`docs/findings/2026-10-12-m01-b-obj-1-fu1-runtime-effect-alignment.md`.
**No original executable was run**; nothing here is `verified_original`.

This note settles the two content-side items #1362 left open because
`crates/cs_content` was outside its owner paths.

## Item 1: the within-block conflict is now accepted by the declared schema

### The decision

`ObjectivesSchemaError::AmbiguousCompletionEffect` used to refuse **two
different completion effects naming the same objective in one program** —
within one declaration as much as across two. M01-B-OBJ-1 measured the engine
resolving exactly the within-one-declaration shape (the corpus's one conflict
in 1338 blocks, `zbd/c3/m05` `OBJECTIVE8`, a `WAKE` and a `NAP` both naming
objective 68) with its fixed order `WAKE` → `NAP` → `KILL` → `SLEEP`, whatever
order the block spells, and #1362 aligned `cs_sim::objectives::runtime` to
that measurement: it accepts a declaration carrying two effects on one target
and applies them in the engine's order.

The declared schema now follows: **within one declaring objective the shape is
accepted**, and the refusal narrows to what stays unmeasured — **two different
completing objectives declaring different effects on one target**. The engine
completes one objective per pass, so those two completions never collide
there; which of two same-tick completions should apply last depends on the
pass-origin rule (M01-B-OBJ-1 open question 2), which is still not decoded.

### What changed, and where

* `crates/cs_content/src/objectives.rs` — `check_completion_effects` now
  records **every** `(declaring objective, kind)` pair each target is named
  with (the same every-kind scan #1362's review added to the runtime, so a
  declaration carrying two kinds on one target cannot hide its second kind
  from the check) and refuses only a pair whose *source differs* and whose
  *kind differs*. `ObjectivesSchemaError::AmbiguousCompletionEffect` keeps its
  name with narrowed docs; its `Display` now states the pass-origin reason.
  The module docs, the `DeclaredPrecedence` doc and the fixture docs were
  updated to the measured state.
* `crates/cs_app/tests/accept_f39_e5_completion_effects.rs` — the suite now
  pins the new behaviour: `accept_f39_e5_one_declaration_two_effects_on_one_target_is_accepted`
  (both authored orders accepted and carried as spelled; the runtime
  registers the shape) and
  `accept_f39_e5_two_completions_naming_one_objective_are_refused_by_name`
  (both authored assignments refused at the schema, both registration orders
  refused at the runtime, and the agreeing neighbour stays legal). The retail
  gate test `accept_f39_e5_an_original_record_with_effects_is_never_played`
  is untouched and still passes.
* `crates/cs_app/tests/accept_f39_e6_repeated_effect_key.rs` — the last
  assertion of `accept_f39_e6_the_declared_form_refuses_a_repeat_by_name`,
  which pinned the within-block shape as the ambiguous one, now pins the
  cross-objective shape under the same error name; the repeat refusals are
  unchanged.
* Three new unit tests in `crates/cs_content/src/objectives.rs` carry the task
  prefix:
  `accept_f39_e5_fu1_one_declaration_two_effects_on_one_target_is_accepted`
  (fails if the old refusal is restored),
  `accept_f39_e5_fu1_cross_objective_conflicts_are_still_refused_by_name` and
  `accept_f39_e5_fu1_multi_effect_declaration_still_collides_cross_objective`.

### What deliberately did **not** change

`UNMEASURED_BLOCK_PRECEDENCE`, `MeasuredBranchPrecedence` and the census
counters stay exactly as F39-E2 measured them: the corpus's **bytes** still
carry no order (both authored directions of every repeated pair occur), and
that is what the census verdict says. The engine's fixed order is a different
measurement, taken from the executable image rather than the corpus, and it is
carried where the effects are applied (`cs_sim::objectives::runtime`'s
`engine_order()`), not in the census. The F39-E2/E6 census suites and the
evidence reports are therefore untouched.

## Item 2: the SLEEP spelling stays out of the declared schema vocabulary

### The decision

The declared schema keeps `BranchEffectKind` at the **four corpus
spellings**. It does not gain a `Sleep` variant in this task.

### Why

1. **The enum's contract is the corpus census, not the engine's reader
   table.** `BranchEffectKind`/`BRANCH_EFFECT_KEY_VOCABULARY` are F39-D and
   F39-E2's measured census over the owner's installation (53 mission readers,
   1338 blocks: 412 `WAKE`, 417 `NAP`, 225 `KILL`, 2 `WAKEUP`) — "the
   spellings the original's records write", pinned by
   `accept_f39_e5_the_effect_vocabulary_is_the_measured_one` and the F39-E2
   evidence reports. Both directions of that vocabulary are load-bearing and
   already asymmetric with the engine: `Wakeup` is spelled by the corpus
   although the engine image has **no reader** for it, and `SLEEP` is read by
   the engine although **no record spells it**. Folding the reader set into
   the census vocabulary would merge the two measurements and change what
   `all()`/`from_measured_key` mean for every census consumer.
2. **The engine-reader-set vocabulary already exists in this crate, with the
   exact provenance.** `cs_content::objectives_engine::keys::SLEEP` carries
   the key ("The engine reads it; no retail record spells it") for the
   recovery, and `cs_content::mission_control`'s measured-directive table
   carries the same reading. The measured capability is not lost anywhere; the
   declared schema is the one layer whose job is what a **record** declares.
3. **Nothing a record can reach is unreachable today.** The corpus spells the
   key zero times (F39-D's 1338 blocks; F39-E6 extended the sweep to the
   shared and world-group readers and every `targets.zrd` record), so no
   declared program could carry the variant; `cs_sim`'s
   `CompletionEffectKind::Sleep`/`ObjectiveState::Asleep` stay carried for
   callers that build programs without the declared schema, and
   `cs_app::objectives::lower_effect_kind`'s doc already names the boundary
   exactly as it stands. Adding the variant now could only ever be exercised
   synthetically, which AGENTS.md rule 5 does not accept as evidence.
4. **The revisit condition is one small measured change.** The moment a
   record that spells `SLEEP_OBJECTIVE_WHEN_I_COMPLETE` exists to declare —
   owner-authored mission content or a mod record, since the declared schema
   is the authoring surface — the closed vocabulary extends with the
   measured reader (engine image 0x468c97, the same `-1`-terminated list
   shape as `WAKE`/`KILL`, no argument), carrying the recovery's provenance
   note, and `lower_effect_kind` maps it to `CompletionEffectKind::Sleep`.
   Nothing else blocks that extension; this task deliberately does not make
   it speculatively.

## What stays unknown, unchanged

The pass-origin rule (which of two simultaneously complete objectives
completes first — the reason the cross-objective refusal stands), the nap
time's unit, the dormant/awake timers, the wake's dormancy bookkeeping,
whether `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` is truly inert, and whether the
image's behaviour matches this reading when run. `DeclaredSupport::Original`
stays unplayable for the completion-effect declarations; none of this is a
fidelity claim.

Evidence class: none of this is `verified_original`; it is a declared-schema
change aligned to a static reading of the engine image, with synthetic tests
over the project's own fixtures plus the untouched retail census suites.
