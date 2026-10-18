# F13-D-FU4: the `event_records` rule now decodes the animation payload

Task: Rally #1421. Follows F13-D (#640), limitation `anim_payload_gate`.

## What changed

`model_selection` (`tools/cs_inspect/src/script_discovery.rs`) used to file an
animation container as `event_records` on family plus the validated 8-byte
header. It now also requires `ContainerDiscovery::animation_payload()` to be
`AnimationPayloadCheck::Decoded`, computed by
`cs_formats::script_raw::discovery` from the container's own bytes:

1. the animation index and payload header read;
2. the record walk (`AnimationPayload::records`) succeeds and yields at least
   one record;
3. every sequence block's event stream steps exactly by its length words,
   with each tag in the shape F20 measured (zero high half, second byte
   `1..=3`; length at least 8 and a multiple of 4).

Any failure is `AnimationPayloadCheck::Refused { code }` and the program keeps
the binary walk and its honest `unknown_opcode` stop, so it also stays in the
`--signatures` fail-closed gate.

## Dependency boundary

F20's `walk_event_stream` lives in `cs_app`, which depends on Bevy; `cs_formats`
may not. Step 3 therefore restates F20's two-word rule locally
(`events_step_exactly`) rather than importing it. It checks the stream shape
only; it does not join opcodes to statements, so it does not claim event
semantics.

## Evidence

Method and confidence of the report rows are unchanged (`executable_static_analysis`,
never `verified_original`). The retail acceptance test
`accept_f13_d_retail_every_program_family_has_its_real_model` still models all
61 carriers, so the gate refuses none of them. The `anim_payload_gate` entry is
removed from the F13-D evidence harness's unresolved list because this task
resolves it; the committed `docs/findings/evidence/F13-D.json` predates this
change and is regenerated only by the owner-run harness.
