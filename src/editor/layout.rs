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

    /// Re-places every node on a day/event grid. A title starting `D.E` (or
    /// `D-E`) is event `E` of day `D`: days become rows (top to bottom), events
    /// become columns (left to right). Variants such as `3.3a` / `3.3b` sit to the
    /// right of `3.3`, stacked vertically from the row's top line. A branch joins the
    /// cell of the element leading into it. Nodes that can't be placed that way
    /// go in a block below the grid. Returns how many were put there.
    pub fn arrange_by_day(&mut self, nodes: &[ArrangeNode], edges: &[(String, String)]) -> usize {
        use std::collections::{BTreeMap, BTreeSet};

        const COL_GAP: f32 = 120.0;
        const ROW_GAP: f32 = 90.0;
        const STACK_GAP: f32 = 40.0;
        const SUB_GAP: f32 = 90.0;
        const ORIGIN: f32 = 40.0;

        let index: HashMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        let mut key: HashMap<&str, (u32, u32)> = HashMap::new();
        let mut suffix: HashMap<&str, String> = HashMap::new();
        for n in nodes.iter().filter(|n| !n.branch) {
            if let Some((d, e, suf)) = parse_day_event(&n.title) {
                key.insert(n.id.as_str(), (d, e));
                suffix.insert(n.id.as_str(), suf);
            }
        }
        // Branches follow whatever feeds them (also through chained branches).
        for _ in 0..nodes.len() {
            let mut changed = false;
            for n in nodes.iter().filter(|n| n.branch) {
                if key.contains_key(n.id.as_str()) {
                    continue;
                }
                let found = edges
                    .iter()
                    .filter(|(_, to)| *to == n.id)
                    .find_map(|(from, _)| {
                        Some((key.get(from.as_str())?, suffix.get(from.as_str())?))
                    });
                if let Some((k, suf)) = found {
                    let (k, suf) = (*k, suf.clone());
                    key.insert(n.id.as_str(), k);
                    suffix.insert(n.id.as_str(), suf);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Each cell has a main stack and, to its right, a stack of variants.
        #[derive(Default)]
        struct Cell<'a> {
            main: Vec<&'a ArrangeNode>,
            variants: Vec<&'a ArrangeNode>,
        }
        let mut grouped: BTreeMap<(u32, u32), Vec<&ArrangeNode>> = BTreeMap::new();
        let mut loose: Vec<&ArrangeNode> = Vec::new();
        for n in nodes {
            match key.get(n.id.as_str()) {
                Some(k) => grouped.entry(*k).or_default().push(n),
                None => loose.push(n),
            }
        }
        let suffix_of = |n: &ArrangeNode| suffix.get(n.id.as_str()).cloned().unwrap_or_default();
        let mut cells: BTreeMap<(u32, u32), Cell> = BTreeMap::new();
        for (k, members) in grouped {
            let has_main = members.iter().any(|n| suffix_of(n).is_empty());
            let mut cell = Cell::default();
            for n in members {
                if has_main && !suffix_of(n).is_empty() {
                    cell.variants.push(n);
                } else {
                    cell.main.push(n);
                }
            }
            cell.main
                .sort_by_key(|n| (suffix_of(n), n.branch, index[n.id.as_str()]));
            cell.variants
                .sort_by_key(|n| (suffix_of(n), n.branch, index[n.id.as_str()]));
            cells.insert(k, cell);
        }

        let days: Vec<u32> = cells
            .keys()
            .map(|k| k.0)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let events: Vec<u32> = cells
            .keys()
            .map(|k| k.1)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let col_of = |e: u32| events.iter().position(|x| *x == e).unwrap_or(0);

        let widest = |stack: &[&ArrangeNode]| stack.iter().map(|n| n.size.0).fold(0.0, f32::max);
        let mut main_w = vec![0.0_f32; events.len()];
        let mut var_w = vec![0.0_f32; events.len()];
        for (&(_, e), cell) in &cells {
            let c = col_of(e);
            main_w[c] = main_w[c].max(widest(&cell.main));
            var_w[c] = var_w[c].max(widest(&cell.variants));
        }
        let mut col_x = vec![ORIGIN; events.len()];
        for c in 1..events.len() {
            let prev = main_w[c - 1]
                + if var_w[c - 1] > 0.0 {
                    SUB_GAP + var_w[c - 1]
                } else {
                    0.0
                };
            col_x[c] = col_x[c - 1] + prev + COL_GAP;
        }

        let stack_h = |stack: &[&ArrangeNode]| {
            stack.iter().map(|n| n.size.1).sum::<f32>()
                + STACK_GAP * stack.len().saturating_sub(1) as f32
        };
        let mut y = ORIGIN;
        for day in &days {
            let mut band = 0.0_f32;
            for (&(_, e), cell) in cells.iter().filter(|(k, _)| k.0 == *day) {
                let c = col_of(e);
                let (main_h, var_h) = (stack_h(&cell.main), stack_h(&cell.variants));
                let height = main_h.max(var_h);
                // Main events define the row: they all start at the band's top, and
                // variants hang from that same line, never the other way round.
                let mut my = y;
                for n in &cell.main {
                    self.positions.insert(n.id.clone(), (col_x[c], my));
                    my += n.size.1 + STACK_GAP;
                }
                let mut vy = y;
                let vx = col_x[c] + main_w[c] + SUB_GAP;
                for n in &cell.variants {
                    self.positions.insert(n.id.clone(), (vx, vy));
                    vy += n.size.1 + STACK_GAP;
                }
                band = band.max(height);
            }
            y += band + ROW_GAP;
        }

        // Anything without a day/event goes below, in a wrapped block.
        let (mut x, mut row_h, mut count) = (ORIGIN, 0.0_f32, 0);
        for n in &loose {
            if count == 6 {
                x = ORIGIN;
                y += row_h + ROW_GAP;
                row_h = 0.0;
                count = 0;
            }
            self.positions.insert(n.id.clone(), (x, y));
            x += n.size.0 + COL_GAP;
            row_h = row_h.max(n.size.1);
            count += 1;
        }
        loose.len()
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

/// One node as seen by [`LayoutStore::arrange_by_day`].
pub struct ArrangeNode {
    pub id: String,
    /// Plain-text title (empty for branches).
    pub title: String,
    pub branch: bool,
    /// Size in world units.
    pub size: (f32, f32),
}

/// `"3.3a Waking up"` -> `(3, 3, "a")`; also accepts `3-3` and `3_3`.
pub fn parse_day_event(title: &str) -> Option<(u32, u32, String)> {
    let t = title.trim();
    let digits = |s: &str| s.chars().take_while(char::is_ascii_digit).count();
    let n = digits(t);
    if n == 0 {
        return None;
    }
    let day = t[..n].parse().ok()?;
    let rest = t[n..].strip_prefix(['.', '-', '_'])?;
    let m = digits(rest);
    if m == 0 {
        return None;
    }
    let event = rest[..m].parse().ok()?;
    let suffix: String = rest[m..]
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect::<String>()
        .to_lowercase();
    Some((day, event, suffix))
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

    #[test]
    fn parses_day_event_titles() {
        assert_eq!(parse_day_event("1.1"), Some((1, 1, String::new())));
        assert_eq!(parse_day_event("2-3"), Some((2, 3, String::new())));
        assert_eq!(parse_day_event(" 3.3a "), Some((3, 3, "a".into())));
        assert_eq!(parse_day_event("3.3B Late"), Some((3, 3, "b".into())));
        assert_eq!(parse_day_event("1.1 Wake up"), Some((1, 1, String::new())));
        assert_eq!(parse_day_event("Start"), None);
        assert_eq!(parse_day_event("12"), None);
    }

    fn node(id: &str, title: &str, branch: bool) -> ArrangeNode {
        ArrangeNode {
            id: id.into(),
            title: title.into(),
            branch,
            size: if branch {
                (290.0, 80.0)
            } else {
                (200.0, 130.0)
            },
        }
    }

    #[test]
    fn days_are_rows_events_are_columns_and_variants_sit_right_of_parent() {
        let nodes = vec![
            node("a", "1.1", false),
            node("b", "1.2", false),
            node("c", "2.1", false),
            node("p", "2.2", false),
            node("d", "2.2a", false),
            node("e", "2.2b", false),
            node("f", "2.3", false),
            node("br", "", true),
            node("x", "Intro", false),
        ];
        let edges = vec![("c".into(), "br".into()), ("br".into(), "p".into())];
        let mut l = LayoutStore::default();
        let loose = l.arrange_by_day(&nodes, &edges);
        let p = |id: &str| l.positions[id];

        assert_eq!(loose, 1);
        assert_eq!(p("a").1, p("b").1, "same day shares a row");
        assert!(p("b").0 > p("a").0, "later event is further right");
        assert_eq!(p("a").0, p("c").0, "same event shares a column");
        assert!(p("c").1 >= p("a").1 + 130.0, "day 2 starts below day 1");
        // 2.2a / 2.2b: right of 2.2, stacked with each other, first one level with 2.2.
        assert!(p("d").0 >= p("p").0 + 200.0);
        assert_eq!(p("d").0, p("e").0);
        assert!(p("e").1 >= p("d").1 + 130.0);
        assert_eq!(p("d").1, p("p").1);
        assert_eq!(p("p").1, p("c").1, "main events share the day's line");
        assert_eq!(p("f").1, p("c").1);
        // The next event starts beyond the variants.
        assert!(p("f").0 >= p("d").0 + 200.0);
        // The branch fed by 2.1 stacks under it, still in day 2.
        assert_eq!(p("br").0, p("c").0);
        assert!(p("br").1 >= p("c").1 + 130.0);
        assert!(p("x").1 > p("e").1, "unparseable node sits below the grid");
    }
}
