use anyhow::{Context, Result, bail};
use minebench::instance::write_json_atomic;
use minebench::protocol::{
    ResultSummary, RunManifest, StatusFile, TelemetrySample, resolve_protocol_path,
};
use minebench::scoring::{calculate, sha256_bytes};
use minebench::time::now_iso8601;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const TESTS: [(&str, &str); 3] = [
    ("graphics_static", "Graphics Static"),
    ("simulation", "Simulation"),
    ("chunk_generation", "Chunk Generation"),
];

pub fn run_from<I>(arguments: I) -> Result<()>
where
    I: IntoIterator<Item = String>,
{
    let args = Args::parse(arguments)?;
    if args.help {
        println!("mock-benchmark --manifest <run-manifest.json> --run-dir <run-directory>");
        return Ok(());
    }
    let manifest_path = args.manifest.context("--manifest is required")?;
    let run_dir = args.run_dir.context("--run-dir is required")?;
    let manifest: RunManifest = serde_json::from_reader(
        fs::File::open(&manifest_path)
            .with_context(|| format!("failed to open manifest {}", manifest_path.display()))?,
    )
    .with_context(|| format!("invalid manifest {}", manifest_path.display()))?;
    if !manifest.autorun {
        bail!("mock Adapter requires autorun=true");
    }

    let instance_dir = run_dir.join("instance");
    let status_path = resolve_protocol_path(&instance_dir, &manifest.paths.status);
    let output_dir = resolve_protocol_path(&instance_dir, &manifest.paths.output);
    let telemetry_path = output_dir.join("telemetry.jsonl");
    let summary_path = output_dir.join("summary.json");
    let marker_path = output_dir.join("complete.marker");
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create output directory {}", output_dir.display()))?;

    let started = Instant::now();
    let mut telemetry = Vec::new();
    let mut sequence = 0_u64;
    set_status(
        &status_path,
        &manifest,
        "BOOT",
        "BOOT",
        0,
        0.0,
        "Starting development Adapter",
    )?;
    pause(120);
    set_status(
        &status_path,
        &manifest,
        "ADAPTER_READY",
        "ADAPTER_READY",
        0,
        0.0,
        "Mock Adapter ready; starting autorun",
    )?;
    pause(120);
    set_status(
        &status_path,
        &manifest,
        "WORLD_PREPARE",
        "WORLD_PREPARE",
        0,
        0.0,
        "Preparing deterministic mock world",
    )?;
    pause(180);
    set_status(
        &status_path,
        &manifest,
        "WARMUP",
        "WARMUP",
        0,
        0.0,
        "Warming up JVM/renderer simulation; excluded from score",
    )?;
    pause(300);

    for (index, (test_id, display_name)) in TESTS.iter().enumerate() {
        if index > 0 {
            set_status(
                &status_path,
                &manifest,
                "COOLDOWN",
                "COOLDOWN",
                index as u32,
                0.0,
                "Cooling down between measured tests",
            )?;
            pause(150);
        }

        let state = format!("TEST_{}", index + 1);
        let start_marker = TelemetrySample {
            schema: 1,
            sequence,
            test: (*test_id).to_string(),
            kind: "test_start".to_string(),
            monotonic_ns: started.elapsed().as_nanos() as u64,
            frame_time_ns: None,
            tick_time_ns: None,
            chunk_time_ns: None,
        };
        sequence += 1;
        telemetry.push(start_marker);
        let sample_count = 40_u64;
        for sample_index in 0..sample_count {
            pause(18);
            let sample = make_sample(test_id, sequence, started.elapsed(), sample_index);
            sequence += 1;
            telemetry.push(sample);
            set_status(
                &status_path,
                &manifest,
                &state,
                display_name,
                (index + 1) as u32,
                (sample_index + 1) as f32 / sample_count as f32,
                &format!("Measuring {display_name}"),
            )?;
        }
        telemetry.push(TelemetrySample {
            schema: 1,
            sequence,
            test: (*test_id).to_string(),
            kind: "test_end".to_string(),
            monotonic_ns: started.elapsed().as_nanos() as u64,
            frame_time_ns: None,
            tick_time_ns: None,
            chunk_time_ns: None,
        });
        sequence += 1;
    }

    set_status(
        &status_path,
        &manifest,
        "FINALIZE",
        "FINALIZE",
        TESTS.len() as u32,
        1.0,
        "Flushing buffered raw telemetry",
    )?;
    let telemetry_bytes = telemetry
        .iter()
        .map(serde_json::to_vec)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .flat_map(|mut line| {
            line.push(b'\n');
            line
        })
        .collect::<Vec<_>>();
    fs::write(&telemetry_path, &telemetry_bytes)
        .with_context(|| format!("failed to write telemetry {}", telemetry_path.display()))?;
    let (metrics, scores, overall_score) = calculate(&telemetry)?;

    set_status(
        &status_path,
        &manifest,
        "WRITE_RESULT",
        "WRITE_RESULT",
        TESTS.len() as u32,
        1.0,
        "Writing result summary",
    )?;
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "adapter_id".to_string(),
        serde_json::Value::String("mock-adapter".to_string()),
    );
    metadata.insert(
        "measurement_mode".to_string(),
        serde_json::Value::String("synthetic".to_string()),
    );
    metadata.insert(
        "selected_mod_count".to_string(),
        serde_json::Value::Number((manifest.selected_mods.len() as u64).into()),
    );
    let summary = ResultSummary {
        schema: 1,
        run_id: manifest.run_id.clone(),
        status: "COMPLETE".to_string(),
        verification: "UNVERIFIED".to_string(),
        score_version: "MOCK-SCORE-v1".to_string(),
        profile: manifest.profile.clone(),
        suite_version: manifest.suite_version.clone(),
        scores,
        overall_score,
        metrics,
        telemetry_sha256: sha256_bytes(&telemetry_bytes),
        completed_at: now_iso8601(),
        metadata,
    };
    write_json_atomic(&summary_path, &summary)?;
    fs::write(&marker_path, format!("{}\n", manifest.run_id)).with_context(|| {
        format!(
            "failed to write completion marker {}",
            marker_path.display()
        )
    })?;
    set_status(
        &status_path,
        &manifest,
        "COMPLETE",
        "COMPLETE",
        TESTS.len() as u32,
        1.0,
        "Benchmark complete",
    )?;
    println!("mock benchmark complete: {:.0}", overall_score);
    pause(60);
    set_status(
        &status_path,
        &manifest,
        "SHUTDOWN",
        "SHUTDOWN",
        TESTS.len() as u32,
        1.0,
        "Requesting clean child shutdown",
    )?;
    Ok(())
}

fn make_sample(test_id: &str, sequence: u64, elapsed: Duration, index: u64) -> TelemetrySample {
    let wobble = index % 7;
    let (frame_time_ns, tick_time_ns, chunk_time_ns) = match test_id {
        "graphics_static" => (Some(6_800_000 + wobble * 90_000), None, None),
        "simulation" => (None, Some(11_800_000 + wobble * 120_000), None),
        "chunk_generation" => (None, None, Some(32_000_000 + wobble * 700_000)),
        _ => (None, None, None),
    };
    TelemetrySample {
        schema: 1,
        sequence,
        test: test_id.to_string(),
        kind: "sample".to_string(),
        monotonic_ns: elapsed.as_nanos() as u64,
        frame_time_ns,
        tick_time_ns,
        chunk_time_ns,
    }
}

fn set_status(
    path: &Path,
    manifest: &RunManifest,
    state: &str,
    phase: &str,
    test_index: u32,
    progress: f32,
    message: &str,
) -> Result<()> {
    let status = StatusFile {
        schema: 1,
        run_id: manifest.run_id.clone(),
        state: state.to_string(),
        phase: phase.to_string(),
        test_index,
        test_total: TESTS.len() as u32,
        progress,
        message: message.to_string(),
        updated_at: now_iso8601(),
    };
    write_json_atomic(path, &status)
}

fn pause(milliseconds: u64) {
    let fast = std::env::var_os("MINEBENCH_MOCK_FAST").is_some();
    thread::sleep(Duration::from_millis(if fast {
        (milliseconds / 20).max(1)
    } else {
        milliseconds
    }));
}

#[derive(Debug, Default)]
struct Args {
    manifest: Option<PathBuf>,
    run_dir: Option<PathBuf>,
    help: bool,
}

impl Args {
    fn parse<I>(arguments: I) -> Result<Self>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = arguments.into_iter();
        let mut parsed = Self::default();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--manifest" => parsed.manifest = args.next().map(PathBuf::from),
                "--run-dir" => parsed.run_dir = args.next().map(PathBuf::from),
                "--help" | "-h" => parsed.help = true,
                unknown => bail!("unknown argument: {unknown}"),
            }
        }
        Ok(parsed)
    }
}

fn main() -> Result<()> {
    run_from(std::env::args().skip(1))
}
