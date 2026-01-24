//! Window tracking module: register foreground window for toggle control

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsWindow,
};

use crate::animation::Direction;

/// Registered window handle for toggle control
static TRACKED_HWND: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

/// Stored window bounds for animation
static STORED_BOUNDS: AtomicPtr<WindowBounds> = AtomicPtr::new(null_mut());

/// Window bounds (position + size)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl WindowBounds {
    /// Create from Win32 RECT
    pub fn from_rect(rect: &RECT) -> Self {
        Self {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        }
    }
}

/// Register window for toggle control
pub fn set_tracked(hwnd: HWND) {
    TRACKED_HWND.store(hwnd.0 as *mut _, Ordering::SeqCst);
}

/// Get registered window handle
pub fn get_tracked() -> HWND {
    HWND(TRACKED_HWND.load(Ordering::SeqCst) as *mut _)
}

/// Check if tracked window is valid
pub fn is_tracked_valid() -> bool {
    let hwnd = get_tracked();
    hwnd != HWND::default() && unsafe { IsWindow(Some(hwnd)) }.as_bool()
}

/// Save current window bounds before slide-out
/// Returns captured bounds, or None if GetWindowRect fails
pub fn save_bounds(hwnd: HWND) -> Option<WindowBounds> {
    let mut rect = RECT::default();
    if !unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
        return None;
    }

    let bounds = WindowBounds::from_rect(&rect);
    let boxed = Box::new(bounds);
    let ptr = Box::into_raw(boxed);

    // Swap old pointer, leak previous allocation (acceptable for single-window app)
    STORED_BOUNDS.store(ptr, Ordering::SeqCst);

    Some(bounds)
}

/// Load stored bounds
pub fn load_bounds() -> Option<WindowBounds> {
    let ptr = STORED_BOUNDS.load(Ordering::SeqCst);
    if ptr.is_null() {
        None
    } else {
        // Safety: ptr was created by Box::into_raw and is valid
        Some(unsafe { *ptr })
    }
}

/// Clear stored bounds
pub fn clear_bounds() {
    let ptr = STORED_BOUNDS.swap(null_mut(), Ordering::SeqCst);
    if !ptr.is_null() {
        // Safety: ptr was created by Box::into_raw
        drop(unsafe { Box::from_raw(ptr) });
    }
}

/// Calculate overlap ratio between bounds and region
/// Returns intersection_area / window_area ∈ [0, 1]
fn overlap_ratio(bounds: &WindowBounds, region: &RECT) -> f64 {
    let x1 = bounds.x.max(region.left);
    let y1 = bounds.y.max(region.top);
    let x2 = (bounds.x + bounds.width).min(region.right);
    let y2 = (bounds.y + bounds.height).min(region.bottom);

    if x2 <= x1 || y2 <= y1 {
        return 0.0;
    }

    let intersection = (x2 - x1) as i64 * (y2 - y1) as i64;
    let window_area = bounds.width as i64 * bounds.height as i64;

    if window_area == 0 {
        return 0.0;
    }

    intersection as f64 / window_area as f64
}

/// Calculate optimal slide direction based on overlap with screen halves
/// Returns direction with maximum overlap ratio
pub fn calc_direction(bounds: &WindowBounds, work_area: &RECT) -> Direction {
    let mid_x = (work_area.left + work_area.right) / 2;
    let mid_y = (work_area.top + work_area.bottom) / 2;

    let regions = [
        (
            Direction::Left,
            RECT {
                left: work_area.left,
                top: work_area.top,
                right: mid_x,
                bottom: work_area.bottom,
            },
        ),
        (
            Direction::Right,
            RECT {
                left: mid_x,
                top: work_area.top,
                right: work_area.right,
                bottom: work_area.bottom,
            },
        ),
        (
            Direction::Top,
            RECT {
                left: work_area.left,
                top: work_area.top,
                right: work_area.right,
                bottom: mid_y,
            },
        ),
        (
            Direction::Bottom,
            RECT {
                left: work_area.left,
                top: mid_y,
                right: work_area.right,
                bottom: work_area.bottom,
            },
        ),
    ];

    regions
        .iter()
        .map(|(dir, region)| (*dir, overlap_ratio(bounds, region)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(dir, _)| dir)
        .unwrap_or(Direction::Left)
}

/// Get window title for logging
pub fn get_window_title(hwnd: HWND) -> String {
    if hwnd == HWND::default() {
        return String::new();
    }

    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len == 0 {
            return String::new();
        }

        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buf);
        if copied == 0 {
            return String::new();
        }

        String::from_utf16_lossy(&buf[..copied as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    // ========== HWND Tracking Tests ==========

    #[test]
    fn test_tracked_initially_null() {
        // Reset state for test isolation
        TRACKED_HWND.store(null_mut(), Ordering::SeqCst);

        let hwnd = get_tracked();
        assert_eq!(hwnd, HWND::default());
    }

    #[test]
    fn test_set_get_tracked_roundtrip() {
        // Create fake HWND for testing (non-null pointer)
        let fake_ptr = 0x12345678 as *mut c_void;
        let fake_hwnd = HWND(fake_ptr);

        set_tracked(fake_hwnd);
        let retrieved = get_tracked();

        assert_eq!(retrieved, fake_hwnd);

        // Cleanup
        TRACKED_HWND.store(null_mut(), Ordering::SeqCst);
    }

    #[test]
    fn test_get_window_title_null_hwnd() {
        let title = get_window_title(HWND::default());
        assert!(title.is_empty());
    }

    #[test]
    fn test_is_tracked_valid_null() {
        TRACKED_HWND.store(null_mut(), Ordering::SeqCst);
        assert!(!is_tracked_valid());
    }

    // ========== WindowBounds Tests ==========

    #[test]
    fn test_window_bounds_from_rect() {
        let rect = make_rect(100, 200, 500, 600);
        let bounds = WindowBounds::from_rect(&rect);

        assert_eq!(bounds.x, 100);
        assert_eq!(bounds.y, 200);
        assert_eq!(bounds.width, 400);
        assert_eq!(bounds.height, 400);
    }

    #[test]
    fn test_load_bounds_initially_none() {
        clear_bounds();
        assert!(load_bounds().is_none());
    }

    // ========== Overlap Ratio Tests ==========

    #[test]
    fn test_overlap_ratio_full() {
        let bounds = WindowBounds {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let region = make_rect(0, 0, 100, 100);
        assert!((overlap_ratio(&bounds, &region) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_overlap_ratio_none() {
        let bounds = WindowBounds {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let region = make_rect(200, 200, 300, 300);
        assert_eq!(overlap_ratio(&bounds, &region), 0.0);
    }

    #[test]
    fn test_overlap_ratio_half() {
        let bounds = WindowBounds {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let region = make_rect(50, 0, 150, 100); // right half overlaps
        assert!((overlap_ratio(&bounds, &region) - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_overlap_ratio_quarter() {
        let bounds = WindowBounds {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let region = make_rect(50, 50, 150, 150); // bottom-right quarter
        assert!((overlap_ratio(&bounds, &region) - 0.25).abs() < 1e-10);
    }

    // ========== Direction Calculation Tests ==========

    #[test]
    fn test_calc_direction_left_half() {
        // Window in left half of screen
        let bounds = WindowBounds {
            x: 100,
            y: 100,
            width: 400,
            height: 600,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let dir = calc_direction(&bounds, &work_area);
        assert_eq!(dir, Direction::Left);
    }

    #[test]
    fn test_calc_direction_right_half() {
        // Window in right half of screen
        let bounds = WindowBounds {
            x: 1400,
            y: 100,
            width: 400,
            height: 600,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let dir = calc_direction(&bounds, &work_area);
        assert_eq!(dir, Direction::Right);
    }

    #[test]
    fn test_calc_direction_top_half() {
        // Window in top half, centered horizontally
        let bounds = WindowBounds {
            x: 760,
            y: 50,
            width: 400,
            height: 300,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let dir = calc_direction(&bounds, &work_area);
        assert_eq!(dir, Direction::Top);
    }

    #[test]
    fn test_calc_direction_bottom_half() {
        // Window in bottom half, centered horizontally
        let bounds = WindowBounds {
            x: 760,
            y: 700,
            width: 400,
            height: 300,
        };
        let work_area = make_rect(0, 0, 1920, 1080);
        let dir = calc_direction(&bounds, &work_area);
        assert_eq!(dir, Direction::Bottom);
    }
}
