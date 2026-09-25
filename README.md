# Chartreuse

A cross-platform screenshot and annotation tool that lives in the menu bar. See
[PLAN.md](PLAN.md) for what it does and how it is built, and [TASKS.md](TASKS.md) for
the build checklist.

## Development setup (macOS)

macOS ties the Screen Recording permission to an app's code signature, so
Chartreuse always runs as a signed `.app` bundle, launched through LaunchServices.
See [macOS code signing](PLAN.md#macos-code-signing) for the background.

### 1. Tools

- [rustup](https://rustup.rs). The toolchain version is pinned in
  `rust-toolchain.toml` and installed automatically.
- Xcode Command Line Tools (`xcode-select --install`) for `codesign` and `iconutil`.

### 2. A signing identity

macOS remembers the Screen Recording permission for a build's signing identity.
Builds signed ad-hoc (the fallback) have none: to macOS every rebuild is a new app, so
the permission never sticks, and the app does not ask for it (macOS would ask again on
every launch). Set up an identity once, either way below.

**`cargo xtask dev-cert` (recommended).** It creates a self-signed code-signing
certificate in a keychain of its own, `~/Library/Keychains/chartreuse-dev-signing.keychain-db`,
without any prompt and without touching your login keychain. `cargo xtask bundle` and
`cargo xtask run` sign with it whenever `CHARTREUSE_SIGN_IDENTITY` is unset, so the
builds of every checkout and jj workspace are one app to macOS. Once per machine:

```sh
cargo xtask dev-cert
# macOS keeps one Screen Recording record per bundle id, pinned to the build that
# first asked. If any earlier build asked (ad-hoc, or signed with another
# certificate), no build signed with this identity matches it, so macOS would ask
# on every launch. Drop it:
tccutil reset ScreenCapture io.jennings.chartreuse.dev
# macOS asks once: allow Chartreuse Dev in System Settings, then relaunch.
cargo xtask run
```

The keychain's password is fixed and not a secret, so any program running as you can
sign code with this identity and so inherit Chartreuse Dev's Screen Recording
permission (as with a login-keychain identity that `codesign` may *Always Allow*).
Remove it with `security delete-keychain
~/Library/Keychains/chartreuse-dev-signing.keychain-db`. A recreated identity is a new
certificate, so the old record matches no new build: run `cargo xtask dev-cert`, then
`tccutil reset ScreenCapture io.jennings.chartreuse.dev`, then allow the permission again.

**Your own certificate**, in your login keychain:

- **Apple Development** (free with an Apple ID): Xcode → Settings → Accounts → add
  your Apple ID → Manage Certificates → **+** → Apple Development.
- **Self-signed**: Keychain Access → Certificate Assistant → Create a Certificate…,
  with Identity Type *Self-Signed Root* and Certificate Type *Code Signing*.

List the identities `codesign` can use, and point the build at yours:

```sh
security find-identity -v -p codesigning
export CHARTREUSE_SIGN_IDENTITY="Apple Development: Your Name (TEAMID1234)"
```

The value is the quoted name (or the 40-character hash) from `security
find-identity`. Put it in your shell profile. It takes precedence over the
`dev-cert` identity. Without either, builds are signed ad-hoc and a loud warning
says so.

### 3. Build and run

```sh
cargo xtask run
```

This builds `target/debug/Chartreuse Dev.app`, signs it, and launches it with `open`,
with the app's logs on your terminal. The app opens no windows at startup: it lives
in the menu bar, and the status item's Quit quits it. Set `RUST_LOG` (for example
`RUST_LOG=debug`) to change the log level.

`cargo xtask run --fake` (or `CHARTREUSE_BACKEND=fake cargo xtask run`) swaps in the
synthetic platform backend (fake displays, windows, and captures) for UI work without
real capture. It calls no macOS privacy API, so it never makes macOS prompt. The fake
status item is not a real menu bar icon, so quit that build with
`pkill -f 'Chartreuse Dev.app/Contents/MacOS'`.

### Automated work (AI agents, scripts)

Unattended work must not put a Screen Recording prompt in front of the person at
the Mac:

- `cargo xtask check`, `cargo test`, and `cargo xtask bundle` never prompt. Tests only
  read the permission status, and macOS attributes them to the terminal.
- Launch the app with `cargo xtask run --fake`.
- Plain `cargo xtask run` (the real backend, for example to try hotkeys or the status
  item) does not prompt from an ad-hoc build. A `dev-cert` build asks macOS once
  per identity: leave that first launch, and its answer, to the person (step 2).
- Examples that capture (`cargo run -p chartreuse-platform --example
  capture_displays`) and `screencapture` run as the terminal app, so macOS asks on
  the terminal's behalf if it lacks Screen Recording.
- Never run `tccutil reset`.

### Troubleshooting

- `codesign -d -r- "target/debug/Chartreuse Dev.app"` prints the designated
  requirement. It should name the bundle identifier and your certificate; a `cdhash`
  means the build was signed ad-hoc.
- A signed build that asks for Screen Recording on every launch: macOS's record for
  `io.jennings.chartreuse.dev` was made by another identity (usually an ad-hoc
  build). `tccutil reset ScreenCapture io.jennings.chartreuse.dev` clears it.
- The same command resets the grant to test the first-run flow. That flow needs a
  signed build, since ad-hoc builds never ask.
- With a self-signed certificate from Keychain Access, the keychain may ask for
  access to the private key on every build; choose *Always Allow* for `codesign`.
  (`dev-cert` sets up its key so that it never asks.)

## Commands

Everything beyond `cargo build` is a `cargo xtask` command, and CI runs nothing else.

| Command | Result |
|---|---|
| `cargo xtask check` | `cargo fmt --check`, `cargo clippy` with warnings denied, `cargo test` |
| `cargo xtask bundle` | Signed `target/debug/Chartreuse Dev.app` (macOS) |
| `cargo xtask run` | `bundle`, then launch it through LaunchServices. `--fake` uses the synthetic platform backend |
| `cargo xtask dev-cert` | Once per machine (macOS): create the self-signed development signing identity that `bundle` uses when `CHARTREUSE_SIGN_IDENTITY` is unset |
| `cargo xtask release` | Release build for the host platform, archived into `target/dist/` (emptied first) as `Chartreuse-<version>-<os>-<arch>`. macOS: `Chartreuse.app` signed with `CHARTREUSE_RELEASE_SIGN_IDENTITY` (a Developer ID Application identity), zipped; `--allow-ad-hoc` signs ad-hoc instead when the variable is unset, and the archive name ends in `-unsigned`. Windows (`.zip`) and Linux (`.tar.gz`): the executable with `LICENSE` and `README.md`. |

The build flavor (development or release: bundle identifier, name, accent color) is
chosen by the `release-flavor` cargo feature, which only `cargo xtask release`
enables; it is independent of the optimization profile.

## Repository layout

| Path | Contents |
|---|---|
| `crates/chartreuse` | The app: `iced::daemon`, one module per feature |
| `crates/chartreuse-core` | Shared types and pure logic (geometry, images, hotkeys, errors, build flavor) |
| `crates/chartreuse-platform` | Platform traits, one backend directory per OS with one file per trait, and a `fake` backend |
| `crates/chartreuse-imaging` | Pixel operations, encoding, and decoding |
| `crates/chartreuse-config` | Settings schema and persistence |
| `crates/chartreuse-overlay` | Selection overlay canvas programs |
| `crates/chartreuse-editor` | Editor document model, tools, and canvas |
| `xtask` | Build automation (`cargo xtask`) |
