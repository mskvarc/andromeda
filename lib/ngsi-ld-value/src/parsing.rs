use crate::types::{Number, Value};
use compact_str::CompactString;

/// Parsing logic for primitive types from strings with high performance and
/// support for "dirty" inputs (e.g., surrounding quotes, commas as decimals).
pub struct Parser;

impl Parser {
    /// Parses a boolean from a string or value.
    pub fn parse_boolean(s: &str) -> bool {
        let s = s.trim().to_lowercase();
        s == "true" || s == "1" || s == "yes" || s == "on"
    }

    /// Parses an i64 from a string with heuristic cleaning.
    pub fn parse_integer(s: &str) -> Option<i64> {
        let s = s.trim();
        let cleaned = Self::strip_quotes(s);

        cleaned
            .parse::<i64>()
            .or_else(|_| cleaned.replace(',', ".").parse::<f64>().map(|f| f as i64))
            .ok()
    }

    /// Parses an f64 from a string with heuristic cleaning.
    pub fn parse_float(s: &str) -> Option<f64> {
        let s = s.trim();
        let cleaned = Self::strip_quotes(s);

        cleaned.parse::<f64>().or_else(|_| cleaned.replace(',', ".").parse::<f64>()).ok()
    }

    /// Helper to strip surrounding quotes if they exist.
    fn strip_quotes(s: &str) -> &str {
        if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
            if s.len() >= 2 { &s[1..s.len() - 1].trim() } else { s }
        } else {
            s
        }
    }
}

impl Value {
    /// High-performance conversion from a string that might be structured JSON (Array/Object).
    pub fn from_string(s: String) -> Self {
        let trimmed = s.trim();
        if (trimmed.starts_with('[') && trimmed.ends_with(']')) || (trimmed.starts_with('{') && trimmed.ends_with('}')) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                return Value::from(json);
            }
        }
        Value::String(CompactString::from(s))
    }

    /// Infallible conversion to string using Display implementation.
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        format!("{}", self)
    }

    /// Infallible conversion to boolean.
    pub fn to_boolean(&self) -> bool {
        match self {
            Value::Boolean(b) => *b,
            Value::String(s) => Parser::parse_boolean(s),
            Value::Number(n) => match n {
                Number::Integer(i) => *i != 0,
                Number::Float(f) => *f != 0.0,
            },
            Value::Null => false,
            _ => false,
        }
    }

    /// Infallible conversion to integer.
    pub fn to_integer(&self) -> i64 {
        match self {
            Value::Number(n) => match n {
                Number::Integer(i) => *i,
                Number::Float(f) => *f as i64,
            },
            Value::String(s) => Parser::parse_integer(s).unwrap_or(0),
            Value::Boolean(b) => {
                if *b {
                    1
                } else {
                    0
                }
            }
            Value::Temporal(t) => match t {
                crate::types::TemporalValue::DateTime(dt) | crate::types::TemporalValue::Date(dt) | crate::types::TemporalValue::Time(dt) => dt.timestamp(),
            },
            _ => 0,
        }
    }

    /// Infallible conversion to float.
    pub fn to_float(&self) -> f64 {
        match self {
            Value::Number(n) => match n {
                Number::Integer(i) => *i as f64,
                Number::Float(f) => *f,
            },
            Value::String(s) => Parser::parse_float(s).unwrap_or(0.0),
            Value::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Temporal(t) => match t {
                crate::types::TemporalValue::DateTime(dt) | crate::types::TemporalValue::Date(dt) | crate::types::TemporalValue::Time(dt) => {
                    dt.timestamp() as f64 + (dt.timestamp_subsec_nanos() as f64 / 1_000_000_000.0)
                }
            },
            _ => 0.0,
        }
    }
}
