//! Shared helpers: errors, workspace paths, and running commands.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// An xtask failure, printed as `error: …` before exiting with status 1.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self(message.to_owned())
    }
}

pub type Result<T = (), E = Error> = std::result::Result<T, E>;

/// Adds context to I/O errors.
pub trait Context<T> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T>;
}

impl<T> Context<T> for std::io::Result<T> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|error| Error(format!("{}: {error}", what())))
    }
}

/// The workspace root (the parent of this crate's directory).
pub fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the workspace")
}

/// Cargo's target directory, honoring `CARGO_TARGET_DIR`.
pub fn target_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => workspace_root().join(dir),
        None => workspace_root().join("target"),
    }
}

/// The `cargo` that invoked us, so the pinned toolchain is used throughout.
pub fn cargo() -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(workspace_root());
    command
}

/// A command for an external tool, run from the workspace root.
pub fn tool(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.current_dir(workspace_root());
    command
}

fn describe(command: &Command) -> String {
    let program = Path::new(command.get_program());
    let mut text = program
        .file_name()
        .unwrap_or(program.as_os_str())
        .to_string_lossy()
        .into_owned();
    for arg in command.get_args() {
        let arg = arg.to_string_lossy();
        if arg.contains(' ') {
            text.push_str(&format!(" '{arg}'"));
        } else {
            text.push(' ');
            text.push_str(&arg);
        }
    }
    text
}

/// Runs a command with inherited stdio, failing if it does not succeed.
pub fn run(command: &mut Command) -> Result {
    eprintln!("$ {}", describe(command));
    let status = command
        .status()
        .context(|| format!("could not run {}", describe(command)))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error(format!("{} failed ({status})", describe(command))))
    }
}

/// Runs a command and captures its output, failing if it does not succeed.
pub fn capture(command: &mut Command) -> Result<Output> {
    let output = command
        .output()
        .context(|| format!("could not run {}", describe(command)))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(Error(format!(
            "{} failed ({}): {}",
            describe(command),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// Prints a warning that is hard to miss.
pub fn loud_warning(lines: &[&str]) {
    let rule = "=".repeat(78);
    eprintln!("warning: {rule}");
    for line in lines {
        eprintln!("warning: {line}");
    }
    eprintln!("warning: {rule}");
}
