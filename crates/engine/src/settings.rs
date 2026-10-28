//! App-wide persisted settings envelope: versioned, serde-based, with
//! forgiving parsing (missing fields default, unknown fields ignored).
//! Pure data (no GUI types).

use serde::{Deserialize, Serialize};

use crate::shortcuts::StoredShortcuts;

/// Schema version written into every settings file.
pub const SETTINGS_VERSION: u32 = 1;

/// Everything ProCraft persists across runs. `#[serde(default)]` makes
/// every field optional on read (missing -> default) and unknown fields
/// are ignored, so older/newer files stay loadable.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Schema version (see SETTINGS_VERSION).
    pub version: u32,
    /// Theme id ("dark", "light", "paper").
    pub theme: String,
    /// View-menu toggles.
    pub show_tools: bool,
    pub show_layers: bool,
    /// Persisted canvas zoom (clamped by the app on load).
    pub zoom: f32,
    /// Recent files, newest first.
    pub recent: Vec<String>,
    /// Sparse shortcut overrides (only non-default entries).
    pub shortcuts: StoredShortcuts,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            version: SETTINGS_VERSION,
            theme: "dark".into(),
            show_tools: true,
            show_layers: true,
            zoom: 1.0,
            recent: Vec::new(),
            shortcuts: StoredShortcuts::default(),
        }
    }
}

impl Settings {
    /// Parse a settings JSON document; Err is a human-readable message.
    pub fn from_json(s: &str) -> Result<Settings, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }

    /// Pretty JSON for on-disk storage (empty string only if serde fails,
    /// which cannot happen for these plain types).
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_pinned() {
        let s = Settings::default();
        assert_eq!(s.version, 1);
        assert_eq!(s.theme, "dark");
        assert!(s.show_tools && s.show_layers);
        assert_eq!(s.zoom, 1.0);
        assert!(s.recent.is_empty());
        assert_eq!(
            s.shortcuts,
            crate::shortcuts::Shortcuts::default().to_stored()
        );
    }

    #[test]
    fn json_round_trip_is_lossless() {
        let s = Settings {
            theme: "dark".into(),
            show_tools: false,
            zoom: 2.5,
            recent: vec!["a.procraft".into(), "b.procraft".into()],
            ..Settings::default()
        };
        let s2 = Settings::from_json(&s.to_json_pretty()).unwrap();
        assert_eq!(s2, s);
    }

    #[test]
    fn partial_and_extra_json_fall_back_to_defaults() {
        let s = Settings::from_json(r#"{"theme":"light","future_field":42}"#).unwrap();
        assert_eq!(s.theme, "light");
        assert_eq!(s.zoom, 1.0); // missing field -> default
        assert!(!s.shortcuts.bindings.contains_key("undo")); // none customized
    }

    #[test]
    fn wrong_types_are_a_clean_error() {
        assert!(Settings::from_json(r#"{"version":"one"}"#).is_err());
        assert!(Settings::from_json("not json").is_err());
    }
}
