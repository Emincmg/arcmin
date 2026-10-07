use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Arcweave's own x/y per element isn't preserved by the `arcweave-rust` data
/// model (it only models runtime-relevant fields), so the editor keeps its
/// own canvas layout in a small sidecar file next to the project json.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct LayoutStore {
    #[serde(default)]
    positions: HashMap<String, (f32, f32)>,
    /// Per-node size overrides; nodes without an entry use the editor default.
    #[serde(default)]
    sizes: HashMap<String, (f32, f32)>,
}

impl LayoutStore {
    pub fn path_for(project_path: &Path) -> PathBuf {
        let mut path = project_path.to_path_buf();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("project")
            .to_owned();
        path.set_file_name(format!("{stem}.arcmin-layout.json"));
        path
    }

    pub fn load(project_path: &Path) -> Self {
        std::fs::read_to_string(Self::path_for(project_path))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, project_path: &Path) -> std::io::Result<()> {
        let data = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(Self::path_for(project_path), data)
    }

    /// Returns the stored position for an element, or assigns and stores `default`.
    pub fn get_or_insert(&mut self, id: &str, default: (f32, f32)) -> (f32, f32) {
        *self.positions.entry(id.to_owned()).or_insert(default)
    }

    /// Places every node that has no stored position into a layered grid:
    /// BFS depth from `start` (then from any still-unplaced node) picks the
    /// column, order within the layer picks the row. A column is as wide as its
    /// widest node (`widths`, default 200), so wide nodes never overlap the next column.
    pub fn auto_place(
        &mut self,
        ids: &[String],
        edges: &[(String, String)],
        start: &str,
        widths: &HashMap<String, f32>,
    ) {
        use std::collections::{HashSet, VecDeque};

        let missing: HashSet<&str> = ids
            .iter()
            .map(String::as_str)
            .filter(|id| !self.positions.contains_key(*id))
            .collect();
        if missing.is_empty() {
            return;
        }

        let mut next: HashMap<&str, Vec<&str>> = HashMap::new();
        for (from, to) in edges {
            next.entry(from.as_str()).or_default().push(to.as_str());
        }

        let mut depth: HashMap<&str, usize> = HashMap::new();
        let roots = std::iter::once(start).chain(ids.iter().map(String::as_str));
        for root in roots {
            if !missing.contains(root) || depth.contains_key(root) {
                continue;
            }
            depth.insert(root, 0);
            let mut queue = VecDeque::from([root]);
            while let Some(cur) = queue.pop_front() {
                let d = depth[cur];
                for &n in next.get(cur).into_iter().flatten() {
                    if missing.contains(n) && !depth.contains_key(n) {
                        depth.insert(n, d + 1);
                        queue.push_back(n);
                    }
                }
            }
        }

        const COL_GAP: f32 = 80.0;
        const DEFAULT_W: f32 = 200.0;
        const ROW_H: f32 = 170.0;

        let column_of = |id: &String| depth.get(id.as_str()).copied().unwrap_or(0);
        let width_of = |id: &String| widths.get(id).copied().unwrap_or(DEFAULT_W);
        let placing: Vec<&String> = ids
            .iter()
            .filter(|id| missing.contains(id.as_str()))
            .collect();

        let columns = placing.iter().map(|id| column_of(id)).max().unwrap_or(0) + 1;
        let mut column_width = vec![0.0_f32; columns];
        for id in &placing {
            let col = column_of(id);
            column_width[col] = column_width[col].max(width_of(id));
        }
        let mut column_x = vec![40.0_f32; columns];
        for col in 1..columns {
            column_x[col] = column_x[col - 1] + column_width[col - 1] + COL_GAP;
        }

        let mut rows_used: HashMap<usize, usize> = HashMap::new();
        for id in placing {
            let col = column_of(id);
            let row = rows_used.entry(col).or_default();
            self.positions
                .insert(id.clone(), (column_x[col], 40.0 + *row as f32 * ROW_H));
            *row += 1;
        }
    }

    pub fn set(&mut self, id: &str, pos: (f32, f32)) {
        self.positions.insert(id.to_owned(), pos);
    }

    pub fn remove(&mut self, id: &str) {
        self.positions.remove(id);
        self.sizes.remove(id);
    }

    pub fn size(&self, id: &str) -> Option<(f32, f32)> {
        self.sizes.get(id).copied()
    }

    pub fn set_size(&mut self, id: &str, size: (f32, f32)) {
        self.sizes.insert(id.to_owned(), size);
    }

    /// A free spot below/right of existing nodes, for placing newly created elements.
    pub fn next_spawn_point(&self) -> (f32, f32) {
        let max_y = self
            .positions
            .values()
            .map(|(_, y)| *y)
            .fold(0.0_f32, f32::max);
        (40.0, max_y + 200.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_nodes_push_the_next_column_right() {
        let ids: Vec<String> = ["a", "branch", "b"].iter().map(|s| s.to_string()).collect();
        let edges = vec![("a".into(), "branch".into()), ("branch".into(), "b".into())];
        let widths = HashMap::from([("branch".to_owned(), 290.0)]);

        let mut layout = LayoutStore::default();
        layout.auto_place(&ids, &edges, "a", &widths);
        let x = |id: &str| layout.positions[id].0;

        assert_eq!(x("a"), 40.0);
        assert_eq!(x("branch"), 40.0 + 200.0 + 80.0);
        assert!(
            x("b") >= x("branch") + 290.0,
            "the column after a 290px branch starts beyond its right edge"
        );
    }

    #[test]
    fn already_placed_nodes_are_left_alone() {
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let mut layout = LayoutStore::default();
        layout.set("a", (999.0, 999.0));
        layout.auto_place(&ids, &[("a".into(), "b".into())], "a", &HashMap::new());
        assert_eq!(layout.positions["a"], (999.0, 999.0));
        assert!(layout.positions.contains_key("b"));
    }
}
