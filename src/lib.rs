//! DALI — the Davlgd Arch Linux Installer.
//!
//! An opinionated, single-binary installer for Arch Linux. The crate is split
//! into small modules with one job each:
//!
//! - [`config`] — the opinionated install spec (host-specific bits like CPU
//!   microcode are probed at install time, not stored here).
//! - [`system`] — the effects boundary: run commands, write files, or just
//!   record a dry-run plan; plus read-only [`system::probe`]s.
//! - [`steps`] — the ordered installation pipeline; step-specific data is
//!   co-located with each step (e.g. provisioning installers in `steps/provision`).
//! - [`tui`] — the interactive terminal interface that gathers a [`config`].
//! - [`cli`] — the clap command-line argument definitions.
//! - [`report`] — user-facing progress output.
//! - [`error`] — the shared [`Error`](error::Error)/[`Result`](error::Result) type.
//! - [`app`] — wires the above together for the binary.

pub mod app;
pub mod cli;
pub mod config;
pub mod error;
pub mod report;
pub mod steps;
pub mod system;
pub mod tui;
