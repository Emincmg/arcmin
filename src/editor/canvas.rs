//! The node-graph canvas: elements, branches and the connections between them.

use std::collections::HashMap;

use arcweave_rust::project::{Board, BranchRef, CondRef, ElementRef, SourceRef, TargetRef};
use eframe::egui;

use super::logic::{self, CondKind};
use super::{EditorState, NODE_H, NODE_MAX, NODE_MIN, NODE_W};
use crate::assets::texture_for;
use crate::content;
use crate::covers;

pub const BRANCH_W: f32 = 290.0;
const BRANCH_HEADER: f32 = 24.0;
const BRANCH_ROW: f32 = 26.0;
const AMBER: egui::Color32 = egui::Color32::from_rgb(240, 190, 90);

/// What a dragged connection was dropped on.
enum Drop {
    Element(ElementRef),
    Branch(BranchRef),
}

pub fn show(ui: &mut egui::Ui, ctx: &egui::Context, state: &mut EditorState) {
    let canvas_rect = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(canvas_rect, 0.0, egui::Color32::from_gray(24));
    let bg_id = ui.id().with("arcmin_canvas_bg");
    let bg_response = ui.interact(canvas_rect, bg_id, egui::Sense::click_and_drag());
    let z = state.zoom;

    let (element_ids, branch_ids): (Vec<ElementRef>, Vec<BranchRef>) =
        match state.project.boards.get(&state.board) {
            Some(Board::Node {
                elements, branches, ..
            }) => (elements.clone(), branches.clone()),
            _ => (vec![], vec![]),
        };

    // ---- geometry -------------------------------------------------------
    let to_screen = |state: &EditorState, (wx, wy): (f32, f32)| {
        canvas_rect.min + state.pan + egui::vec2(wx, wy) * state.zoom
    };

    let mut rects: HashMap<ElementRef, egui::Rect> = HashMap::new();
    for id in &element_ids {
        let pos = state.layout.get_or_insert(id.as_str(), (40.0, 40.0));
        let (nw, nh) = state.layout.size(id.as_str()).unwrap_or((NODE_W, NODE_H));
        rects.insert(
            id.clone(),
            egui::Rect::from_min_size(to_screen(state, pos), egui::vec2(nw, nh) * z),
        );
    }

    let mut branch_rects: HashMap<BranchRef, egui::Rect> = HashMap::new();
    let mut cond_rows: HashMap<BranchRef, Vec<logic::CondInfo>> = HashMap::new();
    let mut cond_handles: HashMap<CondRef, egui::Pos2> = HashMap::new();
    for id in &branch_ids {
        let conds = logic::branch_conditions(&state.project, id);
        let pos = state.layout.get_or_insert(id.as_str(), (40.0, 40.0));
        let height = BRANCH_HEADER + conds.len() as f32 * BRANCH_ROW + 6.0;
        let rect =
            egui::Rect::from_min_size(to_screen(state, pos), egui::vec2(BRANCH_W, height) * z);
        for (i, cond) in conds.iter().enumerate() {
            let y = rect.min.y + (BRANCH_HEADER + (i as f32 + 0.5) * BRANCH_ROW) * z;
            cond_handles.insert(cond.id.clone(), egui::pos2(rect.right(), y));
        }
        branch_rects.insert(id.clone(), rect);
        cond_rows.insert(id.clone(), conds);
    }

    let pointer_over_node = bg_response
        .interact_pointer_pos()
        .map(|p| {
            rects
                .values()
                .chain(branch_rects.values())
                .any(|r| r.contains(p))
        })
        .unwrap_or(false);

    if bg_response.dragged() && !pointer_over_node {
        state.pan += bg_response.drag_delta();
    }
    if bg_response.clicked() && !pointer_over_node {
        state.clear_selection();
    }

    if let Some(hover) = ctx.input(|i| i.pointer.hover_pos())
        && canvas_rect.contains(hover)
    {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            state.zoom = (state.zoom * (1.0 + scroll * 0.001)).clamp(0.3, 2.5);
        }
    }

    let painter = ui.painter_at(canvas_rect);

    // ---- connections ----------------------------------------------------
    let mut segments: Vec<(arcweave_rust::project::ConnRef, egui::Pos2, egui::Pos2)> = Vec::new();
    if let Some(Board::Node { connections, .. }) = state.project.boards.get(&state.board) {
        for conn_ref in connections {
            let Some(conn) = state.project.connections.get(conn_ref) else {
                continue;
            };
            let p0 = match &conn.source {
                SourceRef::Element(e) => rects.get(e).map(|r| r.right_center()),
                SourceRef::Condition(c) => cond_handles.get(c).copied(),
                SourceRef::Jumper(_) => None,
            };
            let p1 = match &conn.target {
                TargetRef::Element(e) => rects.get(e).map(|r| r.left_center()),
                TargetRef::Branch(b) => branch_rects.get(b).map(|r| r.left_center()),
                TargetRef::Jumper(_) => None,
            };
            let (Some(p0), Some(p1)) = (p0, p1) else {
                continue;
            };
            let selected = state.selected_conn.as_ref() == Some(conn_ref);
            let from_condition = matches!(conn.source, SourceRef::Condition(_));
            let color = if selected {
                egui::Color32::YELLOW
            } else if from_condition {
                AMBER
            } else {
                egui::Color32::LIGHT_BLUE
            };
            painter.line_segment(
                [p0, p1],
                egui::Stroke::new(if selected { 3.0 } else { 2.0 }, color),
            );
            segments.push((conn_ref.clone(), p0, p1));
        }
    }
    if bg_response.clicked()
        && let Some(pos) = bg_response.interact_pointer_pos()
        && let Some((conn_ref, _, _)) = segments
            .iter()
            .find(|(_, a, b)| dist_to_segment(pos, *a, *b) < 6.0)
    {
        state.select_connection(conn_ref.clone());
    }
    state.canvas_segments = segments;

    // Rubber band while dragging a new connection.
    let hover_pos = ctx.input(|i| i.pointer.hover_pos());
    if let (Some(from), Some(pos)) = (&state.connecting_from, hover_pos)
        && let Some(fr) = rects.get(from)
    {
        painter.line_segment(
            [fr.right_center(), pos],
            egui::Stroke::new(2.0, egui::Color32::YELLOW),
        );
    }
    if let (Some(cond), Some(pos)) = (&state.connecting_cond, hover_pos)
        && let Some(handle) = cond_handles.get(cond)
    {
        painter.line_segment([*handle, pos], egui::Stroke::new(2.0, AMBER));
    }

    let mut drop_target: Option<Drop> = None;
    let pointer_released = ctx.input(|i| i.pointer.any_released());
    let release_pos = ctx.input(|i| i.pointer.interact_pos());

    // ---- element nodes ----------------------------------------------------
    for id in &element_ids {
        let rect = rects[id];
        let Some(element) = state.project.elements.get(id) else {
            continue;
        };
        let is_start = &state.project.starting_element == id;
        let is_selected = state.selected_element.as_ref() == Some(id);

        let fill = if is_selected {
            egui::Color32::from_rgb(70, 70, 110)
        } else {
            egui::Color32::from_rgb(45, 45, 55)
        };
        let stroke_color = if is_start {
            egui::Color32::GOLD
        } else if is_selected {
            egui::Color32::WHITE
        } else {
            egui::Color32::GRAY
        };
        painter.rect(
            rect,
            6.0,
            fill,
            egui::Stroke::new(2.0, stroke_color),
            egui::StrokeKind::Inside,
        );

        let text_painter = painter.with_clip_rect(rect.shrink(2.0));
        let wrap_width = (rect.width() - 16.0 * z).max(1.0);

        // Cover band along the top of the node, cropped to fill.
        let cover_tex = state
            .covers
            .get(id.as_str())
            .and_then(|asset| covers::file_name(&state.project, asset))
            .and_then(|name| texture_for(&mut state.textures, &state.assets, ctx, &name, 360));
        let mut text_top = 6.0 * z;
        if let Some(tex) = cover_tex {
            let band = egui::Rect::from_min_size(
                rect.min + egui::vec2(2.0, 2.0),
                egui::vec2(rect.width() - 4.0, rect.height() * 0.46),
            );
            let tex_size = tex.size_vec2();
            let band_aspect = band.width() / band.height();
            let tex_aspect = tex_size.x / tex_size.y;
            let uv = if tex_aspect > band_aspect {
                let w = band_aspect / tex_aspect;
                egui::Rect::from_min_max(
                    egui::pos2((1.0 - w) / 2.0, 0.0),
                    egui::pos2((1.0 + w) / 2.0, 1.0),
                )
            } else {
                let h = tex_aspect / band_aspect;
                egui::Rect::from_min_max(
                    egui::pos2(0.0, (1.0 - h) / 2.0),
                    egui::pos2(1.0, (1.0 + h) / 2.0),
                )
            };
            text_painter.image(tex.id(), band, uv, egui::Color32::WHITE);
            text_top = band.height() + 8.0 * z;
        }

        let title = content::strip_html(element.title.as_deref().unwrap_or_default());
        text_painter.text(
            rect.left_top() + egui::vec2(8.0 * z, text_top),
            egui::Align2::LEFT_TOP,
            &title,
            egui::FontId::proportional(14.0 * z),
            egui::Color32::WHITE,
        );
        let body_preview = content::html_to_editor(element.content.as_deref().unwrap_or_default());
        let preview: String = body_preview.chars().take(700).collect();
        let galley = text_painter.layout(
            preview,
            egui::FontId::proportional(11.0 * z),
            egui::Color32::LIGHT_GRAY,
            wrap_width,
        );
        text_painter.galley(
            rect.left_top() + egui::vec2(8.0 * z, text_top + 20.0 * z),
            galley,
            egui::Color32::LIGHT_GRAY,
        );

        let node_id = ui.id().with(("arcmin_node", id.as_str()));
        let node_response = ui.interact(rect, node_id, egui::Sense::click_and_drag());
        if node_response.clicked() || node_response.secondary_clicked() {
            state.select_element(id.clone());
        }
        if node_response.dragged() {
            let (wx, wy) = state.layout.get_or_insert(id.as_str(), (40.0, 40.0));
            let delta = node_response.drag_delta() / state.zoom;
            state.layout.set(id.as_str(), (wx + delta.x, wy + delta.y));
            state.changed(format!("move:{}", id.as_str()));
        }
        super::context_menu::element_menu(&node_response, state, id);

        // Bottom-right grip: drag to resize this node.
        let grip = egui::Rect::from_min_size(
            rect.right_bottom() - egui::vec2(16.0, 16.0),
            egui::vec2(16.0, 16.0),
        );
        let grip_response = ui.interact(
            grip,
            ui.id().with(("arcmin_resize", id.as_str())),
            egui::Sense::drag(),
        );
        let grip_color = if grip_response.hovered() || grip_response.dragged() {
            egui::Color32::WHITE
        } else {
            egui::Color32::GRAY
        };
        for off in [5.0, 9.0, 13.0] {
            painter.line_segment(
                [
                    rect.right_bottom() - egui::vec2(off, 3.0),
                    rect.right_bottom() - egui::vec2(3.0, off),
                ],
                egui::Stroke::new(1.5, grip_color),
            );
        }
        if grip_response.hovered() || grip_response.dragged() {
            ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        }
        if grip_response.dragged() {
            let (w, h) = state.layout.size(id.as_str()).unwrap_or((NODE_W, NODE_H));
            let delta = grip_response.drag_delta() / state.zoom;
            state.layout.set_size(
                id.as_str(),
                (
                    (w + delta.x).clamp(NODE_MIN.0, NODE_MAX.0),
                    (h + delta.y).clamp(NODE_MIN.1, NODE_MAX.1),
                ),
            );
            state.changed(format!("size:{}", id.as_str()));
        }

        let handle_center = rect.right_center();
        painter.circle_filled(handle_center, 6.0, egui::Color32::LIGHT_GREEN);
        let handle_rect = egui::Rect::from_center_size(handle_center, egui::vec2(16.0, 16.0));
        let handle_response = ui.interact(
            handle_rect,
            ui.id().with(("arcmin_handle", id.as_str())),
            egui::Sense::drag(),
        );
        if handle_response.drag_started() {
            state.connecting_from = Some(id.clone());
        }

        let dragging_something = state.connecting_from.as_ref().is_some_and(|f| f != id)
            || state.connecting_cond.is_some();
        if dragging_something && pointer_released && release_pos.is_some_and(|p| rect.contains(p)) {
            drop_target = Some(Drop::Element(id.clone()));
        }
    }

    // ---- branch nodes -----------------------------------------------------
    for id in &branch_ids {
        let rect = branch_rects[id];
        let conds = &cond_rows[id];
        let is_selected = state.selected_branch.as_ref() == Some(id);

        let fill = if is_selected {
            egui::Color32::from_rgb(78, 66, 40)
        } else {
            egui::Color32::from_rgb(54, 47, 34)
        };
        let stroke = if is_selected {
            egui::Color32::WHITE
        } else {
            AMBER
        };
        painter.rect(
            rect,
            8.0 * z.min(1.0),
            fill,
            egui::Stroke::new(2.0, stroke),
            egui::StrokeKind::Inside,
        );

        let clip = painter.with_clip_rect(rect.shrink(2.0));
        let header =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), BRANCH_HEADER * z));
        clip.rect_filled(header.shrink(2.0), 6.0, egui::Color32::from_rgb(96, 78, 40));
        clip.text(
            header.left_center() + egui::vec2(10.0 * z, 0.0),
            egui::Align2::LEFT_CENTER,
            "Branch",
            egui::FontId::proportional(13.0 * z),
            egui::Color32::WHITE,
        );

        for (i, cond) in conds.iter().enumerate() {
            let row_y = rect.min.y + (BRANCH_HEADER + (i as f32 + 0.5) * BRANCH_ROW) * z;
            let kind_color = AMBER;
            let kind_pos = egui::pos2(rect.min.x + 10.0 * z, row_y);
            clip.text(
                kind_pos,
                egui::Align2::LEFT_CENTER,
                cond.kind.label(),
                egui::FontId::proportional(12.0 * z),
                kind_color,
            );
            if let Some(label) = logic::arm_label(&state.project, &cond.output) {
                clip.text(
                    egui::pos2(rect.right() - 16.0 * z, row_y),
                    egui::Align2::RIGHT_CENTER,
                    format!("\"{}\"", label.lines().next().unwrap_or_default()),
                    egui::FontId::proportional(12.0 * z),
                    egui::Color32::from_rgb(140, 190, 255),
                );
            }
            if cond.kind != CondKind::Else {
                let script = cond.script.clone().unwrap_or_default();
                let broken = logic::validate_condition(&script).is_some();
                let shown = script
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&amp;", "&");
                clip.text(
                    egui::pos2(rect.min.x + 62.0 * z, row_y),
                    egui::Align2::LEFT_CENTER,
                    shown,
                    egui::FontId::proportional(12.0 * z),
                    if broken {
                        egui::Color32::from_rgb(240, 110, 110)
                    } else {
                        egui::Color32::LIGHT_GRAY
                    },
                );
            }

            // Output handle for this condition: drag onto an element to re-point it.
            let handle = cond_handles[&cond.id];
            painter.circle_filled(handle, 6.0, AMBER);
            let handle_rect = egui::Rect::from_center_size(handle, egui::vec2(16.0, 16.0));
            let handle_response = ui.interact(
                handle_rect,
                ui.id().with(("arcmin_cond_handle", cond.id.as_str())),
                egui::Sense::drag(),
            );
            if handle_response.drag_started() {
                state.connecting_cond = Some(cond.id.clone());
            }
        }

        let node_id = ui.id().with(("arcmin_branch", id.as_str()));
        let node_response = ui.interact(rect, node_id, egui::Sense::click_and_drag());
        if node_response.clicked() || node_response.secondary_clicked() {
            state.select_branch(id.clone());
        }
        if node_response.dragged() {
            let (wx, wy) = state.layout.get_or_insert(id.as_str(), (40.0, 40.0));
            let delta = node_response.drag_delta() / state.zoom;
            state.layout.set(id.as_str(), (wx + delta.x, wy + delta.y));
            state.changed(format!("move:{}", id.as_str()));
        }
        super::context_menu::branch_menu(&node_response, state, id);

        if state.connecting_from.is_some()
            && pointer_released
            && release_pos.is_some_and(|p| rect.contains(p))
        {
            drop_target = Some(Drop::Branch(id.clone()));
        }
    }

    // ---- finish a connection drag ------------------------------------------
    if pointer_released {
        let from_element = state.connecting_from.take();
        let from_cond = state.connecting_cond.take();
        match (from_element, from_cond, drop_target) {
            (Some(from), _, Some(Drop::Element(to))) => state.connect_elements(&from, &to),
            (Some(from), _, Some(Drop::Branch(branch))) => state.connect_to_branch(&from, &branch),
            (_, Some(cond), Some(Drop::Element(to))) => state.retarget_condition(&cond, &to),
            _ => {}
        }
    }

    super::context_menu::canvas_menu(&bg_response, state, canvas_rect);

    // ---- keyboard ------------------------------------------------------------
    let editing_text = ctx.memory(|m| m.focused().is_some());
    if !editing_text
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        state.delete_selection();
    }
}

fn dist_to_segment(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let apx = p.x - a.x;
    let apy = p.y - a.y;
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 {
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let projx = a.x + abx * t;
    let projy = a.y + aby * t;
    let dx = p.x - projx;
    let dy = p.y - projy;
    (dx * dx + dy * dy).sqrt()
}

/// The connection whose line passes within `tolerance` of `pos`, if any.
pub fn connection_at(
    segments: &[(arcweave_rust::project::ConnRef, egui::Pos2, egui::Pos2)],
    pos: egui::Pos2,
    tolerance: f32,
) -> Option<arcweave_rust::project::ConnRef> {
    segments
        .iter()
        .find(|(_, a, b)| dist_to_segment(pos, *a, *b) < tolerance)
        .map(|(c, _, _)| c.clone())
}
