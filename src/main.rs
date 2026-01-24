mod animation;
mod error;
mod focus;
mod notification;
mod tracking;

use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, error, info, trace, warn};

use animation::{AnimConfig, run_animation};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    IsWindowVisible, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, SetForegroundWindow, TranslateMessage, WM_QUIT,
};
use windows::core::BOOL;

/// Track window visibility state (atomic for thread safety)
static WINDOW_VISIBLE: AtomicBool = AtomicBool::new(false);

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    debug!("=== Window List ===");
    list_windows();
    debug!("===================");

    let manager =
        GlobalHotKeyManager::new().map_err(|e| anyhow::anyhow!("GlobalHotKeyManager: {e}"))?;

    // Toggle hotkey: F8
    let hotkey_toggle = HotKey::new(None, Code::F8);
    manager
        .register(hotkey_toggle)
        .map_err(|e| anyhow::anyhow!("Toggle hotkey register: {e}"))?;

    // Tracking hotkey: Ctrl+Alt+Q
    let hotkey_track = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyQ);
    manager
        .register(hotkey_track)
        .map_err(|e| anyhow::anyhow!("Track hotkey register: {e}"))?;

    info!("Hotkeys registered: F8 (toggle), Ctrl+Alt+Q (track)");
    info!("Focus a window and press Ctrl+Alt+Q to register it, then F8 to toggle.");

    run_event_loop(hotkey_toggle.id(), hotkey_track.id())?;

    if let Err(e) = focus::uninstall_hook() {
        error!("Focus unhook error: {e}");
    }

    Ok(())
}

fn run_event_loop(toggle_id: u32, track_id: u32) -> anyhow::Result<()> {
    let receiver = GlobalHotKeyEvent::receiver();
    let mut msg = MSG::default();

    loop {
        // Wait for message OR 16ms timeout
        unsafe {
            MsgWaitForMultipleObjectsEx(None, 16, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }

        // Check hotkey events (non-blocking)
        while let Ok(event) = receiver.try_recv() {
            if event.state() == HotKeyState::Pressed {
                match event.id() {
                    id if id == toggle_id => toggle_window(),
                    id if id == track_id => register_foreground(),
                    _ => {}
                }
            }
        }

        // Process Win32 messages
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                return Ok(());
            }
            match msg.message {
                m if m == focus::WM_FOCUS_CHANGED => handle_focus_lost(),
                _ => unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                },
            }
        }
    }
}

fn list_windows() {
    unsafe extern "system" fn enum_callback(hwnd: HWND, _: LPARAM) -> BOOL {
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let len = GetWindowTextLengthW(hwnd);
                if len > 0 {
                    let mut buf = vec![0u16; (len + 1) as usize];
                    GetWindowTextW(hwnd, &mut buf);
                    let title = String::from_utf16_lossy(&buf[..len as usize]);
                    if !title.is_empty() {
                        trace!(hwnd = ?hwnd, title, "window");
                    }
                }
            }
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnumWindows(Some(enum_callback), LPARAM(0));
    }
}

fn register_foreground() {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND::default() {
        warn!("No foreground window");
        return;
    }

    let title = tracking::get_window_title(hwnd);
    tracking::set_tracked(hwnd);
    tracking::save_bounds(hwnd);
    focus::set_target(hwnd);
    if let Err(e) = focus::install_hook(hwnd) {
        error!("Focus hook error: {e}");
    }
    WINDOW_VISIBLE.store(true, Ordering::SeqCst);
    notification::show_tracked(&title);
    info!(hwnd = ?hwnd, title = %title, "Window tracked (visible)");
}

/// Get monitor work area for a window
fn get_work_area(hwnd: HWND) -> Option<RECT> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        Some(info.rcWork)
    } else {
        None
    }
}

fn toggle_window() {
    // Get tracked window (registered via Ctrl+Alt+Q)
    if !tracking::is_tracked_valid() {
        warn!("No tracked window - press Ctrl+Alt+Q to register");
        return;
    }

    let hwnd = tracking::get_tracked();
    let config = AnimConfig::default();
    let currently_visible = WINDOW_VISIBLE.load(Ordering::SeqCst);

    // Get work area for direction calculation
    let work_area = match get_work_area(hwnd) {
        Some(wa) => wa,
        None => {
            error!("GetMonitorInfo failed");
            return;
        }
    };

    if currently_visible {
        // === SLIDE OUT (visible → hidden) ===
        // 1. Capture current bounds BEFORE hiding
        let bounds = match tracking::save_bounds(hwnd) {
            Some(b) => b,
            None => {
                error!("GetWindowRect failed");
                return;
            }
        };

        // 2. Calculate direction based on overlap
        let direction = tracking::calc_direction(&bounds, &work_area);

        // 3. Restore focus before animation starts
        let prev = focus::get_previous();
        if prev != HWND::default() {
            let _ = unsafe { SetForegroundWindow(prev) };
        }

        // 4. Slide out
        run_animation(hwnd, &config, direction, &bounds, &work_area, false);
        WINDOW_VISIBLE.store(false, Ordering::SeqCst);
        info!(direction = ?direction, "Window: focus restored → slide out → hidden");
    } else {
        // === SLIDE IN (hidden → visible) ===
        // 1. Load stored bounds or capture current position
        let bounds = tracking::load_bounds()
            .unwrap_or_else(|| tracking::save_bounds(hwnd).expect("GetWindowRect failed"));

        // 2. Calculate direction based on stored position
        let direction = tracking::calc_direction(&bounds, &work_area);

        // 3. Save current foreground window before taking focus
        let prev = unsafe { GetForegroundWindow() };
        focus::save_previous(prev);

        // 4. Slide in
        run_animation(hwnd, &config, direction, &bounds, &work_area, true);
        let _ = unsafe { SetForegroundWindow(hwnd) };
        focus::set_target(hwnd);
        if let Err(e) = focus::install_hook(hwnd) {
            error!("Focus hook error: {e}");
        }
        WINDOW_VISIBLE.store(true, Ordering::SeqCst);
        info!(direction = ?direction, "Window: slide in → visible + focused");
    }
}

fn handle_focus_lost() {
    if !WINDOW_VISIBLE.load(Ordering::SeqCst) {
        return;
    }

    let target = focus::get_target();
    if target == HWND::default() {
        return;
    }

    // Get work area
    let work_area = match get_work_area(target) {
        Some(wa) => wa,
        None => {
            error!("GetMonitorInfo failed");
            return;
        }
    };

    // Capture current bounds before hiding
    let bounds = match tracking::save_bounds(target) {
        Some(b) => b,
        None => {
            error!("GetWindowRect failed");
            return;
        }
    };

    // Calculate direction based on overlap
    let direction = tracking::calc_direction(&bounds, &work_area);

    let config = AnimConfig::default();
    run_animation(target, &config, direction, &bounds, &work_area, false);
    WINDOW_VISIBLE.store(false, Ordering::SeqCst);
    info!(direction = ?direction, "Window: focus lost → hidden");
}
