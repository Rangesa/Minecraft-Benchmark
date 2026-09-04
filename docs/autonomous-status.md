# MineBench autonomous status

## Current phase

Phase 2: Universal telemetry and deterministic-run groundwork, followed by the first Adapter boundary refactor.

## Completed in this cycle

- Run manifest now carries the benchmark seed, remote chunk-probe coordinates, standard client settings, and score profile.
- The Fabric Adapter applies those settings at runtime and verifies the created world's seed.
- Source `options.txt`, `config`, `resourcepacks`, and `shaderpacks` are no longer copied into benchmark runs.
- Chunk samples use absolute remote coordinates with a spacing stride and reject already loaded targets.
- Storage metadata is selected from the source Instance's drive when possible.
- Active GPU metadata uses an attached-display/healthy-adapter selection and records the selection method.
- The score is explicitly marked `PROVISIONAL`.
- The embedded real Adapter selection is centralized behind the Adapter resolver.

## Current compatibility

- Fabric 1.21.1: real Adapter vertical slice is implemented and previously passed real Minecraft launch, autorun, telemetry, and shutdown.
- Fabric other versions: detected, but no compatible deep Adapter is published yet.
- Forge, NeoForge, Quilt: detected, but no real deep Adapter is published yet.
- PresentMon: integrated and pinned, but Windows ETW lifecycle/flush behavior still needs a clean regression run.

See `compatibility-matrix.json` for the machine-readable state.

## Next automatic actions

1. Split the current Fabric 1.21.1 Adapter into a version-family contract and binding layer.
2. Add a clean PresentMon capture lifecycle test and promote the current environment only after CSV flush is repeatable.
3. Add Fabric 1.20.1, 1.20.4, and 1.21.4 Adapter fixtures in separate compatibility entries.
4. Add NeoForge modern-family discovery and a STANDARD external-measurement path.
5. Revisit the provisional score with a published calibration dataset before enabling ranking uploads.
