//! Linux desktop integration.
//!
//! GNOME pairs a running window with its launcher entry (and so its icon) by the window's
//! app ID: the Wayland app_id, or the X11 WM_CLASS. Tauri leaves that to the program name,
//! which depends on how the app was started, so it's pinned to the bundle identifier here
//! and every launcher entry names the same ID.
//!
//! A .deb installs its own entry and icons. An AppImage is a single file and doesn't, so on
//! each start from an AppImage the entry and icon are written into ~/.local/share, pointing
//! at the AppImage's current path (updates replace the file in place, so it stays valid).

use std::path::{Path, PathBuf};

pub const APP_ID: &str = "com.timgoodchild.workouttracker";
const ICON: &[u8] = include_bytes!("../icons/icon.png");
/// Written by 0.2.1 under the old name; removed so the menu doesn't list the app twice.
const OLD_ENTRY: &str = "applications/workout-tracker.desktop";
const DEB_ENTRY: &str = "/usr/share/applications/Workout Tracker.desktop";

/// Must run before GTK starts (i.e. before the Tauri builder).
pub fn pin_app_id() {
    glib::set_prgname(Some(APP_ID));
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

fn entry_path(data: &Path) -> PathBuf {
    data.join(format!("applications/{APP_ID}.desktop"))
}

pub fn integrate_appimage() {
    let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from) else { return };
    let Some(data) = data_home() else { return };
    let _ = std::fs::remove_file(data.join(OLD_ENTRY));
    let _ = std::fs::remove_file(data.join("icons/hicolor/512x512/apps/workout-tracker.png"));
    // Installed from the .deb as well? Its own entry already covers it.
    if Path::new(DEB_ENTRY).exists() {
        return;
    }
    // Absolute icon path: no dependence on the icon theme or its cache.
    let icon = data.join(format!("icons/hicolor/512x512/apps/{APP_ID}.png"));
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Workout Tracker\n\
         Comment=Workout coach that reads and writes the Workout Tracker Google Sheet\n\
         Exec=\"{}\"\nIcon={}\nStartupWMClass={APP_ID}\nCategories=Utility;\nTerminal=false\n",
        appimage.display(),
        icon.display()
    );
    write_if_changed(&icon, ICON);
    write_if_changed(&entry_path(&data), entry.as_bytes());
}

/// One line for Settings, so a mismatch can be read off the screen instead of guessed.
pub fn diagnostics() -> String {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
    let prgname = glib::prgname().map(|s| s.to_string()).unwrap_or_default();
    let install = if std::env::var_os("APPIMAGE").is_some() { "AppImage" } else if Path::new(DEB_ENTRY).exists() { ".deb" } else { "other" };
    let entry = if install == "AppImage" {
        data_home().map(|d| entry_path(&d)).filter(|p| p.exists()).map(|p| p.display().to_string()).unwrap_or_else(|| "not written".into())
    } else {
        DEB_ENTRY.into()
    };
    format!("Linux: {install} · {session} session · app ID {prgname} · launcher entry {entry}")
}

fn write_if_changed(path: &Path, bytes: &[u8]) {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, bytes);
}
