//! `cs` — the Crimson Skies application binary.
//!
//! The request comes from [`cs_app::cli::parse`]: `--help`/`--version` exit 0
//! on stdout without touching the environment or the retail installation
//! (F00-B, acceptance case AC02), and `--synthetic --headless --ticks <n>`
//! runs the fixed-tick synthetic smoke through [`cs_app::run`]
//! (F00-C, acceptance case AC03). Whatever this binary cannot do, it reports
//! on stderr with a nonzero exit code — per `docs/contracts/CLI-EVIDENCE.md`
//! a failure is never returned as success.

use std::process::ExitCode;

use cs_app::cli::{self, CliRequest};
use cs_app::run;

fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1)) {
        CliRequest::Help => {
            print!("{}", cli::HELP_TEXT);
            ExitCode::SUCCESS
        }
        CliRequest::Version => {
            println!("{}", cli::version_text());
            ExitCode::SUCCESS
        }
        CliRequest::Synthetic(request) => match run::run_synthetic(&request) {
            Ok(report) => {
                print_run_report(&report);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("cs: {error}");
                ExitCode::from(error.exit_code())
            }
        },
        CliRequest::MissingInput => {
            eprintln!("{}", cli::missing_input_message());
            ExitCode::from(cli::EXIT_INVALID_INPUT)
        }
        CliRequest::Unsupported { args } => {
            eprintln!("{}", cli::unsupported_message(&args));
            ExitCode::from(cli::EXIT_INVALID_INPUT)
        }
        CliRequest::Invalid { reason } => {
            eprintln!("{}", cli::invalid_message(&reason));
            ExitCode::from(cli::EXIT_INVALID_INPUT)
        }
    }
}

/// Reports a finished run on stdout: the provenance marker, how far the world
/// really got, and where the trace went when one was requested.
fn print_run_report(report: &run::SyntheticRunReport) {
    println!(
        "synthetic headless run: {} {}/{} ticks at {} Hz, ended at tick {}",
        report.provenance.label(),
        report.ticks_completed,
        report.requested_ticks,
        cs_app::synthetic::TICK_HZ,
        report.final_sample.tick.0
    );
    if let Some(path) = &report.trace {
        println!("trace: {}", path.display());
    }
}
