use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

static EMBEDDED_ADAPTER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded_adapter.bin"));

pub fn embedded_size() -> usize {
    EMBEDDED_ADAPTER.len()
}

pub fn materialize(cache_dir: &Path) -> Result<PathBuf> {
    if EMBEDDED_ADAPTER.is_empty() {
        bail!("the Fabric 1.21.1 Adapter is not embedded; run Gradle build before cargo release");
    }
    let destination = cache_dir
        .join("adapters")
        .join("mcbenchmark-fabric-1.21.1.jar");
    let parent = destination
        .parent()
        .context("embedded Adapter destination has no parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create Adapter cache {}", parent.display()))?;
    if destination.is_file() {
        let existing = fs::read(&destination)
            .with_context(|| format!("failed to read cached Adapter {}", destination.display()))?;
        if existing == EMBEDDED_ADAPTER {
            return Ok(destination);
        }
    }
    let temporary = destination.with_extension("jar.tmp");
    fs::write(&temporary, EMBEDDED_ADAPTER)
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
