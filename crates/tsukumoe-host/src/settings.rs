// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The host's settings folder: saved preferences and user theme files.
//!
//! `<config>/preferences.json` holds the mode, density and chosen theme;
//! every `<config>/themes/*.json` is a theme file (`tsukumoe_ui::ThemeFile`)
//! listed under its file name.

use std::fs;
use std::path::{Path, PathBuf};

use tsukumoe_ui::{NamedTheme, Preferences, builtin_themes};

const PREFERENCES: &str = "preferences.json";
const THEMES: &str = "themes";

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

/// Saved preferences; missing or damaged files give the defaults.
pub fn read_preferences(dir: &Path) -> Preferences {
    fs::read_to_string(dir.join(PREFERENCES))
        .map(|json| Preferences::from_json(&json))
        .unwrap_or_default()
}

pub fn write_preferences(dir: &Path, prefs: &Preferences) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(PREFERENCES);
    fs::write(&path, prefs.to_json()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The built-in themes, then the user's from `<dir>/themes`. A user theme with
/// a built-in's name replaces it. Files that fail to read or parse are skipped
/// and reported in the second value.
pub fn load_themes(dir: &Path) -> (Vec<NamedTheme>, Vec<String>) {
    let mut themes = builtin_themes();
    let mut errors = Vec::new();
    let mut files: Vec<PathBuf> = fs::read_dir(dir.join(THEMES))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for path in files {
        let name = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let loaded = fs::read_to_string(&path)
            .map_err(|e| format!("{name}: {e}"))
            .and_then(|json| NamedTheme::from_json(&name, &json));
        match loaded {
            Ok(theme) => {
                themes.retain(|t| t.name != theme.name);
                themes.push(theme);
            }
            Err(e) => errors.push(e),
        }
    }
    (themes, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_ui::Density;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tsukumoe-host-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn preferences_round_trip_through_the_folder() {
        let dir = scratch("prefs");
        assert_eq!(read_preferences(&dir), Preferences::default());
        let prefs = Preferences {
            density: Density::Regular,
            theme: Some("Figma Dark".into()),
            ..Default::default()
        };
        write_preferences(&dir, &prefs).unwrap();
        assert_eq!(read_preferences(&dir), prefs);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn user_themes_join_the_built_in_ones_and_bad_files_are_reported() {
        let dir = scratch("themes");
        fs::create_dir_all(dir.join(THEMES)).unwrap();
        let theme = |name: &str, json: &str| fs::write(dir.join(THEMES).join(name), json).unwrap();
        theme("mine.json", r#"{"appearance":"dark"}"#);
        theme("Figma Dark.json", r#"{"appearance":"dark"}"#);
        theme("broken.json", "{");
        theme("notes.txt", "ignored");
        let (themes, errors) = load_themes(&dir);
        let names: Vec<_> = themes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Figma Light", "Figma Dark", "mine"]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("broken"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
