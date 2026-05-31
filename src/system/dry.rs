//! The dry-run effects implementation: nothing is executed or written, every
//! action is recorded and echoed so the user can review the exact plan.

use std::cell::RefCell;

use super::{Command, Sys};
use crate::error::Result;

/// Records the actions a real run *would* take, without performing any of them.
#[derive(Debug, Default)]
pub struct DrySys {
    actions: RefCell<Vec<String>>,
}

impl DrySys {
    /// Create an empty dry-run recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// The ordered list of actions recorded so far.
    pub fn actions(&self) -> Vec<String> {
        self.actions.borrow().clone()
    }

    fn record(&self, line: String) {
        println!("  {line}");
        self.actions.borrow_mut().push(line);
    }

    /// Record a content-writing action (`write`/`append`) with a size summary.
    fn record_content(&self, verb: &str, path: &str, contents: &str) {
        let bytes = contents.len();
        let lines = contents.lines().count();
        self.record(format!("{verb}: {path} ({bytes} bytes, {lines} lines)"));
    }
}

impl Sys for DrySys {
    fn run(&self, command: &Command) -> Result<()> {
        self.record(format!("run: {command}"));
        Ok(())
    }

    fn capture(&self, command: &Command) -> Result<String> {
        self.record(format!("capture: {command}"));
        Ok(String::new())
    }

    fn write(&self, path: &str, contents: &str) -> Result<()> {
        self.record_content("write", path, contents);
        Ok(())
    }

    fn mkdir_p(&self, path: &str) -> Result<()> {
        self.record(format!("mkdir -p {path}"));
        Ok(())
    }

    fn append(&self, path: &str, contents: &str) -> Result<()> {
        self.record_content("append", path, contents);
        Ok(())
    }

    fn write_block(&self, path: &str, _begin: &str, _end: &str, block: &str) -> Result<()> {
        self.record_content("write_block", path, block);
        Ok(())
    }

    fn is_real(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_records_without_executing() {
        let sys = DrySys::new();
        sys.run(&Command::new("pacstrap").arg("/mnt").arg("base"))
            .unwrap();
        sys.write("/mnt/etc/hostname", "arch\n").unwrap();
        let actions = sys.actions();
        assert_eq!(actions.len(), 2);
        assert!(actions[0].contains("pacstrap /mnt base"));
        assert!(actions[1].contains("/mnt/etc/hostname"));
    }

    #[test]
    fn dry_run_is_not_real() {
        assert!(!DrySys::new().is_real());
    }
}
