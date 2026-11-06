use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/embedded_adapters.rs"));

pub fn embedded_size() -> usize {
    ADAPTERS.iter().map(|(_, bytes)| bytes.len()).sum()
}

pub fn materialize(cache_dir: &Path) -> Result<PathBuf> {
    materialize_for_version(cache_dir, "1.21.1")
}

pub fn materialize_for_version(cache_dir: &Path, minecraft_version: &str) -> Result<PathBuf> {
    crate::catalog::target(minecraft_version)?;
    let bytes = ADAPTERS
        .iter()
        .find(|(version, _)| *version == minecraft_version)
        .map(|(_, bytes)| *bytes)
        .context("no embedded Adapter bytes for this Minecraft version")?;
    if bytes.is_empty() {
        bail!("the Adapter for Minecraft {minecraft_version} was not built; run python scripts/build_versions.py");
    }
    let destination = cache_dir
        .join("adapters")
        .join(format!("mcbenchmark-fabric-{minecraft_version}.jar"));
    let parent = destination
        .parent()
        .context("embedded Adapter destination has no parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create Adapter cache {}", parent.display()))?;
    if destination.is_file() {
        let existing = fs::read(&destination)
            .with_context(|| format!("failed to read cached Adapter {}", destination.display()))?;
        if existing == bytes {
            return Ok(destination);
        }
    }
    let temporary = destination.with_extension("jar.tmp");
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write embedded Adapter {}", temporary.display()))?;
    if destination.exists() {
        fs::remove_file(&destination).with_context(|| {
            format!("failed to replace cached Adapter {}", destination.display())
        })?;
    }
    fs::rename(&temporary, &destination).with_context(|| {
        format!(
            "failed to finalize embedded Adapter {}",
            destination.display()
        )
    })?;
    Ok(destination)
}
