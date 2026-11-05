//! Windows only: embeds the executable's icon and version metadata as a resource, so Explorer,
//! the Start menu and the taskbar show the CSV Editor icon for the `.exe` itself. The window's
//! own icon is set at run time (`src/icon.rs`), on every platform. Other targets do nothing.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(target_os = "windows")]
    windows::embed();
}

#[cfg(target_os = "windows")]
mod windows {
    pub fn embed() {
        println!("cargo:rerun-if-changed=../../assets/icon/csv-editor.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/csv-editor.ico")
            .set("ProductName", "CSV Editor")
            .set("FileDescription", "CSV Editor")
            .set("LegalCopyright", "")
            .set("OriginalFilename", "csv-editor.exe");
        if let Err(e) = res.compile() {
            // No resource compiler (Windows SDK) on this machine: the build still succeeds, the
            // executable just has no embedded icon. Said out loud so it is not missed.
            println!("cargo:warning=icon resource not embedded: {e}");
        }
    }
}
