use crate::domain::uri::Uri;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumString};
use url::Url;

/// Kind of JSON-LD @context managed by the broker (ETSI GS CIM 009 clause 5.13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Display, EnumString, sqlx::Type)]
#[strum(serialize_all = "PascalCase")]
#[sqlx(type_name = "jsonld_context_kind")]
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
    pub url: Option<Url>,
    pub local_id: Uri,
    pub kind: ContextKind,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_usage: DateTime<Utc>,
    pub number_of_hits: u64,
    pub extra_info: Option<Value>,
}
