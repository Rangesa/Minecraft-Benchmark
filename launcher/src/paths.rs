use anyhow::{Context, Result, bail};
use std::env;
use std::path::{Path, PathBuf};

/// Resolve MineBench's local data root without using the source Minecraft
/// instance as a write target.
pub fn default_data_dir() -> PathBuf {
    if let Some(path) = env::var_os("MINEBENCH_DATA_DIR") {
        return PathBuf::from(path);
    }

    if let Some(path) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path).join("MineBench");
    }

    env::temp_dir().join("MineBench")
}

pub fn find_mock_binary() -> Result<PathBuf> {
    if let Some(path) = env::var_os("MINEBENCH_MOCK_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        bail!(
            "MINEBENCH_MOCK_BIN does not point to a file: {}",
            path.display()
        );
    }

    let current = env::current_exe().context("failed to resolve current launcher executable")?;
    if current.is_file() {
        Ok(current)
    } else {
        bail!(
            "current launcher executable is not a file: {}",
            current.display()
        )
    }
}

pub fn ensure_within(base: &Path, candidate: &Path) -> Result<()> {
    let base = base
        .canonicalize()
        .with_context(|| format!("failed to canonicalize base path {}", base.display()))?;
    let candidate = candidate
        .canonicalize()
        .with_context(|| format!("failed to canonicalize path {}", candidate.display()))?;
    if candidate.starts_with(&base) {
        Ok(())
    } else {
        bail!(
            "path escapes expected base directory: {}",
            candidate.display()
        )
    }
}
