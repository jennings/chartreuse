//! Code signing with `codesign`.
//!
//! macOS keys privacy grants such as Screen Recording on the bundle's designated
//! requirement. With a real certificate that requirement names the bundle id and
//! the certificate, so grants survive rebuilds; with an ad-hoc signature it is the
//! binary's `cdhash`, so every rebuild looks like a new app.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::util::{capture, loud_warning, run, tool, Result};

/// The environment variable naming the development signing identity.
pub const DEV_IDENTITY_ENV: &str = "CHARTREUSE_SIGN_IDENTITY";

/// What to sign with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// A certificate in the keychain, by name or SHA-1 hash (as listed by
    /// `security find-identity -v -p codesigning`).
    Named(String),
    /// The identity `cargo xtask dev-cert` created, by SHA-1 hash, in its own
    /// keychain (which is not on the search list).
    DevCert { keychain: PathBuf, hash: String },
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

    /// The development identity: the one named by [`DEV_IDENTITY_ENV`]'s value if
    /// set, else the `cargo xtask dev-cert` identity if `dev_cert` finds one, else
    /// ad-hoc.
    pub fn development(
        env_value: Option<&str>,
        dev_cert: impl FnOnce() -> Result<Option<Self>>,
    ) -> Result<Self> {
        match Self::from_env_value(env_value) {
            Self::AdHoc => Ok(dev_cert()?.unwrap_or(Self::AdHoc)),
            named => Ok(named),
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
    let timestamp = if options.timestamp {
        "--timestamp"
    } else {
        "--timestamp=none"
    };
    let mut args: Vec<OsString> = ["--force", "--options", "runtime", timestamp]
        .map(OsString::from)
        .into();
    match identity {
        Identity::Named(name) => args.extend(["--sign".into(), name.into()]),
        Identity::DevCert { keychain, hash } => args.extend([
            "--keychain".into(),
            keychain.into(),
            "--sign".into(),
            hash.into(),
        ]),
        Identity::AdHoc => args.extend(["--sign".into(), "-".into()]),
    }
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
    match identity {
        Identity::AdHoc if env_var == DEV_IDENTITY_ENV => loud_warning(&[
            &format!("{env_var} is not set and `cargo xtask dev-cert` has not been run:"),
            "signing ad-hoc. Ad-hoc signatures change with every build, and macOS ties",
            "Screen Recording to the signature, so ad-hoc builds never keep the",
            "permission and the app does not ask for it. Run `cargo xtask dev-cert`",
            &format!("once, or set {env_var} to an identity from"),
            "`security find-identity -v -p codesigning` (see README.md).",
        ]),
        Identity::AdHoc => loud_warning(&[
            &format!("{env_var} is not set: signing ad-hoc."),
            "Ad-hoc signatures change with every build, so macOS forgets the Screen",
            "Recording permission after each rebuild and captures come back blank.",
            &format!("Set {env_var} to a code-signing identity from"),
            "`security find-identity -v -p codesigning` (see README.md).",
        ]),
        Identity::DevCert { .. } => {
            eprintln!("signing with the `cargo xtask dev-cert` identity");
        }
        Identity::Named(_) => {}
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
    fn development_prefers_the_env_identity_then_the_dev_cert() {
        let dev_cert = || Identity::DevCert {
            keychain: "/k/dev.keychain-db".into(),
            hash: "AB76".into(),
        };
        // An explicit identity wins; the dev keychain is not even unlocked.
        let named = Identity::development(Some("Apple Development: Jo"), || {
            panic!("the dev cert must not be looked up")
        });
        assert_eq!(
            named.unwrap(),
            Identity::Named("Apple Development: Jo".into())
        );
        assert_eq!(
            Identity::development(Some(" "), || Ok(Some(dev_cert()))).unwrap(),
            dev_cert()
        );
        assert_eq!(
            Identity::development(None, || Ok(None)).unwrap(),
            Identity::AdHoc
        );
        // A dev keychain that exists but cannot be used fails the build rather
        // than silently signing ad-hoc.
        assert!(Identity::development(None, || Err("broken".into())).is_err());
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
    fn dev_cert_signing_searches_only_its_keychain() {
        // The dev keychain is not on the search list, so codesign must be
        // pointed at it.
        let identity = Identity::DevCert {
            keychain: "/Users/jo/Library/Keychains/dev.keychain-db".into(),
            hash: "AB76B80BF7B3F1D0FC53E1242D3F79D566418749".into(),
        };
        let args = codesign_args(
            Path::new("/tmp/Chartreuse Dev.app"),
            &identity,
            SignOptions { timestamp: false },
        );
        assert_eq!(
            &args[4..],
            [
                "--keychain",
                "/Users/jo/Library/Keychains/dev.keychain-db",
                "--sign",
                "AB76B80BF7B3F1D0FC53E1242D3F79D566418749",
                "/tmp/Chartreuse Dev.app"
            ]
            .map(OsString::from)
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
