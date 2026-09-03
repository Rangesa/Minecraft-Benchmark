use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Challenge {
    pub nonce: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestPaths {
    pub output: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestMod {
    pub filename: String,
    pub sha512: String,
    pub loader_family: String,
    pub mod_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BenchmarkConfig {
    #[serde(default = "default_warmup_ticks")]
    pub warmup_ticks: u32,
    #[serde(default = "default_graphics_frames")]
    pub graphics_frames: u32,
    #[serde(default = "default_simulation_ticks")]
    pub simulation_ticks: u32,
    #[serde(default = "default_chunk_samples")]
    pub chunk_samples: u32,
    #[serde(default = "default_entity_count")]
    pub entity_count: u32,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            warmup_ticks: default_warmup_ticks(),
            graphics_frames: default_graphics_frames(),
            simulation_ticks: default_simulation_ticks(),
            chunk_samples: default_chunk_samples(),
            entity_count: default_entity_count(),
        }
    }
}

const fn default_warmup_ticks() -> u32 {
    120
}

const fn default_graphics_frames() -> u32 {
    900
}

const fn default_simulation_ticks() -> u32 {
    180
}

const fn default_chunk_samples() -> u32 {
    32
}

const fn default_entity_count() -> u32 {
    96
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureConfig {
    #[serde(default = "default_capture_backend")]
    pub backend: String,
    #[serde(default)]
    pub started: bool,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub executable: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            backend: default_capture_backend(),
            started: false,
            output: None,
            executable: None,
            version: None,
            error: None,
        }
    }
}

fn default_capture_backend() -> String {
    "pending".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunManifest {
    pub schema: u32,
    pub run_id: String,
    pub autorun: bool,
    pub mode: String,
    pub minecraft_version: String,
    pub loader: String,
    pub profile: String,
    pub suite_version: String,
    pub challenge: Challenge,
    pub paths: ManifestPaths,
    pub selected_mods: Vec<ManifestMod>,
    #[serde(default)]
    pub benchmark: BenchmarkConfig,
    #[serde(default)]
    pub capture: CaptureConfig,
    #[serde(default)]
    pub hardware: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusFile {
    pub schema: u32,
    pub run_id: String,
    pub state: String,
    pub phase: String,
    pub test_index: u32,
    pub test_total: u32,
    pub progress: f32,
    pub message: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CategoryScores {
    pub graphics: f64,
    pub simulation: f64,
    pub world_generation: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricSummary {
    pub average_fps: f64,
    pub one_percent_low_fps: f64,
    pub median_mspt: f64,
    pub p99_mspt: f64,
    pub chunks_per_second: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResultSummary {
    pub schema: u32,
    pub run_id: String,
    pub status: String,
    pub verification: String,
    pub score_version: String,
    pub profile: String,
    pub suite_version: String,
    pub scores: CategoryScores,
    pub overall_score: f64,
    pub metrics: MetricSummary,
    pub telemetry_sha256: String,
    pub completed_at: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetrySample {
    pub schema: u32,
    pub sequence: u64,
    pub test: String,
    pub kind: String,
    pub monotonic_ns: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_time_ns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tick_time_ns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_time_ns: Option<u64>,
}

pub fn resolve_protocol_path(instance_dir: &Path, relative: &str) -> std::path::PathBuf {
    instance_dir.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR))
}
