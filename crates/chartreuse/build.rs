//! Embeds the Windows application manifest (`chartreuse.exe.manifest`) into the
//! executable. MSVC's linker does it itself (`/MANIFEST:EMBED`), so no resource
//! compiler is needed; other targets are left alone.

fn main() {
    const MANIFEST: &str = "chartreuse.exe.manifest";
    println!("cargo::rerun-if-changed={MANIFEST}");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    if target_env != "msvc" {
        println!(
            "cargo::warning=not embedding {MANIFEST}: only the MSVC linker is supported, so \
             Windows falls back to the DPI awareness winit sets at startup"
        );
        return;
    }
    let manifest =
        std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join(MANIFEST);
    println!("cargo::rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo::rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
