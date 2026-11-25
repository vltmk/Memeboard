fn main() {
    tauri_build::build();

    #[cfg(target_os = "windows")]
    configure_windows_gnu_manifests();
}

#[cfg(target_os = "windows")]
fn configure_windows_gnu_manifests() {
    use std::path::PathBuf;
    use std::process::Command;

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "gnu" {
        return;
    }

    let out_dir = match std::env::var_os("OUT_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => return,
    };

    // 1. Strip MinGW GCC 16's `default-manifest.o` via a dynamic `*endfile:` specs override
    //    so GNU ld never hits `.rsrc merge failure: multiple non-default manifests` when linking
    //    tauri-build's resource.rc (and so unit-test harnesses get a clean single manifest).
    let cc = std::env::var("CC").unwrap_or_else(|_| "gcc".to_string());
    if let Ok(output) = Command::new(&cc).arg("-dumpspecs").output() {
        if output.status.success() {
            let specs = String::from_utf8_lossy(&output.stdout);
            if let Some(endfile_body) = extract_spec_section(&specs, "*endfile:") {
                let cleaned =
                    endfile_body.replace("%{!shared:%:if-exists(default-manifest.o%s)}", "");
                let spec_path = out_dir.join("no-default-manifest.specs");
                let content = format!("*endfile:\n{}\n", cleaned.trim());
                if std::fs::write(&spec_path, content).is_ok() {
                    println!("cargo:rustc-link-arg=-specs={}", spec_path.display());
                }
            }
        }
    }

    // 2. Build a dedicated static manifest archive `libtest_manifest.a` linked ONLY under
    //    `#[cfg(test)]` in `src/lib.rs`. `tauri-build` emits `cargo:rustc-link-arg-bins=...`
    //    which skips `cargo test --lib`; without a Common Controls v6 manifest, test binaries
    //    fail at launch with STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139) on TaskDialogIndirect.
    let manifest_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;
    let manifest_path = out_dir.join("test_manifest.xml");
    let rc_path = out_dir.join("test_manifest.rc");
    let obj_path = out_dir.join("test_manifest.o");
    let lib_path = out_dir.join("libtest_manifest.a");

    let rc_content = format!(
        "1 24 \"{}\"\n",
        manifest_path.to_string_lossy().replace('\\', "/")
    );
    if std::fs::write(&manifest_path, manifest_xml).is_ok()
        && std::fs::write(&rc_path, rc_content).is_ok()
    {
        let windres_ok = ["windres", "x86_64-w64-mingw32-windres"].iter().any(|tool| {
            Command::new(tool)
                .args([
                    "-i",
                    &rc_path.to_string_lossy(),
                    "-o",
                    &obj_path.to_string_lossy(),
                    "--output-format=coff",
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        });

        if windres_ok {
            let _ = std::fs::remove_file(&lib_path);
            let ar_ok = ["ar", "x86_64-w64-mingw32-ar"].iter().any(|tool| {
                Command::new(tool)
                    .args([
                        "rcs",
                        &lib_path.to_string_lossy(),
                        &obj_path.to_string_lossy(),
                    ])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            });

            if ar_ok && lib_path.exists() {
                println!("cargo:rustc-link-search=native={}", out_dir.display());
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn extract_spec_section(specs: &str, header: &str) -> Option<String> {
    let mut lines = specs.lines();
    while let Some(line) = lines.next() {
        if line.trim() == header {
            let mut body = String::new();
            for next in lines {
                if next.starts_with('*') && next.trim_end().ends_with(':') {
                    break;
                }
                if !next.trim().is_empty() {
                    if !body.is_empty() {
                        body.push(' ');
                    }
                    body.push_str(next.trim());
                }
            }
            return Some(body);
        }
    }
    None
}
