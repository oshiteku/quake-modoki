//! Auto-launch via Windows Registry (HKCU\Software\Microsoft\Windows\CurrentVersion\Run)

use std::env;
use thiserror::Error;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APP_NAME: &str = "Quake Modoki";

#[derive(Debug, Error)]
pub enum AutoLaunchError {
    #[error("Registry access failed: {0}")]
    Registry(#[from] std::io::Error),

    #[error("Executable path not found")]
    ExePath,
}

/// Check if auto-launch enabled in registry
pub fn is_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ)
        .ok()
        .and_then(|key| key.get_value::<String, _>(APP_NAME).ok())
        .is_some()
}

/// Enable auto-launch (write exe path to registry)
pub fn enable() -> Result<(), AutoLaunchError> {
    let exe_path = env::current_exe().map_err(|_| AutoLaunchError::ExePath)?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY)?;
    key.set_value(APP_NAME, &format!("\"{}\"", exe_path.display()))?;
    Ok(())
}

/// Disable auto-launch (remove registry key)
pub fn disable() -> Result<(), AutoLaunchError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey_with_flags(RUN_KEY, KEY_WRITE)?;
    // Ignore error if key doesn't exist
    let _ = key.delete_value(APP_NAME);
    Ok(())
}

/// Toggle auto-launch state, returns new state
pub fn toggle() -> Result<bool, AutoLaunchError> {
    if is_enabled() {
        disable()?;
        Ok(false)
    } else {
        enable()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_is_enabled_initially_false() {
        // Clean up first
        let _ = disable();
        assert!(!is_enabled());
    }

    #[test]
    #[serial]
    fn test_enable_disable_cycle() {
        // Clean state
        let _ = disable();
        assert!(!is_enabled());

        // Enable
        enable().expect("enable failed");
        assert!(is_enabled());

        // Disable
        disable().expect("disable failed");
        assert!(!is_enabled());
    }

    #[test]
    #[serial]
    fn test_toggle() {
        // Clean state
        let _ = disable();

        // Toggle on
        let new_state = toggle().expect("toggle failed");
        assert!(new_state);
        assert!(is_enabled());

        // Toggle off
        let new_state = toggle().expect("toggle failed");
        assert!(!new_state);
        assert!(!is_enabled());
    }
}
