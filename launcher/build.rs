use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("out dir"));
    let configured = env::var_os("MINEBENCH_ADAPTER_JAR").map(PathBuf::from);
    let candidates = [
        configured,
        Some(manifest_dir.join("..\\build\\libs\\mcbench-1.0.jar")),
    ];
    let source = candidates
        .into_iter()
        .flatten()
        .find(|path| path.is_file() && path.extension().is_some_and(|value| value == "jar"));
    let destination = out_dir.join("embedded_adapter.bin");
    match source {
        Some(path) => {
            fs::copy(&path, &destination).unwrap_or_else(|error| {
                panic!("failed to embed Fabric Adapter {}: {error}", path.display())
            });
            println!("cargo:rerun-if-changed={}", path.display());
        }
        None => {
            // Keep `cargo check` usable before Gradle has produced the JAR. A
            // real release will fail at runtime with a precise adapter error;
            // the release script builds Gradle first and embeds the artifact.
            fs::write(&destination, []).expect("create empty embedded adapter");
            println!(
                "cargo:warning=Fabric Adapter JAR not found; build Gradle before a real release"
            );
        }
    }
    println!("cargo:rerun-if-env-changed=MINEBENCH_ADAPTER_JAR");
    if cfg!(windows) && env::var("PROFILE").as_deref() == Ok("release") {
        println!("cargo:rerun-if-changed=minebench.rc");
        embed_resource::compile("minebench.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed the MineBench UAC manifest");
    }
}
