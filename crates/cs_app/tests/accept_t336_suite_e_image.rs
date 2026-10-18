//! Task #336: `cs_app_tests_e` keeps `__eh_frame` under the compact-unwind
//! encoding limit (see `accept_t336_eh_frame_unwind_table.rs`).

#[cfg(target_os = "macos")]
#[test]
fn accept_t336_suite_e_image_keeps_eh_frame_under_the_encoding_limit() {
    let image = std::env::current_exe().expect("the test binary path must resolve");
    crate::eh_frame_gate::assert_image_keeps_unwind_table(&image);
}
