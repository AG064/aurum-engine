//! A minimal TOML reader, limited to what `aurum.toml` needs.
//!
//! The project keeps a hard zero-dependency policy, and a full TOML crate
//! would be the largest single dependency in the workspace. `aurum.toml` uses
//! a small, fixed shape — tables of strings, integers, booleans, and string
//! arrays — so this reads exactly that.
//!
//! **It is deliberately not a TOML implementation.** Anything outside the
//! subset is refused with a message naming the line, rather than being
//! silently misread. A configuration file that half-parses is worse than one
//! that fails, because the failure surfaces much later and somewhere else.
//!
//! Supported:
//!
//! - `key = "string"`, `key = 42`, `key = true`
//! - `key = ["a", "b"]`
//! - `[table]` and `[table.sub]` headers
//! - `#` comments, blank lines, and trailing commas
//!
//! Refused: multi-line strings, inline tables, arrays of anything but strings,
//! dotted keys, datetimes, and arrays spanning lines.

use std::collections::BTreeMap;
use std::fmt;

/// A parsed TOML value from the supported subset.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Boolean(bool),
    Array(Vec<String>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[String]> {
        match self {
            Self::Array(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// The type name, for error messages that say what was expected.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::String(_) => "string",
            Self::Integer(_) => "integer",
            Self::Boolean(_) => "boolean",
            Self::Array(_) => "array of strings",
        }
    }
}

/// A parsing failure, always carrying the line it happened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// A parsed document: dotted table path to key to value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    entries: BTreeMap<String, Value>,
}

impl Document {
    /// Parse a document.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut document = Self::default();
        let mut table = String::new();

        for (line_number, line) in logical_lines(text)? {
            if let Some(rest) = line.strip_prefix('[') {
                let name = rest.strip_suffix(']').ok_or_else(|| ParseError {
                    line: line_number,
                    message: "table header is missing its closing ']'".into(),
                })?;
                let name = name.trim();
                if name.is_empty() {
                    return Err(ParseError {
                        line: line_number,
                        message: "table name must not be empty".into(),
                    });
                }
                // TOML allows quoted keys; this subset does not, and saying so
                // beats accepting a name with quotes baked into it.
                if name.starts_with('"') || name.starts_with('\'') {
                    return Err(ParseError {
                        line: line_number,
                        message: "quoted table names are not supported".into(),
                    });
                }
                if !name
                    .split('.')
                    .all(|part| !part.is_empty() && is_bare_key(part))
                {
                    return Err(ParseError {
                        line: line_number,
                        message: format!("'{name}' is not a valid table name"),
                    });
                }
                table = name.to_string();
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                return Err(ParseError {
                    line: line_number,
                    message: format!("expected 'key = value', got '{line}'"),
                });
            };
            let key = key.trim();
            if key.is_empty() || !is_bare_key(key) {
                return Err(ParseError {
                    line: line_number,
                    message: format!("'{key}' is not a valid key"),
                });
            }

            let value = parse_value(value.trim(), line_number)?;
            let path = if table.is_empty() {
                key.to_string()
            } else {
                format!("{table}.{key}")
            };
            if document.entries.contains_key(&path) {
                return Err(ParseError {
                    line: line_number,
                    message: format!("'{path}' is defined more than once"),
                });
            }
            document.entries.insert(path, value);
        }

        Ok(document)
    }

    /// Look up a value by its dotted path.
    pub fn get(&self, path: &str) -> Option<&Value> {
        self.entries.get(path)
    }

    pub fn string(&self, path: &str) -> Option<&str> {
        self.get(path).and_then(Value::as_str)
    }

    pub fn integer(&self, path: &str) -> Option<i64> {
        self.get(path).and_then(Value::as_integer)
    }

    pub fn boolean(&self, path: &str) -> Option<bool> {
        self.get(path).and_then(Value::as_bool)
    }

    pub fn array(&self, path: &str) -> Option<&[String]> {
        self.get(path).and_then(Value::as_array)
    }

    /// Every entry, for diagnostics.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }
}

fn is_bare_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Join physical lines into logical ones.
///
/// A multi-line array is ordinary TOML — trailing commas and a bracket per
/// line are the idiomatic way to write a list — so the reader joins until the
/// brackets balance rather than refusing the shape.
fn logical_lines(text: &str) -> Result<Vec<(usize, String)>, ParseError> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    let mut depth = 0i32;

    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let cleaned = strip_comment(raw).trim().to_string();

        // A blank line inside nothing is just a blank line.
        if cleaned.is_empty() && pending.is_none() {
            continue;
        }

        let (start, mut combined) = pending.take().unwrap_or((number, String::new()));
        if !combined.is_empty() && !cleaned.is_empty() {
            combined.push(' ');
        }
        combined.push_str(&cleaned);

        depth += bracket_delta(&cleaned);
        if depth > 0 {
            pending = Some((start, combined));
        } else {
            if depth < 0 {
                return Err(ParseError {
                    line: number,
                    message: "unbalanced ']'".into(),
                });
            }
            out.push((start, combined));
            depth = 0;
        }
    }

    if let Some((start, _)) = pending {
        return Err(ParseError {
            line: start,
            message: "array is missing its closing ']'".into(),
        });
    }
    Ok(out)
}

/// Net `[` minus `]` on a line, ignoring brackets inside strings.
fn bracket_delta(line: &str) -> i32 {
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;
    for byte in line.bytes() {
        match byte {
            b'\\' if in_string => escaped = !escaped,
            b'"' if !escaped => in_string = !in_string,
            b'[' if !in_string => depth += 1,
            b']' if !in_string => depth -= 1,
            _ => escaped = false,
        }
    }
    depth
}

/// Remove a `#` comment, ignoring one inside a string.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'\\' if in_string => escaped = !escaped,
            b'"' if !escaped => in_string = !in_string,
            b'#' if !in_string => return &line[..index],
            _ => escaped = false,
        }
    }
    line
}

fn parse_value(text: &str, line: usize) -> Result<Value, ParseError> {
    if text.is_empty() {
        return Err(ParseError {
            line,
            message: "value is missing".into(),
        });
    }

    // Checked before the general string case: `"""` otherwise parses as an
    // empty string followed by junk, and the real problem goes unreported.
    if text.starts_with("\"\"\"") || text.starts_with("'''") {
        return Err(ParseError {
            line,
            message: "multi-line strings are not supported".into(),
        });
    }
    if text.starts_with('"') {
        return parse_string(text, line).map(Value::String);
    }
    if text.starts_with('[') {
        return parse_string_array(text, line).map(Value::Array);
    }
    if text == "true" {
        return Ok(Value::Boolean(true));
    }
    if text == "false" {
        return Ok(Value::Boolean(false));
    }
    if let Ok(number) = text.replace('_', "").parse::<i64>() {
        return Ok(Value::Integer(number));
    }

    // Name the unsupported construct rather than a generic failure, so the
    // message tells the author what to write instead.

    if text.starts_with('{') {
        return Err(ParseError {
            line,
            message: "inline tables are not supported; use a [table] header".into(),
        });
    }
    // A date leads with four digits and carries dashes; a time carries colons.
    // Negative integers are already parsed above, so this cannot swallow one.
    let looks_like_a_date = text.len() >= 8
        && text.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        && text.contains('-');
    if looks_like_a_date || (text.contains(':') && !text.starts_with('"')) {
        return Err(ParseError {
            line,
            message: "datetimes are not supported".into(),
        });
    }
    Err(ParseError {
        line,
        message: format!(
            "'{text}' is not a supported value (string, integer, boolean, or array of strings)"
        ),
    })
}

fn parse_string(text: &str, line: usize) -> Result<String, ParseError> {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut index = 1; // Skip the opening quote.

    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                // Anything after the closing quote is not part of this subset.
                if !text[index + 1..].trim().is_empty() {
                    return Err(ParseError {
                        line,
                        message: format!(
                            "unexpected text after the closing quote: '{}'",
                            &text[index + 1..]
                        ),
                    });
                }
                return Ok(out);
            }
            b'\\' => {
                index += 1;
                let Some(escape) = bytes.get(index) else {
                    break;
                };
                match escape {
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    other => {
                        return Err(ParseError {
                            line,
                            message: format!("unsupported escape '\\{}'", *other as char),
                        })
                    }
                }
            }
            other => out.push(other as char),
        }
        index += 1;
    }

    Err(ParseError {
        line,
        message: "unterminated string".into(),
    })
}

fn parse_string_array(text: &str, line: usize) -> Result<Vec<String>, ParseError> {
    let inner = text
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| ParseError {
            line,
            message: "array is missing its closing ']' (arrays cannot span lines)".into(),
        })?;

    let mut items = Vec::new();
    let mut rest = inner.trim();
    while !rest.is_empty() {
        if !rest.starts_with('"') {
            return Err(ParseError {
                line,
                message: format!(
                    "arrays may only hold strings; '{rest}' does not start with a quote"
                ),
            });
        }
        // Find the closing quote, honouring escapes.
        let bytes = rest.as_bytes();
        let mut index = 1;
        let mut escaped = false;
        let mut end = None;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' if !escaped => escaped = true,
                b'"' if !escaped => {
                    end = Some(index);
                    break;
                }
                _ => escaped = false,
            }
            index += 1;
        }
        let Some(end) = end else {
            return Err(ParseError {
                line,
                message: "unterminated string in array".into(),
            });
        };

        items.push(parse_string(&rest[..end + 1], line)?);
        rest = rest[end + 1..].trim_start();
        if let Some(tail) = rest.strip_prefix(',') {
            rest = tail.trim_start();
        } else if !rest.is_empty() {
            return Err(ParseError {
                line,
                message: format!("expected ',' between array items, got '{rest}'"),
            });
        }
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shape_aurum_toml_uses() {
        let text = r#"
# Aurum project configuration.
schema_version = 1
name = "aurum-engine"
godot_version = "4.7"
rust_package = "aurum-godot"
addon_destination = "godot/addons/aurum"
modules = ["aurum-2d", "aurum-3d", "aurum-space",]
debug_builds = true

[engine]
path_hint = "../aurum-engine"
managed = false

[validation]
contract = "scripts/tests/phase0_contract.ps1"
"#;
        let document = Document::parse(text).unwrap();
        assert_eq!(document.integer("schema_version"), Some(1));
        assert_eq!(document.string("name"), Some("aurum-engine"));
        assert_eq!(document.string("godot_version"), Some("4.7"));
        assert_eq!(document.boolean("debug_builds"), Some(true));
        assert_eq!(
            document.array("modules").unwrap(),
            ["aurum-2d", "aurum-3d", "aurum-space"],
            "a trailing comma should be accepted"
        );
        assert_eq!(document.string("engine.path_hint"), Some("../aurum-engine"));
        assert_eq!(document.boolean("engine.managed"), Some(false));
        assert_eq!(
            document.string("validation.contract"),
            Some("scripts/tests/phase0_contract.ps1")
        );
    }

    #[test]
    fn multi_line_arrays_are_joined() {
        let document =
            Document::parse("modules = [\n    \"aurum-2d\",\n    \"aurum-3d\",\n]\nnext = 1\n")
                .unwrap();
        assert_eq!(document.array("modules").unwrap(), ["aurum-2d", "aurum-3d"]);
        // The line after a joined array is still its own entry.
        assert_eq!(document.integer("next"), Some(1));
    }

    #[test]
    fn an_unterminated_multi_line_array_is_reported_at_its_start() {
        let error = Document::parse("ok = 1\nmodules = [\n  \"a\",\n").unwrap_err();
        assert!(
            error.message.contains("missing its closing"),
            "{}",
            error.message
        );
        assert_eq!(error.line, 2, "the error should point at the array");
    }

    #[test]
    fn brackets_inside_strings_do_not_confuse_the_joiner() {
        let document = Document::parse("note = \"a [ bracket\"\nnext = 2\n").unwrap();
        assert_eq!(document.string("note"), Some("a [ bracket"));
        assert_eq!(document.integer("next"), Some(2));
    }

    #[test]
    fn a_table_header_is_not_treated_as_an_open_array() {
        let document = Document::parse("[engine]\npath_hint = \".\"\n").unwrap();
        assert_eq!(document.string("engine.path_hint"), Some("."));
    }

    #[test]
    fn unbalanced_closing_brackets_are_refused() {
        assert!(Document::parse("a = 1]\n").is_err());
    }

    #[test]
    fn empty_and_comment_only_documents_parse() {
        assert!(Document::parse("").unwrap().entries().next().is_none());
        assert!(Document::parse("# nothing here\n\n   \n")
            .unwrap()
            .entries()
            .next()
            .is_none());
    }

    #[test]
    fn comments_inside_strings_are_kept() {
        let document =
            Document::parse(r##"note = "hash # is not a comment" # but this is"##).unwrap();
        assert_eq!(document.string("note"), Some("hash # is not a comment"));
    }

    #[test]
    fn escapes_are_decoded() {
        let document =
            Document::parse("path = \"C:\\\\Games\\\\Godot\"\nquote = \"a\\\"b\"\n").unwrap();
        assert_eq!(document.string("path"), Some(r"C:\Games\Godot"));
        assert_eq!(document.string("quote"), Some("a\"b"));
    }

    #[test]
    fn unsupported_constructs_are_named_not_ignored() {
        for (text, expected) in [
            (r#"a = """multi""""#, "multi-line strings"),
            ("a = { b = 1 }", "inline tables"),
            ("a = 1\nb = 2\na = 3", "defined more than once"),
            ("a = [1, 2]", "may only hold strings"),
            ("a = 1979-05-27", "datetimes"),
            ("not a key value pair", "expected 'key = value'"),
            ("a = [\"unterminated\\n", "missing its closing"),
            ("a = \"unterminated", "unterminated string"),
            ("[unclosed", "missing its closing"),
            ("[\"\"]", "quoted table names"),
        ] {
            let error =
                Document::parse(text).expect_err(&format!("'{text}' should have been refused"));
            assert!(
                error.message.contains(expected),
                "'{text}' gave '{}', expected something about '{expected}'",
                error.message
            );
            assert!(error.line >= 1, "an error must name its line");
        }
    }

    #[test]
    fn error_line_numbers_point_at_the_problem() {
        let error = Document::parse("ok = 1\nfine = 2\nbroken =\n").unwrap_err();
        assert_eq!(error.line, 3);
    }

    #[test]
    fn file_paths_may_contain_colons_and_slashes() {
        // A Windows drive letter must not be mistaken for a datetime.
        let document = Document::parse(r#"godot = "A:/Tools/Godot/godot.exe""#).unwrap();
        assert_eq!(document.string("godot"), Some("A:/Tools/Godot/godot.exe"));
    }

    #[test]
    fn typed_accessors_return_none_for_the_wrong_type() {
        let document = Document::parse("count = 3\nname = \"x\"").unwrap();
        assert_eq!(document.string("count"), None);
        assert_eq!(document.integer("name"), None);
        assert_eq!(document.boolean("name"), None);
        assert_eq!(document.array("name"), None);
        assert_eq!(document.get("missing"), None);
    }

    #[test]
    fn underscore_digit_separators_are_accepted() {
        let document = Document::parse("big = 1_000_000").unwrap();
        assert_eq!(document.integer("big"), Some(1_000_000));
    }

    #[test]
    fn value_kinds_are_reported_for_diagnostics() {
        assert_eq!(Value::String("x".into()).kind(), "string");
        assert_eq!(Value::Integer(1).kind(), "integer");
        assert_eq!(Value::Boolean(true).kind(), "boolean");
        assert_eq!(Value::Array(vec![]).kind(), "array of strings");
    }
}
