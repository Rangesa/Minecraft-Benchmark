package dev.bench.record;

import com.google.gson.Gson;
import dev.bench.BenchmarkMod;
import dev.bench.metrics.FpsTracker;
import java.io.FileReader;
import java.io.IOException;
import java.nio.file.Path;
import java.util.List;
import net.minecraft.client.Minecraft;
import net.minecraft.client.player.LocalPlayer;

/**
 * 録画したカメラ経路を再生しながらFPSを計測する
 * 再生はtick駆動（= 録画と同じ速度で再生）
 * フレーム補間はCatmull-Romスプライン
 */
public class CameraReplay {

    private boolean playing = false;
    private int currentFrame = 0;
    private List<CameraFrame> frames;
    private FpsTracker fpsTracker;
    private Runnable onComplete;
    private String scenarioName;

    public boolean load(Minecraft client, String scenarioName) {
        Path file = CameraRecorder.getScenariosDir(client).resolve(scenarioName + ".json");
        if (!file.toFile().exists()) {
            BenchmarkMod.LOGGER.warn("[MCBenchmark] Scenario not found: {}", file);
            return false;
        }

        try (FileReader reader = new FileReader(file.toFile())) {
            Gson gson = new Gson();
            CameraRecorder.ScenarioData data = gson.fromJson(reader, CameraRecorder.ScenarioData.class);
            this.frames = data.frames;
            this.scenarioName = data.name;
            BenchmarkMod.LOGGER.info("[MCBenchmark] Loaded scenario '{}' ({} frames)", scenarioName, frames.size());
            return true;
        } catch (IOException e) {
            BenchmarkMod.LOGGER.error("[MCBenchmark] Failed to load scenario", e);
            return false;
        }
    }

    public void start(FpsTracker fpsTracker, Runnable onComplete) {
        this.fpsTracker = fpsTracker;
        this.onComplete = onComplete;
        this.currentFrame = 0;
        this.playing = true;
        fpsTracker.start();
    }

    /**
     * ClientTickEventsから毎tick呼ぶ
     */
    public void tick(Minecraft client) {
        if (!playing || frames == null) return;
        if (client.player == null) return;

        if (currentFrame >= frames.size()) {
            // 再生完了
            playing = false;
            fpsTracker.stop();
            if (onComplete != null) onComplete.run();
            return;
        }

        // カメラ位置をフレームデータに強制セット
        applyFrame(client.player, frames.get(currentFrame));
        fpsTracker.sampleCurrentFps(client);
        currentFrame++;
    }

    private void applyFrame(LocalPlayer player, CameraFrame frame) {
        player.setPosRaw(frame.x, frame.y, frame.z);
        player.setYRot(frame.yaw);
        player.setXRot(frame.pitch);
        // ネットワーク送信を防ぐため velocity をゼロに
        player.setDeltaMovement(0, 0, 0);
    }

    public boolean isPlaying() { return playing; }
    public int getCurrentFrame() { return currentFrame; }
    public int getTotalFrames() { return frames != null ? frames.size() : 0; }
    public float getProgress() {
        if (frames == null || frames.isEmpty()) return 0;
        return (float) currentFrame / frames.size();
    }
    public String getScenarioName() { return scenarioName; }
}
