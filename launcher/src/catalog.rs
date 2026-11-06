use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeTarget {
    pub minecraft: String,
    pub loader_version: String,
    pub fabric_api_version: String,
    pub fabric_api_sha1: String,
    pub fabric_api_size: u64,
    pub minimum_java: u32,
}

pub fn targets() -> Vec<RuntimeTarget> {
    serde_json::from_str(include_str!("../adapter-catalog.json"))
        .expect("the built-in Minecraft runtime catalog is valid JSON")
}

pub fn target(version: &str) -> Result<RuntimeTarget> {
    targets()
        .into_iter()
        .find(|entry| entry.minecraft == version)
        .with_context(|| {
            format!("no pinned Fabric Adapter/runtime is available for Minecraft {version}")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_is_exact_unique_and_has_verified_dependency_metadata() {
        let entries = targets();
        let builds: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../minecraft-versions.json")).unwrap();
        assert_eq!(entries.len(), 15);
        let mut seen = std::collections::HashSet::new();
        for entry in entries {
            let build = builds
                .iter()
                .find(|build| build["minecraft"] == entry.minecraft)
                .unwrap();
            assert_eq!(build["properties"]["loader_version"], entry.loader_version);
            assert_eq!(
                build["properties"]["fabric_version"],
                entry.fabric_api_version
            );
            assert!(seen.insert(entry.minecraft.clone()));
            assert!(!entry.minecraft.contains('*'));
            assert!(
                entry
                    .fabric_api_version
                    .ends_with(&format!("+{}", entry.minecraft))
            );
            assert_eq!(entry.fabric_api_sha1.len(), 40);
            assert!(entry.fabric_api_sha1.bytes().all(|c| c.is_ascii_hexdigit()));
            assert!(entry.fabric_api_size > 0);
            assert_eq!(
                entry.minimum_java,
                if entry.minecraft.starts_with("26.") {
                    25
                } else {
                    21
                }
            );
        }
        assert!(target("26.2").is_ok());
        assert!(target("26.3").is_err());
        assert!(target("1.21").is_err());
    }
}
