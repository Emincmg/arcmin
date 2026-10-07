//! Editing actions shared by the canvas, the inspector and the context menus.
//!
//! Every method that changes the project records exactly one undo step.

use arcweave_rust::project::{BranchRef, CondRef, ConnRef, ElementRef};
use eframe::egui;

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
