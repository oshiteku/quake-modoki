fn main() {
    #[cfg(windows)]
    {
        let mut res = tauri_winres::WindowsResource::new();
        // ordinal 1 → exe icon + systray resource
        res.set_icon_with_id("assets/icon.ico", "1");
        res.compile().expect("Failed to compile resources");
    }
}
