//! DALI binary entry point: parse arguments, run, and turn any error into a
//! tidy non-zero exit instead of a panic/backtrace.

use std::process::ExitCode;

use clap::Parser;
use dali::app;
use dali::cli::Cli;
use dali::error::Error;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match app::run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Aborted) => {
            eprintln!("Aborted.");
            ExitCode::from(130)
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
