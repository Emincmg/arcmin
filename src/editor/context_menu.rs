//! Right-click menus for the canvas, elements, branches and connections.

use arcweave_rust::project::{BranchRef, ElementRef, SourceRef, TargetRef};
use eframe::egui;

use super::EditorState;
use super::canvas::connection_at;
use super::logic::CondKind;

pub fn element_menu(response: &egui::Response, state: &mut EditorState, id: &ElementRef) {
    response.context_menu(|ui| {
        if ui.button("Add Connected Element").clicked() {
            state.add_connected_element(id);
            ui.close();
        }
        if ui.button("Add Branch After").clicked() {
            // A branch to the right of this element, wired in, with its `if` leading on.
            let (x, y) = state.layout.get_or_insert(id.as_str(), (40.0, 40.0));
            let (w, _) = state
                .layout
                .size(id.as_str())
                .unwrap_or((super::NODE_W, super::NODE_H));
            let branch = state.new_branch_at((x + w + 60.0, y));
            state.connect_to_branch(id, &branch);
            state.select_branch(branch);
            ui.close();
        }
        ui.separator();
        let is_start = &state.project.starting_element == id;
        if ui
            .add_enabled(!is_start, egui::Button::new("Set as Start"))
            .clicked()
        {
            state.project.starting_element = id.clone();
            state.action();
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            state.duplicate_element(id);
            ui.close();
        }
        ui.separator();
        if ui.button("Delete").clicked() {
            state.delete_element(id);
            ui.close();
        }
    });
}

pub fn branch_menu(response: &egui::Response, state: &mut EditorState, id: &BranchRef) {
    response.context_menu(|ui| {
        if ui.button("Add Else If").clicked() {
            state.add_branch_condition(id, CondKind::ElseIf);
            ui.close();
        }
        let has_else = super::logic::branch_conditions(&state.project, id)
            .iter()
            .any(|c| c.kind == CondKind::Else);
        if ui
            .add_enabled(!has_else, egui::Button::new("Add Else"))
            .clicked()
        {
            state.add_branch_condition(id, CondKind::Else);
            ui.close();
        }
        ui.separator();
        if ui.button("Delete Branch").clicked() {
            state.delete_branch(id);
            ui.close();
        }
    });
}

/// The menu for empty canvas space, or for a connection when the click landed on one.
pub fn canvas_menu(response: &egui::Response, state: &mut EditorState, canvas_rect: egui::Rect) {
    if response.secondary_clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.context_world = state.screen_to_world(pos, canvas_rect.min);
        state.context_conn = connection_at(&state.canvas_segments, pos, 8.0);
        if let Some(conn) = state.context_conn.clone() {
            state.select_connection(conn);
        }
    }

    response.context_menu(|ui| {
        if let Some(conn) = state.context_conn.clone() {
            let (from_element, to_element) = state
                .project
                .connections
                .get(&conn)
                .map(|c| {
                    (
                        matches!(c.source, SourceRef::Element(_)),
                        matches!(c.target, TargetRef::Element(_)),
                    )
                })
                .unwrap_or((false, false));
            if ui
                .add_enabled(
                    from_element && to_element,
                    egui::Button::new("Insert Branch"),
                )
                .on_disabled_hover_text("Only plain element-to-element connections")
                .clicked()
            {
                state.insert_branch_on_connection(&conn);
                ui.close();
            }
            ui.separator();
            if ui.button("Delete Connection").clicked() {
                state.delete_connection(&conn);
                ui.close();
            }
            return;
        }

        let here = state.context_world;
        if ui.button("New Element Here").clicked() {
            state.new_element_at(here);
            ui.close();
        }
        if ui.button("New Branch Here").clicked() {
            state.new_branch_at(here);
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(state.history.can_undo(), egui::Button::new("Undo"))
            .clicked()
        {
            state.undo();
            ui.close();
        }
        if ui
            .add_enabled(state.history.can_redo(), egui::Button::new("Redo"))
            .clicked()
        {
            state.redo();
            ui.close();
        }
        ui.separator();
        if ui.button("Variables…").clicked() {
            state.show_variables = true;
            ui.close();
        }
        if ui.button("Reset View").clicked() {
            state.pan = egui::Vec2::ZERO;
            state.zoom = 1.0;
            ui.close();
        }
    });
}
