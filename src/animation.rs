//! Animation module: easing, position calculation, animation loop

use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, WS_EX_COMPOSITED,
};

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
    Linear,
    Quad,
    Cubic,
    Expo,
}

impl Easing {
    /// Apply easing function: t ∈ [0,1] → [0,1]
    /// ease-out variant: fast start, slow end
    pub fn apply(&self, t: f64) -> f64 {
        match self {
            Easing::Linear => t,
            Easing::Quad => 1.0 - (1.0 - t).powi(2),
            Easing::Cubic => 1.0 - (1.0 - t).powi(3),
            Easing::Expo => {
                if t <= 0.0 {
                    0.0
                } else if t >= 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f64.powf(-10.0 * t)
                }
            }
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
    pub direction: Direction,
    pub width_percent: u32,
    pub height_percent: u32,
}

impl Default for AnimConfig {
    fn default() -> Self {
        Self {
            duration_ms: 200,
            easing: Easing::Cubic,
            direction: Direction::Left,
            width_percent: 40,
            height_percent: 100,
        }
    }
}

/// Calculate window position based on direction and progress
/// Returns (x, y) for the window
///
/// slide_in=true:  progress 0→1 moves from hidden → visible
/// slide_in=false: progress 0→1 moves from visible → hidden
pub fn calc_position(
    direction: Direction,
    work_area: &RECT,
    width: i32,
    height: i32,
    progress: f64,
    slide_in: bool,
) -> (i32, i32) {
    let t = if slide_in { progress } else { 1.0 - progress };

    match direction {
        Direction::Left => {
            let x = lerp(-width, 0, t);
            (x, work_area.top)
        }
        Direction::Right => {
            let visible_x = work_area.right - width;
            let x = lerp(work_area.right, visible_x, t);
            (x, work_area.top)
        }
        Direction::Top => {
            let y = lerp(-height, 0, t);
            (work_area.left, y)
        }
        Direction::Bottom => {
            let visible_y = work_area.bottom - height;
            let y = lerp(work_area.bottom, visible_y, t);
            (work_area.left, y)
        }
    }
}

/// Run slide animation
/// slide_in=true: hidden → visible (show window, animate in)
/// slide_in=false: visible → hidden (animate out, hide window)
pub fn run_animation(hwnd: HWND, config: &AnimConfig, slide_in: bool) {
    // Get monitor work area
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return;
    }

    let work_area = info.rcWork;
    let screen_w = work_area.right - work_area.left;
    let screen_h = work_area.bottom - work_area.top;

    // Calculate window dimensions
    let width = (screen_w * config.width_percent as i32) / 100;
    let height = (screen_h * config.height_percent as i32) / 100;

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
        let (x, y) = calc_position(config.direction, &work_area, width, height, 0.0, true);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
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

        let (x, y) = calc_position(config.direction, &work_area, width, height, t, slide_in);

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
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, flags);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Easing Tests ==========

    #[test]
    fn test_easing_linear_boundaries() {
        assert_eq!(Easing::Linear.apply(0.0), 0.0);
        assert_eq!(Easing::Linear.apply(1.0), 1.0);
    }

    #[test]
    fn test_easing_linear_mid() {
        assert_eq!(Easing::Linear.apply(0.5), 0.5);
    }

    #[test]
    fn test_easing_quad_boundaries() {
        assert_eq!(Easing::Quad.apply(0.0), 0.0);
        assert_eq!(Easing::Quad.apply(1.0), 1.0);
    }

    #[test]
    fn test_easing_quad_mid() {
        // ease-out-quad: 1 - (1 - t)^2 = 1 - 0.25 = 0.75
        assert!((Easing::Quad.apply(0.5) - 0.75).abs() < 1e-10);
    }

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

    #[test]
    fn test_easing_expo_boundaries() {
        assert_eq!(Easing::Expo.apply(0.0), 0.0);
        assert_eq!(Easing::Expo.apply(1.0), 1.0);
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

    #[test]
    fn test_calc_position_left_slide_in_start() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let (x, y) = calc_position(Direction::Left, &work_area, 768, 1080, 0.0, true);
        assert_eq!(x, -768); // hidden: x = -width
        assert_eq!(y, 0);
    }

    #[test]
    fn test_calc_position_left_slide_in_end() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let (x, y) = calc_position(Direction::Left, &work_area, 768, 1080, 1.0, true);
        assert_eq!(x, 0); // visible: x = 0
        assert_eq!(y, 0);
    }

    #[test]
    fn test_calc_position_left_slide_out_end() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let (x, y) = calc_position(Direction::Left, &work_area, 768, 1080, 1.0, false);
        assert_eq!(x, -768); // hidden: x = -width
        assert_eq!(y, 0);
    }

    #[test]
    fn test_calc_position_right_slide_in_start() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let (x, y) = calc_position(Direction::Right, &work_area, 768, 1080, 0.0, true);
        assert_eq!(x, 1920); // hidden: x = screen_width
        assert_eq!(y, 0);
    }

    #[test]
    fn test_calc_position_right_slide_in_end() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let (x, y) = calc_position(Direction::Right, &work_area, 768, 1080, 1.0, true);
        assert_eq!(x, 1920 - 768); // visible: x = screen_width - width
        assert_eq!(y, 0);
    }

    #[test]
    fn test_calc_position_top_slide_in() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let (x, y) = calc_position(Direction::Top, &work_area, 1920, 540, 0.0, true);
        assert_eq!(x, 0);
        assert_eq!(y, -540); // hidden: y = -height
    }

    #[test]
    fn test_calc_position_bottom_slide_in() {
        let work_area = make_work_area(0, 0, 1920, 1080);
        let (x, y) = calc_position(Direction::Bottom, &work_area, 1920, 540, 0.0, true);
        assert_eq!(x, 0);
        assert_eq!(y, 1080); // hidden: y = screen_height
    }
}
