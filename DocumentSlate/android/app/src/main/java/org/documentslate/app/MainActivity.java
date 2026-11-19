package org.documentslate.app;

import android.content.Context;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.print.PrintManager;
import android.provider.OpenableColumns;
import android.util.Log;

import com.google.androidgamesdk.GameActivity;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;

/**
 * Thin Java shell around the Rust app.
 * GameActivity loads libdocument_slate.so (see android.app.lib_name in the manifest)
 * and gives the on-screen keyboard a proper text connection.
 * Public methods here are called from Rust over JNI (src/android_print.rs).
 *
 * Opening files: Android hands apps a content:// address, not a file path. This class reads
 * the bytes (on a background thread) and parks them until Rust collects them with
 * takePendingName() / takePendingBytes(). Two ways in:
 *  - "Open with" / tapping a file in another app: an ACTION_VIEW intent (onCreate / onNewIntent).
 *  - The app's Open button: openDocumentPicker() shows Android's own file picker.
 */
public class MainActivity extends GameActivity {
    private static final String TAG = "DocumentSlate";
    private static final int PICK_DOCUMENT = 1;
    private static final int CREATE_DOCUMENT = 2;
    private static final int PICK_IMAGE = 3;
    private static final int PICK_ID_FILE = 4;
    private static final int CREATE_COPY = 5;
    private static final int CONFIRM_CREDENTIAL = 6;
    /** Refuse files bigger than this, so a huge file can't run the phone out of memory. */
    private static final long MAX_BYTES = 200L * 1024 * 1024;

    private final Object pendingLock = new Object();
    private String pendingName;
    private String pendingUri;
    private byte[] pendingBytes;
    private String pendingError;

    // ---- Back button / back gesture -------------------------------------------------------
    // We always catch "back" ourselves:
    //  - document open  -> raise a flag; Rust closes search / returns to the start screen.
    //  - start screen   -> send the app to the background (like Home) instead of finishing it.
    //    Finishing would destroy the activity while the process lives on, and the Rust side
    //    (android-activity) can't start again inside the same process: relaunching would hang
    //    on the splash screen.
    // Android 13+ delivers the back gesture through OnBackInvokedCallback, older versions as
    // KEYCODE_BACK.
    private volatile boolean backHandled;
    private volatile boolean backPressed;

    /** Called from Rust: true while a document is open (back = close document). */
    public void setBackHandled(boolean on) {
        backHandled = on;
    }

    /** Called from Rust: was back pressed since the last call? */
    public boolean takeBackPressed() {
        boolean b = backPressed;
        backPressed = false;
        return b;
    }

    private void onBack() {
        if (backHandled) {
            backPressed = true;
        } else {
            moveTaskToBack(true);
        }
    }

    private void registerBackCallback() {
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, this::onBack);
        }
    }

    @Override
    public boolean dispatchKeyEvent(android.view.KeyEvent e) {
        if (e.getKeyCode() == android.view.KeyEvent.KEYCODE_BACK) {
            if (e.getAction() == android.view.KeyEvent.ACTION_UP) {
                onBack();
            }
            return true;
        }
        return super.dispatchKeyEvent(e);
    }

    // ---- Share ----------------------------------------------------------------------------

    /**
     * Called from Rust: offer a file (written by Rust under files/share/) to other apps through
     * Android's share sheet. FileProvider gives the receiving app temporary read access only.
     */
    public void shareFile(String path, String mime) {
        runOnUiThread(() -> {
            try {
                java.io.File f = new java.io.File(path);
                Uri uri = androidx.core.content.FileProvider.getUriForFile(this, getPackageName() + ".files", f);
                Intent send = new Intent(Intent.ACTION_SEND);
                send.setType(mime);
                send.putExtra(Intent.EXTRA_STREAM, uri);
                send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                startActivity(Intent.createChooser(send, f.getName()));
            } catch (RuntimeException e) {
                Log.w(TAG, "share failed: " + e);
            }
        });
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        registerBackCallback();
        handleViewIntent(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleViewIntent(intent);
    }

    private void handleViewIntent(Intent intent) {
        if (intent != null && Intent.ACTION_VIEW.equals(intent.getAction()) && intent.getData() != null) {
            readInBackground(intent.getData());
        }
    }

    /** Called from Rust: show Android's file picker for the document types above. */
    public void openDocumentPicker() {
        openDocumentPickerAt(null);
    }

    /** The file picker, starting in the folder `initialUri` (a DocumentsContract URI) if given. */
    public void openDocumentPickerAt(String initialUri) {
        runOnUiThread(() -> {
            Intent pick = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            pick.addCategory(Intent.CATEGORY_OPENABLE);
            // Ask for read + write access that survives restarts, so Recent files can reopen it
            // later and Save can write changes back to the same file.
            pick.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
            pick.setType("*/*");
            // Any file: the engines read many more formats than MIME types can list (any text
            // file, templates, macro-enabled files), and Android often labels them wrongly.
            // Rust picks the app by extension/content and says so if it can't open one.
            if (initialUri != null && android.os.Build.VERSION.SDK_INT >= 26) {
                pick.putExtra(android.provider.DocumentsContract.EXTRA_INITIAL_URI, android.net.Uri.parse(initialUri));
            }
            startActivityForResult(pick, PICK_DOCUMENT);
        });
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == CREATE_DOCUMENT) {
            finishSaveAs(resultCode, data);
            return;
        }
        if (requestCode == PICK_IMAGE) {
            finishPickImage(resultCode, data);
            return;
        }
        if (requestCode == PICK_ID_FILE) {
            finishPickIdFile(resultCode, data);
            return;
        }
        if (requestCode == CREATE_COPY) {
            finishSaveCopy(resultCode, data);
            return;
        }
        if (requestCode == CONFIRM_CREDENTIAL) {
            authResult = resultCode == RESULT_OK ? "ok" : "!cancelled";
            return;
        }
        if (requestCode == PICK_DOCUMENT && resultCode == RESULT_OK && data != null && data.getData() != null) {
            Uri uri = data.getData();
            try {
                getContentResolver().takePersistableUriPermission(uri,
                        Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            } catch (SecurityException e) {
                // Some providers only allow reading; keep read access at least.
                try {
                    getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
                } catch (SecurityException e2) {
                    Log.w(TAG, "no lasting access to " + uri + ": " + e2);
                }
            }
            readInBackground(uri);
        }
    }

    // ---- Insert › Picture -----------------------------------------------------------------
    // Android's image picker; the chosen image is copied into the app's private folder
    // (files/insert/) and Rust inserts it from there with the engine's insert.picture.
    private volatile String pendingImagePath;

    /** Called from Rust: show Android's image picker. The copy arrives via takePendingImage(). */
    public void pickImage() {
        runOnUiThread(() -> {
            Intent pick = new Intent(Intent.ACTION_GET_CONTENT);
            pick.addCategory(Intent.CATEGORY_OPENABLE);
            pick.setType("image/*");
            pick.putExtra(Intent.EXTRA_MIME_TYPES, new String[]{"image/png", "image/jpeg", "image/gif", "image/bmp", "image/webp"});
            startActivityForResult(pick, PICK_IMAGE);
        });
    }

    private void finishPickImage(int resultCode, Intent data) {
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            return;
        }
        Uri uri = data.getData();
        new Thread(() -> {
            String mime = getContentResolver().getType(uri);
            String ext = mime == null ? "png" : mime.substring(mime.indexOf('/') + 1).replace("jpeg", "jpg");
            java.io.File dir = new java.io.File(getFilesDir(), "insert");
            //noinspection ResultOfMethodCallIgnored
            dir.mkdirs();
            java.io.File out = new java.io.File(dir, "picture-" + System.currentTimeMillis() + "." + ext);
            try (InputStream in = getContentResolver().openInputStream(uri);
                 java.io.OutputStream os = new java.io.FileOutputStream(out)) {
                if (in == null) {
                    throw new IOException("could not open the picture");
                }
                byte[] buf = new byte[64 * 1024];
                long total = 0;
                int n;
                while ((n = in.read(buf)) > 0) {
                    total += n;
                    if (total > 50L * 1024 * 1024) {
                        throw new IOException("picture is larger than 50 MB");
                    }
                    os.write(buf, 0, n);
                }
                pendingImagePath = out.getAbsolutePath();
            } catch (IOException | SecurityException e) {
                Log.w(TAG, "picture failed: " + e);
            }
        }, "pick-image").start();
    }

    /** Called from Rust: path of a picked picture (then cleared), or null. */
    public String takePendingImage() {
        String p = pendingImagePath;
        pendingImagePath = null;
        return p;
    }

    // ---- Save as / Export: Android's "save to" screen --------------------------------------
    // Rust writes the new file to a temporary path first; after the user picks where it goes,
    // Java copies it there and reports the new address (or an error / "cancelled").
    private String saveSource;
    private volatile String savedUri;
    private volatile String savedName;
    private volatile String saveError;

    /** Called from Rust: show the "save to" screen for a file Rust already wrote to sourcePath. */
    public void saveAsPicker(String suggestedName, String mime, String sourcePath) {
        runOnUiThread(() -> {
            saveSource = sourcePath;
            Intent create = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            create.addCategory(Intent.CATEGORY_OPENABLE);
            create.setType(mime);
            create.putExtra(Intent.EXTRA_TITLE, suggestedName);
            create.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
            startActivityForResult(create, CREATE_DOCUMENT);
        });
    }

    private void finishSaveAs(int resultCode, Intent data) {
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            saveError = "cancelled";
            return;
        }
        Uri uri = data.getData();
        try {
            getContentResolver().takePersistableUriPermission(uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        } catch (SecurityException e) {
            Log.w(TAG, "no lasting access to " + uri + ": " + e);
        }
        String source = saveSource;
        new Thread(() -> {
            String err = writeUri(uri.toString(), source);
            if (err != null) {
                saveError = err;
            } else {
                savedName = displayName(uri);
                savedUri = uri.toString();
            }
        }, "save-as").start();
    }

    /** Called from Rust: "uri\nname" of a finished Save as, "!error" (incl. "!cancelled"), or null. */
    public String takeSaveResult() {
        String e = saveError;
        if (e != null) {
            saveError = null;
            return "!" + e;
        }
        String u = savedUri;
        if (u != null) {
            savedUri = null;
            return u + "\n" + savedName;
        }
        return null;
    }

    /**
     * Called from Rust: reopen a document by its content:// address (Recent files).
     * Works while Android still grants access (always for files picked with Open; files that
     * came from another app's "Open with" may lose access after a restart).
     */
    public void openUri(String uri) {
        readInBackground(Uri.parse(uri));
    }

    private void readInBackground(Uri uri) {
        new Thread(() -> {
            String name = displayName(uri);
            try (InputStream in = getContentResolver().openInputStream(uri)) {
                if (in == null) {
                    throw new IOException("could not open " + name);
                }
                ByteArrayOutputStream out = new ByteArrayOutputStream();
                byte[] buf = new byte[64 * 1024];
                long total = 0;
                int n;
                while ((n = in.read(buf)) > 0) {
                    total += n;
                    if (total > MAX_BYTES) {
                        throw new IOException(name + " is larger than 200 MB");
                    }
                    out.write(buf, 0, n);
                }
                synchronized (pendingLock) {
                    pendingName = name;
                    pendingUri = uri.toString();
                    pendingBytes = out.toByteArray();
                    pendingError = null;
                }
            } catch (SecurityException e) {
                // Android withdrew access (a Recent file that came from another app, or whose
                // permission expired). "noaccess" + newline + uri lets Rust explain and drop it
                // from Recent.
                Log.w(TAG, "open failed: " + e);
                synchronized (pendingLock) {
                    pendingError = "noaccess\n" + uri;
                }
            } catch (IOException e) {
                Log.w(TAG, "open failed: " + e);
                synchronized (pendingLock) {
                    pendingError = String.valueOf(e.getMessage());
                }
            }
        }, "open-document").start();
    }

    /** File name as the user sees it (the engine uses the extension to pick the reader). */
    private String displayName(Uri uri) {
        try (Cursor c = getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst()) {
                String s = c.getString(0);
                if (s != null && !s.isEmpty()) {
                    return s;
                }
            }
        } catch (RuntimeException e) {
            Log.w(TAG, "display name: " + e);
        }
        String last = uri.getLastPathSegment();
        return last != null ? last : "document";
    }

    /** Called from Rust: name of a file waiting to be opened, or null. Call before takePendingBytes(). */
    public String takePendingName() {
        synchronized (pendingLock) {
            return pendingBytes != null ? pendingName : null;
        }
    }

    /** Called from Rust: content:// address of the waiting file (for Recent files), or null. */
    public String takePendingUri() {
        synchronized (pendingLock) {
            return pendingBytes != null ? pendingUri : null;
        }
    }

    /** Called from Rust: true if the app was launched to open a file ("Open with"), so skip the start screen. */
    public boolean launchedWithFile() {
        Intent i = getIntent();
        return i != null && Intent.ACTION_VIEW.equals(i.getAction()) && i.getData() != null;
    }

    /** Called from Rust: the waiting file's bytes (then cleared), or null. */
    public byte[] takePendingBytes() {
        synchronized (pendingLock) {
            byte[] b = pendingBytes;
            pendingBytes = null;
            return b;
        }
    }

    /** Called from Rust: a reading error to show (then cleared), or null. */
    public String takePendingError() {
        synchronized (pendingLock) {
            String e = pendingError;
            pendingError = null;
            return e;
        }
    }

    /**
     * Called from Rust. Height in pixels of the system bars at the top
     * (status bar, camera cut-out). Android 15+ draws apps edge-to-edge, so the app must keep
     * its own top bar clear of these.
     */
    public int topInsetPx() {
        android.view.View decor = getWindow().getDecorView();
        android.view.WindowInsets insets = decor.getRootWindowInsets();
        if (insets == null) {
            return 0;
        }
        return insets.getInsets(android.view.WindowInsets.Type.systemBars()
                | android.view.WindowInsets.Type.displayCutout()).top;
    }

    /**
     * Called from Rust (Save): copy a file Rust just wrote over the document at a content://
     * address. Returns null on success, or a short error message.
     * "wt" = write + truncate, so a shorter new version leaves no old bytes at the end.
     */
    public String writeUri(String uri, String path) {
        try (InputStream in = new java.io.FileInputStream(path);
             java.io.OutputStream out = getContentResolver().openOutputStream(Uri.parse(uri), "wt")) {
            if (out == null) {
                return "could not open the file for writing";
            }
            byte[] buf = new byte[64 * 1024];
            int n;
            while ((n = in.read(buf)) > 0) {
                out.write(buf, 0, n);
            }
            return null;
        } catch (IOException | SecurityException | IllegalArgumentException e) {
            Log.w(TAG, "save failed: " + e);
            return e instanceof SecurityException ? "no permission to change this file" : String.valueOf(e.getMessage());
        }
    }

    /**
     * Called from Rust: height in pixels covered at the bottom (on-screen keyboard when open,
     * else the navigation bar), so the edit toolbar can sit just above it.
     */
    public int bottomInsetPx() {
        android.view.WindowInsets insets = getWindow().getDecorView().getRootWindowInsets();
        if (insets == null) {
            return 0;
        }
        return insets.getInsets(android.view.WindowInsets.Type.ime()
                | android.view.WindowInsets.Type.navigationBars()).bottom;
    }

    /**
     * Called from Rust: dark status-bar icons (clock, battery) on light backgrounds,
     * light icons on dark or coloured ones.
     */
    public void setDarkStatusIcons(boolean dark) {
        runOnUiThread(() -> {
            android.view.WindowInsetsController c = getWindow().getInsetsController();
            if (c != null) {
                int flag = android.view.WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS;
                c.setSystemBarsAppearance(dark ? flag : 0, flag);
            }
        });
    }

    /** Called from Rust: keep the screen on (preference "keep screen on", while a document is open). */
    public void setKeepScreenOn(boolean on) {
        runOnUiThread(() -> {
            int flag = android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON;
            if (on) getWindow().addFlags(flag); else getWindow().clearFlags(flag);
        });
    }

    /** Called from Rust. Opens Android's system print dialog. */
    public void printPdf(String path, String jobName) {
        runOnUiThread(() -> {
            PrintManager pm = (PrintManager) getSystemService(Context.PRINT_SERVICE);
            pm.print(jobName, new PdfPrintAdapter(path, jobName), null);
        });
    }

    // ---- Links ----------------------------------------------------------------------------

    /**
     * Called from Rust: open a link. Web links go to the default browser (never an app that
     * claims the address); mailto:, tel:, geo: go to the app Android picks for them. Returns null,
     * or why it failed.
     */
    public String openLink(String url) {
        Uri uri = Uri.parse(url);
        String scheme = uri.getScheme() == null ? "" : uri.getScheme().toLowerCase(java.util.Locale.ROOT);
        Intent view;
        switch (scheme) {
            case "http":
            case "https":
                view = new Intent(Intent.ACTION_VIEW, uri);
                view.addCategory(Intent.CATEGORY_BROWSABLE);
                // The user's default browser: what a plain web address resolves to (an app that
                // claims this particular site, e.g. a maps or video app, is skipped).
                String browser = defaultBrowser();
                if (browser != null) {
                    view.setPackage(browser);
                }
                break;
            case "tel":
                view = new Intent(Intent.ACTION_DIAL, uri);
                break;
            case "mailto":
            case "geo":
            case "sms":
            case "smsto":
                view = new Intent(Intent.ACTION_VIEW, uri);
                break;
            default:
                return "links of this kind can't be opened";
        }
        view.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        final Intent first = view;
        runOnUiThread(() -> {
            try {
                startActivity(first);
            } catch (RuntimeException e) {
                // No app answers the browser selector (rare): let Android choose any web viewer.
                try {
                    Intent plain = new Intent(Intent.ACTION_VIEW, uri);
                    plain.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                    startActivity(plain);
                } catch (RuntimeException e2) {
                    Log.w(TAG, "no app to open " + scheme + " link: " + e2);
                    linkError = "No app on this phone can open that link";
                }
            }
        });
        return null;
    }

    private volatile String linkError;

    /** Package of the default browser, or null when none is set (Android then shows its chooser). */
    private String defaultBrowser() {
        Intent probe = new Intent(Intent.ACTION_VIEW, Uri.parse("https://example.com/"));
        probe.addCategory(Intent.CATEGORY_BROWSABLE);
        android.content.pm.ResolveInfo ri = getPackageManager().resolveActivity(probe, android.content.pm.PackageManager.MATCH_DEFAULT_ONLY);
        if (ri == null || ri.activityInfo == null) {
            return null;
        }
        String pkg = ri.activityInfo.packageName;
        // "android" = the system chooser: no default browser chosen yet.
        return "android".equals(pkg) ? null : pkg;
    }

    /** Called from Rust: why the last openLink failed on the UI thread, once. */
    public String takeLinkError() {
        String e = linkError;
        linkError = null;
        return e;
    }

    /** Called from Rust: copy text to the clipboard (barcode contents, Wi-Fi passwords). */
    public void copyText(String label, String text) {
        runOnUiThread(() -> {
            android.content.ClipboardManager cm = (android.content.ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
            if (cm != null) {
                cm.setPrimaryClip(android.content.ClipData.newPlainText(label, text));
            }
        });
    }

    // ---- Screen lock confirmation (digital IDs) ------------------------------------------
    // Rust asks for the user's fingerprint / face / PIN / pattern / password before a digital
    // ID's key is created or used; the Keystore keys then work for SecureKeys.AUTH_SECONDS.

    private volatile String authResult;

    /** Called from Rust: has the phone a screen lock (PIN, pattern, password)? Keys need one. */
    public boolean deviceSecure() {
        android.app.KeyguardManager km = (android.app.KeyguardManager) getSystemService(Context.KEYGUARD_SERVICE);
        return km != null && km.isDeviceSecure();
    }

    /** Called from Rust: show the system unlock prompt. The outcome arrives via takeAuthResult(). */
    @SuppressWarnings("deprecation")
    public void authenticate(String title, String subtitle) {
        authResult = null;
        runOnUiThread(() -> {
            try {
                if (android.os.Build.VERSION.SDK_INT >= 28) {
                    android.hardware.biometrics.BiometricPrompt.Builder b = new android.hardware.biometrics.BiometricPrompt.Builder(this)
                            .setTitle(title)
                            .setSubtitle(subtitle);
                    if (android.os.Build.VERSION.SDK_INT >= 30) {
                        b.setAllowedAuthenticators(android.hardware.biometrics.BiometricManager.Authenticators.BIOMETRIC_STRONG
                                | android.hardware.biometrics.BiometricManager.Authenticators.DEVICE_CREDENTIAL);
                    } else {
                        b.setDeviceCredentialAllowed(true);
                    }
                    b.build().authenticate(new android.os.CancellationSignal(), getMainExecutor(),
                            new android.hardware.biometrics.BiometricPrompt.AuthenticationCallback() {
                                @Override
                                public void onAuthenticationSucceeded(android.hardware.biometrics.BiometricPrompt.AuthenticationResult r) {
                                    authResult = "ok";
                                }

                                @Override
                                public void onAuthenticationError(int code, CharSequence msg) {
                                    boolean cancelled = code == android.hardware.biometrics.BiometricPrompt.BIOMETRIC_ERROR_USER_CANCELED
                                            || code == android.hardware.biometrics.BiometricPrompt.BIOMETRIC_ERROR_CANCELED;
                                    authResult = cancelled ? "!cancelled" : "!" + msg;
                                }
                            });
                } else {
                    android.app.KeyguardManager km = (android.app.KeyguardManager) getSystemService(Context.KEYGUARD_SERVICE);
                    Intent i = km == null ? null : km.createConfirmDeviceCredentialIntent(title, subtitle);
                    if (i == null) {
                        authResult = "!nolock";
                    } else {
                        startActivityForResult(i, CONFIRM_CREDENTIAL);
                    }
                }
            } catch (RuntimeException e) {
                authResult = "!" + e.getMessage();
            }
        });
    }

    /** Called from Rust: "ok", "!cancelled", "!message", or null while the prompt is up. */
    public String takeAuthResult() {
        String r = authResult;
        authResult = null;
        return r;
    }

    public boolean ksImport(String alias, byte[] pkcs8, String family, byte[] certDer) {
        return SecureKeys.importKey(alias, pkcs8, family, certDer);
    }

    public byte[] ksGenerate(String alias) {
        return SecureKeys.generateRsa(alias);
    }

    public byte[] ksSign(String alias, String digest, byte[] msg) {
        return SecureKeys.sign(alias, digest, msg);
    }

    public boolean ksHas(String alias) {
        return SecureKeys.has(alias);
    }

    public boolean ksDelete(String alias) {
        return SecureKeys.delete(alias);
    }

    public String ksError() {
        String e = SecureKeys.lastError;
        SecureKeys.lastError = null;
        return e;
    }

    // ---- Pick a digital ID file (.p12 / .pfx) ---------------------------------------------
    // Separate from the document picker: the bytes go to Rust's digital-ID import, not the viewer.

    private volatile byte[] pendingIdBytes;
    private volatile String pendingIdName;

    public void pickIdFile() {
        runOnUiThread(() -> {
            Intent pick = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            pick.addCategory(Intent.CATEGORY_OPENABLE);
            pick.setType("*/*");
            startActivityForResult(pick, PICK_ID_FILE);
        });
    }

    private void finishPickIdFile(int resultCode, Intent data) {
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            return;
        }
        Uri uri = data.getData();
        new Thread(() -> {
            try (InputStream in = getContentResolver().openInputStream(uri)) {
                if (in == null) return;
                ByteArrayOutputStream out = new ByteArrayOutputStream();
                byte[] buf = new byte[16 * 1024];
                int n;
                while ((n = in.read(buf)) > 0) {
                    out.write(buf, 0, n);
                    if (out.size() > 1024 * 1024) throw new IOException("too large for a digital ID");
                }
                pendingIdName = displayName(uri);
                pendingIdBytes = out.toByteArray();
            } catch (IOException e) {
                Log.w(TAG, "digital ID read failed: " + e);
            }
        }, "read-id").start();
    }

    public String takePendingIdName() {
        return pendingIdName;
    }

    public byte[] takePendingIdBytes() {
        byte[] b = pendingIdBytes;
        pendingIdBytes = null;
        pendingIdName = null;
        return b;
    }

    // ---- Save a copy (digital ID backup) --------------------------------------------------
    // Like saveAsPicker, but the result is kept apart so it never becomes "the open document".

    private volatile String copySource;
    private volatile String copyResult;

    public void saveCopyPicker(String suggestedName, String mime, String sourcePath) {
        runOnUiThread(() -> {
            copySource = sourcePath;
            Intent create = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            create.addCategory(Intent.CATEGORY_OPENABLE);
            create.setType(mime);
            create.putExtra(Intent.EXTRA_TITLE, suggestedName);
            startActivityForResult(create, CREATE_COPY);
        });
    }

    private void finishSaveCopy(int resultCode, Intent data) {
        String source = copySource;
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            copyResult = "!cancelled";
            new java.io.File(source).delete();
            return;
        }
        Uri uri = data.getData();
        new Thread(() -> {
            String err = writeUri(uri.toString(), source);
            new java.io.File(source).delete();
            copyResult = err == null ? displayName(uri) : "!" + err;
        }, "save-copy").start();
    }

    /** Called from Rust: the saved copy's name, "!error" ("!cancelled"), or null while waiting. */
    public String takeCopyResult() {
        String r = copyResult;
        copyResult = null;
        return r;
    }
}
