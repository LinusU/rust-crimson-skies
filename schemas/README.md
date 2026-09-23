# Machine-readable contracts

JSON Schema draft 2020-12 descriptions accompany the human contracts. The package uses only the Python standard library; its validator checks the concrete invariants it needs, not arbitrary JSON Schema evaluation. A separate JSON Schema engine may be used by downstream tooling.

`mission-binding` permits unresolved templates only when `verified` is false. `evidence` describes a report, not proof that its claims are true. `tools/validate_evidence.py` checks structure, command/test outcomes, artifact hashes and obvious contradictions; it cannot establish flight fidelity, audible playback, visual quality or honest human review. No tool in this pack automatically awards `verified_original` or release approval.

All references use local filenames. The `example.invalid` schema ids are stable identifiers, not download endpoints. Do not fetch them.
