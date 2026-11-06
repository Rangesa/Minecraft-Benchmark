package dev.bench.record;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import dev.bench.BenchmarkMod;
import net.minecraft.client.MinecraftClient;
import net.minecraft.client.network.ClientPlayerEntity;

import java.io.FileWriter;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/**
 * /bench record start|stop
 * tickごとにプレイヤー位置・視点を記録し、JSONファイルに書き出す
 */
public class CameraRecorder {

    private boolean recording = false;
    private long tickIndex = 0;
    private final List<CameraFrame> frames = new ArrayList<>();
    private String scenarioName = "default";

    public void startRecording(String name) {
        this.scenarioName = name;
        this.frames.clear();
        this.tickIndex = 0;
        this.recording = true;
        BenchmarkMod.LOGGER.info("[MCBenchmark] Recording started: {}", name);
    }

    public Path stopRecording(MinecraftClient client) throws IOException {
        this.recording = false;
        BenchmarkMod.LOGGER.info("[MCBenchmark] Recording stopped. {} frames captured.", frames.size());
        return saveToFile(client);
    }

    /**
     * ClientTickEventsから毎tick呼ぶ
     */
    public void tick(MinecraftClient client) {
        if (!recording) return;
        ClientPlayerEntity player = client.player;
        if (player == null) return;

        frames.add(new CameraFrame(
                player.getX(),
                player.getY(),
                player.getZ(),
                player.getYaw(),
                player.getPitch(),
                tickIndex++
        ));
    }

    public boolean isRecording() { return recording; }
    public int getFrameCount() { return frames.size(); }

    private Path saveToFile(MinecraftClient client) throws IOException {
        Path dir = getScenariosDir(client);
        Files.createDirectories(dir);
        Path file = dir.resolve(scenarioName + ".json");

        Gson gson = new GsonBuilder().setPrettyPrinting().create();
        ScenarioData data = new ScenarioData(scenarioName, frames);

        try (FileWriter writer = new FileWriter(file.toFile())) {
            gson.toJson(data, writer);
        }

        BenchmarkMod.LOGGER.info("[MCBenchmark] Saved to: {}", file);
        return file;
    }

    public static Path getScenariosDir(MinecraftClient client) {
        return client.runDirectory.toPath().resolve("mcbenchmark").resolve("scenarios");
    }

    // JSON用データクラス
    public static class ScenarioData {
        public String name;
        public int frameCount;
        public List<CameraFrame> frames;

        public ScenarioData(String name, List<CameraFrame> frames) {
            this.name = name;
            this.frameCount = frames.size();
            this.frames = frames;
        }
    }
}
