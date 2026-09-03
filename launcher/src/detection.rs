use crate::modscan::{ModRecord, scan_mods};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetectedLoader {
    pub family: String,
    pub version: Option<String>,
    pub confidence: String,
    pub evidence: Vec<String>,
}

impl DetectedLoader {
    fn unknown() -> Self {
        Self {
            family: "unknown".to_string(),
            version: None,
            confidence: "none".to_string(),
            evidence: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceDetection {
    pub input_path: PathBuf,
    pub instance_dir: PathBuf,
    pub mods_dir: Option<PathBuf>,
    pub minecraft_version: Option<String>,
    pub loader: DetectedLoader,
    pub mods: Vec<ModRecord>,
    pub warnings: Vec<String>,
}

/// Detect an instance or a direct `mods` folder without writing anything to it.
pub fn detect_instance(input: &Path) -> Result<InstanceDetection> {
    if !input.exists() {
        bail!(
            "instance or mods folder does not exist: {}",
            input.display()
        );
    }
    if !input.is_dir() {
        bail!(
            "instance or mods path is not a directory: {}",
            input.display()
        );
    }

    let input_path = input
        .canonicalize()
        .with_context(|| format!("failed to resolve input path {}", input.display()))?;
    let (instance_dir, mods_dir) = choose_instance_and_mods_dir(&input_path)?;
    let mods = match &mods_dir {
        Some(path) => scan_mods(path)?,
        None => Vec::new(),
    };

    let mut warnings = Vec::new();
    let (minecraft_version, version_evidence) = detect_minecraft_version(&instance_dir)?;
    if minecraft_version.is_none() {
        warnings.push(
            "Minecraft Version was not found in local instance metadata; external measurement remains possible".to_string(),
        );
    }

    let (loader, loader_warnings) = detect_loader(&instance_dir, &mods, version_evidence);
    warnings.extend(loader_warnings);
    if loader.family == "unknown" {
        warnings
            .push("Loader was not identified from instance metadata or mod metadata".to_string());
    }

    Ok(InstanceDetection {
        input_path,
        instance_dir,
        mods_dir,
        minecraft_version,
        loader,
        mods,
        warnings,
    })
}

fn choose_instance_and_mods_dir(input: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    let is_named_mods = input
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("mods"));
    if is_named_mods {
        let instance = input
            .parent()
            .context("mods folder has no parent instance directory")?;
        return Ok((instance.to_path_buf(), Some(input.to_path_buf())));
    }

    let child_mods = input.join("mods");
    if child_mods.is_dir() {
        return Ok((input.to_path_buf(), Some(child_mods)));
    }

    let has_direct_jars = fs::read_dir(input)
        .with_context(|| format!("failed to inspect input directory {}", input.display()))?
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
        });
    if has_direct_jars {
        return Ok((input.to_path_buf(), Some(input.to_path_buf())));
    }

    Ok((input.to_path_buf(), None))
}

fn detect_minecraft_version(instance_dir: &Path) -> Result<(Option<String>, Vec<String>)> {
    let mut evidence = Vec::new();

    if let Some((version, source)) = read_version_json(instance_dir)? {
        evidence.push(source);
        return Ok((Some(version), evidence));
    }

    if let Some((version, source)) = read_mmc_pack(instance_dir)?
        .and_then(|info| info.minecraft_version.map(|version| (version, info.source)))
    {
        evidence.push(source);
        return Ok((Some(version), evidence));
    }

    if let Some(version) = read_instance_cfg_version(instance_dir)? {
        evidence.push("instance.cfg".to_string());
        return Ok((Some(version), evidence));
    }

    Ok((None, evidence))
}

#[derive(Debug, Default)]
struct MmcPackInfo {
    minecraft_version: Option<String>,
    loader_family: Option<String>,
    loader_version: Option<String>,
    source: String,
}

fn read_mmc_pack(instance_dir: &Path) -> Result<Option<MmcPackInfo>> {
    let path = instance_dir.join("mmc-pack.json");
    if !path.is_file() {
        return Ok(None);
    }
    let value: Value = serde_json::from_reader(
        fs::File::open(&path).with_context(|| format!("failed to open {}", path.display()))?,
    )
    .with_context(|| format!("invalid JSON in {}", path.display()))?;

    let mut result = MmcPackInfo {
        source: "mmc-pack.json".to_string(),
        ..Default::default()
    };
    if let Some(components) = value.get("components").and_then(Value::as_array) {
        for component in components {
            let uid = component
                .get("uid")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let version = component
                .get("version")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if uid == "net.minecraft" {
                result.minecraft_version = version;
            } else if let Some(family) = loader_family_from_uid(uid) {
                result.loader_family = Some(family.to_string());
                result.loader_version = version;
            }
        }
    }
    Ok(Some(result))
}

fn read_version_json(instance_dir: &Path) -> Result<Option<(String, String)>> {
    for filename in ["version.json", "versions.json"] {
        let path = instance_dir.join(filename);
        if !path.is_file() {
            continue;
        }
        let value: Value = serde_json::from_reader(
            fs::File::open(&path).with_context(|| format!("failed to open {}", path.display()))?,
        )
        .with_context(|| format!("invalid JSON in {}", path.display()))?;
        if let Some(version) = value
            .get("id")
            .or_else(|| value.get("version"))
            .and_then(Value::as_str)
            .filter(|version| !version.is_empty())
        {
            return Ok(Some((version.to_string(), filename.to_string())));
        }
    }
    Ok(None)
}

fn read_instance_cfg_version(instance_dir: &Path) -> Result<Option<String>> {
    let path = instance_dir.join("instance.cfg");
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {} as UTF-8", path.display()))?;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if matches!(
            key.trim().to_ascii_lowercase().as_str(),
            "minecraft-version" | "minecraft_version" | "intendedversion" | "mcversion"
        ) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(Some(value.to_string()));
            }
        }
    }
    Ok(None)
}

fn detect_loader(
    instance_dir: &Path,
    mods: &[ModRecord],
    version_evidence: Vec<String>,
) -> (DetectedLoader, Vec<String>) {
    let mut warnings = Vec::new();
    if let Ok(Some(pack)) = read_mmc_pack(instance_dir)
        && let Some(family) = pack.loader_family
    {
        let mut evidence = version_evidence;
        evidence.push(pack.source);
        return (
            DetectedLoader {
                family,
                version: pack.loader_version,
                confidence: "high".to_string(),
                evidence,
            },
            warnings,
        );
    }

    let mut families = mods
        .iter()
        .filter(|record| record.loader_family != "unknown")
        .map(|record| record.loader_family.clone())
        .collect::<Vec<_>>();
    families.sort();
    families.dedup();
    if families.len() > 1 {
        warnings.push(format!(
            "multiple loader families detected in selected folder: {}",
            families.join(", ")
        ));
        return (
            DetectedLoader {
                family: "ambiguous".to_string(),
                version: None,
                confidence: "low".to_string(),
                evidence: families,
            },
            warnings,
        );
    }
    if let Some(family) = families.into_iter().next() {
        return (
            DetectedLoader {
                family,
                version: None,
                confidence: "medium".to_string(),
                evidence: vec!["mod metadata".to_string()],
            },
            warnings,
        );
    }

    (DetectedLoader::unknown(), warnings)
}

fn loader_family_from_uid(uid: &str) -> Option<&'static str> {
    let uid = uid.to_ascii_lowercase();
    if uid.contains("fabricmc.fabric-loader") || uid == "net.fabricmc.fabric-loader" {
        Some("fabric")
    } else if uid.contains("neoforge") {
        Some("neoforge")
    } else if uid.contains("forge") {
        Some("forge")
    } else if uid.contains("quiltmc.quilt-loader") {
        Some("quilt")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use uuid::Uuid;
    use zip::write::SimpleFileOptions;

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("minebench-detection-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn detects_mmc_instance_and_mods_folder() {
        let root = temp_dir();
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(
            root.join("mmc-pack.json"),
            r#"{"components":[{"uid":"net.minecraft","version":"1.21.1"},{"uid":"net.fabricmc.fabric-loader","version":"0.16.10"}]}"#,
        )
        .unwrap();
        let file = fs::File::create(root.join("mods/example.jar")).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("fabric.mod.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(br#"{"schemaVersion":1,"id":"example","version":"1"}"#)
            .unwrap();
        writer.finish().unwrap();

        let detection = detect_instance(&root.join("mods")).unwrap();
        assert_eq!(detection.minecraft_version.as_deref(), Some("1.21.1"));
        assert_eq!(detection.loader.family, "fabric");
        assert_eq!(detection.loader.version.as_deref(), Some("0.16.10"));
        assert_eq!(detection.mods.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_jar_folder_is_accepted() {
        let root = temp_dir();
        let file = fs::File::create(root.join("example.jar")).unwrap();
        zip::ZipWriter::new(file).finish().unwrap();
        let detection = detect_instance(&root).unwrap();
        assert_eq!(detection.mods.len(), 1);
        assert_eq!(detection.instance_dir, root.canonicalize().unwrap());
        let _ = fs::remove_dir_all(root);
    }
}
