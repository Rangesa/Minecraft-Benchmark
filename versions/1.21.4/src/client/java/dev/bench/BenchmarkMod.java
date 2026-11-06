package dev.bench;

import dev.bench.command.BenchmarkCommand;
import dev.bench.phase.PhaseManager;
import dev.bench.runtime.BenchmarkSession;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public class BenchmarkMod implements ClientModInitializer {

    public static final String MOD_ID = "mcbenchmark";
    public static final Logger LOGGER = LoggerFactory.getLogger(MOD_ID);

    public static final PhaseManager PHASE_MANAGER = new PhaseManager();
    public static final BenchmarkSession SESSION = new BenchmarkSession();

    @Override
    public void onInitializeClient() {
        LOGGER.info("[MCBenchmark] Initializing...");

        BenchmarkCommand.register();

        // クライアントtick: 録画・リプレイ
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            PHASE_MANAGER.tick(client);
            SESSION.tick(client);
        });

        // サーバーtick: エンティティ計測用の時間計測
        ServerTickEvents.START_SERVER_TICK.register(server -> {
            PHASE_MANAGER.onServerTickStart();
            SESSION.onServerTickStart(server);
        });
        ServerTickEvents.END_SERVER_TICK.register(server -> {
            PHASE_MANAGER.onServerTickEnd();
            SESSION.onServerTickEnd(server);
        });

        LOGGER.info("[MCBenchmark] Ready. Production runs are driven by minebench/run.json.");
    }
}
