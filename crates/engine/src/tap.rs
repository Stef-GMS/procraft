//! M47 touch-tap classifier: which multi-finger tap (if any) a quick
//! clean release means. Pure function (no GUI types) so the app tracker
//! shares exactly the logic the host tests pin down.

/// What a touch release means: nothing, or the M26 undo/redo tap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapAction {
    None,
    Undo,
    Redo,
}

/// Longest press that still counts as a tap (a resting palm holds longer).
/// Matches the app tracker's long-standing 0.35 s bound.
pub const MAX_TAP_SECS: f64 = 0.35;

/// Classify a release: `fingers` peaked touches, `duration_secs` since the
/// first touch, `moved` when the centroid travelled (pan/pinch), and
/// `painting` while a stroke is down — a palm landing mid-stroke must
/// never steal an undo. Single-finger taps paint, so they never arm.
pub fn classify_touch_tap(
    fingers: usize,
    duration_secs: f64,
    moved: bool,
    painting: bool,
) -> TapAction {
    if moved || painting || duration_secs >= MAX_TAP_SECS {
        return TapAction::None;
    }
    match fingers {
        2 => TapAction::Undo,
        3 => TapAction::Redo,
        _ => TapAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_fingers_quick_clean_is_undo() {
        assert_eq!(classify_touch_tap(2, 0.12, false, false), TapAction::Undo);
    }

    #[test]
    fn three_fingers_quick_clean_is_redo() {
        assert_eq!(classify_touch_tap(3, 0.12, false, false), TapAction::Redo);
    }

    #[test]
    fn guards_reject_everything_else() {
        // Travelled (pan/pinch) never fires.
        assert_eq!(classify_touch_tap(2, 0.12, true, false), TapAction::None);
        // Slow release (resting palm) never fires; boundary at 0.35 s.
        assert_eq!(classify_touch_tap(2, 0.349, false, false), TapAction::Undo);
        assert_eq!(classify_touch_tap(2, 0.351, false, false), TapAction::None);
        // Single-finger taps paint — never arm undo/redo.
        assert_eq!(classify_touch_tap(1, 0.12, false, false), TapAction::None);
        // Four fingers is not a gesture.
        assert_eq!(classify_touch_tap(4, 0.12, false, false), TapAction::None);
        // Palm during a paint stroke never fires, even when quick+clean.
        assert_eq!(classify_touch_tap(2, 0.12, false, true), TapAction::None);
        assert_eq!(classify_touch_tap(3, 0.12, false, true), TapAction::None);
    }
}
