//! Build script: embed the peacock-feather exe icon (assets/icon.ico) via
//! the Windows SDK resource compiler when one is available. New SDK
//! versions are preferred; without rc.exe the build still succeeds, just
//! without an Explorer icon (the in-app window icon is separate).
use std::path::PathBuf;

fn find_rc_exe() -> Option<PathBuf> {
    // 1. PATH (VS developer prompts, some CI images).
    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join("rc.exe");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    // 2. Well-known Windows SDK install locations, newest kit first.
    let roots = [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ];
    let mut best: Option<(String, PathBuf)> = None;
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("10.") {
                continue;
            }
            for arch in ["x64", "x86"] {
                let p = entry.path().join(arch).join("rc.exe");
                if p.is_file() && best.as_ref().is_none_or(|(v, _)| name > *v) {
                    best = Some((name.clone(), p));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico = manifest
        .join("..")
        .join("..")
        .join("assets")
        .join("icon.ico");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc_path = out.join("procraft-icon.rc");
    let res_path = out.join("procraft-icon.res");
    // NOTE: backslashes must be doubled: RC treats "\a" etc. as escapes.
    let ico_escaped = ico.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc_path, format!("1 ICON \"{ico_escaped}\"\n")).unwrap();

    let Some(rc_exe) = find_rc_exe() else {
        println!("cargo:warning=rc.exe not found: building without an embedded exe icon");
        return;
    };
    let status = std::process::Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_path)
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-arg-bins={}", res_path.display());
        }
        other => {
            println!(
                "cargo:warning=rc.exe failed ({other:?}): building without an embedded exe icon"
            );
        }
    }
}
