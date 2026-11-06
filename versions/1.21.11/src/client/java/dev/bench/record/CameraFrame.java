package dev.bench.record;

/**
 * 1tickごとのカメラ状態スナップショット
 */
public class CameraFrame {
    public final double x, y, z;
    public final float yaw, pitch;
    public final long tickIndex;

    public CameraFrame(double x, double y, double z, float yaw, float pitch, long tickIndex) {
        this.x = x;
        this.y = y;
        this.z = z;
        this.yaw = yaw;
        this.pitch = pitch;
        this.tickIndex = tickIndex;
    }
}
