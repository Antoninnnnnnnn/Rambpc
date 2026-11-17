package org.rambpc.referenceprobe;

import java.util.Map;
import java.util.TreeMap;
import java.util.UUID;

/** Pure reducer/JSON encoder; retains only public numeric types and source flags. */
public final class AudioInputInventory {
    public enum Failure { UNSUPPORTED_API, AUDIO_MANAGER_UNAVAILABLE, DEVICE_QUERY_FAILED }

    private final UUID runId;
    private final long timeUnixMs;
    private final TreeMap<Integer, int[]> counts = new TreeMap<>();
    private int deviceCount;
    private Failure failure;

    public AudioInputInventory(UUID runId, long timeUnixMs) {
        if (runId == null) throw new IllegalArgumentException("runId required");
        this.runId = runId;
        this.timeUnixMs = timeUnixMs;
    }

    public void add(int type, boolean isSource) {
        if (failure != null) throw new IllegalStateException("Failed inventory is final");
        int[] pair = counts.get(type);
        if (pair == null) {
            pair = new int[2];
            counts.put(type, pair);
        }
        pair[isSource ? 1 : 0]++;
        deviceCount++;
    }

    public void fail(Failure reason) {
        if (reason == null) throw new IllegalArgumentException("failure required");
        // Never publish partial counts as a complete enumeration.
        counts.clear();
        deviceCount = 0;
        failure = reason;
    }

    public boolean succeeded() { return failure == null; }

    public String toJson() {
        // All strings are generated UUIDs or fixed enums; no arbitrary text needs escaping.
        StringBuilder json = new StringBuilder("{\"schema_version\":1,\"run_id\":\"")
            .append(runId).append("\",\"snapshot_time_unix_ms\":").append(timeUnixMs)
            .append(",\"success\":").append(succeeded()).append(",\"failure\":");
        if (failure == null) json.append("null");
        else json.append('"').append(failure.name()).append('"');
        json.append(",\"recording_started\":false,\"routing_proven\":false,\"device_count\":")
            .append(deviceCount).append(",\"type_counts\":[");
        boolean first = true;
        for (Map.Entry<Integer, int[]> entry : counts.entrySet()) {
            for (int source = 0; source <= 1; source++) {
                int count = entry.getValue()[source];
                if (count == 0) continue;
                if (!first) json.append(',');
                first = false;
                json.append("{\"type\":").append(entry.getKey())
                    .append(",\"is_source\":").append(source == 1)
                    .append(",\"count\":").append(count).append('}');
            }
        }
        return json.append("]}\n").toString();
    }
}
