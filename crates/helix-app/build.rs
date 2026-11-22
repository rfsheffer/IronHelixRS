//! Gives the Windows program an icon when `pack windows` asks for one.
//!
//! `pack windows` makes the probe icon from the player's own copy of the game
//! and passes it in `IRONHELIX_ICON`; it's compiled with the Windows SDK's
//! resource compiler and linked in. Ordinary builds have no icon (nothing
//! from the game is part of the source), and the window still shows the
//! probe, read from the game files when it opens.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=IRONHELIX_ICON");
    let Ok(ico) = std::env::var("IRONHELIX_ICON") else { return };
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed={ico}");
    let Some(rc) = resource_compiler() else {
        println!("cargo:warning=no Windows SDK resource compiler (rc.exe) found; the program gets no icon");
        return;
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let script = out.join("icon.rc");
    let res = out.join("icon.res");
    std::fs::write(&script, format!("1 ICON \"{}\"\n", ico.replace('\\', "\\\\"))).unwrap();
    let ok = Command::new(rc).arg("/nologo").arg("/fo").arg(&res).arg(&script).status().is_ok_and(|s| s.success());
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    } else {
        println!("cargo:warning=rc.exe failed; the program gets no icon");
    }
}

/// The newest `rc.exe` in the Windows 10/11 SDK.
fn resource_compiler() -> Option<PathBuf> {
    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(kits).ok()?.flatten().map(|e| e.path()).filter(|p| p.join("x64").join("rc.exe").is_file()).collect();
    versions.sort();
    versions.pop().map(|v| v.join("x64").join("rc.exe"))
}
