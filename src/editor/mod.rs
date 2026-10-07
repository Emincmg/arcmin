pub mod actions;
pub mod canvas;
pub mod context_menu;
pub mod history;
pub mod inspector;
pub mod layout;
pub mod logic;
pub mod model;
pub mod variables;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use arcweave_rust::project::{
    Board, BoardRef, BranchRef, CondRef, ConnRef, ElementRef, Project, SourceRef, TargetRef,
};
use eframe::egui;

use crate::assets::AssetIndex;
use crate::covers::{self, Covers};
use crate::persist;
use crate::theme;
use history::{History, Snapshot};
use layout::LayoutStore;

const NODE_W: f32 = 200.0;
const NODE_H: f32 = 130.0;
const NODE_MIN: (f32, f32) = (150.0, 90.0);
const NODE_MAX: (f32, f32) = (700.0, 600.0);

/// How long unsaved changes may sit before they are written to disk in the background.
const AUTOSAVE_EVERY: Duration = Duration::from_secs(5 * 60);

/// "Cmd" on macOS, "Ctrl" elsewhere (egui maps `Modifiers::COMMAND` the same way).
fn modifier_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    }
}

pub enum EditorExit {
    ToTitle,
    Play,
}

pub struct EditorState {
    pub project: Project,
    pub project_path: PathBuf,
    pub layout: LayoutStore,
    pub board: BoardRef,
    pub selected_element: Option<ElementRef>,
    pub selected_conn: Option<ConnRef>,
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub dirty: bool,
    pub connecting_from: Option<ElementRef>,
    pub selected_branch: Option<BranchRef>,
    /// Set while dragging a branch condition's output handle.
    pub connecting_cond: Option<CondRef>,
    pub show_variables: bool,
    /// Where (in canvas coordinates) the last right-click happened.
    pub context_world: (f32, f32),
    /// The connection under the last right-click, if any.
    pub context_conn: Option<ConnRef>,
    /// Screen-space line of every drawn connection, for hit-testing clicks.
    pub canvas_segments: Vec<(ConnRef, egui::Pos2, egui::Pos2)>,
    last_save: Instant,
    pub covers: Covers,
    assets: AssetIndex,
    textures: HashMap<String, egui::TextureHandle>,
    /// One-off message under the toolbar: (text, is_warning).
    pub notice: Option<(String, bool)>,
    history: History,
}

impl EditorState {
    /// Opens a project in the editor. Fails only if it has no board to show.
    pub fn open(project: Project, project_path: PathBuf) -> Result<Self, String> {
        let board = model::find_main_board(&project)
            .or_else(|| project.boards.keys().next().cloned())
            .ok_or_else(|| "This project has no boards, so there is nothing to edit.".to_owned())?;
        let covers = covers::load(&project_path);
        let assets = AssetIndex::build(&project_path);
        let mut layout = LayoutStore::load(&project_path);
        if let Some(Board::Node {
            elements,
            branches,
            connections,
            ..
        }) = project.boards.get(&board)
        {
            let mut ids: Vec<String> = elements.iter().map(|e| e.as_str().to_owned()).collect();
            ids.extend(branches.iter().map(|b| b.as_str().to_owned()));
            let edges: Vec<(String, String)> = connections
                .iter()
                .filter_map(|c| project.connections.get(c))
                .filter_map(|c| {
                    let from = match &c.source {
                        SourceRef::Element(e) => e.as_str().to_owned(),
                        SourceRef::Condition(cond) => logic::branch_of_condition(&project, cond)?
                            .as_str()
                            .to_owned(),
                        SourceRef::Jumper(_) => return None,
                    };
                    let to = match &c.target {
                        TargetRef::Element(e) => e.as_str().to_owned(),
                        TargetRef::Branch(b) => b.as_str().to_owned(),
                        TargetRef::Jumper(_) => return None,
                    };
                    Some((from, to))
                })
                .collect();
            let widths: HashMap<String, f32> = branches
                .iter()
                .map(|b| (b.as_str().to_owned(), canvas::BRANCH_W))
                .collect();
            layout.auto_place(&ids, &edges, project.starting_element.as_str(), &widths);
        }
        let history = History::new(Snapshot {
            project: project.clone(),
            covers: covers.clone(),
            layout: layout.clone(),
        });
        Ok(Self {
            project,
            project_path,
            layout,
            board,
            selected_element: None,
            selected_conn: None,
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            dirty: false,
            connecting_from: None,
            selected_branch: None,
            connecting_cond: None,
            show_variables: false,
            context_world: (40.0, 40.0),
            context_conn: None,
            canvas_segments: Vec::new(),
            last_save: Instant::now(),
            covers,
            assets,
            textures: HashMap::new(),
            notice: None,
            history,
        })
    }

    pub fn save(&mut self) {
        if let Err(e) = persist::save(&self.project_path, &self.project, &self.covers) {
            self.notice = Some((format!("Could not save project: {e}"), true));
            return;
        }
        let _ = self.layout.save(&self.project_path);
        self.dirty = false;
        self.last_save = Instant::now();
        // Saving always succeeds, but tell the author if the story is structurally broken.
        let problems = logic::check_integrity(&self.project);
        if let Some(first) = problems.first() {
            self.notice = Some((
                format!(
                    "Saved, but the project has {} structural problem(s): {first}",
                    problems.len()
                ),
                true,
            ));
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            project: self.project.clone(),
            covers: self.covers.clone(),
            layout: self.layout.clone(),
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.project = snapshot.project;
        self.covers = snapshot.covers;
        self.layout = snapshot.layout;
        self.connecting_from = None;
        self.connecting_cond = None;
        self.prune_selection();
        self.dirty = true;
    }

    fn commit_history(&mut self) {
        let (project, covers, layout) = (&self.project, &self.covers, &self.layout);
        self.history.commit(|| Snapshot {
            project: project.clone(),
            covers: covers.clone(),
            layout: layout.clone(),
        });
    }

    /// A change that merges with others sharing `key` (typing, dragging).
    fn changed(&mut self, key: String) {
        self.dirty = true;
        self.history.changed(key);
    }

    /// A discrete change that is always its own undo step.
    fn action(&mut self) {
        self.dirty = true;
        self.history.action();
    }

    pub fn undo(&mut self) {
        let current = self.snapshot();
        if let Some(previous) = self.history.undo(current) {
            self.restore(previous);
        }
    }

    pub fn redo(&mut self) {
        let current = self.snapshot();
        if let Some(next) = self.history.redo(current) {
            self.restore(next);
        }
    }

    pub fn select_element(&mut self, id: ElementRef) {
        self.clear_selection();
        self.selected_element = Some(id);
    }
}

pub fn show(ui: &mut egui::Ui, ctx: &egui::Context, state: &mut EditorState) -> Option<EditorExit> {
    let mut exit = None;

    // Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z (or +Y). While a text field has focus, egui's own
    // text undo handles the keys instead.
    if !ctx.memory(|m| m.focused().is_some()) {
        let redo = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
        });
        let undo =
            !redo && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z));
        if redo {
            state.redo();
        } else if undo {
            state.undo();
        }
    }

    if state.dirty && state.last_save.elapsed() >= AUTOSAVE_EVERY {
        state.save();
    }
    // Wake up periodically so the autosave timer fires even when the user is idle.
    ctx.request_repaint_after(Duration::from_secs(15));

    ui.horizontal(|ui| {
        ui.heading(&state.project.name);
        ui.label(format!(
            "· {}",
            model::board_name(&state.project, &state.board)
        ));
        ui.separator();
        if ui.add(theme::primary("+ Element")).clicked() {
            let id = model::add_element(&mut state.project, &state.board);
            let pos = state.layout.next_spawn_point();
            state.layout.set(id.as_str(), pos);
            state.select_element(id);
            state.action();
        }
        if ui.button("+ Branch").clicked() {
            let pos = state.layout.next_spawn_point();
            state.new_branch_at(pos);
        }
        if ui.button("Variables").clicked() {
            state.show_variables = !state.show_variables;
        }
        ui.separator();
        if ui.button("Save").clicked() {
            state.save();
        }
        if ui
            .add_enabled(state.history.can_undo(), egui::Button::new("Undo"))
            .on_hover_text(format!("{}+Z", modifier_name()))
            .clicked()
        {
            state.undo();
        }
        if ui
            .add_enabled(state.history.can_redo(), egui::Button::new("Redo"))
            .on_hover_text(format!("{}+Shift+Z", modifier_name()))
            .clicked()
        {
            state.redo();
        }
        if state.dirty {
            ui.colored_label(egui::Color32::YELLOW, "Unsaved changes");
        } else {
            ui.label(egui::RichText::new("All changes saved").weak());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Back to Menu").clicked() {
                state.save();
                exit = Some(EditorExit::ToTitle);
            }
            if ui.button("▶ Play").clicked() {
                state.save();
                exit = Some(EditorExit::Play);
            }
        });
    });
    ui.separator();

    if let Some((text, is_warning)) = state.notice.clone() {
        let color = if is_warning {
            egui::Color32::from_rgb(255, 190, 90)
        } else {
            egui::Color32::LIGHT_GREEN
        };
        ui.horizontal(|ui| {
            ui.colored_label(color, text);
            if ui.small_button("✕").clicked() {
                state.notice = None;
            }
        });
        ui.separator();
    }

    egui::Panel::right("arcmin_inspector")
        .min_size(300.0)
        .resizable(true)
        .show(ui, |ui| inspector::show(ui, state));

    egui::CentralPanel::default().show(ui, |ui| canvas::show(ui, ctx, state));

    variables::show(ctx, state);

    state.commit_history();

    exit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_new() -> EditorState {
        let dir = std::env::temp_dir().join(format!("arcmin-undo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (project, _, _) = model::new_project("Undo test");
        EditorState::open(project, dir.join("project_settings.json")).unwrap()
    }

    fn element_count(s: &EditorState) -> usize {
        s.project.elements.len()
    }

    #[test]
    fn undo_redo_restores_project_layout_covers_and_selection() {
        let mut s = open_new();
        let start_count = element_count(&s);

        // Add an element, give it a position and a cover.
        let id = model::add_element(&mut s.project, &s.board);
        s.layout.set(id.as_str(), (500.0, 500.0));
        s.select_element(id.clone());
        s.action();
        s.commit_history();
        assert_eq!(element_count(&s), start_count + 1);

        s.covers
            .insert(id.as_str().to_owned(), "asset-1".to_owned());
        s.action();
        s.commit_history();

        // Undo the cover, then the element.
        s.undo();
        assert!(!s.covers.contains_key(id.as_str()), "cover undone");
        assert_eq!(element_count(&s), start_count + 1, "element still there");

        s.undo();
        assert_eq!(element_count(&s), start_count, "element add undone");
        assert!(s.selected_element.is_none(), "stale selection is cleared");
        assert!(s.layout.size(id.as_str()).is_none());

        // Redo both.
        s.redo();
        assert_eq!(element_count(&s), start_count + 1);
        s.redo();
        assert_eq!(
            s.covers.get(id.as_str()).map(String::as_str),
            Some("asset-1")
        );
        assert!(!s.history.can_redo());
    }

    #[test]
    fn dragging_one_node_is_a_single_undo_step() {
        let mut s = open_new();
        let id = s.project.starting_element.clone();
        let before = s.layout.get_or_insert(id.as_str(), (40.0, 40.0));

        // Fifty frames of dragging the same node.
        for i in 1..=50 {
            s.layout.set(id.as_str(), (before.0 + i as f32, before.1));
            s.changed(format!("move:{}", id.as_str()));
            s.commit_history();
        }
        assert_eq!(
            s.layout.get_or_insert(id.as_str(), (0.0, 0.0)).0,
            before.0 + 50.0
        );

        s.undo();
        assert_eq!(s.layout.get_or_insert(id.as_str(), (0.0, 0.0)), before);
        assert!(!s.history.can_undo(), "the whole drag was one step");
    }

    // ---- real frames, run headlessly ------------------------------------------------

    /// Runs one UI frame of the editor with the given input events.
    fn frame(state: &mut EditorState, events: Vec<egui::Event>) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            show(ui, &ctx, state);
        });
        output.drop_without_applying_deltas();
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }]
    }

    /// Cmd on macOS, Ctrl elsewhere, the way a backend would report it.
    fn command() -> egui::Modifiers {
        egui::Modifiers {
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        }
    }

    fn minutes_ago(n: u64) -> Instant {
        Instant::now()
            .checked_sub(Duration::from_secs(n * 60))
            .expect("the machine has been up for longer than that")
    }

    #[test]
    fn autosave_writes_the_project_after_five_minutes_and_not_before() {
        // Four minutes of unsaved changes: nothing is written yet.
        let mut s = open_new();
        s.action();
        s.commit_history();
        s.last_save = minutes_ago(4);
        frame(&mut s, vec![]);
        assert!(!s.project_path.exists(), "too early to autosave");
        assert!(s.dirty);

        // Six minutes: the frame saves in the background.
        s.last_save = minutes_ago(6);
        frame(&mut s, vec![]);
        assert!(s.project_path.exists(), "autosave wrote the project");
        assert!(!s.dirty);
        assert!(
            s.last_save.elapsed() < Duration::from_secs(5),
            "timer restarted"
        );
        let reloaded =
            arcweave_rust::project::Project::from_file(s.project_path.to_string_lossy().as_ref())
                .unwrap();
        assert_eq!(reloaded.name, "Undo test");

        // Nothing changed since: even after another six minutes there is nothing to write.
        std::fs::remove_file(&s.project_path).unwrap();
        s.last_save = minutes_ago(6);
        frame(&mut s, vec![]);
        assert!(
            !s.project_path.exists(),
            "a clean project is never rewritten"
        );
    }

    #[test]
    fn delete_and_backspace_remove_the_selection_and_undo_shortcut_restores_it() {
        for deleting_key in [egui::Key::Delete, egui::Key::Backspace] {
            let mut s = open_new();
            let id = s.new_element_at((300.0, 300.0));
            s.commit_history();
            let before = element_count(&s);

            frame(&mut s, key(deleting_key, Default::default()));
            assert_eq!(
                element_count(&s),
                before - 1,
                "{deleting_key:?} deleted the element"
            );
            assert!(!s.project.elements.contains_key(&id));

            frame(&mut s, key(egui::Key::Z, command()));
            assert_eq!(element_count(&s), before, "undo shortcut brought it back");
            frame(
                &mut s,
                key(
                    egui::Key::Z,
                    egui::Modifiers {
                        shift: true,
                        ..command()
                    },
                ),
            );
            assert_eq!(
                element_count(&s),
                before - 1,
                "redo shortcut deleted it again"
            );
        }
    }

    #[test]
    fn every_selection_state_draws_with_branches_variables_and_covers() {
        use arcweave_rust::project::Value;
        let mut s = open_new();
        let start = s.project.starting_element.clone();
        let next = s.new_element_at((400.0, 40.0));
        s.connect_elements(&start, &next);
        let conn = s.selected_conn.clone().unwrap();
        s.insert_branch_on_connection(&conn);
        let branch = s.selected_branch.clone().unwrap();
        s.add_branch_condition(&branch, logic::CondKind::ElseIf);
        s.add_branch_condition(&branch, logic::CondKind::Else);
        logic::add_variable(&mut s.project, "hp", Value::Integer(3)).unwrap();
        s.project.elements.get_mut(&start).unwrap().content = Some(crate::content::editor_to_html(
            "Hello *world*\n$ hp = hp + 1\n$ if hp >",
        ));
        s.covers
            .insert(start.as_str().to_owned(), "missing-asset".into());
        s.show_variables = true;

        let arm = logic::branch_conditions(&s.project, &branch);
        type Select = Box<dyn Fn(&mut EditorState)>;
        let selections: Vec<Select> = vec![
            Box::new(|s| s.clear_selection()),
            Box::new({
                let id = start.clone();
                move |s| s.select_element(id.clone())
            }),
            Box::new({
                let id = branch.clone();
                move |s| s.select_branch(id.clone())
            }),
            Box::new({
                let id = conn.clone();
                move |s| s.select_connection(id.clone())
            }),
            Box::new({
                let id = arm[0].output.clone();
                move |s| s.select_connection(id.clone())
            }),
        ];
        for select in &selections {
            select(&mut s);
            // Two frames: the second runs with the first one's layout in place.
            frame(&mut s, vec![]);
            frame(&mut s, vec![]);
        }
        assert!(logic::check_integrity(&s.project).is_empty());
    }

    #[test]
    fn branch_actions_are_undoable_and_leave_a_consistent_project() {
        use logic::{CondKind, branch_conditions, check_integrity};

        let mut s = open_new();
        let elements = element_count(&s);

        // A new branch brings its own target element.
        let branch = s.new_branch_at((100.0, 100.0));
        s.commit_history();
        assert_eq!(s.project.branches.len(), 1);
        assert_eq!(element_count(&s), elements + 1);
        assert_eq!(s.selected_branch.as_ref(), Some(&branch));
        assert!(check_integrity(&s.project).is_empty());

        // else if / else each get a fresh element; a second else is refused cleanly.
        s.add_branch_condition(&branch, CondKind::ElseIf);
        s.commit_history();
        s.add_branch_condition(&branch, CondKind::Else);
        s.commit_history();
        let after_else = element_count(&s);
        s.add_branch_condition(&branch, CondKind::Else);
        s.commit_history();
        assert_eq!(
            element_count(&s),
            after_else,
            "refused else leaves no placeholder behind"
        );
        assert_eq!(branch_conditions(&s.project, &branch).len(), 3);
        assert!(check_integrity(&s.project).is_empty());

        // Wire the start element into the branch.
        let start = s.project.starting_element.clone();
        s.connect_to_branch(&start, &branch);
        s.commit_history();
        assert!(check_integrity(&s.project).is_empty());

        // Delete the branch: everything it owned goes, in one undo step.
        let before_delete = s.project.connections.len();
        s.delete_selection_for(&branch);
        s.commit_history();
        assert!(s.project.branches.is_empty());
        assert!(check_integrity(&s.project).is_empty());
        assert!(s.project.connections.len() < before_delete);
        s.undo();
        assert_eq!(s.project.branches.len(), 1);
        assert_eq!(s.project.connections.len(), before_delete);
        assert_eq!(branch_conditions(&s.project, &branch).len(), 3);
        assert!(check_integrity(&s.project).is_empty());
        assert!(
            s.layout.get_or_insert(branch.as_str(), (0.0, 0.0)) != (0.0, 0.0),
            "position restored"
        );
    }

    #[test]
    fn inserting_a_branch_on_a_connection_and_deleting_the_selection() {
        let mut s = open_new();
        let start = s.project.starting_element.clone();
        let next = s.new_element_at((300.0, 40.0));
        s.commit_history();
        s.connect_elements(&start, &next);
        s.commit_history();
        let conn = s.selected_conn.clone().unwrap();

        s.insert_branch_on_connection(&conn);
        s.commit_history();
        let branch = s
            .selected_branch
            .clone()
            .expect("the new branch is selected");
        assert!(logic::check_integrity(&s.project).is_empty());
        // start -> branch -> (if) next
        assert!(matches!(
            s.project.connections[&conn].target,
            TargetRef::Branch(ref b) if b == &branch
        ));

        // Delete key removes whatever is selected.
        s.delete_selection();
        s.commit_history();
        assert!(s.project.branches.is_empty());
        assert!(logic::check_integrity(&s.project).is_empty());
    }

    #[test]
    fn duplicating_an_element_copies_text_cover_and_size() {
        let mut s = open_new();
        let id = s.project.starting_element.clone();
        s.project.elements.get_mut(&id).unwrap().content = Some("<p>Hello</p>".into());
        s.covers.insert(id.as_str().to_owned(), "asset-9".into());
        s.layout.set_size(id.as_str(), (320.0, 200.0));
        let before = element_count(&s);

        s.duplicate_element(&id);
        s.commit_history();
        let copy = s.selected_element.clone().unwrap();
        assert_ne!(copy, id);
        assert_eq!(element_count(&s), before + 1);
        assert_eq!(
            s.project.elements[&copy].content.as_deref(),
            Some("<p>Hello</p>")
        );
        assert_eq!(
            s.covers.get(copy.as_str()).map(String::as_str),
            Some("asset-9")
        );
        assert_eq!(s.layout.size(copy.as_str()), Some((320.0, 200.0)));
        s.undo();
        assert_eq!(element_count(&s), before);
    }

    #[test]
    fn variable_edits_are_undoable() {
        let mut s = open_new();
        let id = logic::add_variable(
            &mut s.project,
            "hp",
            arcweave_rust::project::Value::Integer(5),
        )
        .unwrap();
        s.action();
        s.commit_history();
        logic::set_variable_value(
            &mut s.project,
            &id,
            arcweave_rust::project::Value::Integer(9),
        );
        s.changed(format!("var:{}", id.as_str()));
        s.commit_history();
        s.undo();
        let hp = logic::list_variables(&s.project);
        assert!(matches!(
            hp[0].value,
            arcweave_rust::project::Value::Integer(5)
        ));
        s.undo();
        assert!(logic::list_variables(&s.project).is_empty());
    }

    #[test]
    fn undo_marks_the_project_dirty_so_autosave_persists_it() {
        let mut s = open_new();
        s.action();
        s.commit_history();
        s.dirty = false;
        s.undo();
        assert!(s.dirty);
    }
}
