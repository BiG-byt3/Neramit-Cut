// Packs the release NeramitCut.exe (deflate-compressed) into the installer, and embeds the icon,
// version information and an `asInvoker` manifest: without it Windows guesses from the name
// "Setup" that the program needs administrator rights and asks for them.
fn main() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let app = std::env::var("NERAMIT_APP_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| manifest.join("..").join("target").join("release").join("NeramitCut.exe"));
    println!("cargo:rerun-if-changed={}", app.display());
    println!("cargo:rerun-if-env-changed=NERAMIT_APP_EXE");
    println!("cargo:rerun-if-changed=setup.manifest");
    println!("cargo:rerun-if-changed=../assets/neramitcut.ico");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let bytes = std::fs::read(&app).unwrap_or_else(|e| panic!("build NeramitCut.exe first ({}): {e}", app.display()));
    std::fs::write(out.join("NeramitCut.exe.deflate"), miniz_oxide::deflate::compress_to_vec(&bytes, 9)).unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let designer = std::env::var("CARGO_PKG_AUTHORS").unwrap_or_default();
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "Neramit Cut");
        res.set("FileDescription", "Neramit Cut Setup");
        res.set("LegalCopyright", &format!("© 2026 Neramit Cut Made by {designer}"));
        res.set("CompanyName", &designer);
        embed(res, &manifest).expect("Could not embed the installer resources");
    }
}

/// As in the main build.rs: windres cannot work in a folder with a space in its name, so the
/// resource is compiled in a temporary folder.
fn embed(mut res: winresource::WindowsResource, manifest: &std::path::Path) -> std::io::Result<()> {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let work = std::env::temp_dir().join(format!("neramit-setup-res-{}", std::process::id()));
    std::fs::create_dir_all(&work)?;
    std::fs::copy(manifest.join("..").join("assets").join("neramitcut.ico"), work.join("icon.ico"))?;
    std::fs::copy(manifest.join("setup.manifest"), work.join("setup.manifest"))?;
    res.set_icon("icon.ico");
    res.set_manifest_file("setup.manifest");
    res.write_resource_file(work.join("resource.rc"))?;
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "windres".into());
    let status = std::process::Command::new(windres)
        .current_dir(&work)
        .args(["--target", "pe-x86-64", "resource.rc", "resource.o"])
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other("Could not compile resource file"));
    }
    let obj = out.join("neramit_setup_res.o");
    std::fs::copy(work.join("resource.o"), &obj)?;
    let _ = std::fs::remove_dir_all(&work);
    println!("cargo:rustc-link-arg-bin=NeramitCut-Setup={}", obj.display());
    Ok(())
}
