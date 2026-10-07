//! Bounded JSON admission. Duplicate member names are rejected at any depth
//! after unescaping, before a map could silently keep only one of them.
//! Errors carry a code only, never the input.

use std::cell::Cell;
use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

use crate::codes::{Rejection, RejectionCode};

/// Maximum accepted input size for [`parse_unique`].
pub const MAX_JSON_BYTES: usize = 64 * 1024;
/// Maximum container nesting for [`parse_unique`]; the root container is depth 1.
pub const MAX_JSON_DEPTH: usize = 8;

/// Parses one JSON value, rejecting duplicate keys (`duplicate_key`), trailing
/// data, nesting deeper than [`MAX_JSON_DEPTH`] (`invalid_json`) and input
/// larger than [`MAX_JSON_BYTES`] (`envelope_too_large`).
pub fn parse_unique(text: &str) -> Result<Value, Rejection> {
    parse_bounded(text, MAX_JSON_BYTES, MAX_JSON_DEPTH)
}

pub(crate) fn parse_bounded(
    text: &str,
    max_bytes: usize,
    max_depth: usize,
) -> Result<Value, Rejection> {
    if text.len() > max_bytes {
        return Err(Rejection::new(RejectionCode::EnvelopeTooLarge));
    }
    let state = State {
        duplicate: Cell::new(false),
        max_depth,
    };
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let parsed = UniqueSeed {
        state: &state,
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .and_then(|value| deserializer.end().map(|()| value));
    parsed.map_err(|_| {
        if state.duplicate.get() {
            Rejection::new(RejectionCode::DuplicateKey)
        } else {
            Rejection::new(RejectionCode::InvalidJson)
        }
    })
}

struct State {
    duplicate: Cell<bool>,
    max_depth: usize,
}

#[derive(Clone, Copy)]
struct UniqueSeed<'s> {
    state: &'s State,
    depth: usize,
}

impl UniqueSeed<'_> {
    fn enter<E: de::Error>(self) -> Result<Self, E> {
        let depth = self.depth + 1;
        if depth > self.state.max_depth {
            return Err(E::custom("nesting too deep"));
        }
        Ok(Self {
            state: self.state,
            depth,
        })
    }
}

impl<'de> DeserializeSeed<'de> for UniqueSeed<'_> {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for UniqueSeed<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON with unique member names")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        // Floats are kept so that kind validation reports them precisely;
        // no audit kind accepts a non-integer number.
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("invalid number"))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let inner = self.enter()?;
        let mut items = Vec::new();
        while let Some(item) = access.next_element_seed(inner)? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let inner = self.enter()?;
        let mut map = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            if map.contains_key(&key) {
                self.state.duplicate.set(true);
                return Err(de::Error::custom("duplicate member"));
            }
            let value = access.next_value_seed(inner)?;
            map.insert(key, value);
        }
        Ok(Value::Object(map))
    }
}

/// Rebuilds every object with keys inserted in byte order, so that
/// serialization is identical whether or not `serde_json/preserve_order` is
/// enabled by feature unification.
pub(crate) fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(text: &str) -> RejectionCode {
        parse_unique(text).expect_err("must reject").code
    }

    #[test]
    fn accepts_unique_nested_json() {
        let value = parse_unique(r#"{"a":{"b":[1,{"c":true}]},"d":null}"#).expect("valid");
        assert_eq!(value["a"]["b"][1]["c"], Value::Bool(true));
    }

    #[test]
    fn rejects_duplicates_at_any_depth_including_escaped_aliases() {
        assert_eq!(code(r#"{"a":1,"a":2}"#), RejectionCode::DuplicateKey);
        assert_eq!(code(r#"{"x":{"a":1,"a":2}}"#), RejectionCode::DuplicateKey);
        assert_eq!(
            code(r#"[{"k":1},{"k":1,"k":1}]"#),
            RejectionCode::DuplicateKey
        );
    }

    #[test]
    fn rejects_trailing_data_depth_and_size() {
        assert_eq!(code(r#"{"a":1} {"b":2}"#), RejectionCode::InvalidJson);
        assert_eq!(code(r#"{"a":1"#), RejectionCode::InvalidJson);
        let eight = "[[[[[[[[1]]]]]]]]";
        assert!(parse_unique(eight).is_ok());
        assert_eq!(code("[[[[[[[[[1]]]]]]]]]"), RejectionCode::InvalidJson);
        let big = format!("\"{}\"", "a".repeat(MAX_JSON_BYTES));
        assert_eq!(code(&big), RejectionCode::EnvelopeTooLarge);
    }

    #[test]
    fn canonicalize_sorts_keys_recursively() {
        let value = parse_unique(r#"{"b":{"z":1,"a":2},"a":[{"y":1,"x":2}]}"#).expect("valid");
        let text = serde_json::to_string(&canonicalize(value)).expect("serialize");
        assert_eq!(text, r#"{"a":[{"x":2,"y":1}],"b":{"a":2,"z":1}}"#);
    }
}
