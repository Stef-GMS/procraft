//! iOS file-dialog shim with the exact call shape ProCraft uses from `rfd`:
//! `FileDialog::new().add_filter(..).save_file()/pick_file()`. Every dialog
//! is declined (`None`) because `rfd` has no iOS backend; native document
//! pickers arrive in a later milestone. Original code, MIT like `rfd`.
use std::path::PathBuf;

/// Builder matching the `rfd::FileDialog` calls in `apps/procraft`.
#[derive(Clone, Debug, Default)]
pub struct FileDialog {
    _private: (),
}

impl FileDialog {
    /// Fresh dialog builder.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Accept a file-type filter (ignored: no native dialog yet).
    pub fn add_filter(self, _name: &str, _extensions: &[&str]) -> Self {
        self
    }

    /// Save dialog: always declined on iOS for now.
    pub fn save_file(self) -> Option<PathBuf> {
        None
    }

    /// Open dialog: always declined on iOS for now.
    pub fn pick_file(self) -> Option<PathBuf> {
        None
    }
}
