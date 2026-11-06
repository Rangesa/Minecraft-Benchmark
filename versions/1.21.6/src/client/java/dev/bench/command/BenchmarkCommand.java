package dev.bench.command;

import dev.bench.BenchmarkMod;
import dev.bench.metrics.EntityTracker;
import net.fabricmc.fabric.api.client.command.v2.ClientCommandManager;
import net.fabricmc.fabric.api.client.command.v2.ClientCommandRegistrationCallback;
import com.mojang.brigadier.arguments.IntegerArgumentType;
import com.mojang.brigadier.arguments.StringArgumentType;
import net.minecraft.text.Text;

/**
 * /bench コマンド群
 *
 * /bench record start <name>   - 録画開始
 * /bench record stop           - 録画停止＆保存
 * /bench play <scenario> [count] - ベンチマーク開始
 * /bench status                - 現在フェーズ表示
 * /bench abort                 - 中断
 * /bench list                  - シナリオ一覧
 */
public class BenchmarkCommand {

    public static void register() {
        ClientCommandRegistrationCallback.EVENT.register((dispatcher, registryAccess) -> {
            dispatcher.register(
                ClientCommandManager.literal("bench")

                    // /bench record start <name>
                    // /bench record stop
                    .then(ClientCommandManager.literal("record")
                        .then(ClientCommandManager.literal("start")
                            .then(ClientCommandManager.argument("name", StringArgumentType.word())
                                .executes(ctx -> {
                                    String name = StringArgumentType.getString(ctx, "name");
                                    String msg = BenchmarkMod.PHASE_MANAGER.startRecording(name);
                                    ctx.getSource().sendFeedback(Text.literal(msg));
                                    return 1;
                                })
                            )
                            // 名前省略時はデフォルト名
                            .executes(ctx -> {
                                String msg = BenchmarkMod.PHASE_MANAGER.startRecording("default");
                                ctx.getSource().sendFeedback(Text.literal(msg));
                                return 1;
                            })
                        )
                        .then(ClientCommandManager.literal("stop")
                            .executes(ctx -> {
                                String msg = BenchmarkMod.PHASE_MANAGER.stopRecording(ctx.getSource().getClient());
                                ctx.getSource().sendFeedback(Text.literal(msg));
                                return 1;
                            })
                        )
                    )

                    // /bench play <scenario> [entityCount]
                    .then(ClientCommandManager.literal("play")
                        .then(ClientCommandManager.argument("scenario", StringArgumentType.word())
                            .then(ClientCommandManager.argument("entityCount", IntegerArgumentType.integer(1, 2000))
                                .executes(ctx -> {
                                    String scenario = StringArgumentType.getString(ctx, "scenario");
                                    int count = IntegerArgumentType.getInteger(ctx, "entityCount");
                                    String msg = BenchmarkMod.PHASE_MANAGER.startPlay(
                                            ctx.getSource().getClient(), scenario, count);
                                    ctx.getSource().sendFeedback(Text.literal(msg));
                                    return 1;
                                })
                            )
                            // entityCount省略時はデフォルト
                            .executes(ctx -> {
                                String scenario = StringArgumentType.getString(ctx, "scenario");
                                String msg = BenchmarkMod.PHASE_MANAGER.startPlay(
                                        ctx.getSource().getClient(), scenario, EntityTracker.DEFAULT_ENTITY_COUNT);
                                ctx.getSource().sendFeedback(Text.literal(msg));
                                return 1;
                            })
                        )
                    )

                    // /bench status
                    .then(ClientCommandManager.literal("status")
                        .executes(ctx -> {
                            ctx.getSource().sendFeedback(
                                Text.literal(BenchmarkMod.PHASE_MANAGER.getStatusMessage()));
                            return 1;
                        })
                    )

                    // /bench abort
                    .then(ClientCommandManager.literal("abort")
                        .executes(ctx -> {
                            ctx.getSource().sendFeedback(
                                Text.literal(BenchmarkMod.PHASE_MANAGER.abortBenchmark()));
                            return 1;
                        })
                    )

                    // /bench list  - シナリオファイル一覧
                    .then(ClientCommandManager.literal("list")
                        .executes(ctx -> {
                            var dir = dev.bench.record.CameraRecorder.getScenariosDir(
                                    ctx.getSource().getClient()).toFile();
                            if (!dir.exists() || dir.listFiles() == null) {
                                ctx.getSource().sendFeedback(Text.literal("§7[Bench] No scenarios found."));
                                return 1;
                            }
                            StringBuilder sb = new StringBuilder("§7[Bench] Scenarios:\n");
                            for (var f : dir.listFiles()) {
                                if (f.getName().endsWith(".json")) {
                                    sb.append("  §f").append(f.getName().replace(".json", "")).append("\n");
                                }
                            }
                            ctx.getSource().sendFeedback(Text.literal(sb.toString()));
                            return 1;
                        })
                    )
            );
        });
    }
}
