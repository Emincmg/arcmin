use std::collections::HashMap;

use arcweave_rust::project::{
    Board, BoardRef, ConnRef, Connection, Element, ElementRef, Project, SourceRef, TargetRef,
};

pub(super) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Wraps plain text (as edited in the inspector) back into Arcweave-style HTML,
/// one `<p>` per blank-separated paragraph. This is the inverse of
/// `content::strip_html`, used for the simple paragraphs this editor supports.
pub fn wrap_html(plain: &str) -> String {
    let plain = plain.trim();
    if plain.is_empty() {
        return "<p></p>".to_owned();
    }
    plain
        .split("\n\n")
        .map(|para| format!("<p>{}</p>", para.replace('\n', "<br>")))
        .collect::<Vec<_>>()
        .join("")
}

pub fn new_project(name: &str) -> (Project, BoardRef, ElementRef) {
    let board_ref = BoardRef::from(new_id().as_str());
    let root_ref = BoardRef::from(new_id().as_str());
    let start_ref = ElementRef::from(new_id().as_str());

    let mut elements = HashMap::new();
    elements.insert(
        start_ref.clone(),
        Element {
            theme: "default".to_owned(),
            outputs: vec![],
            attributes: vec![],
            components: vec![],
            content: Some(wrap_html("")),
            title: Some(wrap_html("Start")),
        },
    );

    let mut boards = HashMap::new();
    boards.insert(
        root_ref.clone(),
        Board::Root {
            name: "Root".to_owned(),
            root: true,
            children: vec![board_ref.clone()],
        },
    );
    boards.insert(
        board_ref.clone(),
        Board::Node {
            name: "Ana Board".to_owned(),
            notes: vec![],
            jumpers: vec![],
            branches: vec![],
            custom_id: None,
            elements: vec![start_ref.clone()],
            connections: vec![],
        },
    );

    let project = Project {
        name: name.to_owned(),
        starting_element: start_ref.clone(),
        cover: None,
        boards,
        notes: HashMap::new(),
        elements,
        jumpers: HashMap::new(),
        connections: HashMap::new(),
        branches: HashMap::new(),
        components: HashMap::new(),
        attributes: HashMap::new(),
        assets: HashMap::new(),
        variables: HashMap::new(),
        conditions: HashMap::new(),
    };

    (project, board_ref, start_ref)
}

/// The first "real" (non-root) board in the project, i.e. one that actually holds elements.
pub fn find_main_board(project: &Project) -> Option<BoardRef> {
    project
        .boards
        .iter()
        .find(|(_, b)| matches!(b, Board::Node { .. }))
        .map(|(r, _)| r.clone())
}

pub fn board_name(project: &Project, board: &BoardRef) -> String {
    match project.boards.get(board) {
        Some(Board::Node { name, .. }) => name.clone(),
        Some(Board::Root { name, .. }) => name.clone(),
        None => "?".to_owned(),
    }
}

pub fn add_element(project: &mut Project, board: &BoardRef) -> ElementRef {
    let id = ElementRef::from(new_id().as_str());
    project.elements.insert(
        id.clone(),
        Element {
            theme: "default".to_owned(),
            outputs: vec![],
            attributes: vec![],
            components: vec![],
            content: Some(wrap_html("")),
            title: Some(wrap_html("New Element")),
        },
    );
    if let Some(Board::Node { elements, .. }) = project.boards.get_mut(board) {
        elements.push(id.clone());
    }
    id
}

pub fn delete_element(project: &mut Project, board: &BoardRef, element: &ElementRef) {
    // Remove every connection touching this element first.
    let dangling: Vec<ConnRef> = project
        .connections
        .iter()
        .filter(|(_, c)| source_is(c, element) || target_is(c, element))
        .map(|(r, _)| r.clone())
        .collect();
    for conn in &dangling {
        delete_connection(project, board, conn);
    }

    project.elements.remove(element);
    if let Some(Board::Node { elements, .. }) = project.boards.get_mut(board) {
        elements.retain(|e| e != element);
    }
    if &project.starting_element == element
        && let Some(other) = project.elements.keys().next().cloned()
    {
        project.starting_element = other;
    }
}

fn source_is(conn: &Connection, element: &ElementRef) -> bool {
    matches!(&conn.source, SourceRef::Element(e) if e == element)
}

fn target_is(conn: &Connection, element: &ElementRef) -> bool {
    matches!(&conn.target, TargetRef::Element(e) if e == element)
}

pub fn add_connection(
    project: &mut Project,
    board: &BoardRef,
    from: &ElementRef,
    to: &ElementRef,
) -> ConnRef {
    let id = ConnRef::from(new_id().as_str());
    project.connections.insert(
        id.clone(),
        Connection {
            ty: "Straight".to_owned(),
            theme: "flow".to_owned(),
            source: SourceRef::Element(from.clone()),
            target: TargetRef::Element(to.clone()),
            target_face: None,
            label: None,
        },
    );
    if let Some(element) = project.elements.get_mut(from) {
        element.outputs.push(id.clone());
    }
    if let Some(Board::Node { connections, .. }) = project.boards.get_mut(board) {
        connections.push(id.clone());
    }
    id
}

/// Deletes a connection. A connection coming out of a branch condition can't exist
/// on its own, so deleting one removes the condition (and the whole branch when it
/// was the `if`).
pub fn delete_connection(project: &mut Project, board: &BoardRef, conn: &ConnRef) {
    if let Some(SourceRef::Condition(cond)) =
        project.connections.get(conn).map(|c| c.source.clone())
    {
        super::logic::delete_condition(project, board, &cond);
        return;
    }
    remove_connection_raw(project, board, conn);
}

/// Removes the connection and its bookkeeping, without any cascade.
pub(super) fn remove_connection_raw(project: &mut Project, board: &BoardRef, conn: &ConnRef) {
    if let Some(connection) = project.connections.remove(conn)
        && let SourceRef::Element(source) = &connection.source
        && let Some(element) = project.elements.get_mut(source)
    {
        element.outputs.retain(|c| c != conn);
    }
    if let Some(Board::Node { connections, .. }) = project.boards.get_mut(board) {
        connections.retain(|c| c != conn);
    }
}
