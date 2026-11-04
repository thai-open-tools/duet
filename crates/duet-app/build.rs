//! Embeds the app icon (`assets/duet.ico`, drawn by `scripts/make-icon.py`) in duet.exe as icon
//! resource 1. gpui loads resource 1 for the window and the taskbar, and Explorer shows it for
//! the exe and its shortcuts. Uses the Windows SDK's rc.exe (or `RC`); without one the build
//! goes on with a warning and an exe without an icon. The same resource script carries the
//! version block (product name, version, copyright) that Explorer and the installer show.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/duet.ico");
    println!("cargo:rerun-if-env-changed=RC");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(rc) = find_rc() else {
        println!(
            "cargo:warning=rc.exe not found: duet.exe is built without its icon (install the Windows SDK or set RC)"
        );
        return;
    };
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("set by cargo"));
    let icon = manifest.join("assets").join("duet.ico");
    let script = out.join("duet.rc");
    let quoted = icon.display().to_string().replace('\\', "\\\\");
    std::fs::write(&script, format!("1 ICON \"{quoted}\"\n{}", version_info()))
        .expect("write duet.rc");
    let res = out.join("duet.res");
    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg(&script)
        .status();
    match status {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg-bin=duet={}", res.display());
        }
        Ok(status) => {
            println!("cargo:warning=rc.exe failed ({status}): duet.exe is built without its icon")
        }
        Err(err) => {
            println!("cargo:warning=rc.exe did not run ({err}): duet.exe is built without its icon")
        }
    }
}

/// The version block Explorer shows under Properties > Details, and the installer reads.
fn version_info() -> String {
    let version = std::env::var("CARGO_PKG_VERSION").expect("set by cargo");
    let mut parts: Vec<u32> = version
        .split(['.', '-'])
        .take(3)
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    parts.resize(4, 0);
    let numeric = parts
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "ball46\0"
      VALUE "FileDescription", "duet compare\0"
      VALUE "FileVersion", "{version}\0"
      VALUE "InternalName", "duet\0"
      VALUE "LegalCopyright", "Copyright (c) 2026 ball46. MIT License.\0"
      VALUE "OriginalFilename", "duet.exe\0"
      VALUE "ProductName", "duet compare\0"
      VALUE "ProductVersion", "{version}\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}

/// `RC` when set, else the newest Windows 10/11 SDK's rc.exe for the target architecture.
fn find_rc() -> Option<PathBuf> {
    if let Some(rc) = std::env::var_os("RC") {
        return Some(rc.into());
    }
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x64",
        Ok("aarch64") => "arm64",
        Ok("x86") => "x86",
        _ => return None,
    };
    let root = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    let bin = root.join("Windows Kits").join("10").join("bin");
    let mut found: Vec<(Vec<u32>, PathBuf)> = std::fs::read_dir(&bin)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version: Option<Vec<u32>> = name.split('.').map(|part| part.parse().ok()).collect();
            let rc = entry.path().join(arch).join("rc.exe");
            Some((version?, rc)).filter(|(_, rc)| rc.is_file())
        })
        .collect();
    found.sort();
    found
        .pop()
        .map(|(_, rc)| rc)
        .or_else(|| Some(bin.join(arch).join("rc.exe")).filter(|rc| Path::new(rc).is_file()))
}
