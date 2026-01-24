mod animation;
mod error;
mod focus;

use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, error, info, trace, warn};

use animation::{AnimConfig, run_animation};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_F8,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetForegroundWindow, GetMessageW, GetWindowTextLengthW,
    GetWindowTextW, IsWindowVisible, MSG, SetForegroundWindow, WM_HOTKEY,
};
use windows::core::{BOOL, w};

/// Track window visibility state (atomic for thread safety)
static WINDOW_VISIBLE: AtomicBool = AtomicBool::new(false);

const HOTKEY_ID: i32 = 1;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    debug!("=== Window List ===");
    list_windows();
    debug!("===================");

    unsafe {
        RegisterHotKey(None, HOTKEY_ID, MOD_NOREPEAT, VK_F8.0 as u32)?;
    }

    info!("Hotkey F8 registered. Press F8 to toggle window visibility.");
    info!("Press Ctrl+C to exit.");

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        match msg.message {
            WM_HOTKEY if msg.wParam.0 as i32 == HOTKEY_ID => {
                toggle_window();
            }
            m if m == focus::WM_FOCUS_CHANGED => {
                handle_focus_lost();
            }
            _ => {}
        }
    }

    if let Err(e) = focus::uninstall_hook() {
        error!("Focus unhook error: {e}");
    }
    unsafe { UnregisterHotKey(None, HOTKEY_ID)? };

    Ok(())
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
