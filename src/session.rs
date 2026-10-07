use std::collections::HashMap;

use arcweave_rust::project::{
    AssetSource, Component, ConnRef, ElementRef, Project, Resolve, TargetRef, VarRef,
};
use arcweave_rust::script::Environment;
use arcweave_rust::{Runtime, RuntimeState, RuntimeVariable};

use crate::content;
use crate::editor::logic::{self, CondKind};

#[ouroboros::self_referencing]
pub struct Session {
    project: Project,
    #[borrows(project)]
    #[covariant]
    runtime: Runtime<'this>,
    /// The choices for the current element; cleared whenever the story moves.
    choices_cache: Option<Vec<Choice>>,
}

/// A single clickable choice offered from the current element.
#[derive(Clone)]
pub struct Choice {
    /// The connection to follow when the player picks this choice.
    pub conn: ConnRef,
    pub label: String,
}

impl Session {
    pub fn start(project: Project) -> Self {
        SessionBuilder {
            project,
            runtime_builder: |project| Runtime::new(project),
            choices_cache: None,
        }
        .build()
    }

    pub fn start_from_save(project: Project, saved: &str) -> anyhow::Result<Self> {
        let mut session = Self::start(project);
        session.with_runtime_mut(|runtime| runtime.load(saved))?;
        session.with_choices_cache_mut(|cache| *cache = None);
        Ok(session)
    }

    pub fn save(&self) -> anyhow::Result<String> {
        Ok(self.with_runtime(|runtime| runtime.save())?)
    }

    pub fn title(&self) -> String {
        self.with_runtime(|runtime| {
            runtime
                .get_current_element()
                .ok()
                .and_then(|el| el.title.as_deref())
                .map(content::strip_html)
                .unwrap_or_default()
        })
    }

    /// Id of the element the player is currently on.
    pub fn current_element_id(&self) -> Option<String> {
        let project = self.borrow_project();
        self.with_runtime(|runtime| {
            let current = runtime.get_current_element().ok()?;
            project
                .elements
                .iter()
                .find(|(_, el)| std::ptr::eq(*el, current))
                .map(|(id, _)| id.as_str().to_owned())
        })
    }

    pub fn body_text(&self) -> String {
        self.with_runtime(|runtime| match runtime.render_current_content() {
            Ok(Some(c)) => content::flatten(&c),
            _ => String::new(),
        })
    }

    /// What the player can pick right now, in the order the author connected them.
    ///
    /// * A connection to an element is a choice.
    /// * A connection into a branch whose arms have no labels is a router: it is a
    ///   single choice (hidden when no condition is true) and the first true
    ///   condition is followed automatically.
    /// * A branch with at least one labelled arm offers each labelled arm whose
    ///   condition is true as its own choice (`if`/`else if` independently, `else`
    ///   when none of the conditions is true).
    pub fn choices(&mut self) -> Vec<Choice> {
        if let Some(cached) = self.borrow_choices_cache() {
            return cached.clone();
        }
        let computed = self.compute_choices();
        self.with_choices_cache_mut(|cache| *cache = Some(computed.clone()));
        computed
    }

    fn compute_choices(&mut self) -> Vec<Choice> {
        let rendered = self.with_runtime_mut(|runtime| runtime.render_current_options());
        let Ok(rendered) = rendered else {
            return Vec::new();
        };
        let project = self.borrow_project();
        let saved = self.with_runtime(|runtime| runtime.save().ok());
        let state = saved.as_deref().and_then(|s| state_from_save(project, s));
        let outputs = self.with_runtime(|runtime| {
            runtime
                .get_current_element()
                .map(|el| el.outputs.clone())
                .unwrap_or_default()
        });

        let rendered_label = |conn: &ConnRef, content: Option<&arcweave_rust::Content>| -> String {
            content
                .map(content::flatten)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    conn.resolve(project)
                        .ok()
                        .and_then(|c| c.label.clone())
                        .map(|s| content::strip_html(&s))
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "Continue".to_owned())
                })
        };
        let truth = |script: &str| -> bool {
            state
                .as_ref()
                .is_some_and(|st| Environment::new(st).eval_branch(script).unwrap_or(false))
        };
        // A label may contain scripts and mentions, so render it against the live state.
        let arm_label = |conn: &ConnRef| -> Option<String> {
            let html = conn.resolve(project).ok()?.label.clone()?;
            let rendered = state
                .as_ref()
                .and_then(|st| Environment::new(st).build_content(&html).ok().flatten())
                .map(|c| content::flatten(&c))
                .unwrap_or_else(|| content::strip_html(&html));
            (!rendered.trim().is_empty()).then_some(rendered)
        };

        let mut choices = Vec::new();
        for conn in outputs {
            let Some(connection) = project.connections.get(&conn) else {
                continue;
            };
            let TargetRef::Branch(branch) = &connection.target else {
                choices.push(Choice {
                    label: rendered_label(&conn, rendered.get(&conn).and_then(Option::as_ref)),
                    conn,
                });
                continue;
            };

            let arms = logic::branch_conditions(project, branch);
            let holds: Vec<bool> = arms
                .iter()
                .map(|arm| arm.script.as_deref().is_some_and(&truth))
                .collect();
            let labels: Vec<Option<String>> = arms.iter().map(|arm| arm_label(&arm.output)).collect();

            if labels.iter().all(Option::is_none) {
                // Router: one choice, offered only if some condition (or the else) can win.
                let winnable = holds.iter().any(|&h| h) || arms.iter().any(|a| a.kind == CondKind::Else);
                if winnable {
                    choices.push(Choice {
                        label: rendered_label(&conn, rendered.get(&conn).and_then(Option::as_ref)),
                        conn,
                    });
                }
                continue;
            }

            let none_hold = !holds.iter().any(|&h| h);
            for ((arm, label), hold) in arms.iter().zip(labels).zip(holds) {
                let Some(label) = label else { continue };
                let available = match arm.kind {
                    CondKind::Else => none_hold,
                    _ => hold,
                };
                if available {
                    choices.push(Choice { conn: arm.output.clone(), label });
                }
            }
        }
        choices
    }

    pub fn follow(&mut self, conn: &ConnRef) -> anyhow::Result<()> {
        self.with_choices_cache_mut(|cache| *cache = None);
        self.with_runtime_mut(|runtime| -> anyhow::Result<()> {
            runtime.follow(conn)?;
            runtime.flush();
            Ok(())
        })
    }

    /// Filenames (resolved later through the AssetIndex) of any character/component
    /// cover images attached to the current element, in element order.
    pub fn current_covers(&self) -> Vec<String> {
        let project = self.borrow_project();
        self.with_runtime(|runtime| {
            let Ok(element) = runtime.get_current_element() else {
                return Vec::new();
            };
            element
                .components
                .iter()
                .filter_map(|comp_ref| comp_ref.resolve(project).ok())
                .filter_map(|comp| match comp {
                    Component::Node { assets, .. } => assets.as_ref(),
                    Component::Root { .. } => None,
                })
                .filter_map(|assets| assets.cover.as_ref())
                .filter_map(|source| match source {
                    AssetSource::ById { id } => id.resolve(project).ok().and_then(|asset| {
                        match asset {
                            arcweave_rust::project::Asset::Node { name, .. } => {
                                Some(name.clone())
                            }
                            arcweave_rust::project::Asset::Root { .. } => None,
                        }
                    }),
                    AssetSource::ByFile { .. } => None,
                })
                .collect()
        })
    }
}

/// Rebuilds the runtime's current [`RuntimeState`] from its saved form, because the
/// runtime doesn't hand its state out. The last saved state is the current one.
fn state_from_save<'a>(project: &'a Project, saved: &str) -> Option<RuntimeState<'a>> {
    let states: Vec<serde_json::Value> = serde_json::from_str(saved).ok()?;
    let last = states.last()?;
    let variables: HashMap<VarRef, RuntimeVariable> =
        serde_json::from_value(last.get("variables")?.clone()).ok()?;
    let element = |id: &str| -> Option<&'a ElementRef> {
        project
            .elements
            .get_key_value(&ElementRef::from(id))
            .map(|(key, _)| key)
    };
    let visits = last
        .get("visits")?
        .as_object()?
        .iter()
        .filter_map(|(id, n)| Some((element(id)?, i32::try_from(n.as_i64()?).ok()?)))
        .collect();
    Some(RuntimeState {
        variables,
        visits,
        current_element: element(last.get("current_element")?.as_str()?)?,
    })
}
