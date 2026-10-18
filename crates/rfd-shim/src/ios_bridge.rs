//! iOS backing: real `UIDocumentPickerViewController` flows.
//!
//! Opens use Import mode (single tap picks; the system copies the file into
//! the sandbox, so no security-scoped access claims are needed). The picker
//! is presented eagerly on the calling (UI) thread and the result is polled
//! via [`poll`] — the caller never blocks, so the watchdog stays happy.
//!
//! The delegate class carries no state: pending picks live in a global map
//! keyed by delegate address (plain `usize`, so the map stays `Send`), and
//! the delegate itself is retained manually with `into_raw` / `from_raw`
//! because `setDelegate` is weak.
//!
//! Lint notes: `deprecated` (type-based picker init + UIWindow access work
//! on the deployment target; replacements need newer API) and
//! `non_snake_case` (ObjC delegate selectors must keep exact names) are
//! allowed for this bridge module.
#![allow(deprecated, non_snake_case)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSArray, NSMutableArray, NSObject, NSString, NSURL};
use objc2_ui_kit::{
    UIActivityViewController, UIApplication, UIDocumentPickerDelegate, UIDocumentPickerMode,
    UIDocumentPickerViewController, UIViewController, UIWindow,
};

struct Shared {
    done: Option<Vec<PathBuf>>,
}

type SharedCell = std::sync::Arc<Mutex<Shared>>;

static PENDING: OnceLock<Mutex<HashMap<usize, SharedCell>>> = OnceLock::new();

fn pending() -> &'static Mutex<HashMap<usize, SharedCell>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// At most one pick in flight (a second tap while the picker is up keeps
/// the first request instead of orphaning delegates).
static SLOT: OnceLock<Mutex<Option<Slot>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Slot>> {
    SLOT.get_or_init(|| Mutex::new(None))
}

struct Slot {
    shared: SharedCell,
    /// Presentation failure reason (resolves immediately at next poll).
    error: Option<&'static str>,
    presented_at: std::time::Instant,
}

/// Seconds before a pick with no delegate callback resolves as cancelled
/// (e.g. the picker landed on a hidden window).
const PICK_TIMEOUT_SECS: u64 = 60;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ProCraftPickerDelegate"]
    struct PickerDelegate;

    unsafe impl NSObjectProtocol for PickerDelegate {}

    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn documentPicker_didPickDocumentsAtURLs(
            &self,
            _controller: &UIDocumentPickerViewController,
            urls: &NSArray<NSURL>,
        ) {
            let key = self as *const Self as usize;
            if let Some(shared) = pending().lock().unwrap().remove(&key) {
                let mut paths = Vec::new();
                for i in 0..urls.len() {
                    let url = urls.objectAtIndex(i);
                    // Import mode already copied into the sandbox: readable.
                    if let Some(ns) = url.path() {
                        let p = PathBuf::from(ns.to_string());
                        if p.exists() {
                            paths.push(p);
                        }
                    }
                }
                // Balance the manual retain from presentation.
                unsafe {
                    drop(Retained::<PickerDelegate>::from_raw(
                        key as *mut PickerDelegate,
                    ))
                };
                shared.lock().unwrap().done = Some(paths);
            }
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn documentPickerWasCancelled(&self, _controller: &UIDocumentPickerViewController) {
            let key = self as *const Self as usize;
            if let Some(shared) = pending().lock().unwrap().remove(&key) {
                unsafe {
                    drop(Retained::<PickerDelegate>::from_raw(
                        key as *mut PickerDelegate,
                    ))
                };
                shared.lock().unwrap().done = Some(Vec::new());
            }
        }
    }
);

/// Extension → UTI for the picker. Unknown types fall back to `public.item`
/// (everything) so no file the app could convert is ever hidden.
fn uti_for_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "public.png",
        "jpg" | "jpeg" => "public.jpeg",
        "gif" => "com.compuserve.gif",
        "tif" | "tiff" => "public.tiff",
        "bmp" => "com.microsoft.bmp",
        "webp" => "org.webmproject.webp",
        "pdf" => "com.adobe.pdf",
        // M38 fonts: a real font UTI so the Files browser offers
        // typefaces (falling back to public.item hid nothing, but some
        // providers filter dimmer without an exact type).
        "ttf" | "otf" | "ttc" => "public.font",
        _ => "public.item",
    }
}

fn utis_for_filters(filters: &[String]) -> Vec<String> {
    let mut out: Vec<String> = filters.iter().map(|e| uti_for_ext(e).to_string()).collect();
    out.sort();
    out.dedup();
    if out.is_empty() {
        out.push("public.item".to_string());
    }
    out
}

/// The app's window. `keyWindow` is scene-deprecated and often nil, so fall
/// back to any key window, then any window at all.
fn find_window() -> Result<Retained<UIWindow>, &'static str> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err("no-main-thread");
    };
    let app = UIApplication::sharedApplication(mtm);
    match app.keyWindow() {
        Some(w) => Ok(w),
        None => {
            let windows = app.windows();
            let mut found: Option<Retained<UIWindow>> = None;
            for i in 0..windows.len() {
                let w = windows.objectAtIndex(i);
                if w.isKeyWindow() {
                    found = Some(w);
                    break;
                }
                if found.is_none() {
                    found = Some(w);
                }
            }
            found.ok_or("no-window")
        }
    }
}

/// The app's root view controller for presenting modal UI.
fn find_root() -> Result<Retained<UIViewController>, &'static str> {
    find_window()?.rootViewController().ok_or("no-root-vc")
}

/// Window safe-area insets (top, bottom) in points for edge-to-edge padding.
///
/// iPadOS 26 regression family: `safeAreaInsets` can go stale after a
/// rotation, leaving the top bar under the status bar. The status bar
/// manager's frame stays fresh, so while the bar is visible the top inset
/// is the larger of the two — in portrait they agree, so the max can only
/// restore a stale value, never inflate a correct one.
pub(super) fn safe_area_insets() -> Result<(u32, u32), &'static str> {
    let window = find_window()?;
    let insets = window.safeAreaInsets();
    let mut top = insets.top;
    if let Some(manager) = window
        .windowScene()
        .and_then(|scene| scene.statusBarManager())
    {
        if !manager.isStatusBarHidden() {
            top = top.max(manager.statusBarFrame().size.height);
        }
    }
    Ok((
        top.max(0.0).round() as u32,
        insets.bottom.max(0.0).round() as u32,
    ))
}

/// Present an Import-mode picker for `doc_types` (UTIs). Runs on the calling
/// (UI) thread.
fn present(doc_types: &[String], shared: &SharedCell) -> Result<(), &'static str> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err("no-main-thread");
    };
    let root = find_root()?;
    let types = NSMutableArray::<NSString>::new();
    for t in doc_types {
        types.addObject(&NSString::from_str(t));
    }
    let picker = UIDocumentPickerViewController::initWithDocumentTypes_inMode(
        UIDocumentPickerViewController::alloc(mtm),
        &types,
        UIDocumentPickerMode::Import,
    );
    picker.setAllowsMultipleSelection(false);
    let delegate: Retained<PickerDelegate> = unsafe { msg_send![PickerDelegate::class(), new] };
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let raw = Retained::into_raw(delegate) as usize;
    pending().lock().unwrap().insert(raw, shared.clone());
    root.presentViewController_animated_completion(&picker, true, None);
    Ok(())
}

fn new_shared() -> SharedCell {
    std::sync::Arc::new(Mutex::new(Shared { done: None }))
}

/// Called on the UI thread. Presents the picker now and stashes the
/// pending pick for [`poll`]. A second call while one is in flight keeps
/// the first request. Presentation failures stash an error that resolves
/// at the next poll instead of failing silently.
pub(super) fn present_import(filters: &[String]) {
    let mut slot = slot().lock().unwrap();
    if slot.is_some() {
        return;
    }
    let shared = new_shared();
    match present(&utis_for_filters(filters), &shared) {
        Ok(()) => {
            *slot = Some(Slot {
                shared,
                error: None,
                presented_at: std::time::Instant::now(),
            });
        }
        Err(reason) => {
            *slot = Some(Slot {
                shared,
                error: Some(reason),
                presented_at: std::time::Instant::now(),
            });
        }
    }
}

/// Poll a pending pick: waiting, picked, or cancelled (user cancel,
/// presentation failure, or 60s without a delegate callback).
pub(super) fn poll() -> super::PickPoll {
    let mut slot = slot().lock().unwrap();
    let Some(inner) = slot.as_ref() else {
        return super::PickPoll::NoRequest;
    };
    if let Some(reason) = inner.error {
        *slot = None;
        return super::PickPoll::Cancelled(Some(reason));
    }
    let done = inner.shared.lock().unwrap().done.take();
    match done {
        Some(mut paths) => {
            *slot = None;
            match paths.pop() {
                Some(p) => super::PickPoll::Picked(p),
                None => super::PickPoll::Cancelled(None),
            }
        }
        None => {
            if inner.presented_at.elapsed().as_secs() >= PICK_TIMEOUT_SECS {
                *slot = None;
                super::PickPoll::Cancelled(Some("picker timed out"))
            } else {
                super::PickPoll::Waiting
            }
        }
    }
}

/// App sandbox Documents directory (visible in Files with
/// `UIFileSharingEnabled`). Created on demand.
fn sandbox_documents() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let docs = PathBuf::from(home).join("Documents");
    std::fs::create_dir_all(&docs).ok()?;
    Some(docs)
}

/// Resolve `stem` into Documents, uniquified so saves never overwrite.
pub(super) fn documents_path(stem: &str) -> Option<PathBuf> {
    let docs = sandbox_documents()?;
    if !docs.join(stem).exists() {
        return Some(docs.join(stem));
    }
    for n in 2..1000 {
        let candidate = format!("{stem} ({n})");
        if !docs.join(&candidate).exists() {
            return Some(docs.join(candidate));
        }
    }
    Some(docs.join(stem))
}

/// Present the iOS share sheet for an already-written file. Must run on the
/// UI thread (the app calls this right after saving). Returns false when
/// there is no window; on iPad the sheet is anchored to the root view.
pub(super) fn share_file(path: &std::path::Path) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Ok(root) = find_root() else {
        return false;
    };
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let items = NSMutableArray::<objc2::runtime::AnyObject>::new();
    items.addObject(&url);
    let sheet = unsafe {
        UIActivityViewController::initWithActivityItems_applicationActivities(
            UIActivityViewController::alloc(mtm),
            &items,
            None,
        )
    };
    if let Some(popover) = sheet.popoverPresentationController() {
        let Some(view) = root.view() else {
            return false;
        };
        popover.setSourceView(Some(&view));
        popover.setSourceRect(view.bounds());
    }
    root.presentViewController_animated_completion(&sheet, true, None);
    true
}
