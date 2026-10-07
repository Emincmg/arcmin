//! App-managed project library: every project lives in its own folder under the
//! app data directory, so opening one never needs a file picker.

use std::path::{Path, PathBuf};

use arcweave_rust::project::{Asset, Project};
use serde::{Deserialize, Serialize};

use crate::covers;
use crate::editor;

pub const PROJECT_FILE: &str = "project_settings.json";

pub struct ProjectEntry {
    pub name: String,
    pub json: PathBuf,
    pub has_save: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct LibraryState {
    last: Option<String>,
}

/// Where arcmin keeps its projects: the platform's per-user data directory
/// (`~/Library/Application Support` on macOS, `%APPDATA%` on Windows,
/// `$XDG_DATA_HOME` or `~/.local/share` on Linux). Set `ARCMIN_DATA_DIR` to use
/// another location, e.g. for a portable install.
fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ARCMIN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("arcmin")
}

/// What to call the system file manager on this platform.
pub fn file_manager_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(target_os = "windows") {
        "Show in Explorer"
    } else {
        "Show Files"
    }
}

/// Opens the folder that holds a project in the system file manager.
pub fn reveal(project_json: &Path) {
    let Some(dir) = project_json.parent() else {
        return;
    };
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    // Explorer reports failure even when it works, so the result is ignored.
    let _ = std::process::Command::new(opener).arg(dir).spawn();
}

fn projects_dir() -> PathBuf {
    data_dir().join("projects")
}

fn state_path() -> PathBuf {
    data_dir().join("state.json")
}

fn load_state() -> LibraryState {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Remembers which project to reopen on the next launch.
pub fn set_last(json: &Path) {
    let Some(dir_name) = json
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
    else {
        return;
    };
    if json.parent().and_then(|d| d.parent()) != Some(projects_dir().as_path()) {
        return;
    }
    let state = LibraryState {
        last: Some(dir_name.to_owned()),
    };
    if let Ok(data) = serde_json::to_string_pretty(&state) {
        let _ = std::fs::create_dir_all(data_dir());
        let _ = std::fs::write(state_path(), data);
    }
}

pub fn last_project() -> Option<PathBuf> {
    let json = projects_dir().join(load_state().last?).join(PROJECT_FILE);
    json.exists().then_some(json)
}

pub fn list() -> Vec<ProjectEntry> {
    let Ok(read) = std::fs::read_dir(projects_dir()) else {
        return vec![];
    };
    let mut entries: Vec<ProjectEntry> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let dir = e.path();
            let json = dir.join(PROJECT_FILE);
            if !json.exists() {
                return None;
            }
            let name = dir.file_name()?.to_str()?.to_owned();
            let has_save = dir.join("project_settings.arcmin-save.json").exists();
            Some(ProjectEntry {
                name,
                json,
                has_save,
            })
        })
        .collect();
    entries.sort_by_key(|e| e.name.to_lowercase());
    entries
}

fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim();
    if s.is_empty() {
        "Untitled".to_owned()
    } else {
        s.to_owned()
    }
}

fn unique_dir(name: &str) -> PathBuf {
    let base = sanitize(name);
    let root = projects_dir();
    let mut dir = root.join(&base);
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{base} {n}"));
        n += 1;
    }
    dir
}

pub fn create(name: &str) -> Result<PathBuf, String> {
    let dir = unique_dir(name);
    std::fs::create_dir_all(dir.join("assets")).map_err(|e| e.to_string())?;
    let (project, _board, _start) = editor::model::new_project(name.trim());
    let data = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
    let json = dir.join(PROJECT_FILE);
    std::fs::write(&json, data).map_err(|e| e.to_string())?;
    Ok(json)
}

/// Copies an Arcweave export (the json, its `assets/` folder and any arcmin
/// sidecar files) into the library. Only those items are copied, never the
/// rest of the source folder.
pub struct ImportReport {
    pub json: PathBuf,
    pub summary: String,
    pub has_problems: bool,
}

pub fn import(src_json: &Path) -> Result<ImportReport, String> {
    let project = Project::from_file(src_json.to_string_lossy().as_ref())
        .map_err(|e| format!("Could not read project: {e}"))?;
    let src_dir = src_json.parent().ok_or("Invalid project path")?;
    let stem = src_json
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project");

    let dir = unique_dir(&project.name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = dir.join(PROJECT_FILE);
    std::fs::copy(src_json, &json).map_err(|e| e.to_string())?;

    for suffix in ["arcmin-save", "arcmin-layout"] {
        let from = src_dir.join(format!("{stem}.{suffix}.json"));
        if from.exists() {
            let _ = std::fs::copy(from, dir.join(format!("project_settings.{suffix}.json")));
        }
    }

    let assets = src_dir.join("assets");
    if assets.is_dir() {
        copy_dir(&assets, &dir.join("assets")).map_err(|e| e.to_string())?;
    } else {
        let _ = std::fs::create_dir_all(dir.join("assets"));
    }

    // Verify what actually landed in the library: every asset the project lists,
    // and every element cover, must resolve to a real file.
    let copied = dir.join("assets");
    let listed: Vec<(&str, &str)> = project
        .assets
        .iter()
        .filter_map(|(id, a)| match a {
            Asset::Node { name, .. } => Some((id.as_str(), name.as_str())),
            Asset::Root { .. } => None,
        })
        .collect();
    let missing: Vec<&str> = listed
        .iter()
        .filter(|(_, name)| !copied.join(name).exists())
        .map(|(_, name)| *name)
        .collect();
    let covers = covers::load(&json);
    let broken_covers = covers
        .values()
        .filter(|id| match covers::file_name(&project, id) {
            Some(name) => !copied.join(name).exists(),
            None => true,
        })
        .count();

    let mut summary = format!(
        "Imported \"{}\": {}/{} assets, {} element covers.",
        project.name,
        listed.len() - missing.len(),
        listed.len(),
        covers.len() - broken_covers,
    );
    let has_problems = !missing.is_empty() || broken_covers > 0;
    if !missing.is_empty() {
        let shown: Vec<&str> = missing.iter().take(3).copied().collect();
        summary += &format!(
            " Missing {} asset file(s): {}{}. Was the assets folder next to the json?",
            missing.len(),
            shown.join(", "),
            if missing.len() > 3 { ", …" } else { "" },
        );
    }
    if broken_covers > 0 {
        summary += &format!(" {broken_covers} cover(s) point to a missing file.");
    }
    Ok(ImportReport {
        json,
        summary,
        has_problems,
    })
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set `ARCMIN_TEST_JSON` to a real Arcweave export (json next to its `assets/`).
    #[test]
    fn import_copies_assets_and_keeps_cover_mapping() {
        let Ok(src) = std::env::var("ARCMIN_TEST_JSON") else {
            eprintln!("ARCMIN_TEST_JSON not set, skipping");
            return;
        };
        let home = std::env::temp_dir().join("arcmin-import-test-home");
        let _ = std::fs::remove_dir_all(&home);
        // SAFETY: this is the only test that touches ARCMIN_DATA_DIR.
        unsafe { std::env::set_var("ARCMIN_DATA_DIR", &home) };

        let src = Path::new(&src);
        let report = import(src).unwrap();
        println!("{}", report.summary);
        assert!(!report.has_problems, "{}", report.summary);
        assert!(report.json.starts_with(&home));

        let src_assets = std::fs::read_dir(src.parent().unwrap().join("assets"))
            .unwrap()
            .count();
        let dst_assets = std::fs::read_dir(report.json.parent().unwrap().join("assets"))
            .unwrap()
            .count();
        assert_eq!(src_assets, dst_assets);

        // The cover mapping read back from the library copy matches the original.
        assert_eq!(covers::load(src), covers::load(&report.json));
        assert!(!covers::load(&report.json).is_empty());

        // Importing the same export again must not overwrite the first copy.
        let again = import(src).unwrap();
        assert_ne!(again.json, report.json);
        assert_eq!(list().len(), 2);
        let _ = std::fs::remove_dir_all(&home);
    }
}
