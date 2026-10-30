//! Builds the macOS helper processes the app drives (`resources/native/*.swift`)
//! and the vendored MediaRemoteAdapter framework into `resources/bin`.
//!
//! Whisper ships for Apple silicon only, so the helpers are built for arm64.
//! A missing toolchain only produces a cargo warning: the Rust code degrades
//! gracefully when a helper is absent. Set `WHISPER_SKIP_NATIVE_BUILD=1` to
//! skip this step entirely.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

const CPU_TYPE_ARM64: i32 = 0x0100_000c;
const MH_MAGIC_64: u32 = 0xfeed_facf;
const SWIFT_TARGET: &str = "arm64-apple-macosx11.0";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=resources/native");
    println!("cargo:rerun-if-changed=resources/mediaremote-adapter");
    println!("cargo:rerun-if-changed=scripts/build-mediaremote-adapter.sh");
    println!("cargo:rerun-if-env-changed=WHISPER_SKIP_NATIVE_BUILD");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    if std::env::var_os("WHISPER_SKIP_NATIVE_BUILD").is_some_and(|v| v != "0") {
        return;
    }

    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("aarch64") {
        warn("native helpers are built for arm64 only; skipping");
        return;
    }

    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));

    let native_dir = root.join("resources").join("native");
    let out_dir = root.join("resources").join("bin");
    if let Err(error) = std::fs::create_dir_all(&out_dir) {
        warn(&format!("cannot create {}: {error}", out_dir.display()));
        return;
    }

    let mut sources: Vec<PathBuf> = std::fs::read_dir(&native_dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension().is_some_and(|ext| ext == "swift")
                        && path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("macos-"))
                })
                .collect()
        })
        .unwrap_or_default();
    sources.sort();

    for source in &sources {
        build_swift_helper(source, &out_dir);
    }

    build_media_remote_adapter(&root);
}

fn warn(message: &str) {
    println!("cargo:warning=[native] {message}");
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn is_arm64_binary(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    if bytes.len() < 8 {
        return false;
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let cpu = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    magic == MH_MAGIC_64 && cpu == CPU_TYPE_ARM64
}

/// Frameworks each helper links explicitly.
fn frameworks_for(name: &str) -> &'static [&'static str] {
    match name {
        "macos-mic-listener" => &["Foundation", "IOKit"],
        _ => &[],
    }
}

fn build_swift_helper(source: &Path, out_dir: &Path) {
    let Some(name) = source.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let output = out_dir.join(name);

    let up_to_date = match (mtime(&output), mtime(source)) {
        (Some(built), Some(src)) => built >= src && is_arm64_binary(&output),
        _ => false,
    };
    if up_to_date {
        return;
    }

    let module_cache = out_dir.join(".swift-module-cache");
    let _ = std::fs::create_dir_all(&module_cache);

    let mut args: Vec<String> = vec![
        source.display().to_string(),
        "-O".into(),
        "-target".into(),
        SWIFT_TARGET.into(),
        "-module-cache-path".into(),
        module_cache.display().to_string(),
        "-o".into(),
        output.display().to_string(),
    ];
    for framework in frameworks_for(name) {
        args.push("-framework".into());
        args.push((*framework).into());
    }

    let compile = |program: &str, prefix: &[&str]| {
        Command::new(program)
            .args(prefix)
            .args(&args)
            .env("SWIFT_MODULE_CACHE_PATH", &module_cache)
            .output()
    };

    let mut result = compile("xcrun", &["swiftc"]);
    if !matches!(&result, Ok(out) if out.status.success()) {
        result = compile("swiftc", &[]);
    }

    match result {
        Ok(out) if out.status.success() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o755));
            }
            if !is_arm64_binary(&output) {
                warn(&format!(
                    "{name}: compiled binary is not arm64 - helper may not run"
                ));
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let first = stderr.lines().find(|l| l.contains("error")).unwrap_or("");
            warn(&format!("failed to compile {name} with swiftc: {first}"));
        }
        Err(error) => warn(&format!("swiftc unavailable, skipping {name}: {error}")),
    }
}

fn build_media_remote_adapter(root: &Path) {
    let script = root.join("scripts").join("build-mediaremote-adapter.sh");
    if !script.is_file() {
        return;
    }
    match Command::new("/bin/bash")
        .arg(&script)
        .current_dir(root)
        .output()
    {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let last = stderr.lines().last().unwrap_or("");
            warn(&format!(
                "MediaRemoteAdapter.framework build failed: {last}"
            ));
        }
        Err(error) => warn(&format!("cannot run {}: {error}", script.display())),
    }
}
