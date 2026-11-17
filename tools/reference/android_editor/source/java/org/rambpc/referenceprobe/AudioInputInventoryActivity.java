package org.rambpc.referenceprobe;

import android.app.Activity;
import android.content.Context;
import android.media.AudioDeviceInfo;
import android.media.AudioManager;
import android.os.Build;
import android.os.Bundle;
import android.util.AtomicFile;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.UUID;

/** Original passive dev fixture: no UI, editor access, recording or routing changes. */
public final class AudioInputInventoryActivity extends Activity {
    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved);
        try {
            AudioInputInventory result = new AudioInputInventory(
                UUID.randomUUID(), System.currentTimeMillis());
            if (Build.VERSION.SDK_INT < 23) {
                result.fail(AudioInputInventory.Failure.UNSUPPORTED_API);
            } else {
                try {
                    AudioManager manager = (AudioManager) getSystemService(Context.AUDIO_SERVICE);
                    if (manager == null) {
                        result.fail(AudioInputInventory.Failure.AUDIO_MANAGER_UNAVAILABLE);
                    } else {
                        for (AudioDeviceInfo device : manager.getDevices(AudioManager.GET_DEVICES_INPUTS)) {
                            // Do not read product names, addresses, IDs, or any identifying metadata.
                            result.add(device.getType(), device.isSource());
                        }
                    }
                } catch (RuntimeException error) {
                    // Fixed code only: exception text may contain device-specific data.
                    result.fail(AudioInputInventory.Failure.DEVICE_QUERY_FAILED);
                }
            }
            writePrivate(result.toJson());
        } catch (IOException | RuntimeException error) {
            // No logs or fallback paths. An unwritable file cannot report its own failure;
            // AtomicFile retains the previous result, which must be rejected as stale.
        } finally {
            // Required for Theme.NoDisplay; no view/IME is created or editor intent delivered.
            finish();
        }
    }

    private void writePrivate(String json) throws IOException {
        AtomicFile file = new AtomicFile(new File(getFilesDir(), "audio-input-inventory.json"));
        FileOutputStream stream = null;
        try {
            stream = file.startWrite();
            stream.write(json.getBytes(StandardCharsets.UTF_8));
            file.finishWrite(stream);
        } catch (IOException | RuntimeException error) {
            if (stream != null) file.failWrite(stream);
            throw error;
        }
    }
}
