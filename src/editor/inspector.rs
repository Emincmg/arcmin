//! The right-hand "Properties" panel.

use arcweave_rust::project::{BranchRef, CondRef, ConnRef, ElementRef, SourceRef, TargetRef};
use eframe::egui;

use super::logic::{self, CondKind};
use super::{EditorState, model};
use crate::assets::texture_for;
use crate::content;
use crate::covers;
use crate::theme;

const WARN: egui::Color32 = egui::Color32::from_rgb(255, 190, 90);
const ERROR: egui::Color32 = egui::Color32::from_rgb(240, 110, 110);

pub fn show(ui: &mut egui::Ui, state: &mut EditorState) {
    ui.heading("Properties");
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(id) = state.selected_element.clone() {
                show_element(ui, state, &id);
            } else if let Some(id) = state.selected_branch.clone() {
                show_branch(ui, state, &id);
            } else if let Some(id) = state.selected_conn.clone() {
                show_connection(ui, state, &id);
            } else {
                ui.label("Select an element, branch or connection.");
                ui.add_space(8.0);
                ui.label(
                    "Tip: drag from the green handle on a node's right edge and drop it \
                     on an element or branch to connect them.",
                );
                ui.add_space(4.0);
                ui.label("Right-click anywhere for more actions. Press Delete or Backspace to remove the selection.");
            }
        });
}

/// The text to show in an edit field. While the user types, the stored value is
/// a lossy rendering of the text (trailing spaces and blank lines are trimmed),
/// so re-deriving the field from it every frame would eat the space just typed.
/// `buffer` is what the field held after the last edit and which stored value
/// it produced: while the stored value is still that one, keep the raw text.
fn edit_text(
    buffer: Option<(String, String)>,
    stored: &str,
    derive: impl FnOnce() -> String,
) -> String {
    match buffer {
        Some((text, produced)) if produced == stored => text,
        _ => derive(),
    }
}

/// A multi-line field for content that may contain `$ ` script lines, with a live
/// syntax check underneath. Returns the new HTML when the text was edited.
fn content_editor(ui: &mut egui::Ui, key: &str, html: &str, rows: usize) -> Option<String> {
    let buf_id = ui.id().with(("arcmin_edit_buffer", key));
    let buffer = ui.data_mut(|d| d.get_temp::<(String, String)>(buf_id));
    let mut text = edit_text(buffer, html, || content::html_to_editor(html));
    let changed = ui
        .add(
            egui::TextEdit::multiline(&mut text)
                .desired_rows(rows)
                .desired_width(f32::INFINITY),
        )
        .changed();
    let new_html = changed.then(|| content::editor_to_html(&text));
    if let Some(new_html) = &new_html {
        ui.data_mut(|d| d.insert_temp(buf_id, (text, new_html.clone())));
    }
    if let Some(error) = logic::validate_content(new_html.as_deref().unwrap_or(html)) {
        ui.colored_label(ERROR, error);
    }
    new_html
}

fn show_element(ui: &mut egui::Ui, state: &mut EditorState, id: &ElementRef) {
    let Some(element) = state.project.elements.get(id).cloned() else {
        return;
    };
    let is_start = &state.project.starting_element == id;

    ui.label("Title");
    let stored_title = element.title.clone().unwrap_or_default();
    let title_buf = ui.id().with(("arcmin_title_buffer", id.as_str()));
    let buffer = ui.data_mut(|d| d.get_temp::<(String, String)>(title_buf));
    let mut title = edit_text(buffer, &stored_title, || content::strip_html(&stored_title));
    if ui.text_edit_singleline(&mut title).changed() {
        let html = model::wrap_html(&title);
        ui.data_mut(|d| d.insert_temp(title_buf, (title, html.clone())));
        if let Some(e) = state.project.elements.get_mut(id) {
            e.title = Some(html);
        }
        state.changed(format!("title:{}", id.as_str()));
    }

    ui.add_space(8.0);
    ui.label("Text");
    if let Some(html) = content_editor(
        ui,
        &format!("body:{}", id.as_str()),
        element.content.as_deref().unwrap_or("<p></p>"),
        12,
    ) {
        if let Some(e) = state.project.elements.get_mut(id) {
            e.content = Some(html);
        }
        state.changed(format!("body:{}", id.as_str()));
    }
    ui.label(
        egui::RichText::new(
            "Script lines start with `$ ` (e.g. `$ hp += 1`, `$ if hp < 4` … `$ endif`). \
             Use *italic* and **bold**.",
        )
        .small()
        .weak(),
    );

    ui.add_space(12.0);
    show_cover_picker(ui, state, id);

    ui.add_space(12.0);
    if is_start {
        ui.colored_label(egui::Color32::GOLD, "★ Starting element");
    } else if ui.button("Set as Start").clicked() {
        state.project.starting_element = id.clone();
        state.action();
    }

    ui.add_space(8.0);
    if ui.add(theme::danger("Delete Element")).clicked() {
        state.delete_element(id);
    }
}

fn show_connection(ui: &mut egui::Ui, state: &mut EditorState, id: &ConnRef) {
    let Some(conn) = state.project.connections.get(id).cloned() else {
        return;
    };

    if matches!(conn.source, SourceRef::Condition(_)) {
        ui.label("This connection leaves a branch condition.");
        ui.label(
            egui::RichText::new("Select the branch to edit its conditions. Deleting this connection deletes the condition.")
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        if ui.add(theme::danger("Delete Condition")).clicked() {
            state.delete_connection(id);
        }
        return;
    }

    ui.label("Choice text (button label, empty = \"Continue\")");
    if let Some(html) = content_editor(
        ui,
        &format!("label:{}", id.as_str()),
        conn.label.as_deref().unwrap_or("<p></p>"),
        3,
    ) {
        if let Some(c) = state.project.connections.get_mut(id) {
            c.label = (html != "<p></p>").then_some(html);
        }
        state.changed(format!("label:{}", id.as_str()));
    }

    if matches!(conn.target, TargetRef::Branch(_)) {
        ui.label(egui::RichText::new("Leads into a branch.").small().weak());
    }

    ui.add_space(8.0);
    if ui.add(theme::danger("Delete Connection")).clicked() {
        state.delete_connection(id);
    }
}

fn target_name(state: &EditorState, conn: &ConnRef) -> String {
    match state.project.connections.get(conn).map(|c| &c.target) {
        Some(TargetRef::Element(e)) => state
            .project
            .elements
            .get(e)
            .and_then(|el| el.title.as_deref())
            .map(content::strip_html)
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "(untitled)".to_owned()),
        Some(_) => "…".to_owned(),
        None => "(nothing)".to_owned(),
    }
}

fn show_branch(ui: &mut egui::Ui, state: &mut EditorState, id: &BranchRef) {
    if !state.project.branches.contains_key(id) {
        return;
    }
    ui.label(egui::RichText::new("Branch").strong());
    let conditions = logic::branch_conditions(&state.project, id);
    let choice_style = conditions
        .iter()
        .any(|c| logic::arm_label(&state.project, &c.output).is_some());
    ui.label(
        egui::RichText::new(if choice_style {
            "Players pick between the labelled arms whose condition is true. \
             Arms without a label are never offered."
        } else {
            "Players see one choice. The first true condition is followed automatically. \
             Give an arm a choice label to offer it to the player instead."
        })
        .small()
        .weak(),
    );
    ui.add_space(8.0);

    let variables: Vec<String> = logic::list_variables(&state.project)
        .into_iter()
        .map(|v| v.name)
        .collect();

    let mut remove: Option<CondRef> = None;
    for cond in &conditions {
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(cond.kind.label())
                        .strong()
                        .color(egui::Color32::from_rgb(240, 190, 90)),
                );
                ui.label(
                    egui::RichText::new(format!("→ {}", target_name(state, &cond.output))).weak(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if cond.kind != CondKind::If && ui.small_button("Remove").clicked() {
                        remove = Some(cond.id.clone());
                    }
                });
            });
            if cond.kind != CondKind::Else {
                let mut script = cond.script.clone().unwrap_or_default();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut script)
                        .hint_text("e.g. hp > 5 and has_key")
                        .desired_width(f32::INFINITY),
                );
                if response.changed() {
                    logic::set_condition_script(&mut state.project, &cond.id, Some(script.clone()));
                    state.changed(format!("cond:{}", cond.id.as_str()));
                }
                if let Some(error) = logic::validate_condition(&script) {
                    ui.colored_label(ERROR, error);
                } else {
                    let unknown = unknown_identifiers(&script, &variables);
                    if !unknown.is_empty() {
                        ui.colored_label(WARN, format!("Unknown variable: {}", unknown.join(", ")));
                    }
                }
            } else {
                ui.label(
                    egui::RichText::new("Taken when no condition above holds.")
                        .small()
                        .weak(),
                );
            }

            ui.label(
                egui::RichText::new("Choice label (optional)")
                    .small()
                    .weak(),
            );
            let current = state
                .project
                .connections
                .get(&cond.output)
                .and_then(|c| c.label.clone())
                .unwrap_or_else(|| "<p></p>".to_owned());
            if let Some(html) =
                content_editor(ui, &format!("arm:{}", cond.id.as_str()), &current, 1)
            {
                if let Some(c) = state.project.connections.get_mut(&cond.output) {
                    c.label = (html != "<p></p>").then_some(html);
                }
                state.changed(format!("armlabel:{}", cond.id.as_str()));
            }
        });
        ui.add_space(4.0);
    }

    if let Some(cond) = remove {
        state.delete_condition(&cond);
        return;
    }

    let has_labelled_else = conditions
        .iter()
        .any(|c| c.kind == CondKind::Else && logic::arm_label(&state.project, &c.output).is_some());
    if choice_style && !has_labelled_else {
        ui.colored_label(
            WARN,
            "No labelled `else`: if no condition holds, the player gets no choices and the story ends here.",
        );
        ui.add_space(4.0);
    }

    ui.horizontal(|ui| {
        if ui.button("+ Else if").clicked() {
            state.add_branch_condition(id, CondKind::ElseIf);
        }
        let has_else = conditions.iter().any(|c| c.kind == CondKind::Else);
        if ui
            .add_enabled(!has_else, egui::Button::new("+ Else"))
            .clicked()
        {
            state.add_branch_condition(id, CondKind::Else);
        }
    });
    ui.label(
        egui::RichText::new("Each new condition leads to a new element; drag its handle onto another element to re-point it.")
            .small()
            .weak(),
    );

    ui.add_space(12.0);
    if ui.add(theme::danger("Delete Branch")).clicked() {
        state.delete_branch(id);
    }
}

/// Identifiers in a condition that are neither variables nor Arcscript words.
fn unknown_identifiers(script: &str, variables: &[String]) -> Vec<String> {
    let decoded = script
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    let mut unknown: Vec<String> = Vec::new();
    let mut in_string = false;
    let mut word = String::new();
    let flush = |word: &mut String, unknown: &mut Vec<String>| {
        if let Some(first) = word.chars().next() {
            let is_name = first.is_ascii_alphabetic() || first == '_' || first == '$';
            if is_name
                && !logic::RESERVED.contains(&word.as_str())
                && !variables.iter().any(|v| v == word)
                && !unknown.contains(word)
            {
                unknown.push(word.clone());
            }
        }
        word.clear();
    };
    for c in decoded.chars() {
        if c == '"' {
            flush(&mut word, &mut unknown);
            in_string = !in_string;
        } else if in_string {
            continue;
        } else if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
            word.push(c);
        } else {
            flush(&mut word, &mut unknown);
        }
    }
    flush(&mut word, &mut unknown);
    unknown
}

fn show_cover_picker(ui: &mut egui::Ui, state: &mut EditorState, id: &ElementRef) {
    ui.label("Cover image");

    let current_asset = state.covers.get(id.as_str()).cloned();
    let current_name = current_asset
        .as_deref()
        .and_then(|asset| covers::file_name(&state.project, asset));

    if let Some(name) = &current_name {
        match texture_for(&mut state.textures, &state.assets, ui.ctx(), name, 640) {
            Some(tex) => {
                let size = tex.size_vec2();
                let scale = (ui.available_width().min(320.0) / size.x).min(220.0 / size.y);
                ui.image((tex.id(), size * scale));
            }
            None => {
                ui.colored_label(WARN, "Image file not found");
            }
        }
    }

    let mut chosen: Option<Option<String>> = None;
    egui::ComboBox::from_id_salt(("arcmin_cover", id.as_str()))
        .width(ui.available_width() - 8.0)
        .selected_text(current_name.clone().unwrap_or_else(|| "None".to_owned()))
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(current_asset.is_none(), "None")
                .clicked()
            {
                chosen = Some(None);
            }
            for (asset_id, name) in covers::choices(&state.project) {
                let selected = current_asset.as_deref() == Some(asset_id.as_str());
                if ui.selectable_label(selected, name).clicked() {
                    chosen = Some(Some(asset_id));
                }
            }
        });

    if ui.button("Upload image…").clicked() {
        state.upload_cover(id);
    }

    if let Some(choice) = chosen {
        state.set_cover(id, choice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_trailing_space_survives_until_the_stored_value_changes_elsewhere() {
        // User typed "Hello " -> stored html is "<p>Hello</p>".
        let buffer = Some(("Hello ".to_owned(), "<p>Hello</p>".to_owned()));
        let kept = edit_text(buffer.clone(), "<p>Hello</p>", || "Hello".to_owned());
        assert_eq!(kept, "Hello ", "the space must not be trimmed away");

        // Undo (or selecting other content) changed the stored value: re-derive.
        let fresh = edit_text(buffer, "<p>Hi</p>", || "Hi".to_owned());
        assert_eq!(fresh, "Hi");

        // No buffer yet: derive.
        assert_eq!(edit_text(None, "<p>x</p>", || "x".to_owned()), "x");
    }
}
