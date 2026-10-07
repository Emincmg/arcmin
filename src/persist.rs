//! Lossless-as-possible project saving.
//!
//! `arcweave-rust`'s structs only model what its runtime needs, so serialising a
//! `Project` straight to disk would silently drop every field it doesn't know about
//! (element covers, sizes, themes' extras, ...). Instead the freshly serialised
//! project is merged onto the json currently on disk: modeled fields take the new
//! values, unknown fields are kept, and elements/connections that were deleted in
//! the editor disappear.

use std::path::Path;

use arcweave_rust::project::Project;
use serde_json::{Map, Value, json};

use crate::covers::Covers;

/// Top-level maps keyed by id, whose entries are merged one by one.
const COLLECTIONS: &[&str] = &[
    "boards",
    "notes",
    "elements",
    "jumpers",
    "connections",
    "branches",
    "components",
    "attributes",
    "assets",
    "variables",
    "conditions",
];

pub fn save(path: &Path, project: &Project, covers: &Covers) -> std::io::Result<()> {
    let old: Option<Value> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let mut new = serde_json::to_value(project).map_err(std::io::Error::other)?;

    if let (Some(Value::Object(old)), Value::Object(new)) = (&old, &mut new) {
        merge_root(old, new);
    }
    apply_covers(&mut new, covers);

    let data = serde_json::to_string_pretty(&new).map_err(std::io::Error::other)?;
    // Write to a temp file first so a crash mid-write can't corrupt the project.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(tmp, path)
}

fn merge_root(old: &Map<String, Value>, new: &mut Map<String, Value>) {
    for (key, new_val) in new.iter_mut() {
        let (Some(Value::Object(old_coll)), Value::Object(new_coll)) = (old.get(key), new_val)
        else {
            continue;
        };
        if !COLLECTIONS.contains(&key.as_str()) {
            continue;
        }
        for (id, new_entry) in new_coll.iter_mut() {
            if let Some(old_entry) = old_coll.get(id) {
                merge_entry(old_entry, new_entry);
            }
        }
    }
    for (key, old_val) in old {
        new.entry(key.clone()).or_insert_with(|| old_val.clone());
    }
}

/// Keeps every key of `old` that `new` doesn't have, recursing into nested objects.
fn merge_entry(old: &Value, new: &mut Value) {
    let (Value::Object(old), Value::Object(new)) = (old, new) else {
        return;
    };
    for (key, old_val) in old {
        match new.get_mut(key) {
            Some(new_val) => merge_entry(old_val, new_val),
            None => {
                new.insert(key.clone(), old_val.clone());
            }
        }
    }
}

/// Makes each element's `assets.cover` match `covers`, leaving untouched covers as-is.
fn apply_covers(root: &mut Value, covers: &Covers) {
    let Some(elements) = root.get_mut("elements").and_then(Value::as_object_mut) else {
        return;
    };
    for (id, element) in elements.iter_mut() {
        let Some(element) = element.as_object_mut() else {
            continue;
        };
        let current = element
            .get("assets")
            .and_then(|a| a.pointer("/cover/id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        match (covers.get(id), current) {
            (Some(want), Some(have)) if *want == have => {}
            (Some(want), _) => {
                let assets = element.entry("assets").or_insert_with(|| json!({}));
                if let Some(assets) = assets.as_object_mut() {
                    assets.insert("cover".into(), json!({ "id": want, "type": "image" }));
                }
            }
            (None, Some(_)) => {
                if let Some(assets) = element.get_mut("assets").and_then(Value::as_object_mut) {
                    assets.remove("cover");
                    if assets.is_empty() {
                        element.remove("assets");
                    }
                }
            }
            (None, None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Documents why `save` merges instead of serialising directly: the plain
    /// `Project` serialisation has no element covers at all.
    #[test]
    fn plain_serialisation_would_drop_covers() {
        let Ok(src) = std::env::var("ARCMIN_TEST_JSON") else {
            return;
        };
        let project = Project::from_file(&src).unwrap();
        let plain = serde_json::to_value(&project).unwrap();
        let kept = plain["elements"]
            .as_object()
            .unwrap()
            .values()
            .filter(|e| e.pointer("/assets/cover/id").is_some())
            .count();
        let in_export = crate::covers::load(std::path::Path::new(&src)).len();
        assert!(in_export > 0);
        assert_eq!(kept, 0, "plain serialisation unexpectedly kept covers");
    }

    /// Set `ARCMIN_TEST_JSON` to a real Arcweave export to run this against it.
    #[test]
    fn roundtrip_keeps_everything_on_a_real_export() {
        let Ok(src) = std::env::var("ARCMIN_TEST_JSON") else {
            eprintln!("ARCMIN_TEST_JSON not set, skipping");
            return;
        };
        let dir = std::env::temp_dir().join("arcmin-roundtrip-test");
        std::fs::create_dir_all(&dir).unwrap();
        let copy = dir.join("project_settings.json");
        std::fs::copy(&src, &copy).unwrap();

        let project = Project::from_file(copy.to_string_lossy().as_ref()).unwrap();
        let covers = crate::covers::load(&copy);
        assert!(!covers.is_empty(), "expected the export to contain element covers");
        save(&copy, &project, &covers).unwrap();

        let before: Value = serde_json::from_str(&std::fs::read_to_string(&src).unwrap()).unwrap();
        let after: Value = serde_json::from_str(&std::fs::read_to_string(&copy).unwrap()).unwrap();

        // Every cover survived, byte-for-byte equal in structure.
        assert_eq!(covers, crate::covers::load(&copy));
        for section in COLLECTIONS {
            let (Some(b), Some(a)) = (before.get(*section), after.get(*section)) else {
                continue;
            };
            assert_eq!(
                b.as_object().map(|m| m.len()),
                a.as_object().map(|m| m.len()),
                "entry count changed in {section}"
            );
        }
        // No key that existed before may be missing afterwards.
        fn missing(path: String, old: &Value, new: &Value, out: &mut Vec<String>) {
            if let (Value::Object(o), Value::Object(n)) = (old, new) {
                for (k, v) in o {
                    match n.get(k) {
                        Some(nv) => missing(format!("{path}/{k}"), v, nv, out),
                        None => out.push(format!("{path}/{k}")),
                    }
                }
            }
        }
        let mut lost = vec![];
        missing(String::new(), &before, &after, &mut lost);
        assert!(lost.is_empty(), "lost keys: {:?}", &lost[..lost.len().min(10)]);
    }
}
