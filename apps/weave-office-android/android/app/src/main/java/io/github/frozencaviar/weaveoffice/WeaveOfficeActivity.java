package io.github.frozencaviar.weaveoffice;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.content.SharedPreferences;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.provider.OpenableColumns;
import android.util.Log;
import android.webkit.MimeTypeMap;

import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import com.google.androidgamesdk.GameActivity;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * Weave Office's Activity: GameActivity runs the Rust app (libweave_office_android.so); this
 * class adds what winit doesn't offer on Android.
 *
 * <ul>
 *   <li><b>Documents</b>: Storage Access Framework pickers, driven by small JSON files in
 *       {@code files/bridge} (see {@code src/bridge.rs} for the protocol). Picked documents are
 *       copied in; files the app writes are copied back to the document they stand for.</li>
 *   <li><b>The window</b>: the app draws its own chrome, so the system bars are hidden and the
 *       surface runs edge to edge under a camera cutout.</li>
 * </ul>
 */
public class WeaveOfficeActivity extends GameActivity {
    private static final String TAG = "WeaveOffice";
    private static final int REQ_OPEN = 1001;
    private static final int REQ_SAVE = 1002;

    private File bridge;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ExecutorService io = Executors.newSingleThreadExecutor();
    private long pendingId = -1;
    private String pendingName = "";
    /** The last modification time copied back for each remembered path (-1: not written yet). */
    private final Map<String, Long> written = new java.util.concurrent.ConcurrentHashMap<>();
    /** A counter for the names of documents other apps sent. */
    private int incomingSeq = 0;
    /** The self-updater (GitHub releases). */
    private Updater updater;

    private final Runnable poll = new Runnable() {
        @Override
        public void run() {
            try {
                checkRequest();
                checkSync();
                checkClipboard();
                checkPrint();
                checkUpdateRequest();
                checkQuit();
            } catch (Exception e) {
                Log.e(TAG, "bridge", e);
            }
            handler.postDelayed(this, 150);
        }
    };

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        bridge = new File(getFilesDir(), "bridge");
        //noinspection ResultOfMethodCallIgnored
        bridge.mkdirs();
        hideSystemBars();
        // Draw under a front camera's cutout on every Android version, as Android 15 does for
        // apps targeting it.
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P) {
            android.view.WindowManager.LayoutParams lp = getWindow().getAttributes();
            lp.layoutInDisplayCutoutMode = android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R
                    ? android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
                    : android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            getWindow().setAttributes(lp);
        }
        handleSentDocument(getIntent());
        updater = new Updater(this);
        if (state == null && updater.checkAtStartup()) {
            // A moment after launch, so the first frames aren't competing with it.
            handler.postDelayed(() -> updater.check(false), 4000);
        }
        // The system clipboard: publish it to the bridge whenever it changes, so the app's
        // paste reads the current text without a round trip.
        android.content.ClipboardManager clipboard =
                (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard != null) {
            clipboard.addPrimaryClipChangedListener(this::writeClipboard);
            writeClipboard();
        }
        handler.post(poll);
    }

    /** The last pointer tool written to the bridge ({@code MotionEvent.TOOL_TYPE_*}). */
    private int lastTool = -1;

    /**
     * Record which pointer is in use, so the app can tell the stylus from a finger: winit's
     * Android backend reports every MotionEvent as a touch, with no tool type of its own. The
     * file is written only when the tool changes, so a stroke costs nothing.
     */
    @Override
    public boolean dispatchTouchEvent(android.view.MotionEvent event) {
        int tool = event.getToolType(0);
        if (tool != lastTool) {
            lastTool = tool;
            try (FileOutputStream out = new FileOutputStream(new File(bridge, "tool.txt"))) {
                out.write(Integer.toString(tool).getBytes(StandardCharsets.UTF_8));
            } catch (IOException e) {
                Log.e(TAG, "tool", e);
            }
        }
        return super.dispatchTouchEvent(event);
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleSentDocument(intent);
    }

    @Override
    protected void onResume() {
        super.onResume();
        // An update downloaded while the user was allowing Weave Office to install apps.
        if (updater != null) {
            updater.resumePendingInstall();
        }
    }

    @Override
    protected void onDestroy() {
        handler.removeCallbacks(poll);
        io.shutdown();
        super.onDestroy();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    private void hideSystemBars() {
        WindowCompat.setDecorFitsSystemWindows(getWindow(), false);
        WindowInsetsControllerCompat controller =
                WindowCompat.getInsetsController(getWindow(), getWindow().getDecorView());
        controller.hide(WindowInsetsCompat.Type.systemBars());
        controller.setSystemBarsBehavior(
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
    }

    // ---- Document pickers -------------------------------------------------------------------

    private void checkRequest() throws Exception {
        File f = new File(bridge, "request.json");
        if (pendingId != -1 || !f.isFile()) {
            return;
        }
        JSONObject r = new JSONObject(readText(f));
        //noinspection ResultOfMethodCallIgnored
        f.delete();
        long id = r.getLong("id");
        String kind = r.getString("kind");
        Intent intent;
        int code;
        if ("open".equals(kind)) {
            // The request's extensions are advisory: the picker opens on */* so any file can be
            // picked, and the app sniffs the format from the bytes.
            intent = new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*");
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, r.optBoolean("multiple", false));
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
            code = REQ_OPEN;
        } else if ("save".equals(kind)) {
            pendingName = r.optString("file_name", "Untitled");
            intent = new Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
                    .setType(mimeFor(pendingName))
                    .putExtra(Intent.EXTRA_TITLE, pendingName);
            code = REQ_SAVE;
        } else {
            answerError(id, "unknown request " + kind);
            return;
        }
        pendingId = id;
        try {
            startActivityForResult(intent, code);
        } catch (ActivityNotFoundException e) {
            pendingId = -1;
            answerError(id, "no document picker on this device");
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != REQ_OPEN && requestCode != REQ_SAVE) {
            return;
        }
        final long id = pendingId;
        pendingId = -1;
        if (id == -1) {
            return;
        }
        if (resultCode != Activity.RESULT_OK || data == null) {
            answerCancelled(id);
            return;
        }
        if (requestCode == REQ_OPEN) {
            final List<Uri> uris = new ArrayList<>();
            ClipData clip = data.getClipData();
            if (clip != null) {
                for (int i = 0; i < clip.getItemCount(); i++) {
                    uris.add(clip.getItemAt(i).getUri());
                }
            } else if (data.getData() != null) {
                uris.add(data.getData());
            }
            final int flags = data.getFlags();
            io.execute(() -> copyIn(id, uris, flags));
        } else {
            final Uri uri = data.getData();
            if (uri == null) {
                answerCancelled(id);
                return;
            }
            String provided = displayName(uri, pendingName);
            String name = savedName(provided, pendingName);
            Log.i(TAG, "save: asked for \"" + pendingName + "\", the provider named it \"" + provided + "\"; writing \"" + name + "\"");
            keepAccess(uri, Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            File dest = new File(bridge, "out/" + id + "/" + safeName(name));
            //noinspection ResultOfMethodCallIgnored
            dest.getParentFile().mkdirs();
            remember(dest, uri);
            try {
                answer(new JSONObject().put("id", id).put("paths", new JSONArray().put(dest.getAbsolutePath())));
            } catch (Exception e) {
                Log.e(TAG, "save answer", e);
            }
        }
    }

    /** Copy picked documents into the bridge inbox and answer with their paths. */
    private void copyIn(long id, List<Uri> uris, int flags) {
        JSONArray paths = new JSONArray();
        String error = null;
        for (Uri uri : uris) {
            boolean writable = keepAccess(uri, flags);
            File dest = new File(bridge, "inbox/" + id + "/" + safeName(nameWithExtension(uri)));
            //noinspection ResultOfMethodCallIgnored
            dest.getParentFile().mkdirs();
            try (InputStream in = getContentResolver().openInputStream(uri);
                 OutputStream out = new FileOutputStream(dest)) {
                if (in == null) {
                    throw new IOException("the provider returned no data");
                }
                copy(in, out);
                paths.put(dest.getAbsolutePath());
                if (writable) {
                    remember(dest, uri);
                }
            } catch (Exception e) {
                Log.e(TAG, "open " + uri, e);
                error = e.getMessage();
            }
        }
        try {
            JSONObject a = new JSONObject().put("id", id).put("paths", paths);
            if (error != null) {
                a.put("error", error);
            }
            answer(a);
        } catch (Exception e) {
            Log.e(TAG, "open answer", e);
        }
    }

    /** Keep access to an opened document across restarts; true when it may be written back. */
    private boolean keepAccess(Uri uri, int flags) {
        boolean write = (flags & Intent.FLAG_GRANT_WRITE_URI_PERMISSION) != 0;
        try {
            getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION
                    | (write ? Intent.FLAG_GRANT_WRITE_URI_PERMISSION : 0));
        } catch (SecurityException e) {
            // Not persistable: the grant lasts while the app runs, which is enough to save back.
        }
        return write;
    }

    // ---- Writing back ----------------------------------------------------------------------

    /** Copy a file the app wrote back to the document it stands for, once per write. */
    private void checkSync() {
        SharedPreferences docs = documents();
        for (Map.Entry<String, ?> entry : docs.getAll().entrySet()) {
            File local = new File(entry.getKey());
            if (!local.isFile()) {
                continue;
            }
            long mtime = local.lastModified();
            Long seen = written.get(local.getAbsolutePath());
            if (seen == null) {
                // A path remembered by an earlier run: record it, don't copy the staged copy back.
                written.put(local.getAbsolutePath(), mtime);
                continue;
            }
            if (seen == mtime) {
                continue;
            }
            written.put(local.getAbsolutePath(), mtime);
            final Uri uri = Uri.parse((String) entry.getValue());
            io.execute(() -> copyOut(local, uri));
        }
    }

    private void copyOut(File local, Uri uri) {
        try (InputStream in = new FileInputStream(local);
             OutputStream out = getContentResolver().openOutputStream(uri, "wt")) {
            if (out == null) {
                throw new IOException("the provider refused the write");
            }
            copy(in, out);
            Log.i(TAG, "saved " + local.getName() + " to " + uri);
        } catch (Exception e) {
            Log.e(TAG, "save " + uri, e);
            toast("Couldn't save " + local.getName() + ": " + e.getMessage());
        }
    }

    private SharedPreferences documents() {
        return getSharedPreferences("documents", MODE_PRIVATE);
    }

    private void remember(File local, Uri uri) {
        documents().edit().putString(local.getAbsolutePath(), uri.toString()).apply();
        written.put(local.getAbsolutePath(), local.exists() ? local.lastModified() : -1L);
    }

    // ---- Leaving ---------------------------------------------------------------------------

    private void checkQuit() {
        File f = new File(bridge, "quit");
        if (!f.exists()) {
            return;
        }
        //noinspection ResultOfMethodCallIgnored
        f.delete();
        finish();
    }

    // ---- Clipboard -------------------------------------------------------------------------

    /** Publish the system clipboard's text to the bridge (the app's paste reads it). */
    private void writeClipboard() {
        android.content.ClipboardManager clipboard =
                (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard == null) {
            return;
        }
        String text = "";
        ClipData clip = clipboard.getPrimaryClip();
        if (clip != null && clip.getItemCount() > 0) {
            CharSequence value = clip.getItemAt(0).coerceToText(this);
            if (value != null) {
                text = value.toString();
            }
        }
        try {
            File tmp = new File(bridge, "clip.txt.tmp");
            try (OutputStream out = new FileOutputStream(tmp)) {
                out.write(text.getBytes(StandardCharsets.UTF_8));
            }
            if (!tmp.renameTo(new File(bridge, "clip.txt"))) {
                Log.w(TAG, "clipboard: rename failed");
            }
        } catch (Exception e) {
            Log.e(TAG, "clipboard", e);
        }
    }

    /** The app copied text: put it on the system clipboard. */
    private void checkClipboard() {
        File set = new File(bridge, "clip-set.txt");
        if (!set.isFile()) {
            return;
        }
        String text;
        try {
            text = readText(set);
        } catch (Exception e) {
            Log.e(TAG, "clipboard", e);
            //noinspection ResultOfMethodCallIgnored
            set.delete();
            return;
        }
        //noinspection ResultOfMethodCallIgnored
        set.delete();
        android.content.ClipboardManager clipboard =
                (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard != null) {
            clipboard.setPrimaryClip(ClipData.newPlainText("Weave Office", text));
        }
    }

    // ---- Updates ---------------------------------------------------------------------------

    /** The app asked for an update check (the Shortcut Pad's Update key). */
    private void checkUpdateRequest() {
        File request = new File(bridge, "check-update");
        if (request.exists() && request.delete()) {
            updater.check(true);
        }
    }

    // ---- Printing --------------------------------------------------------------------------

    /** The app exported a PDF to print: hand it to Android's print service. */
    private void checkPrint() {
        File request = new File(bridge, "print.json");
        if (!request.isFile()) {
            return;
        }
        String path;
        try {
            path = new JSONObject(readText(request)).getString("path");
        } catch (Exception e) {
            Log.e(TAG, "print", e);
            //noinspection ResultOfMethodCallIgnored
            request.delete();
            return;
        }
        //noinspection ResultOfMethodCallIgnored
        request.delete();
        final File pdf = new File(path);
        if (!pdf.isFile()) {
            toast("Couldn't print: the PDF is missing");
            return;
        }
        android.print.PrintManager manager = (android.print.PrintManager) getSystemService(PRINT_SERVICE);
        if (manager == null) {
            toast("Printing isn't available on this device");
            return;
        }
        android.print.PrintDocumentAdapter adapter = new android.print.PrintDocumentAdapter() {
            @Override
            public void onLayout(android.print.PrintAttributes oldAttributes, android.print.PrintAttributes attributes,
                                 android.os.CancellationSignal signal, LayoutResultCallback callback, Bundle extras) {
                android.print.PrintDocumentInfo info = new android.print.PrintDocumentInfo.Builder(pdf.getName())
                        .setContentType(android.print.PrintDocumentInfo.CONTENT_TYPE_DOCUMENT)
                        .build();
                callback.onLayoutFinished(info, true);
            }

            @Override
            public void onWrite(android.print.PageRange[] pages, android.os.ParcelFileDescriptor destination,
                                android.os.CancellationSignal signal, WriteResultCallback callback) {
                try (InputStream in = new FileInputStream(pdf);
                     OutputStream out = new FileOutputStream(destination.getFileDescriptor())) {
                    copy(in, out);
                    callback.onWriteFinished(new android.print.PageRange[]{android.print.PageRange.ALL_PAGES});
                } catch (Exception e) {
                    Log.e(TAG, "print write", e);
                    callback.onWriteFailed(e.getMessage());
                }
            }
        };
        manager.print(pdf.getName(), adapter, new android.print.PrintAttributes.Builder().build());
    }

    // ---- Documents from other apps ---------------------------------------------------------

    /** "Open with Weave Office" and "Share to Weave Office": copy the document into the bridge. */
    private void handleSentDocument(Intent intent) {
        if (intent == null) {
            return;
        }
        Uri uri = null;
        if (Intent.ACTION_VIEW.equals(intent.getAction())) {
            uri = intent.getData();
        } else if (Intent.ACTION_SEND.equals(intent.getAction())) {
            uri = intent.getParcelableExtra(Intent.EXTRA_STREAM);
        }
        if (uri == null) {
            return;
        }
        // Handled: a recreated Activity mustn't open it again.
        setIntent(new Intent(Intent.ACTION_MAIN));
        final Uri from = uri;
        final String name = (incomingSeq++) + "-" + safeName(nameWithExtension(uri));
        io.execute(() -> {
            File dir = new File(bridge, "incoming");
            //noinspection ResultOfMethodCallIgnored
            dir.mkdirs();
            File tmp = new File(dir, name + ".tmp");
            try (InputStream in = getContentResolver().openInputStream(from);
                 OutputStream out = new FileOutputStream(tmp)) {
                if (in == null) {
                    throw new IOException("the provider returned no data");
                }
                copy(in, out);
            } catch (Exception e) {
                Log.e(TAG, "incoming " + from, e);
                //noinspection ResultOfMethodCallIgnored
                tmp.delete();
                return;
            }
            if (!tmp.renameTo(new File(dir, name))) {
                Log.e(TAG, "incoming: rename failed");
            }
        });
    }

    // ---- Helpers ---------------------------------------------------------------------------

    private void answerCancelled(long id) {
        try {
            answer(new JSONObject().put("id", id).put("cancelled", true));
        } catch (Exception e) {
            Log.e(TAG, "answer", e);
        }
    }

    private void answerError(long id, String error) {
        try {
            answer(new JSONObject().put("id", id).put("cancelled", true).put("error", error));
        } catch (Exception e) {
            Log.e(TAG, "answer", e);
        }
    }

    /** Write answer.json under a temporary name, then rename, so Rust never reads half of it. */
    private synchronized void answer(JSONObject a) throws IOException {
        File tmp = new File(bridge, "answer.json.tmp");
        try (OutputStream out = new FileOutputStream(tmp)) {
            out.write(a.toString().getBytes(StandardCharsets.UTF_8));
        }
        if (!tmp.renameTo(new File(bridge, "answer.json"))) {
            throw new IOException("couldn't publish the answer");
        }
    }

    private String displayName(Uri uri, String fallback) {
        try (Cursor c = getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst() && !c.isNull(0)) {
                return c.getString(0);
            }
        } catch (Exception e) {
            Log.w(TAG, "display name", e);
        }
        return fallback;
    }

    /** The name typed in the picker wins; a name without an extension gets the requested one. */
    private static String savedName(String provided, String requested) {
        if (provided == null || provided.isEmpty()) {
            return requested;
        }
        int dot = provided.lastIndexOf('.');
        if (dot <= 0 && requested.lastIndexOf('.') > 0) {
            return provided + requested.substring(requested.lastIndexOf('.'));
        }
        return provided;
    }

    private String nameWithExtension(Uri uri) {
        String name = displayName(uri, "document");
        if (name.lastIndexOf('.') <= 0) {
            String type = getContentResolver().getType(uri);
            String extension = type == null ? null : MimeTypeMap.getSingleton().getExtensionFromMimeType(type);
            if (extension != null && !extension.isEmpty()) {
                name = name + "." + extension;
            }
        }
        return name;
    }

    private static String safeName(String name) {
        String base = name.replace('/', '_').replace('\\', '_').replace(':', '_');
        return base.isEmpty() ? "document" : base;
    }

    private static String mimeFor(String name) {
        int dot = name.lastIndexOf('.');
        String extension = dot < 0 ? "" : name.substring(dot + 1).toLowerCase(java.util.Locale.ROOT);
        switch (extension) {
            case "docx": return "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
            case "xlsx": return "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
            case "pptx": return "application/vnd.openxmlformats-officedocument.presentationml.presentation";
            case "pdf": return "application/pdf";
            case "md": return "text/markdown";
            case "txt": return "text/plain";
            case "html": case "htm": return "text/html";
            case "csv": return "text/csv";
            case "json": return "application/json";
            default: return "application/octet-stream";
        }
    }

    private static String readText(File f) throws IOException {
        try (InputStream in = new FileInputStream(f)) {
            java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
            copy(in, out);
            return out.toString(StandardCharsets.UTF_8.name());
        }
    }

    private static void copy(InputStream in, OutputStream out) throws IOException {
        byte[] buffer = new byte[64 * 1024];
        int n;
        while ((n = in.read(buffer)) > 0) {
            out.write(buffer, 0, n);
        }
    }

    private void toast(String message) {
        handler.post(() -> android.widget.Toast.makeText(this, message, android.widget.Toast.LENGTH_LONG).show());
    }
}
