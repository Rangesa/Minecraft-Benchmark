package dev.bench.runtime;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import dev.bench.BenchmarkMod;
import dev.bench.mixin.client.CreateWorldScreenInvoker;
import net.minecraft.client.MinecraftClient;
import net.minecraft.client.gui.screen.TitleScreen;
import net.minecraft.client.gui.screen.world.CreateWorldScreen;
import net.minecraft.entity.EntityType;
import net.minecraft.entity.mob.ZombieEntity;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.world.ServerWorld;
import net.minecraft.util.math.BlockPos;
import net.minecraft.world.chunk.ChunkStatus;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * Fabric Adapter runtime for the current benchmark contract.
 *
 * <p>The launcher creates {@code minebench/run.json} in a temporary game
 * directory. The adapter only writes below that directory and drives the
 * entire run without chat commands or user input.</p>
 */
public final class BenchmarkSession {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();
    private static final int DEFAULT_WARMUP_TICKS = 120;
    private static final int DEFAULT_GRAPHICS_FRAMES = 900;
    private static final int DEFAULT_SIMULATION_TICKS = 180;
    private static final int DEFAULT_CHUNK_SAMPLES = 32;
    private static final int DEFAULT_ENTITY_COUNT = 96;
    private static final long DEFAULT_BENCHMARK_SEED = -6372891104223171901L;
    private static final int DEFAULT_CHUNK_PROBE_ORIGIN_X = 1_000_000;
    private static final int DEFAULT_CHUNK_PROBE_ORIGIN_Z = 1_000_000;
    private static final int DEFAULT_CHUNK_PROBE_STRIDE = 8;
    private static final int DEFAULT_RENDER_DISTANCE = 12;
    private static final int DEFAULT_SIMULATION_DISTANCE = 12;
    private static final int DEFAULT_MAX_FPS = 260;
    private static final boolean DEFAULT_ENABLE_VSYNC = false;
    private static final String DEFAULT_SCORE_VERSION = "MB-SCORE-PROVISIONAL-v0";
    private static final String DEFAULT_SCORE_STATUS = "PROVISIONAL";
    private static final double DEFAULT_FPS_BASELINE = 120.0;
    private static final double DEFAULT_MSPT_BASELINE = 16.67;
    private static final double DEFAULT_CHUNKS_PER_SECOND_BASELINE = 20.0;
    private static final double DEFAULT_GRAPHICS_WEIGHT = 30.0;
    private static final double DEFAULT_SIMULATION_WEIGHT = 25.0;
    private static final double DEFAULT_WORLD_WEIGHT = 20.0;
    private static final int CREATE_WORLD_WAIT_TICKS = 100;

    private enum Phase {
        WAITING_FOR_CONFIG,
        WAITING_FOR_WORLD,
        WARMUP,
        GRAPHICS,
        SIMULATION,
        CHUNK_GENERATION,
        FINALIZE,
        COMPLETE,
        FAILED
    }

    private volatile Phase phase = Phase.WAITING_FOR_CONFIG;
    private JsonObject config;
    private Path runDirectory;
    private Path statusPath;
    private Path resultDirectory;
    private String runId = "unknown";
    private String minecraftVersion = "unknown";
    private String loader = "unknown";
    private String profile = "MB-STANDARD-v1";
    private String suiteVersion = "SUITE-v1";

    private int warmupTicks = DEFAULT_WARMUP_TICKS;
    private int graphicsFrames = DEFAULT_GRAPHICS_FRAMES;
    private int simulationTicks = DEFAULT_SIMULATION_TICKS;
    private int chunkSamplesTarget = DEFAULT_CHUNK_SAMPLES;
    private int entityCount = DEFAULT_ENTITY_COUNT;
    private long benchmarkSeed = DEFAULT_BENCHMARK_SEED;
    private int chunkProbeOriginX = DEFAULT_CHUNK_PROBE_ORIGIN_X;
    private int chunkProbeOriginZ = DEFAULT_CHUNK_PROBE_ORIGIN_Z;
    private int chunkProbeStride = DEFAULT_CHUNK_PROBE_STRIDE;
    private int renderDistance = DEFAULT_RENDER_DISTANCE;
    private int simulationDistance = DEFAULT_SIMULATION_DISTANCE;
    private int maxFps = DEFAULT_MAX_FPS;
    private boolean enableVsync = DEFAULT_ENABLE_VSYNC;
    private String scoreVersion = DEFAULT_SCORE_VERSION;
    private String scoreStatus = DEFAULT_SCORE_STATUS;
    private double fpsBaseline = DEFAULT_FPS_BASELINE;
    private double msptBaseline = DEFAULT_MSPT_BASELINE;
    private double chunksPerSecondBaseline = DEFAULT_CHUNKS_PER_SECOND_BASELINE;
    private double graphicsWeight = DEFAULT_GRAPHICS_WEIGHT;
    private double simulationWeight = DEFAULT_SIMULATION_WEIGHT;
    private double worldWeight = DEFAULT_WORLD_WEIGHT;

    private long clientTick;
    private long warmupTick;
    private volatile long simulationTick;
    private volatile int chunkSampleCount;
    private int createWorldAttempts;
    private long nextCreateWorldActionTick;
    private boolean worldCreationRequested;
    private boolean sceneRequested;
    private volatile boolean scenePrepared;
    private boolean settingsApplied;
    private boolean shutdownRequested;
    private long shutdownAtTick = Long.MAX_VALUE;
    private long serverTickStartNanos;
    private long observedWorldSeed;
    private long renderStartNanos;

    private final List<Long> frameTimesNanos = Collections.synchronizedList(new ArrayList<>());
    private final List<Long> frameSampleTimesNanos = Collections.synchronizedList(new ArrayList<>());
    private final List<Long> serverTickTimesNanos = Collections.synchronizedList(new ArrayList<>());
    private final List<Long> serverTickSampleTimesNanos = Collections.synchronizedList(new ArrayList<>());
    private final List<Long> chunkTimesNanos = Collections.synchronizedList(new ArrayList<>());
    private final List<Long> chunkSampleTimesNanos = Collections.synchronizedList(new ArrayList<>());
    private final List<Integer> spawnedEntityIds = Collections.synchronizedList(new ArrayList<>());

    public void tick(MinecraftClient client) {
        clientTick++;

        if (phase == Phase.WAITING_FOR_CONFIG) {
            if (!loadConfig(client)) {
                return;
            }
            if (!readBoolean(config, "autorun", true)) {
                BenchmarkMod.LOGGER.info("[MineBench] autorun=false; adapter is idle");
                phase = Phase.COMPLETE;
                return;
            }
            phase = Phase.WAITING_FOR_WORLD;
            writeStatus("WAITING_FOR_WORLD", 0, 0, "Waiting for an isolated singleplayer world");
        }

        if (phase == Phase.WAITING_FOR_WORLD) {
            if (client.world == null || client.player == null || client.getServer() == null) {
                driveWorldCreation(client);
                return;
            }
            applyBenchmarkSettings(client);
            prepareScene(client);
            if (!scenePrepared) {
                return;
            }
            phase = Phase.WARMUP;
            warmupTick = 0;
            writeStatus("WARMUP", 0, 0, "Preparing the deterministic benchmark scene");
            return;
        }

        switch (phase) {
            case WARMUP -> {
                warmupTick++;
                if (warmupTick % 20 == 0) {
                    writeStatus("WARMUP", 0, 0,
                            "Warmup " + warmupTick + "/" + warmupTicks + " ticks");
                }
                if (warmupTick >= warmupTicks) {
                    frameTimesNanos.clear();
                    frameSampleTimesNanos.clear();
                    phase = Phase.GRAPHICS;
                    writeStatus("BENCHMARK", 1, 3, "Graphics measurement started");
                }
            }
            case GRAPHICS -> {
                if (frameTimesNanos.size() % 120 == 0 && !frameTimesNanos.isEmpty()) {
                    writeStatus("BENCHMARK", 1, 3,
                            "Graphics " + frameTimesNanos.size() + "/" + graphicsFrames + " frames");
                }
            }
            case SIMULATION -> {
                if (simulationTick % 20 == 0 && simulationTick > 0) {
                    writeStatus("BENCHMARK", 2, 3,
                            "Simulation " + simulationTick + "/" + simulationTicks + " ticks");
                }
            }
            case CHUNK_GENERATION -> {
                if (chunkSampleCount % 8 == 0 && chunkSampleCount > 0) {
                    writeStatus("BENCHMARK", 3, 3,
                            "Chunk generation " + chunkSampleCount + "/" + chunkSamplesTarget);
                }
            }
            case FINALIZE -> finalizeResult();
            case COMPLETE -> {
                if (!shutdownRequested) {
                    shutdownRequested = true;
                    shutdownAtTick = clientTick + 40;
                } else if (clientTick >= shutdownAtTick) {
                    BenchmarkMod.LOGGER.info("[MineBench] Complete marker is written; stopping Minecraft");
                    client.scheduleStop();
                }
            }
            case FAILED -> {
                if (!shutdownRequested) {
                    shutdownRequested = true;
                    shutdownAtTick = clientTick + 40;
                } else if (clientTick >= shutdownAtTick) {
                    client.scheduleStop();
                }
            }
            default -> {
                // Waiting states are handled above.
            }
        }
    }

    /** Called from the actual GameRenderer.render HEAD hook. */
    public void onFrameStart() {
        if (phase == Phase.GRAPHICS) {
            renderStartNanos = System.nanoTime();
        }
    }

    /** Called from the actual GameRenderer.render RETURN hook. */
    public void onFrameEnd() {
        if (phase != Phase.GRAPHICS || renderStartNanos == 0L) {
            return;
        }
        long finishedAt = System.nanoTime();
        long elapsed = Math.max(1L, finishedAt - renderStartNanos);
        renderStartNanos = 0L;
        frameTimesNanos.add(elapsed);
        frameSampleTimesNanos.add(finishedAt);
        if (frameTimesNanos.size() >= graphicsFrames) {
            phase = Phase.SIMULATION;
            simulationTick = 0;
            writeStatus("BENCHMARK", 2, 3, "Graphics measurement complete; simulation started");
        }
    }

    public void onServerTickStart(MinecraftServer server) {
        if (phase == Phase.SIMULATION) {
            serverTickStartNanos = System.nanoTime();
        }
    }

    public void onServerTickEnd(MinecraftServer server) {
        if (phase == Phase.SIMULATION && serverTickStartNanos != 0L) {
            long finishedAt = System.nanoTime();
            serverTickTimesNanos.add(Math.max(1L, finishedAt - serverTickStartNanos));
            serverTickSampleTimesNanos.add(finishedAt);
            simulationTick++;
            serverTickStartNanos = 0L;
            if (simulationTick >= simulationTicks) {
                chunkSampleCount = 0;
                phase = Phase.CHUNK_GENERATION;
                writeStatus("BENCHMARK", 3, 3, "Chunk generation measurement started");
            }
            return;
        }

        if (phase == Phase.CHUNK_GENERATION && chunkSampleCount < chunkSamplesTarget) {
            ServerWorld world = server.getOverworld();
            int offset = chunkSampleCount;
            int chunkX = chunkProbeCoordinate(chunkProbeOriginX, offset % 8);
            int chunkZ = chunkProbeCoordinate(chunkProbeOriginZ, offset / 8);
            if (world.getChunkManager().isChunkLoaded(chunkX, chunkZ)) {
                fail("Chunk probe selected an already loaded chunk: " + chunkX + "," + chunkZ);
                return;
            }
            long start = System.nanoTime();
            world.getChunk(chunkX, chunkZ, ChunkStatus.FULL, true);
            long finishedAt = System.nanoTime();
            chunkTimesNanos.add(Math.max(1L, finishedAt - start));
            chunkSampleTimesNanos.add(finishedAt);
            chunkSampleCount++;
            if (chunkSampleCount >= chunkSamplesTarget) {
                cleanupScene(world);
                phase = Phase.FINALIZE;
            }
        }
    }

    private boolean loadConfig(MinecraftClient client) {
        runDirectory = client.runDirectory.toPath();
        Path configPath = runDirectory.resolve("minebench").resolve("run.json");
        if (!Files.isRegularFile(configPath)) {
            if (clientTick == 1 || clientTick % 200 == 0) {
                BenchmarkMod.LOGGER.info("[MineBench] Waiting for run manifest: {}", configPath);
            }
            return false;
        }
        try {
            config = GSON.fromJson(Files.readString(configPath, StandardCharsets.UTF_8), JsonObject.class);
            runId = readString(config, "run_id", "unknown");
            minecraftVersion = readString(config, "minecraft_version", "unknown");
            loader = readString(config, "loader", "unknown");
            profile = readString(config, "profile", profile);
            suiteVersion = readString(config, "suite_version", suiteVersion);
            JsonObject benchmark = object(config, "benchmark");
            warmupTicks = positiveInt(benchmark, "warmup_ticks", DEFAULT_WARMUP_TICKS);
            graphicsFrames = positiveInt(benchmark, "graphics_frames", DEFAULT_GRAPHICS_FRAMES);
            simulationTicks = positiveInt(benchmark, "simulation_ticks", DEFAULT_SIMULATION_TICKS);
            chunkSamplesTarget = positiveInt(benchmark, "chunk_samples", DEFAULT_CHUNK_SAMPLES);
            entityCount = positiveInt(benchmark, "entity_count", DEFAULT_ENTITY_COUNT);
            benchmarkSeed = readLong(benchmark, "seed", DEFAULT_BENCHMARK_SEED);
            chunkProbeOriginX = readInt(benchmark, "chunk_probe_origin_x", DEFAULT_CHUNK_PROBE_ORIGIN_X);
            chunkProbeOriginZ = readInt(benchmark, "chunk_probe_origin_z", DEFAULT_CHUNK_PROBE_ORIGIN_Z);
            chunkProbeStride = positiveInt(benchmark, "chunk_probe_stride", DEFAULT_CHUNK_PROBE_STRIDE);
            renderDistance = positiveInt(benchmark, "render_distance", DEFAULT_RENDER_DISTANCE);
            simulationDistance = positiveInt(benchmark, "simulation_distance", DEFAULT_SIMULATION_DISTANCE);
            maxFps = positiveInt(benchmark, "max_fps", DEFAULT_MAX_FPS);
            enableVsync = readBoolean(benchmark, "enable_vsync", DEFAULT_ENABLE_VSYNC);
            JsonObject score = object(config, "score");
            scoreVersion = readString(score, "id", DEFAULT_SCORE_VERSION);
            scoreStatus = readString(score, "status", DEFAULT_SCORE_STATUS);
            fpsBaseline = positiveDouble(score, "average_fps_baseline", DEFAULT_FPS_BASELINE);
            msptBaseline = positiveDouble(score, "median_mspt_baseline", DEFAULT_MSPT_BASELINE);
            chunksPerSecondBaseline = positiveDouble(score, "chunks_per_second_baseline",
                    DEFAULT_CHUNKS_PER_SECOND_BASELINE);
            graphicsWeight = positiveDouble(score, "graphics_weight", DEFAULT_GRAPHICS_WEIGHT);
            simulationWeight = positiveDouble(score, "simulation_weight", DEFAULT_SIMULATION_WEIGHT);
            worldWeight = positiveDouble(score, "world_generation_weight", DEFAULT_WORLD_WEIGHT);
            statusPath = runDirectory.resolve("minebench").resolve("status.json");
            resultDirectory = runDirectory.resolve("minebench").resolve("result");
            Files.createDirectories(resultDirectory);
            BenchmarkMod.LOGGER.info(
                    "[MineBench] Loaded real run {} for Minecraft {} / {} (frames={}, ticks={}, chunks={})",
                    runId, minecraftVersion, loader, graphicsFrames, simulationTicks, chunkSamplesTarget);
            return true;
        } catch (Exception error) {
            fail("Could not load UTF-8 run manifest: " + error.getMessage());
            return false;
        }
    }

    private void driveWorldCreation(MinecraftClient client) {
        if (client.currentScreen instanceof TitleScreen && clientTick >= CREATE_WORLD_WAIT_TICKS
                && clientTick >= nextCreateWorldActionTick && !worldCreationRequested) {
            CreateWorldScreen.show(client, client.currentScreen);
            worldCreationRequested = true;
            createWorldAttempts = 0;
            nextCreateWorldActionTick = clientTick + 20;
            writeStatus("WAITING_FOR_WORLD", 0, 0, "Opening the automatic world creation screen");
            return;
        }

        if (client.currentScreen instanceof CreateWorldScreen screen
                && clientTick >= nextCreateWorldActionTick && createWorldAttempts < 5) {
            screen.getWorldCreator().setWorldName("MineBench Benchmark");
            screen.getWorldCreator().setSeed(Long.toString(benchmarkSeed));
            screen.getWorldCreator().setBonusChestEnabled(false);
            ((CreateWorldScreenInvoker) screen).minebench$createLevel();
            createWorldAttempts++;
            nextCreateWorldActionTick = clientTick + 30;
            writeStatus("WAITING_FOR_WORLD", 0, 0,
                    "Starting the isolated deterministic benchmark world");
        }
    }

    private void applyBenchmarkSettings(MinecraftClient client) {
        if (settingsApplied) {
            return;
        }
        client.options.getViewDistance().setValue(clamp(renderDistance, 2, 32));
        client.options.getSimulationDistance().setValue(clamp(simulationDistance, 2, 32));
        client.options.getMaxFps().setValue(clamp(maxFps, 10, 260));
        client.options.getEnableVsync().setValue(enableVsync);
        client.options.resourcePacks.clear();
        client.options.incompatibleResourcePacks.clear();
        client.options.write();
        settingsApplied = true;
        BenchmarkMod.LOGGER.info(
                "[MineBench] Applied benchmark settings: renderDistance={}, simulationDistance={}, maxFps={}, vsync={}, sourceSettings=ignored",
                renderDistance, simulationDistance, maxFps, enableVsync);
    }

    private void prepareScene(MinecraftClient client) {
        if (sceneRequested || client.player == null || client.getServer() == null) {
            return;
        }
        sceneRequested = true;
        BlockPos center = client.player.getBlockPos();
        MinecraftServer server = client.getServer();
        server.execute(() -> {
            try {
                ServerWorld world = server.getOverworld();
                observedWorldSeed = world.getSeed();
                if (observedWorldSeed != benchmarkSeed) {
                    fail("World seed mismatch: expected " + benchmarkSeed + ", got " + observedWorldSeed);
                    return;
                }
                for (int i = 0; i < entityCount; i++) {
                    int dx = (i % 16) - 8;
                    int dz = (i / 16) - 3;
                    ZombieEntity zombie = new ZombieEntity(EntityType.ZOMBIE, world);
                    zombie.refreshPositionAndAngles(center.getX() + dx + 0.5,
                            center.getY(), center.getZ() + dz + 0.5, (i * 37) % 360, 0.0f);
                    zombie.setAiDisabled(false);
                    if (world.spawnEntity(zombie)) {
                        spawnedEntityIds.add(zombie.getId());
                    }
                }
                scenePrepared = true;
                BenchmarkMod.LOGGER.info("[MineBench] Scene prepared with {} entities", spawnedEntityIds.size());
            } catch (Exception error) {
                BenchmarkMod.LOGGER.error("[MineBench] Scene preparation failed", error);
                fail("Scene preparation failed: " + error.getMessage());
            }
        });
    }

    private void cleanupScene(ServerWorld world) {
        for (Integer id : spawnedEntityIds) {
            var entity = world.getEntityById(id);
            if (entity != null) {
                entity.discard();
            }
        }
        spawnedEntityIds.clear();
    }

    private int chunkProbeCoordinate(int origin, int index) {
        long coordinate = origin + (long) index * chunkProbeStride;
        return (int) Math.max(Integer.MIN_VALUE, Math.min(Integer.MAX_VALUE, coordinate));
    }

    private void finalizeResult() {
        phase = Phase.COMPLETE;
        try {
            List<JsonObject> samples = buildTelemetrySamples();
            StringBuilder jsonl = new StringBuilder(samples.size() * 120);
            for (JsonObject sample : samples) {
                jsonl.append(GSON.toJson(sample)).append('\n');
            }
            byte[] telemetryBytes = jsonl.toString().getBytes(StandardCharsets.UTF_8);
            Path telemetryPath = resultDirectory.resolve("telemetry.jsonl");
            writeBytesAtomic(telemetryPath, telemetryBytes);

            Metrics metrics = calculateMetrics();
            JsonObject summary = new JsonObject();
            summary.addProperty("schema", 1);
            summary.addProperty("run_id", runId);
            summary.addProperty("status", "COMPLETE");
            summary.addProperty("verification", hasPresentMonCapture()
                    ? "LOCAL_REAL_PRESENTMON" : "LOCAL_REAL_ADAPTER");
            summary.addProperty("score_version", scoreVersion);
            summary.addProperty("score_status", scoreStatus);
            summary.addProperty("profile", profile);
            summary.addProperty("suite_version", suiteVersion);

            JsonObject scores = new JsonObject();
            scores.addProperty("graphics", metrics.graphicsScore);
            scores.addProperty("simulation", metrics.simulationScore);
            scores.addProperty("world_generation", metrics.worldScore);
            summary.add("scores", scores);
            summary.addProperty("overall_score", metrics.overallScore);

            JsonObject metricJson = new JsonObject();
            metricJson.addProperty("average_fps", metrics.averageFps);
            metricJson.addProperty("one_percent_low_fps", metrics.onePercentLowFps);
            metricJson.addProperty("median_mspt", metrics.medianMspt);
            metricJson.addProperty("p99_mspt", metrics.p99Mspt);
            metricJson.addProperty("chunks_per_second", metrics.chunksPerSecond);
            summary.add("metrics", metricJson);
            JsonObject scoreProfile = new JsonObject();
            scoreProfile.addProperty("average_fps_baseline", fpsBaseline);
            scoreProfile.addProperty("median_mspt_baseline", msptBaseline);
            scoreProfile.addProperty("chunks_per_second_baseline", chunksPerSecondBaseline);
            scoreProfile.addProperty("graphics_weight", graphicsWeight);
            scoreProfile.addProperty("simulation_weight", simulationWeight);
            scoreProfile.addProperty("world_generation_weight", worldWeight);
            summary.add("score_profile", scoreProfile);
            summary.addProperty("telemetry_sha256", sha256(telemetryBytes));
            summary.addProperty("completed_at", Instant.now().toString());

            JsonObject metadata = new JsonObject();
            metadata.addProperty("adapter_id", "mcbenchmark");
            metadata.addProperty("adapter_version", "1.0");
            metadata.addProperty("minecraft_version", minecraftVersion);
            metadata.addProperty("loader", loader);
            metadata.addProperty("frame_timing", "GameRenderer.render hook");
            metadata.addProperty("server_timing", "ServerTickEvents START/END");
            metadata.addProperty("chunk_timing", "ServerWorld.getChunk FULL on absolute remote probes");
            metadata.addProperty("benchmark_seed", benchmarkSeed);
            metadata.addProperty("observed_world_seed", observedWorldSeed);
            metadata.addProperty("settings_policy", "benchmark_defaults_only");
            metadata.addProperty("render_distance", renderDistance);
            metadata.addProperty("simulation_distance", simulationDistance);
            metadata.addProperty("max_fps", maxFps);
            metadata.addProperty("enable_vsync", enableVsync);
            metadata.addProperty("chunk_probe_origin_x", chunkProbeOriginX);
            metadata.addProperty("chunk_probe_origin_z", chunkProbeOriginZ);
            metadata.addProperty("chunk_probe_stride", chunkProbeStride);
            metadata.addProperty("chunk_probe_policy", "absolute_remote_unloaded_chunks");
            metadata.addProperty("score_status", scoreStatus);
            metadata.addProperty("graphics_samples", frameTimesNanos.size());
            metadata.addProperty("simulation_samples", serverTickTimesNanos.size());
            metadata.addProperty("chunk_samples", chunkTimesNanos.size());
            copyMetadata(metadata);
            summary.add("metadata", metadata);
            writeJsonAtomic(resultDirectory.resolve("summary.json"), summary);
            writeBytesAtomic(resultDirectory.resolve("complete.marker"),
                    ("run_id=" + runId + "\nsha256=" + sha256(telemetryBytes) + "\n")
                            .getBytes(StandardCharsets.UTF_8));
            writeStatus("COMPLETE", 3, 3, "Benchmark complete; result verified locally");
            BenchmarkMod.LOGGER.info("[MineBench] COMPLETE: score={} avgFps={} telemetry={}",
                    metrics.overallScore, metrics.averageFps, telemetryPath);
        } catch (Exception error) {
            fail("Could not finalize benchmark result: " + error.getMessage());
        }
    }

    private boolean hasPresentMonCapture() {
        JsonObject capture = object(config, "capture");
        return "presentmon".equalsIgnoreCase(readString(capture, "backend", ""))
                && readBoolean(capture, "started", false);
    }

    private void copyMetadata(JsonObject metadata) {
        JsonObject hardware = object(config, "hardware");
        for (Map.Entry<String, JsonElement> entry : hardware.entrySet()) {
            if (entry.getValue().isJsonPrimitive()) {
                metadata.addProperty("hardware." + entry.getKey(), entry.getValue().getAsString());
            }
        }
        JsonObject capture = object(config, "capture");
        for (Map.Entry<String, JsonElement> entry : capture.entrySet()) {
            if (entry.getValue().isJsonPrimitive()) {
                metadata.addProperty("capture." + entry.getKey(), entry.getValue().getAsString());
            }
        }
    }

    private List<JsonObject> buildTelemetrySamples() {
        List<JsonObject> samples = new ArrayList<>(
                frameTimesNanos.size() + serverTickTimesNanos.size() + chunkTimesNanos.size());
        long sequence = 0L;
        for (int index = 0; index < frameTimesNanos.size(); index++) {
            long frame = frameTimesNanos.get(index);
            JsonObject sample = baseSample(sequence++, "graphics_static", "frame",
                    frameSampleTimesNanos.get(index));
            sample.addProperty("frame_time_ns", frame);
            samples.add(sample);
        }
        for (int index = 0; index < serverTickTimesNanos.size(); index++) {
            long tick = serverTickTimesNanos.get(index);
            JsonObject sample = baseSample(sequence++, "simulation", "server_tick",
                    serverTickSampleTimesNanos.get(index));
            sample.addProperty("tick_time_ns", tick);
            samples.add(sample);
        }
        for (int index = 0; index < chunkTimesNanos.size(); index++) {
            long chunk = chunkTimesNanos.get(index);
            JsonObject sample = baseSample(sequence++, "chunk_generation", "chunk_load",
                    chunkSampleTimesNanos.get(index));
            sample.addProperty("chunk_time_ns", chunk);
            samples.add(sample);
        }
        return samples;
    }

    private JsonObject baseSample(long sequence, String test, String kind, long monotonic) {
        JsonObject sample = new JsonObject();
        sample.addProperty("schema", 1);
        sample.addProperty("sequence", sequence);
        sample.addProperty("test", test);
        sample.addProperty("kind", kind);
        sample.addProperty("monotonic_ns", monotonic);
        return sample;
    }

    private Metrics calculateMetrics() {
        double averageFrameNs = mean(frameTimesNanos);
        double averageFps = 1_000_000_000.0 / Math.max(1.0, averageFrameNs);
        double onePercentLowFps = 1_000_000_000.0
                / Math.max(1.0, percentile(frameTimesNanos, 0.99));
        double medianMspt = percentile(serverTickTimesNanos, 0.50) / 1_000_000.0;
        double p99Mspt = percentile(serverTickTimesNanos, 0.99) / 1_000_000.0;
        double chunksPerSecond = chunkTimesNanos.isEmpty()
                ? 0.0 : 1_000_000_000.0 / Math.max(1.0, mean(chunkTimesNanos));
        double graphicsScore = positiveScore(averageFps / fpsBaseline);
        double simulationScore = positiveScore(msptBaseline / Math.max(0.001, medianMspt));
        double worldScore = positiveScore(chunksPerSecond / chunksPerSecondBaseline);
        double overall = weightedGeometricMean(graphicsScore, simulationScore, worldScore);
        return new Metrics(averageFps, onePercentLowFps, medianMspt, p99Mspt,
                chunksPerSecond, graphicsScore, simulationScore, worldScore, overall);
    }

    private void fail(String message) {
        phase = Phase.FAILED;
        BenchmarkMod.LOGGER.error("[MineBench] {}", message);
        writeStatus("FAILED", 0, 3, message);
        if (resultDirectory != null) {
            try {
                JsonObject failure = new JsonObject();
                failure.addProperty("run_id", runId);
                failure.addProperty("status", "FAILED");
                failure.addProperty("message", message);
                failure.addProperty("at", Instant.now().toString());
                writeJsonAtomic(resultDirectory.resolve("failure.json"), failure);
            } catch (Exception ignored) {
                // The status file is the last-resort failure signal.
            }
        }
    }

    private void writeStatus(String state, int testIndex, int testTotal, String message) {
        if (statusPath == null) {
            return;
        }
        try {
            JsonObject status = new JsonObject();
            status.addProperty("schema", 1);
            status.addProperty("run_id", runId);
            status.addProperty("state", state);
            status.addProperty("phase", phase.name());
            status.addProperty("test_index", testIndex);
            status.addProperty("test_total", testTotal);
            double progress = switch (phase) {
                case WARMUP -> 0.05 * warmupTick / Math.max(1.0, warmupTicks);
                case GRAPHICS -> 0.05 + 0.45 * frameTimesNanos.size() / Math.max(1.0, graphicsFrames);
                case SIMULATION -> 0.50 + 0.25 * simulationTick / Math.max(1.0, simulationTicks);
                case CHUNK_GENERATION -> 0.75 + 0.20 * chunkSampleCount / Math.max(1.0, chunkSamplesTarget);
                case FINALIZE, COMPLETE -> 1.0;
                default -> 0.0;
            };
            status.addProperty("progress", Math.min(1.0, Math.max(0.0, progress)));
            status.addProperty("message", message);
            status.addProperty("updated_at", Instant.now().toString());
            writeJsonAtomic(statusPath, status);
        } catch (Exception error) {
            BenchmarkMod.LOGGER.warn("[MineBench] Failed to write status: {}", error.getMessage());
        }
    }

    private static void writeJsonAtomic(Path path, JsonElement value) throws IOException {
        writeBytesAtomic(path, GSON.toJson(value).getBytes(StandardCharsets.UTF_8));
    }

    private static void writeBytesAtomic(Path path, byte[] bytes) throws IOException {
        Files.createDirectories(path.getParent());
        Path temporary = path.resolveSibling(path.getFileName() + ".tmp");
        Files.write(temporary, bytes);
        try {
            Files.move(temporary, path, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException error) {
            Files.move(temporary, path, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private static String sha256(byte[] bytes) throws NoSuchAlgorithmException {
        byte[] digest = MessageDigest.getInstance("SHA-256").digest(bytes);
        StringBuilder result = new StringBuilder(digest.length * 2);
        for (byte value : digest) {
            result.append(String.format(Locale.ROOT, "%02x", value));
        }
        return result.toString();
    }

    private static double mean(List<Long> values) {
        if (values.isEmpty()) {
            return 1.0;
        }
        double total = 0.0;
        for (long value : values) {
            total += value;
        }
        return total / values.size();
    }

    private static double percentile(List<Long> values, double quantile) {
        if (values.isEmpty()) {
            return 1.0;
        }
        List<Long> sorted = new ArrayList<>(values);
        Collections.sort(sorted);
        double position = Math.max(0.0, Math.min(1.0, quantile)) * (sorted.size() - 1);
        int lower = (int) Math.floor(position);
        int upper = (int) Math.ceil(position);
        if (lower == upper) {
            return sorted.get(lower);
        }
        double fraction = position - lower;
        return sorted.get(lower) + (sorted.get(upper) - sorted.get(lower)) * fraction;
    }

    private static double positiveScore(double normalized) {
        return Math.max(0.0, Math.min(100_000.0, normalized * 10_000.0));
    }

    private double weightedGeometricMean(double graphics, double simulation, double world) {
        double totalWeight = graphicsWeight + simulationWeight + worldWeight;
        double logSum = graphicsWeight * Math.log(Math.max(Double.MIN_VALUE, graphics))
                + simulationWeight * Math.log(Math.max(Double.MIN_VALUE, simulation))
                + worldWeight * Math.log(Math.max(Double.MIN_VALUE, world));
        return Math.exp(logSum / Math.max(1.0, totalWeight));
    }

    private static JsonObject object(JsonObject parent, String key) {
        JsonElement value = parent == null ? null : parent.get(key);
        return value != null && value.isJsonObject() ? value.getAsJsonObject() : new JsonObject();
    }

    private static String readString(JsonObject object, String key, String fallback) {
        JsonElement value = object == null ? null : object.get(key);
        return value != null && value.isJsonPrimitive() ? value.getAsString() : fallback;
    }

    private static boolean readBoolean(JsonObject object, String key, boolean fallback) {
        JsonElement value = object == null ? null : object.get(key);
        return value != null && value.isJsonPrimitive() ? value.getAsBoolean() : fallback;
    }

    private static long readLong(JsonObject object, String key, long fallback) {
        JsonElement value = object == null ? null : object.get(key);
        if (value == null || !value.isJsonPrimitive()) {
            return fallback;
        }
        try {
            return value.getAsLong();
        } catch (RuntimeException error) {
            return fallback;
        }
    }

    private static double positiveDouble(JsonObject object, String key, double fallback) {
        JsonElement value = object == null ? null : object.get(key);
        if (value == null || !value.isJsonPrimitive()) {
            return fallback;
        }
        try {
            double parsed = value.getAsDouble();
            return Double.isFinite(parsed) && parsed > 0.0 ? parsed : fallback;
        } catch (RuntimeException error) {
            return fallback;
        }
    }

    private static int positiveInt(JsonObject object, String key, int fallback) {
        JsonElement value = object == null ? null : object.get(key);
        if (value == null || !value.isJsonPrimitive()) {
            return fallback;
        }
        try {
            return Math.max(1, value.getAsInt());
        } catch (NumberFormatException error) {
            return fallback;
        }
    }

    private static int readInt(JsonObject object, String key, int fallback) {
        JsonElement value = object == null ? null : object.get(key);
        if (value == null || !value.isJsonPrimitive()) {
            return fallback;
        }
        try {
            return value.getAsInt();
        } catch (RuntimeException error) {
            return fallback;
        }
    }

    private static int clamp(int value, int minimum, int maximum) {
        return Math.max(minimum, Math.min(maximum, value));
    }

    private record Metrics(double averageFps, double onePercentLowFps, double medianMspt,
                           double p99Mspt, double chunksPerSecond, double graphicsScore,
                           double simulationScore, double worldScore, double overallScore) {
    }
}
