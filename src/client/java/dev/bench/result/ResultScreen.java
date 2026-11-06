package dev.bench.result;

import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.gui.screen.Screen;
import net.minecraft.client.gui.widget.ButtonWidget;
import net.minecraft.text.Text;

/**
 * FFベンチマーク風の結果表示画面
 * 数値のみシンプルに表示
 */
public class ResultScreen extends Screen {

    private final BenchmarkResult result;

    public ResultScreen(BenchmarkResult result) {
        super(Text.literal("Benchmark Result"));
        this.result = result;
    }

    @Override
    protected void init() {
        // 閉じるボタン
        addDrawableChild(ButtonWidget.builder(
                Text.literal("Close"),
                btn -> this.close()
        ).dimensions(this.width / 2 - 50, this.height - 40, 100, 20).build());

        // 結果をコピーするボタン
        addDrawableChild(ButtonWidget.builder(
                Text.literal("Copy to Clipboard"),
                btn -> {
                    assert this.client != null;
                    this.client.keyboard.setClipboard(buildCopyText());
                }
        ).dimensions(this.width / 2 + 55, this.height - 40, 120, 20).build());
    }

    @Override
    public void render(DrawContext context, int mouseX, int mouseY, float delta) {
        // 背景
        renderBackground(context, mouseX, mouseY, delta);

        int cx = this.width / 2;
        int y = 30;

        // タイトル
        context.drawCenteredTextWithShadow(textRenderer,
                Text.literal("§6§lMC Benchmark Result"), cx, y, 0xFFFFFFFF);
        y += 18;

        // シナリオ名 / 日時
        context.drawCenteredTextWithShadow(textRenderer,
                Text.literal("§7Scenario: §f" + result.scenarioName), cx, y, 0xFFFFFFFF);
        y += 12;
        context.drawCenteredTextWithShadow(textRenderer,
                Text.literal("§7" + result.timestamp), cx, y, 0xFFAAAAAA);
        y += 25;

        // ---- FPS セクション ----
        context.drawCenteredTextWithShadow(textRenderer,
                Text.literal("§b--- Rendering ---"), cx, y, 0xFFFFFFFF);
        y += 14;

        drawStat(context, cx, y, "Avg FPS", String.format("§f%.1f fps", result.avgFps));
        y += 14;
        drawStat(context, cx, y, "Avg Frametime", String.format("§f%.2f ms", result.avgFrametimeMs));
        y += 14;
        drawStat(context, cx, y, "1% Low FPS", String.format("§f%.1f fps", result.fps1PercentLow));
        y += 14;
        drawStat(context, cx, y, "Min / Max FPS", String.format("§f%d / %d fps", result.minFps, result.maxFps));
        y += 14;
        drawStat(context, cx, y, "Samples", String.format("§7%d ticks", result.fpsSampleCount));
        y += 22;

        // ---- Entity セクション ----
        context.drawCenteredTextWithShadow(textRenderer,
                Text.literal("§d--- Entity Tick ---"), cx, y, 0xFFFFFFFF);
        y += 14;

        drawStat(context, cx, y, "Avg Tick Time", String.format("§f%.3f ms", result.avgEntityTickMs));
        y += 14;
        drawStat(context, cx, y, "Max Tick Time", String.format("§f%.3f ms", result.maxEntityTickMs));
        y += 22;

        // FPS評価コメント
        context.drawCenteredTextWithShadow(textRenderer,
                Text.literal(getComment(result.avgFps)), cx, y, 0xFFFFFFFF);

        super.render(context, mouseX, mouseY, delta);
    }

    private void drawStat(DrawContext context, int cx, int y, String label, String value) {
        context.drawTextWithShadow(textRenderer,
                Text.literal("§7" + label + ":"), cx - 130, y, 0xFFFFFFFF);
        context.drawTextWithShadow(textRenderer,
                Text.literal(value), cx + 10, y, 0xFFFFFFFF);
    }

    private String getComment(double avgFps) {
        if (avgFps >= 120) return "§a§lExcellent! ─ 快適な環境です";
        if (avgFps >= 60)  return "§2Good ─ 十分なパフォーマンスです";
        if (avgFps >= 30)  return "§ePlayable ─ 軽量化MODを試してみては?";
        return "§cPoor ─ 最適化の余地が大きいです";
    }

    private String buildCopyText() {
        return String.format(
                "=== MC Benchmark Result ===\n" +
                "Scenario: %s\n" +
                "Date: %s\n" +
                "--- Rendering ---\n" +
                "Avg FPS:       %.1f fps\n" +
                "Avg Frametime: %.2f ms\n" +
                "1%% Low FPS:    %.1f fps\n" +
                "Min/Max FPS:   %d / %d fps\n" +
                "--- Entity Tick ---\n" +
                "Avg Tick:      %.3f ms\n" +
                "Max Tick:      %.3f ms\n",
                result.scenarioName, result.timestamp,
                result.avgFps, result.avgFrametimeMs, result.fps1PercentLow,
                result.minFps, result.maxFps,
                result.avgEntityTickMs, result.maxEntityTickMs
        );
    }

    @Override
    public boolean shouldPause() {
        return false; // ゲームを止めない
    }
}
