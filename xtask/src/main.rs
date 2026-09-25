//! Build automation for Chartreuse, run as `cargo xtask <command>`.
//!
//! Every CI step and every developer workflow beyond `cargo build` is a command
//! here, so anything CI does can be reproduced locally.

mod bundle;
mod check;
mod icon;
mod info_plist;
mod sign;
mod util;

use std::process::ExitCode;

const USAGE: &str = "\
Usage: cargo xtask <command>

Commands:
  check     cargo fmt --check, cargo clippy (warnings denied), cargo test
  bundle    build and sign target/debug/Chartreuse Dev.app (macOS); signs with
            $CHARTREUSE_SIGN_IDENTITY, or ad-hoc with a warning when unset
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check"] => check::check(),
        ["bundle"] => bundle::Bundle::development().build().map(drop),
        [] | ["help" | "--help" | "-h"] => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => Err(util::Error(format!(
            "unknown arguments {args:?}\n\n{USAGE}"
        ))),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
