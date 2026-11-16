//! Windows: the program icon and version details in Nexus.exe.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let icon = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../icons/nexus.ico");
    println!("cargo:rerun-if-changed={icon}");
    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon);
    res.set("ProductName", "Nexus");
    res.set("FileDescription", "Nexus music player");
    res.set("CompanyName", "GameBitBan");
    res.set("LegalCopyright", "GPL-3.0-or-later");
    if let Err(e) = res.compile() {
        println!("cargo:warning=couldn't embed the icon: {e}");
    }
}
