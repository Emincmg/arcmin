//! "Export for Godot": the same bundle Arcweave's own Godot export produces.
//!
//! Arcweave writes a `.zip` holding `project.json` (the standard Arcweave
//! project JSON, which is exactly what arcmin stores) and, optionally, the
//! project's media under `assets/`. Godot's Arcweave plugin then reads
//! `project.json` through an `ArcweaveAsset` resource.

use std::io::Write;
use std::path::{Path, PathBuf};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

pub struct ExportReport {
    pub assets: usize,
}

/// Writes `dest` (a `.zip`) from the saved project json at `project_json` and,
/// when `include_assets`, every file under the `assets/` folder next to it.
pub fn godot_zip(
    project_json: &Path,
    dest: &Path,
    include_assets: bool,
) -> Result<ExportReport, String> {
    let json = std::fs::read(project_json).map_err(|e| format!("Could not read project: {e}"))?;

    // Build beside the destination and move into place, so a failure never
    // leaves a half-written zip where the author expects a good one.
    let tmp = dest.with_extension("zip.part");
    let result = write_zip(&tmp, &json, project_json, include_assets);
    match result {
        Ok(report) => {
            std::fs::rename(&tmp, dest).map_err(|e| format!("Could not write {dest:?}: {e}"))?;
            Ok(report)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn write_zip(
    tmp: &Path,
    json: &[u8],
    project_json: &Path,
    include_assets: bool,
) -> Result<ExportReport, String> {
    let file = std::fs::File::create(tmp).map_err(|e| format!("Could not create zip: {e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let err = |e: &dyn std::fmt::Display| format!("Zip error: {e}");

    zip.start_file("project.json", opts).map_err(|e| err(&e))?;
    zip.write_all(json).map_err(|e| err(&e))?;

    let mut assets = 0;
    if include_assets && let Some(dir) = project_json.parent().map(|d| d.join("assets")) {
        for path in files_under(&dir) {
            let Ok(rel) = path.strip_prefix(&dir) else {
                continue;
            };
            let name = format!(
                "assets/{}",
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            );
            // Images are already compressed; storing them avoids pointless CPU.
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file(name, stored).map_err(|e| err(&e))?;
            let mut src = std::fs::File::open(&path).map_err(|e| err(&e))?;
            std::io::copy(&mut src, &mut zip).map_err(|e| err(&e))?;
            assets += 1;
        }
    }
    zip.finish().map_err(|e| err(&e))?;
    Ok(ExportReport { assets })
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn project_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arcmin-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("assets/sub")).unwrap();
        std::fs::write(dir.join("project_settings.json"), br#"{"name":"demo"}"#).unwrap();
        std::fs::write(dir.join("assets/a.png"), b"AAAA").unwrap();
        std::fs::write(dir.join("assets/sub/b.png"), b"BB").unwrap();
        dir
    }

    fn names(zip_path: &Path) -> Vec<String> {
        let mut z = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
        let mut v: Vec<String> = (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn zip_holds_project_json_and_assets() {
        let dir = project_dir();
        let dest = dir.join("out.zip");
        let report = godot_zip(&dir.join("project_settings.json"), &dest, true).unwrap();
        assert_eq!(report.assets, 2);
        assert_eq!(
            names(&dest),
            ["assets/a.png", "assets/sub/b.png", "project.json"]
        );

        let mut z = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        let mut json = String::new();
        z.by_name("project.json")
            .unwrap()
            .read_to_string(&mut json)
            .unwrap();
        assert_eq!(json, r#"{"name":"demo"}"#);
        assert!(!dir.join("out.zip.part").exists(), "no leftover temp file");
    }

    #[test]
    fn data_only_export_skips_assets() {
        let dir = project_dir();
        let dest = dir.join("data.zip");
        let report = godot_zip(&dir.join("project_settings.json"), &dest, false).unwrap();
        assert_eq!(report.assets, 0);
        assert_eq!(names(&dest), ["project.json"]);
    }
}
