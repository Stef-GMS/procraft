//! User-remappable keyboard shortcuts: layout-independent chord keys,
//! action ids with pinned defaults, exact-match conflict lookup, and a
//! sparse persisted form. Pure data + logic (no GUI types) so host tests
//! pin exactly what the app dispatches.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Layout-independent key identity: letters, digits, function keys,
/// navigation/editing keys and punctuation. Display names are short so
/// the settings UI can render "Ctrl+Shift+Z" or just "[".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChordKey {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Space,
    Tab,
    Comma,
    Period,
    Slash,
    Semicolon,
    Quote,
    Minus,
    Equals,
    OpenBracket,
    CloseBracket,
    Backslash,
    Backtick,
}

impl ChordKey {
    /// Display name shown in the shortcuts UI ("B", "F5", "Up", "[").
    pub fn name(&self) -> &'static str {
        match self {
            ChordKey::A => "A",
            ChordKey::B => "B",
            ChordKey::C => "C",
            ChordKey::D => "D",
            ChordKey::E => "E",
            ChordKey::F => "F",
            ChordKey::G => "G",
            ChordKey::H => "H",
            ChordKey::I => "I",
            ChordKey::J => "J",
            ChordKey::K => "K",
            ChordKey::L => "L",
            ChordKey::M => "M",
            ChordKey::N => "N",
            ChordKey::O => "O",
            ChordKey::P => "P",
            ChordKey::Q => "Q",
            ChordKey::R => "R",
            ChordKey::S => "S",
            ChordKey::T => "T",
            ChordKey::U => "U",
            ChordKey::V => "V",
            ChordKey::W => "W",
            ChordKey::X => "X",
            ChordKey::Y => "Y",
            ChordKey::Z => "Z",
            ChordKey::Num0 => "0",
            ChordKey::Num1 => "1",
            ChordKey::Num2 => "2",
            ChordKey::Num3 => "3",
            ChordKey::Num4 => "4",
            ChordKey::Num5 => "5",
            ChordKey::Num6 => "6",
            ChordKey::Num7 => "7",
            ChordKey::Num8 => "8",
            ChordKey::Num9 => "9",
            ChordKey::F1 => "F1",
            ChordKey::F2 => "F2",
            ChordKey::F3 => "F3",
            ChordKey::F4 => "F4",
            ChordKey::F5 => "F5",
            ChordKey::F6 => "F6",
            ChordKey::F7 => "F7",
            ChordKey::F8 => "F8",
            ChordKey::F9 => "F9",
            ChordKey::F10 => "F10",
            ChordKey::F11 => "F11",
            ChordKey::F12 => "F12",
            ChordKey::ArrowUp => "Up",
            ChordKey::ArrowDown => "Down",
            ChordKey::ArrowLeft => "Left",
            ChordKey::ArrowRight => "Right",
            ChordKey::Home => "Home",
            ChordKey::End => "End",
            ChordKey::PageUp => "PgUp",
            ChordKey::PageDown => "PgDn",
            ChordKey::Insert => "Ins",
            ChordKey::Space => "Space",
            ChordKey::Tab => "Tab",
            ChordKey::Comma => ",",
            ChordKey::Period => ".",
            ChordKey::Slash => "/",
            ChordKey::Semicolon => ";",
            ChordKey::Quote => "'",
            ChordKey::Minus => "-",
            ChordKey::Equals => "=",
            ChordKey::OpenBracket => "[",
            ChordKey::CloseBracket => "]",
            ChordKey::Backslash => "\\",
            ChordKey::Backtick => "`",
        }
    }
}

/// One key plus its modifier state, compared exactly (no logical matching:
/// Ctrl+Shift+Z never matches Ctrl+Z).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyChord {
    pub key: ChordKey,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyChord {
    pub const fn new(key: ChordKey, ctrl: bool, shift: bool, alt: bool) -> Self {
        KeyChord {
            key,
            ctrl,
            shift,
            alt,
        }
    }

    /// Exact equality on all four fields.
    pub fn matches(&self, key: ChordKey, ctrl: bool, shift: bool, alt: bool) -> bool {
        self.key == key && self.ctrl == ctrl && self.shift == shift && self.alt == alt
    }

    /// UI form: "Ctrl+Shift+Z" (prefix order Ctrl, Shift, Alt), "B".
    pub fn format(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s.push_str(self.key.name());
        s
    }
}

/// A modifier combination on its own (used for drag gestures).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModSet {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl ModSet {
    /// The default brush-size drag modifier.
    pub const SHIFT: ModSet = ModSet {
        ctrl: false,
        shift: true,
        alt: false,
    };

    /// No modifier held.
    pub fn is_empty(&self) -> bool {
        !self.ctrl && !self.shift && !self.alt
    }

    /// Exact equality on all three fields.
    pub fn matches(&self, ctrl: bool, shift: bool, alt: bool) -> bool {
        self.ctrl == ctrl && self.shift == shift && self.alt == alt
    }
}

/// The editable text tools the app dispatches to, in toolbar order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolId {
    Brush,
    Eraser,
    Smudge,
    Select,
    Lasso,
    Fill,
    Picker,
    Move,
    Hand,
    Liquify,
    Crop,
    Shapes,
    Text,
}

impl ToolId {
    /// Stable persistence fragment ("brush", "eraser", ...).
    fn key(&self) -> &'static str {
        match self {
            ToolId::Brush => "brush",
            ToolId::Eraser => "eraser",
            ToolId::Smudge => "smudge",
            ToolId::Select => "select",
            ToolId::Lasso => "lasso",
            ToolId::Fill => "fill",
            ToolId::Picker => "picker",
            ToolId::Move => "move",
            ToolId::Hand => "hand",
            ToolId::Liquify => "liquify",
            ToolId::Crop => "crop",
            ToolId::Shapes => "shapes",
            ToolId::Text => "text",
        }
    }

    /// Display name ("Brush", "Lasso", ...).
    pub fn label(&self) -> &'static str {
        match self {
            ToolId::Brush => "Brush",
            ToolId::Eraser => "Eraser",
            ToolId::Smudge => "Smudge",
            ToolId::Select => "Select",
            ToolId::Lasso => "Lasso",
            ToolId::Fill => "Fill",
            ToolId::Picker => "Picker",
            ToolId::Move => "Move",
            ToolId::Hand => "Hand",
            ToolId::Liquify => "Liquify",
            ToolId::Crop => "Crop",
            ToolId::Shapes => "Shapes",
            ToolId::Text => "Text",
        }
    }
}

/// Everything a keyboard shortcut can trigger, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionId {
    Tool(ToolId),
    Undo,
    Redo,
    RedoAlt,
}

impl ActionId {
    /// All actions in display order (tools first, then history actions).
    pub const ALL: [ActionId; 16] = [
        ActionId::Tool(ToolId::Brush),
        ActionId::Tool(ToolId::Eraser),
        ActionId::Tool(ToolId::Smudge),
        ActionId::Tool(ToolId::Select),
        ActionId::Tool(ToolId::Lasso),
        ActionId::Tool(ToolId::Fill),
        ActionId::Tool(ToolId::Picker),
        ActionId::Tool(ToolId::Move),
        ActionId::Tool(ToolId::Hand),
        ActionId::Tool(ToolId::Liquify),
        ActionId::Tool(ToolId::Crop),
        ActionId::Tool(ToolId::Shapes),
        ActionId::Tool(ToolId::Text),
        ActionId::Undo,
        ActionId::Redo,
        ActionId::RedoAlt,
    ];

    /// Stable persistence key ("tool.brush", "undo", "redo_alt").
    pub fn to_key(&self) -> String {
        match self {
            ActionId::Tool(t) => format!("tool.{}", t.key()),
            ActionId::Undo => "undo".into(),
            ActionId::Redo => "redo".into(),
            ActionId::RedoAlt => "redo_alt".into(),
        }
    }

    /// Inverse of to_key; None for unknown persisted keys.
    pub fn from_key(k: &str) -> Option<ActionId> {
        ActionId::ALL.into_iter().find(|a| a.to_key() == k)
    }

    /// Display name ("Brush", ..., "Redo (alt combo)").
    pub fn label(&self) -> String {
        match self {
            ActionId::Tool(t) => t.label().to_string(),
            ActionId::Undo => "Undo".into(),
            ActionId::Redo => "Redo".into(),
            ActionId::RedoAlt => "Redo (alt combo)".into(),
        }
    }
}

/// Drag gesture that carries a modifier combo instead of a chord.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragActionId {
    BrushSize,
}

impl DragActionId {
    /// Stable persistence key.
    pub fn to_key(&self) -> &'static str {
        match self {
            DragActionId::BrushSize => "brush_size",
        }
    }

    /// Inverse of to_key; None for unknown persisted keys.
    pub fn from_key(k: &str) -> Option<DragActionId> {
        match k {
            "brush_size" => Some(DragActionId::BrushSize),
            _ => None,
        }
    }
}

/// Direction a canvas size-drag measures its travel along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeDragMode {
    /// Left–right travel: right grows, left shrinks.
    #[default]
    Horizontal,
    /// Up–down travel: up grows, down shrinks.
    Vertical,
    /// Straight-line distance from the drag's starting point: any
    /// direction grows; within one gesture the size never drops below
    /// where it started.
    Distance,
}

impl SizeDragMode {
    /// Stable persistence key.
    pub fn to_key(&self) -> &'static str {
        match self {
            SizeDragMode::Horizontal => "horizontal",
            SizeDragMode::Vertical => "vertical",
            SizeDragMode::Distance => "distance",
        }
    }

    /// Inverse of to_key; None for unknown persisted keys.
    pub fn from_key(k: &str) -> Option<SizeDragMode> {
        match k {
            "horizontal" => Some(SizeDragMode::Horizontal),
            "vertical" => Some(SizeDragMode::Vertical),
            "distance" => Some(SizeDragMode::Distance),
            _ => None,
        }
    }
}

fn default_bindings() -> BTreeMap<ActionId, Option<KeyChord>> {
    let mut m = BTreeMap::new();
    for a in ActionId::ALL {
        m.insert(a, None);
    }
    m.insert(
        ActionId::Tool(ToolId::Brush),
        Some(KeyChord::new(ChordKey::B, false, false, false)),
    );
    m.insert(
        ActionId::Tool(ToolId::Eraser),
        Some(KeyChord::new(ChordKey::E, false, false, false)),
    );
    m.insert(
        ActionId::Tool(ToolId::Fill),
        Some(KeyChord::new(ChordKey::F, false, false, false)),
    );
    m.insert(
        ActionId::Undo,
        Some(KeyChord::new(ChordKey::Z, true, false, false)),
    );
    m.insert(
        ActionId::Redo,
        Some(KeyChord::new(ChordKey::Y, true, false, false)),
    );
    m.insert(
        ActionId::RedoAlt,
        Some(KeyChord::new(ChordKey::Z, true, true, false)),
    );
    m
}

fn default_drag() -> BTreeMap<DragActionId, ModSet> {
    let mut m = BTreeMap::new();
    m.insert(DragActionId::BrushSize, ModSet::SHIFT);
    m
}

/// The live shortcut table: every action is always present; a `None`
/// binding is an explicit "unbound". Lookups and conflicts are exact
/// (all modifiers must match).
#[derive(Clone, PartialEq, Debug)]
pub struct Shortcuts {
    bindings: BTreeMap<ActionId, Option<KeyChord>>,
    drag: BTreeMap<DragActionId, ModSet>,
    /// Direction the brush-size drag measures travel along.
    size_drag_mode: SizeDragMode,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Shortcuts {
            bindings: default_bindings(),
            drag: default_drag(),
            size_drag_mode: SizeDragMode::default(),
        }
    }
}

impl Shortcuts {
    /// Chord bound to `a`; None = unbound.
    pub fn get(&self, a: ActionId) -> Option<KeyChord> {
        self.bindings.get(&a).copied().flatten()
    }

    /// The action currently holding `chord`, if any (exact match).
    pub fn owner_of(&self, chord: KeyChord) -> Option<ActionId> {
        self.bindings
            .iter()
            .find(|(_, v)| **v == Some(chord))
            .map(|(a, _)| *a)
    }

    /// Bind `a` to `chord`. Refuses (Err = current owner) when another
    /// action already holds it; nothing changes on refusal.
    pub fn set_binding(&mut self, a: ActionId, chord: KeyChord) -> Result<(), ActionId> {
        match self.owner_of(chord) {
            Some(owner) if owner != a => Err(owner),
            _ => {
                self.bindings.insert(a, Some(chord));
                Ok(())
            }
        }
    }

    /// Unbind `a` (persists as an explicit null).
    pub fn clear_binding(&mut self, a: ActionId) {
        self.bindings.insert(a, None);
    }

    /// Restore every action binding and drag modifier to its default.
    pub fn reset_all(&mut self) {
        self.bindings = default_bindings();
        self.drag = default_drag();
        self.size_drag_mode = SizeDragMode::default();
    }

    /// Modifier combo for drag gesture `d`.
    pub fn drag_get(&self, d: DragActionId) -> ModSet {
        self.drag.get(&d).copied().unwrap_or(ModSet::SHIFT)
    }

    /// Set the modifier combo for drag gesture `d`.
    pub fn set_drag(&mut self, d: DragActionId, set: ModSet) {
        self.drag.insert(d, set);
    }

    /// Direction the brush-size drag measures its travel along.
    pub fn size_drag_mode(&self) -> SizeDragMode {
        self.size_drag_mode
    }

    /// Set the brush-size drag direction.
    pub fn set_size_drag_mode(&mut self, mode: SizeDragMode) {
        self.size_drag_mode = mode;
    }

    /// Sparse persisted form: only entries differing from the defaults
    /// (a cleared default persists as an explicit null).
    pub fn to_stored(&self) -> StoredShortcuts {
        let base = Shortcuts::default();
        let mut bindings = BTreeMap::new();
        for (a, v) in &self.bindings {
            if *v != base.bindings.get(a).copied().flatten() {
                bindings.insert(a.to_key(), *v);
            }
        }
        let mut drag = BTreeMap::new();
        for (d, s) in &self.drag {
            if base.drag.get(d).copied() != Some(*s) {
                drag.insert(d.to_key().to_string(), *s);
            }
        }
        let size_drag_mode = (self.size_drag_mode != SizeDragMode::default())
            .then(|| self.size_drag_mode.to_key().to_string());
        StoredShortcuts {
            bindings,
            drag,
            size_drag_mode,
        }
    }

    /// Rebuild from the sparse form: defaults first, then apply entries in
    /// display order (the first claimant keeps a duplicated chord, later
    /// ones are skipped), unknown keys are skipped. The warning strings
    /// are human-readable notes for the settings status line.
    pub fn from_stored(s: &StoredShortcuts) -> (Shortcuts, Vec<String>) {
        let mut out = Shortcuts::default();
        let mut warns = Vec::new();
        for a in ActionId::ALL {
            let Some(v) = s.bindings.get(&a.to_key()) else {
                continue;
            };
            match v {
                Some(chord) => {
                    if let Some(owner) = out.owner_of(*chord) {
                        if owner != a {
                            warns.push(format!(
                                "Shortcut '{}' ignored: {} is already used by '{}'",
                                a.label(),
                                chord.format(),
                                owner.label()
                            ));
                        }
                    } else {
                        out.bindings.insert(a, Some(*chord));
                    }
                }
                None => {
                    out.bindings.insert(a, None);
                }
            }
        }
        for k in s.bindings.keys() {
            if ActionId::from_key(k).is_none() {
                warns.push(format!("Shortcut entry '{}' ignored: unknown action", k));
            }
        }
        for (k, v) in &s.drag {
            match DragActionId::from_key(k) {
                Some(d) => {
                    // An empty set matches every plain drag — never let a
                    // hand-edited file hijack painting.
                    if v.is_empty() {
                        warns.push(format!(
                            "Drag modifier for '{}' ignored: needs at least one modifier",
                            k
                        ));
                    } else {
                        out.drag.insert(d, *v);
                    }
                }
                None => warns.push(format!(
                    "Drag modifier entry '{}' ignored: unknown action",
                    k
                )),
            }
        }
        if let Some(k) = &s.size_drag_mode {
            match SizeDragMode::from_key(k) {
                Some(m) => out.size_drag_mode = m,
                None => warns.push(format!("Drag direction '{}' ignored: unknown mode", k)),
            }
        }
        (out, warns)
    }
}

/// Sparse persisted form of [`Shortcuts`]. Only non-default entries are
/// stored; a null binding value means "explicitly unbound".
#[derive(Default, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct StoredShortcuts {
    #[serde(default)]
    pub bindings: BTreeMap<String, Option<KeyChord>>,
    #[serde(default)]
    pub drag: BTreeMap<String, ModSet>,
    /// Brush-size drag direction; absent/None = horizontal (the default).
    #[serde(default)]
    pub size_drag_mode: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_pin_the_agreed_bindings() {
        let s = Shortcuts::default();
        assert_eq!(
            s.get(ActionId::Tool(ToolId::Brush)),
            Some(KeyChord::new(ChordKey::B, false, false, false))
        );
        assert_eq!(
            s.get(ActionId::Tool(ToolId::Eraser)),
            Some(KeyChord::new(ChordKey::E, false, false, false))
        );
        assert_eq!(
            s.get(ActionId::Tool(ToolId::Fill)),
            Some(KeyChord::new(ChordKey::F, false, false, false))
        );
        assert_eq!(
            s.get(ActionId::Undo),
            Some(KeyChord::new(ChordKey::Z, true, false, false))
        );
        assert_eq!(
            s.get(ActionId::Redo),
            Some(KeyChord::new(ChordKey::Y, true, false, false))
        );
        assert_eq!(
            s.get(ActionId::RedoAlt),
            Some(KeyChord::new(ChordKey::Z, true, true, false))
        );
        assert_eq!(s.get(ActionId::Tool(ToolId::Lasso)), None);
        assert_eq!(s.drag_get(DragActionId::BrushSize), ModSet::SHIFT);
    }

    #[test]
    fn exact_match_redoalt_is_not_undo() {
        // The latent bug: logical matching would let Ctrl+Shift+Z match Ctrl+Z.
        let ctrl_z = KeyChord::new(ChordKey::Z, true, false, false);
        assert!(!ctrl_z.matches(ChordKey::Z, true, true, false));
        let s = Shortcuts::default();
        assert_eq!(
            s.owner_of(KeyChord::new(ChordKey::Z, true, true, false)),
            Some(ActionId::RedoAlt)
        );
    }

    #[test]
    fn conflict_refuses_and_names_owner() {
        let mut s = Shortcuts::default();
        let err = s.set_binding(
            ActionId::RedoAlt,
            KeyChord::new(ChordKey::Z, true, false, false),
        );
        assert_eq!(err, Err(ActionId::Undo));
        assert_eq!(
            s.get(ActionId::RedoAlt),
            Some(KeyChord::new(ChordKey::Z, true, true, false))
        );
    }

    #[test]
    fn clear_reset_and_unbound_survive() {
        let mut s = Shortcuts::default();
        s.clear_binding(ActionId::Undo);
        assert_eq!(s.get(ActionId::Undo), None);
        s.set_binding(
            ActionId::Redo,
            KeyChord::new(ChordKey::G, false, false, false),
        )
        .unwrap();
        s.reset_all();
        assert_eq!(
            s.get(ActionId::Undo),
            Some(KeyChord::new(ChordKey::Z, true, false, false))
        );
        assert_eq!(
            s.get(ActionId::Redo),
            Some(KeyChord::new(ChordKey::Y, true, false, false))
        );
    }

    #[test]
    fn stored_form_is_sparse_and_round_trips() {
        let mut s = Shortcuts::default();
        assert!(s.to_stored().bindings.is_empty(), "defaults store nothing");
        s.clear_binding(ActionId::Undo);
        s.set_binding(
            ActionId::Tool(ToolId::Brush),
            KeyChord::new(ChordKey::G, false, false, false),
        )
        .unwrap();
        s.set_drag(
            DragActionId::BrushSize,
            ModSet {
                ctrl: true,
                shift: true,
                alt: false,
            },
        );
        let json = serde_json::to_string(&s.to_stored()).unwrap();
        let back: StoredShortcuts = serde_json::from_str(&json).unwrap();
        let (s2, warns) = Shortcuts::from_stored(&back);
        assert!(warns.is_empty());
        assert_eq!(s2, s);
        assert!(json.contains("\"undo\":null"));
        assert!(json.contains("\"tool.brush\""));
    }

    #[test]
    fn stored_unknown_ids_are_skipped_not_resurrected() {
        let mut stored = StoredShortcuts::default();
        stored.bindings.insert(
            "tool.sparkle".into(),
            Some(KeyChord::new(ChordKey::Q, false, false, false)),
        );
        stored.bindings.insert("undo".into(), None);
        let (s, warns) = Shortcuts::from_stored(&stored);
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("tool.sparkle"));
        assert_eq!(s.get(ActionId::Undo), None); // explicit null honored
        assert_eq!(
            s.get(ActionId::Tool(ToolId::Brush)),
            Some(KeyChord::new(ChordKey::B, false, false, false))
        );
    }

    #[test]
    fn format_names_chords_like_the_ui() {
        assert_eq!(
            KeyChord::new(ChordKey::Z, true, true, false).format(),
            "Ctrl+Shift+Z"
        );
        assert_eq!(
            KeyChord::new(ChordKey::B, false, false, false).format(),
            "B"
        );
        assert_eq!(
            KeyChord::new(ChordKey::F5, false, false, true).format(),
            "Alt+F5"
        );
        assert_eq!(
            KeyChord::new(ChordKey::Num0, true, false, false).format(),
            "Ctrl+0"
        );
    }

    #[test]
    fn stored_empty_drag_modifier_is_ignored_with_warning() {
        // Hand-edited file: a drag with no modifiers would hijack every
        // plain drag (matches(false, false, false) is always true).
        let mut stored = StoredShortcuts::default();
        stored.drag.insert(
            DragActionId::BrushSize.to_key().to_string(),
            ModSet {
                ctrl: false,
                shift: false,
                alt: false,
            },
        );
        let (s, warns) = Shortcuts::from_stored(&stored);
        assert_eq!(s.drag_get(DragActionId::BrushSize), ModSet::SHIFT);
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("modifier"));
    }

    #[test]
    fn size_drag_mode_defaults_to_horizontal_and_stores_nothing() {
        let s = Shortcuts::default();
        assert_eq!(s.size_drag_mode(), SizeDragMode::Horizontal);
        assert!(
            s.to_stored().size_drag_mode.is_none(),
            "default isn't stored"
        );
    }

    #[test]
    fn size_drag_mode_is_sparse_and_round_trips() {
        let mut s = Shortcuts::default();
        s.set_size_drag_mode(SizeDragMode::Distance);
        let stored = s.to_stored();
        assert_eq!(stored.size_drag_mode.as_deref(), Some("distance"));
        let json = serde_json::to_string(&stored).unwrap();
        let back: StoredShortcuts = serde_json::from_str(&json).unwrap();
        let (s2, warns) = Shortcuts::from_stored(&back);
        assert!(warns.is_empty());
        assert_eq!(s2.size_drag_mode(), SizeDragMode::Distance);
    }

    #[test]
    fn old_settings_without_size_drag_mode_load_as_horizontal() {
        // Pre-v0.9.1 files have no "size_drag_mode" key at all.
        let back: StoredShortcuts = serde_json::from_str("{}").unwrap();
        let (s, warns) = Shortcuts::from_stored(&back);
        assert!(warns.is_empty());
        assert_eq!(s.size_drag_mode(), SizeDragMode::Horizontal);
    }

    #[test]
    fn stored_unknown_size_drag_mode_warns_and_keeps_default() {
        let stored = StoredShortcuts {
            size_drag_mode: Some("sideways".into()),
            ..StoredShortcuts::default()
        };
        let (s, warns) = Shortcuts::from_stored(&stored);
        assert_eq!(s.size_drag_mode(), SizeDragMode::Horizontal);
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("sideways"));
    }

    #[test]
    fn reset_all_restores_the_default_size_drag_mode() {
        let mut s = Shortcuts::default();
        s.set_size_drag_mode(SizeDragMode::Vertical);
        s.reset_all();
        assert_eq!(s.size_drag_mode(), SizeDragMode::Horizontal);
    }
}
