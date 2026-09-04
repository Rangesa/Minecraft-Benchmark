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
    #[serde(default = "default_benchmark_seed")]
    pub seed: i64,
    #[serde(default = "default_chunk_probe_origin_x")]
    pub chunk_probe_origin_x: i32,
    #[serde(default = "default_chunk_probe_origin_z")]
    pub chunk_probe_origin_z: i32,
    #[serde(default = "default_chunk_probe_stride")]
    pub chunk_probe_stride: u32,
    #[serde(default = "default_render_distance")]
    pub render_distance: u32,
    #[serde(default = "default_simulation_distance")]
    pub simulation_distance: u32,
    #[serde(default = "default_max_fps")]
    pub max_fps: u32,
    #[serde(default = "default_enable_vsync")]
    pub enable_vsync: bool,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            warmup_ticks: default_warmup_ticks(),
            graphics_frames: default_graphics_frames(),
            simulation_ticks: default_simulation_ticks(),
            chunk_samples: default_chunk_samples(),
            entity_count: default_entity_count(),
            seed: default_benchmark_seed(),
            chunk_probe_origin_x: default_chunk_probe_origin_x(),
            chunk_probe_origin_z: default_chunk_probe_origin_z(),
            chunk_probe_stride: default_chunk_probe_stride(),
            render_distance: default_render_distance(),
            simulation_distance: default_simulation_distance(),
            max_fps: default_max_fps(),
            enable_vsync: default_enable_vsync(),
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

// These values are part of the provisional benchmark contract. They belong
// in the run manifest so a future Adapter can replace them without changing
// the universal Launcher protocol.
const fn default_benchmark_seed() -> i64 {
    -6_372_891_104_223_171_901
}

const fn default_chunk_probe_origin_x() -> i32 {
    1_000_000
}

const fn default_chunk_probe_origin_z() -> i32 {
    1_000_000
}

const fn default_chunk_probe_stride() -> u32 {
    8
}

const fn default_render_distance() -> u32 {
    12
}

const fn default_simulation_distance() -> u32 {
    12
}

const fn default_max_fps() -> u32 {
    260
}

const fn default_enable_vsync() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreProfile {
    pub id: String,
    pub status: String,
    pub average_fps_baseline: f64,
    pub median_mspt_baseline: f64,
    pub chunks_per_second_baseline: f64,
    pub graphics_weight: f64,
    pub simulation_weight: f64,
    pub world_generation_weight: f64,
}

impl Default for ScoreProfile {
    fn default() -> Self {
        Self {
            id: "MB-SCORE-PROVISIONAL-v0".to_string(),
            status: "PROVISIONAL".to_string(),
            average_fps_baseline: 120.0,
            median_mspt_baseline: 16.67,
            chunks_per_second_baseline: 20.0,
            graphics_weight: 30.0,
            simulation_weight: 25.0,
            world_generation_weight: 20.0,
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub score: ScoreProfile,
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
    #[serde(default = "default_score_status")]
    pub score_status: String,
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

fn default_score_status() -> String {
    "PROVISIONAL".to_string()
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
