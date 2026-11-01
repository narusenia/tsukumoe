// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The per-user settings folder (shared with the host's: theme preferences
//! live in `preferences.json` there) and the editor's own files:
//! `editor/layouts.json`, `editor/state.json`.

use std::fs;
use std::path::{Path, PathBuf};

/// The per-user settings folder, or `None` when the environment gives no home.
pub fn config_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let base = if cfg!(target_os = "macos") {
        PathBuf::from(var("HOME")?).join("Library/Application Support")
    } else if cfg!(windows) {
        PathBuf::from(var("APPDATA")?)
    } else {
        var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))?
    };
    Some(base.join("tsukumoe"))
}

/// The editor's folder inside `config`.
pub fn editor_dir(config: &Path) -> PathBuf {
    config.join("editor")
}

/// A file's text; missing and unreadable files give `None`.
pub fn read(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name)).ok()
}

pub fn write(dir: &Path, name: &str, text: &str) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(name);
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_and_missing_is_none() {
        let dir =
            std::env::temp_dir().join(format!("tsukumoe-app-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(read(&dir, "a.json"), None);
        write(&dir.join("sub"), "a.json", "{}").unwrap();
        assert_eq!(read(&dir.join("sub"), "a.json").as_deref(), Some("{}"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
