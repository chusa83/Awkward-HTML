// Embeds the app icon in the Windows executable. No-op on other platforms.
fn main() {
    println!("cargo:rerun-if-changed=branding/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("branding/icon.ico");
        res.set("ProductName", "awkward HTML");
        res.set("FileDescription", "awkward HTML");
        res.compile().expect("failed to embed Windows icon");
    }
}
