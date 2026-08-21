//! Context Source identity configuration.

use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use confique::Config;
use serde::Serialize;
use serde_json::Value;
use url::Url;

/// Parse the `contextSourceExtras` environment value as a JSON document.
fn parse_extras(raw: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(raw)
}

/// Identity returned by `/info/sourceIdentity` and used for distributed loop detection.
#[derive(Config, Debug, Clone, Serialize)]
pub struct InfoConfig {
    /// URI identifying this Context Source, per Table 5.2.40-1.
    #[config(env = "ANDROMEDA_INFO_SOURCE_ID", default = "urn:ngsi-ld:ContextSourceIdentity:andromeda")]
    pub source_id: Uri,
    /// Pseudonym appended to forwarded requests' `Via` headers.
    #[config(env = "ANDROMEDA_INFO_SOURCE_ALIAS", default = "andromeda")]
    pub source_alias: ContextSourcePseudonym,
    /// Instance-specific information returned verbatim as
    /// `contextSourceExtras` (Table 5.2.40-1); raw JSON, never JSON-LD.
    #[config(env = "ANDROMEDA_INFO_SOURCE_EXTRAS", parse_env = parse_extras)]
    pub source_extras: Option<Value>,
    /// The origin peers can reach this broker at, such as `https://broker-a:1056`.
    ///
    /// Subscription forwarding (clauses 5.8.1.4 and 5.8.6) names this broker as
    /// the notification endpoint of every subscription it places at a context
    /// source; without it the broker cannot receive those notifications, and
    /// forwarding stays off.
    #[config(env = "ANDROMEDA_INFO_ADVERTISED_URL")]
    pub advertised_url: Option<Url>,
}
