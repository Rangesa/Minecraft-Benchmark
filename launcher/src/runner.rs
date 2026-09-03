use crate::adapter_embed;
use crate::external::{
    CaptureReport, ExternalCapturePlan, ensure_presentmon, finish_capture, start_capture,
};
use crate::hardware::collect_hardware;
use crate::instance::{
    PrepareRunRequest, PreparedRun, prepare_run, update_run_manifest, write_json_atomic,
};
use crate::protocol::{BenchmarkConfig, CaptureConfig, ResultSummary, StatusFile};
use crate::runtime::{FabricRuntimeRequest, ensure_fabric_api, inject_mod, prepare_fabric_runtime};
use crate::scoring::sha256_bytes;
use crate::time::now_iso8601;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

const MOCK_TIMEOUT: Duration = Duration::from_secs(120);
const REAL_TIMEOUT: Duration = Duration::from_secs(900);

#[derive(Debug, Clone)]
pub struct MockRunRequest {
    pub data_dir: PathBuf,
    pub minecraft_version: String,
    pub loader: String,
    pub selected_mods: Vec<crate::modscan::ModRecord>,
}

#[derive(Debug, Clone)]
pub struct RealRunRequest {
    pub data_dir: PathBuf,
    pub source_instance_dir: PathBuf,
    pub minecraft_version: String,
    pub loader: String,
    pub loader_version: Option<String>,
    pub username: String,
    pub selected_mods: Vec<crate::modscan::ModRecord>,
    pub benchmark: BenchmarkConfig,
    pub heap_mb: u32,
    pub enable_presentmon: bool,
    pub require_presentmon: bool,
}

#[derive(Debug)]
pub enum RunnerEvent {
    Prepared(Box<PreparedRun>),
    Status(StatusFile),
    Completed {
        prepared: Box<PreparedRun>,
        summary: Box<ResultSummary>,
    },
    Failed {
        run_dir: Option<PathBuf>,
        message: String,
    },
}

pub fn start_real_run(request: RealRunRequest) -> Result<Receiver<RunnerEvent>> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("minebench-real-runner".to_string())
        .spawn(move || run_real_process(request, sender))
        .context("failed to start real Minecraft runner thread")?;
    Ok(receiver)
}

pub fn start_mock_run(
    request: MockRunRequest,
    mock_binary: PathBuf,
) -> Result<Receiver<RunnerEvent>> {
    if !mock_binary.is_file() {
        bail!(
            "mock benchmark executable does not exist: {}",
            mock_binary.display()
        );
    }
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("minebench-mock-runner".to_string())
        .spawn(move || run_mock_process(request, mock_binary, sender))
        .context("failed to start launcher worker thread")?;
    Ok(receiver)
}

fn run_real_process(request: RealRunRequest, sender: Sender<RunnerEvent>) {
    let mut prepared = match prepare_run(&PrepareRunRequest {
        data_dir: request.data_dir.clone(),
        minecraft_version: request.minecraft_version.clone(),
        loader: request.loader.clone(),
        selected_mods: request.selected_mods.clone(),
        source_instance_dir: Some(request.source_instance_dir.clone()),
        benchmark: request.benchmark.clone(),
    }) {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = sender.send(RunnerEvent::Failed {
                run_dir: None,
                message: format!("failed to prepare temporary run: {error:#}"),
            });
            return;
        }
    };
    let _ = sender.send(RunnerEvent::Prepared(Box::new(prepared.clone())));

    match run_real_after_prepare(&mut prepared, &request, &sender) {
        Ok(summary) => {
            let _ = sender.send(RunnerEvent::Completed {
                prepared: Box::new(prepared),
                summary: Box::new(summary),
            });
        }
        Err(error) => {
            let message = format!("real Minecraft benchmark failed: {error:#}");
            let _ = write_launcher_status(&prepared, &sender, "FAILED", 1.0, &message);
            let _ = write_failure_artifact(&prepared, &message);
            send_failure(&sender, &prepared, message);
        }
    }
}

fn run_real_after_prepare(
    prepared: &mut PreparedRun,
    request: &RealRunRequest,
    sender: &Sender<RunnerEvent>,
) -> Result<ResultSummary> {
    if !request.loader.eq_ignore_ascii_case("fabric") {
        bail!(
            "the integrated real runtime currently supports Fabric only; detected/requested loader is {}",
            request.loader
        );
    }
    if request.minecraft_version != "1.21.1" {
        bail!(
            "the integrated Fabric Adapter currently supports Minecraft 1.21.1 only; got {}",
            request.minecraft_version
        );
    }
    if request.selected_mods.iter().any(|record| {
        !matches!(
            record.loader_family.to_ascii_lowercase().as_str(),
            "fabric" | "unknown"
        )
    }) {
        bail!(
            "the selected mod set contains a non-Fabric mod; choose a Fabric-compatible instance"
        );
    }
    write_launcher_status(
        prepared,
        sender,
        "PREPARING",
        0.0,
        "Collecting CPU/GPU/RAM/storage/OS information",
    )?;
    let hardware = collect_hardware().context("hardware collection failed")?;

    write_launcher_status(
        prepared,
        sender,
        "PREPARING",
        0.05,
        "Materializing the embedded Fabric 1.21.1 Adapter",
    )?;
    let adapter_path = adapter_embed::materialize(&request.data_dir)?;

    write_launcher_status(
        prepared,
        sender,
        "PREPARING",
        0.12,
        "Resolving Fabric API and the official Minecraft runtime",
    )?;
    let fabric_api_path = ensure_fabric_api(&request.data_dir)?;
    let runtime = prepare_fabric_runtime(&FabricRuntimeRequest {
        data_dir: request.data_dir.clone(),
        instance_dir: prepared.instance_dir.clone(),
        minecraft_version: request.minecraft_version.clone(),
        loader_version: request.loader_version.clone(),
        username: request.username.clone(),
        heap_mb: request.heap_mb,
    })?;

    inject_mod(
        &prepared.instance_dir,
        &adapter_path,
        "mcbenchmark-fabric-1.21.1.jar",
    )?;
    if !request.selected_mods.iter().any(|record| {
        record
            .mod_id
            .as_deref()
            .is_some_and(|mod_id| mod_id.eq_ignore_ascii_case("fabric-api"))
    }) {
        inject_mod(
            &prepared.instance_dir,
            &fabric_api_path,
            &format!("fabric-api-{}.jar", "0.116.17+1.21.1"),
        )?;
    }

    let capture_plan = if !request.enable_presentmon {
        None
    } else if request.require_presentmon {
        Some(ensure_presentmon(&request.data_dir)?)
    } else {
        ensure_presentmon(&request.data_dir).ok()
    };
    let capture_output = prepared.run_dir.join("logs").join("presentmon.csv");
    let initial_capture = capture_plan
        .as_ref()
        .map(|plan| capture_config(plan, &capture_output, false, None))
        .unwrap_or_else(|| {
            let disabled = !request.enable_presentmon;
            CaptureConfig {
                backend: if disabled { "disabled" } else { "unavailable" }.to_string(),
                started: false,
                output: Some(capture_output.to_string_lossy().into_owned()),
                executable: None,
                version: None,
                error: Some(if disabled {
                    "disabled by user".to_string()
                } else {
                    "PresentMon was not found or could not be downloaded".to_string()
                }),
            }
        });
    update_run_manifest(prepared, initial_capture, hardware.clone())?;

    write_launcher_status(
        prepared,
        sender,
        "LAUNCHING",
        0.20,
        &format!(
            "Launching Minecraft 1.21.1 Fabric ({} libraries, Java {})",
            runtime.library_count,
            runtime.java_path.display()
        ),
    )?;
    let mut minecraft = spawn_minecraft(prepared, &runtime)?;
    let pid = minecraft.id();

    let mut active_capture = None;
    if let Some(plan) = capture_plan {
        let failure_plan = plan.clone();
        match start_capture(plan, pid, &capture_output) {
            Ok(capture) => {
                let config = capture_config(&capture.plan, &capture.output, true, None);
                if let Err(error) = update_run_manifest(prepared, config, hardware.clone()) {
                    terminate_child(&mut minecraft);
                    let _ = finish_capture(capture);
                    return Err(error).context("failed to publish PresentMon capture state");
                }
                active_capture = Some(capture);
            }
            Err(error) if request.require_presentmon => {
                let message = error.to_string();
                let config =
                    capture_config(&failure_plan, &capture_output, false, Some(message.clone()));
                let _ = update_run_manifest(prepared, config, hardware.clone());
                let _ = write_launcher_status(
                    prepared,
                    sender,
                    "FAILED",
                    0.20,
                    &format!("PresentMon could not start: {message}"),
                );
                terminate_child(&mut minecraft);
                return Err(error).context("required PresentMon capture could not start");
            }
            Err(error) => {
                let config = capture_config(
                    &failure_plan,
                    &capture_output,
                    false,
                    Some(error.to_string()),
                );
                update_run_manifest(prepared, config, hardware.clone())?;
            }
        }
    } else {
        write_launcher_status(
            prepared,
            sender,
            "LAUNCHING",
            0.22,
            "PresentMon unavailable; continuing with Adapter telemetry",
        )?;
    }

    write_launcher_status(
        prepared,
        sender,
        "RUNNING",
        0.25,
        "Minecraft is running; waiting for automatic benchmark completion",
    )?;
    let deadline = Instant::now() + REAL_TIMEOUT;
    let mut last_status_json = String::new();
    loop {
        forward_status(prepared, sender, &mut last_status_json);
        if let Some(exit) = minecraft
            .try_wait()
            .context("failed to inspect Minecraft process")?
        {
            let capture_report = match active_capture.take() {
                Some(capture) => Some(finish_capture(capture)?),
                None => None,
            };
            if !exit.success() {
                bail!(
                    "Minecraft exited with {exit}; inspect {}",
                    prepared.run_dir.join("logs").display()
                );
            }
            let summary = validate_result(prepared)?;
            if request.require_presentmon && capture_report.is_none() {
                bail!("Minecraft completed without an active PresentMon capture");
            }
            if let Some(report) = capture_report {
                return attach_capture_report(prepared, summary, &report);
            }
            return Ok(summary);
        }
        if Instant::now() >= deadline {
            terminate_child(&mut minecraft);
            if let Some(capture) = active_capture.take() {
                let _ = finish_capture(capture);
            }
            bail!(
                "Minecraft benchmark timed out after {} seconds",
                REAL_TIMEOUT.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn spawn_minecraft(prepared: &PreparedRun, runtime: &crate::runtime::RuntimeSpec) -> Result<Child> {
    let stdout_path = prepared.run_dir.join("logs").join("minecraft.stdout.log");
    let stderr_path = prepared.run_dir.join("logs").join("minecraft.stderr.log");
    let stdout = File::create(&stdout_path)
        .with_context(|| format!("failed to create {}", stdout_path.display()))?;
    let stderr = File::create(&stderr_path)
        .with_context(|| format!("failed to create {}", stderr_path.display()))?;
    let mut command = Command::new(&runtime.java_path);
    command
        .args(&runtime.jvm_args)
        .args(&runtime.game_args)
        .current_dir(&prepared.instance_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .spawn()
        .context("failed to spawn Minecraft Java process")
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn capture_config(
    plan: &ExternalCapturePlan,
    output: &Path,
    started: bool,
    error: Option<String>,
) -> CaptureConfig {
    CaptureConfig {
        backend: plan.backend.clone(),
        started,
        output: Some(output.to_string_lossy().into_owned()),
        executable: (!plan.executable.as_os_str().is_empty())
            .then(|| plan.executable.to_string_lossy().into_owned()),
        version: Some(plan.version.clone()),
        error,
    }
}

fn write_launcher_status(
    prepared: &PreparedRun,
    sender: &Sender<RunnerEvent>,
    phase: &str,
    progress: f32,
    message: &str,
) -> Result<()> {
    let status = StatusFile {
        schema: 1,
        run_id: prepared.run_id.clone(),
        state: phase.to_string(),
        phase: phase.to_string(),
        test_index: 0,
        test_total: 3,
        progress,
        message: message.to_string(),
        updated_at: now_iso8601(),
    };
    write_json_atomic(&prepared.status_path, &status)?;
    let _ = sender.send(RunnerEvent::Status(status));
    Ok(())
}

fn forward_status(prepared: &PreparedRun, sender: &Sender<RunnerEvent>, last: &mut String) {
    if let Ok(contents) = fs::read_to_string(&prepared.status_path)
        && contents != *last
    {
        *last = contents.clone();
        if let Ok(status) = serde_json::from_str::<StatusFile>(&contents) {
            let _ = sender.send(RunnerEvent::Status(status));
        }
    }
}

fn validate_result(prepared: &PreparedRun) -> Result<ResultSummary> {
    let complete_marker = prepared.result_dir.join("complete.marker");
    let summary_path = prepared.result_dir.join("summary.json");
    let telemetry_path = prepared.result_dir.join("telemetry.jsonl");
    if !complete_marker.is_file() || !summary_path.is_file() || !telemetry_path.is_file() {
        bail!(
            "Minecraft exited without a complete result; inspect {}",
            prepared.result_dir.display()
        );
    }
    let summary = read_summary(&summary_path)?;
    if summary.run_id != prepared.run_id || summary.status != "COMPLETE" {
        bail!(
            "result validation failed: run_id={}, status={}",
            summary.run_id,
            summary.status
        );
    }
    let telemetry = fs::read(&telemetry_path)
        .with_context(|| format!("failed to read telemetry {}", telemetry_path.display()))?;
    let actual_hash = sha256_bytes(&telemetry);
    if !actual_hash.eq_ignore_ascii_case(&summary.telemetry_sha256) {
        bail!(
            "telemetry SHA-256 mismatch: summary={}, actual={}",
            summary.telemetry_sha256,
            actual_hash
        );
    }
    Ok(summary)
}

fn attach_capture_report(
    prepared: &PreparedRun,
    summary: ResultSummary,
    report: &CaptureReport,
) -> Result<ResultSummary> {
    let report_path = prepared.result_dir.join("presentmon.json");
    write_json_atomic(&report_path, report)?;
    let summary_path = prepared.result_dir.join("summary.json");
    let mut value: Value = serde_json::from_reader(
        File::open(&summary_path)
            .with_context(|| format!("failed to open summary {}", summary_path.display()))?,
    )?;
    value["verification"] = Value::String("LOCAL_REAL_PRESENTMON".to_string());
    let metadata = value
        .get_mut("metadata")
        .and_then(Value::as_object_mut)
        .context("result metadata is not an object")?;
    metadata.insert("presentmon.version".to_string(), json!(report.version));
    metadata.insert("presentmon.csv".to_string(), json!(report.output));
    metadata.insert("presentmon.csv_sha256".to_string(), json!(report.sha256));
    metadata.insert(
        "presentmon.frame_count".to_string(),
        json!(report.frame_count),
    );
    metadata.insert(
        "presentmon.average_frame_time_ms".to_string(),
        json!(report.average_frame_time_ms),
    );
    metadata.insert(
        "presentmon.p99_frame_time_ms".to_string(),
        json!(report.p99_frame_time_ms),
    );
    metadata.insert(
        "presentmon.average_fps".to_string(),
        json!(report.average_fps),
    );
    metadata.insert(
        "presentmon.one_percent_low_fps".to_string(),
        json!(report.one_percent_low_fps),
    );
    write_json_atomic(&summary_path, &value)?;
    let updated = read_summary(&summary_path)?;
    if updated.run_id != summary.run_id {
        bail!("PresentMon result update changed run identity");
    }
    Ok(updated)
}

fn read_summary(path: &Path) -> Result<ResultSummary> {
    serde_json::from_reader(
        File::open(path).with_context(|| format!("failed to open result {}", path.display()))?,
    )
    .with_context(|| format!("invalid result JSON {}", path.display()))
}

fn send_failure(sender: &Sender<RunnerEvent>, prepared: &PreparedRun, message: String) {
    let _ = sender.send(RunnerEvent::Failed {
        run_dir: Some(prepared.run_dir.clone()),
        message,
    });
}

fn write_failure_artifact(prepared: &PreparedRun, message: &str) -> Result<()> {
    let failure = json!({
        "run_id": prepared.run_id,
        "status": "FAILED",
        "message": message,
        "at": now_iso8601(),
    });
    write_json_atomic(&prepared.result_dir.join("failure.json"), &failure)
}

fn run_mock_process(request: MockRunRequest, mock_binary: PathBuf, sender: Sender<RunnerEvent>) {
    let prepared = match prepare_run(&PrepareRunRequest {
        data_dir: request.data_dir,
        minecraft_version: request.minecraft_version,
        loader: request.loader,
        selected_mods: request.selected_mods,
        source_instance_dir: None,
        benchmark: BenchmarkConfig::default(),
    }) {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = sender.send(RunnerEvent::Failed {
                run_dir: None,
                message: format!("failed to prepare temporary run: {error:#}"),
            });
            return;
        }
    };
    let _ = sender.send(RunnerEvent::Prepared(Box::new(prepared.clone())));

    let stdout_path = prepared.run_dir.join("logs").join("mock.stdout.log");
    let stderr_path = prepared.run_dir.join("logs").join("mock.stderr.log");
    let stdout = match File::create(&stdout_path) {
        Ok(file) => file,
        Err(error) => {
            send_failure(
                &sender,
                &prepared,
                format!("failed to create stdout log: {error}"),
            );
            return;
        }
    };
    let stderr = match File::create(&stderr_path) {
        Ok(file) => file,
        Err(error) => {
            send_failure(
                &sender,
                &prepared,
                format!("failed to create stderr log: {error}"),
            );
            return;
        }
    };

    let embedded_mock = std::env::current_exe()
        .ok()
        .is_some_and(|current| current == mock_binary);
    let mut command = Command::new(&mock_binary);
    if embedded_mock {
        command.arg("--mock-child");
    }
    let mut child = match command
        .arg("--manifest")
        .arg(&prepared.manifest_path)
        .arg("--run-dir")
        .arg(&prepared.run_dir)
        .current_dir(&prepared.instance_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            send_failure(
                &sender,
                &prepared,
                format!("failed to spawn mock benchmark process: {error}"),
            );
            return;
        }
    };

    let deadline = Instant::now() + MOCK_TIMEOUT;
    let mut last_status_json = String::new();
    loop {
        forward_status(&prepared, &sender, &mut last_status_json);
        match child.try_wait() {
            Ok(Some(exit)) => {
                let complete_marker = prepared.result_dir.join("complete.marker");
                let summary_path = prepared.result_dir.join("summary.json");
                if exit.success() && complete_marker.is_file() && summary_path.is_file() {
                    match read_summary(&summary_path) {
                        Ok(summary)
                            if summary.run_id == prepared.run_id
                                && summary.status == "COMPLETE" =>
                        {
                            let _ = sender.send(RunnerEvent::Completed {
                                prepared: Box::new(prepared),
                                summary: Box::new(summary),
                            });
                        }
                        Ok(summary) => send_failure(
                            &sender,
                            &prepared,
                            format!(
                                "result validation failed: run_id={}, status={}",
                                summary.run_id, summary.status
                            ),
                        ),
                        Err(error) => send_failure(
                            &sender,
                            &prepared,
                            format!("failed to read result summary: {error:#}"),
                        ),
                    }
                } else {
                    send_failure(
                        &sender,
                        &prepared,
                        format!("mock benchmark exited with {exit}, without a complete result"),
                    );
                }
                return;
            }
            Ok(None) => {}
            Err(error) => {
                send_failure(
                    &sender,
                    &prepared,
                    format!("failed to inspect child process: {error}"),
                );
                return;
            }
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            send_failure(
                &sender,
                &prepared,
                "mock benchmark timed out after 120 seconds".to_string(),
            );
            return;
        }
        thread::sleep(Duration::from_millis(80));
    }
}
