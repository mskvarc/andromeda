//! Context Source identity configuration.

use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use confique::Config;
use serde::Serialize;

/// Identity returned by `/info` and used for distributed loop detection.
#[derive(Config, Debug, Clone, Serialize)]
pub struct InfoConfig {
    /// URI identifying this Context Source, per Table 5.2.40-1.
    #[config(env = "ANDROMEDA_INFO_SOURCE_ID", default = "urn:ngsi-ld:ContextSourceIdentity:andromeda")]
    pub source_id: Uri,
    /// Pseudonym appended to forwarded requests' `Via` headers.
    #[config(env = "ANDROMEDA_INFO_SOURCE_ALIAS", default = "andromeda")]
    pub source_alias: ContextSourcePseudonym,
}
