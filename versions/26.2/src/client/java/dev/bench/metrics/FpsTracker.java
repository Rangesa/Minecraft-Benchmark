package dev.bench.metrics;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import net.minecraft.client.Minecraft;

/**
 * リプレイ再生中のFPSをサンプリングして統計を出す
 * currentFpsはMinecraftClientから取得（Minecraft内部の1秒平均FPS）
 */
public class FpsTracker {

    private final List<Integer> fpsSamples = new ArrayList<>();
    private boolean running = false;
    private long startTimeMs;

    public void start() {
        fpsSamples.clear();
        running = true;
        startTimeMs = System.currentTimeMillis();
    }

    public void stop() {
        running = false;
    }

    /**
     * 毎tick呼ぶ。Minecraftの内部FPSカウンタを読む。
     */
    public void sampleCurrentFps(Minecraft client) {
        if (!running) return;
        int fps = client.getFps();
        if (fps > 0) {
            fpsSamples.add(fps);
        }
    }

    // --- 統計 ---

    public double getAvgFps() {
        if (fpsSamples.isEmpty()) return 0;
        return fpsSamples.stream().mapToInt(i -> i).average().orElse(0);
    }

    public int getMinFps() {
        return fpsSamples.stream().mapToInt(i -> i).min().orElse(0);
    }

    public int getMaxFps() {
        return fpsSamples.stream().mapToInt(i -> i).max().orElse(0);
    }

    /**
     * 1% Low FPS (下位1%の平均) - フレームドロップの指標として重要
     */
    public double get1PercentLow() {
        if (fpsSamples.isEmpty()) return 0;
        List<Integer> sorted = new ArrayList<>(fpsSamples);
        Collections.sort(sorted);
        int count = Math.max(1, (int) (sorted.size() * 0.01));
        return sorted.subList(0, count).stream().mapToInt(i -> i).average().orElse(0);
    }

    /**
     * 平均frametimeをmsで返す
     */
    public double getAvgFrametimeMs() {
        double avg = getAvgFps();
        return avg > 0 ? 1000.0 / avg : 0;
    }

    public int getSampleCount() { return fpsSamples.size(); }

    public long getDurationMs() {
        return System.currentTimeMillis() - startTimeMs;
    }

    public List<Integer> getAllSamples() { return Collections.unmodifiableList(fpsSamples); }
}
