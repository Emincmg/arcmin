use std::collections::HashMap;
use std::path::{Path, PathBuf};

use arcweave_rust::project::Project;
use eframe::egui;

use crate::assets::{AssetIndex, texture_for};
use crate::covers::{self, Covers};
use crate::editor::{self, EditorExit, EditorState};
use crate::library;
use crate::session::Session;
use crate::theme;

fn save_path_for(project_path: &Path) -> PathBuf {
    let mut path = project_path.to_path_buf();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_owned();
    path.set_file_name(format!("{stem}.arcmin-save.json"));
    path
}

struct LoadedProject {
    path: PathBuf,
    project: Project,
    assets: AssetIndex,
    covers: Covers,
}

enum Screen {
    Title { error: Option<String> },
    Playing { session: Session },
    Editing { editor: Box<EditorState> },
    Ended,
}

enum LibAction {
    Edit,
    Play,
    Continue,
}

pub struct App {
    loaded: Option<LoadedProject>,
    screen: Screen,
    textures: HashMap<String, egui::TextureHandle>,
    new_name: String,
}

impl Default for App {
    fn default() -> Self {
        let mut app = Self {
            loaded: None,
            screen: Screen::Title { error: None },
            textures: HashMap::new(),
            new_name: String::new(),
        };
        // Reopen the last project straight into the editor.
        if let Some(json) = library::last_project()
            && app.load(json).is_ok()
            && let Ok(screen) = app.edit_screen()
        {
            app.screen = screen;
        }
        app
    }
}

/// Draws a cover scaled to fit `max_w` x `max_h` (never upscaled), with rounded corners.
fn show_cover(
    ui: &mut egui::Ui,
    textures: &mut HashMap<String, egui::TextureHandle>,
    assets: &AssetIndex,
    ctx: &egui::Context,
    filename: &str,
    max_w: f32,
    max_h: f32,
) {
    if let Some(tex) = texture_for(textures, assets, ctx, filename, 1600) {
        let size = tex.size_vec2();
        let scale = (max_w / size.x).min(max_h / size.y).min(1.0);
        ui.add(egui::Image::new((tex.id(), size * scale)).corner_radius(10.0));
    }
}

fn open_project(path: PathBuf) -> Result<LoadedProject, String> {
    let project = Project::from_file(path.to_string_lossy().as_ref())
        .map_err(|e| format!("Could not open project: {e}"))?;
    let assets = AssetIndex::build(&path);
    let covers = covers::load(&path);
    Ok(LoadedProject {
        path,
        project,
        assets,
        covers,
    })
}

impl App {
    fn load(&mut self, json: PathBuf) -> Result<(), String> {
        let loaded = open_project(json)?;
        library::set_last(&loaded.path);
        self.loaded = Some(loaded);
        self.textures.clear();
        Ok(())
    }

    fn edit_screen(&self) -> Result<Screen, String> {
        let loaded = self.loaded.as_ref().ok_or("No project is open")?;
        Ok(Screen::Editing {
            editor: Box::new(EditorState::open(
                loaded.project.clone(),
                loaded.path.clone(),
            )?),
        })
    }

    fn autosave(&self, session: &Session) {
        let Some(loaded) = &self.loaded else { return };
        if let Ok(data) = session.save() {
            let _ = std::fs::write(save_path_for(&loaded.path), data);
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        if let Screen::Editing { editor } = &mut self.screen
            && editor.dirty
        {
            editor.save();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // Take ownership of the current screen so the UI code below is free to call
        // back into `self` (texture cache, loaded project, autosave) without aliasing
        // a borrow held through `self.screen`.
        let screen = std::mem::replace(&mut self.screen, Screen::Ended);
        let mut next_screen = None;

        egui::CentralPanel::default().show(ui, |ui| match screen {
            Screen::Title { error } => {
                let mut error = error;
                let mut action: Option<(PathBuf, LibAction)> = None;
                let mut create_requested = false;
                let mut import_requested = false;
                let mut notice: Option<(String, bool)> = None;
                let projects = library::list();

                ui.vertical_centered(|ui| {
                    ui.add_space(32.0);
                    ui.heading("arcmin");
                    ui.label(egui::RichText::new("Interactive story editor").weak());
                    ui.add_space(24.0);

                    let width = ui.available_width().min(580.0);
                    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                        ui.set_width(width);

                        if projects.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "No projects yet. Create one below or import an Arcweave export.",
                                )
                                .weak(),
                            );
                        }
                        for p in &projects {
                            theme::card(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&p.name).size(17.0).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.add(theme::primary("Edit")).clicked() {
                                                action = Some((p.json.clone(), LibAction::Edit));
                                            }
                                            if ui.button("Play").clicked() {
                                                action = Some((p.json.clone(), LibAction::Play));
                                            }
                                            if ui.button(library::file_manager_label()).clicked() {
                                                library::reveal(&p.json);
                                            }
                                            if p.has_save && ui.button("Continue").clicked() {
                                                action = Some((p.json.clone(), LibAction::Continue));
                                            }
                                        },
                                    );
                                });
                            });
                            ui.add_space(4.0);
                        }

                        ui.add_space(20.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut self.new_name)
                                    .hint_text("New project name")
                                    .desired_width(260.0),
                            );
                            let enter = edit.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let can_create = !self.new_name.trim().is_empty();
                            if ui.add_enabled(can_create, theme::primary("Create")).clicked()
                                || (enter && can_create)
                            {
                                create_requested = true;
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Import Arcweave export…").clicked() {
                                        import_requested = true;
                                    }
                                },
                            );
                        });

                        if let Some(err) = &error {
                            ui.add_space(12.0);
                            ui.colored_label(egui::Color32::LIGHT_RED, err);
                        }
                    });
                });

                let mut open_json: Option<(PathBuf, LibAction)> = action;
                if create_requested {
                    match library::create(&self.new_name) {
                        Ok(json) => {
                            self.new_name.clear();
                            open_json = Some((json, LibAction::Edit));
                        }
                        Err(e) => error = Some(format!("Could not create project: {e}")),
                    }
                }
                if import_requested
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Arcweave export (json)", &["json"])
                        .set_title("Select the Arcweave export (project_settings.json)")
                        .pick_file()
                    {
                        match library::import(&path) {
                            Ok(report) => {
                                notice = Some((report.summary, report.has_problems));
                                open_json = Some((report.json, LibAction::Edit));
                            }
                            Err(e) => error = Some(e),
                        }
                    }

                if let Some((json, what)) = open_json {
                    match self.load(json) {
                        Ok(()) => {
                            let Some(loaded) = self.loaded.as_ref() else {
                                next_screen = Some(Screen::Title {
                                    error: Some("No project is open".to_owned()),
                                });
                                return;
                            };
                            match what {
                                LibAction::Edit => match self.edit_screen() {
                                    Ok(mut screen) => {
                                        if let Screen::Editing { editor } = &mut screen {
                                            editor.notice = notice.take();
                                        }
                                        next_screen = Some(screen);
                                    }
                                    Err(e) => error = Some(e),
                                },
                                LibAction::Play => {
                                    next_screen = Some(Screen::Playing {
                                        session: Session::start(loaded.project.clone()),
                                    })
                                }
                                LibAction::Continue => {
                                    match std::fs::read_to_string(save_path_for(&loaded.path))
                                        .map_err(|e| e.to_string())
                                        .and_then(|saved| {
                                            Session::start_from_save(loaded.project.clone(), &saved)
                                                .map_err(|e| e.to_string())
                                        }) {
                                        Ok(session) => {
                                            next_screen = Some(Screen::Playing { session })
                                        }
                                        Err(e) => {
                                            error = Some(format!("Could not read save: {e}"))
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => error = Some(e),
                    }
                }

                if next_screen.is_none() {
                    next_screen = Some(Screen::Title { error });
                }
            }
            Screen::Playing { mut session } => {
                let project_name = self
                    .loaded
                    .as_ref()
                    .map(|l| l.project.name.clone())
                    .unwrap_or_default();

                let mut leave_to_title = false;
                let mut end_story = false;

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&project_name).weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Save and Exit").clicked() {
                            self.autosave(&session);
                            leave_to_title = true;
                        }
                    });
                });
                ui.separator();

                let title = session.title();
                let body = session.body_text();
                let mut covers = session.current_covers();
                if let (Some(loaded), Some(element)) = (&self.loaded, session.current_element_id())
                    && let Some(name) = loaded
                        .covers
                        .get(&element)
                        .and_then(|asset_id| covers::file_name(&loaded.project, asset_id))
                    {
                        covers.insert(0, name);
                    }
                let choices = session.choices();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // A centred reading column: long lines are hard to read.
                        let col_w = ui.available_width().min(780.0);
                        ui.vertical_centered(|ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(col_w, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.add_space(20.0);
                                    if let Some(loaded) = &self.loaded {
                                        for file in &covers {
                                            ui.vertical_centered(|ui| {
                                                show_cover(
                                                    ui,
                                                    &mut self.textures,
                                                    &loaded.assets,
                                                    ctx,
                                                    file,
                                                    col_w,
                                                    460.0,
                                                );
                                            });
                                            ui.add_space(16.0);
                                        }
                                    }

                                    if !title.is_empty() {
                                        ui.label(egui::RichText::new(title).size(13.0).weak());
                                        ui.add_space(4.0);
                                    }

                                    ui.label(
                                        egui::RichText::new(body)
                                            .size(18.0)
                                            .line_height(Some(28.0)),
                                    );
                                    ui.add_space(28.0);

                                    if choices.is_empty() {
                                        ui.separator();
                                        ui.add_space(8.0);
                                        if ui
                                            .add(theme::primary("— The End — (Back to Main Menu)"))
                                            .clicked()
                                        {
                                            end_story = true;
                                        }
                                    } else {
                                        for choice in &choices {
                                            let clicked = ui
                                                .add_sized(
                                                    [col_w, 46.0],
                                                    egui::Button::new(
                                                        egui::RichText::new(&choice.label).size(16.0),
                                                    ),
                                                )
                                                .clicked();
                                            if clicked {
                                                let _ = session.follow(&choice.conn);
                                                self.autosave(&session);
                                            }
                                        }
                                    }
                                    ui.add_space(40.0);
                                },
                            );
                        });
                    });

                if leave_to_title {
                    next_screen = Some(Screen::Title { error: None });
                } else if end_story {
                    self.autosave(&session);
                    next_screen = Some(Screen::Ended);
                } else {
                    next_screen = Some(Screen::Playing { session });
                }
            }
            Screen::Editing { mut editor } => {
                match editor::show(ui, ctx, &mut editor) {
                    Some(EditorExit::ToTitle) => {
                        if let Some(loaded) = &mut self.loaded {
                            loaded.project = editor.project.clone();
                            loaded.covers = editor.covers.clone();
                        }
                        next_screen = Some(Screen::Title { error: None });
                    }
                    Some(EditorExit::Play) => {
                        if let Some(loaded) = &mut self.loaded {
                            loaded.project = editor.project.clone();
                            loaded.covers = editor.covers.clone();
                            next_screen = Some(Screen::Playing {
                                session: Session::start(loaded.project.clone()),
                            });
                        }
                    }
                    None => {
                        next_screen = Some(Screen::Editing { editor });
                    }
                }
            }
            Screen::Ended => {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.heading("— The End —");
                    ui.add_space(20.0);
                    if ui.button("Main Menu").clicked() {
                        next_screen = Some(Screen::Title { error: None });
                    }
                });
                if next_screen.is_none() {
                    next_screen = Some(Screen::Ended);
                }
            }
        });

        self.screen = next_screen.unwrap_or(Screen::Title { error: None });
    }
}
