use minebench::instance::{PrepareRunRequest, prepare_run};
use minebench::modscan::scan_mods;
use minebench::protocol::{BenchmarkConfig, ResultSummary, StatusFile};
use std::fs;
use std::io::Write;
use std::process::Command;
use uuid::Uuid;
use zip::write::SimpleFileOptions;

#[test]
fn mock_child_process_completes_file_protocol() {
    let root = std::env::temp_dir().join(format!("minebench-e2e-{}", Uuid::new_v4()));
    let source = root.join("source-mods");
    fs::create_dir_all(&source).unwrap();
    make_fabric_jar(&source.join("example mod.jar"));
    let mods = scan_mods(&source).unwrap();
    let prepared = prepare_run(&PrepareRunRequest {
        data_dir: root.join("data"),
        minecraft_version: "1.21.1".to_string(),
        loader: "fabric".to_string(),
        selected_mods: mods,
        source_instance_dir: None,
        benchmark: BenchmarkConfig::default(),
    })
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_minebench"))
        .arg("--mock-child")
        .arg("--manifest")
        .arg(&prepared.manifest_path)
        .arg("--run-dir")
        .arg(&prepared.run_dir)
        .env("MINEBENCH_MOCK_FAST", "1")
        .status()
        .unwrap();
    assert!(status.success());

    let final_status: StatusFile =
        serde_json::from_reader(fs::File::open(&prepared.status_path).unwrap()).unwrap();
    assert_eq!(final_status.run_id, prepared.run_id);
    assert_eq!(final_status.state, "SHUTDOWN");

    let summary_path = prepared.result_dir.join("summary.json");
    let summary: ResultSummary =
        serde_json::from_reader(fs::File::open(summary_path).unwrap()).unwrap();
    assert_eq!(summary.status, "COMPLETE");
    assert_eq!(summary.run_id, prepared.run_id);
    assert!(summary.overall_score > 0.0);
    assert!(prepared.result_dir.join("telemetry.jsonl").is_file());
    assert!(prepared.result_dir.join("complete.marker").is_file());
    assert!(source.join("example mod.jar").is_file());

    let _ = fs::remove_dir_all(root);
}

fn make_fabric_jar(path: &std::path::Path) {
    let file = fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("fabric.mod.json", SimpleFileOptions::default())
        .unwrap();
    writer
        .write_all(br#"{"schemaVersion":1,"id":"example","version":"1"}"#)
        .unwrap();
    writer.finish().unwrap();
}
