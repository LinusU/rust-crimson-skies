# F13-D-FU3-RESOLVE (#1623): no record key is outside the vocabularies

Date: 2026-10-28. Capability used: `retail` (read-only `$CS_GAME_DIR`).

Command: `cs-inspect scripts --cs-path "$CS_GAME_DIR" --coverage --out private/f13-census.json`
(exit 0).

Result on the retail installation:

- `record_keys` rows with status `outside_vocabulary`: **0**.
- `record_key_gate`: `{"passes": true, "violations": 0, "dropped": 0}`.

This agrees with F13-C-RECKEYS (205 classified rows, 0 unclassified). There is
no key to measure, so `CONTROL_RECORD_KEY_VOCABULARY` and
`CONTROL_RECORD_SOUND_KEY_VOCABULARY` are unchanged and no code changed.

#1623 stays the standing resolver: if a mod or a newly measured archive ever
yields an `outside_vocabulary` row, re-run the command above, measure the key
in the engine image (owner-static, cited) and add it to the right vocabulary.
