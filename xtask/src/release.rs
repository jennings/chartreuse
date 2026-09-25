//! `cargo xtask release`: the release build for the host platform.
//!
//! macOS: an optimized, release-flavor `Chartreuse.app` signed with the Developer
//! ID identity and a secure timestamp, zipped into `target/dist/`. Track 5A
//! extends this to a universal binary and a notarized disk image.

use std::path::Path;

use chartreuse_core::flavor::Flavor;

use crate::bundle::{Bundle, Profile};
use crate::sign::{Identity, SignOptions};
use crate::util::{run, target_dir, tool, Context, Error, Result};

/// The environment variable naming the release (Developer ID) signing identity.
pub const RELEASE_IDENTITY_ENV: &str = "CHARTREUSE_RELEASE_SIGN_IDENTITY";

/// The release signing identity. A missing identity is an error naming the
/// variable, unless an ad-hoc build was explicitly requested.
pub fn release_identity(value: Option<&str>, allow_ad_hoc: bool) -> Result<Identity> {
    match Identity::from_env_value(value) {
        Identity::AdHoc if !allow_ad_hoc => Err(Error(format!(
            "{RELEASE_IDENTITY_ENV} is not set. Set it to your Developer ID Application \
             identity (see `security find-identity -v -p codesigning`), or pass \
             --allow-ad-hoc for a local build that cannot be distributed."
        ))),
        identity => Ok(identity),
    }
}

/// The name of the macOS release archive.
#[must_use]
pub fn archive_name(version: &str, arch: &str) -> String {
    format!("Chartreuse-{version}-macos-{arch}.zip")
}

pub fn release(allow_ad_hoc: bool) -> Result {
    match std::env::consts::OS {
        "macos" => release_macos(allow_ad_hoc),
        os => Err(Error(format!(
            "`cargo xtask release` has no steps for {os} yet (added in Stage 5)"
        ))),
    }
}

fn release_macos(allow_ad_hoc: bool) -> Result {
    let identity = release_identity(
        std::env::var(RELEASE_IDENTITY_ENV).ok().as_deref(),
        allow_ad_hoc,
    )?;
    let timestamp = matches!(identity, Identity::Named(_));
    let layout = Bundle {
        flavor: Flavor::Release,
        profile: Profile::Release,
        identity,
        sign_options: SignOptions { timestamp },
        identity_env: RELEASE_IDENTITY_ENV,
    }
    .build()?;

    let dist = target_dir().join("dist");
    std::fs::create_dir_all(&dist).context(|| format!("creating {}", dist.display()))?;
    let archive = dist.join(archive_name(
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
    ));
    zip_app(&layout.app, &archive)?;
    eprintln!("release archive: {}", archive.display());
    Ok(())
}

/// Zips a bundle with `ditto`, which preserves the code signature.
fn zip_app(app: &Path, archive: &Path) -> Result {
    if archive.exists() {
        std::fs::remove_file(archive).context(|| format!("removing {}", archive.display()))?;
    }
    run(tool("ditto")
        .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
        .arg(app)
        .arg(archive))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_release_identity_names_the_variable() {
        let error = release_identity(None, false).unwrap_err();
        assert!(error.0.contains(RELEASE_IDENTITY_ENV), "{error}");
        assert!(release_identity(Some(""), false).is_err());
    }

    #[test]
    fn ad_hoc_release_requires_opting_in() {
        assert_eq!(release_identity(None, true).unwrap(), Identity::AdHoc);
        assert_eq!(
            release_identity(Some("Developer ID Application: Jo (ABC123)"), false).unwrap(),
            Identity::Named("Developer ID Application: Jo (ABC123)".into())
        );
    }
}
