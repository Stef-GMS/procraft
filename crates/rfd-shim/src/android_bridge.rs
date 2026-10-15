//! Android file bridge: app-private external files + media scan.
//!
//! `NativeActivity` has no activity-result plumbing, so there is no system
//! picker here: opens list this dir in-app (USB/`adb push` drop point) and
//! saves land here, then get media-scanned into shared collections. No
//! storage permission is needed for either direction.

use std::path::{Path, PathBuf};

use jni::objects::{JObject, JString, JValue};
use jni::{jni_sig, jni_str};

/// Run `f` with the current thread attached to the JVM.
fn with_attached<T>(
    f: impl for<'e> FnOnce(&mut jni::Env<'e>) -> jni::errors::Result<T>,
) -> jni::errors::Result<T> {
    let ctx = ndk_context::android_context();
    // SAFETY: android-activity associates a JVM + activity before `android_main`.
    if ctx.vm().is_null() || ctx.context().is_null() {
        return Err(jni::errors::Error::NullPtr("no Android context"));
    }
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    vm.attach_current_thread(f)
}

/// App-private external files dir (visible over USB MTP).
/// Errors carry the reason for the status line.
pub fn external_files_dir() -> Result<PathBuf, &'static str> {
    with_attached(|env| {
        let ctx = ndk_context::android_context();
        let activity = unsafe { JObject::from_raw(env, ctx.context() as jni::sys::jobject) };
        let dir: JObject = env
            .call_method(
                &activity,
                jni_str!("getExternalFilesDir"),
                jni_sig!("(Ljava/lang/String;)Ljava/io/File;"),
                &[JValue::Object(&JObject::null())],
            )?
            .l()?;
        if dir.is_null() {
            return Err(jni::errors::Error::NullPtr("no external files dir"));
        }
        let raw: JObject = env
            .call_method(
                &dir,
                jni_str!("getAbsolutePath"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let js = unsafe { JString::from_raw(env, raw.as_raw() as jni::sys::jstring) };
        let s = js.try_to_string(env)?;
        Ok(PathBuf::from(s))
    })
    .map_err(|_| "external storage unavailable")
}

/// Media-scan an already-written file so Photos/file managers pick it up.
/// Best-effort: false only means "not indexed", the file is still saved.
pub fn scan_file(path: &Path) -> bool {
    let s = match path.to_str() {
        Some(s) => s,
        None => return false,
    };
    with_attached(|env| {
        let ctx = ndk_context::android_context();
        let activity = unsafe { JObject::from_raw(env, ctx.context() as jni::sys::jobject) };
        let jpath = env.new_string(s)?;
        let arr = env.new_object_array(1, jni_str!("java/lang/String"), &jpath)?;
        env.call_static_method(
            jni_str!("android/media/MediaScannerConnection"),
            jni_str!("scanFile"),
            jni_sig!("(Landroid/content/Context;[Ljava/lang/String;[Ljava/lang/String;Landroid/media/MediaScannerConnection$OnConnectionCompletedListener;)V"),
            &[
                JValue::Object(&activity),
                JValue::Object(&arr),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
            ],
        )?;
        Ok(())
    })
    .is_ok()
}
