//! Code signing with `codesign`.
//!
//! macOS keys privacy grants such as Screen Recording on the bundle's designated
//! requirement. With a real certificate that requirement names the bundle id and
//! the certificate, so grants survive rebuilds; with an ad-hoc signature it is the
//! binary's `cdhash`, so every rebuild looks like a new app.

use std::ffi::OsString;
use std::path::Path;

use crate::util::{capture, loud_warning, run, tool, Result};

/// The environment variable naming the development signing identity.
pub const DEV_IDENTITY_ENV: &str = "CHARTREUSE_SIGN_IDENTITY";

/// What to sign with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// A certificate in the keychain, by name or SHA-1 hash (as listed by
    /// `security find-identity -v -p codesigning`).
    Named(String),
    /// An ad-hoc signature: runs locally, but privacy grants do not persist.
    AdHoc,
}

impl Identity {
    /// The identity from an environment variable's value; unset or blank means
    /// ad-hoc.
    #[must_use]
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some(name) if !name.is_empty() => Self::Named(name.to_owned()),
            _ => Self::AdHoc,
        }
    }
}

/// Options that differ between development and release signing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignOptions {
    /// Request a secure timestamp from Apple (release only; needs the network).
    pub timestamp: bool,
}

/// The `codesign` arguments that sign `bundle`: hardened runtime always, for
/// parity between development and release.
#[must_use]
pub fn codesign_args(bundle: &Path, identity: &Identity, options: SignOptions) -> Vec<OsString> {
    let identity = match identity {
        Identity::Named(name) => name.as_str(),
        Identity::AdHoc => "-",
    };
    let timestamp = if options.timestamp {
        "--timestamp"
    } else {
        "--timestamp=none"
    };
    let mut args: Vec<OsString> = [
        "--force",
        "--options",
        "runtime",
        timestamp,
        "--sign",
        identity,
    ]
    .map(OsString::from)
    .into();
    args.push(bundle.into());
    args
}

/// Extracts the designated requirement from `codesign --display -r-` output.
/// Implicit requirements (the usual case) are printed as a `# designated => …`
/// comment.
#[must_use]
pub fn designated_requirement(display_output: &str) -> Option<&str> {
    display_output
        .lines()
        .map(|line| line.trim_start_matches(['#', ' ', '\t']))
        .find_map(|line| line.strip_prefix("designated =>"))
        .map(str::trim)
}

/// True if the requirement pins the exact binary, as ad-hoc signatures do.
#[must_use]
pub fn requirement_is_ad_hoc(requirement: &str) -> bool {
    requirement.contains("cdhash")
}

/// Signs `bundle`, then prints its designated requirement and warns if it will
/// not keep privacy grants across rebuilds.
pub fn sign(bundle: &Path, identity: &Identity, options: SignOptions, env_var: &str) -> Result {
    if *identity == Identity::AdHoc {
        loud_warning(&[
            &format!("{env_var} is not set: signing ad-hoc."),
            "Ad-hoc signatures change with every build, so macOS forgets the Screen",
            "Recording permission after each rebuild and captures come back blank.",
            &format!("Set {env_var} to a code-signing identity from"),
            "`security find-identity -v -p codesigning` (see README.md).",
        ]);
    }
    run(tool("codesign").args(codesign_args(bundle, identity, options)))?;

    let output = capture(tool("codesign").args(["--display", "-r-"]).arg(bundle))?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    match designated_requirement(&text) {
        Some(requirement) => {
            eprintln!("designated requirement: {requirement}");
            if requirement_is_ad_hoc(requirement) {
                loud_warning(&[
                    "The designated requirement contains a cdhash: Screen Recording",
                    "grants will not survive the next rebuild.",
                ]);
            }
        }
        None => eprintln!("warning: codesign printed no designated requirement:\n{text}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_or_missing_identity_means_ad_hoc() {
        assert_eq!(Identity::from_env_value(None), Identity::AdHoc);
        assert_eq!(Identity::from_env_value(Some("  ")), Identity::AdHoc);
        assert_eq!(
            Identity::from_env_value(Some(" Apple Development: Jo (ABC123) ")),
            Identity::Named("Apple Development: Jo (ABC123)".into())
        );
    }

    #[test]
    fn signing_always_uses_the_hardened_runtime() {
        let bundle = Path::new("/tmp/Chartreuse Dev.app");
        let args = codesign_args(bundle, &Identity::AdHoc, SignOptions { timestamp: false });
        assert_eq!(
            args,
            [
                "--force",
                "--options",
                "runtime",
                "--timestamp=none",
                "--sign",
                "-",
                "/tmp/Chartreuse Dev.app"
            ]
            .map(OsString::from)
        );
        let args = codesign_args(
            bundle,
            &Identity::Named("Dev ID".into()),
            SignOptions { timestamp: true },
        );
        assert_eq!(
            &args[3..6],
            ["--timestamp", "--sign", "Dev ID"].map(OsString::from)
        );
    }

    #[test]
    fn requirement_is_parsed_from_codesign_output() {
        let certificate = "Executable=/x/Chartreuse Dev.app/Contents/MacOS/chartreuse\n\
            designated => identifier \"io.jennings.chartreuse.dev\" and anchor apple generic \
            and certificate leaf[subject.CN] = \"Apple Development: Jo (ABC123)\"\n";
        let requirement = designated_requirement(certificate).unwrap();
        assert!(requirement.starts_with("identifier \"io.jennings.chartreuse.dev\""));
        assert!(!requirement_is_ad_hoc(requirement));

        let ad_hoc = "Executable=/x/chartreuse\n# designated => cdhash H\"0123abcd\"\n";
        assert_eq!(designated_requirement(ad_hoc), Some("cdhash H\"0123abcd\""));
        assert!(requirement_is_ad_hoc(
            designated_requirement(ad_hoc).unwrap()
        ));

        assert_eq!(designated_requirement("Executable=/x/chartreuse\n"), None);
    }
}
