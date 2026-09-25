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

### 2. A code-signing certificate

Create one of these once, in your login keychain:

- **Apple Development** (free with an Apple ID): Xcode → Settings → Accounts → add
  your Apple ID → Manage Certificates → **+** → Apple Development.
- **Self-signed**: Keychain Access → Certificate Assistant → Create a Certificate…,
  with Identity Type *Self-Signed Root* and Certificate Type *Code Signing*.

List the identities `codesign` can use:

```sh
security find-identity -v -p codesigning
```

### 3. Point the build at it

```sh
export CHARTREUSE_SIGN_IDENTITY="Apple Development: Your Name (TEAMID1234)"
```

The value is the quoted name (or the 40-character hash) from `security
find-identity`. Put it in your shell profile. Without it, builds are signed ad-hoc
and a loud warning explains that Screen Recording grants will not survive a rebuild.

### 4. Build and run

```sh
cargo xtask run
```

This builds `target/debug/Chartreuse Dev.app`, signs it, and launches it with `open`,
with the app's logs on your terminal. Until the status item lands, the app shows a
placeholder window; closing it (or its Quit button) quits the app. Set `RUST_LOG`
(for example `RUST_LOG=debug`) to change the log level.

`CHARTREUSE_BACKEND=fake cargo xtask run` swaps in the synthetic platform backend
(fake displays, windows, and captures) for UI work without real capture.

### Troubleshooting

- `codesign -d -r- "target/debug/Chartreuse Dev.app"` prints the designated
  requirement. It should name the bundle identifier and your certificate; a `cdhash`
  means the build was signed ad-hoc.
- `tccutil reset ScreenCapture io.jennings.chartreuse.dev` resets the Screen
  Recording grant, to test the first-run flow.
- With a self-signed certificate the keychain may ask for access to the private key
  on every build; choose *Always Allow* for `codesign`.

## Commands

Everything beyond `cargo build` is a `cargo xtask` command, and CI runs nothing else.

| Command | Result |
|---|---|
| `cargo xtask check` | `cargo fmt --check`, `cargo clippy` with warnings denied, `cargo test` |
| `cargo xtask bundle` | Signed `target/debug/Chartreuse Dev.app` (macOS) |
| `cargo xtask run` | `bundle`, then launch it through LaunchServices |
| `cargo xtask release` | Release build for the host platform. macOS: `Chartreuse.app` signed with `CHARTREUSE_RELEASE_SIGN_IDENTITY` (a Developer ID Application identity), zipped into `target/dist/`. `--allow-ad-hoc` signs ad-hoc instead, for a local build that cannot be distributed. |

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
