//! Focus tracking module: detect foreground window changes via SetWinEventHook

use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_USER};

/// Custom message for focus change notification
pub const WM_FOCUS_CHANGED: u32 = WM_USER + 1;

// Win32 constants (not exported by windows-rs feature)
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

/// Global hook handle for cleanup
static HOOK_HANDLE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());

/// Target window being monitored
static TARGET_HWND: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());

/// Install focus hook
/// target_hwnd: window being monitored for focus loss
pub fn install_hook(target_hwnd: HWND) {
    TARGET_HWND.store(target_hwnd.0 as *mut _, Ordering::SeqCst);

    unsafe {
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );

        if !hook.is_invalid() {
            HOOK_HANDLE.store(hook.0, Ordering::SeqCst);
        }
    }
}

/// Uninstall focus hook
pub fn uninstall_hook() {
    let handle = HOOK_HANDLE.swap(null_mut(), Ordering::SeqCst);
    if !handle.is_null() {
        unsafe {
            let _ = UnhookWinEvent(HWINEVENTHOOK(handle));
        }
    }
}

/// Update target window
pub fn set_target(hwnd: HWND) {
    TARGET_HWND.store(hwnd.0 as *mut _, Ordering::SeqCst);
}

/// Get current target window
pub fn get_target() -> HWND {
    HWND(TARGET_HWND.load(Ordering::SeqCst) as *mut _)
}

/// Win event callback: fired when foreground window changes
unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    let target = HWND(TARGET_HWND.load(Ordering::SeqCst) as *mut _);

    // Only notify if focus moved away from target window
    if target != HWND::default() && hwnd != target {
        // Post to thread's message queue (NULL hwnd posts to thread)
        unsafe {
            let _ = PostMessageW(None, WM_FOCUS_CHANGED, WPARAM(hwnd.0 as usize), LPARAM(0));
        }
    }
}
