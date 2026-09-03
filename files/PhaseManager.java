package dev.bench.phase;

import dev.bench.BenchmarkMod;
import dev.bench.metrics.EntityTracker;
import dev.bench.metrics.FpsTracker;
import dev.bench.record.CameraRecorder;
import dev.bench.record.CameraReplay;
import dev.bench.result.BenchmarkResult;
import dev.bench.result.ResultScreen;
import net.minecraft.client.MinecraftClient;

/**
 * ベンチマークの状態機械
 * IDLE → RECORDING
 * IDLE → REPLAY_RUNNING → ENTITY_MEASURING → DONE
 */
public class PhaseManager {

    public enum Phase {
        IDLE,
        RECORDING,
        REPLAY_RUNNING,
        ENTITY_MEASURING,
        DONE
    }

    private Phase currentPhase = Phase.IDLE;

    private final CameraRecorder recorder = new CameraRecorder();
    private final CameraReplay replay = new CameraReplay();
    private final FpsTracker fpsTracker = new FpsTracker();
    private final EntityTracker entityTracker = new EntityTracker();

    // 現在の計測結果を保持
    private BenchmarkResult lastResult;

    // --- コマンドから呼ぶ ---

    public String startRecording(String name) {
        if (currentPhase != Phase.IDLE) {
            return "§c[Bench] Another phase is running: " + currentPhase;
        }
        recorder.startRecording(name);
        currentPhase = Phase.RECORDING;
        return "§a[Bench] Recording started. Move around, then /bench record stop";
    }

    public String stopRecording(MinecraftClient client) {
        if (currentPhase != Phase.RECORDING) {
            return "§c[Bench] Not recording.";
        }
        try {
            var path = recorder.stopRecording(client);
            currentPhase = Phase.IDLE;
            return "§a[Bench] Saved: " + path.getFileName() + " (" + recorder.getFrameCount() + " frames)";
        } catch (Exception e) {
            currentPhase = Phase.IDLE;
            return "§c[Bench] Failed to save: " + e.getMessage();
        }
    }

    /**
     * /bench play <scenario> [entityCount]
     */
    public String startPlay(MinecraftClient client, String scenarioName, int entityCount) {
        if (currentPhase != Phase.IDLE) {
            return "§c[Bench] Another phase is running: " + currentPhase;
        }
        if (!replay.load(client, scenarioName)) {
            return "§c[Bench] Scenario not found: " + scenarioName;
        }

        currentPhase = Phase.REPLAY_RUNNING;
        replay.start(fpsTracker, () -> {
            // リプレイ完了 → エンティティ計測へ
            BenchmarkMod.LOGGER.info("[MCBenchmark] Replay done. Starting entity phase...");
            currentPhase = Phase.ENTITY_MEASURING;
            entityTracker.start(client, entityCount, () -> {
                // エンティティ計測完了 → 結果画面へ
                currentPhase = Phase.DONE;
                lastResult = new BenchmarkResult(scenarioName, fpsTracker, entityTracker);
                client.execute(() -> client.setScreen(new ResultScreen(lastResult)));
            });
        });

        return "§a[Bench] Benchmark started! Scenario: " + scenarioName;
    }

    public String abortBenchmark() {
        currentPhase = Phase.IDLE;
        return "§e[Bench] Benchmark aborted.";
    }

    // --- tick ---

    public void tick(MinecraftClient client) {
        switch (currentPhase) {
            case RECORDING -> recorder.tick(client);
            case REPLAY_RUNNING -> replay.tick(client);
            case ENTITY_MEASURING -> entityTracker.tick(client);
            default -> {}
        }
    }

    // --- getter ---

    public Phase getCurrentPhase() { return currentPhase; }
    public CameraRecorder getRecorder() { return recorder; }
    public CameraReplay getReplay() { return replay; }
    public BenchmarkResult getLastResult() { return lastResult; }

    public String getStatusMessage() {
        return switch (currentPhase) {
            case IDLE -> "§7[Bench] Idle";
            case RECORDING -> "§e[Bench] Recording... " + recorder.getFrameCount() + " frames";
            case REPLAY_RUNNING -> String.format("§b[Bench] Replaying... %.0f%%", replay.getProgress() * 100);
            case ENTITY_MEASURING -> "§d[Bench] Entity phase... " + entityTracker.getMeasureTick() + "/" + EntityTracker.MEASURE_TICKS + " ticks";
            case DONE -> "§a[Bench] Done!";
        };
    }
}
