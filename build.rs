use std::process::Command;

fn main() {
    // Only rebuild if git state changes
    println!("cargo:rerun-if-changed=.git/HEAD");

    let version = get_version();
    println!("cargo:rustc-env=LITH_VERSION={}", version);
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
