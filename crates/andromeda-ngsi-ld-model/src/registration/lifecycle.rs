//! When a registration was written and when it stops applying (ETSI GS CIM 009 clauses 4.8 and 5.2.9).

use andromeda_ngsi_ld_lexicon::rfc3339;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// When a registration was written and when it stops applying
/// (ETSI GS CIM 009 clauses 4.8 and 5.2.9).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationLifecycle {
    /// Registration expiration timestamp.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    #[builder(default = Default::default())]
    pub expires_at: Option<DateTime<Utc>>,
    /// Timestamp when the registration was created.
    #[serde(with = "rfc3339::datetime")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the registration was last modified.
    #[serde(with = "rfc3339::datetime")]
    pub modified_at: DateTime<Utc>,
}
