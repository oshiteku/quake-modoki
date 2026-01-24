//! Window tracking module: register foreground window for toggle control

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW, IsWindow};

/// Registered window handle for toggle control
static TRACKED_HWND: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

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
}
