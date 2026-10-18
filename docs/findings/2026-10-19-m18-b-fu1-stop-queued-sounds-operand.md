# M18-B-FU1: the sound group M18's `STOP_QUEUED_SOUNDS` sites name

Date: 2026-10-19. Task: M18-B-FU1 "Resolve the sound group M18's stop sites
name" (Rally #1254, follow-up of M18-B #310,
`docs/findings/2026-10-10-m18-b-compatibility-gaps.md`). Capabilities used:
`retail` (`$CS_GAME_DIR`, read-only) and the owner-supplied decrypted engine
image (`$CS_ENGINE_IMAGE`, SHA-256 `43540fc9…c37d75`, read-only, nothing of it
committed). Implementer: **claude-2/claude-2** (Rally #1254, session of
2026-10-19). Provenance is **static** (AGENTS.md rule 3 owner exception: spans
are cited by address, length and SHA-256, no disassembly is committed). No
original run was made, so nothing here is `verified_original` (rule 8).

## Question

Blocks 12 and 50 of M18's control record spell
`STOP_QUEUED_SOUNDS snd_c4-RM-m3_BlackSwan_27`, and no shipped record declares
that name (M18-B: absent from every decoded `.zbd` member but the control
member; raw bytes in exactly one file, `ZBD/C4/M03/zrdr.zbd`). What does the
original do with it?

## Answer

**The operand is not resolved against any declaration table. It is compared,
byte for byte, with the name of each sound currently in the queued-sound list,
and a name no queued sound carries ends the sweep silently.**

| Span (image VA, length, SHA-256) | What it does |
| --- | --- |
| `0x5920f0`, 84, `5a3a8116…cbef` | The `STOP_QUEUED_SOUNDS` handler. Returns at once if the queue `[0x639eac]` is empty. Otherwise it calls the list search `0x5791a0(queue, 0x592090, operand)`; a null result returns; a hit is unlinked (`0x5790a0`), passed to `0x592010`, and the search repeats, so **every** queued entry of that name is taken. |
| `0x592090`, 96, `6b684f5c…12bb` | The search's predicate. It follows `entry->[+4]->[+8]` (the entry's name string) and compares it with the operand pair of bytes at a time until they differ or the NUL is reached: an exact, case-sensitive string compare. Null or empty names count as no match. |
| `0x592010`, 48, `893f944b…` | What a hit receives: `[entry+0x10] = 1` and a removal time of `now + 10.0` (`0x603390` holds the float 10.0) on the second queue `[0x639eb0]`. This is M01-LC-DIRECTIVE-D's measured effect. |

This is the same handler, and the same reading, as
`docs/findings/2026-10-18-m17-b-fu1-record-only-operand-names.md` gives for M17's
four undeclared `snd_c4-RM-m2_*` operands; this note confirms the digest of the
handler and adds the predicate and the removal routine.

The name itself occurs **zero** times in the image (`snd_c4-RM-m3` occurs
nowhere in it), and **twice** in `ZBD/C4/M03/zrdr.zbd`: the two stop sites. So
no shipped file and no literal in the engine supplies a queued sound with that
name. Its sibling `BlackSwan_28`, spelled at the same sites, is declared in the
shared `ZBD/zrdr.zbd` sounds table and can be queued under its name.

## Conclusion

* Resolution: a `STOP_QUEUED_SOUNDS` operand needs no declaration; "declared by
  no shipped record" is not an error condition in the original.
* For `BlackSwan_27`, nothing static in the installation or the image can put a
  sound under that name in the queue, so the stop is, as far as this build can
  show, **inert**: the sweep finds nothing and returns. Nothing about the
  retail data is wrong and nothing is deleted or "fixed".
* **Not decided here:** whether the sound producers (the code that enqueues a
  sound and fills the name at `entry->[+4]->[+8]`) could build the name at run
  time from other data (a prefix and a number, say). That producer was not
  traced. If it does, the stop would act on a sound this build cannot name; a
  reference run under M18-C (#311, `human_play`) would settle it. No production
  change follows from this note: the runtime already treats an unmatched name
  as a no-op in M17's measured handling, and M18-B's tests keep pinning the
  retail fact.

## Test

`accept_m18_b_fu1_an_undeclared_stop_operand_is_compared_with_queued_names_only`
(`crates/cs_app/tests/campaign/m18_b_fu1.rs`, retail, `#[ignore]` without
`CS_GAME_DIR` and `CS_ENGINE_IMAGE`) re-derives the handler and predicate
digests and the key opcode bytes from `$CS_ENGINE_IMAGE`, and pins the name's
absence from the image and its two occurrences in M18's reader archive.
