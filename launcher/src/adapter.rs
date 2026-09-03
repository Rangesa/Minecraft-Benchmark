use crate::detection::InstanceDetection;
use crate::modscan::sha512_file;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterManifest {
    pub schema: u32,
    pub manifest_version: String,
    pub generated_at: String,
    pub adapters: Vec<AdapterArtifact>,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterArtifact {
    pub id: String,
    pub version: String,
    pub kind: String,
    pub loader_family: String,
    pub minecraft_versions: Vec<String>,
    #[serde(default)]
    pub loader_versions: Vec<String>,
    pub file_name: String,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub sha512: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AdapterResolution {
    Adapter(AdapterArtifact),
    ExternalFallback { reason: String },
}

impl AdapterManifest {
    pub fn resolve(
        &self,
        detection: &InstanceDetection,
        allow_dev_mock: bool,
    ) -> AdapterResolution {
        let version = detection.minecraft_version.as_deref();
        let loader = detection.loader.family.as_str();
        let mut candidates = self
            .adapters
            .iter()
            .filter(|artifact| allow_dev_mock || artifact.kind != "mock")
            .filter(|artifact| matches_value(&artifact.loader_family, loader))
            .filter(|artifact| {
                version.is_some_and(|version| matches_any(&artifact.minecraft_versions, version))
            })
            .filter(|artifact| {
                artifact.loader_versions.is_empty()
                    || detection.loader.version.is_none()
                    || detection
                        .loader
                        .version
                        .as_deref()
                        .is_some_and(|version| matches_any(&artifact.loader_versions, version))
            })
            .cloned()
            .collect::<Vec<_>>();

        candidates.sort_by_key(|artifact| {
            Reverse((
                version.is_some_and(|version| {
                    artifact
                        .minecraft_versions
                        .iter()
                        .any(|value| value == version)
                }),
                artifact.loader_family.eq_ignore_ascii_case(loader),
                detection
                    .loader
                    .version
                    .as_deref()
                    .is_some_and(|loader_version| {
                        artifact
                            .loader_versions
                            .iter()
                            .any(|value| value == loader_version)
                    }),
            ))
        });

        if let Some(artifact) = candidates.into_iter().next() {
            return AdapterResolution::Adapter(artifact);
        }

        let reason = match (version, loader) {
            (None, _) => {
                "Minecraft Version is unknown; using external frametime fallback".to_string()
            }
            (_, "unknown" | "ambiguous") => {
                "Loader is unknown or ambiguous; using external frametime fallback".to_string()
            }
            (Some(version), loader) => format!(
                "no Adapter matches Minecraft {version} / {loader}; using external frametime fallback"
            ),
        };
        AdapterResolution::ExternalFallback { reason }
    }
}

pub fn development_manifest() -> AdapterManifest {
    AdapterManifest {
        schema: 1,
        manifest_version: "BUILTIN-MANIFEST-v2".to_string(),
        generated_at: crate::time::now_iso8601(),
        adapters: vec![
            AdapterArtifact {
                id: "mcbenchmark-fabric".to_string(),
                version: "1.0".to_string(),
                kind: "minecraft".to_string(),
                loader_family: "fabric".to_string(),
                minecraft_versions: vec!["1.21.1".to_string()],
                loader_versions: vec!["0.19.5".to_string()],
                file_name: "mcbenchmark-fabric-1.21.1.jar".to_string(),
                download_url: None,
                sha512: None,
            },
            AdapterArtifact {
                id: "mock-adapter".to_string(),
                version: "0.1.0".to_string(),
                kind: "mock".to_string(),
                loader_family: "*".to_string(),
                minecraft_versions: vec!["*".to_string()],
                loader_versions: Vec::new(),
                file_name: if cfg!(windows) {
                    "mock-benchmark.exe".to_string()
                } else {
                    "mock-benchmark".to_string()
                },
                download_url: None,
                sha512: None,
            },
        ],
        signature: None,
    }
}

pub fn load_manifest(path: Option<&Path>) -> Result<AdapterManifest> {
    if let Some(path) = path {
        return read_manifest(path);
    }
    if let Some(path) = std::env::var_os("MINEBENCH_ADAPTER_MANIFEST") {
        return read_manifest(&PathBuf::from(path));
    }
    if let Ok(url) = std::env::var("MINEBENCH_ADAPTER_MANIFEST_URL") {
        return fetch_manifest(&url);
    }
    Ok(development_manifest())
}

pub fn ensure_cached(artifact: &AdapterArtifact, cache_dir: &Path) -> Result<Option<PathBuf>> {
    if artifact.kind == "mock" && artifact.download_url.is_none() {
        return Ok(None);
    }
    let file_name = safe_file_name(&artifact.file_name)?;
    let adapters_dir = cache_dir.join("adapters");
    fs::create_dir_all(&adapters_dir)
        .with_context(|| format!("failed to create adapter cache {}", adapters_dir.display()))?;
    let destination = adapters_dir.join(file_name);

    if destination.is_file() {
        verify_artifact_hash(&destination, artifact)?;
        return Ok(Some(destination));
    }

    let url = artifact
        .download_url
        .as_deref()
        .context("Adapter has no download URL and is not a built-in mock")?;
    if !url.starts_with("https://") {
        bail!("Adapter download URL must use HTTPS: {url}");
    }
    if artifact.sha512.as_deref().is_none() {
        bail!("Adapter download is missing its required SHA-512 allowlist entry");
    }

    let temporary = destination.with_extension("download");
    let mut response = ureq::get(url)
        .call()
        .with_context(|| format!("failed to download Adapter from {url}"))?;
    const MAX_ADAPTER_BYTES: u64 = 512 * 1024 * 1024;
    if response
        .body()
        .content_length()
        .is_some_and(|length| length > MAX_ADAPTER_BYTES)
    {
        bail!("Adapter download exceeds the 512 MiB safety limit");
    }
    let mut file = File::create(&temporary)
        .with_context(|| format!("failed to create adapter download {}", temporary.display()))?;
    let mut limited_reader = response.body_mut().as_reader().take(MAX_ADAPTER_BYTES + 1);
    let written = std::io::copy(&mut limited_reader, &mut file)
        .with_context(|| format!("failed to write adapter download {}", temporary.display()))?;
    if written > MAX_ADAPTER_BYTES {
        let _ = fs::remove_file(&temporary);
        bail!("Adapter download exceeds the 512 MiB safety limit");
    }
    file.flush().ok();
    verify_artifact_hash(&temporary, artifact).with_context(|| {
        format!(
            "downloaded Adapter failed hash verification: {}",
            temporary.display()
        )
    })?;

    if destination.exists() {
        fs::remove_file(&destination).with_context(|| {
            format!("failed to replace cached Adapter {}", destination.display())
        })?;
    }
    fs::rename(&temporary, &destination).with_context(|| {
        format!(
            "failed to finalize cached Adapter {}",
            destination.display()
        )
    })?;
    Ok(Some(destination))
}

fn read_manifest(path: &Path) -> Result<AdapterManifest> {
    serde_json::from_reader(
        File::open(path)
            .with_context(|| format!("failed to open Adapter manifest {}", path.display()))?,
    )
    .with_context(|| format!("invalid Adapter manifest {}", path.display()))
}

fn fetch_manifest(url: &str) -> Result<AdapterManifest> {
    if !url.starts_with("https://") {
        bail!("Adapter manifest URL must use HTTPS: {url}");
    }
    let mut response = ureq::get(url)
        .call()
        .with_context(|| format!("failed to fetch Adapter manifest {url}"))?;
    let bytes = response
        .body_mut()
        .read_to_vec()
        .context("failed to read Adapter manifest response")?;
    serde_json::from_slice(&bytes).context("invalid Adapter manifest response")
}

fn verify_artifact_hash(path: &Path, artifact: &AdapterArtifact) -> Result<()> {
    let expected = artifact
        .sha512
        .as_deref()
        .context("Adapter artifact has no SHA-512")?;
    let actual = sha512_file(path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        bail!(
            "Adapter SHA-512 mismatch for {}: expected {}, got {}",
            path.display(),
            expected,
            actual
        );
    }
    Ok(())
}

fn safe_file_name(file_name: &str) -> Result<&str> {
    let path = Path::new(file_name);
    if file_name.is_empty() || path.file_name().and_then(|name| name.to_str()) != Some(file_name) {
        bail!("unsafe Adapter file name: {file_name}");
    }
    Ok(file_name)
}

fn matches_any(values: &[String], candidate: &str) -> bool {
    values.iter().any(|value| matches_value(value, candidate))
}

fn matches_value(value: &str, candidate: &str) -> bool {
    value == "*" || value.eq_ignore_ascii_case(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::{DetectedLoader, InstanceDetection};

    fn detection(version: Option<&str>, loader: &str) -> InstanceDetection {
        InstanceDetection {
            input_path: PathBuf::from("input"),
            instance_dir: PathBuf::from("instance"),
            mods_dir: None,
            minecraft_version: version.map(ToOwned::to_owned),
            loader: DetectedLoader {
                family: loader.to_string(),
                version: Some("1".to_string()),
                confidence: "test".to_string(),
                evidence: Vec::new(),
            },
            mods: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn resolves_exact_adapter_before_wildcard() {
        let manifest = AdapterManifest {
            schema: 1,
            manifest_version: "test".to_string(),
            generated_at: "now".to_string(),
            adapters: vec![
                AdapterArtifact {
                    id: "wildcard".to_string(),
                    version: "1".to_string(),
                    kind: "minecraft".to_string(),
                    loader_family: "fabric".to_string(),
                    minecraft_versions: vec!["*".to_string()],
                    loader_versions: Vec::new(),
                    file_name: "wildcard.jar".to_string(),
                    download_url: None,
                    sha512: None,
                },
                AdapterArtifact {
                    id: "exact".to_string(),
                    version: "1".to_string(),
                    kind: "minecraft".to_string(),
                    loader_family: "fabric".to_string(),
                    minecraft_versions: vec!["1.21.1".to_string()],
                    loader_versions: vec!["1".to_string()],
                    file_name: "exact.jar".to_string(),
                    download_url: None,
                    sha512: None,
                },
            ],
            signature: None,
        };
        let resolution = manifest.resolve(&detection(Some("1.21.1"), "fabric"), false);
        let AdapterResolution::Adapter(artifact) = resolution else {
            panic!("expected Adapter");
        };
        assert_eq!(artifact.id, "exact");
    }

    #[test]
    fn unknown_environment_falls_back() {
        let resolution = development_manifest().resolve(&detection(None, "unknown"), false);
        assert!(matches!(
            resolution,
            AdapterResolution::ExternalFallback { .. }
        ));
    }

    #[test]
    fn unsafe_file_names_are_rejected() {
        assert!(safe_file_name("..\\evil.jar").is_err());
        assert!(safe_file_name("adapter.jar").is_ok());
    }
}
