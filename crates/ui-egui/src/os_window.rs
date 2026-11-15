//! Extra OS windows (document windows, Camera Raw, the screen color picker) share the main
//! window's app id, so docks show the PhotoCraft icon on them and Wayland compositors' window
//! rules can match them (Hyprland's `class:`, #6697). Without one their app id is empty.

/// Matches the `.desktop` file and hicolor icon name, so Wayland docks pick up the icon.
///
/// The "PhotoCraft with SGL Tools" build carries its own id, from the same compile-time switch as
/// `apps/photocraft/src/app_dirs.rs::SGL_EDITION` (see the rebuild note there): the id also becomes
/// the X11 `WM_CLASS` and the Windows AppUserModelID, so keeping it distinct is what stops the two
/// products' windows being grouped together and stops one product's `.desktop`/bundle identity
/// being applied to the other's windows. It must keep matching what the packaging installs:
/// `packaging/linux/ai.storyteller.photocraft.sgl.desktop` (`StartupWMClass`, `Icon`) and
/// `packaging/macos/Info.plist.sgl.in` (`CFBundleIdentifier`);
/// `the_desktop_entry_names_the_window_class` in `apps/photocraft/src/main.rs` checks the Linux pair.
pub const APP_ID: &str = match option_env!("PHOTOCRAFT_SGL_EDITION") {
    Some(_) => "ai.storyteller.photocraft.sgl",
    None => "ai.storyteller.photocraft",
};

/// A builder for an extra OS window, carrying [`APP_ID`].
pub fn builder() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default().with_app_id(APP_ID)
}

#[cfg(test)]
mod tests {
    #[test]
    fn extra_windows_carry_the_app_id() {
        assert_eq!(super::builder().app_id.as_deref(), Some(super::APP_ID));
        // The id for this build, pinned for each edition; `the_desktop_entry_names_the_window_class`
        // in `apps/photocraft/src/main.rs` ties it to the packaged `.desktop` file for this edition.
        let want = if option_env!("PHOTOCRAFT_SGL_EDITION").is_some() { "ai.storyteller.photocraft.sgl" } else { "ai.storyteller.photocraft" };
        assert_eq!(super::APP_ID, want);
    }
}
