//! Desktop integration for the AppImage build. A .deb registers a launcher entry and
//! icons when it's installed; an AppImage is a single file and doesn't, so GNOME shows a
//! generic icon in the dock and there's no menu entry. On each start from an AppImage,
//! write both into the user's ~/.local/share, pointing at the AppImage's current path
//! (updates replace the file in place, so the path stays valid).

use std::path::{Path, PathBuf};

const ICON: &[u8] = include_bytes!("../icons/icon.png");
/// Matches the window's Wayland app_id / X11 WM_CLASS (the binary name), so the dock
/// pairs the running window with this entry and shows its icon.
const APP_ID: &str = "workout-tracker";

pub fn integrate_appimage() {
    let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from) else { return };
    // Installed from the .deb as well? Its own entry already covers it.
    if Path::new("/usr/share/applications/Workout Tracker.desktop").exists() {
        return;
    }
    let Some(data) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    else {
        return;
    };
    let icon = data.join(format!("icons/hicolor/512x512/apps/{APP_ID}.png"));
    let desktop = data.join(format!("applications/{APP_ID}.desktop"));
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Workout Tracker\n\
         Comment=Workout coach that reads and writes the Workout Tracker Google Sheet\n\
         Exec=\"{}\"\nIcon={APP_ID}\nStartupWMClass={APP_ID}\nCategories=Utility;\nTerminal=false\n",
        appimage.display()
    );
    write_if_changed(&icon, ICON);
    write_if_changed(&desktop, entry.as_bytes());
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
