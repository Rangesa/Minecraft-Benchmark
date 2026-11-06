package dev.bench.metrics;

import dev.bench.BenchmarkMod;
import net.minecraft.client.Minecraft;
import net.minecraft.core.BlockPos;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.monster.zombie.Zombie;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * 指定数のモブをサーバーサイドでスポーンし、ServerTickEventsでtick処理時間を計測する。
 * シングルプレイヤー専用（client.getServer() が非null）。
 * 計測後はスポーンしたエンティティを全削除する。
 */
public class EntityTracker {

    public static final int DEFAULT_ENTITY_COUNT = 500;
    public static final int MEASURE_TICKS = 60; // 3秒分

    private volatile boolean running = false;
    private int measureTick = 0;
    private final List<Double> tickTimeSamples = new ArrayList<>();
    private final List<Integer> spawnedEntityIds = new ArrayList<>();
    private Runnable onComplete;
    private Minecraft client;

    /**
     * 計測開始。スポーンはサーバースレッドで実行される。
     */
    public void start(Minecraft client, int entityCount, Runnable onComplete) {
        this.client = client;
        this.onComplete = onComplete;
        this.tickTimeSamples.clear();
        this.spawnedEntityIds.clear();
        this.measureTick = 0;

        MinecraftServer server = client.getSingleplayerServer();
        if (server == null) {
            BenchmarkMod.LOGGER.warn("[MCBenchmark] EntityTracker: server is null (multiplayer not supported)");
            if (onComplete != null) onComplete.run();
            return;
        }
        if (client.player == null) {
            BenchmarkMod.LOGGER.warn("[MCBenchmark] EntityTracker: player is null");
            return;
        }

        BlockPos center = client.player.blockPosition();
        // サーバースレッドでスポーン
        server.execute(() -> {
            ServerLevel world = server.overworld();
            spawnEntities(world, center, entityCount);
            running = true;
            BenchmarkMod.LOGGER.info("[MCBenchmark] EntityTracker: spawned {} entities, measuring {} ticks...",
                    entityCount, MEASURE_TICKS);
        });
    }

    private void spawnEntities(ServerLevel world, BlockPos center, int count) {
        for (int i = 0; i < count; i++) {
            int dx = (int)(Math.random() * 20 - 10);
            int dz = (int)(Math.random() * 20 - 10);
            Zombie zombie = new Zombie(EntityType.ZOMBIE, world);
            zombie.setPosRaw(center.getX() + dx + 0.5, center.getY(), center.getZ() + dz + 0.5);
            zombie.setNoAi(false);
            world.addFreshEntity(zombie);
            spawnedEntityIds.add(zombie.getId());
        }
    }

    /**
     * PhaseManager経由でServerTickEvents.START/END_SERVER_TICKから呼ぶ。
     * サーバースレッドで実行される。
     * @param elapsedNanos 1サーバーtickの経過時間（nanoseconds）
     */
    public void recordServerTick(long elapsedNanos) {
        if (!running) return;

        tickTimeSamples.add(elapsedNanos / 1_000_000.0);
        measureTick++;

        if (measureTick >= MEASURE_TICKS) {
            running = false;
            MinecraftServer server = client.getSingleplayerServer();
            if (server != null) {
                server.execute(() -> {
                    cleanup(server);
                    // コールバックはクライアントスレッドで呼ぶ（ResultScreen表示のため）
                    client.execute(() -> {
                        if (onComplete != null) onComplete.run();
                    });
                });
            } else {
                client.execute(() -> {
                    if (onComplete != null) onComplete.run();
                });
            }
        }
    }

    private void cleanup(MinecraftServer server) {
        ServerLevel world = server.overworld();
        spawnedEntityIds.forEach(id -> {
            var entity = world.getEntity(id);
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
