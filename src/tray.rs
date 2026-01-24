//! System tray integration using tray-icon crate

use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use thiserror::Error;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Error)]
pub enum TrayError {
    #[error("Tray icon creation failed: {0}")]
    Creation(String),

    #[error("Menu operation failed: {0}")]
    Menu(String),
}

/// System tray state and menu IDs
pub struct TrayState {
    _icon: TrayIcon,
    menu_untrack: MenuId,
    menu_autolaunch: MenuId,
    menu_exit: MenuId,
    status_item: MenuItem,
    autolaunch_item: CheckMenuItem,
}

impl TrayState {
    /// Create tray icon with menu
    pub fn new() -> Result<Self, TrayError> {
        // Create menu items
        let status_item = MenuItem::with_id("status", "No window tracked", false, None);
        let untrack_item = MenuItem::with_id("untrack", "Untrack", true, None);
        let autolaunch_item =
            CheckMenuItem::with_id("autolaunch", "Start with Windows", true, false, None);
        let exit_item = MenuItem::with_id("exit", "Exit", true, None);

        // Store IDs
        let menu_untrack = untrack_item.id().clone();
        let menu_autolaunch = autolaunch_item.id().clone();
        let menu_exit = exit_item.id().clone();

        // Build menu
        let menu = Menu::new();
        menu.append(&status_item)
            .map_err(|e| TrayError::Menu(e.to_string()))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| TrayError::Menu(e.to_string()))?;
        menu.append(&untrack_item)
            .map_err(|e| TrayError::Menu(e.to_string()))?;
        menu.append(&autolaunch_item)
            .map_err(|e| TrayError::Menu(e.to_string()))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| TrayError::Menu(e.to_string()))?;
        menu.append(&exit_item)
            .map_err(|e| TrayError::Menu(e.to_string()))?;

        // Create default icon (simple colored square)
        let icon = create_default_icon()?;

        // Build tray icon
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Quake Modoki")
            .with_icon(icon)
            .build()
            .map_err(|e| TrayError::Creation(e.to_string()))?;

        Ok(Self {
            _icon: tray,
            menu_untrack,
            menu_autolaunch,
            menu_exit,
            status_item,
            autolaunch_item,
        })
    }

    /// Update status display (tracked window title)
    pub fn update_status(&self, title: Option<&str>) {
        let text = match title {
            Some(t) => format!("Tracking: {}", truncate_title(t, 30)),
            None => "No window tracked".to_string(),
        };
        self.status_item.set_text(&text);
    }

    /// Set autolaunch checkbox state
    pub fn set_autolaunch_checked(&self, checked: bool) {
        self.autolaunch_item.set_checked(checked);
    }

    /// Check if event matches untrack menu
    pub fn is_untrack(&self, id: &MenuId) -> bool {
        *id == self.menu_untrack
    }

    /// Check if event matches autolaunch menu
    pub fn is_autolaunch(&self, id: &MenuId) -> bool {
        *id == self.menu_autolaunch
    }

    /// Check if event matches exit menu
    pub fn is_exit(&self, id: &MenuId) -> bool {
        *id == self.menu_exit
    }
}

/// Get menu event receiver
pub fn menu_receiver() -> &'static muda::MenuEventReceiver {
    MenuEvent::receiver()
}

/// Load icon from embedded Windows resource
fn create_default_icon() -> Result<Icon, TrayError> {
    // Resource ordinal 1 = icon set by winres in build.rs
    Icon::from_resource(1, None).map_err(|e| TrayError::Creation(e.to_string()))
}

/// Truncate title with ellipsis if too long (char-based, UTF-8 safe)
fn truncate_title(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .nth(max_chars.saturating_sub(3))
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_title_short() {
        assert_eq!(truncate_title("Hello", 10), "Hello");
    }

    #[test]
    fn test_truncate_title_exact() {
        assert_eq!(truncate_title("HelloWorld", 10), "HelloWorld");
    }

    #[test]
    fn test_truncate_title_long() {
        assert_eq!(truncate_title("Hello World Long", 10), "Hello W...");
    }

    #[test]
    fn test_truncate_title_unicode_middle_dot() {
        // Exact string from panic: byte 27 falls inside · (bytes 26..28)
        let s = "Issue Quake · Issue #268 · oshiteku/memo - Google Chrome";
        let result = truncate_title(s, 30);
        assert!(result.ends_with("..."));
        assert!(result.chars().count() <= 30);
    }

    #[test]
    fn test_truncate_title_emoji() {
        // 🔥 = U+1F525 (4 bytes in UTF-8)
        let s = "🔥 Hot Topic 🔥";
        let result = truncate_title(s, 10);
        assert!(result.ends_with("..."));
        assert!(result.chars().count() <= 10);
    }
}
