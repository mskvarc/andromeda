//! When a registration was written and when it stops applying (ETSI GS CIM 009 clauses 4.8 and 5.2.9).

use andromeda_ngsi_ld_lexicon::time::instant_serde;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// When a registration was written and when it stops applying
/// (ETSI GS CIM 009 clauses 4.8 and 5.2.9).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationLifecycle {
    /// Registration expiration timestamp.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional", default)]
    #[builder(default = Default::default())]
    pub expires_at: Option<DateTime<Utc>>,
    /// Timestamp when the registration was created.
    #[serde(with = "instant_serde::required")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the registration was last modified.
    #[serde(with = "instant_serde::required")]
    pub modified_at: DateTime<Utc>,
}
