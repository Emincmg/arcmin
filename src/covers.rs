//! Element cover images.
//!
//! `arcweave-rust`'s `Element` doesn't model Arcweave's per-element `assets.cover`
//! field, so covers are read from the raw project json and carried alongside the
//! `Project`. They are written back by `persist::save`.

use std::collections::HashMap;
use std::path::Path;

use arcweave_rust::project::{Asset, AssetRef, Project};

/// element id -> asset id
pub type Covers = HashMap<String, String>;

pub fn load(json_path: &Path) -> Covers {
    let Ok(text) = std::fs::read_to_string(json_path) else {
        return Covers::new();
    };
    let Ok(raw) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Covers::new();
    };
    raw.get("elements")
        .and_then(|v| v.as_object())
        .map(|elements| {
            elements
                .iter()
                .filter_map(|(id, el)| {
                    let asset_id = el.pointer("/assets/cover/id")?.as_str()?;
                    Some((id.clone(), asset_id.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The image filename an asset id points at (resolved later through `AssetIndex`).
pub fn file_name(project: &Project, asset_id: &str) -> Option<String> {
    match project.assets.get(&AssetRef::from(asset_id))? {
        Asset::Node { name, .. } => Some(name.clone()),
        Asset::Root { .. } => None,
    }
}

/// All image assets a cover can be picked from: `(asset id, filename)`, sorted by name.
pub fn choices(project: &Project) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = project
        .assets
        .iter()
        .filter_map(|(id, asset)| match asset {
            Asset::Node { name, ty } if ty.is_empty() || ty == "image" => {
                Some((id.as_str().to_owned(), name.clone()))
            }
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}
