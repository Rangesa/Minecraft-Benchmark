# MineBench

MineBench is a Windows launcher plus a real Minecraft benchmark Adapter.

## Current production path

MineBench.exe starts an isolated temporary instance, launches Minecraft 1.21.1 Fabric, creates a fixed-seed world automatically, applies benchmark-owned graphics settings, runs warmup and graphics/simulation/chunk measurements, writes raw telemetry and a summary, exits Minecraft, and displays the local result.

The current Adapter is pinned to Minecraft 1.21.1, Fabric Loader 0.19.5, and Fabric API 0.116.17+1.21.1. The detector recognizes Fabric, Forge, NeoForge, and Quilt metadata; only the Fabric 1.21.1 runtime is enabled in this implementation. Unsupported loader/version combinations fail closed instead of launching the wrong runtime.

## Build

Run this from the project directory:

    .\scripts\build-release.ps1

The distribution artifact is:

    dist\MineBench.exe

The release executable embeds the Fabric Adapter JAR and has a Windows requireAdministrator manifest because PresentMon needs an elevated trace session on this machine. Windows shows UAC before the launcher starts.

## Use

Start dist\MineBench.exe, choose an Instance folder or its mods folder, press Detect, then press Start Benchmark. The source instance is not opened for writing. MineBench hardlinks or copies selected JARs into a new run directory; source options, configs, resource packs, shader packs, saves, logs, accounts, and launcher caches are not copied. The Adapter applies the benchmark settings from the run manifest so user render distance and shader settings cannot change the local score.

For a repeatable command-line run:

    .\dist\MineBench.exe --run-real --instance "D:\Minecraft\Instances\Example" --fast

The fast option is a short validation profile. A normal GUI run uses the full standard profile. The no-presentmon option disables external capture for local development diagnostics only and is not the production path.

## Local files and downloads

MineBench keeps run data and caches below %LOCALAPPDATA%\MineBench unless MINEBENCH_DATA_DIR is set. The one-file distribution downloads and verifies the official Mojang runtime, Fabric Loader libraries, Fabric API, and PresentMon into that cache on first use. The original Minecraft instance remains outside this write root.

Each completed run contains:

- instance\minebench\result\summary.json
- instance\minebench\result\telemetry.jsonl
- instance\minebench\result\presentmon.json when PresentMon is active
- logs\presentmon.csv
- logs\presentmon.stdout.log and logs\presentmon.stderr.log
- manifest.json

The launcher verifies the telemetry SHA-256 and the completed run identity before displaying the score.

The current score profile is explicitly marked provisional (`MB-SCORE-PROVISIONAL-v0`). Its fixed seed, remote chunk-probe coordinates, benchmark settings, and baseline values are recorded in each run manifest and result metadata until the public scoring calibration is finalized.
