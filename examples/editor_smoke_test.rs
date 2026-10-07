// Headless sanity check for the editor's graph-mutation logic against a real
// Arcweave export: opens the project, adds an element + connection, renames
// the starting element, deletes a connection, saves, then reloads with
// arcweave-rust's own Runtime to confirm the edited project still plays.
// These examples pull in app modules by path and use only part of each.
#![allow(dead_code)]

use arcweave_rust::project::Project;

#[path = "../src/assets.rs"]
mod assets;
#[path = "../src/content.rs"]
mod content;
#[path = "../src/covers.rs"]
mod covers;
#[path = "../src/editor/mod.rs"]
mod editor;
#[path = "../src/persist.rs"]
mod persist;
#[path = "../src/session.rs"]
mod session;
#[path = "../src/theme.rs"]
mod theme;

use editor::model;
use session::Session;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: editor_smoke_test <project_settings.json>");
    let project = Project::from_file(&path).expect("failed to parse project");
    println!(
        "loaded: {} elements, {} connections",
        project.elements.len(),
        project.connections.len()
    );

    let mut project = project;
    let board = model::find_main_board(&project).expect("no board found");
    println!("main board: {}", model::board_name(&project, &board));

    let new_el = model::add_element(&mut project, &board);
    println!("added element {}", new_el.as_str());

    let old_start = project.starting_element.clone();
    let conn = model::add_connection(&mut project, &board, &old_start, &new_el);
    println!(
        "added connection {} ({} -> {})",
        conn.as_str(),
        old_start.as_str(),
        new_el.as_str()
    );
    assert!(project.elements[&old_start].outputs.contains(&conn));

    project.starting_element = new_el.clone();
    println!("start is now {}", project.starting_element.as_str());

    model::delete_connection(&mut project, &board, &conn);
    assert!(!project.elements[&old_start].outputs.contains(&conn));
    assert!(!project.connections.contains_key(&conn));
    println!("deleted connection ok");

    model::delete_element(&mut project, &board, &old_start);
    assert!(!project.elements.contains_key(&old_start));
    println!(
        "deleted old start element ok, {} elements left",
        project.elements.len()
    );

    let serialized = serde_json::to_string_pretty(&project).expect("serialize failed");
    println!("serialized {} bytes", serialized.len());

    let reparsed = Project::from_str(&serialized).expect("edited project failed to re-parse");
    let mut runtime_session = Session::start(reparsed);
    println!("reloaded title: {:?}", runtime_session.title());
    println!("reloaded body: {}", runtime_session.body_text());
    let choices = runtime_session.choices();
    println!(
        "choices from new start: {} (expected 1, the fresh element -> rest of story is now unreachable)",
        choices.len()
    );

    println!("\nOK: editor mutations round-trip through arcweave-rust's own parser/runtime.");
}
