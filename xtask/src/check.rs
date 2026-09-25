//! `cargo xtask check`: what CI runs on every push.

use crate::util::{cargo, run, Result};

/// Formatting, lints (warnings are errors), and every test in the workspace.
pub fn check() -> Result {
    run(cargo().args(["fmt", "--all", "--check"]))?;
    run(cargo().args([
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "--deny",
        "warnings",
    ]))?;
    run(cargo().args(["test", "--workspace"]))?;
    eprintln!("check passed");
    Ok(())
}
