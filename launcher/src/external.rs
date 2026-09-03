use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PRESENTMON_VERSION: &str = "2.5.1";
const PRESENTMON_URL: &str =
    "https://github.com/GameTechDev/PresentMon/releases/download/v2.5.1/PresentMon-2.5.1-x64.exe";
const PRESENTMON_SHA256: &str = "9bec3083069f58f911e6a512f4806db51a27bd096103087bc1d05ef54c80a191";
const PRESENTMON_SIZE: u64 = 956_768;
const MAX_CAPTURE_DOWNLOAD_BYTES: u64 = 16 * 1024 * 1024;
const PRESENTMON_SESSION_NAME: &str = "MineBench";

#[derive(Debug, Clone)]
pub struct ExternalCapturePlan {
    pub backend: String,
    pub executable: PathBuf,
    pub arguments_template: Vec<String>,
    pub verification: String,
    pub version: String,
}

pub struct ActiveCapture {
    child: Child,
    pub output: PathBuf,
    stderr: PathBuf,
    stdout: PathBuf,
    pub plan: ExternalCapturePlan,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureReport {
    pub backend: String,
    pub version: String,
    pub output: String,
    pub sha256: String,
    pub frame_count: usize,
    pub average_frame_time_ms: f64,
    pub p99_frame_time_ms: f64,
    pub average_fps: f64,
    pub one_percent_low_fps: f64,
}

pub fn discover_external_capture() -> Option<ExternalCapturePlan> {
    let executable = env::var_os("MINEBENCH_EXTERNAL_CAPTURE_BIN")
        .map(PathBuf::from)
        .or_else(|| {
            find_on_path(if cfg!(windows) {
                "PresentMon.exe"
            } else {
                "PresentMon"
            })
        })?;
    if !executable.is_file() {
        return None;
    }
    Some(plan_for_path(executable, "configured"))
}

pub fn ensure_presentmon(data_dir: &Path) -> Result<ExternalCapturePlan> {
    if let Some(plan) = discover_external_capture() {
        return Ok(plan);
    }
    let destination = data_dir
        .join("tools")
        .join(format!("PresentMon-{PRESENTMON_VERSION}-x64.exe"));
    let valid = destination.is_file()
        && fs::metadata(&destination).is_ok_and(|metadata| metadata.len() == PRESENTMON_SIZE)
        && sha256_file(&destination)
            .is_ok_and(|actual| actual.eq_ignore_ascii_case(PRESENTMON_SHA256));
    if !valid {
        download_presentmon(&destination)?;
    }
    Ok(plan_for_path(destination, PRESENTMON_VERSION))
}

pub fn start_capture(plan: ExternalCapturePlan, pid: u32, output: &Path) -> Result<ActiveCapture> {
    validate_output_path(output)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create PresentMon output directory {}",
                parent.display()
            )
        })?;
    }
    stop_existing_session(&plan);
    let arguments = materialize_arguments(&plan, pid, output);
    let stderr_path = output.with_file_name("presentmon.stderr.log");
    let stdout_path = output.with_file_name("presentmon.stdout.log");
    let stderr = File::create(&stderr_path)
        .with_context(|| format!("failed to create PresentMon log {}", stderr_path.display()))?;
    let stdout = File::create(&stdout_path)
        .with_context(|| format!("failed to create PresentMon log {}", stdout_path.display()))?;
    let child = Command::new(&plan.executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .with_context(|| format!("failed to start PresentMon {}", plan.executable.display()))?;
    let mut active = ActiveCapture {
        child,
        output: output.to_path_buf(),
        stderr: stderr_path,
        stdout: stdout_path,
        plan,
    };
    std::thread::sleep(Duration::from_millis(100));
    if let Some(status) = active
        .child
        .try_wait()
        .context("failed to inspect PresentMon")?
    {
        let detail = capture_stderr(&active.stderr);
        let detail = capture_process_output(&active.stdout, detail);
        if detail.is_empty() {
            bail!("PresentMon exited before Minecraft rendering began: {status}");
        }
        bail!("PresentMon exited before Minecraft rendering began: {status}; {detail}");
    }
    Ok(active)
}

pub fn finish_capture(mut active: ActiveCapture) -> Result<CaptureReport> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut exit = None;
    while Instant::now() < deadline {
        if let Some(status) = active
            .child
            .try_wait()
            .context("failed to inspect PresentMon")?
        {
            exit = Some(status);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if exit.is_none() {
        stop_existing_session(&active.plan);
        let drain_deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < drain_deadline {
            if let Some(status) = active
                .child
                .try_wait()
                .context("failed to inspect PresentMon after stopping its trace session")?
            {
                exit = Some(status);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if exit.is_none() {
            let _ = active.child.kill();
            exit = active.child.wait().ok();
        }
    }
    let output = &active.output;
    if !output.is_file() {
        let detail = capture_process_output(&active.stdout, capture_stderr(&active.stderr));
        if detail.is_empty() {
            bail!(
                "PresentMon exited without creating CSV output: {} (exit status: {})",
                output.display(),
                exit.as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "unknown".to_string())
            );
        }
        bail!(
            "PresentMon exited without creating CSV output: {} (exit status: {}); {}",
            output.display(),
            exit.as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown".to_string()),
            detail
        );
    }
    let metadata = fs::metadata(output)
        .with_context(|| format!("failed to stat PresentMon output {}", output.display()))?;
    if metadata.len() < 32 {
        let detail = capture_process_output(&active.stdout, capture_stderr(&active.stderr));
        if detail.is_empty() {
            bail!(
                "PresentMon CSV output is empty: {} (exit status: {})",
                output.display(),
                exit.as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "unknown".to_string())
            );
        }
        bail!(
            "PresentMon CSV output is empty: {} (exit status: {}); {}",
            output.display(),
            exit.as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown".to_string()),
            detail
        );
    }
    let metrics = parse_presentmon_csv(output)?;
    if metrics.frame_count == 0 {
        bail!("PresentMon CSV contains no usable frame-time samples");
    }
    let _ = exit;
    Ok(CaptureReport {
        backend: active.plan.backend,
        version: active.plan.version,
        output: output.to_string_lossy().into_owned(),
        sha256: sha256_file(output)?,
        frame_count: metrics.frame_count,
        average_frame_time_ms: metrics.average_frame_time_ms,
        p99_frame_time_ms: metrics.p99_frame_time_ms,
        average_fps: metrics.average_fps,
        one_percent_low_fps: metrics.one_percent_low_fps,
    })
}

pub fn materialize_arguments(plan: &ExternalCapturePlan, pid: u32, output: &Path) -> Vec<String> {
    plan.arguments_template
        .iter()
        .map(|argument| {
            argument
                .replace("{pid}", &pid.to_string())
                .replace("{output}", &output.to_string_lossy())
        })
        .collect()
}

pub fn require_external_capture(data_dir: &Path) -> Result<ExternalCapturePlan> {
    ensure_presentmon(data_dir)
}

fn plan_for_path(executable: PathBuf, version: &str) -> ExternalCapturePlan {
    let arguments_template = env::var("MINEBENCH_EXTERNAL_CAPTURE_ARGS")
        .ok()
        .map(|value| value.split_whitespace().map(ToOwned::to_owned).collect())
        .unwrap_or_else(|| {
            vec![
                "--process_id".to_string(),
                "{pid}".to_string(),
                "--output_file".to_string(),
                "{output}".to_string(),
                "--no_console_stats".to_string(),
                "--qpc_time_ms".to_string(),
                "--terminate_on_proc_exit".to_string(),
                "--stop_existing_session".to_string(),
                "--session_name".to_string(),
                PRESENTMON_SESSION_NAME.to_string(),
            ]
        });
    ExternalCapturePlan {
        backend: "presentmon".to_string(),
        executable,
        arguments_template,
        verification: "PRESENTMON_CSV".to_string(),
        version: version.to_string(),
    }
}

fn stop_existing_session(plan: &ExternalCapturePlan) {
    let session_name = session_name_for_plan(plan);
    let Ok(query) = Command::new("logman.exe")
        .args(["query", session_name.as_str(), "-ets"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };
    if !query.status.success() {
        return;
    }

    let Ok(mut child) = Command::new(&plan.executable)
        .args([
            "--terminate_existing_session",
            "--session_name",
            session_name.as_str(),
            "--no_console_stats",
            "--no_csv",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    std::thread::sleep(Duration::from_millis(150));
}

fn session_name_for_plan(plan: &ExternalCapturePlan) -> String {
    plan.arguments_template
        .windows(2)
        .find_map(|pair| (pair[0] == "--session_name").then(|| pair[1].clone()))
        .filter(|name| !name.is_empty() && !name.contains('{'))
        .unwrap_or_else(|| PRESENTMON_SESSION_NAME.to_string())
}

fn download_presentmon(destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create PresentMon cache {}", parent.display()))?;
    }
    let temporary = destination.with_extension("download");
    let mut response = ureq::get(PRESENTMON_URL)
        .call()
        .context("failed to download official PresentMon")?;
    if response
        .body()
        .content_length()
        .is_some_and(|size| size > MAX_CAPTURE_DOWNLOAD_BYTES)
    {
        bail!("PresentMon download exceeds the safety limit");
    }
    let mut file = File::create(&temporary).with_context(|| {
        format!(
            "failed to create PresentMon download {}",
            temporary.display()
        )
    })?;
    let mut reader = response
        .body_mut()
        .as_reader()
        .take(MAX_CAPTURE_DOWNLOAD_BYTES + 1);
    let size = std::io::copy(&mut reader, &mut file)?;
    file.flush().ok();
    if size != PRESENTMON_SIZE || !sha256_file(&temporary)?.eq_ignore_ascii_case(PRESENTMON_SHA256)
    {
        let _ = fs::remove_file(&temporary);
        bail!("downloaded PresentMon failed pinned size/SHA-256 verification");
    }
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(temporary, destination)?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to hash {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hasher.update(&buffer[..size]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn capture_stderr(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .replace(['\r', '\n'], " ")
}

fn capture_process_output(stdout: &Path, stderr_text: String) -> String {
    let stdout_text = capture_stderr(stdout);
    match (stdout_text.is_empty(), stderr_text.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("stdout: {stdout_text}"),
        (true, false) => format!("stderr: {stderr_text}"),
        (false, false) => format!("stdout: {stdout_text}; stderr: {stderr_text}"),
    }
}

#[derive(Debug)]
struct CsvMetrics {
    frame_count: usize,
    average_frame_time_ms: f64,
    p99_frame_time_ms: f64,
    average_fps: f64,
    one_percent_low_fps: f64,
}

fn parse_presentmon_csv(path: &Path) -> Result<CsvMetrics> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read PresentMon CSV {}", path.display()))?;
    let content = String::from_utf8_lossy(&bytes);
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
    let header_line = lines
        .find(|line| line.contains(',') && line.to_ascii_lowercase().contains("process"))
        .context("PresentMon CSV header was not found")?;
    let headers = parse_csv_line(header_line);
    let frame_index = headers
        .iter()
        .position(|header| {
            let normalized = normalize_header(header);
            matches!(
                normalized.as_str(),
                "msbetweenpresents" | "frametime" | "displayedtodisplayed"
            )
        })
        .context("PresentMon CSV has no frame-time column")?;
    let mut samples = Vec::new();
    for line in lines {
        let fields = parse_csv_line(line);
        let Some(value) = fields.get(frame_index) else {
            continue;
        };
        let Ok(milliseconds) = value.trim().parse::<f64>() else {
            continue;
        };
        if milliseconds.is_finite() && milliseconds > 0.0 && milliseconds < 10_000.0 {
            samples.push(milliseconds);
        }
    }
    if samples.is_empty() {
        bail!("PresentMon CSV has no numeric frame-time values");
    }
    samples.sort_by(f64::total_cmp);
    let average = samples.iter().sum::<f64>() / samples.len() as f64;
    let p99 = percentile_f64(&samples, 0.99);
    Ok(CsvMetrics {
        frame_count: samples.len(),
        average_frame_time_ms: average,
        p99_frame_time_ms: p99,
        average_fps: 1_000.0 / average,
        one_percent_low_fps: 1_000.0 / p99,
    })
}

fn normalize_header(header: &str) -> String {
    header
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' if quoted && characters.peek() == Some(&'"') => {
                current.push('"');
                let _ = characters.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                result.push(current.trim().to_string());
                current.clear();
            }
            value => current.push(value),
        }
    }
    result.push(current.trim().to_string());
    result
}

fn percentile_f64(values: &[f64], quantile: f64) -> f64 {
    let position = quantile.clamp(0.0, 1.0) * (values.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        return values[lower];
    }
    values[lower] + (values[upper] - values[lower]) * (position - lower as f64)
}

fn find_on_path(filename: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn validate_output_path(path: &Path) -> Result<()> {
    if path.extension().is_none() {
        bail!("external capture output must have a file extension");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_parser_preserves_quoted_commas() {
        assert_eq!(
            parse_csv_line("\"Minecraft, Client\",123,4.5"),
            vec!["Minecraft, Client", "123", "4.5"]
        );
    }

    #[test]
    fn frame_time_column_is_normalized() {
        assert_eq!(normalize_header("Ms Between Presents"), "msbetweenpresents");
    }

    #[test]
    fn argument_templates_replace_process_and_output() {
        let plan = plan_for_path(PathBuf::from("PresentMon.exe"), "test");
        let args = materialize_arguments(&plan, 42, Path::new("capture.csv"));
        assert!(args.contains(&"42".to_string()));
        assert!(args.contains(&"capture.csv".to_string()));
    }
}
