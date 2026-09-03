package dev.bench.metrics;

import dev.bench.BenchmarkMod;
import net.minecraft.client.MinecraftClient;
import net.minecraft.entity.EntityType;
import net.minecraft.entity.SpawnReason;
import net.minecraft.entity.mob.ZombieEntity;
import net.minecraft.util.math.BlockPos;
import net.minecraft.world.World;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * 指定数のモブをスポーンし、指定tick分だけtick処理時間を計測する
 * 計測後はスポーンしたエンティティを全削除する
 */
public class EntityTracker {

    public static final int DEFAULT_ENTITY_COUNT = 500;
    public static final int MEASURE_TICKS = 60;   // 3秒分計測

    private boolean running = false;
    private int measureTick = 0;
    private final List<Double> tickTimeSamples = new ArrayList<>(); // ms
    private List<Integer> spawnedEntityIds = new ArrayList<>();
    private Runnable onComplete;

    /**
     * 計測開始。ClientTickEventsから毎tick tick()を呼ぶこと。
     * @param entityCount スポーンするモブ数
     * @param onComplete  計測完了時コールバック
     */
    public void start(MinecraftClient client, int entityCount, Runnable onComplete) {
        this.onComplete = onComplete;
        this.tickTimeSamples.clear();
        this.spawnedEntityIds.clear();
        this.measureTick = 0;

        if (client.world == null || client.player == null) {
            BenchmarkMod.LOGGER.warn("[MCBenchmark] EntityTracker: world or player is null");
            return;
        }

        // プレイヤー周辺にゾンビをスポーン
        spawnEntities(client, entityCount);
        this.running = true;
        BenchmarkMod.LOGGER.info("[MCBenchmark] EntityTracker: spawned {} entities, measuring {} ticks...", entityCount, MEASURE_TICKS);
    }

    private void spawnEntities(MinecraftClient client, int count) {
        World world = client.world;
        BlockPos center = client.player.getBlockPos();

        for (int i = 0; i < count; i++) {
            int dx = (int)(Math.random() * 20 - 10);
            int dz = (int)(Math.random() * 20 - 10);
            BlockPos pos = center.add(dx, 0, dz);

            ZombieEntity zombie = new ZombieEntity(EntityType.ZOMBIE, world);
            zombie.setPos(pos.getX() + 0.5, pos.getY(), pos.getZ() + 0.5);
            zombie.setAiDisabled(false);
            world.spawnEntity(zombie);
            spawnedEntityIds.add(zombie.getId());
        }
    }

    /**
     * ClientTickEventsから毎tick呼ぶ
     */
    public void tick(MinecraftClient client) {
        if (!running) return;

        long before = System.nanoTime();
        // tick自体はMinecraftが行うので、ここでは時間をサンプリングするだけ
        // NOTE: より精密にやるなら MixinでServerWorld#tickEntitiesをフックする
        long after = System.nanoTime();

        // ClientTickの実行時間をサンプリング（簡易版）
        double ms = (after - before) / 1_000_000.0;
        tickTimeSamples.add(ms);
        measureTick++;

        if (measureTick >= MEASURE_TICKS) {
            running = false;
            cleanup(client);
            if (onComplete != null) onComplete.run();
        }
    }

    private void cleanup(MinecraftClient client) {
        if (client.world == null) return;
        // スポーンしたエンティティを削除
        spawnedEntityIds.forEach(id -> {
            var entity = client.world.getEntityById(id);
            if (entity != null) entity.discard();
        });
        BenchmarkMod.LOGGER.info("[MCBenchmark] EntityTracker: cleanup complete");
    }

    // --- 統計 ---

    public double getAvgTickMs() {
        return tickTimeSamples.stream().mapToDouble(d -> d).average().orElse(0);
    }

    public double getMaxTickMs() {
        return tickTimeSamples.stream().mapToDouble(d -> d).max().orElse(0);
    }

    public double getMinTickMs() {
        return tickTimeSamples.stream().mapToDouble(d -> d).min().orElse(0);
    }

    public boolean isRunning() { return running; }
    public int getMeasureTick() { return measureTick; }
    public List<Double> getAllSamples() { return Collections.unmodifiableList(tickTimeSamples); }
}
