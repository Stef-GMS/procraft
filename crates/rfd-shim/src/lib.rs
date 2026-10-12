//! ProCraft file dialogs with the `rfd` 0.17 call shape the app uses.
//!
//! - Desktop: this crate is not compiled (real `rfd` is used instead).
//! - iOS: `rfd` has no backend, so this crate backs the same calls with a
//!   `UIDocumentPickerViewController` bridge. Opens use Import mode (single
//!   tap picks; the system copies into the sandbox, so no access claims are
//!   needed). Saves resolve into the sandboxed Documents folder and the app
//!   presents the share sheet afterwards for the real destination.
//!
//! The Objective-C mechanics (window fallback, delegate retention, iPad
//! popover anchor) follow the battle-tested PdfCraft iOS bridge.

use std::path::{Path, PathBuf};

/// Builder matching the `rfd::FileDialog` calls in `apps/procraft`.
#[derive(Clone, Debug, Default)]
pub struct FileDialog {
    filters: Vec<String>,
}

impl FileDialog {
    /// Fresh dialog builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Accept a file-type filter (becomes picker document types on iOS).
    pub fn add_filter(mut self, _name: &str, extensions: &[&str]) -> Self {
        self.filters.extend(extensions.iter().map(|e| e.to_ascii_lowercase()));
        self
    }

    /// Open dialog. Presents the picker and blocks (pumping the runloop)
    /// until the user picks or cancels. `None` on cancel or when there is
    /// no window to present from.
    pub fn pick_file(self) -> Option<PathBuf> {
        #[cfg(target_os = "ios")]
        {
            ios_bridge::pick_import(&self.filters)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = &self.filters;
            None
        }
    }

    /// Save dialog. No native panel: resolves to a uniquified path in the
    /// sandboxed Documents folder. The app writes the bytes itself and then
    /// calls [`ios_share_file`] for the real destination.
    pub fn save_file(self) -> Option<PathBuf> {
        #[cfg(target_os = "ios")]
        {
            ios_bridge::documents_path(&unique_stem())
        }
        #[cfg(not(target_os = "ios"))]
        {
            None
        }
    }
}

/// Present the iOS share sheet for an already-written file (call right after
/// saving, on the UI thread). Returns false when no window is available.
/// iOS only; the desktop `rfd` has no such function (call sites cfg-gate it).
#[cfg(target_os = "ios")]
pub fn ios_share_file(path: &Path) -> bool {
    ios_bridge::share_file(path)
}

/// Timestamped stem so saves never silently overwrite.
fn unique_stem() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("ProCraft-{ms}")
}

#[cfg(target_os = "ios")]
mod ios_bridge;
