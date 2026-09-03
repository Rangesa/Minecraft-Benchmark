package dev.bench;

import dev.bench.command.BenchmarkCommand;
import dev.bench.phase.PhaseManager;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public class BenchmarkMod implements ClientModInitializer {

    public static final String MOD_ID = "mcbenchmark";
    public static final Logger LOGGER = LoggerFactory.getLogger(MOD_ID);

    public static final PhaseManager PHASE_MANAGER = new PhaseManager();

    @Override
    public void onInitializeClient() {
        LOGGER.info("[MCBenchmark] Initializing...");

        // コマンド登録
        BenchmarkCommand.register();

        // tick毎の更新
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            PHASE_MANAGER.tick(client);
        });

        LOGGER.info("[MCBenchmark] Ready. Use /bench to get started.");
    }
}
