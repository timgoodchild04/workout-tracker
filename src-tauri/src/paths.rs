//! Turning whatever was typed or pasted into the key box into a file that exists.
//!
//! People paste paths with quotes (Ubuntu's Files copies them that way), as file:// links,
//! with ~ for home, relative to their home folder, or with a slash missing between a folder
//! and the file name. Rather than make them fix it, try the obvious repairs, then look for a
//! file of that name in the usual places.

use std::path::{Path, PathBuf};

/// Quotes, a file:// prefix and a leading ~ for the home folder.
pub fn clean_path(raw: &str) -> String {
    let mut p = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim().to_string();
    if let Some(rest) = p.strip_prefix("file://") {
        p = urlencoding::decode(rest).map(|s| s.into_owned()).unwrap_or_else(|_| rest.to_string());
    }
    if p == "~" || p.starts_with("~/") || p.starts_with("~\\") {
        if let Some(home) = home_dir() {
            p = format!("{}{}", home.display(), &p[1..]);
        }
    }
    p
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Folders a downloaded key usually sits in.
fn usual_dirs(home: &Path) -> Vec<PathBuf> {
    ["Downloads", "Desktop", "Documents", ""].iter().map(|d| home.join(d)).collect()
}

/// The file `raw` most likely means, or None if nothing sensible exists.
pub fn find_file(raw: &str, home: Option<&Path>) -> Option<PathBuf> {
    let cleaned = PathBuf::from(clean_path(raw));
    if cleaned.as_os_str().is_empty() {
        return None;
    }
    // As typed, then relative to the home folder ("Downloads/k.json") or to the folder
    // that holds home folders ("tim/Downloads/k.json" → /home/tim/Downloads/k.json).
    let mut tries = vec![cleaned.clone()];
    if cleaned.is_relative() {
        if let Some(h) = home {
            tries.push(h.join(&cleaned));
            if let Some(parent) = h.parent() {
                tries.push(parent.join(&cleaned));
            }
        }
    }
    for t in &tries {
        if t.is_file() {
            return Some(t.clone());
        }
        if let Some(fixed) = missing_separator(t) {
            return Some(fixed);
        }
    }
    // Last resort: a file with that name, or whose name the last part ends with, in the usual folders.
    let last = cleaned.file_name()?.to_string_lossy().to_string();
    let home = home?;
    for dir in usual_dirs(home) {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if e.path().is_file() && name.len() >= 6 && (name == last || last.ends_with(&name)) {
                return Some(e.path());
            }
        }
    }
    None
}

/// "/home/tim/Downloadsk.json" → "/home/tim/Downloads/k.json": the last part starts with
/// the name of a folder next to it, and the rest is a file in that folder.
fn missing_separator(p: &Path) -> Option<PathBuf> {
    let parent = p.parent()?;
    let last = p.file_name()?.to_string_lossy().to_string();
    for e in std::fs::read_dir(parent).ok()?.flatten() {
        let dir = e.file_name().to_string_lossy().to_string();
        if let Some(rest) = last.strip_prefix(&dir) {
            let candidate = e.path().join(rest);
            if !rest.is_empty() && e.path().is_dir() && candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Service-account keys sitting in the usual folders, for the "couldn't find it" message.
pub fn key_candidates(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else { return vec![] };
    let mut found = Vec::new();
    for dir in usual_dirs(home) {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let is_key = p.extension().is_some_and(|x| x == "json")
                && std::fs::read_to_string(&p).is_ok_and(|s| s.contains("\"service_account\"") && s.contains("private_key"));
            if is_key {
                found.push(p);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_path_handles_pasted_forms() {
        assert_eq!(clean_path("  '/a/b c.json' "), "/a/b c.json");
        assert_eq!(clean_path("file:///a/b%20c.json"), "/a/b c.json");
        assert_eq!(clean_path("\"/x.json\""), "/x.json");
        if let Some(home) = home_dir() {
            assert_eq!(clean_path("~/keys/k.json"), format!("{}/keys/k.json", home.display()));
        }
    }

    /// A fake /home/tim with ~/Downloads/even-sun-1.json, under a temp folder.
    fn fake_home() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("wt-paths-{}-{}", std::process::id(), rand_suffix()));
        let home = root.join("home").join("tim");
        std::fs::create_dir_all(home.join("Downloads")).unwrap();
        std::fs::write(home.join("Downloads/even-sun-1.json"), r#"{"type": "service_account", "private_key": "x"}"#).unwrap();
        (root, home)
    }
    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn finds_the_key_from_mangled_paths() {
        let (root, home) = fake_home();
        let want = home.join("Downloads/even-sun-1.json");
        let h = Some(home.as_path());
        // Exact, and the forms people actually type or paste.
        for typed in [
            want.display().to_string(),
            format!("'{}'", want.display()),
            format!("file://{}", want.display()),
            "Downloads/even-sun-1.json".to_string(),                          // relative to home
            "tim/Downloads/even-sun-1.json".to_string(),                      // relative to /home
            "tim/Downloadseven-sun-1.json".to_string(),                       // the real one: no /home/, no slash
            format!("{}/Downloadseven-sun-1.json", home.display()),           // missing slash only
            "even-sun-1.json".to_string(),                                     // just the name
            format!("{}/Desktop/even-sun-1.json", home.display()),            // right name, wrong folder
        ] {
            assert_eq!(find_file(&typed, h).as_deref(), Some(want.as_path()), "typed {typed:?}");
        }
        assert_eq!(find_file("nothing-like-it.json", h), None);
        assert_eq!(key_candidates(h), vec![want.clone()]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
