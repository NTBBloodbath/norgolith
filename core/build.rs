use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Only rebuild if git state changes
    println!("cargo:rerun-if-changed=.git/HEAD");

    let version = get_version();
    println!("cargo:rustc-env=LITH_VERSION={}", version);

    // The overhead of compiling test plugins without a compilation feature flag
    // is small enough not to care about it. I cannot be bothered to add '--features test-plugins'
    // to the 'cargo nextest run' command arguments every time and complaining when I forget it
    compile_test_plugins();
    compile_rust_test_plugins();
}

fn get_version() -> String {
    // Check if current commit is tagged with v*
    let output = Command::new("git")
        .args(["describe", "--exact-match", "--tags", "HEAD"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let tag = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return tag.strip_prefix('v').unwrap_or(&tag).to_string();
        }
    }

    // Not at a tag, append commit hash
    let cargo_version = env!("CARGO_PKG_VERSION");
    let hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    if hash.is_empty() {
        cargo_version.to_string()
    } else {
        format!("{}+{}", cargo_version, hash)
    }
}

fn compile_test_plugins() {
    let plugins_dir = PathBuf::from("tests/plugins");
    if !plugins_dir.is_dir() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_dir = out_dir.parent().unwrap().parent().unwrap(); // target/debug or target/release

    let cc = "cc";

    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("c") {
            continue;
        }

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap();
        let lib_name = if cfg!(target_os = "macos") {
            format!("lib{}.dylib", stem)
        } else {
            format!("lib{}.so", stem)
        };

        let out_path = target_dir.join(&lib_name);

        let mut cmd = Command::new(cc);
        cmd.arg("-shared")
            .arg("-fPIC")
            .arg("-o")
            .arg(&out_path)
            .arg(&path);

        #[cfg(target_os = "macos")]
        cmd.arg("-undefined").arg("dynamic_lookup");

        let status = cmd.status();
        if status.as_ref().map(|s| !s.success()).unwrap_or(true) {
            // Non-fatal: test plugins are optional
            println!(
                "cargo:warning=failed to compile test plugin {}: {:?}",
                stem, status
            );
        }
    }
}

fn compile_rust_test_plugins() {
    let plugins_dir = PathBuf::from("tests/plugins");
    if !plugins_dir.is_dir() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_dir = out_dir.parent().unwrap().parent().unwrap();

    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() || !path.join("Cargo.toml").exists() {
            continue;
        }

        let name = path.file_name().and_then(|s| s.to_str()).unwrap();

        let mut cmd = Command::new("cargo");
        cmd.arg("build")
            .arg("--release")
            .arg("--manifest-path")
            .arg(path.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target_dir);

        let status = cmd.status();
        if status.as_ref().map(|s| !s.success()).unwrap_or(true) {
            println!(
                "cargo:warning=failed to compile Rust test plugin {}: {:?}",
                name, status
            );
        }
    }
}
