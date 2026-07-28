//! Story interpreter — advances a cursor through `Story` entries, producing
//! typed `Event`s for the caller to render.

use std::collections::BTreeMap;

use crate::story::{
    Condition, Entry, Scene, SetOp, Story, StoryError, VarValue,
};

/// Events emitted by the interpreter. The presentation layer (GDScript or
/// another Rust module) turns these into pixels and sounds.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A line of dialogue to show. `text_key` and `speaker_key` are
    /// localization keys (if present).
    Dialogue {
        speaker: Option<String>,
        text: String,
        character: String,
        position: String,
        background: String,
        emotion: String,
        presentation: String,
        text_key: String,
        speaker_key: String,
        append: bool,
    },
    /// A choice block to show. `entry_index` is the index into the scene.
    Choice {
        entry_index: usize,
        choices: Vec<ChoiceData>,
    },
    /// Story reached the end of the current scene.
    SceneEnded,
    /// Story requested quit.
    Quit,
    /// Goto fired (after applying a goto or a branch).
    Goto(String),
    /// Generic command from the story (e.g. `{"type": "play_music", "track": "intro"}`).
    Command(serde_json::Value),
    /// Error during execution.
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceData {
    pub text: String,
    pub goto: String,
    pub text_key: String,
    pub condition: Option<Condition>,
    pub set: Option<SetOp>,
}

#[derive(Debug, Clone, Default)]
pub struct InterpreterState {
    pub current_scene: String,
    pub current_entry_index: i32,
    pub variables: BTreeMap<String, VarValue>,
    pub pending_choice_entry: Option<usize>,
    pub pending_choice_indices: Vec<usize>,
    pub story_version: String,
}

#[derive(Debug)]
pub struct Interpreter {
    story: Story,
    state: InterpreterState,
}

impl Interpreter {
    pub fn new(story: Story, start_scene: &str) -> Result<Self, StoryError> {
        if !story.scenes.contains_key(start_scene) {
            return Err(StoryError::SceneNotFound(start_scene.to_string()));
        }
        let state = InterpreterState {
            current_scene: start_scene.to_string(),
            current_entry_index: 0,
            variables: story.variables.clone(),
            story_version: story.version.clone(),
            ..Default::default()
        };
        Ok(Self { story, state })
    }

    pub fn story(&self) -> &Story {
        &self.story
    }

    pub fn state(&self) -> &InterpreterState {
        &self.state
    }

    /// Mutable access to the interpreter state. Useful for set operations
    /// that need to mutate variables directly.
    pub fn state_mut(&mut self) -> &mut InterpreterState {
        &mut self.state
    }

    /// Advance one step. Returns the event the caller should act on.
    pub fn advance(&mut self) -> Event {
        self.advance_with_limit(10_000)
    }

    fn advance_with_limit(&mut self, mut limit: usize) -> Event {
        loop {
            if limit == 0 {
                return Event::Error("automatic story command limit reached".to_string());
            }
            limit -= 1;

            let scene = match self.story.scene(&self.state.current_scene) {
                Some(s) => s,
                None => {
                    return Event::Error(format!(
                        "scene '{}' not found",
                        self.state.current_scene
                    ));
                }
            };

            let idx = match usize::try_from(self.state.current_entry_index) {
                Ok(i) => i,
                Err(_) => return Event::Error("entry index overflow".to_string()),
            };

            if idx >= scene.entries.len() {
                return Event::SceneEnded;
            }

            let entry = scene.entries[idx].clone();
            self.state.current_entry_index += 1;
            self.state.pending_choice_entry = None;
            self.state.pending_choice_indices.clear();

            match entry {
                Entry::Dialogue(d) => return Event::Dialogue {
                    speaker: d.speaker,
                    text: d.text,
                    character: d.character,
                    position: d.position,
                    background: d.background,
                    emotion: d.emotion,
                    presentation: d.presentation,
                    text_key: d.text_key,
                    speaker_key: d.speaker_key,
                    append: d.append,
                },
                Entry::Choice { choices } => {
                    let mut visible: Vec<(usize, &crate::story::ChoiceEntry)> = Vec::new();
                    for (i, c) in choices.iter().enumerate() {
                        if !c.if_flag.is_empty()
                            && !self
                                .state
                                .variables
                                .get(&c.if_flag)
                                .and_then(value_as_bool)
                                .unwrap_or(false)
                        {
                            continue;
                        }
                        if let Some(cond) = &c.condition {
                            if !evaluate_condition(cond, &self.state.variables) {
                                continue;
                            }
                        }
                        visible.push((i, c));
                    }
                    if visible.is_empty() {
                        continue;
                    }
                    let entry_index = idx;
                    self.state.pending_choice_entry = Some(entry_index);
                    self.state.pending_choice_indices =
                        visible.iter().map(|(i, _)| *i).collect();
                    let data: Vec<ChoiceData> = visible
                        .into_iter()
                        .map(|(_, c)| ChoiceData {
                            text: c.text.clone(),
                            goto: c.goto.clone(),
                            text_key: c.text_key.clone(),
                            condition: c.condition.clone(),
                            set: c.set.clone(),
                        })
                        .collect();
                    return Event::Choice {
                        entry_index,
                        choices: data,
                    };
                }
                Entry::Quit => return Event::Quit,
                Entry::Goto { target } => {
                    if let Err(e) = self.jump_to(&target) {
                        return Event::Error(format!("{:?}", e));
                    }
                    return Event::Goto(target);
                }
                Entry::Text { data } => {
                    return Event::Dialogue {
                        speaker: None,
                        text: data,
                        character: String::new(),
                        position: String::new(),
                        background: String::new(),
                        emotion: String::new(),
                        presentation: "narration".to_string(),
                        text_key: String::new(),
                        speaker_key: String::new(),
                        append: false,
                    };
                }
                Entry::Set(op) => {
                    if let Err(e) = apply_set(&op, &mut self.state.variables) {
                        return Event::Error(format!("{:?}", e));
                    }
                }
                Entry::Branch {
                    condition,
                    then,
                    r#else,
                } => {
                    let target = if evaluate_condition(&condition, &self.state.variables) {
                        then
                    } else {
                        r#else
                    };
                    if target.is_empty() {
                        continue;
                    }
                    if let Err(e) = self.jump_to(&target) {
                        return Event::Error(format!("{:?}", e));
                    }
                    return Event::Goto(target);
                }
                Entry::Command(value) => return Event::Command(value),
            }
        }
    }

    /// Apply a choice (by visible index). The caller passes the index the
    /// player picked from the visible list.
    pub fn pick_choice(&mut self, visible_index: usize) -> Result<(), StoryError> {
        let entry_idx = self
            .state
            .pending_choice_entry
            .ok_or_else(|| StoryError::UnknownOp("no pending choice".to_string()))?;
        let original_index = *self
            .state
            .pending_choice_indices
            .get(visible_index)
            .ok_or_else(|| StoryError::UnknownOp("choice index out of range".to_string()))?;
        let scene = self.story.scene(&self.state.current_scene).unwrap();
        let entry = scene.entries.get(entry_idx).unwrap();
        let Entry::Choice { choices } = entry else {
            return Err(StoryError::UnknownOp("entry is not a choice".to_string()));
        };
        let choice = choices.get(original_index).unwrap().clone();
        if !choice.goto.is_empty() {
            self.jump_to(&choice.goto)?;
        } else {
            self.state.current_entry_index = (entry_idx + 1) as i32;
            self.state.pending_choice_entry = None;
            self.state.pending_choice_indices.clear();
        }
        if let Some(set) = &choice.set {
            apply_set(set, &mut self.state.variables)?;
        }
        if !choice.set_flag.is_empty() {
            let parts: Vec<&str> = choice.set_flag.splitn(2, '=').collect();
            if parts.len() == 2 {
                let name = parts[0].trim().to_string();
                let raw = parts[1].trim();
                let value = match raw {
                    "true" | "1" => VarValue::Bool(true),
                    "false" | "0" => VarValue::Bool(false),
                    _ => return Err(StoryError::InvalidVarValue),
                };
                self.state.variables.insert(name, value);
            }
        }
        Ok(())
    }

    /// Jump to a target. The target can be a scene name, a scene name with
    /// a label (`scene:label`), or a label in the current scene (`.label`).
    pub fn jump_to(&mut self, target: &str) -> Result<(), StoryError> {
        let target = target.trim();
        if target.is_empty() {
            return Err(StoryError::UnknownOp("empty jump target".to_string()));
        }
        let (scene_name, label) = if let Some(l) = target.strip_prefix('.') {
            (self.state.current_scene.clone(), Some(l))
        } else if let Some((s, l)) = target.split_once(':') {
            (s.to_string(), Some(l))
        } else {
            (target.to_string(), None)
        };
        if scene_name.is_empty() {
            return Err(StoryError::UnknownOp("empty scene".to_string()));
        }
        let scene = self
            .story
            .scene(&scene_name)
            .ok_or_else(|| StoryError::SceneNotFound(scene_name.clone()))?;
        let entry_index = if let Some(label) = label {
            find_label(scene, label)?
        } else {
            0
        };
        self.state.current_scene = scene_name;
        self.state.current_entry_index = entry_index as i32;
        self.state.pending_choice_entry = None;
        self.state.pending_choice_indices.clear();
        Ok(())
    }

    /// Export the current state as JSON for save files.
    pub fn export_state(&self) -> String {
        let payload = serde_json::json!({
            "story_version": self.state.story_version,
            "current_scene": self.state.current_scene,
            "current_entry_index": self.state.current_entry_index,
            "variables": self.state.variables,
        });
        serde_json::to_string(&payload).unwrap_or_default()
    }

    /// Import state from JSON. Replaces the current scene cursor and
    /// variables. Returns true on success.
    pub fn import_state(&mut self, json: &str) -> bool {
        let value: serde_json::Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let Some(obj) = value.as_object() else {
            return false;
        };
        if let Some(s) = obj.get("current_scene").and_then(|v| v.as_str()) {
            self.state.current_scene = s.to_string();
        }
        if let Some(idx) = obj.get("current_entry_index").and_then(|v| v.as_i64()) {
            if let Ok(i) = i32::try_from(idx) {
                self.state.current_entry_index = i;
            }
        }
        if let Some(vars) = obj.get("variables").and_then(|v| v.as_object()) {
            self.state.variables.clear();
            for (k, v) in vars {
                let value = match v {
                    serde_json::Value::Bool(b) => VarValue::Bool(*b),
                    serde_json::Value::Number(n) => {
                        VarValue::Number(n.as_f64().unwrap_or(0.0))
                    }
                    serde_json::Value::String(s) => VarValue::String(s.clone()),
                    _ => continue,
                };
                self.state.variables.insert(k.clone(), value);
            }
        }
        self.state.pending_choice_entry = None;
        self.state.pending_choice_indices.clear();
        true
    }
}

fn find_label(scene: &Scene, label: &str) -> Result<usize, StoryError> {
    for (i, entry) in scene.entries.iter().enumerate() {
        if let Entry::Dialogue(d) = entry {
            if d.label == label {
                return Ok(i);
            }
        }
    }
    Err(StoryError::LabelNotFound(
        label.to_string(),
        "<scene>".to_string(),
    ))
}

fn value_as_bool(v: &VarValue) -> Option<bool> {
    match v {
        VarValue::Bool(b) => Some(*b),
        _ => None,
    }
}

fn value_as_f64(v: &VarValue) -> Option<f64> {
    match v {
        VarValue::Number(n) => Some(*n),
        _ => None,
    }
}

fn evaluate_condition(c: &Condition, vars: &BTreeMap<String, VarValue>) -> bool {
    let current = vars.get(&c.variable);
    match c.operator.as_str() {
        "truthy" => value_is_truthy(current),
        "falsy" => !value_is_truthy(current),
        "==" => current == Some(&c.value),
        "!=" => current != Some(&c.value),
        ">" | ">=" | "<" | "<=" => {
            let (Some(l), Some(r)) = (current.and_then(value_as_f64), value_as_f64(&c.value))
            else {
                return false;
            };
            match c.operator.as_str() {
                ">" => l > r,
                ">=" => l >= r,
                "<" => l < r,
                "<=" => l <= r,
                _ => false,
            }
        }
        _ => false,
    }
}

fn value_is_truthy(v: Option<&VarValue>) -> bool {
    match v {
        Some(VarValue::Bool(b)) => *b,
        Some(VarValue::Number(n)) => *n != 0.0,
        Some(VarValue::String(s)) => !s.is_empty(),
        None => false,
    }
}

fn apply_set(
    op: &SetOp,
    vars: &mut BTreeMap<String, VarValue>,
) -> Result<(), StoryError> {
    if !is_valid_var_name(&op.variable) {
        return Err(StoryError::InvalidVarName(op.variable.clone()));
    }
    if !is_supported_var_value(&op.value) {
        return Err(StoryError::InvalidVarValue);
    }
    let next = match op.operation.as_str() {
        "set" => op.value.clone(),
        "toggle" => {
            let current = vars
                .get(&op.variable)
                .and_then(value_as_bool)
                .unwrap_or(false);
            VarValue::Bool(!current)
        }
        "add" => match (vars.get(&op.variable), &op.value) {
            (Some(VarValue::String(l)), VarValue::String(r)) => {
                VarValue::String(format!("{}{}", l, r))
            }
            (Some(VarValue::Number(l)), VarValue::Number(r)) => VarValue::Number(l + r),
            (None, VarValue::String(s)) => VarValue::String(s.clone()),
            (None, VarValue::Number(n)) => VarValue::Number(*n),
            _ => return Err(StoryError::InvalidVarValue),
        },
        "subtract" => {
            let (l, r) = numbers(op, vars)?;
            VarValue::Number(l - r)
        }
        "multiply" => {
            let (l, r) = numbers(op, vars)?;
            VarValue::Number(l * r)
        }
        "divide" => {
            let (l, r) = numbers(op, vars)?;
            if r == 0.0 {
                return Err(StoryError::DivideByZero);
            }
            VarValue::Number(l / r)
        }
        _ => return Err(StoryError::UnknownOp(op.operation.clone())),
    };
    vars.insert(op.variable.clone(), next);
    Ok(())
}

fn numbers(
    op: &SetOp,
    vars: &BTreeMap<String, VarValue>,
) -> Result<(f64, f64), StoryError> {
    let l = vars
        .get(&op.variable)
        .and_then(value_as_f64)
        .ok_or_else(|| StoryError::InvalidVarValue)?;
    let r = value_as_f64(&op.value).ok_or(StoryError::InvalidVarValue)?;
    Ok((l, r))
}

fn is_valid_var_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 96 {
        return false;
    }
    s.split('.').all(|part| {
        let mut chars = part.chars();
        matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn is_supported_var_value(v: &VarValue) -> bool {
    matches!(v, VarValue::Bool(_) | VarValue::Number(_) | VarValue::String(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn story() -> Story {
        let json = r#"{
            "version": "0.1.0",
            "scenes": {
                "start": {
                    "background": "cafe",
                    "entries": [
                        { "type": "dialogue", "text": "Hello" },
                        { "type": "dialogue", "label": "middle", "text": "Middle" },
                        { "type": "choice", "choices": [
                            { "text": "Yes", "goto": "yes" },
                            { "text": "No",  "goto": "no" }
                        ]}
                    ]
                },
                "yes": { "entries": [ { "type": "dialogue", "text": "Yes path" } ] },
                "no":  { "entries": [ { "type": "dialogue", "text": "No path"  } ] }
            },
            "variables": { "score": 0 }
        }"#;
        Story::from_json(json).unwrap()
    }

    #[test]
    fn advance_dialogue() {
        let mut interp = Interpreter::new(story(), "start").unwrap();
        let ev = interp.advance();
        assert!(matches!(ev, Event::Dialogue { ref text, .. } if text == "Hello"));
    }

    #[test]
    fn jump_to_label() {
        let mut interp = Interpreter::new(story(), "start").unwrap();
        interp.jump_to(".middle").unwrap();
        let ev = interp.advance();
        assert!(matches!(ev, Event::Dialogue { ref text, .. } if text == "Middle"));
    }

    #[test]
    fn pick_choice_routes() {
        let mut interp = Interpreter::new(story(), "start").unwrap();
        // skip first dialogue
        let _ = interp.advance();
        // jump to choice by manually moving entry index
        interp.state.current_entry_index = 2;
        let ev = interp.advance();
        let Event::Choice { choices, .. } = ev else {
            panic!("expected choice");
        };
        assert_eq!(choices.len(), 2);
        interp.pick_choice(0).unwrap();
        let ev = interp.advance();
        assert!(matches!(ev, Event::Dialogue { ref text, .. } if text == "Yes path"));
    }
}
