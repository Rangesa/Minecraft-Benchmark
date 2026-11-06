package dev.bench.mixin.client;

import dev.bench.BenchmarkMod;
import net.minecraft.client.DeltaTracker;
import net.minecraft.client.renderer.GameRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** Captures actual render-call durations, not Minecraft's averaged FPS counter. */
@Mixin(GameRenderer.class)
public abstract class GameRendererMixin {
    @Inject(method = "render", at = @At("HEAD"))
    private void minebench$frameStart(DeltaTracker tickCounter, boolean tick, CallbackInfo callbackInfo) {
        BenchmarkMod.SESSION.onFrameStart();
    }

    @Inject(method = "render", at = @At("RETURN"))
    private void minebench$frameEnd(DeltaTracker tickCounter, boolean tick, CallbackInfo callbackInfo) {
        BenchmarkMod.SESSION.onFrameEnd();
    }
}
