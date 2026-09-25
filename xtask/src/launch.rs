//! `cargo xtask run`: bundle, then launch through LaunchServices.
//!
//! Launching with `open` (rather than executing the binary) makes the app the
//! "responsible process" for privacy checks, so it gets its own Screen Recording
//! grant instead of borrowing the terminal's.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use crate::bundle::{Bundle, Layout};
use crate::util::{run, target_dir, tool, Context, Error, Result};

/// Environment variables passed through to the app.
const FORWARDED_ENV: [&str; 3] = ["RUST_LOG", "RUST_BACKTRACE", "CHARTREUSE_BACKEND"];

/// The `open` arguments that launch `app`, wait for it to exit, and send its
/// stdout and stderr to `output`.
#[must_use]
pub fn open_args(app: &Path, output: &Path, env: &[(String, String)]) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-W".into(),
        "--stdout".into(),
        output.into(),
        "--stderr".into(),
        output.into(),
    ];
    for (name, value) in env {
        args.push("--env".into());
        args.push(format!("{name}={value}").into());
    }
    args.push(app.into());
    args
}

/// The controlling terminal's device path, if there is one.
fn terminal() -> Option<PathBuf> {
    let output = tool("tty")
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (output.status.success() && path.starts_with("/dev/")).then(|| PathBuf::from(path))
}

pub fn launch() -> Result {
    let bundle = Bundle::development();
    // Check before rebuilding: replacing a running app's bundle confuses macOS.
    let expected = Layout::new(&target_dir().join(bundle.profile.dir_name()), bundle.flavor);
    let running = tool("pgrep")
        .arg("-f")
        .arg(&expected.executable)
        .stdout(Stdio::null())
        .status()
        .context(|| "could not run pgrep".into())?;
    if running.success() {
        return Err(Error(format!(
            "{} is already running; quit it first (`open` would only bring the old build forward)",
            expected.app.display()
        )));
    }
    let layout = bundle.build()?;

    let env: Vec<(String, String)> = FORWARDED_ENV
        .iter()
        .filter_map(|name| Some(((*name).to_owned(), std::env::var(name).ok()?)))
        .collect();
    eprintln!("launching; quit the app (or close its placeholder window) to return");
    match terminal() {
        Some(tty) => run(tool("open").args(open_args(&layout.app, &tty, &env))),
        None => launch_with_log_file(&layout.app, &env),
    }
}

/// Without a terminal (e.g. under an IDE), route the app's output through a log
/// file and copy it to our stderr until the app exits.
fn launch_with_log_file(app: &Path, env: &[(String, String)]) -> Result {
    let log = app.with_extension("log");
    std::fs::File::create(&log).context(|| format!("creating {}", log.display()))?;
    let mut reader = std::fs::File::open(&log).context(|| format!("opening {}", log.display()))?;
    let mut child = tool("open")
        .args(open_args(app, &log, env))
        .spawn()
        .context(|| "could not run open".into())?;
    let copy = |reader: &mut std::fs::File| -> Result {
        let mut buffer = Vec::new();
        reader
            .read_to_end(&mut buffer)
            .context(|| format!("reading {}", log.display()))?;
        std::io::stderr()
            .write_all(&buffer)
            .context(|| "writing to stderr".into())
    };
    let status = loop {
        copy(&mut reader)?;
        if let Some(status) = child.try_wait().context(|| "waiting for open".into())? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    copy(&mut reader)?;
    if status.success() {
        Ok(())
    } else {
        Err(Error(format!("open failed ({status})")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_waits_and_routes_output_and_environment() {
        let args = open_args(
            Path::new("/t/Chartreuse Dev.app"),
            Path::new("/dev/ttys003"),
            &[("RUST_LOG".into(), "debug".into())],
        );
        assert_eq!(
            args,
            [
                "-W",
                "--stdout",
                "/dev/ttys003",
                "--stderr",
                "/dev/ttys003",
                "--env",
                "RUST_LOG=debug",
                "/t/Chartreuse Dev.app"
            ]
            .map(OsString::from)
        );
    }
}
