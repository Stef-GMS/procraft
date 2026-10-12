//! iOS backing: real `UIDocumentPickerViewController` flows.
//!
//! Opens use Import mode (single tap picks; the system copies the file into
//! the sandbox, so no security-scoped access claims are needed). The picker
//! is presented eagerly on the calling (UI) thread; the synchronous API then
//! pumps the main runloop until the delegate resolves or cancels.
//!
//! The delegate class carries no state: pending picks live in a global map
//! keyed by delegate address (plain `usize`, so the map stays `Send`), and
//! the delegate itself is retained manually with `into_raw` / `from_raw`
//! because `setDelegate` is weak.

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
                unsafe { drop(Retained::<PickerDelegate>::from_raw(key as *mut PickerDelegate)) };
                shared.lock().unwrap().done = Some(paths);
            }
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn documentPickerWasCancelled(&self, _controller: &UIDocumentPickerViewController) {
            let key = self as *const Self as usize;
            if let Some(shared) = pending().lock().unwrap().remove(&key) {
                unsafe { drop(Retained::<PickerDelegate>::from_raw(key as *mut PickerDelegate)) };
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

/// The app's root view controller for presenting modal UI. `keyWindow` is
/// scene-deprecated and often nil, so fall back to any key window, then any
/// window at all.
fn find_root() -> Result<Retained<UIViewController>, &'static str> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err("no-main-thread");
    };
    let app = UIApplication::sharedApplication(mtm);
    let window: Retained<UIWindow> = match app.keyWindow() {
        Some(w) => w,
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
            match found {
                Some(w) => w,
                None => return Err("no-window"),
            }
        }
    };
    window.rootViewController().ok_or("no-root-vc")
}

/// Present an Import-mode picker for `doc_types` (UTIs). Runs on the calling
/// (UI) thread. Returns the shared cell the caller pumps until resolved.
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

/// Pump the main runloop in short slices until the delegate resolves the
/// pick. The delegate callback needs the runloop, so a plain thread-park
/// here would deadlock.
fn pump_until_done(shared: &SharedCell) {
    use objc2_foundation::{NSDate, NSRunLoop};
    loop {
        if shared.lock().unwrap().done.is_some() {
            break;
        }
        let runloop = NSRunLoop::currentRunLoop();
        let date = NSDate::dateWithTimeIntervalSinceNow(0.05);
        runloop.runUntilDate(&date);
    }
}

/// Synchronous Import-mode pick. Presents now (UI thread) and pumps until
/// the user picks or cancels.
pub(super) fn pick_import(filters: &[String]) -> Option<PathBuf> {
    let shared = new_shared();
    if present(&utis_for_filters(filters), &shared).is_err() {
        return None;
    }
    pump_until_done(&shared);
    let mut s = shared.lock().unwrap();
    s.done.take().and_then(|mut v| v.pop())
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
