// Headless sanity check against a real Arcweave export: opens the project,
// walks a few choices, saves, reloads from the save, and prints what it sees.
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

use session::Session;

fn main() {
    let path = std::env::args().nth(1).expect("usage: smoke_test <project_settings.json>");
    let project = Project::from_file(&path).expect("failed to parse project");
    println!("Project: {}", project.name);
    println!("Elements: {}", project.elements.len());
    println!("Connections: {}", project.connections.len());

    let mut session = Session::start(project.clone());
    println!("\n--- start ---");
    println!("title: {:?}", session.title());
    println!("body: {}", session.body_text());

    for step in 0..3 {
        let choices = session.choices();
        println!("\nstep {step}: {} choice(s)", choices.len());
        for c in &choices {
            println!("  -> {}", c.label);
        }
        let Some(first) = choices.into_iter().next() else {
            println!("(dead end)");
            break;
        };
        session.follow(&first.conn).expect("follow failed");
        println!("moved to title: {:?}", session.title());
        println!("body: {}", session.body_text());
    }

    let saved = session.save().expect("save failed");
    println!("\n--- save/load roundtrip ---");
    println!("saved {} bytes", saved.len());
    let session2 = Session::start_from_save(project, &saved).expect("load failed");
    println!("reloaded title: {:?}", session2.title());
    assert_eq!(session.title(), session2.title());
    println!("OK: resumed state matches.");
}
