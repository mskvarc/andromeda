use crate::types::{Number, Value};
use compact_str::CompactString;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;

impl Value {
    pub fn into_json_value(self) -> JsonValue {
        JsonValue::from(self)
    }

    pub fn to_object(&self) -> IndexMap<CompactString, Value> {
        match self {
            Value::Object(map) => map.as_ref().clone(),
            _ => IndexMap::new(),
        }
    }

    pub fn try_parse_datetime(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        match self {
            Value::Temporal(t) => match t {
                crate::types::TemporalValue::DateTime(dt) | crate::types::TemporalValue::Date(dt) | crate::types::TemporalValue::Time(dt) => Some(*dt),
            },
            Value::String(s) => crate::types::TemporalValue::try_parse(s).map(|t| match t {
                crate::types::TemporalValue::DateTime(dt) | crate::types::TemporalValue::Date(dt) | crate::types::TemporalValue::Time(dt) => dt,
            }),
            _ => None,
        }
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Number(crate::types::Number::Integer(i))
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Number(crate::types::Number::Float(f))
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Boolean(b)
    }
}

impl From<JsonValue> for Value {
    fn from(json: JsonValue) -> Self {
        match json {
            JsonValue::Null => Value::Null,
            JsonValue::Bool(b) => Value::Boolean(b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Number(Number::Integer(i))
                } else {
                    Value::Number(Number::Float(n.as_f64().unwrap_or(0.0)))
                }
            }
            JsonValue::String(s) => Value::String(CompactString::from(s)),
            JsonValue::Array(arr) => Value::Array(arr.into_iter().map(Value::from).collect()),
            JsonValue::Object(obj) => {
                let mut map = IndexMap::new();
                for (k, v) in obj {
                    map.insert(CompactString::from(k), Value::from(v));
                }
                Value::Object(Box::new(map))
            }
        }
    }
}

impl From<Value> for JsonValue {
    fn from(val: Value) -> Self {
        match val {
            Value::Null => JsonValue::Null,
            Value::Boolean(b) => JsonValue::Bool(b),
            Value::Number(n) => match n {
                Number::Integer(i) => JsonValue::Number(i.into()),
                Number::Float(f) => serde_json::Number::from_f64(f).map(JsonValue::Number).unwrap_or(JsonValue::Null),
            },
            Value::String(s) => JsonValue::String(s.to_string()),
            Value::Temporal(t) => match t {
                crate::types::TemporalValue::DateTime(dt) | crate::types::TemporalValue::Date(dt) | crate::types::TemporalValue::Time(dt) => {
                    JsonValue::String(dt.to_rfc3339())
                }
            },
            Value::Geospatial(g) => serde_json::to_value(g).unwrap_or(JsonValue::Null),
            Value::Array(arr) => JsonValue::Array(arr.into_iter().map(JsonValue::from).collect()),
            Value::Object(obj) => {
                let mut map = serde_json::Map::new();
                for (k, v) in *obj {
                    map.insert(k.to_string(), JsonValue::from(v));
                }
                JsonValue::Object(map)
            }
        }
    }
}
