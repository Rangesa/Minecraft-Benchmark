use crate::modscan::ModRecord;
use crate::protocol::{
    BenchmarkConfig, CaptureConfig, Challenge, ManifestMod, ManifestPaths, RunManifest,
};
use anyhow::Result;
use chrono::{Duration, Utc};
use uuid::Uuid;

pub const MANIFEST_SCHEMA: u32 = 1;
pub const PROFILE_ID: &str = "MB-STANDARD-v1";
pub const SUITE_VERSION: &str = "SUITE-v1";

pub fn new_run_id() -> String {
    format!("MB-{}", Uuid::new_v4().simple())
}

pub fn build_manifest(
    run_id: String,
    minecraft_version: &str,
    loader: &str,
    selected_mods: &[ModRecord],
) -> RunManifest {
    build_manifest_with_config(
        run_id,
        minecraft_version,
        loader,
        selected_mods,
        BenchmarkConfig::default(),
    )
}

pub fn build_manifest_with_config(
    run_id: String,
    minecraft_version: &str,
    loader: &str,
    selected_mods: &[ModRecord],
    benchmark: BenchmarkConfig,
) -> RunManifest {
    RunManifest {
        schema: MANIFEST_SCHEMA,
        run_id,
        autorun: true,
        mode: "fabric".to_string(),
        minecraft_version: minecraft_version.to_string(),
        loader: loader.to_string(),
        profile: PROFILE_ID.to_string(),
        suite_version: SUITE_VERSION.to_string(),
        challenge: Challenge {
            nonce: format!("local-{}", Uuid::new_v4().simple()),
            expires_at: (Utc::now() + Duration::minutes(5)).to_rfc3339(),
        },
        paths: ManifestPaths {
            output: "minebench/result".to_string(),
            status: "minebench/status.json".to_string(),
        },
        selected_mods: selected_mods.iter().map(manifest_mod).collect(),
        benchmark,
        capture: CaptureConfig::default(),
        hardware: Default::default(),
    }
}

fn manifest_mod(record: &ModRecord) -> ManifestMod {
    ManifestMod {
        filename: record.filename.clone(),
        sha512: record.sha512.clone(),
        loader_family: record.loader_family.clone(),
        mod_id: record.mod_id.clone(),
        name: record.name.clone(),
        version: record.version.clone(),
    }
}

pub fn pretty_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(value)?)
}
