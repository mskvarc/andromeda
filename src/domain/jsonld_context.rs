use crate::domain::serde_helpers;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, sqlx::Type)]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "PascalCase")]
pub enum ContextKind {
    Cached,
    Hosted,
    ImplicitlyCreated,
}

impl fmt::Display for ContextKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextKind::Cached => write!(f, "Cached"),
            ContextKind::Hosted => write!(f, "Hosted"),
            ContextKind::ImplicitlyCreated => write!(f, "ImplicitlyCreated"),
        }
    }
}

impl std::str::FromStr for ContextKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Cached" => Ok(ContextKind::Cached),
            "Hosted" => Ok(ContextKind::Hosted),
            "ImplicitlyCreated" => Ok(ContextKind::ImplicitlyCreated),
            _ => Err(format!("Invalid context kind: {}", s)),
        }
    }
}

/// Detailed metadata response (for details=true)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextMetadata {
    #[serde(rename = "URL")]
    pub url: Option<String>,
    pub local_id: String,
    pub kind: ContextKind,
    #[serde(with = "serde_helpers::rfc3339")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "serde_helpers::rfc3339_option")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(with = "serde_helpers::rfc3339")]
    pub last_usage: DateTime<Utc>,
    pub number_of_hits: i32,
    pub extra_info: Option<Value>,
}
