//! Windows only: put the icon and version details into the .exe, so Explorer and
//! Task Manager show "Cache Deck" with its icon. Does nothing on other systems.

fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/build.rs");
    println!("cargo:rerun-if-changed=assets/app-icon/icon.ico");

    // `cfg(windows)` here means the machine doing the build. The resource compiler only
    // exists there, so the Windows build has to run on Windows.
    #[cfg(windows)]
    embed_windows_resources();
}

#[cfg(windows)]
fn embed_windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app-icon/icon.ico");
    res.set("ProductName", "Cache Deck");
    res.set("FileDescription", "Cache Deck");
    res.set("CompanyName", "Shaon Dey");
    res.set("LegalCopyright", "© 2026 Shaon Dey");
    res.compile().expect("embed the Windows icon and version details");
}
