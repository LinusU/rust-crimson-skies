// TEMPORARY CI diagnostic for task #430. It is deleted in the follow-up commit.
// It prints the runner's filesystem headroom, the size of the build tree and
// the memory the job has, then fails so the numbers reach the CI log, because
// `cargo test` swallows the stdout of a passing test.

use std::process::Command;

fn out(program: &str, args: &[&str]) -> String {
    match Command::new(program).args(args).output() {
        Ok(o) => format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => format!("<{program} {args:?} failed: {e}>"),
    }
}

fn key(name: &str, text: &str) -> String {
    text.lines()
        .find(|l| l.starts_with(name))
        .unwrap_or("<missing>")
        .to_string()
}

#[test]
fn zz_tmp_probe_runner_headroom() {
    let cwd = std::env::current_dir().unwrap();
    let target = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());
    let temp = std::env::temp_dir();
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();

    let report = format!(
        "cwd={cwd:?}\n\
         CARGO_TARGET_DIR={target}\n\
         GITHUB_WORKSPACE={}\n\
         TMPDIR={:?} RUNNER_TEMP={}\n\
         CARGO_INCREMENTAL={:?} RUSTFLAGS={:?} RUST_BACKTRACE={:?}\n\
         GITHUB_RUN_ID={} GITHUB_IMAGE={}\n\
         nproc={}\n\
         df workspace:\n{}\n\
         df temp:\n{}\n\
         du target:\n{}\n\
         {} / {}",
        std::env::var("GITHUB_WORKSPACE").unwrap_or_default(),
        std::env::var("TMPDIR"),
        std::env::var("RUNNER_TEMP").unwrap_or_default(),
        std::env::var("CARGO_INCREMENTAL"),
        std::env::var("RUSTFLAGS"),
        std::env::var("RUST_BACKTRACE"),
        std::env::var("GITHUB_RUN_ID").unwrap_or_default(),
        std::env::var("ImageOS").unwrap_or_default(),
        out("nproc", &[]),
        out("df", &["-k", &cwd.display().to_string()]),
        out("df", &["-k", &temp.display().to_string()]),
        out("du", &["-sk", &format!("{target}/debug")]),
        key("MemTotal:", &meminfo),
        key("MemAvailable:", &meminfo),
    );

    panic!("#430 CI probe\n{report}");
}
