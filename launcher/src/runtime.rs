use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

const MOJANG_VERSION_MANIFEST: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const FABRIC_PROFILE_TEMPLATE: &str =
    "https://meta.fabricmc.net/v2/versions/loader/{version}/{loader}/profile/json";
const FABRIC_MAVEN: &str = "https://maven.fabricmc.net/";
const DEFAULT_FABRIC_LOADER: &str = "0.19.5";
const FABRIC_API_VERSION: &str = "0.116.17+1.21.1";
const FABRIC_API_SHA1: &str = "810b2b0195371a012906241d8b85a32a1d6de53c";
const FABRIC_API_SIZE: u64 = 2_452_735;
const MAX_DOWNLOAD_BYTES: u64 = 1_024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct FabricRuntimeRequest {
    pub data_dir: PathBuf,
    pub instance_dir: PathBuf,
    pub minecraft_version: String,
    pub loader_version: Option<String>,
    pub username: String,
    pub heap_mb: u32,
}

#[derive(Debug, Clone)]
pub struct RuntimeSpec {
    pub java_path: PathBuf,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub loader_version: String,
    pub asset_index_name: String,
    pub library_count: usize,
    pub client_jar: PathBuf,
}

pub fn prepare_fabric_runtime(request: &FabricRuntimeRequest) -> Result<RuntimeSpec> {
    if request.minecraft_version != "1.21.1" {
        bail!(
            "the integrated Fabric Adapter currently supports Minecraft 1.21.1; got {}",
            request.minecraft_version
        );
    }
    if request.username.trim().is_empty() {
        bail!("Minecraft username cannot be empty");
    }

    let cache = request.data_dir.join("cache");
    let minecraft_cache = cache.join("minecraft").join(&request.minecraft_version);
    let libraries_dir = cache.join("libraries");
    let natives_dir = cache.join("natives").join(format!(
        "{}-{}",
        request.minecraft_version, DEFAULT_FABRIC_LOADER
    ));
    fs::create_dir_all(&minecraft_cache).with_context(|| {
        format!(
            "failed to create runtime cache {}",
            minecraft_cache.display()
        )
    })?;
    fs::create_dir_all(&libraries_dir)
        .with_context(|| format!("failed to create library cache {}", libraries_dir.display()))?;
    fs::create_dir_all(&natives_dir)
        .with_context(|| format!("failed to create native cache {}", natives_dir.display()))?;

    let version_json = fetch_minecraft_version(&request.minecraft_version, &cache)?;
    let client_download = version_json
        .pointer("/downloads/client")
        .context("Mojang version JSON has no client download")?;
    let client_jar = ensure_download_object(
        client_download,
        &minecraft_cache.join("client.jar"),
        "Mojang client JAR",
    )?;

    let mut classpath = Vec::new();
    let mut library_count = 0;
    if let Some(libraries) = version_json.get("libraries").and_then(Value::as_array) {
        for library in libraries {
            if !library_allowed(library) {
                continue;
            }
            if let Some(path) = ensure_library(library, &libraries_dir, "Mojang library")? {
                classpath.push(path);
                library_count += 1;
            }
            if let Some(native_path) = ensure_windows_native(library, &libraries_dir)? {
                extract_native_jar(&native_path, &natives_dir, library)?;
            }
        }
    }

    let loader_version = choose_loader_version(request.loader_version.as_deref());
    let profile_url = FABRIC_PROFILE_TEMPLATE
        .replace("{version}", &request.minecraft_version)
        .replace("{loader}", &loader_version);
    let profile = fetch_json_cached(
        &profile_url,
        &cache.join("fabric").join(format!(
            "{}-{}.json",
            request.minecraft_version, loader_version
        )),
        "Fabric loader profile",
    )?;
    let profile_libraries = profile
        .get("libraries")
        .and_then(Value::as_array)
        .context("Fabric profile has no libraries")?;
    for library in profile_libraries {
        if !library_allowed(library) {
            continue;
        }
        if let Some(path) = ensure_library(library, &libraries_dir, "Fabric library")?
            && !classpath.iter().any(|existing| existing == &path)
        {
            classpath.push(path);
            library_count += 1;
        }
    }
    classpath.push(client_jar.clone());

    let assets = ensure_assets(&version_json, &cache)?;
    let java_path = find_java()?;
    let classpath_value = classpath
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(if cfg!(windows) { ";" } else { ":" });
    let natives = natives_dir.to_string_lossy();
    let heap_mb = request.heap_mb.clamp(1024, 32_768);
    let jvm_args = vec![
        "-Xms512M".to_string(),
        format!("-Xmx{}M", heap_mb),
        "-XX:+UseG1GC".to_string(),
        "--enable-native-access=ALL-UNNAMED".to_string(),
        format!("-Djava.library.path={natives}"),
        format!("-Djna.tmpdir={natives}"),
        format!("-Dorg.lwjgl.system.SharedLibraryExtractPath={natives}"),
        format!("-Dio.netty.native.workdir={natives}"),
        "-Dminecraft.launcher.brand=MineBench".to_string(),
        "-Dminecraft.launcher.version=1.0".to_string(),
        "-cp".to_string(),
        classpath_value,
        profile
            .get("mainClass")
            .and_then(Value::as_str)
            .unwrap_or("net.fabricmc.loader.impl.launch.knot.KnotClient")
            .to_string(),
    ];
    let game_args = vec![
        "--username".to_string(),
        request.username.trim().to_string(),
        "--version".to_string(),
        request.minecraft_version.clone(),
        "--gameDir".to_string(),
        request.instance_dir.to_string_lossy().into_owned(),
        "--assetsDir".to_string(),
        assets.directory.to_string_lossy().into_owned(),
        "--assetIndex".to_string(),
        assets.index_name.clone(),
        "--uuid".to_string(),
        "00000000-0000-0000-0000-000000000001".to_string(),
        "--accessToken".to_string(),
        "0".to_string(),
        "--userType".to_string(),
        "legacy".to_string(),
        "--versionType".to_string(),
        "release".to_string(),
        "--width".to_string(),
        "1280".to_string(),
        "--height".to_string(),
        "720".to_string(),
    ];

    Ok(RuntimeSpec {
        java_path,
        jvm_args,
        game_args,
        loader_version,
        asset_index_name: assets.index_name,
        library_count,
        client_jar,
    })
}

pub fn ensure_fabric_api(data_dir: &Path) -> Result<PathBuf> {
    let destination = data_dir
        .join("cache")
        .join("mods")
        .join(format!("fabric-api-{FABRIC_API_VERSION}.jar"));
    let url = format!(
        "{FABRIC_MAVEN}net/fabricmc/fabric-api/fabric-api/{FABRIC_API_VERSION}/fabric-api-{FABRIC_API_VERSION}.jar"
    );
    ensure_download(
        &url,
        &destination,
        Some(FABRIC_API_SHA1),
        Some(FABRIC_API_SIZE),
        "Fabric API",
    )
}

pub fn inject_mod(instance_dir: &Path, source: &Path, filename: &str) -> Result<PathBuf> {
    if !source.is_file() {
        bail!("Adapter dependency does not exist: {}", source.display());
    }
    let name = Path::new(filename)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| *value == filename)
        .context("unsafe injected mod filename")?;
    let mods_dir = instance_dir.join("mods");
    fs::create_dir_all(&mods_dir).with_context(|| {
        format!(
            "failed to create temporary mods directory {}",
            mods_dir.display()
        )
    })?;
    let destination = mods_dir.join(name);
    fs::copy(source, &destination).with_context(|| {
        format!(
            "failed to inject {} into {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(destination)
}

fn choose_loader_version(requested: Option<&str>) -> String {
    if std::env::var("MINEBENCH_ALLOW_UNPINNED_LOADER").is_ok()
        && requested.is_some_and(|value| !value.trim().is_empty())
    {
        return requested.unwrap().trim().to_string();
    }
    DEFAULT_FABRIC_LOADER.to_string()
}

fn fetch_minecraft_version(version: &str, cache: &Path) -> Result<Value> {
    let destination = cache.join("versions").join(format!("{version}.json"));
    if destination.is_file() {
        return read_json(&destination, "cached Mojang version JSON");
    }
    let manifest = fetch_json(MOJANG_VERSION_MANIFEST, "Mojang version manifest")?;
    let entry = manifest
        .get("versions")
        .and_then(Value::as_array)
        .and_then(|versions| {
            versions
                .iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some(version))
        })
        .context("requested Minecraft version is not present in Mojang manifest")?;
    let url = entry
        .get("url")
        .and_then(Value::as_str)
        .context("Mojang version entry has no profile URL")?;
    let value = fetch_json(url, "Mojang version JSON")?;
    write_json(&destination, &value)?;
    Ok(value)
}

fn fetch_json_cached(url: &str, destination: &Path, label: &str) -> Result<Value> {
    if destination.is_file() {
        return read_json(destination, label);
    }
    let value = fetch_json(url, label)?;
    write_json(destination, &value)?;
    Ok(value)
}

fn fetch_json(url: &str, label: &str) -> Result<Value> {
    validate_official_url(url)?;
    let mut response = ureq::get(url)
        .call()
        .with_context(|| format!("failed to download {label} from {url}"))?;
    let bytes = response
        .body_mut()
        .read_to_vec()
        .with_context(|| format!("failed to read {label} response"))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid {label} JSON"))
}

fn read_json(path: &Path, label: &str) -> Result<Value> {
    serde_json::from_reader(
        File::open(path).with_context(|| format!("failed to open {label} {}", path.display()))?,
    )
    .with_context(|| format!("invalid {label} {}", path.display()))
}

fn ensure_download_object(value: &Value, destination: &Path, label: &str) -> Result<PathBuf> {
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .context("download entry has no URL")?;
    let sha1 = value.get("sha1").and_then(Value::as_str);
    let size = value.get("size").and_then(Value::as_u64);
    ensure_download(url, destination, sha1, size, label)
}

fn ensure_library(library: &Value, libraries_dir: &Path, label: &str) -> Result<Option<PathBuf>> {
    let name = library
        .get("name")
        .and_then(Value::as_str)
        .context("library has no Maven name")?;
    let artifact = library.pointer("/downloads/artifact");
    let (url, relative_path, sha1, size) = if let Some(artifact) = artifact {
        (
            artifact.get("url").and_then(Value::as_str),
            artifact.get("path").and_then(Value::as_str),
            artifact.get("sha1").and_then(Value::as_str),
            artifact.get("size").and_then(Value::as_u64),
        )
    } else {
        (None, None, None, None)
    };
    let relative_path = relative_path
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| maven_path(name).unwrap_or_else(|_| format!("{name}.jar")));
    let destination = libraries_dir.join(&relative_path);
    let url = url
        .map(ToOwned::to_owned)
        .or_else(|| {
            library
                .get("url")
                .and_then(Value::as_str)
                .map(|base| format!("{}{}", base, relative_path))
        })
        .or_else(|| Some(format!("{FABRIC_MAVEN}{relative_path}")))
        .context("library has no download URL")?;
    Ok(Some(ensure_download(
        &url,
        &destination,
        sha1,
        size,
        label,
    )?))
}

fn ensure_windows_native(library: &Value, libraries_dir: &Path) -> Result<Option<PathBuf>> {
    let classifier = library.pointer("/natives/windows").and_then(Value::as_str);
    let Some(classifier) = classifier else {
        return Ok(None);
    };
    let entry = library.pointer(&format!("/downloads/classifiers/{classifier}"));
    let Some(entry) = entry else {
        return Ok(None);
    };
    let url = entry
        .get("url")
        .and_then(Value::as_str)
        .context("native library has no URL")?;
    let relative_path = entry
        .get("path")
        .and_then(Value::as_str)
        .context("native library has no path")?;
    let destination = libraries_dir.join(relative_path);
    Ok(Some(ensure_download(
        url,
        &destination,
        entry.get("sha1").and_then(Value::as_str),
        entry.get("size").and_then(Value::as_u64),
        "Windows native library",
    )?))
}

fn extract_native_jar(path: &Path, destination: &Path, library: &Value) -> Result<()> {
    let file = File::open(path)
        .with_context(|| format!("failed to open native library {}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read native library {}", path.display()))?;
    let excludes = library
        .pointer("/extract/exclude")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .context("failed to inspect native entry")?;
        let name = entry.name().replace('\\', "/");
        if entry.is_dir() || excludes.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let relative = Path::new(&name);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            bail!("native archive contains unsafe path: {name}");
        }
        let output = destination.join(relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&output)
            .with_context(|| format!("failed to extract native {}", output.display()))?;
        std::io::copy(&mut entry, &mut file)
            .with_context(|| format!("failed to extract native {}", output.display()))?;
    }
    Ok(())
}

fn ensure_assets(version_json: &Value, cache: &Path) -> Result<Assets> {
    let asset_index = version_json
        .get("assetIndex")
        .context("Mojang version JSON has no assetIndex")?;
    let id = asset_index
        .get("id")
        .and_then(Value::as_str)
        .context("assetIndex has no id")?;
    let local_roots = [
        std::env::var_os("MINECRAFT_ASSETS_DIR").map(PathBuf::from),
        std::env::var_os("APPDATA").map(|root| PathBuf::from(root).join(".minecraft/assets")),
        std::env::var_os("USERPROFILE")
            .map(|root| PathBuf::from(root).join(".gradle/caches/fabric-loom/assets")),
    ];
    for root in local_roots.into_iter().flatten() {
        let exact = root.join("indexes").join(format!("{id}.json"));
        if exact.is_file() {
            return Ok(Assets {
                directory: root,
                index_name: id.to_string(),
            });
        }
        let loom = root.join("indexes").join(format!("1.21.1-{id}.json"));
        if loom.is_file() {
            return Ok(Assets {
                directory: root,
                index_name: format!("1.21.1-{id}"),
            });
        }
    }

    let assets_dir = cache.join("assets");
    let index_path = assets_dir.join("indexes").join(format!("{id}.json"));
    let index = if index_path.is_file() {
        read_json(&index_path, "cached asset index")?
    } else {
        let value = fetch_json(
            asset_index
                .get("url")
                .and_then(Value::as_str)
                .context("assetIndex has no URL")?,
            "Minecraft asset index",
        )?;
        write_json(&index_path, &value)?;
        value
    };
    let objects = index
        .get("objects")
        .and_then(Value::as_object)
        .context("asset index has no objects")?;
    for object in objects.values() {
        let hash = object
            .get("hash")
            .and_then(Value::as_str)
            .context("asset object has no hash")?;
        let object_path = assets_dir.join("objects").join(&hash[..2]).join(hash);
        if object_path.is_file() {
            continue;
        }
        let url = format!(
            "https://resources.download.minecraft.net/{}/{}",
            &hash[..2],
            hash
        );
        ensure_download(&url, &object_path, Some(hash), None, "Minecraft asset")?;
    }
    Ok(Assets {
        directory: assets_dir,
        index_name: id.to_string(),
    })
}

#[derive(Debug, Clone)]
struct Assets {
    directory: PathBuf,
    index_name: String,
}

fn library_allowed(library: &Value) -> bool {
    let Some(rules) = library.get("rules").and_then(Value::as_array) else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule) {
            allowed = rule.get("action").and_then(Value::as_str) == Some("allow");
        }
    }
    allowed
}

fn rule_matches(rule: &Value) -> bool {
    if let Some(name) = rule.pointer("/os/name").and_then(Value::as_str)
        && name != "windows"
    {
        return false;
    }
    if let Some(arch) = rule.pointer("/os/arch").and_then(Value::as_str)
        && matches!(arch, "x86" | "arm" | "arm32")
    {
        return false;
    }
    if let Some(features) = rule.get("features").and_then(Value::as_object)
        && features.values().any(|value| value.as_bool() == Some(true))
    {
        return false;
    }
    true
}

fn maven_path(name: &str) -> Result<String> {
    let parts = name.split(':').collect::<Vec<_>>();
    if parts.len() < 3 || parts.len() > 4 || parts.iter().any(|part| part.is_empty()) {
        bail!("invalid Maven coordinate: {name}");
    }
    if parts
        .iter()
        .any(|part| part.contains('/') || part.contains('\\'))
    {
        bail!("unsafe Maven coordinate: {name}");
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts.get(3).copied();
    let suffix = classifier
        .map(|value| format!("-{value}"))
        .unwrap_or_default();
    Ok(format!(
        "{group}/{artifact}/{version}/{artifact}-{version}{suffix}.jar"
    ))
}

fn ensure_download(
    url: &str,
    destination: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
    label: &str,
) -> Result<PathBuf> {
    validate_official_url(url)?;
    if destination.is_file()
        && expected_size.is_none_or(|size| fs::metadata(destination).is_ok_and(|m| m.len() == size))
        && expected_sha1.is_none_or(|hash| {
            sha1_file(destination).is_ok_and(|actual| actual.eq_ignore_ascii_case(hash))
        })
    {
        return Ok(destination.to_path_buf());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create download directory {}", parent.display()))?;
    }
    let temporary =
        destination.with_extension(format!("{}.download", uuid::Uuid::new_v4().simple()));
    let mut response = ureq::get(url)
        .call()
        .with_context(|| format!("failed to download {label} from {url}"))?;
    if response
        .body()
        .content_length()
        .is_some_and(|size| size > MAX_DOWNLOAD_BYTES)
    {
        bail!("{label} exceeds the 1 GiB safety limit");
    }
    let mut file = File::create(&temporary).with_context(|| {
        format!(
            "failed to create temporary download {}",
            temporary.display()
        )
    })?;
    let mut reader = response.body_mut().as_reader().take(MAX_DOWNLOAD_BYTES + 1);
    let written = std::io::copy(&mut reader, &mut file)
        .with_context(|| format!("failed to write {label} {}", temporary.display()))?;
    file.flush().ok();
    if written > MAX_DOWNLOAD_BYTES {
        let _ = fs::remove_file(&temporary);
        bail!("{label} exceeds the 1 GiB safety limit");
    }
    if expected_size.is_some_and(|size| size != written) {
        let _ = fs::remove_file(&temporary);
        bail!("{label} size verification failed");
    }
    if expected_sha1.is_some_and(|hash| {
        sha1_file(&temporary)
            .map(|actual| !actual.eq_ignore_ascii_case(hash))
            .unwrap_or(true)
    }) {
        let _ = fs::remove_file(&temporary);
        bail!("{label} SHA-1 verification failed");
    }
    if destination.exists() {
        fs::remove_file(destination).with_context(|| {
            format!("failed to replace cached {label} {}", destination.display())
        })?;
    }
    fs::rename(&temporary, destination)
        .with_context(|| format!("failed to finalize {label} {}", destination.display()))?;
    Ok(destination.to_path_buf())
}

fn sha1_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to hash {}", path.display()))?;
    let mut hasher = Sha1::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create JSON cache {}", parent.display()))?;
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn validate_official_url(url: &str) -> Result<()> {
    let allowed = [
        "https://piston-meta.mojang.com/",
        "https://piston-data.mojang.com/",
        "https://libraries.minecraft.net/",
        "https://meta.fabricmc.net/",
        "https://maven.fabricmc.net/",
        "https://resources.download.minecraft.net/",
    ];
    if !allowed.iter().any(|prefix| url.starts_with(prefix)) {
        bail!("runtime download URL is not an approved official endpoint: {url}");
    }
    Ok(())
}

fn find_java() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("MINECRAFT_JAVA") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return validate_java(&path);
        }
        bail!(
            "MINECRAFT_JAVA does not point to a file: {}",
            path.display()
        );
    }
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let path =
            PathBuf::from(home)
                .join("bin")
                .join(if cfg!(windows) { "java.exe" } else { "java" });
        if path.is_file()
            && let Ok(java) = validate_java(&path)
        {
            return Ok(java);
        }
    }
    let command = if cfg!(windows) { "where.exe" } else { "which" };
    let executable = if cfg!(windows) { "java.exe" } else { "java" };
    if let Ok(output) = Command::new(command).arg(executable).output()
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let path = PathBuf::from(line.trim());
            if path.is_file()
                && let Ok(java) = validate_java(&path)
            {
                return Ok(java);
            }
        }
    }
    bail!("Java 21+ was not found; set MINECRAFT_JAVA or JAVA_HOME")
}

fn validate_java(path: &Path) -> Result<PathBuf> {
    let output = Command::new(path)
        .arg("-version")
        .output()
        .with_context(|| format!("failed to inspect Java runtime {}", path.display()))?;
    if !output.status.success() {
        bail!("Java runtime failed its version check: {}", path.display());
    }
    let mut version_output = String::from_utf8_lossy(&output.stdout).into_owned();
    version_output.push_str(&String::from_utf8_lossy(&output.stderr));
    let major = parse_java_major(&version_output).with_context(|| {
        format!(
            "could not determine the Java major version from {}",
            path.display()
        )
    })?;
    if major < 21 {
        bail!(
            "Java 21 or newer is required for Minecraft 1.21.1; {} reports Java {}",
            path.display(),
            major
        );
    }
    Ok(path.to_path_buf())
}

fn parse_java_major(version_output: &str) -> Option<u32> {
    let version = version_output.split('"').nth(1).or_else(|| {
        version_output.split_whitespace().find(|token| {
            token
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
        })
    })?;
    let mut parts = version.split(['.', '_']);
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        parts.next()?.parse::<u32>().ok()
    } else {
        Some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_coordinates_are_converted_safely() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.19.5").unwrap(),
            "net/fabricmc/fabric-loader/0.19.5/fabric-loader-0.19.5.jar"
        );
        assert!(maven_path("../evil:x:1").is_err());
    }

    #[test]
    fn optional_non_windows_libraries_are_rejected() {
        let value = serde_json::json!({
            "rules": [{"action":"allow", "os":{"name":"linux"}}]
        });
        assert!(!library_allowed(&value));
    }

    #[test]
    fn windows_rule_is_allowed() {
        let value = serde_json::json!({
            "rules": [{"action":"allow", "os":{"name":"windows"}}]
        });
        assert!(library_allowed(&value));
    }

    #[test]
    fn parses_java_major_versions() {
        assert_eq!(
            parse_java_major("openjdk version \"25.0.3\" 2026-01-20"),
            Some(25)
        );
        assert_eq!(parse_java_major("java version \"1.8.0_401\""), Some(8));
        assert_eq!(parse_java_major("openjdk 21.0.8 2025-07-15"), Some(21));
    }
}
