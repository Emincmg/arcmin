//! The "Variables" window.

use arcweave_rust::project::Value;
use eframe::egui;

use super::EditorState;
use super::logic::{self, ValueKind};
use crate::theme;

const ERROR: egui::Color32 = egui::Color32::from_rgb(240, 110, 110);

pub fn show(ctx: &egui::Context, state: &mut EditorState) {
    if !state.show_variables {
        return;
    }
    let mut open = true;
    egui::Window::new("Variables")
        .open(&mut open)
        .default_width(560.0)
        .default_pos(egui::pos2(260.0, 160.0))
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(
                    "Variables hold story state (health, items, flags). Scripts and branch \
                     conditions read and change them by name.",
                )
                .small()
                .weak(),
            );
            ui.add_space(6.0);

            let vars = logic::list_variables(&state.project);
            if vars.is_empty() {
                ui.label(egui::RichText::new("No variables yet.").weak());
            } else {
                let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
                let usages = logic::variable_usages(&state.project, &names);

                egui::Grid::new("arcmin_variables")
                    .num_columns(5)
                    .spacing([12.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for header in ["Name", "Type", "Value", "Used", ""] {
                            ui.label(egui::RichText::new(header).small().weak());
                        }
                        ui.end_row();

                        for (v, used) in vars.iter().zip(usages) {
                            row(ui, state, v, used);
                            ui.end_row();
                        }
                    });
            }

            ui.add_space(8.0);
            if ui.add(theme::primary("+ Variable")).clicked() {
                let name = logic::fresh_variable_name(&state.project);
                if logic::add_variable(&mut state.project, &name, Value::Integer(0)).is_ok() {
                    state.action();
                }
            }
        });
    state.show_variables = open;
}

fn row(ui: &mut egui::Ui, state: &mut EditorState, v: &logic::VarInfo, used: usize) {
    // Name: edited in a buffer and committed on Enter / focus loss, so a rename
    // (which rewrites scripts) happens once, not per keystroke.
    let buffer_id = egui::Id::new(("arcmin_var_name", v.id.as_str()));
    let mut name = ui
        .data(|d| d.get_temp::<String>(buffer_id))
        .unwrap_or_else(|| v.name.clone());
    let invalid = name != v.name && logic::var_name_error(&name).is_some();
    let response = ui.add(
        egui::TextEdit::singleline(&mut name)
            .desired_width(150.0)
            .text_color_opt(invalid.then_some(ERROR)),
    );
    if response.changed() {
        ui.data_mut(|d| d.insert_temp(buffer_id, name.clone()));
    }
    if invalid {
        if let Some(e) = logic::var_name_error(&name) {
            response.clone().on_hover_text(e);
        }
    } else if let Some(board) = &v.scope {
        response.clone().on_hover_text(format!(
            "Only visible on the board \"{}\"",
            super::model::board_name(&state.project, board)
        ));
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(buffer_id));
        if name != v.name {
            match logic::rename_variable(&mut state.project, &v.id, &name) {
                Ok(updated) => {
                    state.action();
                    state.notice = Some((
                        if updated > 0 {
                            format!("Renamed to \"{name}\" and updated {updated} reference(s).")
                        } else {
                            format!("Renamed to \"{name}\".")
                        },
                        false,
                    ));
                }
                Err(e) => state.notice = Some((e, true)),
            }
        }
    }

    // Type
    let kind = ValueKind::of(&v.value);
    let mut new_kind = kind;
    egui::ComboBox::from_id_salt(("arcmin_var_kind", v.id.as_str()))
        .width(90.0)
        .selected_text(kind.label())
        .show_ui(ui, |ui| {
            for k in ValueKind::ALL {
                ui.selectable_value(&mut new_kind, k, k.label());
            }
        });
    if new_kind != kind {
        logic::set_variable_value(
            &mut state.project,
            &v.id,
            logic::convert_value(&v.value, new_kind),
        );
        state.action();
    }

    // Value
    let mut value = v.value.clone();
    let changed = match &mut value {
        Value::Integer(i) => ui.add(egui::DragValue::new(i)).changed(),
        Value::Float(f) => ui.add(egui::DragValue::new(f).speed(0.1)).changed(),
        Value::Boolean(b) => ui.checkbox(b, "").changed(),
        Value::String(s) => ui
            .add(egui::TextEdit::singleline(s).desired_width(140.0))
            .changed(),
    };
    if changed {
        logic::set_variable_value(&mut state.project, &v.id, value);
        state.changed(format!("var:{}", v.id.as_str()));
    }

    // Used
    let text = if used == 0 {
        "unused".to_owned()
    } else {
        used.to_string()
    };
    ui.label(egui::RichText::new(text).weak());

    // Delete
    if ui.add(theme::danger("Delete")).clicked() {
        logic::delete_variable(&mut state.project, &v.id);
        if used > 0 {
            state.notice = Some((
                format!(
                    "Deleted \"{}\", which scripts still mention {used} time(s). Undo to restore it.",
                    v.name
                ),
                true,
            ));
        }
        state.action();
    }
}
