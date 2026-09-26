//! `cargo xtask upload-release <tag>`: attach every file `cargo xtask release`
//! left in `target/dist/` to the GitHub release for `<tag>`.
//!
//! Uploads go through the GitHub CLI (`gh`), which every GitHub-hosted runner
//! has preinstalled and which reads its token from `GH_TOKEN` or `GITHUB_TOKEN`
//! and its repository from `GH_REPO` (else the checkout's git remote). An HTTP
//! client in the xtask would have to reimplement release lookup, asset
//! replacement, and upload retries, and would add a TLS stack to every `cargo
//! xtask` build.
//!
//! Re-running is safe: an asset that already exists under the same name is
//! replaced (`gh release upload --clobber`), so a re-run job refreshes its own
//! platform's assets and leaves the other platforms' alone.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::release::{archive_extension, dist_dir, VERSION};
use crate::util::{run, tool, Context, Error, Result};

pub fn upload_release(tag: &str) -> Result {
    check_tag(tag, VERSION)?;
    let assets = release_assets(&dist_dir(), VERSION)?;
    run(tool("gh").args(gh_upload_args(tag, &assets)))
}

/// Checks that a release tag (`v1.2.3`, or `1.2.3`) names `version`, the
/// workspace version that the assets are built as and named after.
pub fn check_tag(tag: &str, version: &str) -> Result {
    if tag.strip_prefix('v').unwrap_or(tag) == version {
        Ok(())
    } else {
        Err(Error(format!(
            "release tag `{tag}` does not match the workspace version {version} \
             ([workspace.package] version in Cargo.toml). Tag the release v{version}, \
             or bump the version and tag that commit."
        )))
    }
}

/// The files in `dir`, sorted, after checking that each is named for `version`
/// (`Chartreuse-<version>-<os>-…`), so a stale build is never uploaded.
fn release_assets(dir: &Path, version: &str) -> Result<Vec<PathBuf>> {
    let missing = || {
        Error(format!(
            "no release output in {}: run `cargo xtask release` first",
            dir.display()
        ))
    };
    if !dir.is_dir() {
        return Err(missing());
    }
    let mut assets = Vec::new();
    for entry in std::fs::read_dir(dir).context(|| format!("reading {}", dir.display()))? {
        let path = entry
            .context(|| format!("reading {}", dir.display()))?
            .path();
        if path.is_file() {
            assets.push(path);
        }
    }
    if assets.is_empty() {
        return Err(missing());
    }
    assets.sort();
    for asset in &assets {
        let name = asset.file_name().unwrap_or_default().to_string_lossy();
        if !is_asset_name_for(&name, version) {
            return Err(Error(format!(
                "{} is not a {version} release asset; run `cargo xtask release` again",
                asset.display()
            )));
        }
    }
    Ok(assets)
}

/// Whether `name` is `Chartreuse-<version>-<os>-…` for an OS that `release`
/// builds. Checking the OS, not just the prefix, rejects a pre-release of the
/// same version (`Chartreuse-0.1.0-rc.1-…` when the version is `0.1.0`).
fn is_asset_name_for(name: &str, version: &str) -> bool {
    name.strip_prefix("Chartreuse-")
        .and_then(|rest| rest.strip_prefix(version))
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.split_once('-'))
        .is_some_and(|(os, _)| archive_extension(os).is_ok())
}

/// Arguments for `gh` that upload `assets` to the release for `tag`, replacing
/// assets of the same name.
fn gh_upload_args(tag: &str, assets: &[PathBuf]) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["release", "upload", "--clobber", "--", tag]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.extend(assets.iter().map(|asset| asset.as_os_str().to_owned()));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_must_name_the_workspace_version() {
        assert!(check_tag("v0.2.0", "0.2.0").is_ok());
        assert!(check_tag("0.2.0", "0.2.0").is_ok());
        for tag in [
            "v0.2.1",
            "v0.2",
            "v0.2.0-rc.1",
            "release-0.2.0",
            "vv0.2.0",
            "",
        ] {
            assert!(check_tag(tag, "0.2.0").is_err(), "{tag}");
        }
    }

    #[test]
    fn tag_mismatch_names_both_versions() {
        let error = check_tag("v0.3.0", "0.2.0").unwrap_err();
        assert!(error.0.contains("v0.3.0"), "{error}");
        assert!(error.0.contains("0.2.0"), "{error}");
        assert!(error.0.contains("Cargo.toml"), "{error}");
    }

    /// A fresh, empty directory for one test.
    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("chartreuse-xtask-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn assets_are_every_file_in_dist_sorted() {
        let dir = scratch_dir("assets");
        for name in [
            "Chartreuse-0.2.0-macos-aarch64-unsigned.zip",
            "Chartreuse-0.2.0-linux-x86_64.tar.gz",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        // Directories (e.g. staging leftovers) are not assets.
        std::fs::create_dir(dir.join("Chartreuse-0.2.0-linux-x86_64")).unwrap();

        let assets = release_assets(&dir, "0.2.0").unwrap();
        assert_eq!(
            assets,
            [
                dir.join("Chartreuse-0.2.0-linux-x86_64.tar.gz"),
                dir.join("Chartreuse-0.2.0-macos-aarch64-unsigned.zip"),
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_or_foreign_files_are_not_uploaded() {
        let dir = scratch_dir("stale");
        std::fs::write(dir.join("Chartreuse-0.2.0-linux-x86_64.tar.gz"), b"").unwrap();
        std::fs::write(dir.join("Chartreuse-0.1.0-linux-x86_64.tar.gz"), b"").unwrap();
        let error = release_assets(&dir, "0.2.0").unwrap_err();
        assert!(error.0.contains("Chartreuse-0.1.0"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pre_release_of_the_same_version_is_not_uploaded() {
        let dir = scratch_dir("pre-release");
        std::fs::write(dir.join("Chartreuse-0.1.0-linux-x86_64.tar.gz"), b"").unwrap();
        std::fs::write(dir.join("Chartreuse-0.1.0-rc.1-linux-x86_64.tar.gz"), b"").unwrap();
        let error = release_assets(&dir, "0.1.0").unwrap_err();
        assert!(error.0.contains("Chartreuse-0.1.0-rc.1"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_empty_dist_asks_for_a_release_build() {
        let dir = scratch_dir("empty");
        for dir in [dir.clone(), dir.join("missing")] {
            let error = release_assets(&dir, "0.2.0").unwrap_err();
            assert!(error.0.contains("cargo xtask release"), "{error}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn upload_replaces_existing_assets_and_never_reads_names_as_flags() {
        let assets = [
            PathBuf::from("/dist/a.zip"),
            PathBuf::from("/dist/b.tar.gz"),
        ];
        let args = gh_upload_args("v0.2.0", &assets);
        let separator = args.iter().position(|arg| arg == "--").unwrap();
        assert!(args[..separator].contains(&"--clobber".into()));
        assert_eq!(
            args[separator + 1..],
            ["v0.2.0", "/dist/a.zip", "/dist/b.tar.gz"].map(OsString::from)
        );
    }
}
