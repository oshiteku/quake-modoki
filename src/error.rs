//! Error types for Quake Modoki

use thiserror::Error;

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
    fn test_focus_error_display() {
        let err = FocusError::HookInstall;
        assert_eq!(err.to_string(), "SetWinEventHook → invalid handle");
    }
}
