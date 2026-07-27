//! Typed global state with save/load.
//!
//! `State` is a small key/value store keyed by `&'static str`. Values are one
//! of `bool`, `i64`, `f64`, or `String`. Anything more structured belongs in
//! a component (or a resource) — `State` is for the *few* values that need
//! to be globally accessible and survive save/load.
//!
//! Why string keys? They match the GDScript surface (`Mavis.state.get("score")`)
//! and are easy to log. If a project outgrows this, replace with a typed
//! handle and the GDScript shim can keep the same call site.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("state key not found: {0}")]
    NotFound(String),
    #[error("state value at key '{0}' is not a {1}")]
    TypeMismatch(String, &'static str),
    #[error("serialization error: {0}")]
    Serde(String),
}

impl From<serde_json::Error> for StateError {
    fn from(e: serde_json::Error) -> Self {
        StateError::Serde(e.to_string())
    }
}

/// The supported value types for `State`. Keeps the surface tiny and the
/// GDScript bridge trivial.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StateValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    /// JSON-encoded opaque blob. Use sparingly — better to put complex data
    /// in a resource.
    Json(String),
}

impl StateValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            StateValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_int(&self) -> Option<i64> {
        match self {
            StateValue::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub fn as_float(&self) -> Option<f64> {
        match self {
            StateValue::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            StateValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl From<bool> for StateValue {
    fn from(v: bool) -> Self {
        StateValue::Bool(v)
    }
}
impl From<i32> for StateValue {
    fn from(v: i32) -> Self {
        StateValue::Int(v as i64)
    }
}
impl From<i64> for StateValue {
    fn from(v: i64) -> Self {
        StateValue::Int(v)
    }
}
impl From<f32> for StateValue {
    fn from(v: f32) -> Self {
        StateValue::Float(v as f64)
    }
}
impl From<f64> for StateValue {
    fn from(v: f64) -> Self {
        StateValue::Float(v)
    }
}
impl From<String> for StateValue {
    fn from(v: String) -> Self {
        StateValue::String(v)
    }
}
impl From<&str> for StateValue {
    fn from(v: &str) -> Self {
        StateValue::String(v.to_string())
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct State {
    values: std::collections::HashMap<String, StateValue>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set<V: Into<StateValue>>(&mut self, key: &'static str, value: V) {
        self.values.insert(key.to_string(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&StateValue> {
        self.values.get(key)
    }

    pub fn get_bool(&self, key: &str) -> Result<bool, StateError> {
        self.values
            .get(key)
            .ok_or_else(|| StateError::NotFound(key.to_string()))?
            .as_bool()
            .ok_or_else(|| StateError::TypeMismatch(key.to_string(), "bool"))
    }

    pub fn get_int(&self, key: &str) -> Result<i64, StateError> {
        self.values
            .get(key)
            .ok_or_else(|| StateError::NotFound(key.to_string()))?
            .as_int()
            .ok_or_else(|| StateError::TypeMismatch(key.to_string(), "int"))
    }

    pub fn get_float(&self, key: &str) -> Result<f64, StateError> {
        self.values
            .get(key)
            .ok_or_else(|| StateError::NotFound(key.to_string()))?
            .as_float()
            .ok_or_else(|| StateError::TypeMismatch(key.to_string(), "float"))
    }

    pub fn get_string(&self, key: &str) -> Result<&str, StateError> {
        self.values
            .get(key)
            .ok_or_else(|| StateError::NotFound(key.to_string()))?
            .as_str()
            .ok_or_else(|| StateError::TypeMismatch(key.to_string(), "string"))
    }

    pub fn remove(&mut self, key: &str) -> Option<StateValue> {
        self.values.remove(key)
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(|s| s.as_str())
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Serialize to JSON. Returns the raw string — caller writes to disk.
    pub fn to_json(&self) -> Result<String, StateError> {
        Ok(serde_json::to_string(self)?)
    }

    /// Deserialize from JSON. Replaces all current state.
    pub fn from_json(json: &str) -> Result<Self, StateError> {
        Ok(serde_json::from_str(json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get() {
        let mut s = State::new();
        s.set("score", 42i32);
        s.set("name", "player");
        s.set("alive", true);
        assert_eq!(s.get_int("score").unwrap(), 42);
        assert_eq!(s.get_string("name").unwrap(), "player");
        assert_eq!(s.get_bool("alive").unwrap(), true);
    }

    #[test]
    fn type_mismatch_error() {
        let mut s = State::new();
        s.set("score", 42i32);
        assert!(matches!(
            s.get_bool("score"),
            Err(StateError::TypeMismatch(_, "bool"))
        ));
    }

    #[test]
    fn roundtrip_json() {
        let mut s = State::new();
        s.set("score", 100i64);
        s.set("name", "hero");
        let json = s.to_json().unwrap();
        let back = State::from_json(&json).unwrap();
        assert_eq!(back.get_int("score").unwrap(), 100);
        assert_eq!(back.get_string("name").unwrap(), "hero");
    }
}
