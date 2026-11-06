use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("out dir"));
    let root = manifest_dir.parent().expect("repository root");
    let catalog_path = manifest_dir.join("adapter-catalog.json");
    let catalog: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(&catalog_path).expect("adapter catalog"))
            .expect("valid adapter catalog");
    let mut table = String::from("static ADAPTERS: &[(&str, &[u8])] = &[\n");
    for entry in catalog {
        let version = entry["minecraft"].as_str().expect("Minecraft version");
        let folder = root.join("build").join("versions").join(version);
        let jar = fs::read_dir(&folder)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.extension().is_some_and(|ext| ext == "jar")
                    && !path.to_string_lossy().contains("-sources")
                    && !path.to_string_lossy().contains("-dev")
            });
        let destination = out_dir.join(format!("adapter-{version}.bin"));
        if let Some(path) = jar {
            fs::copy(&path, &destination).expect("copy versioned Adapter");
            println!("cargo:rerun-if-changed={}", path.display());
        } else {
            if env::var("PROFILE").as_deref() == Ok("release") {
                panic!(
                    "missing Adapter for Minecraft {version}; run python scripts/build_versions.py first"
                );
            }
            fs::write(&destination, []).expect("empty debug Adapter");
        }
        table.push_str(&format!(
            "({version:?}, include_bytes!({:?})),\n",
            destination.to_string_lossy()
        ));
    }
    table.push_str("];\n");
    fs::write(out_dir.join("embedded_adapters.rs"), table).expect("write Adapter table");
    println!("cargo:rerun-if-changed={}", catalog_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("build/versions").display()
    );
    if cfg!(windows) && env::var("PROFILE").as_deref() == Ok("release") {
        println!("cargo:rerun-if-changed=minebench.rc");
        embed_resource::compile("minebench.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed the MineBench UAC manifest");
    }
}
