//! Error types for quake-modoki

use thiserror::Error;

/// Animation errors (graceful degradation)
#[derive(Debug, Error)]
pub enum AnimationError {
    #[error("GetMonitorInfo failed")]
    MonitorInfo,
}

/// Focus tracking errors (graceful degradation)
#[derive(Debug, Error)]
pub enum FocusError {
    #[error("SetWinEventHook → invalid handle")]
    HookInstall,

    #[error("UnhookWinEvent failed")]
    HookUninstall,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_error_display() {
        let err = HotkeyError::Register {
            id: 1,
            source: windows::core::Error::from_hresult(windows::core::HRESULT(
                0x80070005u32 as i32,
            )),
        };
        let msg = err.to_string();
        assert!(msg.contains("RegisterHotKey(1)"));
    }

    #[test]
    fn test_window_error_display() {
        let err = WindowError::NotFound {
            pattern: "test".to_string(),
        };
        assert_eq!(err.to_string(), "FindWindow(\"test\") → ∅");
    }

    #[test]
    fn test_animation_error_display() {
        let err = AnimationError::MonitorInfo;
        assert_eq!(err.to_string(), "GetMonitorInfo failed");
    }

    #[test]
    fn test_focus_error_display() {
        let err = FocusError::HookInstall;
        assert_eq!(err.to_string(), "SetWinEventHook → invalid handle");
    }
}
