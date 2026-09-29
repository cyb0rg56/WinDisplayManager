const ICON: &str = "resources/icons/icon.ico";
const MANIFEST: &str = "resources/app.manifest";

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed={MANIFEST}");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }

    let mut res = winresource::WindowsResource::new();
    // data/src/tray.rs loads the tray icon from resource ID 1.
    res.set_icon_with_id(ICON, "1");
    res.set("ProductName", "WinDisplayManager");
    res.set(
        "FileDescription",
        "WinDisplayManager - DDC/CI monitor control",
    );
    res.set("LegalCopyright", "Copyright (c) 2026 cyb0rg56");
    res.set_manifest_file(MANIFEST);
    res.compile()
}
