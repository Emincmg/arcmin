//! Story logic authoring: variables, branches/conditions and Arcscript validation.
//!
//! Everything here works on the plain `Project` data and knows nothing about the
//! UI, so it is tested against `arcweave-rust`'s real `Runtime`.

use arcweave_rust::project::{
    Board, BoardRef, Branch, BranchConditions, BranchRef, CondRef, Condition, ConnRef, Connection,
    ElementRef, Project, SourceRef, TargetRef, Value, VarRef, Variable,
};
use arcweave_rust::script::ast::Input;
use arcweave_rust::script::parser;

use super::model::{new_id, remove_connection_raw};

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Words Arcscript treats specially; a variable called one of these can't be used.
pub const RESERVED: &[&str] = &[
    "abs",
    "and",
    "else",
    "elseif",
    "endif",
    "false",
    "if",
    "is",
    "max",
    "min",
    "not",
    "or",
    "random",
    "reset",
    "resetAll",
    "resetVisits",
    "roll",
    "round",
    "show",
    "sqr",
    "sqrt",
    "true",
    "visits",
];

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

pub fn var_name_error(name: &str) -> Option<String> {
    let mut chars = name.chars();
    match chars.next() {
        None => Some("Name can't be empty".to_owned()),
        Some(c) if !is_ident_start(c) => Some("Must start with a letter, _ or $".to_owned()),
        _ if !name.chars().all(is_ident_char) => {
            Some("Only letters, digits, _ and $ are allowed".to_owned())
        }
        _ if RESERVED.contains(&name) => Some(format!("\"{name}\" is a reserved word")),
        _ => None,
    }
}

fn decode(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// Syntax check for a branch condition such as `hp > 5 and has_key`.
pub fn validate_condition(script: &str) -> Option<String> {
    if script.trim().is_empty() {
        return Some("Condition is empty".to_owned());
    }
    match parser::input(&decode(script)) {
        Ok(("", Input::Branch(_))) => None,
        Ok(("", Input::Script(_))) => Some("Expected a condition, not a statement".to_owned()),
        Ok((rest, _)) => Some(format!("Unexpected input near \"{}\"", rest.trim())),
        Err(e) => Some(format!("Syntax error: {e}")),
    }
}

/// Syntax check for element content / connection labels (text with `$ ` script lines).
pub fn validate_content(html: &str) -> Option<String> {
    match parser::input(&decode(html)) {
        Ok(("", Input::Script(_))) => None,
        Ok(("", Input::Branch(_))) => Some("Expected content, not a condition".to_owned()),
        Ok((rest, _)) => Some(format!("Unexpected input near \"{}\"", rest.trim())),
        Err(e) => Some(format!("Script error: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Variables
// ---------------------------------------------------------------------------

pub struct VarInfo {
    pub id: VarRef,
    pub name: String,
    pub value: Value,
    /// `Some` for variables that belong to a single board.
    pub scope: Option<BoardRef>,
}

pub fn list_variables(project: &Project) -> Vec<VarInfo> {
    let mut out: Vec<VarInfo> = project
        .variables
        .iter()
        .filter_map(|(id, v)| match v {
            Variable::Global { name, value } => Some(VarInfo {
                id: id.clone(),
                name: name.clone(),
                value: value.clone(),
                scope: None,
            }),
            Variable::Board {
                name,
                board_id,
                value,
            } => Some(VarInfo {
                id: id.clone(),
                name: name.clone(),
                value: value.clone(),
                scope: Some(board_id.clone()),
            }),
            Variable::Root { .. } => None,
        })
        .collect();
    out.sort_by_key(|v| v.name.to_lowercase());
    out
}

fn name_taken(project: &Project, name: &str, except: Option<&VarRef>) -> bool {
    list_variables(project)
        .iter()
        .any(|v| v.name == name && Some(&v.id) != except)
}

pub fn add_variable(project: &mut Project, name: &str, value: Value) -> Result<VarRef, String> {
    if let Some(e) = var_name_error(name) {
        return Err(e);
    }
    if name_taken(project, name, None) {
        return Err(format!("A variable named \"{name}\" already exists"));
    }
    let id = VarRef::from(new_id().as_str());
    project.variables.insert(
        id.clone(),
        Variable::Global {
            name: name.to_owned(),
            value,
        },
    );
    let root = project.variables.iter_mut().find_map(|(_, v)| match v {
        Variable::Root { children, .. } => Some(children),
        _ => None,
    });
    match root {
        Some(children) => children.push(id.clone()),
        None => {
            project.variables.insert(
                VarRef::from(new_id().as_str()),
                Variable::Root {
                    root: true,
                    children: vec![id.clone()],
                },
            );
        }
    }
    Ok(id)
}

/// A name for a new variable that isn't taken yet (`variable`, `variable2`, ...).
pub fn fresh_variable_name(project: &Project) -> String {
    let mut n = 1;
    loop {
        let name = if n == 1 {
            "variable".to_owned()
        } else {
            format!("variable{n}")
        };
        if !name_taken(project, &name, None) {
            return name;
        }
        n += 1;
    }
}

pub fn set_variable_value(project: &mut Project, id: &VarRef, new: Value) {
    match project.variables.get_mut(id) {
        Some(Variable::Global { value, .. }) | Some(Variable::Board { value, .. }) => *value = new,
        _ => {}
    }
}

/// Renames a variable and rewrites every script that mentions it.
/// Returns how many references were updated.
pub fn rename_variable(
    project: &mut Project,
    id: &VarRef,
    new_name: &str,
) -> Result<usize, String> {
    let old = match project.variables.get(id) {
        Some(Variable::Global { name, .. }) | Some(Variable::Board { name, .. }) => name.clone(),
        _ => return Err("No such variable".to_owned()),
    };
    if old == new_name {
        return Ok(0);
    }
    if let Some(e) = var_name_error(new_name) {
        return Err(e);
    }
    if name_taken(project, new_name, Some(id)) {
        return Err(format!("A variable named \"{new_name}\" already exists"));
    }
    let updated = rewrite_code(project, &mut |code| {
        let (text, n) = replace_ident(code, &old, new_name);
        (n > 0).then_some((text, n))
    });
    if let Some(Variable::Global { name, .. }) | Some(Variable::Board { name, .. }) =
        project.variables.get_mut(id)
    {
        *name = new_name.to_owned();
    }
    Ok(updated)
}

pub fn delete_variable(project: &mut Project, id: &VarRef) {
    project.variables.remove(id);
    for v in project.variables.values_mut() {
        if let Variable::Root { children, .. } = v {
            children.retain(|c| c != id);
        }
    }
}

/// Every piece of script code in the project (read-only counterpart of `rewrite_code`).
fn code_snippets(project: &Project) -> Vec<String> {
    fn spans(html: &str, out: &mut Vec<String>) {
        let mut rest = html;
        while let Some(start) = rest.find("<code") {
            let Some(open_end) = rest[start..].find('>').map(|i| start + i + 1) else {
                break;
            };
            let Some(close) = rest[open_end..].find("</code>").map(|i| open_end + i) else {
                break;
            };
            out.push(rest[open_end..close].to_owned());
            rest = &rest[close..];
        }
    }
    let mut out = Vec::new();
    for e in project.elements.values() {
        if let Some(c) = &e.content {
            spans(c, &mut out);
        }
    }
    for c in project.connections.values() {
        if let Some(l) = &c.label {
            spans(l, &mut out);
        }
    }
    out.extend(project.conditions.values().filter_map(|c| c.script.clone()));
    out
}

/// How many times scripts in the project mention each of `names` (same order).
pub fn variable_usages(project: &Project, names: &[&str]) -> Vec<usize> {
    let snippets = code_snippets(project);
    names
        .iter()
        .map(|name| {
            snippets
                .iter()
                .map(|code| replace_ident(code, name, name).1)
                .sum()
        })
        .collect()
}

/// How many times scripts in the project mention `name`.
#[cfg(test)]
pub fn variable_usage(project: &Project, name: &str) -> usize {
    variable_usages(project, &[name])[0]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueKind {
    Integer,
    Float,
    Boolean,
    String,
}

impl ValueKind {
    pub const ALL: [ValueKind; 4] = [
        ValueKind::Integer,
        ValueKind::Float,
        ValueKind::Boolean,
        ValueKind::String,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ValueKind::Integer => "integer",
            ValueKind::Float => "float",
            ValueKind::Boolean => "boolean",
            ValueKind::String => "string",
        }
    }

    pub fn of(value: &Value) -> Self {
        match value {
            Value::Integer(_) => ValueKind::Integer,
            Value::Float(_) => ValueKind::Float,
            Value::Boolean(_) => ValueKind::Boolean,
            Value::String(_) => ValueKind::String,
        }
    }
}

/// Converts a variable's value to another type, keeping as much meaning as possible.
pub fn convert_value(value: &Value, to: ValueKind) -> Value {
    let number = match value {
        Value::Integer(i) => *i as f64,
        Value::Float(f) => *f as f64,
        Value::Boolean(b) => f64::from(u8::from(*b)),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
    };
    match to {
        ValueKind::Integer => Value::Integer(number.round() as i32),
        ValueKind::Float => Value::Float(number as f32),
        ValueKind::Boolean => Value::Boolean(match value {
            Value::String(s) => s.trim().eq_ignore_ascii_case("true") || number != 0.0,
            _ => number != 0.0,
        }),
        ValueKind::String => Value::String(match value {
            Value::Integer(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::String(s) => s.clone(),
        }),
    }
}

/// Replaces whole-word occurrences of `old` with `new` in one piece of script code,
/// ignoring string literals, HTML entities and tags. Returns the new text and count.
fn replace_ident(code: &str, old: &str, new: &str) -> (String, usize) {
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len());
    let mut count = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                // copy a string literal untouched
                out.push(c);
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                    if chars[i - 1] == '"' {
                        break;
                    }
                }
            }
            '&' | '<' => {
                // entity (`&lt;`) or tag (`<span ...>`): copy through its terminator
                let end = if c == '&' { ';' } else { '>' };
                let stop = chars[i..].iter().position(|&x| x == end).map(|p| i + p + 1);
                let stop = stop.unwrap_or(i + 1);
                out.extend(&chars[i..stop]);
                i = stop;
            }
            c if is_ident_start(c) => {
                let start = i;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                if word == old {
                    out.push_str(new);
                    count += 1;
                } else {
                    out.push_str(&word);
                }
            }
            c if c.is_ascii_digit() => {
                // numbers like `3d6` or `1e5` must not be split into an identifier
                while i < chars.len() && is_ident_char(chars[i]) {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    (out, count)
}

/// Applies `f` to every piece of script code in the project: the `<pre><code>` blocks
/// of element content and connection labels, and every branch condition.
/// `f` returns `Some((new_code, n))` to replace it. Returns the sum of `n`.
fn rewrite_code(
    project: &mut Project,
    f: &mut dyn FnMut(&str) -> Option<(String, usize)>,
) -> usize {
    fn in_html(
        html: &str,
        f: &mut dyn FnMut(&str) -> Option<(String, usize)>,
    ) -> Option<(String, usize)> {
        let mut out = String::with_capacity(html.len());
        let mut total = 0;
        let mut rest = html;
        while let Some(start) = rest.find("<code") {
            let Some(open_end) = rest[start..].find('>').map(|i| start + i + 1) else {
                break;
            };
            let Some(close) = rest[open_end..].find("</code>").map(|i| open_end + i) else {
                break;
            };
            out.push_str(&rest[..open_end]);
            let code = &rest[open_end..close];
            match f(code) {
                Some((new, n)) => {
                    out.push_str(&new);
                    total += n;
                }
                None => out.push_str(code),
            }
            rest = &rest[close..];
        }
        out.push_str(rest);
        (total > 0).then_some((out, total))
    }

    let mut total = 0;
    for element in project.elements.values_mut() {
        if let Some((new, n)) = element.content.as_deref().and_then(|c| in_html(c, f)) {
            element.content = Some(new);
            total += n;
        }
    }
    for conn in project.connections.values_mut() {
        if let Some((new, n)) = conn.label.as_deref().and_then(|c| in_html(c, f)) {
            conn.label = Some(new);
            total += n;
        }
    }
    for cond in project.conditions.values_mut() {
        if let Some((new, n)) = cond.script.as_deref().and_then(&mut *f) {
            cond.script = Some(new);
            total += n;
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Branches and conditions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CondKind {
    If,
    ElseIf,
    Else,
}

impl CondKind {
    pub fn label(self) -> &'static str {
        match self {
            CondKind::If => "if",
            CondKind::ElseIf => "else if",
            CondKind::Else => "else",
        }
    }
}

pub struct CondInfo {
    pub id: CondRef,
    pub kind: CondKind,
    pub script: Option<String>,
    pub output: ConnRef,
}

/// A branch's conditions in evaluation order: `if`, every `else if`, then `else`.
pub fn branch_conditions(project: &Project, branch: &BranchRef) -> Vec<CondInfo> {
    let Some(b) = project.branches.get(branch) else {
        return vec![];
    };
    let c = &b.conditions;
    let mut order = vec![(c.if_condition.clone(), CondKind::If)];
    order.extend(
        c.else_if_conditions
            .iter()
            .map(|id| (id.clone(), CondKind::ElseIf)),
    );
    order.extend(
        c.else_condition
            .iter()
            .map(|id| (id.clone(), CondKind::Else)),
    );
    order
        .into_iter()
        .filter_map(|(id, kind)| {
            let cond = project.conditions.get(&id)?;
            Some(CondInfo {
                id,
                kind,
                script: cond.script.clone(),
                output: cond.output.clone(),
            })
        })
        .collect()
}

/// The plain-text choice label on a branch arm's outgoing connection, if it has one.
/// Arms with a label are offered to the player as choices.
pub fn arm_label(project: &Project, output: &ConnRef) -> Option<String> {
    let html = project.connections.get(output)?.label.as_deref()?;
    let text = crate::content::strip_html(html);
    (!text.trim().is_empty()).then_some(text)
}

pub fn branch_of_condition(project: &Project, cond: &CondRef) -> Option<BranchRef> {
    project
        .branches
        .iter()
        .find(|(_, b)| {
            let c = &b.conditions;
            &c.if_condition == cond
                || c.else_condition.as_ref() == Some(cond)
                || c.else_if_conditions.contains(cond)
        })
        .map(|(id, _)| id.clone())
}

fn new_condition(
    project: &mut Project,
    board: &BoardRef,
    script: Option<String>,
    target: &ElementRef,
) -> CondRef {
    let cond_id = CondRef::from(new_id().as_str());
    let conn_id = ConnRef::from(new_id().as_str());
    project.connections.insert(
        conn_id.clone(),
        Connection {
            ty: "Straight".to_owned(),
            theme: "flow".to_owned(),
            source: SourceRef::Condition(cond_id.clone()),
            target: TargetRef::Element(target.clone()),
            target_face: None,
            label: None,
        },
    );
    if let Some(Board::Node { connections, .. }) = project.boards.get_mut(board) {
        connections.push(conn_id.clone());
    }
    project.conditions.insert(
        cond_id.clone(),
        Condition {
            output: conn_id,
            script,
        },
    );
    cond_id
}

/// Creates a branch whose `if` leads to `target`.
pub fn add_branch(project: &mut Project, board: &BoardRef, target: &ElementRef) -> BranchRef {
    let if_condition = new_condition(project, board, Some("true".to_owned()), target);
    let id = BranchRef::from(new_id().as_str());
    project.branches.insert(
        id.clone(),
        Branch {
            theme: "default".to_owned(),
            conditions: BranchConditions {
                if_condition,
                else_condition: None,
                else_if_conditions: vec![],
            },
        },
    );
    if let Some(Board::Node { branches, .. }) = project.boards.get_mut(board) {
        branches.push(id.clone());
    }
    id
}

/// Splices a branch into an element -> element connection: `A -> B` becomes
/// `A -> branch` with the branch's `if` leading on to `B`.
pub fn insert_branch_on_connection(
    project: &mut Project,
    board: &BoardRef,
    conn: &ConnRef,
) -> Option<BranchRef> {
    let target = match &project.connections.get(conn)?.target {
        TargetRef::Element(e) => e.clone(),
        _ => return None,
    };
    if !matches!(project.connections.get(conn)?.source, SourceRef::Element(_)) {
        return None;
    }
    let branch = add_branch(project, board, &target);
    project.connections.get_mut(conn)?.target = TargetRef::Branch(branch.clone());
    Some(branch)
}

/// Adds an `else if` (with the given script) or an `else` to a branch.
/// An `else` is only added when the branch has none yet.
pub fn add_condition(
    project: &mut Project,
    board: &BoardRef,
    branch: &BranchRef,
    kind: CondKind,
    target: &ElementRef,
) -> Option<CondRef> {
    let b = project.branches.get(branch)?;
    match kind {
        CondKind::If => return None,
        CondKind::Else if b.conditions.else_condition.is_some() => return None,
        _ => {}
    }
    let script = (kind == CondKind::ElseIf).then(|| "true".to_owned());
    let cond = new_condition(project, board, script, target);
    let b = project.branches.get_mut(branch)?;
    match kind {
        CondKind::ElseIf => b.conditions.else_if_conditions.push(cond.clone()),
        CondKind::Else => b.conditions.else_condition = Some(cond.clone()),
        CondKind::If => unreachable!(),
    }
    Some(cond)
}

pub fn set_condition_script(project: &mut Project, cond: &CondRef, script: Option<String>) {
    if let Some(c) = project.conditions.get_mut(cond) {
        c.script = script;
    }
}

/// Points a condition's outgoing connection at another element.
pub fn retarget_condition(project: &mut Project, cond: &CondRef, target: &ElementRef) {
    let Some(output) = project.conditions.get(cond).map(|c| c.output.clone()) else {
        return;
    };
    if let Some(conn) = project.connections.get_mut(&output) {
        conn.target = TargetRef::Element(target.clone());
    }
}

/// Wires an element into a branch (`element -> branch`).
pub fn connect_to_branch(
    project: &mut Project,
    board: &BoardRef,
    from: &ElementRef,
    branch: &BranchRef,
) -> ConnRef {
    let id = ConnRef::from(new_id().as_str());
    project.connections.insert(
        id.clone(),
        Connection {
            ty: "Straight".to_owned(),
            theme: "flow".to_owned(),
            source: SourceRef::Element(from.clone()),
            target: TargetRef::Branch(branch.clone()),
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

/// Removes a condition and its outgoing connection. Removing a branch's `if`
/// removes the whole branch, since a branch can't exist without one.
pub fn delete_condition(project: &mut Project, board: &BoardRef, cond: &CondRef) {
    let Some(branch) = branch_of_condition(project, cond) else {
        return;
    };
    let is_if = project
        .branches
        .get(&branch)
        .is_some_and(|b| &b.conditions.if_condition == cond);
    if is_if {
        delete_branch(project, board, &branch);
        return;
    }
    if let Some(b) = project.branches.get_mut(&branch) {
        if b.conditions.else_condition.as_ref() == Some(cond) {
            b.conditions.else_condition = None;
        }
        b.conditions.else_if_conditions.retain(|c| c != cond);
    }
    drop_condition(project, board, cond);
}

fn drop_condition(project: &mut Project, board: &BoardRef, cond: &CondRef) {
    if let Some(c) = project.conditions.remove(cond) {
        remove_connection_raw(project, board, &c.output);
    }
}

/// Removes a branch, all its conditions (and their connections) and every
/// connection leading into it.
pub fn delete_branch(project: &mut Project, board: &BoardRef, branch: &BranchRef) {
    let conds: Vec<CondRef> = branch_conditions(project, branch)
        .into_iter()
        .map(|c| c.id)
        .collect();
    for cond in &conds {
        drop_condition(project, board, cond);
    }
    let incoming: Vec<ConnRef> = project
        .connections
        .iter()
        .filter(|(_, c)| matches!(&c.target, TargetRef::Branch(b) if b == branch))
        .map(|(id, _)| id.clone())
        .collect();
    for conn in &incoming {
        remove_connection_raw(project, board, conn);
    }
    project.branches.remove(branch);
    if let Some(Board::Node { branches, .. }) = project.boards.get_mut(board) {
        branches.retain(|b| b != branch);
    }
}

/// Structural problems that would make the project fail to load or play.
/// Empty means consistent.
pub fn check_integrity(project: &Project) -> Vec<String> {
    let mut problems = vec![];
    for (id, c) in &project.connections {
        let ok_source = match &c.source {
            SourceRef::Element(e) => project.elements.contains_key(e),
            SourceRef::Condition(cond) => project.conditions.contains_key(cond),
            SourceRef::Jumper(j) => project.jumpers.contains_key(j),
        };
        let ok_target = match &c.target {
            TargetRef::Element(e) => project.elements.contains_key(e),
            TargetRef::Branch(b) => project.branches.contains_key(b),
            TargetRef::Jumper(j) => project.jumpers.contains_key(j),
        };
        if !ok_source {
            problems.push(format!("connection {} has a missing source", id.as_str()));
        }
        if !ok_target {
            problems.push(format!("connection {} has a missing target", id.as_str()));
        }
    }
    for (id, cond) in &project.conditions {
        if !project.connections.contains_key(&cond.output) {
            problems.push(format!(
                "condition {} has no output connection",
                id.as_str()
            ));
        }
        if branch_of_condition(project, id).is_none() {
            problems.push(format!("condition {} belongs to no branch", id.as_str()));
        }
    }
    for (id, b) in &project.branches {
        let c = &b.conditions;
        let refs = std::iter::once(&c.if_condition)
            .chain(&c.else_if_conditions)
            .chain(c.else_condition.as_ref());
        for r in refs {
            if !project.conditions.contains_key(r) {
                problems.push(format!(
                    "branch {} references missing condition",
                    id.as_str()
                ));
            }
        }
    }
    for (id, el) in &project.elements {
        for out in &el.outputs {
            if !project.connections.contains_key(out) {
                problems.push(format!("element {} lists a missing output", id.as_str()));
            }
        }
    }
    for board in project.boards.values() {
        if let Board::Node {
            connections,
            branches,
            elements,
            ..
        } = board
        {
            problems.extend(
                connections
                    .iter()
                    .filter(|c| !project.connections.contains_key(*c))
                    .map(|c| format!("board lists missing connection {}", c.as_str())),
            );
            problems.extend(
                branches
                    .iter()
                    .filter(|b| !project.branches.contains_key(*b))
                    .map(|b| format!("board lists missing branch {}", b.as_str())),
            );
            problems.extend(
                elements
                    .iter()
                    .filter(|e| !project.elements.contains_key(*e))
                    .map(|e| format!("board lists missing element {}", e.as_str())),
            );
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::editor_to_html;
    use crate::editor::model::{
        add_connection, add_element, delete_connection, delete_element, new_project,
    };
    use arcweave_rust::Runtime;

    struct Story {
        project: Project,
        board: BoardRef,
        a: ElementRef,
        b: ElementRef,
        c: ElementRef,
        /// a -> b
        conn: ConnRef,
    }

    fn story() -> Story {
        let (mut project, board, a) = new_project("test");
        let b = add_element(&mut project, &board);
        let c = add_element(&mut project, &board);
        let conn = add_connection(&mut project, &board, &a, &b);
        Story {
            project,
            board,
            a,
            b,
            c,
            conn,
        }
    }

    fn int(v: i32) -> Value {
        Value::Integer(v)
    }

    /// Reads a variable's current value out of the runtime's saved state.
    fn var_in_save(rt: &Runtime, name: &str) -> serde_json::Value {
        fn find(v: &serde_json::Value, name: &str) -> Option<serde_json::Value> {
            match v {
                serde_json::Value::Object(m) => {
                    if m.get("name").and_then(|n| n.as_str()) == Some(name)
                        && let Some(val) = m.get("value")
                    {
                        return Some(val.clone());
                    }
                    m.values().find_map(|x| find(x, name))
                }
                serde_json::Value::Array(a) => a.iter().find_map(|x| find(x, name)),
                _ => None,
            }
        }
        // The last state is the current one.
        let saved: serde_json::Value = serde_json::from_str(&rt.save().unwrap()).unwrap();
        find(&saved, name).unwrap_or(serde_json::Value::Null)
    }

    /// Plays `conn` from the start element and returns which element we landed on.
    fn land(project: &Project, conn: &ConnRef) -> ElementRef {
        let mut rt = Runtime::new(project);
        rt.follow(conn).expect("follow failed");
        let current = rt.get_current_element().unwrap();
        project
            .elements
            .iter()
            .find(|(_, e)| std::ptr::eq(*e, current))
            .map(|(id, _)| id.clone())
            .unwrap()
    }

    #[test]
    fn branch_routes_by_variable_value() {
        let mut s = story();
        let hp = add_variable(&mut s.project, "hp", int(10)).unwrap();
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        let conds = branch_conditions(&s.project, &branch);
        set_condition_script(&mut s.project, &conds[0].id, Some("hp > 5".into()));
        add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &s.c).unwrap();
        assert_eq!(check_integrity(&s.project), Vec::<String>::new());

        assert_eq!(land(&s.project, &s.conn), s.b, "hp=10 takes the if");
        set_variable_value(&mut s.project, &hp, int(1));
        assert_eq!(land(&s.project, &s.conn), s.c, "hp=1 falls through to else");
    }

    #[test]
    fn else_if_chain_picks_the_first_match() {
        let mut s = story();
        let d = add_element(&mut s.project, &s.board);
        let hp = add_variable(&mut s.project, "hp", int(0)).unwrap();
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        let first = branch_conditions(&s.project, &branch)[0].id.clone();
        set_condition_script(&mut s.project, &first, Some("hp > 8".into()));
        let mid = add_condition(&mut s.project, &s.board, &branch, CondKind::ElseIf, &s.c).unwrap();
        set_condition_script(&mut s.project, &mid, Some("hp > 3".into()));
        add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &d).unwrap();

        let kinds: Vec<_> = branch_conditions(&s.project, &branch)
            .iter()
            .map(|c| c.kind)
            .collect();
        assert_eq!(kinds, [CondKind::If, CondKind::ElseIf, CondKind::Else]);

        for (value, expect) in [(9, &s.b), (5, &s.c), (1, &d)] {
            set_variable_value(&mut s.project, &hp, int(value));
            assert_eq!(&land(&s.project, &s.conn), expect, "hp={value}");
        }
        assert!(
            add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &d).is_none(),
            "a second else is refused"
        );
    }

    #[test]
    fn entering_an_element_runs_its_script_lines() {
        let mut s = story();
        add_variable(&mut s.project, "hp", int(10)).unwrap();
        let html = editor_to_html("You get hurt.\n$ hp = hp - 3");
        s.project.elements.get_mut(&s.b).unwrap().content = Some(html.clone());
        assert_eq!(validate_content(&html), None);

        let mut rt = Runtime::new(&s.project);
        rt.follow(&s.conn).unwrap();
        rt.flush();
        assert_eq!(
            var_in_save(&rt, "hp"),
            serde_json::json!({"Integer": 7}),
            "{}",
            rt.save().unwrap()
        );
    }

    #[test]
    fn rename_rewrites_scripts_but_not_prose_or_strings() {
        let mut s = story();
        let hp = add_variable(&mut s.project, "hp", int(10)).unwrap();
        s.project.elements.get_mut(&s.b).unwrap().content = Some(editor_to_html(
            "Your hp is low, hp matters.\n$ hp = hp - 1\n$ note = \"hp\"\n$ if hp < 4\nweak\n$ endif",
        ));
        s.project.connections.get_mut(&s.conn).unwrap().label =
            Some(editor_to_html("$ hpx = hp + 1"));
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        let first = branch_conditions(&s.project, &branch)[0].id.clone();
        set_condition_script(
            &mut s.project,
            &first,
            Some("hp &gt; 5 and hp &lt; 20".into()),
        );

        assert_eq!(variable_usage(&s.project, "hp"), 6);

        let n = rename_variable(&mut s.project, &hp, "health").unwrap();
        assert_eq!(n, 6);
        let content = s.project.elements[&s.b].content.clone().unwrap();
        assert!(
            content.contains("Your hp is low, hp matters."),
            "prose untouched: {content}"
        );
        assert!(content.contains("health = health - 1"));
        assert!(content.contains("\"hp\""), "string literal untouched");
        assert!(
            content.contains("if health &lt; 4"),
            "entities preserved: {content}"
        );
        let label = s.project.connections[&s.conn].label.clone().unwrap();
        assert!(
            label.contains("hpx = health + 1"),
            "longer identifier untouched: {label}"
        );
        assert_eq!(
            s.project.conditions[&first].script.as_deref(),
            Some("health &gt; 5 and health &lt; 20")
        );
        assert_eq!(variable_usage(&s.project, "hp"), 0);

        assert_eq!(land(&s.project, &s.conn), s.b, "renamed story still plays");
    }

    #[test]
    fn values_convert_between_types() {
        let t = |v: Value, k| format!("{:?}", convert_value(&v, k));
        assert_eq!(t(Value::Integer(3), ValueKind::Float), "Float(3.0)");
        assert_eq!(t(Value::Float(2.6), ValueKind::Integer), "Integer(3)");
        assert_eq!(t(Value::Boolean(true), ValueKind::Integer), "Integer(1)");
        assert_eq!(t(Value::Integer(0), ValueKind::Boolean), "Boolean(false)");
        assert_eq!(
            t(Value::String("42".into()), ValueKind::Integer),
            "Integer(42)"
        );
        assert_eq!(
            t(Value::String("nope".into()), ValueKind::Integer),
            "Integer(0)"
        );
        assert_eq!(
            t(Value::String("true".into()), ValueKind::Boolean),
            "Boolean(true)"
        );
        assert_eq!(t(Value::Integer(7), ValueKind::String), "String(\"7\")");
        assert_eq!(ValueKind::of(&Value::Float(1.0)), ValueKind::Float);
    }

    #[test]
    fn variable_names_are_validated() {
        let mut s = story();
        add_variable(&mut s.project, "hp", int(1)).unwrap();
        for bad in ["", "1abc", "has space", "a-b", "if", "true", "visits", "é"] {
            assert!(
                add_variable(&mut s.project, bad, int(0)).is_err(),
                "{bad:?} should be rejected"
            );
        }
        assert!(
            add_variable(&mut s.project, "hp", int(2)).is_err(),
            "duplicate refused"
        );
        let other = add_variable(&mut s.project, "$gold_2", Value::Boolean(false)).unwrap();
        assert!(rename_variable(&mut s.project, &other, "hp").is_err());
        assert_eq!(fresh_variable_name(&s.project), "variable");
        assert_eq!(list_variables(&s.project).len(), 2);
    }

    #[test]
    fn variables_are_registered_under_the_root_and_deleted_cleanly() {
        let mut s = story();
        let id = add_variable(&mut s.project, "x", int(1)).unwrap();
        let roots = |p: &Project| -> Vec<VarRef> {
            p.variables
                .values()
                .find_map(|v| match v {
                    Variable::Root { children, .. } => Some(children.clone()),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(roots(&s.project), vec![id.clone()]);
        delete_variable(&mut s.project, &id);
        assert!(roots(&s.project).is_empty());
        assert!(list_variables(&s.project).is_empty());
    }

    #[test]
    fn deleting_the_if_target_removes_the_whole_branch() {
        let mut s = story();
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &s.c).unwrap();
        delete_element(&mut s.project, &s.board, &s.b);
        assert!(
            s.project.branches.is_empty(),
            "branch gone with its `if` target"
        );
        assert!(s.project.conditions.is_empty());
        assert_eq!(check_integrity(&s.project), Vec::<String>::new());
        assert!(
            s.project.elements.contains_key(&s.c),
            "other elements survive"
        );
        assert!(
            s.project.connections.is_empty(),
            "incoming + both condition connections gone"
        );
    }

    #[test]
    fn deleting_an_else_target_keeps_the_branch() {
        let mut s = story();
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &s.c).unwrap();
        delete_element(&mut s.project, &s.board, &s.c);
        assert_eq!(check_integrity(&s.project), Vec::<String>::new());
        let conds = branch_conditions(&s.project, &branch);
        assert_eq!(conds.len(), 1);
        assert_eq!(conds[0].kind, CondKind::If);
    }

    #[test]
    fn deleting_a_condition_connection_removes_that_condition() {
        let mut s = story();
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        let els = add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &s.c).unwrap();
        let else_conn = s.project.conditions[&els].output.clone();
        delete_connection(&mut s.project, &s.board, &else_conn);
        assert_eq!(branch_conditions(&s.project, &branch).len(), 1);
        assert_eq!(check_integrity(&s.project), Vec::<String>::new());

        // ...and deleting the `if`'s connection removes the branch itself.
        let if_conn = branch_conditions(&s.project, &branch)[0].output.clone();
        delete_connection(&mut s.project, &s.board, &if_conn);
        assert!(s.project.branches.is_empty());
        assert_eq!(check_integrity(&s.project), Vec::<String>::new());
    }

    #[test]
    fn deleting_a_branch_removes_connections_into_it() {
        let mut s = story();
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        delete_branch(&mut s.project, &s.board, &branch);
        assert!(s.project.connections.is_empty());
        assert!(
            s.project.elements[&s.a].outputs.is_empty(),
            "no dangling output on the source"
        );
        assert_eq!(check_integrity(&s.project), Vec::<String>::new());
    }

    #[test]
    fn elements_can_be_wired_into_an_existing_branch_and_conditions_retargeted() {
        let mut s = story();
        let d = add_element(&mut s.project, &s.board);
        let branch = add_branch(&mut s.project, &s.board, &s.b);
        connect_to_branch(&mut s.project, &s.board, &s.a, &branch);
        let if_cond = branch_conditions(&s.project, &branch)[0].id.clone();
        retarget_condition(&mut s.project, &if_cond, &d);
        assert_eq!(check_integrity(&s.project), Vec::<String>::new());

        let into_branch = s.project.elements[&s.a]
            .outputs
            .iter()
            .find(|c| matches!(s.project.connections[*c].target, TargetRef::Branch(_)))
            .cloned()
            .unwrap();
        assert_eq!(land(&s.project, &into_branch), d);
    }

    #[test]
    fn saved_project_reloads_and_still_routes() {
        let mut s = story();
        let hp = add_variable(&mut s.project, "hp", int(10)).unwrap();
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        let first = branch_conditions(&s.project, &branch)[0].id.clone();
        set_condition_script(&mut s.project, &first, Some("hp > 5".into()));
        add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &s.c).unwrap();

        let dir = std::env::temp_dir().join(format!("arcmin-logic-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("project_settings.json");
        crate::persist::save(&path, &s.project, &Default::default()).unwrap();

        let mut reloaded = Project::from_file(path.to_string_lossy().as_ref()).unwrap();
        assert_eq!(check_integrity(&reloaded), Vec::<String>::new());
        assert_eq!(land(&reloaded, &s.conn), s.b);
        set_variable_value(&mut reloaded, &hp, int(0));
        assert_eq!(land(&reloaded, &s.conn), s.c);
    }

    /// What a player actually sees when an element leads into a branch.
    #[test]
    fn play_shows_one_choice_into_a_branch_and_the_variables_pick_the_outcome() {
        use crate::session::Session;
        let mut s = story();
        let d = add_element(&mut s.project, &s.board);
        let hp = add_variable(&mut s.project, "hp", int(10)).unwrap();
        s.project.connections.get_mut(&s.conn).unwrap().label =
            Some(editor_to_html("Push forward"));
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        let first = branch_conditions(&s.project, &branch)[0].id.clone();
        set_condition_script(&mut s.project, &first, Some("hp > 8".into()));
        let mid = add_condition(&mut s.project, &s.board, &branch, CondKind::ElseIf, &s.c).unwrap();
        set_condition_script(&mut s.project, &mid, Some("hp > 3".into()));
        add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &d).unwrap();

        let mut session = Session::start(s.project.clone());
        let choices = session.choices();
        let labels: Vec<_> = choices.iter().map(|c| c.label.as_str()).collect();
        println!("choices shown at the element: {labels:?}");
        assert_eq!(
            labels,
            ["Push forward"],
            "one choice, not one per branch row"
        );

        session.follow(&choices[0].conn).unwrap();
        assert_eq!(
            session.current_element_id().as_deref(),
            Some(s.b.as_str()),
            "hp=10 -> if"
        );

        // Same story, lower hp -> the else-if / else outcomes.
        for (value, expect) in [(5, &s.c), (1, &d)] {
            let mut p = s.project.clone();
            set_variable_value(&mut p, &hp, int(value));
            let mut session = Session::start(p);
            let choice = session.choices().remove(0);
            session.follow(&choice.conn).unwrap();
            assert_eq!(
                session.current_element_id().as_deref(),
                Some(expect.as_str()),
                "hp={value}"
            );
        }
    }

    // ---- what the player sees at a branch (Session) -----------------------------------

    /// start --(label "Go")--> branch { if has_key: "Use the key" -> b,
    ///                                  else if strong: "Break the door" -> c,
    ///                                  else: "Turn back" -> d }
    struct Door {
        project: Project,
        b: ElementRef,
        c: ElementRef,
        d: ElementRef,
        has_key: VarRef,
        strong: VarRef,
    }

    fn door(labels: [Option<&str>; 3]) -> Door {
        let mut s = story();
        let d = add_element(&mut s.project, &s.board);
        let has_key = add_variable(&mut s.project, "has_key", Value::Boolean(false)).unwrap();
        let strong = add_variable(&mut s.project, "strong", Value::Boolean(false)).unwrap();
        s.project.connections.get_mut(&s.conn).unwrap().label = Some(editor_to_html("Go"));
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        let first = branch_conditions(&s.project, &branch)[0].id.clone();
        set_condition_script(&mut s.project, &first, Some("has_key".into()));
        let mid = add_condition(&mut s.project, &s.board, &branch, CondKind::ElseIf, &s.c).unwrap();
        set_condition_script(&mut s.project, &mid, Some("strong".into()));
        add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &d).unwrap();
        retarget_condition(&mut s.project, &first, &s.b);

        for (arm, label) in branch_conditions(&s.project, &branch).iter().zip(labels) {
            s.project.connections.get_mut(&arm.output).unwrap().label = label.map(editor_to_html);
        }
        Door {
            project: s.project,
            b: s.b,
            c: s.c,
            d,
            has_key,
            strong,
        }
    }

    fn shown(door: &Door, has_key: bool, strong: bool) -> Vec<String> {
        use crate::session::Session;
        let mut p = door.project.clone();
        set_variable_value(&mut p, &door.has_key, Value::Boolean(has_key));
        set_variable_value(&mut p, &door.strong, Value::Boolean(strong));
        Session::start(p)
            .choices()
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    #[test]
    fn labelled_arms_become_choices_when_their_condition_holds() {
        let door = door([
            Some("Use the key"),
            Some("Break the door"),
            Some("Turn back"),
        ]);
        assert_eq!(shown(&door, true, false), ["Use the key"]);
        assert_eq!(shown(&door, false, true), ["Break the door"]);
        assert_eq!(
            shown(&door, false, false),
            ["Turn back"],
            "else is the fallback"
        );
        assert_eq!(
            shown(&door, true, true),
            ["Use the key", "Break the door"],
            "guards are independent"
        );
    }

    #[test]
    fn picking_a_labelled_arm_lands_on_its_target() {
        use crate::session::Session;
        let door = door([
            Some("Use the key"),
            Some("Break the door"),
            Some("Turn back"),
        ]);
        let land_on = |has_key, strong, pick: &str| -> ElementRef {
            let mut p = door.project.clone();
            set_variable_value(&mut p, &door.has_key, Value::Boolean(has_key));
            set_variable_value(&mut p, &door.strong, Value::Boolean(strong));
            let mut session = Session::start(p);
            let choice = session
                .choices()
                .into_iter()
                .find(|c| c.label == pick)
                .unwrap();
            session.follow(&choice.conn).unwrap();
            ElementRef::from(session.current_element_id().unwrap().as_str())
        };
        assert_eq!(land_on(true, true, "Use the key"), door.b);
        assert_eq!(land_on(true, true, "Break the door"), door.c);
        assert_eq!(land_on(false, false, "Turn back"), door.d);
    }

    #[test]
    fn unlabelled_arms_keep_the_branch_a_router() {
        // No labels anywhere: one choice ("Go"), the first true condition is followed.
        let door = door([None, None, None]);
        assert_eq!(shown(&door, false, false), ["Go"]);
        assert_eq!(shown(&door, true, true), ["Go"]);
    }

    #[test]
    fn a_router_with_no_possible_outcome_offers_no_dead_choice() {
        use crate::session::Session;
        let mut s = story();
        add_variable(&mut s.project, "has_key", Value::Boolean(false)).unwrap();
        let branch = insert_branch_on_connection(&mut s.project, &s.board, &s.conn).unwrap();
        let first = branch_conditions(&s.project, &branch)[0].id.clone();
        set_condition_script(&mut s.project, &first, Some("has_key".into()));
        // Only an `if`, and it is false: nothing can win, so the choice is hidden...
        assert!(Session::start(s.project.clone()).choices().is_empty());
        // ...and an `else` makes the choice available again.
        add_condition(&mut s.project, &s.board, &branch, CondKind::Else, &s.c).unwrap();
        assert_eq!(Session::start(s.project.clone()).choices().len(), 1);
    }

    #[test]
    fn labelled_arm_scripts_run_and_choices_refresh_after_moving() {
        use crate::session::Session;
        let mut door = door([Some("Use the key"), None, None]);
        add_variable(&mut door.project, "uses", Value::Integer(0)).unwrap();
        // The arm's label carries a script; it runs when the arm is chosen.
        let arm = door
            .project
            .connections
            .iter()
            .find(|(_, c)| {
                matches!(c.target, TargetRef::Element(ref e) if e == &door.b)
                    && matches!(c.source, SourceRef::Condition(_))
            })
            .map(|(id, _)| id.clone())
            .unwrap();
        door.project.connections.get_mut(&arm).unwrap().label =
            Some(editor_to_html("Use the key\n$ uses = uses + 1"));
        set_variable_value(&mut door.project, &door.has_key, Value::Boolean(true));

        let mut session = Session::start(door.project.clone());
        let before: Vec<_> = session.choices().iter().map(|c| c.label.clone()).collect();
        assert_eq!(before, ["Use the key"]);
        let pick = session.choices().remove(0);
        session.follow(&pick.conn).unwrap();
        assert!(
            session.choices().is_empty(),
            "the cache was dropped: the new element has no outputs"
        );
        let saved = session.save().unwrap();
        assert!(saved.contains("\"Integer\":1"), "label script ran: {saved}");
    }

    #[test]
    fn validation_catches_syntax_errors() {
        for ok in [
            "true",
            "hp > 5",
            "hp > 5 and not dead",
            "visits(x) is 0",
            "a == \"s\"",
        ] {
            if ok.contains("visits") {
                continue; // needs an element mention; covered by the crate's own tests
            }
            assert_eq!(validate_condition(ok), None, "{ok}");
        }
        for bad in ["", "   ", "hp >", "hp > > 5", "(hp"] {
            assert!(
                validate_condition(bad).is_some(),
                "{bad:?} should be an error"
            );
        }
        assert_eq!(
            validate_content(&editor_to_html("Hi\n$ hp += 1\n$ if hp > 3\nok\n$ endif")),
            None
        );
        assert!(validate_content(&editor_to_html("$ hp +=")).is_some());
        assert!(
            validate_content(&editor_to_html("$ if hp < 4\nweak")).is_some(),
            "an `if` without `endif` is an error"
        );
    }

    /// Not a real test: writes a small branching story into the library at
    /// `$ARCMIN_DATA_DIR` so the editor can be looked at with realistic data.
    /// Run with `--ignored`.
    #[test]
    #[ignore]
    fn write_demo_project() {
        let Ok(out) = std::env::var("ARCMIN_DATA_DIR") else {
            return;
        };
        let out = std::path::Path::new(&out).join("projects");
        let (mut project, board, start) = new_project("Branch Demo");
        let a = add_element(&mut project, &board);
        let b = add_element(&mut project, &board);
        let c = add_element(&mut project, &board);
        let d = add_element(&mut project, &board);
        let set = |p: &mut Project, e: &ElementRef, title: &str, text: &str| {
            let el = p.elements.get_mut(e).unwrap();
            el.title = Some(format!("<p>{title}</p>"));
            el.content = Some(editor_to_html(text));
        };
        set(
            &mut project,
            &start,
            "Gate",
            "A guard blocks the gate. You have a key, but the guard looks tired.",
        );
        set(
            &mut project,
            &a,
            "Fight",
            "You fight and get hurt.\n$ hp = hp - 6",
        );
        set(&mut project, &b, "Sneak in", "You slip past with the key.");
        set(&mut project, &c, "Turn back", "You give up and go home.");
        set(&mut project, &d, "Ending", "The story ends.");
        add_variable(&mut project, "hp", Value::Integer(10)).unwrap();
        add_variable(&mut project, "has_key", Value::Boolean(true)).unwrap();
        add_variable(&mut project, "name", Value::String("Hero".into())).unwrap();

        let go = add_connection(&mut project, &board, &start, &a);
        project.connections.get_mut(&go).unwrap().label = Some(editor_to_html("Push forward"));
        let branch = insert_branch_on_connection(&mut project, &board, &go).unwrap();
        let conds = branch_conditions(&project, &branch);
        set_condition_script(&mut project, &conds[0].id, Some("has_key".into()));
        retarget_condition(&mut project, &conds[0].id, &b);
        let mid = add_condition(&mut project, &board, &branch, CondKind::ElseIf, &a).unwrap();
        set_condition_script(&mut project, &mid, Some("hp > 3".into()));
        let els = add_condition(&mut project, &board, &branch, CondKind::Else, &c).unwrap();
        for (cond, label) in [
            (&conds[0].id, "Use the key"),
            (&mid, "Fight your way in"),
            (&els, "Turn back"),
        ] {
            let out = project.conditions[cond].output.clone();
            project.connections.get_mut(&out).unwrap().label = Some(editor_to_html(label));
        }
        add_connection(&mut project, &board, &a, &d);
        add_connection(&mut project, &board, &b, &d);

        let dir = out.join("Branch Demo");
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        crate::persist::save(
            &dir.join("project_settings.json"),
            &project,
            &Default::default(),
        )
        .unwrap();
        std::fs::write(
            out.parent().unwrap().join("state.json"),
            r#"{"last":"Branch Demo"}"#,
        )
        .unwrap();
    }
}
