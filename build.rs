//! zunder.exe gets the zunder icon (Explorer, taskbar, Start menu) and version info (product
//! name, version, publisher; code signing requires them). A no-op on Linux and macOS.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let ico = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons/zunder.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    let ver = env!("CARGO_PKG_VERSION");
    // FILEVERSION wants four numbers: 0.5.0-rc1 -> 0,5,0,0
    let mut nums: Vec<u16> = ver.split(['.', '-', '+']).take(3).map(|n| n.parse().unwrap_or(0)).collect();
    nums.resize(4, 0);
    let nums = nums.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    // written with an absolute path, so the resource compiler finds the icon from any directory
    let rc = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("zunder.rc");
    let path = ico.display().to_string().replace('\\', "/");
    std::fs::write(
        &rc,
        format!(
            r#"1 ICON "{path}"

1 VERSIONINFO
FILEVERSION {nums}
PRODUCTVERSION {nums}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Elyes Ghazel"
      VALUE "FileDescription", "zunder launcher"
      VALUE "FileVersion", "{ver}"
      VALUE "InternalName", "zunder"
      VALUE "LegalCopyright", "Copyright (c) 2026 Elyes Ghazel, MIT License"
      VALUE "OriginalFilename", "zunder.exe"
      VALUE "ProductName", "zunder"
      VALUE "ProductVersion", "{ver}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
        ),
    )
    .unwrap();
    embed_resource::compile(&rc, embed_resource::NONE).manifest_optional().unwrap();
}
