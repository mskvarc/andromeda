use crate::domain::serde_helpers;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumString};

/// Kind of JSON-LD @context managed by the broker (ETSI GS CIM 009 clause 5.13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Display, EnumString, sqlx::Type)]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "PascalCase")]
#[strum(serialize_all = "PascalCase")]
pub enum ContextKind {
    Cached,
    Hosted,
    ImplicitlyCreated,
}

/// Detailed @context metadata response (for details=true).
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
    pub number_of_hits: i64,
    pub extra_info: Option<Value>,
}
