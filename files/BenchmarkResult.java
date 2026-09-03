package dev.bench.result;

import dev.bench.metrics.EntityTracker;
import dev.bench.metrics.FpsTracker;

import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;

/**
 * ベンチマーク1回分の結果をまとめるデータクラス
 */
public class BenchmarkResult {

    public final String scenarioName;
    public final String timestamp;

    // FPS計測結果
    public final double avgFps;
    public final int minFps;
    public final int maxFps;
    public final double fps1PercentLow;
    public final double avgFrametimeMs;
    public final int fpsSampleCount;

    // エンティティ計測結果
    public final double avgEntityTickMs;
    public final double maxEntityTickMs;

    public BenchmarkResult(String scenarioName, FpsTracker fps, EntityTracker entity) {
        this.scenarioName = scenarioName;
        this.timestamp = LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss"));

        this.avgFps = fps.getAvgFps();
        this.minFps = fps.getMinFps();
        this.maxFps = fps.getMaxFps();
        this.fps1PercentLow = fps.get1PercentLow();
        this.avgFrametimeMs = fps.getAvgFrametimeMs();
        this.fpsSampleCount = fps.getSampleCount();

        this.avgEntityTickMs = entity.getAvgTickMs();
        this.maxEntityTickMs = entity.getMaxTickMs();
    }
}
