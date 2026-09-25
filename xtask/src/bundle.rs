//! `cargo xtask bundle`: assemble and sign the macOS `.app`.

use std::path::{Path, PathBuf};

use chartreuse_core::flavor::Flavor;

use crate::icon;
use crate::info_plist::InfoPlist;
use crate::sign::{self, Identity, SignOptions};
use crate::util::{cargo, run, target_dir, Context, Error, Result};

/// The executable's name, in `target/<profile>/` and in `Contents/MacOS/`.
const EXECUTABLE: &str = "chartreuse";
/// The icon's name in `Contents/Resources/`, without `.icns`.
const ICON_FILE: &str = "AppIcon";

/// The Cargo profile to build with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    /// The profile's directory under the target directory.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

/// Where each part of a bundle goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub app: PathBuf,
    pub info_plist: PathBuf,
    pub executable: PathBuf,
    pub icns: PathBuf,
}

impl Layout {
    /// The layout of `flavor`'s bundle in `profile_dir` (e.g. `target/debug`):
    /// `<display name>.app`.
    #[must_use]
    pub fn new(profile_dir: &Path, flavor: Flavor) -> Self {
        let app = profile_dir.join(format!("{}.app", flavor.display_name()));
        let contents = app.join("Contents");
        Self {
            info_plist: contents.join("Info.plist"),
            executable: contents.join("MacOS").join(EXECUTABLE),
            icns: contents.join("Resources").join(format!("{ICON_FILE}.icns")),
            app,
        }
    }
}

/// How to build and sign one bundle.
#[derive(Debug, Clone)]
pub struct Bundle {
    pub flavor: Flavor,
    pub profile: Profile,
    pub identity: Identity,
    pub sign_options: SignOptions,
    /// The environment variable that selects `identity`, named in warnings.
    pub identity_env: &'static str,
}

impl Bundle {
    /// The development bundle: debug profile, development flavor, signed with
    /// [`sign::DEV_IDENTITY_ENV`] or ad-hoc.
    pub fn development() -> Self {
        Self {
            flavor: Flavor::Development,
            profile: Profile::Debug,
            identity: Identity::from_env_value(
                std::env::var(sign::DEV_IDENTITY_ENV).ok().as_deref(),
            ),
            sign_options: SignOptions { timestamp: false },
            identity_env: sign::DEV_IDENTITY_ENV,
        }
    }

    /// Builds the executable, assembles the bundle, and signs it. Returns the
    /// `.app` path.
    pub fn build(&self) -> Result<PathBuf> {
        if !cfg!(target_os = "macos") {
            return Err(Error("app bundles can only be built on macOS".into()));
        }
        let mut build = cargo();
        build.args(["build", "--package", "chartreuse"]);
        if self.profile == Profile::Release {
            build.arg("--release");
        }
        if self.flavor == Flavor::Release {
            build.args(["--features", "release-flavor"]);
        }
        run(&mut build)?;

        let profile_dir = target_dir().join(self.profile.dir_name());
        let layout = Layout::new(&profile_dir, self.flavor);
        self.assemble(&profile_dir, &layout)?;
        sign::sign(
            &layout.app,
            &self.identity,
            self.sign_options,
            self.identity_env,
        )?;
        eprintln!("bundled {}", layout.app.display());
        Ok(layout.app)
    }

    fn assemble(&self, profile_dir: &Path, layout: &Layout) -> Result {
        if layout.app.exists() {
            std::fs::remove_dir_all(&layout.app)
                .context(|| format!("removing {}", layout.app.display()))?;
        }
        for file in [&layout.executable, &layout.icns] {
            let dir = file.parent().expect("bundle files live in directories");
            std::fs::create_dir_all(dir).context(|| format!("creating {}", dir.display()))?;
        }
        let built = profile_dir.join(EXECUTABLE);
        std::fs::copy(&built, &layout.executable)
            .context(|| format!("copying {}", built.display()))?;
        let info = InfoPlist {
            bundle_id: self.flavor.bundle_id(),
            display_name: self.flavor.display_name(),
            executable: EXECUTABLE,
            icon_file: ICON_FILE,
            version: env!("CARGO_PKG_VERSION"),
        };
        std::fs::write(&layout.info_plist, info.render())
            .context(|| format!("writing {}", layout.info_plist.display()))?;
        let work_dir = profile_dir.join(format!("icon-{}", self.flavor.bundle_id()));
        icon::build_icns(self.flavor.accent(), &work_dir, &layout.icns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_are_named_after_the_flavor() {
        let dev = Layout::new(Path::new("target/debug"), Flavor::Development);
        assert_eq!(dev.app, Path::new("target/debug/Chartreuse Dev.app"));
        assert_eq!(
            dev.executable,
            Path::new("target/debug/Chartreuse Dev.app/Contents/MacOS/chartreuse")
        );
        assert_eq!(
            dev.info_plist,
            Path::new("target/debug/Chartreuse Dev.app/Contents/Info.plist")
        );
        assert_eq!(
            dev.icns,
            Path::new("target/debug/Chartreuse Dev.app/Contents/Resources/AppIcon.icns")
        );
        let release = Layout::new(Path::new("target/release"), Flavor::Release);
        assert_eq!(release.app, Path::new("target/release/Chartreuse.app"));
    }
}
