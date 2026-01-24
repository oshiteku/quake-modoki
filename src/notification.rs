//! Desktop notification support

use notify_rust::Notification;

/// Show toast notification for tracked window
pub fn show_tracked(title: &str) {
    if let Err(e) = Notification::new()
        .summary("Quake Modoki")
        .body(&format!("Tracking: {}", title))
        .show()
    {
        tracing::warn!("Notification failed: {e}");
    }
}
