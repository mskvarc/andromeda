//! How the broker reaches a registered Context Source (ETSI GS CIM 009 clauses 4.20 and 5.2.9).

use crate::{
    key_value_pair::KeyValuePair,
    registration::{forwarding_mode::ForwardingMode, operation::CsrOperation},
    registration_management::RegistrationManagementInfo,
};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, dereferenceable_uri::DereferenceableUri};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// How the broker reaches a context source and which operations it may send
/// there (ETSI GS CIM 009 clauses 4.20 and 5.2.9).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceAccess {
    /// The endpoint the Context Source exposes its NGSI-LD interface at.
    ///
    /// Table 5.2.9-1 restricts it to "a dereferenceable URI", and clause 5.2.2
    /// has the broker serve a Context Producer's members back as they were
    /// supplied -- so the spelling is kept rather than the canonical form a
    /// bare [`url::Url`] would print.
    pub endpoint: DereferenceableUri,
    /// Preferred accept media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub accept: Option<MediaTypeBuf>,
    /// Forwarding mode for distributed operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub mode: Option<ForwardingMode>,
    /// Operations this registration supports.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub operations: Option<Vec<CsrOperation>>,
    /// Refresh rate, as an ISO 8601 duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub refresh_rate: Option<Iso8601Duration>,
    /// Management metadata (clause 5.2.34).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub management: Option<RegistrationManagementInfo>,
    /// Context source info.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub context_source_info: Option<Vec<KeyValuePair>>,
    /// Unique pseudonym for this context source, used for Via-based loop detection (clause 5.2.9).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub context_source_alias: Option<ContextSourcePseudonym>,
}
