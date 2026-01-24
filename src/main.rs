use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_F8,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetMessageW, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    MSG, SW_HIDE, SW_SHOW, ShowWindow, WM_HOTKEY,
};
use windows::core::{BOOL, w};

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
            let visible = unsafe { IsWindowVisible(h) }.as_bool();
            let cmd = if visible { SW_HIDE } else { SW_SHOW };
            let _ = unsafe { ShowWindow(h, cmd) };
            println!(
                "Notepad toggled: {}",
                if visible { "hidden" } else { "shown" }
            );
        }
        _ => {
            println!("Notepad not found");
        }
    }
}
