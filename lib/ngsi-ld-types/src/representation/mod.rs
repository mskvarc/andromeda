use crate::error::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::str::FromStr;

pub mod concise;
pub mod normalized;
pub mod simplified;

/// The three NGSI-LD representation formats (ETSI GS CIM 009 clause 4.5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NgsiLdRepresentation {
    Normalized,
    #[default]
    Concise,
    Simplified,
}

impl FromStr for NgsiLdRepresentation {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "normalized" => Ok(NgsiLdRepresentation::Normalized),
            "concise" => Ok(NgsiLdRepresentation::Concise),
            "simplified" | "keyvalues" => Ok(NgsiLdRepresentation::Simplified),
            _ => Err(format!("Invalid representation: {}", s)),
        }
    }
}

/// Whether to skip attributes with null values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NgsiLdSkipNull {
    Include,
    #[default]
    Skip,
}

impl FromStr for NgsiLdSkipNull {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "include" => Ok(NgsiLdSkipNull::Include),
            "skip" => Ok(NgsiLdSkipNull::Skip),
            _ => Err(format!("Invalid skip null: {}", s)),
        }
    }
}

/// Convert an attribute to a JSON value in a given representation.
pub trait ToRepresentation {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>>;
}

/// Serialize an entity (or collection) to JSON in a given representation.
pub trait NgsiLdSerializable {
    fn to_json(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<JsonValue>;

    fn to_string_repr(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull, pretty: bool) -> Result<String>;
}
