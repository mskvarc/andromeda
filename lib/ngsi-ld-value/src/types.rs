use chrono::{DateTime, SecondsFormat, Utc};
use compact_str::CompactString;
use geojson::Geometry;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

/// Represents an NGSI-LD Value, optimized for performance.
/// Uses CompactString for inline small strings to minimize heap allocations.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Value {
    /// A JSON null value
    Null,
    /// A boolean value
    Boolean(bool),
    /// A numeric value (internally either i64 or f64)
    Number(Number),
    /// A string value (optimized for small strings)
    String(CompactString),
    /// A temporal value (DateTime, Date, or Time)
    Temporal(TemporalValue),
    /// A geospatial value (boxed to keep enum size small)
    Geospatial(Box<Geometry>),
    /// An array of values
    Array(Vec<Value>),
    /// A structured object (boxed to keep enum size small)
    Object(Box<IndexMap<CompactString, Value>>),
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Value::Null => serializer.serialize_none(),
            Value::Boolean(b) => serializer.serialize_bool(*b),
            Value::Number(n) => n.serialize(serializer),
            Value::String(s) => s.serialize(serializer),
            Value::Temporal(t) => t.serialize(serializer),
            Value::Geospatial(g) => g.serialize(serializer),
            Value::Array(arr) => arr.serialize(serializer),
            Value::Object(obj) => obj.serialize(serializer),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Number {
    Integer(i64),
    Float(f64),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub enum TemporalValue {
    DateTime(DateTime<Utc>),
    Date(DateTime<Utc>),
    Time(DateTime<Utc>),
}

impl Serialize for TemporalValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = match self {
            TemporalValue::DateTime(dt) => dt.to_rfc3339_opts(SecondsFormat::Millis, true),
            TemporalValue::Date(dt) => dt.format("%Y-%m-%d").to_string(),
            TemporalValue::Time(dt) => dt.format("%H:%M:%S").to_string(),
        };
        serializer.serialize_str(&s)
    }
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    pub fn is_empty_string(&self) -> bool {
        match self {
            Value::String(s) => s.is_empty(),
            _ => false,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, ""),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => match n {
                Number::Integer(i) => write!(f, "{}", i),
                Number::Float(fl) => write!(f, "{}", fl),
            },
            Value::String(s) => write!(f, "{}", s),
            Value::Temporal(t) => match t {
                TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => {
                    write!(f, "{}", dt.to_rfc3339_opts(SecondsFormat::Millis, true))
                }
            },
            Value::Geospatial(g) => write!(f, "{}", g),
            Value::Array(arr) => write!(f, "{:?}", arr),
            Value::Object(obj) => write!(f, "{:?}", obj),
        }
    }
}
