package org.rambpc.referenceprobe;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;
import android.text.Editable;
import android.text.InputFilter;
import android.text.InputType;
import android.text.TextWatcher;
import android.util.AtomicFile;
import android.view.WindowManager;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.TextView;
import org.rambpc.publicvoiceprobe.R;
import org.json.JSONObject;
import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.util.UUID;

/** Public-phrase editor: no messaging, networking, microphone or external-data code. */
public final class PublicVoiceReferenceActivity extends Activity {
    private EditText editor;
    private TextView details;
    private TextView status;
    private CaseCatalog testCase;
    private String runId;
    private int sendCallbackCount;
    private int editorActionCallbackCount;
    private int lastActionId = -1;
    private long snapshotSequence;
    private boolean suppressTextSnapshots;
    private final Runnable textSnapshot = () -> {
        if (!suppressTextSnapshots) saveSnapshot("text_changed", false);
    };

    @Override public void onCreate(Bundle saved) {
        setTheme(android.R.style.Theme_Material_Light_NoActionBar);
        super.onCreate(saved);
        // Window-scoped only; Android ignores this flag while the activity is in the background.
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE
            | WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        setContentView(R.layout.main);
        editor = findViewById(R.id.editor);
        details = findViewById(R.id.case_details);
        status = findViewById(R.id.status);
        editor.setFilters(new InputFilter[] { new InputFilter.LengthFilter(CaseCatalog.MAX_TEXT_UTF16) });
        editor.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
        editor.setHorizontallyScrolling(false);
        // Public phrases only. Omitting NO_PERSONALIZED_LEARNING is not a provider-learning
        // guarantee, a diagnosis of the old grey microphone, or a third-party privacy bypass.
        editor.setImeOptions(EditorInfo.IME_ACTION_SEND | EditorInfo.IME_FLAG_NO_EXTRACT_UI);
        editor.setOnEditorActionListener((view, actionId, event) -> {
            editorActionCallbackCount++;
            lastActionId = actionId;
            if (actionId == EditorInfo.IME_ACTION_SEND) sendCallbackCount++;
            // Consuming SEND never starts another activity, sends anything or clears the text.
            saveSnapshot("ime_callback", false);
            return actionId == EditorInfo.IME_ACTION_SEND;
        });
        bindCase(R.id.case_one, 1);
        bindCase(R.id.case_two, 2);
        bindCase(R.id.case_three, 3);
        ((Button) findViewById(R.id.reset)).setOnClickListener(v -> resetCase(testCase.id));
        ((Button) findViewById(R.id.capture)).setOnClickListener(v -> saveSnapshot("explicit_capture", true));
        if (saved != null && CaseCatalog.isValid(saved.getInt("case_id", -1))) {
            testCase = CaseCatalog.get(saved.getInt("case_id"));
            runId = saved.getString("run_id", UUID.randomUUID().toString());
            sendCallbackCount = saved.getInt("send_count");
            editorActionCallbackCount = saved.getInt("action_count");
            lastActionId = saved.getInt("last_action", -1);
            snapshotSequence = saved.getLong("sequence");
            String text = saved.getString("text", testCase.seed);
            editor.setText(text);
            int length = editor.length();
            editor.setSelection(clamp(saved.getInt("start"), length), clamp(saved.getInt("end"), length));
            showDetails();
            updateStatus("Own editor restored");
        } else {
            int requested = requestedCase(getIntent());
            if (!CaseCatalog.isValid(requested)) {
                finish();
                return;
            }
            resetCase(requested);
        }
        // Observe only this activity's editor, after reset/restoration has fully completed.
        editor.addTextChangedListener(new TextWatcher() {
            @Override public void beforeTextChanged(CharSequence text, int start, int count, int after) { }
            @Override public void onTextChanged(CharSequence text, int start, int before, int count) { }
            @Override public void afterTextChanged(Editable text) {
                if (suppressTextSnapshots) return;
                editor.removeCallbacks(textSnapshot);
                // Defer until the current IME edit finishes updating selection/composition spans.
                editor.post(textSnapshot);
            }
        });
        editor.requestFocus();
        editor.post(() -> {
            // Normal keyboard request only; microphone activation remains a manual user action.
            InputMethodManager input = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
            if (input != null && editor.hasFocus()) {
                input.showSoftInput(editor, InputMethodManager.SHOW_IMPLICIT);
            }
        });
    }

    private int requestedCase(Intent intent) {
        if (intent == null || !intent.hasExtra("case_id")) return 1;
        // getIntExtra accepts only the integer form; strings or out-of-range IDs fail closed.
        return intent.getIntExtra("case_id", -1);
    }

    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        // A plain relaunch must not discard the current controlled editor state.
        // Only an explicitly supplied, valid integer case_id requests a reset.
        if (intent == null || !intent.hasExtra("case_id")) return;
        int requested = requestedCase(intent);
        if (!CaseCatalog.isValid(requested)) {
            updateStatus("Rejected invalid case_id; current state unchanged");
            return;
        }
        setIntent(intent);
        saveSnapshot("before_case_launch", false);
        resetCase(requested);
    }

    private void bindCase(int button, int id) {
        ((Button) findViewById(button)).setOnClickListener(v -> {
            saveSnapshot("before_case_switch", false);
            resetCase(id);
        });
    }

    private void resetCase(int id) {
        editor.removeCallbacks(textSnapshot);
        suppressTextSnapshots = true;
        try {
            testCase = CaseCatalog.get(id);
            runId = UUID.randomUUID().toString();
            sendCallbackCount = 0;
            editorActionCallbackCount = 0;
            lastActionId = -1;
            snapshotSequence = 0;
            editor.setText(testCase.seed);
            editor.setSelection(testCase.selectionStart, testCase.selectionEnd);
            showDetails();
        } finally {
            suppressTextSnapshots = false;
        }
        saveSnapshot("case_reset", false);
    }

    private void showDetails() {
        details.setText("Case " + testCase.id + " · " + testCase.name + "\n" + testCase.instruction);
    }

    private static int clamp(int value, int length) { return Math.max(0, Math.min(value, length)); }

    private void updateStatus(String message) {
        status.setText(message + "\nSEND callbacks: " + sendCallbackCount
            + " · all editor callbacks: " + editorActionCallbackCount);
    }

    private void saveSnapshot(String reason, boolean explicit) {
        if (testCase == null || editor == null) return;
        try {
            Editable text = editor.getText();
            JSONObject result = new JSONObject();
            result.put("schema_version", 1);
            result.put("package_name", getPackageName());
            result.put("case_id", testCase.id);
            result.put("case_name", testCase.name);
            result.put("controlled_instruction", testCase.instruction);
            result.put("initial_text", testCase.seed);
            result.put("initial_selection_start_utf16", testCase.selectionStart);
            result.put("initial_selection_end_utf16", testCase.selectionEnd);
            result.put("run_id", runId);
            result.put("snapshot_sequence", ++snapshotSequence);
            result.put("reason", reason);
            result.put("snapshot_time_unix_ms", System.currentTimeMillis());
            result.put("text", text.toString());
            result.put("text_length_utf16", text.length());
            result.put("selection_start_utf16", editor.getSelectionStart());
            result.put("selection_end_utf16", editor.getSelectionEnd());
            result.put("composing_start_utf16", BaseInputConnection.getComposingSpanStart(text));
            result.put("composing_end_utf16", BaseInputConnection.getComposingSpanEnd(text));
            result.put("send_callback_count", sendCallbackCount);
            result.put("editor_action_callback_count", editorActionCallbackCount);
            result.put("last_action_id", lastActionId);
            result.put("configured_ime_action", EditorInfo.IME_ACTION_SEND);
            result.put("max_text_length_utf16", CaseCatalog.MAX_TEXT_UTF16);
            result.put("sends_nothing", true);
            result.put("public_input_only", true);
            // Records this editor's flag request, not any provider's learning/privacy behavior.
            result.put("personalized_learning_flag_requested", false);
            byte[] bytes = (result.toString(2) + "\n").getBytes(StandardCharsets.UTF_8);
            if (explicit) writePrivate("case-" + testCase.id + "-capture.json", bytes);
            writePrivate("case-" + testCase.id + "-latest.json", bytes);
            updateStatus(explicit ? "Captured own editor privately" : "Saved own editor: " + reason);
        } catch (Exception error) {
            // Never expose editor contents in logs or on an external channel.
            updateStatus("Private capture failed: " + error.getClass().getSimpleName());
        }
    }

    private void writePrivate(String name, byte[] bytes) throws Exception {
        // name is solely fixed case ID + fixed suffix; no path/data comes from the intent.
        AtomicFile file = new AtomicFile(new File(getFilesDir(), name));
        FileOutputStream stream = null;
        try {
            stream = file.startWrite();
            stream.write(bytes);
            file.finishWrite(stream);
        } catch (Exception error) {
            if (stream != null) file.failWrite(stream);
            throw error;
        }
    }

    @Override protected void onPause() {
        if (editor != null) editor.removeCallbacks(textSnapshot);
        saveSnapshot("on_pause", false);
        super.onPause();
    }

    @Override protected void onSaveInstanceState(Bundle out) {
        if (testCase != null) {
            saveSnapshot("on_save_instance_state", false);
            out.putInt("case_id", testCase.id);
            out.putString("run_id", runId);
            out.putInt("send_count", sendCallbackCount);
            out.putInt("action_count", editorActionCallbackCount);
            out.putInt("last_action", lastActionId);
            out.putLong("sequence", snapshotSequence);
            out.putString("text", editor.getText().toString());
            out.putInt("start", editor.getSelectionStart());
            out.putInt("end", editor.getSelectionEnd());
        }
        super.onSaveInstanceState(out);
    }
}
