# Minecraft version builds

Fabric Adapter targets: **1.21.1, 1.21.2, 1.21.3, 1.21.4, 1.21.5, 1.21.6,
1.21.7, 1.21.8, 1.21.9, 1.21.10, 1.21.11, 26.1, 26.1.1, 26.1.2, 26.2**.
Forge, NeoForge and Quilt remain detectable but have no embedded runtime Adapter.
Unsupported combinations still fail before a real benchmark starts.

Each game version has its own Adapter JAR and a pinned Fabric API artifact with
SHA-1/size verification. The launcher embeds all 15 Adapters, resolves the exact
game version, and creates its existing isolated temporary instance. Java 21+
is required for 1.21.x, and Java 25+ for 26.x. The 1.21.1 runtime keeps Loader
0.19.5; the new runtime targets pin Loader 0.20.1.

## Build

Set `JAVA_HOME` to JDK 25 and install Python 3 and the Rust MSVC toolchain.
Windows release builds also require the Visual Studio C++ build tools/Windows SDK.

```powershell
python scripts/build_versions.py
python scripts/build_versions.py --version 26.2
cargo test --workspace --locked
.\scripts\build-release.ps1
```

Installable Adapters are collected under `build/versions/<minecraft-version>/`.
The release executable is `dist/MineBench.exe`. A release build refuses to embed
an incomplete Adapter set. Debug builds remain available before all Adapters have
been built, and refuse to launch a target with missing Adapter bytes.

`minecraft-versions.json` pins compile inputs; `launcher/adapter-catalog.json` pins
the launcher runtime, required Java version, and Fabric API hash/size. Regression
tests compare the two catalogs. Version-specific source families live in `versions/`;
the root project retains its 1.21.1 target.

## Verification limits

The build checks compilation, packaged metadata/entrypoints/Java bytecode, and
Mixin injection/invoker targets against each game version. Launcher tests cover
exact Adapter selection, catalog consistency, unsupported combinations, and the
existing mock child-process file protocol.

The new game targets have **not** been verified in an interactive world or a real
PresentMon measurement. The earlier 1.21.1 real-run evidence remains recorded with
its original date in `docs/compatibility-matrix.json`. Compile success does not
promote a new target to the existing `STANDARD`/`FULL` runtime evidence tiers.
Scores retain the existing provisional profile; cross-version score calibration
has not been performed.
