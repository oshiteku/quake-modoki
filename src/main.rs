mod animation;

use std::sync::atomic::{AtomicBool, Ordering};

use animation::{AnimConfig, run_animation};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_F8,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetMessageW, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    MSG, WM_HOTKEY,
};
use windows::core::{BOOL, w};

/// Track window visibility state (atomic for thread safety)
static WINDOW_VISIBLE: AtomicBool = AtomicBool::new(false);

const HOTKEY_ID: i32 = 1;

fn main() -> anyhow::Result<()> {
    println!("=== Window List ===");
    list_windows();
    println!("===================\n");

    unsafe {
        RegisterHotKey(None, HOTKEY_ID, MOD_NOREPEAT, VK_F8.0 as u32)?;
    }

    println!("Hotkey F8 registered. Press F8 to toggle Notepad visibility.");
    println!("Press Ctrl+C to exit.");

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_ID {
            toggle_notepad();
        }
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
                        println!("  {:?}: {}", hwnd, title);
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

fn toggle_notepad() {
    let hwnd = unsafe { FindWindowW(None, w!("タイトルなし - メモ帳")) };
    match hwnd {
        Ok(h) if h != HWND::default() => {
            let config = AnimConfig::default();
            let currently_visible = WINDOW_VISIBLE.load(Ordering::SeqCst);

            if currently_visible {
                // Slide out (visible → hidden)
                run_animation(h, &config, false);
                WINDOW_VISIBLE.store(false, Ordering::SeqCst);
                println!("Notepad: slide out → hidden");
            } else {
                // Slide in (hidden → visible)
                run_animation(h, &config, true);
                WINDOW_VISIBLE.store(true, Ordering::SeqCst);
                println!("Notepad: slide in → visible");
            }
        }
        _ => {
            println!("Notepad not found");
        }
    }
}
