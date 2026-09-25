//! Build automation for Chartreuse, run as `cargo xtask <command>`.
//!
//! Every CI step and every developer workflow beyond `cargo build` is a command
//! here, so anything CI does can be reproduced locally.

mod bundle;
mod check;
mod dev_cert;
mod icon;
mod info_plist;
mod launch;
mod release;
mod sign;
mod util;

use std::process::ExitCode;

const USAGE: &str = "\
Usage: cargo xtask <command>

Commands:
  check     cargo fmt --check, cargo clippy (warnings denied), cargo test
  bundle    build and sign target/debug/Chartreuse Dev.app (macOS); signs with
            $CHARTREUSE_SIGN_IDENTITY, else the dev-cert identity, else ad-hoc
            with a warning
  run       bundle, then launch the app with `open`, its output on this terminal
            --fake  use the synthetic platform backend: no real capture, and no
                    macOS privacy prompts (use this for automated work)
  dev-cert  create a self-signed development signing identity in its own
            keychain (macOS, once per machine), so the Screen Recording
            permission survives rebuilds
  release   release build for this OS; on macOS a release-flavor Chartreuse.app
            signed with $CHARTREUSE_RELEASE_SIGN_IDENTITY, zipped into target/dist
            --allow-ad-hoc  sign ad-hoc when the identity is unset (local only)
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
        ["bundle"] => bundle::Bundle::development().and_then(|b| b.build().map(drop)),
        ["run"] => launch::launch(false),
        ["run", "--fake"] => launch::launch(true),
        ["dev-cert"] => dev_cert::dev_cert(),
        ["release"] => release::release(false),
        ["release", "--allow-ad-hoc"] => release::release(true),
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
