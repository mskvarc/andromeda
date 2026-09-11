//! What the broker has observed about a registered Context Source (ETSI GS CIM 009 Table 5.2.9-2).

use crate::registration::status::CsrStatus;
use andromeda_ngsi_ld_lexicon::rfc3339;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// What the broker has observed about a context source's availability
/// (ETSI GS CIM 009 clause 5.2.9, `status` and the delivery counters).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationHealth {
    /// Outcome of the last distributed operation this registration triggered.
    ///
    /// Table 5.2.9-2 gives `status` cardinality 0..1 and defines it by "the
    /// last attempt to perform a distributed operation": a registration that
    /// has triggered none has no last attempt to report, so the member is
    /// absent rather than asserting a health nothing has been observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub status: Option<CsrStatus>,
    /// Number of distributed operations this registration triggered, failed
    /// attempts included.
    ///
    /// Absent until the first attempt, for the same reason as `status`: Table
    /// 5.2.9-2 gives it cardinality 0..1 and pairs it with a `lastSuccess`
    /// "created on first successful operation".
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub times_sent: Option<i32>,
    /// Number of those attempts that failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub times_failed: Option<i32>,
    /// Timestamp of last successful notification.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    #[builder(default = Default::default())]
    pub last_success: Option<DateTime<Utc>>,
    /// Timestamp of last failed notification.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    #[builder(default = Default::default())]
    pub last_failure: Option<DateTime<Utc>>,
}
