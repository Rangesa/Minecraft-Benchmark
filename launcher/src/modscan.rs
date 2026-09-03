use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha512};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

const MAX_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModRecord {
    pub filename: String,
    pub file_size: u64,
    pub sha512: String,
    pub loader_family: String,
    pub mod_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub dependency_metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub minecraft_version_constraints: Vec<String>,
    pub source: String,
    #[serde(skip)]
    pub source_path: PathBuf,
    #[serde(default = "default_selected")]
    pub selected: bool,
    #[serde(default)]
    pub metadata_warning: Option<String>,
}

fn default_selected() -> bool {
    true
}

#[derive(Debug, Default)]
struct JarMetadata {
    loader_family: String,
    mod_id: Option<String>,
    name: Option<String>,
    version: Option<String>,
    dependency_metadata: BTreeMap<String, String>,
    minecraft_version_constraints: Vec<String>,
    warning: Option<String>,
}

pub fn scan_mods(folder: &Path) -> Result<Vec<ModRecord>> {
    if !folder.exists() {
        bail!("mods folder does not exist: {}", folder.display());
    }
    if !folder.is_dir() {
        bail!("mods path is not a directory: {}", folder.display());
    }

    let mut paths = std::fs::read_dir(folder)
        .with_context(|| format!("failed to read mods folder {}", folder.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
        })
        .collect::<Vec<_>>();

    paths.sort_by_cached_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });

    let mut records = Vec::with_capacity(paths.len());
    for path in paths {
        records.push(scan_one(&path)?);
    }

    let mut ids = BTreeMap::<String, Vec<String>>::new();
    for record in &records {
        if let Some(mod_id) = &record.mod_id {
            ids.entry(mod_id.clone())
                .or_default()
                .push(record.filename.clone());
        }
    }
    let duplicates = ids
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(id, files)| format!("{id}: {}", files.join(", ")))
        .collect::<Vec<_>>();
    if !duplicates.is_empty() {
        bail!("duplicate Mod ID detected: {}", duplicates.join("; "));
    }

    Ok(records)
}

fn scan_one(path: &Path) -> Result<ModRecord> {
    let file_size = std::fs::metadata(path)
        .with_context(|| format!("failed to stat JAR {}", path.display()))?
        .len();
    let sha512 = sha512_file(path)?;
    let metadata = inspect_jar(path)?;
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .with_context(|| format!("JAR has no filename: {}", path.display()))?;

    Ok(ModRecord {
        filename,
        file_size,
        sha512,
        loader_family: metadata.loader_family,
        mod_id: metadata.mod_id,
        name: metadata.name,
        version: metadata.version,
        dependency_metadata: metadata.dependency_metadata,
        minecraft_version_constraints: metadata.minecraft_version_constraints,
        source: "local".to_string(),
        source_path: path.to_path_buf(),
        selected: true,
        metadata_warning: metadata.warning,
    })
}

pub fn sha512_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open file for SHA-512: {}", path.display()))?;
    let mut hasher = Sha512::new();
    let mut buffer = [0_u8; 1024 * 128];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to hash file: {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn inspect_jar(path: &Path) -> Result<JarMetadata> {
    let file = File::open(path)
        .with_context(|| format!("failed to open JAR metadata: {}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to open JAR as ZIP: {}", path.display()))?;

    if let Some(value) = read_json_entry(&mut archive, "fabric.mod.json")? {
        return Ok(parse_fabric_metadata(&value));
    }

    if archive_contains(&mut archive, "META-INF/neoforge.mods.toml") {
        return Ok(JarMetadata {
            loader_family: "neoforge".to_string(),
            warning: Some(
                "NeoForge metadata detected; detailed parsing is planned for a later phase"
                    .to_string(),
            ),
            ..Default::default()
        });
    }
    if archive_contains(&mut archive, "META-INF/mods.toml") {
        return Ok(JarMetadata {
            loader_family: "forge".to_string(),
            warning: Some(
                "Forge metadata detected; detailed parsing is planned for a later phase"
                    .to_string(),
            ),
            ..Default::default()
        });
    }
    if let Some(value) = read_json_entry(&mut archive, "quilt.mod.json")? {
        return Ok(JarMetadata {
            loader_family: "quilt".to_string(),
            warning: Some(format!(
                "Quilt metadata detected; detailed parsing is planned for a later phase ({})",
                value.get("quilt_loader").is_some()
            )),
            ..Default::default()
        });
    }

    Ok(JarMetadata {
        loader_family: "unknown".to_string(),
        warning: Some("No supported loader metadata was found".to_string()),
        ..Default::default()
    })
}

fn archive_contains<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> bool {
    archive.by_name(name).is_ok()
}

fn read_json_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Option<Value>> {
    let Ok(entry) = archive.by_name(name) else {
        return Ok(None);
    };
    if entry.size() > MAX_METADATA_BYTES {
        bail!("JAR metadata entry is too large: {name}");
    }

    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read JAR metadata entry {name}"))?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        bail!("JAR metadata entry is too large: {name}");
    }
    let value = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid JSON in JAR metadata entry {name}"))?;
    Ok(Some(value))
}

fn parse_fabric_metadata(value: &Value) -> JarMetadata {
    let mut metadata = JarMetadata {
        loader_family: "fabric".to_string(),
        mod_id: value
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        name: value
            .get("name")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        version: value
            .get("version")
            .map(value_as_string)
            .filter(|value| !value.is_empty()),
        ..Default::default()
    };

    if let Some(dependencies) = value.get("depends").and_then(Value::as_object) {
        for (key, dependency) in dependencies {
            let rendered = value_as_string(dependency);
            if key == "minecraft" {
                metadata
                    .minecraft_version_constraints
                    .push(rendered.clone());
            }
            metadata.dependency_metadata.insert(key.clone(), rendered);
        }
    }

    if metadata.mod_id.is_none() || metadata.version.is_none() {
        metadata.warning = Some("Fabric metadata is missing id or version".to_string());
    }
    metadata
}

fn value_as_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .map(value_as_string)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Null => "null".to_string(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use uuid::Uuid;
    use zip::write::SimpleFileOptions;

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("minebench-modscan-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn make_fabric_jar(folder: &Path, filename: &str, id: &str) -> PathBuf {
        let path = folder.join(filename);
        let file = File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("fabric.mod.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(
                format!(
                    r#"{{"schemaVersion":1,"id":"{id}","name":"Test {id}","version":"1.0.0","depends":{{"minecraft":"~1.21.1"}}}}"#
                )
                .as_bytes(),
            )
            .unwrap();
        writer.finish().unwrap();
        path
    }

    #[test]
    fn scans_fabric_metadata_and_hashes_jar() {
        let folder = temp_dir();
        let path = make_fabric_jar(&folder, "example mod.jar", "example");
        let records = scan_mods(&folder).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].filename, "example mod.jar");
        assert_eq!(records[0].loader_family, "fabric");
        assert_eq!(records[0].mod_id.as_deref(), Some("example"));
        assert_eq!(records[0].version.as_deref(), Some("1.0.0"));
        assert_eq!(records[0].sha512.len(), 128);
        assert_eq!(records[0].source_path, path);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn duplicate_mod_ids_are_blocked() {
        let folder = temp_dir();
        make_fabric_jar(&folder, "one.jar", "duplicate");
        make_fabric_jar(&folder, "two.jar", "duplicate");
        let error = scan_mods(&folder).unwrap_err().to_string();
        assert!(error.contains("duplicate Mod ID"));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn unknown_zip_is_kept_as_unknown() {
        let folder = temp_dir();
        let path = folder.join("unknown.jar");
        let file = File::create(&path).unwrap();
        zip::ZipWriter::new(file).finish().unwrap();
        let records = scan_mods(&folder).unwrap();
        assert_eq!(records[0].loader_family, "unknown");
        assert!(records[0].metadata_warning.is_some());
        let _ = std::fs::remove_dir_all(folder);
    }
}
