mod animation;
mod error;
mod focus;

use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, error, info, trace, warn};

use animation::{AnimConfig, run_animation};
use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, FindWindowW, GetForegroundWindow, GetWindowTextLengthW,
    GetWindowTextW, IsWindowVisible, MsgWaitForMultipleObjectsEx, PeekMessageW,
    SetForegroundWindow, TranslateMessage, MWMO_INPUTAVAILABLE, MSG, PM_REMOVE, QS_ALLINPUT,
    WM_QUIT,
};
use windows::core::{BOOL, w};

/// Track window visibility state (atomic for thread safety)
static WINDOW_VISIBLE: AtomicBool = AtomicBool::new(false);

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    debug!("=== Window List ===");
    list_windows();
    debug!("===================");

    let manager = GlobalHotKeyManager::new()
        .map_err(|e| anyhow::anyhow!("GlobalHotKeyManager: {e}"))?;
    let hotkey = HotKey::new(None, Code::F8);
    manager
        .register(hotkey)
        .map_err(|e| anyhow::anyhow!("Hotkey register: {e}"))?;

    info!("Hotkey F8 registered (global-hotkey). Press F8 to toggle window visibility.");
    info!("Press Ctrl+C to exit.");

    run_event_loop()?;

    if let Err(e) = focus::uninstall_hook() {
        error!("Focus unhook error: {e}");
    }

    Ok(())
}

fn run_event_loop() -> anyhow::Result<()> {
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
                toggle_window();
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

fn toggle_window() {
    let hwnd = unsafe { FindWindowW(None, w!("タイトルなし - メモ帳")) };
    match hwnd {
        Ok(h) if h != HWND::default() => {
            let config = AnimConfig::default();
            let currently_visible = WINDOW_VISIBLE.load(Ordering::SeqCst);

            if currently_visible {
                // Restore focus before animation starts
                let prev = focus::get_previous();
                if prev != HWND::default() {
                    let _ = unsafe { SetForegroundWindow(prev) };
                }
                // Slide out (visible → hidden)
                if let Err(e) = run_animation(h, &config, false) {
                    error!("Animation error: {e}");
                }
                WINDOW_VISIBLE.store(false, Ordering::SeqCst);
                info!("Window: focus restored → slide out → hidden");
            } else {
                // Save current foreground window before taking focus
                let prev = unsafe { GetForegroundWindow() };
                focus::save_previous(prev);
                // Slide in (hidden → visible)
                if let Err(e) = run_animation(h, &config, true) {
                    error!("Animation error: {e}");
                }
                let _ = unsafe { SetForegroundWindow(h) };
                focus::set_target(h);
                if let Err(e) = focus::install_hook(h) {
                    error!("Focus hook error: {e}");
                }
                WINDOW_VISIBLE.store(true, Ordering::SeqCst);
                info!("Window: slide in → visible + focused");
            }
        }
        _ => {
            warn!("Target window not found");
        }
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

    let config = AnimConfig::default();
    if let Err(e) = run_animation(target, &config, false) {
        error!("Animation error: {e}");
    }
    WINDOW_VISIBLE.store(false, Ordering::SeqCst);
    info!("Window: focus lost → hidden");
}
