use anyhow::{Context, Result, bail};
use eframe::egui::{self, Color32, RichText};
use minebench::adapter::{AdapterResolution, load_manifest};
use minebench::detection::{InstanceDetection, detect_instance};
use minebench::external::discover_external_capture;
use minebench::paths::default_data_dir;
use minebench::protocol::{BenchmarkConfig, ResultSummary, StatusFile};
use minebench::runner::{RealRunRequest, RunnerEvent, start_real_run};
use std::env;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

fn main() -> eframe::Result {
    let arguments = env::args().collect::<Vec<_>>();
    if arguments.get(1).map(String::as_str) == Some("--mock-child") {
        if let Err(error) = minebench::mock::run_from(std::env::args().skip(2)) {
            eprintln!("MineBench embedded Adapter failed: {error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if arguments.get(1).map(String::as_str) == Some("--run-real") {
        if let Err(error) = run_real_cli(arguments.into_iter().skip(2)) {
            eprintln!("MineBench real run failed: {error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 820.0])
            .with_min_inner_size([640.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MineBench",
        options,
        Box::new(|_creation_context| Ok(Box::new(MineBenchApp::default()))),
    )
}

struct MineBenchApp {
    minecraft_version: String,
    loader: String,
    instance_or_mods_path: String,
    username: String,
    data_dir: PathBuf,
    detection: Option<InstanceDetection>,
    detection_message: String,
    adapter_message: String,
    show_mod_editor: bool,
    run_receiver: Option<Receiver<RunnerEvent>>,
    running: bool,
    progress: f32,
    phase: String,
    status_message: String,
    current_run_id: Option<String>,
    result_path: Option<PathBuf>,
    summary: Option<ResultSummary>,
}

impl Default for MineBenchApp {
    fn default() -> Self {
        Self {
            minecraft_version: String::new(),
            loader: "auto".to_string(),
            instance_or_mods_path: String::new(),
            username: "MineBench".to_string(),
            data_dir: default_data_dir(),
            detection: None,
            detection_message: "Choose an instance folder or its mods folder".to_string(),
            adapter_message: "No environment detected".to_string(),
            show_mod_editor: false,
            run_receiver: None,
            running: false,
            progress: 0.0,
            phase: "IDLE".to_string(),
            status_message: "Ready".to_string(),
            current_run_id: None,
            result_path: None,
            summary: None,
        }
    }
}

impl eframe::App for MineBenchApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_runner();

        let mut should_detect = false;
        let mut should_start = false;
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                ui.heading(RichText::new("MINEBENCH").size(28.0).strong());
                ui.label("Version-independent benchmark orchestrator");
                ui.add_space(18.0);
            });

            egui::Grid::new("configuration")
                .num_columns(2)
                .spacing([16.0, 12.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new("Minecraft Version").strong());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.minecraft_version)
                            .hint_text("auto-detected or e.g. 1.21.1")
                            .desired_width(330.0),
                    );
                    ui.end_row();

                    ui.label(RichText::new("Loader").strong());
                    egui::ComboBox::from_id_salt("loader")
                        .selected_text(&self.loader)
                        .width(330.0)
                        .show_ui(ui, |ui| {
                            for option in [
                                "auto", "vanilla", "fabric", "forge", "neoforge", "quilt", "custom",
                            ] {
                                ui.selectable_value(&mut self.loader, option.to_string(), option);
                            }
                        });
                    ui.end_row();

                    ui.label(RichText::new("Instance / Mods Folder").strong());
                    ui.horizontal(|ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.instance_or_mods_path)
                                .hint_text(r"D:\Minecraft\instance or ...\mods")
                                .desired_width(250.0),
                        );
                        if response.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        {
                            should_detect = true;
                        }
                        if ui.button("Browse...").clicked()
                            && let Some(path) = rfd::FileDialog::new().pick_folder()
                        {
                            self.instance_or_mods_path = path.display().to_string();
                            should_detect = true;
                        }
                        if ui.button("Detect").clicked() {
                            should_detect = true;
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Detected Mods").strong());
                    ui.horizontal(|ui| {
                        let count = self
                            .detection
                            .as_ref()
                            .map(|detection| detection.mods.len())
                            .unwrap_or(0);
                        let selected = self
                            .detection
                            .as_ref()
                            .map(|detection| {
                                detection.mods.iter().filter(|item| item.selected).count()
                            })
                            .unwrap_or(0);
                        ui.label(format!("{count} found / {selected} selected"));
                        if ui
                            .add_enabled(count > 0, egui::Button::new("Edit"))
                            .clicked()
                        {
                            self.show_mod_editor = true;
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Username").strong());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.username)
                            .hint_text("display name")
                            .desired_width(330.0),
                    );
                    ui.end_row();
                });

            ui.add_space(12.0);
            egui::CollapsingHeader::new("Detection and Adapter details")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(&self.detection_message);
                    ui.label(&self.adapter_message);
                    if let Some(detection) = &self.detection {
                        ui.label(format!(
                            "Instance root: {}",
                            detection.instance_dir.display()
                        ));
                        for warning in &detection.warnings {
                            ui.colored_label(Color32::YELLOW, format!("Warning: {warning}"));
                        }
                    }
                    ui.label(format!("Local data: {}", self.data_dir.display()));
                    if let Some(capture) = discover_external_capture() {
                        ui.label(format!(
                            "External fallback: {} ({})",
                            capture.backend,
                            capture.executable.display()
                        ));
                    } else {
                        ui.label(
                            "PresentMon: official v2.5.1 is downloaded to the MineBench cache when a run starts",
                        );
                    }
                });

            ui.add_space(16.0);
            if self.running {
                ui.group(|ui| {
                    ui.label(RichText::new("BENCHMARK RUNNING").strong());
                    ui.label(&self.status_message);
                    ui.add(egui::ProgressBar::new(self.progress).text(format!(
                        "{} {:.0}%",
                        self.phase,
                        self.progress * 100.0
                    )));
                    ui.small("Minecraft runs in an isolated temporary instance.");
                });
            } else if let Some(summary) = &self.summary {
                render_result(ui, summary, self.result_path.as_deref());
            } else {
                ui.label(&self.status_message);
            }

            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                let button = egui::Button::new(
                    RichText::new(if self.summary.is_some() {
                        "RUN AGAIN"
                    } else {
                        "START BENCHMARK"
                    })
                    .size(18.0),
                )
                .min_size(egui::vec2(260.0, 48.0));
                if ui.add_enabled(!self.running, button).clicked() {
                    should_start = true;
                }
            });
        });

        if should_detect {
            self.detect_environment();
        }
        if should_start {
            self.start_benchmark();
        }
        if self.show_mod_editor {
            self.render_mod_editor(&ctx);
        }
        if self.running {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

impl MineBenchApp {
    fn detect_environment(&mut self) {
        let text = self.instance_or_mods_path.trim();
        if text.is_empty() {
            self.detection = None;
            self.detection_message = "Enter an instance or mods folder first".to_string();
            self.adapter_message = "No environment detected".to_string();
            return;
        }
        match detect_instance(&PathBuf::from(text)) {
            Ok(detection) => {
                if let Some(version) = &detection.minecraft_version {
                    self.minecraft_version = version.clone();
                }
                if detection.loader.family != "unknown" && detection.loader.family != "ambiguous" {
                    self.loader = detection.loader.family.clone();
                }
                let mods_count = detection.mods.len();
                let loader = detection.loader.family.clone();
                self.detection_message = format!(
                    "Detected: {} / {} / {} JAR(s)",
                    detection
                        .minecraft_version
                        .as_deref()
                        .unwrap_or("unknown Version"),
                    loader,
                    mods_count
                );
                self.detection = Some(detection);
                self.refresh_adapter_message();
            }
            Err(error) => {
                self.detection = None;
                self.detection_message = format!("Detection failed: {error:#}");
                self.adapter_message = "No Adapter selected".to_string();
            }
        }
    }

    fn refresh_adapter_message(&mut self) {
        let Some(detection) = &self.detection else {
            self.adapter_message = "No environment detected".to_string();
            return;
        };
        match load_manifest(None) {
            Ok(manifest) => match manifest.resolve(detection, false) {
                AdapterResolution::Adapter(artifact) => {
                    self.adapter_message = format!(
                        "Adapter: {} {} [{}] (embedded real Minecraft Adapter)",
                        artifact.id, artifact.version, artifact.kind
                    );
                }
                AdapterResolution::ExternalFallback { reason } => {
                    self.adapter_message =
                        format!("External fallback available; real Adapter not selected: {reason}");
                }
            },
            Err(error) => self.adapter_message = format!("Adapter manifest error: {error:#}"),
        }
    }

    fn start_benchmark(&mut self) {
        if self.instance_or_mods_path.trim().is_empty() {
            self.status_message = "An instance or mods folder is required".to_string();
            return;
        }
        if self.detection.is_none() {
            self.detect_environment();
        }
        let Some(detection) = &self.detection else {
            return;
        };
        let version = if self.minecraft_version.trim().is_empty() {
            detection
                .minecraft_version
                .clone()
                .unwrap_or_else(|| "1.21.1".to_string())
        } else {
            self.minecraft_version.trim().to_string()
        };
        let loader = if self.loader == "auto" {
            match detection.loader.family.as_str() {
                "ambiguous" => {
                    self.status_message =
                        "Multiple loader families were detected; choose the Loader explicitly"
                            .to_string();
                    return;
                }
                "unknown" => "fabric".to_string(),
                family => family.to_string(),
            }
        } else {
            self.loader.clone()
        };
        let selected_mods = detection
            .mods
            .iter()
            .filter(|record| record.selected)
            .cloned()
            .collect::<Vec<_>>();
        let request = RealRunRequest {
            data_dir: self.data_dir.clone(),
            source_instance_dir: detection.instance_dir.clone(),
            minecraft_version: version.clone(),
            loader: loader.clone(),
            loader_version: detection.loader.version.clone(),
            username: if self.username.trim().is_empty() {
                "MineBench".to_string()
            } else {
                self.username.trim().to_string()
            },
            selected_mods,
            benchmark: BenchmarkConfig::default(),
            heap_mb: 4096,
            enable_presentmon: true,
            require_presentmon: true,
        };
        match start_real_run(request) {
            Ok(receiver) => {
                self.run_receiver = Some(receiver);
                self.running = true;
                self.progress = 0.0;
                self.phase = "PREPARING".to_string();
                self.status_message = "Creating isolated temporary instance".to_string();
                self.current_run_id = None;
                self.result_path = None;
                self.summary = None;
            }
            Err(error) => self.status_message = format!("Could not start benchmark: {error:#}"),
        }
    }

    fn poll_runner(&mut self) {
        let events = self
            .run_receiver
            .as_ref()
            .map(|receiver| receiver.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut finished = false;
        for event in events {
            match event {
                RunnerEvent::Prepared(prepared) => {
                    self.current_run_id = Some(prepared.run_id);
                    self.status_message =
                        "Preparing isolated instance and official Minecraft runtime".to_string();
                }
                RunnerEvent::Status(status) => self.apply_status(status),
                RunnerEvent::Completed { prepared, summary } => {
                    self.apply_summary(
                        prepared
                            .run_dir
                            .join("instance/minebench/result/summary.json"),
                        *summary,
                    );
                    finished = true;
                }
                RunnerEvent::Failed { run_dir, message } => {
                    self.running = false;
                    self.phase = "FAILED".to_string();
                    self.status_message = message;
                    self.result_path = run_dir.map(|path| path.join("instance/minebench/result"));
                    finished = true;
                }
            }
        }
        if finished {
            self.run_receiver = None;
        }
    }

    fn apply_status(&mut self, status: StatusFile) {
        self.current_run_id = Some(status.run_id);
        self.phase = status.phase;
        self.progress = status.progress.clamp(0.0, 1.0);
        self.status_message = status.message;
    }

    fn apply_summary(&mut self, result_path: PathBuf, summary: ResultSummary) {
        self.running = false;
        self.phase = "COMPLETE".to_string();
        self.progress = 1.0;
        self.status_message = "Benchmark finished; result verified locally".to_string();
        self.result_path = Some(result_path);
        self.summary = Some(summary);
    }

    fn render_mod_editor(&mut self, ctx: &egui::Context) {
        let mut open = self.show_mod_editor;
        egui::Window::new("Select Mods")
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| {
                let Some(detection) = self.detection.as_mut() else {
                    ui.label("No detected mods");
                    return;
                };
                ui.label(
                    "The source folder is read-only; selections affect only the temporary run.",
                );
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for record in &mut detection.mods {
                        let title = record.name.as_deref().unwrap_or(record.filename.as_str());
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut record.selected, "");
                            ui.label(format!(
                                "{}  [{}] {}",
                                title,
                                record.loader_family,
                                record.version.as_deref().unwrap_or("unknown")
                            ));
                        });
                    }
                });
            });
        self.show_mod_editor = open;
    }
}

fn render_result(
    ui: &mut egui::Ui,
    summary: &ResultSummary,
    result_path: Option<&std::path::Path>,
) {
    ui.group(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("MINEBENCH SCORE").strong());
            ui.label(RichText::new(format!("{:.0}", summary.overall_score)).size(34.0));
            ui.label(format!(
                "{} / {} / {}",
                summary.verification, summary.score_version, summary.score_status
            ));
        });
        ui.separator();
        egui::Grid::new("result_scores")
            .num_columns(2)
            .spacing([24.0, 6.0])
            .show(ui, |ui| {
                ui.label("Graphics");
                ui.label(format!("{:.0}", summary.scores.graphics));
                ui.end_row();
                ui.label("Simulation");
                ui.label(format!("{:.0}", summary.scores.simulation));
                ui.end_row();
                ui.label("World Generation");
                ui.label(format!("{:.0}", summary.scores.world_generation));
                ui.end_row();
                ui.label("Average FPS");
                ui.label(format!("{:.1}", summary.metrics.average_fps));
                ui.end_row();
                ui.label("1% Low FPS");
                ui.label(format!("{:.1}", summary.metrics.one_percent_low_fps));
                ui.end_row();
                ui.label("Median MSPT");
                ui.label(format!("{:.2}", summary.metrics.median_mspt));
                ui.end_row();
                ui.label("Chunk/s");
                ui.label(format!("{:.1}", summary.metrics.chunks_per_second));
                ui.end_row();
            });
        if let Some(path) = result_path {
            ui.add_space(6.0);
            ui.small(format!("Local result: {}", path.display()));
        }
    });
}

fn run_real_cli<I>(arguments: I) -> Result<()>
where
    I: IntoIterator<Item = String>,
{
    let mut instance = None;
    let mut data_dir = default_data_dir();
    let mut version = None;
    let mut loader = None;
    let mut username = "MineBench".to_string();
    let mut benchmark = BenchmarkConfig::default();
    let mut enable_presentmon = true;
    let mut require_presentmon = true;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--instance" => instance = arguments.next().map(PathBuf::from),
            "--data-dir" => {
                data_dir = arguments
                    .next()
                    .map(PathBuf::from)
                    .context("--data-dir requires a path")?;
            }
            "--version" => version = arguments.next(),
            "--loader" => loader = arguments.next(),
            "--username" => {
                username = arguments.next().context("--username requires a value")?;
            }
            "--fast" => {
                benchmark = BenchmarkConfig {
                    warmup_ticks: 10,
                    graphics_frames: 120,
                    simulation_ticks: 40,
                    chunk_samples: 8,
                    entity_count: 16,
                    ..BenchmarkConfig::default()
                };
            }
            "--no-presentmon" => {
                enable_presentmon = false;
                require_presentmon = false;
            }
            "--help" | "-h" => {
                println!(
                    "MineBench --run-real --instance <path> [--data-dir <path>] [--fast] [--no-presentmon]"
                );
                return Ok(());
            }
            unknown => bail!("unknown --run-real argument: {unknown}"),
        }
    }
    let instance = instance.context("--instance is required")?;
    let detection = detect_instance(&instance)
        .with_context(|| format!("failed to detect instance {}", instance.display()))?;
    let minecraft_version = version
        .or(detection.minecraft_version.clone())
        .unwrap_or_else(|| "1.21.1".to_string());
    let loader = loader
        .or_else(|| {
            (!matches!(detection.loader.family.as_str(), "unknown" | "ambiguous"))
                .then(|| detection.loader.family.clone())
        })
        .unwrap_or_else(|| "fabric".to_string());
    let selected_mods = detection
        .mods
        .iter()
        .filter(|record| record.selected)
        .cloned()
        .collect::<Vec<_>>();
    let receiver = start_real_run(RealRunRequest {
        data_dir,
        source_instance_dir: detection.instance_dir,
        minecraft_version,
        loader,
        loader_version: detection.loader.version,
        username,
        selected_mods,
        benchmark,
        heap_mb: 4096,
        enable_presentmon,
        require_presentmon,
    })?;
    for event in receiver {
        match event {
            RunnerEvent::Prepared(prepared) => {
                println!("prepared run {}", prepared.run_id);
            }
            RunnerEvent::Status(status) => {
                println!(
                    "{} {:.0}% {}",
                    status.phase,
                    status.progress * 100.0,
                    status.message
                );
            }
            RunnerEvent::Completed { prepared, summary } => {
                println!("COMPLETE score={:.0}", summary.overall_score);
                println!("result={}", prepared.result_dir.display());
                println!("{}", serde_json::to_string_pretty(&summary)?);
                return Ok(());
            }
            RunnerEvent::Failed { run_dir, message } => {
                if let Some(run_dir) = run_dir {
                    bail!("{message}; run logs: {}", run_dir.join("logs").display());
                }
                bail!("{message}");
            }
        }
    }
    bail!("real benchmark runner ended without a completion event")
}
