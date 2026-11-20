//! Calls from Rust into the Java shell (`MainActivity`, android/app/src/main/java/...).
//! - Printing: Rust writes a PDF, Java hands it to Android's PrintManager (system print dialog:
//!   network IPP/Mopria printers on local Wi-Fi, or "Save as PDF"). No cloud involved.
//! - Opening files: Java's file picker / "Open with" intents read a document's bytes; Rust collects them.
//! - Status bar height (Android 15+ draws apps edge-to-edge).

use std::path::Path;

use jni::objects::{JByteArray, JObject, JString};
use jni::{AttachGuard, JavaVM};
use winit::platform::android::activity::AndroidApp;

/// Attach this thread to the JVM and get the activity object.
fn with_activity<T>(
    app: &AndroidApp,
    f: impl FnOnce(&mut AttachGuard<'_>, &JObject<'_>) -> jni::errors::Result<T>,
) -> Result<T, String> {
    // SAFETY: both pointers come from android-activity and stay valid while the app runs.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    f(&mut env, &activity).map_err(|e| e.to_string())
}

/// `MainActivity.topInsetPx()`: status bar + cut-out height in pixels.
pub fn top_inset_px(app: &AndroidApp) -> Result<i32, String> {
    with_activity(app, |env, act| env.call_method(act, "topInsetPx", "()I", &[])?.i())
}

/// `MainActivity.bottomInsetPx()`: keyboard (when open) or navigation bar height in pixels.
pub fn bottom_inset_px(app: &AndroidApp) -> Result<i32, String> {
    with_activity(app, |env, act| env.call_method(act, "bottomInsetPx", "()I", &[])?.i())
}

/// `MainActivity.writeUri(uri, path)`: copy `path` over the document at `uri` (Save).
pub fn write_uri(app: &AndroidApp, uri: &str, path: &Path) -> Result<(), String> {
    crate::rebrand::scrub_file(path); // no upstream app names in files leaving the app
    let err = with_activity(app, |env, act| {
        let u = env.new_string(uri)?;
        let p = env.new_string(path.to_string_lossy())?;
        let r = env.call_method(act, "writeUri", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", &[(&u).into(), (&p).into()])?.l()?;
        if r.is_null() {
            return Ok(None);
        }
        let s: String = env.get_string(&JString::from(r))?.into();
        Ok(Some(s))
    })?;
    match err {
        None => Ok(()),
        Some(e) => Err(e),
    }
}

/// `MainActivity.saveAsPicker(name, mime, source)`: Android's "save to" screen; Java copies
/// `source` to wherever the user chooses. The outcome arrives through [`take_save_result`].
/// `initial`: the open document's address, so the screen starts in that document's folder.
pub fn save_as_picker(app: &AndroidApp, suggested_name: &str, mime: &str, source: &Path, initial: Option<&str>) -> Result<(), String> {
    crate::rebrand::scrub_file(source); // no upstream app names in files leaving the app
    with_activity(app, |env, act| {
        let n = env.new_string(suggested_name)?;
        let m = env.new_string(mime)?;
        let s = env.new_string(source.to_string_lossy())?;
        let i = match initial {
            Some(u) => JObject::from(env.new_string(u)?),
            None => JObject::null(),
        };
        env.call_method(
            act,
            "saveAsPicker",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            &[(&n).into(), (&m).into(), (&s).into(), (&i).into()],
        )?;
        Ok(())
    })
}

/// `MainActivity.saveToDocuments(name, mime, source)`: first Save of a new document, straight
/// into the phone's Documents folder. The outcome arrives through [`take_save_result`].
pub fn save_to_documents(app: &AndroidApp, name: &str, mime: &str, source: &Path) -> Result<(), String> {
    crate::rebrand::scrub_file(source); // no upstream app names in files leaving the app
    with_activity(app, |env, act| {
        let n = env.new_string(name)?;
        let m = env.new_string(mime)?;
        let s = env.new_string(source.to_string_lossy())?;
        env.call_method(act, "saveToDocuments", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V", &[(&n).into(), (&m).into(), (&s).into()])?;
        Ok(())
    })
}

/// `MainActivity.isPickerOpen()`: the document picker is still on screen.
pub fn is_picker_open(app: &AndroidApp) -> Result<bool, String> {
    with_activity(app, |env, act| env.call_method(act, "isPickerOpen", "()Z", &[])?.z())
}

/// `MainActivity.hasAllFilesFlavor()`: the "full" build on Android 11+ (may list folders itself).
pub fn has_all_files_flavor(app: &AndroidApp) -> Result<bool, String> {
    with_activity(app, |env, act| env.call_method(act, "hasAllFilesFlavor", "()Z", &[])?.z())
}

/// `MainActivity.hasAllFilesAccess()`: "All files access" is on (Java re-checks on resume).
pub fn has_all_files_access(app: &AndroidApp) -> Result<bool, String> {
    with_activity(app, |env, act| env.call_method(act, "hasAllFilesAccess", "()Z", &[])?.z())
}

/// `MainActivity.openAllFilesSettings()`: Android's "All files access" switch for this app.
pub fn open_all_files_settings(app: &AndroidApp) -> Result<(), String> {
    with_activity(app, |env, act| {
        env.call_method(act, "openAllFilesSettings", "()V", &[])?;
        Ok(())
    })
}

/// Save by file path ("full" flavor): scrub `source`, write it over `dest` by way of a hidden
/// copy ([`crate::browse::replace_file`]), and let Android's media index know.
pub fn write_path(app: &AndroidApp, dest: &str, source: &Path) -> Result<(), String> {
    crate::rebrand::scrub_file(source); // no upstream app names in files leaving the app
    crate::browse::replace_file(Path::new(dest), source)?;
    let _ = with_activity(app, |env, act| {
        let p = env.new_string(dest)?;
        env.call_method(act, "scanFile", "(Ljava/lang/String;)V", &[(&p).into()])?;
        Ok(())
    });
    Ok(())
}

pub enum SaveResult {
    Saved { uri: String, name: String },
    Cancelled,
    Failed(String),
}

/// Outcome of the last [`save_as_picker`], once Java has one.
pub fn take_save_result(app: &AndroidApp) -> Result<Option<SaveResult>, String> {
    let s: Option<String> = with_activity(app, |env, act| {
        let r = env.call_method(act, "takeSaveResult", "()Ljava/lang/String;", &[])?.l()?;
        if r.is_null() {
            return Ok(None);
        }
        Ok(Some(env.get_string(&JString::from(r))?.into()))
    })?;
    Ok(s.map(|s| match s.strip_prefix('!') {
        Some("cancelled") => SaveResult::Cancelled,
        Some(e) => SaveResult::Failed(e.to_owned()),
        None => {
            let (uri, name) = s.split_once('\n').unwrap_or((s.as_str(), "document"));
            SaveResult::Saved { uri: uri.to_owned(), name: name.to_owned() }
        }
    }))
}

/// `MainActivity.pickImage()`: Android's image picker; the copy's path arrives via [`take_pending_image`].
pub fn pick_image(app: &AndroidApp) -> Result<(), String> {
    with_activity(app, |env, act| {
        env.call_method(act, "pickImage", "()V", &[])?;
        Ok(())
    })
}

/// Path of a picture Java copied into files/insert/, once one is ready.
pub fn take_pending_image(app: &AndroidApp) -> Result<Option<String>, String> {
    with_activity(app, |env, act| {
        let r = env.call_method(act, "takePendingImage", "()Ljava/lang/String;", &[])?.l()?;
        if r.is_null() {
            return Ok(None);
        }
        Ok(Some(env.get_string(&JString::from(r))?.into()))
    })
}

/// `MainActivity.printPdf(path, jobName)`: open the system print dialog for a PDF file.
pub fn print_pdf(app: &AndroidApp, pdf: &Path, job_name: &str) -> Result<(), String> {
    crate::rebrand::scrub_file(pdf); // no upstream app names in files leaving the app
    with_activity(app, |env, act| {
        let path = env.new_string(pdf.to_string_lossy())?;
        let name = env.new_string(job_name)?;
        env.call_method(act, "printPdf", "(Ljava/lang/String;Ljava/lang/String;)V", &[(&path).into(), (&name).into()])?;
        Ok(())
    })
}

/// `MainActivity.openDocumentPicker()`: show Android's file picker. The chosen file arrives
/// later through [`take_pending_file`].
pub fn open_document_picker(app: &AndroidApp) -> Result<(), String> {
    with_activity(app, |env, act| {
        env.call_method(act, "openDocumentPicker", "()V", &[])?;
        Ok(())
    })
}

/// `MainActivity.openDocumentPickerAt(uri)`: the file picker starting in a given folder.
pub fn open_document_picker_at(app: &AndroidApp, initial_uri: &str) -> Result<(), String> {
    with_activity(app, |env, act| {
        let s = env.new_string(initial_uri)?;
        env.call_method(act, "openDocumentPickerAt", "(Ljava/lang/String;)V", &[(&s).into()])?;
        Ok(())
    })
}

/// `MainActivity.setDarkStatusIcons(dark)`: dark clock/battery icons for light backgrounds.
pub fn set_dark_status_icons(app: &AndroidApp, dark: bool) -> Result<(), String> {
    with_activity(app, |env, act| {
        env.call_method(act, "setDarkStatusIcons", "(Z)V", &[dark.into()])?;
        Ok(())
    })
}

/// `MainActivity.setKeepScreenOn(on)`: stop the screen from timing out.
pub fn set_keep_screen_on(app: &AndroidApp, on: bool) -> Result<(), String> {
    with_activity(app, |env, act| {
        env.call_method(act, "setKeepScreenOn", "(Z)V", &[on.into()])?;
        Ok(())
    })
}

/// `MainActivity.setBackHandled(on)`: while on, back/back-gesture is ours (close the document).
pub fn set_back_handled(app: &AndroidApp, on: bool) -> Result<(), String> {
    with_activity(app, |env, act| {
        env.call_method(act, "setBackHandled", "(Z)V", &[on.into()])?;
        Ok(())
    })
}

/// `MainActivity.takeBackPressed()`: was back pressed since the last call?
pub fn take_back_pressed(app: &AndroidApp) -> Result<bool, String> {
    with_activity(app, |env, act| env.call_method(act, "takeBackPressed", "()Z", &[])?.z())
}

/// `MainActivity.shareFile(path, mime)`: Android share sheet for a file under files/share/.
pub fn share_file(app: &AndroidApp, path: &Path, mime: &str) -> Result<(), String> {
    crate::rebrand::scrub_file(path); // no upstream app names in files leaving the app
    with_activity(app, |env, act| {
        let p = env.new_string(path.to_string_lossy())?;
        let m = env.new_string(mime)?;
        env.call_method(act, "shareFile", "(Ljava/lang/String;Ljava/lang/String;)V", &[(&p).into(), (&m).into()])?;
        Ok(())
    })
}

/// `MainActivity.openUri(uri)`: reopen a Recent file. It arrives through [`take_pending_file`].
pub fn open_uri(app: &AndroidApp, uri: &str) -> Result<(), String> {
    with_activity(app, |env, act| {
        let s = env.new_string(uri)?;
        env.call_method(act, "openUri", "(Ljava/lang/String;)V", &[(&s).into()])?;
        Ok(())
    })
}

/// `MainActivity.launchedWithFile()`: started by "Open with" (skip the start screen).
pub fn launched_with_file(app: &AndroidApp) -> Result<bool, String> {
    with_activity(app, |env, act| env.call_method(act, "launchedWithFile", "()Z", &[])?.z())
}

/// What Java has waiting for us, if anything.
pub enum Pending {
    File { name: String, uri: String, bytes: Vec<u8> },
    Error(String),
}

/// Collect a file (or a read error) that Java's picker / "Open with" produced.
pub fn take_pending_file(app: &AndroidApp) -> Result<Option<Pending>, String> {
    with_activity(app, |env, act| {
        let err = env.call_method(act, "takePendingError", "()Ljava/lang/String;", &[])?.l()?;
        if !err.is_null() {
            let s: String = env.get_string(&JString::from(err))?.into();
            return Ok(Some(Pending::Error(s)));
        }
        let name = env.call_method(act, "takePendingName", "()Ljava/lang/String;", &[])?.l()?;
        if name.is_null() {
            return Ok(None);
        }
        let name: String = env.get_string(&JString::from(name))?.into();
        let uri = env.call_method(act, "takePendingUri", "()Ljava/lang/String;", &[])?.l()?;
        let uri: String = if uri.is_null() { String::new() } else { env.get_string(&JString::from(uri))?.into() };
        let bytes = env.call_method(act, "takePendingBytes", "()[B", &[])?.l()?;
        if bytes.is_null() {
            return Ok(None);
        }
        let bytes = env.convert_byte_array(JByteArray::from(bytes))?;
        Ok(Some(Pending::File { name, uri, bytes }))
    })
}

// ---- Global handle -------------------------------------------------------------------------
// Most calls get the app from the shell; digital IDs and links are used from deep inside the
// screens (and from the signing callback), so the app is also kept here once at start.

static APP: std::sync::OnceLock<AndroidApp> = std::sync::OnceLock::new();

/// Remember the app for [`app`] (called once from `android_main`).
pub fn set_app(app: &AndroidApp) {
    let _ = APP.set(app.clone());
}

pub fn app() -> Option<&'static AndroidApp> {
    APP.get()
}

fn opt_string(env: &mut AttachGuard<'_>, o: JObject<'_>) -> jni::errors::Result<Option<String>> {
    if o.is_null() {
        return Ok(None);
    }
    Ok(Some(env.get_string(&JString::from(o))?.into()))
}

fn opt_bytes(env: &mut AttachGuard<'_>, o: JObject<'_>) -> jni::errors::Result<Option<Vec<u8>>> {
    if o.is_null() {
        return Ok(None);
    }
    Ok(Some(env.convert_byte_array(JByteArray::from(o))?))
}

/// `MainActivity.openLink(url)`: web links in the default browser; mailto/tel/geo/sms in their app.
pub fn open_link(app: &AndroidApp, url: &str) -> Result<(), String> {
    let err = with_activity(app, |env, act| {
        let u = env.new_string(url)?;
        let r = env.call_method(act, "openLink", "(Ljava/lang/String;)Ljava/lang/String;", &[(&u).into()])?.l()?;
        opt_string(env, r)
    })?;
    err.map_or(Ok(()), Err)
}

/// Why the last link couldn't be opened (no app for it), once.
pub fn take_link_error(app: &AndroidApp) -> Option<String> {
    with_activity(app, |env, act| {
        let r = env.call_method(act, "takeLinkError", "()Ljava/lang/String;", &[])?.l()?;
        opt_string(env, r)
    })
    .ok()
    .flatten()
}

/// `MainActivity.copyText(label, text)`: put text on the clipboard.
pub fn copy_text(app: &AndroidApp, label: &str, text: &str) -> Result<(), String> {
    with_activity(app, |env, act| {
        let l = env.new_string(label)?;
        let t = env.new_string(text)?;
        env.call_method(act, "copyText", "(Ljava/lang/String;Ljava/lang/String;)V", &[(&l).into(), (&t).into()])?;
        Ok(())
    })
}

/// Has the phone a screen lock? (Keystore keys that need the user need one.)
pub fn device_secure(app: &AndroidApp) -> Result<bool, String> {
    with_activity(app, |env, act| env.call_method(act, "deviceSecure", "()Z", &[])?.z())
}

/// Show the system fingerprint / PIN prompt; poll [`take_auth_result`].
pub fn authenticate(app: &AndroidApp, title: &str, subtitle: &str) -> Result<(), String> {
    with_activity(app, |env, act| {
        let t = env.new_string(title)?;
        let s = env.new_string(subtitle)?;
        env.call_method(act, "authenticate", "(Ljava/lang/String;Ljava/lang/String;)V", &[(&t).into(), (&s).into()])?;
        Ok(())
    })
}

/// `Some(Ok(()))` unlocked, `Some(Err("cancelled" | message))`, `None` while the prompt is up.
pub fn take_auth_result(app: &AndroidApp) -> Option<Result<(), String>> {
    let r = with_activity(app, |env, act| {
        let r = env.call_method(act, "takeAuthResult", "()Ljava/lang/String;", &[])?.l()?;
        opt_string(env, r)
    })
    .ok()
    .flatten()?;
    Some(match r.strip_prefix('!') {
        Some(e) => Err(e.to_owned()),
        None => Ok(()),
    })
}

fn ks_error(env: &mut AttachGuard<'_>, act: &JObject<'_>) -> jni::errors::Result<String> {
    let r = env.call_method(act, "ksError", "()Ljava/lang/String;", &[])?.l()?;
    Ok(opt_string(env, r)?.unwrap_or_else(|| "the key store refused".into()))
}

/// Move a private key into the Keystore under `alias`.
pub fn ks_import(app: &AndroidApp, alias: &str, pkcs8: &[u8], family: &str, cert: &[u8]) -> Result<(), String> {
    with_activity(app, |env, act| {
        let a = env.new_string(alias)?;
        let k = env.byte_array_from_slice(pkcs8)?;
        let f = env.new_string(family)?;
        let c = env.byte_array_from_slice(cert)?;
        let ok = env
            .call_method(act, "ksImport", "(Ljava/lang/String;[BLjava/lang/String;[B)Z", &[(&a).into(), (&k).into(), (&f).into(), (&c).into()])?
            .z()?;
        Ok(if ok { Ok(()) } else { Err(ks_error(env, act)?) })
    })?
}

/// A new RSA key inside the Keystore; its public key (SPKI DER).
pub fn ks_generate(app: &AndroidApp, alias: &str) -> Result<Vec<u8>, String> {
    with_activity(app, |env, act| {
        let a = env.new_string(alias)?;
        let r = env.call_method(act, "ksGenerate", "(Ljava/lang/String;)[B", &[(&a).into()])?.l()?;
        Ok(match opt_bytes(env, r)? {
            Some(b) => Ok(b),
            None => Err(ks_error(env, act)?),
        })
    })?
}

/// Sign with the Keystore key `alias` ("SHA-256"…). `Err("auth")`: the user must unlock first.
pub fn ks_sign(app: &AndroidApp, alias: &str, digest: &str, msg: &[u8]) -> Result<Vec<u8>, String> {
    with_activity(app, |env, act| {
        let a = env.new_string(alias)?;
        let d = env.new_string(digest)?;
        let m = env.byte_array_from_slice(msg)?;
        let r = env.call_method(act, "ksSign", "(Ljava/lang/String;Ljava/lang/String;[B)[B", &[(&a).into(), (&d).into(), (&m).into()])?.l()?;
        Ok(match opt_bytes(env, r)? {
            Some(b) => Ok(b),
            None => Err(ks_error(env, act)?),
        })
    })?
}

pub fn ks_has(app: &AndroidApp, alias: &str) -> bool {
    with_activity(app, |env, act| {
        let a = env.new_string(alias)?;
        env.call_method(act, "ksHas", "(Ljava/lang/String;)Z", &[(&a).into()])?.z()
    })
    .unwrap_or(false)
}

pub fn ks_delete(app: &AndroidApp, alias: &str) -> Result<(), String> {
    with_activity(app, |env, act| {
        let a = env.new_string(alias)?;
        let ok = env.call_method(act, "ksDelete", "(Ljava/lang/String;)Z", &[(&a).into()])?.z()?;
        Ok(if ok { Ok(()) } else { Err(ks_error(env, act)?) })
    })?
}

/// Android's file picker for a digital ID file; poll [`take_pending_id_file`].
pub fn pick_id_file(app: &AndroidApp) -> Result<(), String> {
    with_activity(app, |env, act| {
        env.call_method(act, "pickIdFile", "()V", &[])?;
        Ok(())
    })
}

/// (file name, bytes) of a picked digital ID file, once read.
pub fn take_pending_id_file(app: &AndroidApp) -> Option<(String, Vec<u8>)> {
    with_activity(app, |env, act| {
        let n = env.call_method(act, "takePendingIdName", "()Ljava/lang/String;", &[])?.l()?;
        let Some(name) = opt_string(env, n)? else { return Ok(None) };
        let b = env.call_method(act, "takePendingIdBytes", "()[B", &[])?.l()?;
        Ok(opt_bytes(env, b)?.map(|b| (name, b)))
    })
    .ok()
    .flatten()
}

/// "Save to" screen for a copy that must not become the open document (an ID backup). Java
/// deletes `source` afterwards. Poll [`take_copy_result`].
pub fn save_copy_picker(app: &AndroidApp, name: &str, mime: &str, source: &Path) -> Result<(), String> {
    with_activity(app, |env, act| {
        let n = env.new_string(name)?;
        let m = env.new_string(mime)?;
        let s = env.new_string(source.to_string_lossy())?;
        env.call_method(act, "saveCopyPicker", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V", &[(&n).into(), (&m).into(), (&s).into()])?;
        Ok(())
    })
}

/// `Some(Ok(saved name))`, `Some(Err("cancelled" | message))`, or `None` while waiting.
pub fn take_copy_result(app: &AndroidApp) -> Option<Result<String, String>> {
    let r = with_activity(app, |env, act| {
        let r = env.call_method(act, "takeCopyResult", "()Ljava/lang/String;", &[])?.l()?;
        opt_string(env, r)
    })
    .ok()
    .flatten()?;
    Some(match r.strip_prefix('!') {
        Some(e) => Err(e.to_owned()),
        None => Ok(r),
    })
}
