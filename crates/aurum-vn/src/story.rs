//! Story data structures.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoryError {
    #[error("JSON parse error: {0}")]
    Json(String),
    #[error("missing 'scenes' object")]
    NoScenes,
    #[error("scene '{0}' not found")]
    SceneNotFound(String),
    #[error("label '{0}' not found in scene '{1}'")]
    LabelNotFound(String, String),
    #[error("invalid variable name '{0}'")]
    InvalidVarName(String),
    #[error("variable value must be bool, number, or string")]
    InvalidVarValue,
    #[error("variable limit reached")]
    VarLimitReached,
    #[error("unknown operation '{0}'")]
    UnknownOp(String),
    #[error("cannot divide by zero")]
    DivideByZero,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum VarValue {
    Bool(bool),
    Number(f64),
    String(String),
}

impl Default for VarValue {
    fn default() -> Self {
        VarValue::Bool(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Condition {
    pub variable: String,
    #[serde(default = "default_op")]
    pub operator: String,
    #[serde(default)]
    pub value: VarValue,
}

fn default_op() -> String {
    "==".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SetOp {
    pub variable: String,
    #[serde(default = "default_set_op")]
    pub operation: String,
    #[serde(default)]
    pub value: VarValue,
}

fn default_set_op() -> String {
    "set".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DialogueEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub emotion: String,
    #[serde(default)]
    pub character: String,
    #[serde(default)]
    pub position: String,
    #[serde(default)]
    pub background: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub presentation: String,
    #[serde(default)]
    pub text_key: String,
    #[serde(default)]
    pub speaker_key: String,
    #[serde(default)]
    pub append: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChoiceEntry {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub goto: String,
    #[serde(default)]
    pub set_flag: String,
    #[serde(default)]
    pub if_flag: String,
    #[serde(default)]
    pub text_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<SetOp>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChoiceBlock {
    pub choices: Vec<ChoiceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    Dialogue(DialogueEntry),
    Choice { choices: Vec<ChoiceEntry> },
    Quit,
    Goto { target: String },
    Text { data: String },
    Set(SetOp),
    Branch {
        condition: Condition,
        #[serde(default)]
        then: String,
        #[serde(default)]
        r#else: String,
    },
    Command(serde_json::Value),
}

impl Default for Entry {
    fn default() -> Self {
        Entry::Dialogue(DialogueEntry::default())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default)]
    pub background: String,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Story {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub scenes: std::collections::BTreeMap<String, Scene>,
    #[serde(default)]
    pub variables: std::collections::BTreeMap<String, VarValue>,
}

impl Story {
    pub fn from_json(json: &str) -> Result<Self, StoryError> {
        serde_json::from_str(json).map_err(|e| StoryError::Json(e.to_string()))
    }

    pub fn scene(&self, name: &str) -> Option<&Scene> {
        self.scenes.get(name)
    }
}

/// Helper: alias for `Choice` block used in `Entry::Choice`.
///
/// We keep `Choice` as a public re-export of the choice block for ergonomics
/// when reading.
pub type Choice = ChoiceBlock;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_story() {
        let json = r#"{
            "version": "0.1.0",
            "scenes": {
                "start": {
                    "background": "black",
                    "entries": [
                        { "type": "dialogue", "text": "Hello, world." }
                    ]
                }
            },
            "variables": { "score": 0 }
        }"#;
        let story = Story::from_json(json).unwrap();
        assert_eq!(story.version, "0.1.0");
        assert!(story.scene("start").is_some());
        assert_eq!(story.scenes.len(), 1);
    }

    #[test]
    fn parse_choice_block() {
        let json = r#"{
            "scenes": {
                "hub": {
                    "entries": [
                        { "type": "choice", "choices": [
                            { "text": "Go left", "goto": "left" },
                            { "text": "Go right", "goto": "right" }
                        ]}
                    ]
                }
            }
        }"#;
        let story = Story::from_json(json).unwrap();
        let scene = story.scene("hub").unwrap();
        if let Entry::Choice { choices } = &scene.entries[0] {
            assert_eq!(choices.len(), 2);
        } else {
            panic!("expected choice");
        }
    }
}
