//! Edge trigger module: show/hide window when cursor reaches screen edge

use std::time::Instant;
use thiserror::Error;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

use crate::animation::Direction;
use crate::tracking::WindowBounds;
use windows::Win32::Foundation::{POINT, RECT};

const SETTINGS_KEY: &str = r"Software\QuakeModoki";
const EDGE_ENABLED: &str = "EdgeEnabled";

#[derive(Debug, Error)]
pub enum EdgeError {
    #[error("Registry access failed: {0}")]
    Registry(#[from] std::io::Error),
}

/// Edge trigger configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeConfig {
    pub threshold_px: i32,
    pub show_delay_ms: u32,
    pub hide_delay_ms: u32,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            threshold_px: 1,
            show_delay_ms: 100,
            hide_delay_ms: 300,
        }
    }
}

/// Edge trigger state machine
#[derive(Debug, Clone, Default)]
pub enum EdgeState {
    #[default]
    Idle,
    PendingShow {
        since: Instant,
    },
    Active,
    PendingHide {
        since: Instant,
    },
}

/// Action to perform after state transition
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeAction {
    Show,
    Hide,
}

/// Check if cursor at edge (within threshold of work area boundary)
/// Returns true if cursor within threshold of edge matching direction
pub fn detect_edge(cursor: POINT, work_area: &RECT, direction: Direction, threshold: i32) -> bool {
    match direction {
        Direction::Left => cursor.x <= work_area.left + threshold,
        Direction::Right => cursor.x >= work_area.right - threshold - 1,
        Direction::Top => cursor.y <= work_area.top + threshold,
        Direction::Bottom => cursor.y >= work_area.bottom - threshold - 1,
    }
}

/// Check if cursor inside window bounds
pub fn cursor_in_window(cursor: POINT, bounds: &WindowBounds) -> bool {
    cursor.x >= bounds.x
        && cursor.x < bounds.x + bounds.width
        && cursor.y >= bounds.y
        && cursor.y < bounds.y + bounds.height
}

/// Check and transition state machine
/// Returns Some(action) when show/hide needed, None otherwise
pub fn check_and_transition(
    state: &mut EdgeState,
    config: &EdgeConfig,
    direction: Direction,
    visible: bool,
    cursor: POINT,
    work_area: &RECT,
    bounds: Option<&WindowBounds>,
) -> Option<EdgeAction> {
    let at_edge = detect_edge(cursor, work_area, direction, config.threshold_px);
    let in_window = bounds.is_some_and(|b| cursor_in_window(cursor, b));

    match state {
        EdgeState::Idle => {
            if !visible && at_edge {
                *state = EdgeState::PendingShow {
                    since: Instant::now(),
                };
            }
            None
        }
        EdgeState::PendingShow { since } => {
            if !at_edge {
                // Left edge before delay
                *state = EdgeState::Idle;
                None
            } else if since.elapsed().as_millis() >= config.show_delay_ms as u128 {
                // Delay elapsed, trigger show
                *state = EdgeState::Active;
                Some(EdgeAction::Show)
            } else {
                None
            }
        }
        EdgeState::Active => {
            if visible && !in_window && !at_edge {
                // Cursor left window and edge, start hide delay
                *state = EdgeState::PendingHide {
                    since: Instant::now(),
                };
            }
            None
        }
        EdgeState::PendingHide { since } => {
            if in_window || at_edge {
                // Returned to window/edge, cancel hide
                *state = EdgeState::Active;
                None
            } else if since.elapsed().as_millis() >= config.hide_delay_ms as u128 {
                // Delay elapsed, trigger hide
                *state = EdgeState::Idle;
                Some(EdgeAction::Hide)
            } else {
                None
            }
        }
    }
}

/// Reset state machine to Idle
pub fn reset_state(state: &mut EdgeState) {
    *state = EdgeState::Idle;
}

// ========== Registry Persistence ==========

/// Check if edge trigger enabled in registry
pub fn is_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey_with_flags(SETTINGS_KEY, KEY_READ)
        .ok()
        .and_then(|key| key.get_value::<u32, _>(EDGE_ENABLED).ok())
        != Some(0)
}

/// Enable/disable edge trigger
pub fn set_enabled(enabled: bool) -> Result<(), EdgeError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(SETTINGS_KEY)?;
    key.set_value(EDGE_ENABLED, &(enabled as u32))?;
    Ok(())
}

/// Toggle edge trigger, returns new state
pub fn toggle() -> Result<bool, EdgeError> {
    let new_state = !is_enabled();
    set_enabled(new_state)?;
    Ok(new_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    fn make_rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn make_point(x: i32, y: i32) -> POINT {
        POINT { x, y }
    }

    fn make_bounds(x: i32, y: i32, width: i32, height: i32) -> WindowBounds {
        WindowBounds {
            x,
            y,
            width,
            height,
        }
    }

    // ========== Edge Detection Tests ==========

    #[test]
    fn test_detect_edge_left() {
        let work_area = make_rect(0, 0, 1920, 1080);
        // At edge (x=0)
        assert!(detect_edge(
            make_point(0, 500),
            &work_area,
            Direction::Left,
            1
        ));
        // Within threshold (x=1)
        assert!(detect_edge(
            make_point(1, 500),
            &work_area,
            Direction::Left,
            1
        ));
        // Outside threshold (x=2)
        assert!(!detect_edge(
            make_point(2, 500),
            &work_area,
            Direction::Left,
            1
        ));
    }

    #[test]
    fn test_detect_edge_right() {
        let work_area = make_rect(0, 0, 1920, 1080);
        // At edge (x=1919)
        assert!(detect_edge(
            make_point(1919, 500),
            &work_area,
            Direction::Right,
            1
        ));
        // Within threshold (x=1918)
        assert!(detect_edge(
            make_point(1918, 500),
            &work_area,
            Direction::Right,
            1
        ));
        // Outside threshold (x=1917)
        assert!(!detect_edge(
            make_point(1917, 500),
            &work_area,
            Direction::Right,
            1
        ));
    }

    #[test]
    fn test_detect_edge_top() {
        let work_area = make_rect(0, 0, 1920, 1080);
        assert!(detect_edge(
            make_point(500, 0),
            &work_area,
            Direction::Top,
            1
        ));
        assert!(detect_edge(
            make_point(500, 1),
            &work_area,
            Direction::Top,
            1
        ));
        assert!(!detect_edge(
            make_point(500, 2),
            &work_area,
            Direction::Top,
            1
        ));
    }

    #[test]
    fn test_detect_edge_bottom() {
        let work_area = make_rect(0, 0, 1920, 1080);
        assert!(detect_edge(
            make_point(500, 1079),
            &work_area,
            Direction::Bottom,
            1
        ));
        assert!(detect_edge(
            make_point(500, 1078),
            &work_area,
            Direction::Bottom,
            1
        ));
        assert!(!detect_edge(
            make_point(500, 1077),
            &work_area,
            Direction::Bottom,
            1
        ));
    }

    #[test]
    fn test_detect_edge_threshold_larger() {
        let work_area = make_rect(0, 0, 1920, 1080);
        // threshold=5: x ∈ [0..5] triggers
        assert!(detect_edge(
            make_point(5, 500),
            &work_area,
            Direction::Left,
            5
        ));
        assert!(!detect_edge(
            make_point(6, 500),
            &work_area,
            Direction::Left,
            5
        ));
    }

    // ========== Cursor in Window Tests ==========

    #[test]
    fn test_cursor_in_window_inside() {
        let bounds = make_bounds(100, 100, 400, 300);
        assert!(cursor_in_window(make_point(200, 200), &bounds));
        assert!(cursor_in_window(make_point(100, 100), &bounds)); // top-left corner
    }

    #[test]
    fn test_cursor_in_window_outside() {
        let bounds = make_bounds(100, 100, 400, 300);
        assert!(!cursor_in_window(make_point(99, 200), &bounds)); // left
        assert!(!cursor_in_window(make_point(500, 200), &bounds)); // right edge (exclusive)
        assert!(!cursor_in_window(make_point(200, 99), &bounds)); // top
        assert!(!cursor_in_window(make_point(200, 400), &bounds)); // bottom edge (exclusive)
    }

    // ========== State Machine Tests ==========

    #[test]
    fn test_state_idle_to_pending_show() {
        let config = EdgeConfig {
            threshold_px: 1,
            show_delay_ms: 100,
            hide_delay_ms: 300,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let mut state = EdgeState::Idle;

        // Cursor at left edge, window not visible
        let action = check_and_transition(
            &mut state,
            &config,
            Direction::Left,
            false,
            make_point(0, 500),
            &work_area,
            None,
        );
        assert_eq!(action, None);
        assert!(matches!(state, EdgeState::PendingShow { .. }));
    }

    #[test]
    fn test_state_pending_show_to_idle_on_leave() {
        let config = EdgeConfig {
            threshold_px: 1,
            show_delay_ms: 100,
            hide_delay_ms: 300,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let mut state = EdgeState::PendingShow {
            since: Instant::now(),
        };

        // Cursor leaves edge
        let action = check_and_transition(
            &mut state,
            &config,
            Direction::Left,
            false,
            make_point(100, 500),
            &work_area,
            None,
        );
        assert_eq!(action, None);
        assert!(matches!(state, EdgeState::Idle));
    }

    #[test]
    fn test_state_pending_show_to_active() {
        let config = EdgeConfig {
            threshold_px: 1,
            show_delay_ms: 10,
            hide_delay_ms: 300,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let mut state = EdgeState::PendingShow {
            since: Instant::now(),
        };

        // Wait for delay
        sleep(Duration::from_millis(15));

        // Cursor still at edge
        let action = check_and_transition(
            &mut state,
            &config,
            Direction::Left,
            false,
            make_point(0, 500),
            &work_area,
            None,
        );
        assert_eq!(action, Some(EdgeAction::Show));
        assert!(matches!(state, EdgeState::Active));
    }

    #[test]
    fn test_state_active_to_pending_hide() {
        let config = EdgeConfig {
            threshold_px: 1,
            show_delay_ms: 100,
            hide_delay_ms: 300,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let bounds = make_bounds(0, 0, 400, 1080);
        let mut state = EdgeState::Active;

        // Cursor outside window and not at edge
        let action = check_and_transition(
            &mut state,
            &config,
            Direction::Left,
            true,
            make_point(500, 500),
            &work_area,
            Some(&bounds),
        );
        assert_eq!(action, None);
        assert!(matches!(state, EdgeState::PendingHide { .. }));
    }

    #[test]
    fn test_state_pending_hide_cancel_on_return() {
        let config = EdgeConfig {
            threshold_px: 1,
            show_delay_ms: 100,
            hide_delay_ms: 300,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let bounds = make_bounds(0, 0, 400, 1080);
        let mut state = EdgeState::PendingHide {
            since: Instant::now(),
        };

        // Cursor returns to window
        let action = check_and_transition(
            &mut state,
            &config,
            Direction::Left,
            true,
            make_point(200, 500),
            &work_area,
            Some(&bounds),
        );
        assert_eq!(action, None);
        assert!(matches!(state, EdgeState::Active));
    }

    #[test]
    fn test_state_pending_hide_to_idle() {
        let config = EdgeConfig {
            threshold_px: 1,
            show_delay_ms: 100,
            hide_delay_ms: 10,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let bounds = make_bounds(0, 0, 400, 1080);
        let mut state = EdgeState::PendingHide {
            since: Instant::now(),
        };

        // Wait for delay
        sleep(Duration::from_millis(15));

        // Cursor still outside
        let action = check_and_transition(
            &mut state,
            &config,
            Direction::Left,
            true,
            make_point(500, 500),
            &work_area,
            Some(&bounds),
        );
        assert_eq!(action, Some(EdgeAction::Hide));
        assert!(matches!(state, EdgeState::Idle));
    }

    #[test]
    fn test_state_idle_stays_idle_when_visible() {
        let config = EdgeConfig::default();
        let work_area = make_rect(0, 0, 1920, 1080);
        let mut state = EdgeState::Idle;

        // Cursor at edge but window already visible
        let action = check_and_transition(
            &mut state,
            &config,
            Direction::Left,
            true,
            make_point(0, 500),
            &work_area,
            None,
        );
        assert_eq!(action, None);
        assert!(matches!(state, EdgeState::Idle));
    }

    // ========== Registry Tests ==========

    #[test]
    fn test_is_enabled_default_false() {
        // Ensure disabled first
        let _ = set_enabled(false);
        assert!(!is_enabled());
    }

    #[test]
    fn test_set_enabled_roundtrip() {
        set_enabled(true).expect("set enabled failed");
        assert!(is_enabled());

        set_enabled(false).expect("set disabled failed");
        assert!(!is_enabled());
    }

    #[test]
    fn test_toggle() {
        let _ = set_enabled(false);

        let new_state = toggle().expect("toggle failed");
        assert!(new_state);
        assert!(is_enabled());

        let new_state = toggle().expect("toggle failed");
        assert!(!new_state);
        assert!(!is_enabled());
    }
}
