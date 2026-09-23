# Synthetic fixtures, not retail evidence

These files were authored solely for this pack by `tools/make_synthetic_fixtures.py`. They contain no copied original content. Regeneration is deterministic.

`rectangular.bm` is a two-by-three example with different corners and masks. `truncated.bm` is one byte short. `flat-uncompressed.rof` contains two newly named files and no compressed entries. It intentionally does not assert the ambiguous compressed-length field meaning. `synthetic.interp` exercises an observed family header and NUL-token envelope; its commands are invented and must never be registered as stock opcodes. `bad-version.interp` must fail dispatch for version 7.

`expected.json` records authored values and hashes. Tests should compare decoded output against independent assertions, not merely compare the expected file to itself. These examples seed parser unit tests; retail compatibility requires separate private corpus tests with provenance and actual reference behavior.

A Rust task needing more binary fixtures should generate them from readable, newly authored byte-building test code. Do not commit new binary blobs; reviewers reject binary additions until the owner has reviewed their provenance.
