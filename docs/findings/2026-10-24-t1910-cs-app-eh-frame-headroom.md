# #1910: cs_app test binaries' `__eh_frame` headroom after #1833

Measured on macOS aarch64, debug build of origin/main (65d4251e7), with
`size -m <binary>` after `cargo test --locked -p cs_app --no-run`. The limit is
16,777,216 B (compact-unwind encoding limit).

| Binary | `__eh_frame` (B) | Headroom (B) |
| --- | ---: | ---: |
| cs_app lib unittests | 6,996,140 | 9,781,076 |
| cs bin unittests | 5,224,468 | 11,552,748 |
| cs_app_tests_a | 12,190,876 | 4,586,340 |
| cs_app_tests_b | 13,627,936 | 3,149,280 |
| cs_app_tests_c | 15,957,264 | 819,952 |
| cs_app_tests_d | 10,870,548 | 5,906,668 |
| cs_app_tests_e | 14,716,896 | 2,060,320 |

`[profile.dev.package.cs_app] opt-level = 1` (#1833) already brought suite E
back under the limit, so no test module was moved. Every binary has well over
150 KB of headroom; suite C is the tightest at about 800 KB. All `accept_t336_`
gates pass.
