//! Shared helpers: errors, workspace paths, and running commands.

use std::fmt;
use std::path::Path;
use std::process::Command;

/// An xtask failure, printed as `error: …` before exiting with status 1.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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

/// The `cargo` that invoked us, so the pinned toolchain is used throughout.
pub fn cargo() -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
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
