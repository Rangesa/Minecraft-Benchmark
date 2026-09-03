use crate::manifest::{build_manifest_with_config, new_run_id};
use crate::modscan::ModRecord;
use crate::paths::ensure_within;
use crate::protocol::{BenchmarkConfig, CaptureConfig, RunManifest};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PrepareRunRequest {
    pub data_dir: PathBuf,
    pub minecraft_version: String,
    pub loader: String,
    pub selected_mods: Vec<ModRecord>,
    pub source_instance_dir: Option<PathBuf>,
    pub benchmark: BenchmarkConfig,
}

#[derive(Debug, Clone)]
pub struct PreparedRun {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub instance_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub status_path: PathBuf,
    pub result_dir: PathBuf,
    pub run_json_path: PathBuf,
    pub manifest: RunManifest,
}

pub fn prepare_run(request: &PrepareRunRequest) -> Result<PreparedRun> {
    let run_id = new_run_id();
    if let Some(source_instance_dir) = &request.source_instance_dir {
        let source = source_instance_dir.canonicalize().with_context(|| {
            format!(
                "failed to resolve source instance {}",
                source_instance_dir.display()
            )
        })?;
        let data_dir = resolve_for_containment(&request.data_dir)?;
        if data_dir.starts_with(&source) {
            bail!(
                "MineBench data directory must be outside the source instance to preserve it: {}",
                data_dir.display()
            );
        }
    }
    let run_dir = request.data_dir.join("runs").join(&run_id);
    let instance_dir = run_dir.join("instance");
    let instance_mods_dir = instance_dir.join("mods");
    let minebench_dir = instance_dir.join("minebench");
    let result_dir = minebench_dir.join("result");
    let logs_dir = run_dir.join("logs");

    fs::create_dir_all(&instance_mods_dir)
        .with_context(|| format!("failed to create {}", instance_mods_dir.display()))?;
    fs::create_dir_all(&result_dir)
        .with_context(|| format!("failed to create {}", result_dir.display()))?;
    fs::create_dir_all(&logs_dir)
        .with_context(|| format!("failed to create {}", logs_dir.display()))?;

    if let Some(source_instance_dir) = &request.source_instance_dir {
        copy_instance_settings(source_instance_dir, &instance_dir)?;
    }

    let manifest = build_manifest_with_config(
        run_id.clone(),
        &request.minecraft_version,
        &request.loader,
        &request.selected_mods,
        request.benchmark.clone(),
    );
    write_selected_mods(&instance_mods_dir, &request.selected_mods)?;

    let manifest_path = run_dir.join("manifest.json");
    write_json_atomic(&manifest_path, &manifest)?;
    let run_json_path = minebench_dir.join("run.json");
    write_json_atomic(&run_json_path, &manifest)?;

    Ok(PreparedRun {
        run_id,
        run_dir,
        instance_dir: instance_dir.clone(),
        manifest_path,
        status_path: instance_dir.join("minebench").join("status.json"),
        result_dir,
        run_json_path,
        manifest,
    })
}

fn resolve_for_containment(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current working directory")?
            .join(path)
    };
    let mut existing = absolute.clone();
    let mut missing = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .with_context(|| format!("cannot resolve path {}", absolute.display()))?;
        missing.push(name.to_os_string());
        if !existing.pop() {
            bail!("cannot resolve path {}", absolute.display());
        }
    }
    let mut resolved = existing
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", existing.display()))?;
    for name in missing.iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

/// Copy only settings/assets that affect a benchmark. Saves, logs, accounts,
/// launcher state, and caches are intentionally omitted so the source
/// instance is never opened for writing and never copied wholesale.
fn copy_instance_settings(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("source instance is not a directory: {}", source.display());
    }
    let source = source
        .canonicalize()
        .with_context(|| format!("failed to resolve source instance {}", source.display()))?;
    if source
        == destination
            .canonicalize()
            .unwrap_or_else(|_| destination.to_path_buf())
    {
        bail!("source instance and temporary instance must be different");
    }

    for directory in ["config", "resourcepacks", "shaderpacks"] {
        let source_dir = source.join(directory);
        if source_dir.is_dir() {
            copy_tree_without_links(&source_dir, &destination.join(directory))?;
        }
    }
    for filename in ["options.txt", "optionsof.txt", "optionsshaders.txt"] {
        let source_file = source.join(filename);
        if source_file.is_file() {
            copy_file_without_link(&source_file, &destination.join(filename))?;
        }
    }
    Ok(())
}

fn copy_tree_without_links(source: &Path, destination: &Path) -> Result<()> {
    let source_type = fs::symlink_metadata(source)
        .with_context(|| format!("failed to inspect {}", source.display()))?
        .file_type();
    if source_type.is_symlink() {
        bail!(
            "symbolic-link instance settings are not accepted: {}",
            source.display()
        );
    }
    fs::create_dir_all(destination).with_context(|| {
        format!(
            "failed to create settings directory {}",
            destination.display()
        )
    })?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read settings directory {}", source.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", source.display()))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", source_path.display()))?;
        if file_type.is_symlink() {
            bail!(
                "symbolic-link instance settings are not accepted: {}",
                source_path.display()
            );
        }
        if file_type.is_dir() {
            copy_tree_without_links(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            copy_file_without_link(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn copy_file_without_link(source: &Path, destination: &Path) -> Result<()> {
    let file_type = fs::symlink_metadata(source)
        .with_context(|| format!("failed to inspect {}", source.display()))?
        .file_type();
    if !file_type.is_file() || file_type.is_symlink() {
        bail!(
            "source settings file is not a regular file: {}",
            source.display()
        );
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "failed to copy settings file {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

pub fn update_run_manifest(
    prepared: &mut PreparedRun,
    capture: CaptureConfig,
    hardware: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    prepared.manifest.capture = capture;
    prepared.manifest.hardware = hardware;
    write_json_atomic(&prepared.manifest_path, &prepared.manifest)?;
    write_json_atomic(&prepared.run_json_path, &prepared.manifest)?;
    Ok(())
}

fn write_selected_mods(destination_dir: &Path, mods: &[ModRecord]) -> Result<()> {
    for record in mods {
        let source = &record.source_path;
        if !source.is_file() {
            bail!("selected JAR no longer exists: {}", source.display());
        }
        let source_root = source
            .parent()
            .context("selected JAR has no parent directory")?;
        ensure_within(source_root, source)?;
        if fs::symlink_metadata(source)
            .with_context(|| format!("failed to inspect {}", source.display()))?
            .file_type()
            .is_symlink()
        {
            bail!("symbolic-link JARs are not accepted: {}", source.display());
        }

        let filename = safe_filename(&record.filename)?;
        let destination = destination_dir.join(filename);
        match fs::hard_link(source, &destination) {
            Ok(()) => {}
            Err(_) => {
                fs::copy(source, &destination).with_context(|| {
                    format!(
                        "failed to copy selected JAR {} to {}",
                        source.display(),
                        destination.display()
                    )
                })?;
            }
        }
    }
    Ok(())
}

fn safe_filename(filename: &str) -> Result<&str> {
    let path = Path::new(filename);
    if filename.is_empty() || path.file_name().and_then(|value| value.to_str()) != Some(filename) {
        bail!("unsafe selected JAR filename: {filename}");
    }
    Ok(filename)
}

pub fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .context("JSON output path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create JSON output directory {}",
            parent.display()
        )
    })?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let bytes = serde_json::to_vec_pretty(value).context("failed to serialize JSON")?;
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write temporary JSON {}", temporary.display()))?;
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("failed to replace JSON {}", path.display()))?;
    }
    fs::rename(&temporary, path)
        .with_context(|| format!("failed to finalize JSON {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modscan::scan_mods;
    use std::io::Write;
    use uuid::Uuid;
    use zip::write::SimpleFileOptions;

    #[test]
    fn selected_mods_are_hardlinked_or_copied_into_new_run_only() {
        let root = std::env::temp_dir().join(format!("minebench-instance-{}", Uuid::new_v4()));
        let source = root.join("source-instance");
        let source_mods = source.join("mods");
        fs::create_dir_all(&source_mods).unwrap();
        let jar = source_mods.join("example.jar");
        let file = fs::File::create(&jar).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("fabric.mod.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(br#"{"schemaVersion":1,"id":"example","version":"1"}"#)
            .unwrap();
        writer.finish().unwrap();
        let mods = scan_mods(&source_mods).unwrap();

        let prepared = prepare_run(&PrepareRunRequest {
            data_dir: root.join("data"),
            minecraft_version: "1.21.1".to_string(),
            loader: "fabric".to_string(),
            selected_mods: mods,
            source_instance_dir: Some(source.clone()),
            benchmark: BenchmarkConfig::default(),
        })
        .unwrap();
        assert!(prepared.instance_dir.join("mods/example.jar").is_file());
        assert!(jar.is_file());
        assert!(prepared.instance_dir.join("minebench/run.json").is_file());
        assert!(prepared.manifest.autorun);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn data_directory_inside_source_instance_is_rejected() {
        let root = std::env::temp_dir().join(format!("minebench-instance-{}", Uuid::new_v4()));
        let source = root.join("source-instance");
        fs::create_dir_all(source.join("mods")).unwrap();
        let result = prepare_run(&PrepareRunRequest {
            data_dir: source.join("minebench-data"),
            minecraft_version: "1.21.1".to_string(),
            loader: "fabric".to_string(),
            selected_mods: Vec::new(),
            source_instance_dir: Some(source),
            benchmark: BenchmarkConfig::default(),
        });
        assert!(result.is_err());
        let _ = fs::remove_dir_all(root);
    }
}
