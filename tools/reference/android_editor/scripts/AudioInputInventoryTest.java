import java.util.UUID;
import org.rambpc.referenceprobe.AudioInputInventory;

/** Pure host tests only; no Android stubs or claims about actual device behavior. */
public final class AudioInputInventoryTest {
    private static final UUID RUN = UUID.fromString("12345678-1234-4234-8234-123456789abc");
    private static final String PREFIX = "{\"schema_version\":1,\"run_id\":\"" + RUN
        + "\",\"snapshot_time_unix_ms\":1700000000123,";
    private static final String FLAGS = ",\"recording_started\":false,\"routing_proven\":false,";

    private static AudioInputInventory fresh() {
        return new AudioInputInventory(RUN, 1700000000123L);
    }

    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }

    public static void main(String[] args) {
        AudioInputInventory empty = fresh();
        check(empty.succeeded(), "empty enumeration is successful, not routing proof");
        check(empty.toJson().equals(PREFIX + "\"success\":true,\"failure\":null" + FLAGS
            + "\"device_count\":0,\"type_counts\":[]}\n"), "exact empty schema/JSON");

        AudioInputInventory counts = fresh();
        counts.add(15, true);
        counts.add(3, false);
        counts.add(15, true);
        counts.add(3, true);
        counts.add(0, true); // Unknown/future numeric types are not relabeled or dropped.
        String expected = PREFIX + "\"success\":true,\"failure\":null" + FLAGS
            + "\"device_count\":5,\"type_counts\":["
            + "{\"type\":0,\"is_source\":true,\"count\":1},"
            + "{\"type\":3,\"is_source\":false,\"count\":1},"
            + "{\"type\":3,\"is_source\":true,\"count\":1},"
            + "{\"type\":15,\"is_source\":true,\"count\":2}]}\n";
        check(counts.toJson().equals(expected), "sorted grouped counts and separate source flags");
        check(counts.toJson().equals(counts.toJson()), "serialization does not mutate counts");

        AudioInputInventory boundaries = fresh();
        boundaries.add(Integer.MAX_VALUE, false);
        boundaries.add(Integer.MIN_VALUE, true);
        check(boundaries.toJson().endsWith("\"device_count\":2,\"type_counts\":["
            + "{\"type\":-2147483648,\"is_source\":true,\"count\":1},"
            + "{\"type\":2147483647,\"is_source\":false,\"count\":1}]}\n"),
            "numeric types stay numeric without a fixed platform type list");

        for (AudioInputInventory.Failure failure : AudioInputInventory.Failure.values()) {
            AudioInputInventory failed = fresh();
            failed.add(15, true);
            failed.fail(failure);
            check(!failed.succeeded(), "failure status");
            check(failed.toJson().equals(PREFIX + "\"success\":false,\"failure\":\""
                + failure.name() + "\"" + FLAGS + "\"device_count\":0,\"type_counts\":[]}\n"),
                "fixed failure code; discard partial data");
            try {
                failed.add(3, true);
                throw new AssertionError("failed inventory accepted more data");
            } catch (IllegalStateException expectedFailure) { }
        }
        try {
            new AudioInputInventory(null, 0);
            throw new AssertionError("null run ID accepted");
        } catch (IllegalArgumentException expectedFailure) { }
        try {
            fresh().fail(null);
            throw new AssertionError("null failure accepted");
        } catch (IllegalArgumentException expectedFailure) { }
        System.out.println("AudioInputInventory host tests passed; no Android/device behavior tested.");
    }
}
