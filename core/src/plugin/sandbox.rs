use std::path::Path;

use eyre::Result;
use tracing::warn;

/// Apply Landlock filesystem restrictions to the current process
///
/// Restricts access to:
/// - `site_dir` (read/write)
/// - `site_dir/public` (write output)
/// - `site_dir/plugins` (read .so files)
/// - Cache directory (read/write)
///
/// Once applied, restrictions are irreversible for the process lifetime.
/// On non-Linux platforms or without `sandbox-linux` feature, this is a no-op.
pub fn apply_landlock(site_dir: &Path) -> Result<()> {
    #[cfg(not(all(target_os = "linux", feature = "sandbox-linux")))]
    {
        let _ = site_dir;
        Ok(())
    }

    #[cfg(all(target_os = "linux", feature = "sandbox-linux"))]
    {
        if !landlock_available() {
            warn!(
                "Landlock unavailable (kernel too old?). \
                 Plugins running without filesystem confinement."
            );
            return Ok(());
        }

        use landlock::*;

        let public_dir = site_dir.join("public");
        let plugins_dir = site_dir.join("plugins");
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("norgolith");

        // Ensure dirs exist for path_beneath_rules (PathFd::new requires the path)
        let _ = std::fs::create_dir_all(&public_dir);
        let _ = std::fs::create_dir_all(&plugins_dir);
        let _ = std::fs::create_dir_all(&cache_dir);

        let allowed_paths: Vec<&Path> = vec![
            site_dir,
            &public_dir,
            &plugins_dir,
            &cache_dir,
        ];

        let access = AccessFs::from_all(ABI::V1);

        let ruleset = Ruleset::default()
            .handle_access(access)
            .map_err(|e| eyre::eyre!("failed to create landlock ruleset: {}", e))?
            .create()
            .map_err(|e| eyre::eyre!("failed to create landlock ruleset: {}", e))?;

        let rules = path_beneath_rules(&allowed_paths, access);

        let ruleset = ruleset
            .add_rules(rules)
            .map_err(|e| eyre::eyre!("failed to add landlock rules: {}", e))?;

        match ruleset.restrict_self() {
            Ok(status) => {
                if status.ruleset != RulesetStatus::FullyEnforced {
                    warn!(
                        "Landlock partially enforced (kernel feature gap). \
                         Some restrictions may not apply."
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to apply Landlock restrictions: {}. \
                     Plugins running without filesystem confinement.",
                    e
                );
            }
        }

        Ok(())
    }
}

/// Check if Landlock is available on this system
#[cfg(all(target_os = "linux", feature = "sandbox-linux"))]
fn landlock_available() -> bool {
    // Try the landlock_create_ruleset syscall with version query
    let ret = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<libc::c_void>(),
            0usize,
            1u32, // LANDLOCK_CREATE_RULESET_VERSION
        )
    };
    // ret > 0 means supported, ENOSYS means not supported
    if ret > 0 {
        return true;
    }
    let errno = unsafe { *libc::__errno_location() };
    errno != libc::ENOSYS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_landlock_no_panic() {
        let tmp = tempfile::tempdir().unwrap();
        // Should not panic, even if Landlock is unavailable
        let _ = apply_landlock(tmp.path());
    }
}
