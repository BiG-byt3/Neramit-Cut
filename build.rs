fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=assets/neramitcut.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "1.0.0".into());
    let authors = std::env::var("CARGO_PKG_AUTHORS").unwrap_or_default();
    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "Neramit Cut");
    res.set("FileDescription", "Neramit Cut - Linear Cutting Optimizer");
    res.set("OriginalFilename", "NeramitCut.exe");
    res.set("CompanyName", &authors);
    res.set("LegalCopyright", "Neramit Cut made by BiGByte Studio");
    // System-DPI aware: the layout is scaled at start-up (ui::theme::scale) so text and
    // drawings stay sharp on high-DPI screens.
    let manifest = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
 <assemblyIdentity version="{version}.0" processorArchitecture="*" name="NeramitCut" type="win32"/>
 <dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency>
 <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings><dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware></windowsSettings></application>
 <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security></trustInfo>
</assembly>"#
    );
    res.set_manifest(&manifest);
    if std::path::Path::new("assets/neramitcut.ico").exists() {
        res.set_icon("assets/neramitcut.ico");
    }
    res.compile().expect("compile Windows resources");
}
