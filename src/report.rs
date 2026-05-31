//! User-facing progress reporting during installation.
//!
//! The pipeline reports progress through the [`Reporter`] trait so that the
//! same install logic can drive a plain console (headless / dry-run) today and
//! could drive a richer surface later without touching any step.

use std::fmt::Write as _;
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

/// A step's progress record, used to diagnose a partial install.
#[derive(Clone, Debug)]
pub struct StepRecord {
    /// 1-based position in the pipeline.
    pub index: usize,
    /// Total number of steps.
    pub total: usize,
    /// The step's name.
    pub name: String,
    /// Whether the step reported completion.
    pub completed: bool,
}

/// Wraps another [`Reporter`], forwarding every event while also capturing a
/// plain-text transcript and a per-step completion record (so the install log
/// and a step-status file can be written into the target afterwards).
pub struct TranscriptReporter<'a> {
    inner: &'a mut dyn Reporter,
    transcript: String,
    steps: Vec<StepRecord>,
}

impl<'a> TranscriptReporter<'a> {
    /// Wrap `inner`, capturing alongside it.
    pub fn new(inner: &'a mut dyn Reporter) -> Self {
        Self {
            inner,
            transcript: String::new(),
            steps: Vec::new(),
        }
    }

    /// The captured plain-text transcript.
    pub fn transcript(&self) -> &str {
        &self.transcript
    }

    /// The per-step completion records.
    pub fn steps(&self) -> &[StepRecord] {
        &self.steps
    }
}

impl Reporter for TranscriptReporter<'_> {
    fn step_start(&mut self, index: usize, total: usize, name: &str) {
        let _ = writeln!(self.transcript, "[{index}/{total}] {name}");
        self.steps.push(StepRecord {
            index,
            total,
            name: name.to_owned(),
            completed: false,
        });
        self.inner.step_start(index, total, name);
    }

    fn info(&mut self, message: &str) {
        let _ = writeln!(self.transcript, "    {message}");
        self.inner.info(message);
    }

    fn step_done(&mut self, name: &str) {
        let _ = writeln!(self.transcript, "    done: {name}");
        if let Some(record) = self.steps.iter_mut().rev().find(|r| r.name == name) {
            record.completed = true;
        }
        self.inner.step_done(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct NullReporter;
    impl Reporter for NullReporter {
        fn step_start(&mut self, _: usize, _: usize, _: &str) {}
        fn info(&mut self, _: &str) {}
        fn step_done(&mut self, _: &str) {}
    }

    #[test]
    fn transcript_captures_events_and_completion() {
        let mut inner = NullReporter;
        let mut t = TranscriptReporter::new(&mut inner);
        t.step_start(1, 2, "First");
        t.info("doing a thing");
        t.step_done("First");
        t.step_start(2, 2, "Second");

        assert!(t.transcript().contains("First"));
        assert!(t.transcript().contains("doing a thing"));
        assert!(t.steps()[0].completed);
        assert!(!t.steps()[1].completed, "unfinished step stays incomplete");
    }
}
