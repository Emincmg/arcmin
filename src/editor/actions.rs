//! Editing actions shared by the canvas, the inspector and the context menus.
//!
//! Every method that changes the project records exactly one undo step.

use arcweave_rust::project::{Asset, AssetRef, Board, BranchRef, CondRef, ConnRef, ElementRef};
use eframe::egui;

use super::layout::ArrangeNode;
use super::logic::{self, CondKind};
use super::{EditorState, model};

/// Horizontal distance between a node and the element created next to it.
const SPAWN_GAP_X: f32 = 300.0;

impl EditorState {
    pub fn clear_selection(&mut self) {
        self.selected_element = None;
        self.selected_conn = None;
        self.selected_branch = None;
    }

    pub fn select_connection(&mut self, id: ConnRef) {
        self.clear_selection();
        self.selected_conn = Some(id);
    }

    pub fn select_branch(&mut self, id: BranchRef) {
        self.clear_selection();
        self.selected_branch = Some(id);
    }

    /// Drops selections that point at things that no longer exist.
    pub(super) fn prune_selection(&mut self) {
        if self
            .selected_element
            .as_ref()
            .is_some_and(|e| !self.project.elements.contains_key(e))
        {
            self.selected_element = None;
        }
        if self
            .selected_conn
            .as_ref()
            .is_some_and(|c| !self.project.connections.contains_key(c))
        {
            self.selected_conn = None;
        }
        if self
            .selected_branch
            .as_ref()
            .is_some_and(|b| !self.project.branches.contains_key(b))
        {
            self.selected_branch = None;
        }
    }

    pub fn screen_to_world(&self, screen: egui::Pos2, canvas_min: egui::Pos2) -> (f32, f32) {
        let p = (screen - canvas_min - self.pan) / self.zoom;
        (p.x, p.y)
    }

    // ---- creating ----------------------------------------------------------

    pub fn new_element_at(&mut self, pos: (f32, f32)) -> ElementRef {
        let id = model::add_element(&mut self.project, &self.board);
        self.layout.set(id.as_str(), pos);
        self.select_element(id.clone());
        self.action();
        id
    }

    /// A new branch at `pos` whose `if` leads to a fresh element to its right.
    pub fn new_branch_at(&mut self, pos: (f32, f32)) -> BranchRef {
        let target = model::add_element(&mut self.project, &self.board);
        self.layout
            .set(target.as_str(), (pos.0 + SPAWN_GAP_X, pos.1));
        let branch = logic::add_branch(&mut self.project, &self.board, &target);
        self.layout.set(branch.as_str(), pos);
        self.select_branch(branch.clone());
        self.action();
        branch
    }

    /// A new element to the right of `from`, connected to it.
    pub fn add_connected_element(&mut self, from: &ElementRef) {
        let (x, y) = self.layout.get_or_insert(from.as_str(), (40.0, 40.0));
        let (w, _) = self
            .layout
            .size(from.as_str())
            .unwrap_or((super::NODE_W, super::NODE_H));
        let id = model::add_element(&mut self.project, &self.board);
        self.layout.set(id.as_str(), (x + w + 80.0, y));
        let conn = model::add_connection(&mut self.project, &self.board, from, &id);
        self.select_connection(conn);
        self.action();
    }

    /// Lays the whole board out as days (rows) x events (columns) by title.
    pub fn arrange_by_day(&mut self) {
        let Some(Board::Node {
            elements,
            branches,
            connections,
            ..
        }) = self.project.boards.get(&self.board)
        else {
            return;
        };
        let mut nodes: Vec<ArrangeNode> = Vec::new();
        for id in elements {
            let title = self
                .project
                .elements
                .get(id)
                .and_then(|e| e.title.as_deref())
                .map(crate::content::strip_html)
                .unwrap_or_default();
            nodes.push(ArrangeNode {
                id: id.as_str().to_owned(),
                title,
                branch: false,
                size: self
                    .layout
                    .size(id.as_str())
                    .unwrap_or((super::NODE_W, super::NODE_H)),
            });
        }
        for id in branches {
            let rows = logic::branch_conditions(&self.project, id).len();
            nodes.push(ArrangeNode {
                id: id.as_str().to_owned(),
                title: String::new(),
                branch: true,
                size: (super::canvas::BRANCH_W, super::canvas::branch_height(rows)),
            });
        }
        let edges = super::graph_edges(&self.project, connections);
        let loose = self.layout.arrange_by_day(&nodes, &edges);
        self.pan = egui::Vec2::ZERO;
        self.zoom = 1.0;
        self.notice = (loose > 0).then(|| {
            (
                format!("{loose} card(s) have no day.event title (like 2.3) and were put below the grid."),
                false,
            )
        });
        self.action();
    }

    /// Sets (or with `None` clears) an element's cover image.
    pub fn set_cover(&mut self, id: &ElementRef, asset_id: Option<String>) {
        match asset_id {
            Some(asset_id) => self.covers.insert(id.as_str().to_owned(), asset_id),
            None => self.covers.remove(id.as_str()),
        };
        self.action();
    }

    /// Saves, then writes the Arcweave-style Godot bundle (`project.json` +
    /// optional `assets/`) to a zip the author picks.
    pub fn export_for_godot(&mut self) {
        let default_name = format!("{}-godot.zip", self.project.name.trim().replace('/', "-"));
        let Some(dest) = rfd::FileDialog::new()
            .add_filter("Zip archive", &["zip"])
            .set_title("Export for Godot")
            .set_file_name(default_name)
            .save_file()
        else {
            return;
        };
        let include_assets = rfd::MessageDialog::new()
            .set_title("Include assets?")
            .set_description(
                "Add the project's images to the zip? Choose No for a data-only export.",
            )
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes;

        // Export what is on disk, so make sure that is what is on screen.
        self.save();
        self.notice = match super::export::godot_zip(&self.project_path, &dest, include_assets) {
            Ok(report) => Some((
                format!(
                    "Exported for Godot: project.json{} -> {}",
                    if include_assets {
                        format!(" + {} asset file(s)", report.assets)
                    } else {
                        String::new()
                    },
                    dest.display()
                ),
                false,
            )),
            Err(e) => Some((e, true)),
        };
    }

    /// Asks for an image file, copies it into the project's `assets/` folder,
    /// registers it and makes it this element's cover.
    pub fn upload_cover(&mut self, id: &ElementRef) {
        let Some(src) = rfd::FileDialog::new()
            .add_filter("Image", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
            .set_title("Choose a cover image")
            .pick_file()
        else {
            return;
        };
        match self.import_image(&src) {
            Ok(asset_id) => {
                self.set_cover(id, Some(asset_id));
                self.notice = None;
            }
            Err(e) => self.notice = Some((e, true)),
        }
    }

    /// Copies `src` into `<project>/assets/` and returns its asset id. A file
    /// already in the project (same name and size) is reused, not duplicated.
    pub(super) fn import_image(&mut self, src: &std::path::Path) -> Result<String, String> {
        image::image_dimensions(src).map_err(|e| format!("Not a readable image: {e}"))?;
        let dir = self
            .project_path
            .parent()
            .ok_or("The project has no folder.")?
            .join("assets");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let original = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Unusable file name.")?
            .to_owned();
        let src_len = std::fs::metadata(src).map_err(|e| e.to_string())?.len();

        let existing = |name: &str| {
            self.project.assets.iter().find_map(|(id, a)| match a {
                Asset::Node { name: n, .. } if n == name => Some(id.as_str().to_owned()),
                _ => None,
            })
        };
        let (stem, ext) = match original.rsplit_once('.') {
            Some((s, e)) => (s.to_owned(), format!(".{e}")),
            None => (original.clone(), String::new()),
        };
        let mut name = original.clone();
        let mut n = 2;
        loop {
            let target = dir.join(&name);
            match std::fs::metadata(&target) {
                Err(_) => break,
                Ok(m) if m.len() == src_len && existing(&name).is_some() => {
                    return Ok(existing(&name).unwrap_or_default());
                }
                Ok(_) => {
                    name = format!("{stem} ({n}){ext}");
                    n += 1;
                }
            }
        }
        let target = dir.join(&name);
        std::fs::copy(src, &target).map_err(|e| format!("Could not copy the image: {e}"))?;

        let id = model::new_id();
        let asset_ref = AssetRef::from(id.as_str());
        self.project.assets.insert(
            asset_ref.clone(),
            Asset::Node {
                name: name.clone(),
                ty: "image".to_owned(),
            },
        );
        for asset in self.project.assets.values_mut() {
            if let Asset::Root { children, .. } = asset {
                children.push(asset_ref.clone());
                break;
            }
        }
        self.assets.add(name, target);
        Ok(id)
    }

    pub fn duplicate_element(&mut self, id: &ElementRef) {
        let Some(original) = self.project.elements.get(id).cloned() else {
            return;
        };
        let copy = model::add_element(&mut self.project, &self.board);
        if let Some(e) = self.project.elements.get_mut(&copy) {
            e.content = original.content.clone();
            e.title = original.title.as_deref().map(|t| {
                let plain = crate::content::strip_html(t);
                model::wrap_html(&format!("{plain} copy"))
            });
            e.theme = original.theme.clone();
        }
        if let Some(cover) = self.covers.get(id.as_str()).cloned() {
            self.covers.insert(copy.as_str().to_owned(), cover);
        }
        let (x, y) = self.layout.get_or_insert(id.as_str(), (40.0, 40.0));
        self.layout.set(copy.as_str(), (x + 30.0, y + 30.0));
        if let Some(size) = self.layout.size(id.as_str()) {
            self.layout.set_size(copy.as_str(), size);
        }
        self.select_element(copy);
        self.action();
    }

    pub fn add_branch_condition(&mut self, branch: &BranchRef, kind: CondKind) {
        let (bx, by) = self.layout.get_or_insert(branch.as_str(), (40.0, 40.0));
        let rows = logic::branch_conditions(&self.project, branch).len() as f32;
        let target = model::add_element(&mut self.project, &self.board);
        self.layout
            .set(target.as_str(), (bx + SPAWN_GAP_X, by + rows * 150.0));
        if logic::add_condition(&mut self.project, &self.board, branch, kind, &target).is_none() {
            // Refused (e.g. a second `else`): undo the placeholder element.
            model::delete_element(&mut self.project, &self.board, &target);
            self.layout.remove(target.as_str());
            return;
        }
        self.action();
    }

    pub fn insert_branch_on_connection(&mut self, conn: &ConnRef) {
        let Some((from, to)) = self.project.connections.get(conn).map(|c| {
            (
                matches!(c.source, arcweave_rust::project::SourceRef::Element(_)),
                match &c.target {
                    arcweave_rust::project::TargetRef::Element(e) => Some(e.clone()),
                    _ => None,
                },
            )
        }) else {
            return;
        };
        let Some(to) = to.filter(|_| from) else {
            return;
        };
        let (tx, ty) = self.layout.get_or_insert(to.as_str(), (40.0, 40.0));
        if let Some(branch) =
            logic::insert_branch_on_connection(&mut self.project, &self.board, conn)
        {
            // Sit just left of where the old target was; the user can move it.
            self.layout.set(branch.as_str(), (tx - 250.0, ty));
            self.select_branch(branch);
            self.action();
        }
    }

    // ---- connecting --------------------------------------------------------

    pub fn connect_elements(&mut self, from: &ElementRef, to: &ElementRef) {
        let conn = model::add_connection(&mut self.project, &self.board, from, to);
        self.select_connection(conn);
        self.action();
    }

    pub fn connect_to_branch(&mut self, from: &ElementRef, branch: &BranchRef) {
        let conn = logic::connect_to_branch(&mut self.project, &self.board, from, branch);
        self.select_connection(conn);
        self.action();
    }

    pub fn retarget_condition(&mut self, cond: &CondRef, to: &ElementRef) {
        logic::retarget_condition(&mut self.project, cond, to);
        self.action();
    }

    // ---- deleting ----------------------------------------------------------

    pub fn delete_element(&mut self, id: &ElementRef) {
        model::delete_element(&mut self.project, &self.board, id);
        self.layout.remove(id.as_str());
        self.covers.remove(id.as_str());
        self.prune_selection();
        self.action();
    }

    pub fn delete_connection(&mut self, id: &ConnRef) {
        model::delete_connection(&mut self.project, &self.board, id);
        self.prune_selection();
        self.action();
    }

    pub fn delete_branch(&mut self, id: &BranchRef) {
        logic::delete_branch(&mut self.project, &self.board, id);
        self.layout.remove(id.as_str());
        self.prune_selection();
        self.action();
    }

    pub fn delete_condition(&mut self, cond: &CondRef) {
        logic::delete_condition(&mut self.project, &self.board, cond);
        self.prune_selection();
        self.action();
    }

    /// Deletes whatever is selected (the Delete key).
    pub fn delete_selection(&mut self) {
        if let Some(branch) = self.selected_branch.clone() {
            self.delete_branch(&branch);
        } else if let Some(element) = self.selected_element.clone() {
            self.delete_element(&element);
        } else if let Some(conn) = self.selected_conn.clone() {
            self.delete_connection(&conn);
        }
    }
}

#[cfg(test)]
impl EditorState {
    pub(super) fn delete_selection_for(&mut self, branch: &BranchRef) {
        self.select_branch(branch.clone());
        self.delete_selection();
    }
}
