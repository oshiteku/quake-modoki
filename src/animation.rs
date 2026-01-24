//! Animation module: easing, position calculation, animation loop

use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, WS_EX_COMPOSITED,
};

use crate::error::AnimationError;
use crate::tracking::WindowBounds;

/// Slide direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Top,
    Bottom,
}

/// Easing function type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    Cubic,
}

impl Easing {
    /// Apply easing function: t ∈ [0,1] → [0,1]
    /// ease-out variant: fast start, slow end
    pub fn apply(&self, t: f64) -> f64 {
        match self {
            Easing::Cubic => 1.0 - (1.0 - t).powi(3),
        }
    }
}

/// Linear interpolation: lerp(a, b, t) = a + (b - a) * t
pub fn lerp(a: i32, b: i32, t: f64) -> i32 {
    (a as f64 + (b - a) as f64 * t).round() as i32
}

/// Animation configuration
#[derive(Debug, Clone)]
pub struct AnimConfig {
    pub duration_ms: u32,
    pub easing: Easing,
}

impl Default for AnimConfig {
    fn default() -> Self {
        Self {
            duration_ms: 200,
            easing: Easing::Cubic,
        }
    }
}

/// Calculate window position based on direction and progress
/// Returns (x, y) for the window
///
/// slide_in=true:  progress 0→1 moves from off-screen → original position
/// slide_in=false: progress 0→1 moves from original position → off-screen
pub fn calc_position(
    direction: Direction,
    work_area: &RECT,
    original: &WindowBounds,
    progress: f64,
    slide_in: bool,
) -> (i32, i32) {
    let t = if slide_in { progress } else { 1.0 - progress };

    match direction {
        Direction::Left => {
            let hidden_x = work_area.left - original.width;
            let x = lerp(hidden_x, original.x, t);
            (x, original.y)
        }
        Direction::Right => {
            let hidden_x = work_area.right;
            let x = lerp(hidden_x, original.x, t);
            (x, original.y)
        }
        Direction::Top => {
            let hidden_y = work_area.top - original.height;
            let y = lerp(hidden_y, original.y, t);
            (original.x, y)
        }
        Direction::Bottom => {
            let hidden_y = work_area.bottom;
            let y = lerp(hidden_y, original.y, t);
            (original.x, y)
        }
    }
}

/// Run slide animation
/// slide_in=true: off-screen → original position (show window, animate in)
/// slide_in=false: original position → off-screen (animate out, hide window)
pub fn run_animation(
    hwnd: HWND,
    config: &AnimConfig,
    direction: Direction,
    bounds: &WindowBounds,
    work_area: &RECT,
    slide_in: bool,
) -> Result<(), AnimationError> {
    let duration = Duration::from_millis(config.duration_ms as u64);
    let start = Instant::now();

    // Frame sync: wait for VSync before rendering
    fn frame_sync() {
        unsafe {
            if DwmFlush().is_err() {
                std::thread::sleep(Duration::from_millis(16));
            }
        }
    }

    // Apply WS_EX_COMPOSITED for double-buffered rendering (anti-flicker)
    let original_exstyle = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            original_exstyle | WS_EX_COMPOSITED.0 as isize,
        );
        // Force repaint after style change to refresh DWM buffer
        let _ = InvalidateRect(Some(hwnd), None, true);
    }

    // Show window at start position if sliding in
    if slide_in {
        frame_sync(); // sync BEFORE window becomes visible
        let (x, y) = calc_position(direction, work_area, bounds, 0.0, true);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                bounds.width,
                bounds.height,
                SWP_SHOWWINDOW,
            );
        }
    }

    // Animation loop
    loop {
        frame_sync(); // sync BEFORE position update

        let elapsed = start.elapsed();
        let raw_t = (elapsed.as_secs_f64() / duration.as_secs_f64()).min(1.0);
        let t = config.easing.apply(raw_t);
        let is_final = raw_t >= 1.0;

        let (x, y) = calc_position(direction, work_area, bounds, t, slide_in);

        // Atomic hide: combine final position with SWP_HIDEWINDOW
        // slide_in: allow activation (no SWP_NOACTIVATE)
        // slide_out: prevent activation + hide at final frame
        let flags = if is_final && !slide_in {
            SWP_NOACTIVATE | SWP_HIDEWINDOW
        } else if slide_in {
            SWP_NOZORDER // allow activation during slide_in
        } else {
            SWP_NOACTIVATE
        };

        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                bounds.width,
                bounds.height,
                flags,
            );
        }

        if is_final {
            break;
        }
    }

    // Ensure hide composited
    if !slide_in {
        frame_sync();
    }

    // Restore original extended style
    unsafe {
        // Invalidate before style restoration to prevent black artifacts
        let _ = InvalidateRect(Some(hwnd), None, true);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, original_exstyle);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Easing Tests ==========

    #[test]
    fn test_easing_cubic_boundaries() {
        assert_eq!(Easing::Cubic.apply(0.0), 0.0);
        assert_eq!(Easing::Cubic.apply(1.0), 1.0);
    }

    #[test]
    fn test_easing_cubic_mid() {
        // ease-out-cubic: 1 - (1 - t)^3 = 1 - 0.125 = 0.875
        assert!((Easing::Cubic.apply(0.5) - 0.875).abs() < 1e-10);
    }

    // ========== Lerp Tests ==========

    #[test]
    fn test_lerp_boundaries() {
        assert_eq!(lerp(0, 100, 0.0), 0);
        assert_eq!(lerp(0, 100, 1.0), 100);
    }

    #[test]
    fn test_lerp_mid() {
        assert_eq!(lerp(0, 100, 0.5), 50);
    }

    #[test]
    fn test_lerp_negative() {
        assert_eq!(lerp(-100, 0, 0.0), -100);
        assert_eq!(lerp(-100, 0, 1.0), 0);
        assert_eq!(lerp(-100, 0, 0.5), -50);
    }

    // ========== Position Tests ==========

    fn make_work_area(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn make_bounds(x: i32, y: i32, width: i32, height: i32) -> WindowBounds {
        WindowBounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn test_calc_position_left_slide_in_start() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let bounds = make_bounds(100, 50, 768, 1080);
        let (x, y) = calc_position(Direction::Left, &work_area, &bounds, 0.0, true);
        assert_eq!(x, -768); // hidden: x = work_area.left - width
        assert_eq!(y, 50); // y = original.y
    }

    #[test]
    fn test_calc_position_left_slide_in_end() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let bounds = make_bounds(100, 50, 768, 1080);
        let (x, y) = calc_position(Direction::Left, &work_area, &bounds, 1.0, true);
        assert_eq!(x, 100); // visible: x = original.x
        assert_eq!(y, 50);
    }

    #[test]
    fn test_calc_position_left_slide_out_end() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let bounds = make_bounds(100, 50, 768, 1080);
        let (x, y) = calc_position(Direction::Left, &work_area, &bounds, 1.0, false);
        assert_eq!(x, -768); // hidden: x = work_area.left - width
        assert_eq!(y, 50);
    }

    #[test]
    fn test_calc_position_right_slide_in_start() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let bounds = make_bounds(1000, 50, 768, 1080);
        let (x, y) = calc_position(Direction::Right, &work_area, &bounds, 0.0, true);
        assert_eq!(x, 1920); // hidden: x = work_area.right
        assert_eq!(y, 50);
    }

    #[test]
    fn test_calc_position_right_slide_in_end() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let bounds = make_bounds(1000, 50, 768, 1080);
        let (x, y) = calc_position(Direction::Right, &work_area, &bounds, 1.0, true);
        assert_eq!(x, 1000); // visible: x = original.x
        assert_eq!(y, 50);
    }

    #[test]
    fn test_calc_position_top_slide_in() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let bounds = make_bounds(200, 100, 1920, 540);
        let (x, y) = calc_position(Direction::Top, &work_area, &bounds, 0.0, true);
        assert_eq!(x, 200); // x = original.x
        assert_eq!(y, -540); // hidden: y = work_area.top - height
    }

    #[test]
    fn test_calc_position_bottom_slide_in() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let bounds = make_bounds(200, 500, 1920, 540);
        let (x, y) = calc_position(Direction::Bottom, &work_area, &bounds, 0.0, true);
        assert_eq!(x, 200); // x = original.x
        assert_eq!(y, 1080); // hidden: y = work_area.bottom
    }
}
