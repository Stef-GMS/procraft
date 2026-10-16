//! M46 import-browser helpers: extension matching and file-size labels.
//! Pure functions (no GUI types) so the Android in-app browser shares
//! exactly the logic the host tests pin down.

/// Case-insensitive extension match against dialog-style filters
/// (`["png", "jpg"]`). `None`/empty never matches, and there is no
/// aliasing (`jpeg != jpg`) — exactly the `android_import_list` rule.
pub fn ext_matches(ext: Option<&str>, filters: &[&str]) -> bool {
    match ext {
        Some(e) if !e.is_empty() => filters.iter().any(|f| f.eq_ignore_ascii_case(e)),
        _ => false,
    }
}

/// Short size label for the browser rows: bytes under 1 KiB, one-decimal
/// KB/MB above (1024-based, matching the storage settings convention).
pub fn file_size_label(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_matches_is_case_insensitive() {
        assert!(ext_matches(Some("png"), &["png", "jpg"]));
        assert!(ext_matches(Some("PNG"), &["png", "jpg"]));
        assert!(ext_matches(Some("Jpg"), &["png", "jpg"]));
        assert!(!ext_matches(Some("gif"), &["png", "jpg"]));
    }

    #[test]
    fn ext_matches_rejects_missing_and_empty() {
        assert!(!ext_matches(None, &["png"]));
        assert!(!ext_matches(Some(""), &["png"]));
        assert!(!ext_matches(Some("png"), &[]));
        // No aliasing: jpeg != jpg, exactly like the dialog filters.
        assert!(!ext_matches(Some("jpeg"), &["jpg"]));
    }

    #[test]
    fn file_size_label_boundaries() {
        assert_eq!(file_size_label(0), "0 B");
        assert_eq!(file_size_label(512), "512 B");
        assert_eq!(file_size_label(1023), "1023 B");
        assert_eq!(file_size_label(1024), "1.0 KB");
        assert_eq!(file_size_label(1536), "1.5 KB");
        assert_eq!(file_size_label(1024 * 1024), "1.0 MB");
    }
}
