use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui;

/// Indexes every file inside an exported Arcweave project folder by filename,
/// so asset/cover references (which only carry a filename) can be resolved to
/// a real path on disk regardless of the subfolder structure.
pub struct AssetIndex {
    by_filename: HashMap<String, PathBuf>,
}

impl AssetIndex {
    pub fn build(project_root: &Path) -> Self {
        let mut by_filename = HashMap::new();
        if let Some(dir) = project_root.parent() {
            Self::walk(dir, &mut by_filename);
        }
        Self { by_filename }
    }

    fn walk(dir: &Path, out: &mut HashMap<String, PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                Self::walk(&path, out);
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                out.insert(name.to_owned(), path);
            }
        }
    }

    pub fn resolve(&self, filename: &str) -> Option<&Path> {
        self.by_filename.get(filename).map(PathBuf::as_path)
    }
}

/// Loads (and caches) a texture for an asset filename. Images larger than `max_dim`
/// are downscaled once with a proper filter; handing a huge image to the GPU and
/// letting it shrink it on the fly looks jagged and shimmers.
pub fn texture_for(
    textures: &mut HashMap<String, egui::TextureHandle>,
    assets: &AssetIndex,
    ctx: &egui::Context,
    filename: &str,
    max_dim: u32,
) -> Option<egui::TextureHandle> {
    let key = format!("{filename}@{max_dim}");
    if let Some(tex) = textures.get(&key) {
        return Some(tex.clone());
    }
    let path = assets.resolve(filename)?;
    let mut image = image::open(path).ok()?;
    if image.width().max(image.height()) > max_dim {
        image = image.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3);
    }
    let image = image.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    let tex = ctx.load_texture(&key, color_image, egui::TextureOptions::LINEAR);
    textures.insert(key, tex.clone());
    Some(tex)
}
