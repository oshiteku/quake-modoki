// Hide console in release builds (background mode)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod animation;
mod autolaunch;
mod edge;
mod error;
mod focus;
mod notification;
mod tracking;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, error, info, trace, warn};

use animation::{AnimConfig, run_animation};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray::TrayState;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, SetConsoleCtrlHandler,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetCursorPos, GetForegroundWindow, GetWindowTextLengthW,
    GetWindowTextW, IsWindowVisible, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx,
    PM_REMOVE, PeekMessageW, QS_ALLINPUT, SetForegroundWindow, TranslateMessage, WM_ENDSESSION,
    WM_QUERYENDSESSION, WM_QUIT,
};
use windows::core::BOOL;

/// Track window visibility state (atomic for thread safety)
static WINDOW_VISIBLE: AtomicBool = AtomicBool::new(false);

/// Shutdown requested via signal (Ctrl-C, console close, etc.)
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Console control handler: signal shutdown via atomic flag
unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> BOOL {
    match ctrl_type {
        x if x == CTRL_C_EVENT || x == CTRL_BREAK_EVENT => {
            // Signal main loop to exit gracefully
            SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
            BOOL(1)
        }
        x if x == CTRL_CLOSE_EVENT => {
            // Terminal closing - must restore here (5s timeout)
            // Process terminates after handler returns
            let _ = tracking::restore_original();
            SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
            BOOL(1)
        }
        _ => BOOL(0),
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    debug!("=== Window List ===");
    list_windows();
    debug!("===================");

    // Initialize system tray
    let tray = TrayState::new().map_err(|e| anyhow::anyhow!("TrayState: {e}"))?;
    tray.set_autolaunch_checked(autolaunch::is_enabled());
    tray.set_edge_trigger_checked(edge::is_enabled());
    info!("System tray initialized");

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

    // Install Ctrl-C handler for graceful shutdown
    unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), true) }
        .map_err(|e| anyhow::anyhow!("SetConsoleCtrlHandler: {e}"))?;

    run_event_loop(hotkey_toggle.id(), hotkey_track.id(), &tray)?;

    // Restore tracked window to original state on exit
    if tracking::restore_original().is_some() {
        info!("Window restored on exit");
    }

    if let Err(e) = focus::uninstall_hook() {
        error!("Focus unhook error: {e}");
    }

    Ok(())
}

fn run_event_loop(toggle_id: u32, track_id: u32, tray: &TrayState) -> anyhow::Result<()> {
    let hotkey_rx = GlobalHotKeyEvent::receiver();
    let menu_rx = tray::menu_receiver();
    let mut msg = MSG::default();

    // Edge trigger state
    let edge_config = edge::load_config();
    let mut edge_state = edge::EdgeState::default();

    loop {
        // Check shutdown flag (set by ctrl_handler)
        if SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
            info!("Shutdown requested");
            return Ok(());
        }

        // Wait for message OR 16ms timeout
        unsafe {
            MsgWaitForMultipleObjectsEx(None, 16, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }

        // Check hotkey events (non-blocking)
        while let Ok(event) = hotkey_rx.try_recv() {
            if event.state() == HotKeyState::Pressed {
                match event.id() {
                    id if id == toggle_id => {
                        toggle_window();
                        edge::reset_state(&mut edge_state); // Hotkey wins, reset edge
                    }
                    id if id == track_id => register_foreground_with_tray(tray),
                    _ => {}
                }
            }
        }

        // Check menu events (non-blocking)
        while let Ok(event) = menu_rx.try_recv() {
            handle_menu_event(&event, tray, &mut edge_state);
        }

        // Edge trigger check (polling)
        if edge::is_enabled()
            && tracking::is_tracked_valid()
            && let Some(action) = check_edge_trigger(&mut edge_state, &edge_config)
        {
            match action {
                edge::EdgeAction::Show if !WINDOW_VISIBLE.load(Ordering::SeqCst) => {
                    toggle_window();
                }
                edge::EdgeAction::Hide if WINDOW_VISIBLE.load(Ordering::SeqCst) => {
                    toggle_window();
                }
                _ => {}
            }
        }

        // Process Win32 messages
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            match msg.message {
                WM_QUIT => return Ok(()),
                WM_QUERYENDSESSION => {
                    // Allow system to proceed with logoff/shutdown
                }
                WM_ENDSESSION if msg.wParam.0 != 0 => {
                    info!("Session ending");
                    return Ok(());
                }
                m if m == focus::WM_FOCUS_CHANGED => {
                    handle_focus_lost();
                    edge::reset_state(&mut edge_state); // Focus lost resets edge state
                }
                _ => unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                },
            }
        }
    }
}

/// Check edge trigger and return action if any
fn check_edge_trigger(
    state: &mut edge::EdgeState,
    config: &edge::EdgeConfig,
) -> Option<edge::EdgeAction> {
    // Get cursor position
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return None;
    }

    // Get work area for monitor containing cursor
    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let work_area = info.rcWork;

    // Get window bounds and direction
    let bounds = tracking::load_bounds();
    let direction = bounds
        .as_ref()
        .map(|b| tracking::calc_direction(b, &work_area))
        .unwrap_or(animation::Direction::Left);

    let visible = WINDOW_VISIBLE.load(Ordering::SeqCst);

    let action = edge::check_and_transition(
        state,
        config,
        direction,
        visible,
        cursor,
        &work_area,
        bounds.as_ref(),
    );

    if action != edge::EdgeAction::None {
        Some(action)
    } else {
        None
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

/// Handle tray menu events
fn handle_menu_event(event: &muda::MenuEvent, tray: &TrayState, edge_state: &mut edge::EdgeState) {
    let id = event.id();

    if tray.is_exit(id) {
        info!("Exit requested via tray menu");
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    } else if tray.is_untrack(id) {
        // Untrack: restore window and clear status
        if tracking::restore_original().is_some() {
            info!("Window untracked via tray menu");
        }
        if let Err(e) = focus::uninstall_hook() {
            error!("Focus unhook error: {e}");
        }
        WINDOW_VISIBLE.store(false, Ordering::SeqCst);
        edge::reset_state(edge_state);
        tray.update_status(None);
    } else if tray.is_autolaunch(id) {
        // Toggle auto-launch
        match autolaunch::toggle() {
            Ok(enabled) => {
                tray.set_autolaunch_checked(enabled);
                info!(enabled, "Auto-launch toggled");
            }
            Err(e) => {
                error!("Auto-launch toggle failed: {e}");
            }
        }
    } else if tray.is_edge_trigger(id) {
        // Toggle edge trigger
        match edge::toggle() {
            Ok(enabled) => {
                tray.set_edge_trigger_checked(enabled);
                edge::reset_state(edge_state);
                info!(enabled, "Edge trigger toggled");
            }
            Err(e) => {
                error!("Edge trigger toggle failed: {e}");
            }
        }
    }
}

/// Register foreground window with tray status update
fn register_foreground_with_tray(tray: &TrayState) {
    // Restore previous tracked window before registering new one
    if tracking::restore_original().is_some() {
        info!("Previous window restored");
    }

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND::default() {
        warn!("No foreground window");
        tray.update_status(None);
        return;
    }

    let title = tracking::get_window_title(hwnd);

    // Save original state before tracking
    if tracking::save_original(hwnd).is_none() {
        warn!("Failed to save original state");
    }

    tracking::set_tracked(hwnd);
    tracking::save_bounds(hwnd);
    focus::set_target(hwnd);
    if let Err(e) = focus::install_hook(hwnd) {
        error!("Focus hook error: {e}");
    }
    WINDOW_VISIBLE.store(true, Ordering::SeqCst);

    // Update tray status
    tray.update_status(Some(&title));

    notification::show_tracked(&title);
    info!(hwnd = ?hwnd, title = %title, "Window tracked (visible)");
}
