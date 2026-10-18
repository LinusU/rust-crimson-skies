# F49-D: integration re-check on main 2f7b59d5

Date: 2026-10-18. Task: F49-D (#204), the integration step after F49-D.01 to
F49-D.04 merged. Capability: **`retail`** (file access only; nothing was run or
played, nothing is `verified_original`).

Re-run on main 2f7b59d5 with `CS_GAME_DIR` set:

* `cargo test --locked -p cs_content -p cs_app -- accept_f49_d_ --include-ignored`
  exit 0: 32 passed (`cs_content`) and 20 passed (`cs_app`), 0 failed.
* `python3 tools/validate_evidence.py --artifact-root private/evidence/F49-D
  --require-pass docs/findings/evidence/F49-D.json` exit 0 (structurally valid;
  it does not verify claims semantically).

No code or evidence changed. The aggregate report and the findings in
`2026-10-18-f49-d-original-instant-action-catalog.md` stand as written,
including the unmet launch half of AC01 (resolving tasks #1575, #1582, #1583,
#1584, #1587, #1588). A merged F49-D is **checked**, not "recreated".
