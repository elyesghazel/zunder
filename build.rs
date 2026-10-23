//! zunder.exe gets the zunder icon (Explorer, taskbar, Start menu). A no-op on Linux and macOS.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let ico = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons/zunder.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    // written with an absolute path, so the resource compiler finds the icon from any directory
    let rc = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("zunder.rc");
    let path = ico.display().to_string().replace('\\', "/");
    std::fs::write(&rc, format!("1 ICON \"{path}\"\n")).unwrap();
    embed_resource::compile(&rc, embed_resource::NONE).manifest_optional().unwrap();
}
