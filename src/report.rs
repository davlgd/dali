//! User-facing progress reporting during installation.
//!
//! The pipeline reports progress through the [`Reporter`] trait so that the
//! same install logic can drive a plain console (headless / dry-run) today and
//! could drive a richer surface later without touching any step.

use std::io::{self, IsTerminal, Write};

/// Receives progress events as the installation pipeline runs.
pub trait Reporter {
    /// A step (1-based `index` of `total`) is starting.
    fn step_start(&mut self, index: usize, total: usize, name: &str);
    /// An informational detail within the current step.
    fn info(&mut self, message: &str);
    /// The current step finished successfully.
    fn step_done(&mut self, name: &str);
}

/// A reporter that prints to standard output with light ANSI styling.
#[derive(Debug, Default)]
pub struct ConsoleReporter {
    color: bool,
}

impl ConsoleReporter {
    /// Create a reporter, enabling colour only when stdout is a terminal.
    pub fn new() -> Self {
        Self {
            color: io::stdout().is_terminal(),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
}

impl Reporter for ConsoleReporter {
    fn step_start(&mut self, index: usize, total: usize, name: &str) {
        let arrow = self.paint("1;34", "==>");
        let counter = self.paint("1;30", &format!("[{index}/{total}]"));
        println!("{arrow} {counter} {name}");
        let _ = io::stdout().flush();
    }

    fn info(&mut self, message: &str) {
        println!("    {message}");
        let _ = io::stdout().flush();
    }

    fn step_done(&mut self, name: &str) {
        let ok = self.paint("1;32", "✓");
        println!("    {ok} {name}");
        let _ = io::stdout().flush();
    }
}
